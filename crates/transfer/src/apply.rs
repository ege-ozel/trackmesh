//! BACKUP -> INSTALL -> COMMIT -> DONE, with the recovery marker preceding disk mutation.
use crate::{
    assemble::assemble,
    installation::{backup_originals, commit_snapshot, install_files},
    journal::{stage_path, write_journal, ApplyJournal, ApplyPhase},
    verification::verify_working_folder,
};
use anyhow::{ensure, Context, Result};
use protocol::SignedSnapshot;
use snapshot::ChunkStore;
use std::{fs, path::Path};
use storage::Store;
use uuid::Uuid;

// Caller must retain a ProjectLock for this whole operation, including a blocking task after cancellation.
pub(crate) fn apply(
    store: &Store,
    chunks: &ChunkStore,
    s: &SignedSnapshot,
    id: Uuid,
) -> Result<()> {
    crypto::verify_snapshot(s)?;
    let project = store.project(s.manifest.project_id)?;
    let root = Path::new(&project.root);
    let session = store
        .transfers()?
        .into_iter()
        .find(|t| t.transfer_id == id)
        .context("transfer missing")?;
    ensure!(
        session.project_id == project.project_id && session.snapshot_id == s.manifest.snapshot_id,
        "transfer target mismatch"
    );
    ensure!(
        project.latest_local_snapshot == session.base_snapshot_id
            || project.latest_local_snapshot == Some(s.manifest.snapshot_id),
        "local head advanced during receive"
    );
    let base = session
        .base_snapshot_id
        .map(|id| store.snapshot(id))
        .transpose()?
        .flatten();
    ensure!(
        session.base_snapshot_id.is_none() || base.is_some(),
        "base snapshot is missing"
    );
    let old_files: Vec<_> = base
        .as_ref()
        .map(|p| {
            p.manifest
                .files
                .iter()
                .map(|f| f.relative_path.clone())
                .collect()
        })
        .unwrap_or_default();
    let new_files: Vec<_> = s
        .manifest
        .files
        .iter()
        .map(|f| f.relative_path.clone())
        .collect();
    let stage = stage_path(root, project.project_id, id)?;
    ensure!(
        !stage.exists() || !snapshot::is_reparse(&fs::symlink_metadata(&stage)?),
        "staging directory is a reparse point"
    );
    fs::create_dir_all(&stage)?;
    let prepared = snapshot::safe_path(&stage, "prepared")?;
    let backup = snapshot::safe_path(&stage, "backup")?;
    fs::create_dir_all(&backup)?;
    let journal_path = snapshot::safe_path(&stage, "journal.json")?;
    let mut journal = if journal_path.exists() {
        ensure!(
            fs::metadata(&journal_path)?.len() <= protocol::MAX_MANIFEST_BYTES as u64,
            "apply journal is too large"
        );
        let journal: ApplyJournal = serde_json::from_slice(&fs::read(&journal_path)?)?;
        ensure!(
            journal.transfer_id == id
                && journal.project_id == project.project_id
                && journal.snapshot_id == s.manifest.snapshot_id
                && journal.old_files == old_files
                && journal.new_files == new_files,
            "apply journal disagrees with signed manifest or local history"
        );
        journal
    } else {
        ensure!(
            !project.recovery_required,
            "recovery journal is missing; original backups must be inspected"
        );
        verify_working_folder(store, chunks, project.project_id, base.as_ref())?;
        assemble(&prepared, chunks, s)?;
        let journal = ApplyJournal {
            transfer_id: id,
            project_id: project.project_id,
            snapshot_id: s.manifest.snapshot_id,
            old_files,
            new_files,
            phase: ApplyPhase::Backup,
        };
        write_journal(&stage, &journal)?;
        journal
    };
    if journal.phase == ApplyPhase::Done {
        ensure!(
            project.latest_local_snapshot == Some(s.manifest.snapshot_id),
            "completed journal has no committed snapshot"
        );
        store.finish_apply(id)?;
        return Ok(());
    }
    // The SQLite recovery marker precedes every modification of the working folder.
    store.begin_apply(id)?;
    if journal.phase == ApplyPhase::Backup {
        assemble(&prepared, chunks, s)?;
        backup_originals(
            root,
            &backup,
            base.as_ref(),
            &journal.old_files,
            &journal.new_files,
        )?;
        journal.phase = ApplyPhase::Install;
        write_journal(&stage, &journal)?;
    }
    if journal.phase == ApplyPhase::Install {
        install_files(root, &prepared, s)?;
        journal.phase = ApplyPhase::Commit;
        write_journal(&stage, &journal)?;
    }
    if journal.phase == ApplyPhase::Commit {
        commit_snapshot(store, chunks, root, &project, s, session.base_snapshot_id)?;
        journal.phase = ApplyPhase::Done;
        write_journal(&stage, &journal)?;
    }
    store.finish_apply(id)?;
    Ok(())
}
