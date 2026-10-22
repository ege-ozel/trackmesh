//! Explicit cleanup of completed, journal-bound recovery folders only.
use crate::journal::{stage_path, ApplyJournal, ApplyPhase};
use anyhow::{ensure, Context, Result};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};
use storage::{Project, Store, TransferInfo, TransferState};

pub struct BackupInventory {
    pub path: PathBuf,
    pub bytes: u64,
    files: Vec<PathBuf>,
    directories: Vec<PathBuf>,
}

pub fn inspect_backup(
    store: &Store,
    project: &Project,
    session: &TransferInfo,
    verify: bool,
) -> Result<BackupInventory> {
    ensure!(
        !project.recovery_required
            && session.state == TransferState::Complete
            && project.project_id == session.project_id,
        "unfinished recovery data cannot be deleted"
    );
    let root = snapshot::canonical_root(Path::new(&project.root))?;
    let stage = stage_path(&root, project.project_id, session.transfer_id)?;
    snapshot::canonical_root(&stage)?;
    let journal_path = snapshot::safe_path(&stage, "journal.json")?;
    ensure!(
        fs::metadata(&journal_path)?.len() <= protocol::MAX_MANIFEST_BYTES as u64,
        "backup journal is too large"
    );
    let journal: ApplyJournal = serde_json::from_slice(&fs::read(&journal_path)?)?;
    let target = store.transfer_manifest(session.transfer_id)?;
    crypto::verify_snapshot(&target)?;
    ensure!(
        target.manifest.project_id == project.project_id
            && target.manifest.snapshot_id == session.snapshot_id,
        "backup target binding mismatch"
    );
    let base = session
        .base_snapshot_id
        .map(|id| store.snapshot(id))
        .transpose()?
        .flatten();
    ensure!(
        session.base_snapshot_id.is_none() || base.is_some(),
        "backup base manifest missing"
    );
    if let Some(base) = &base {
        crypto::verify_snapshot(base)?;
        ensure!(
            base.manifest.project_id == project.project_id,
            "backup base binding mismatch"
        );
    }
    let old_files = base
        .as_ref()
        .map(|s| {
            s.manifest
                .files
                .iter()
                .map(|f| f.relative_path.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let new_files = target
        .manifest
        .files
        .iter()
        .map(|f| f.relative_path.clone())
        .collect::<Vec<_>>();
    ensure!(
        journal.phase == ApplyPhase::Done
            && journal.transfer_id == session.transfer_id
            && journal.project_id == project.project_id
            && journal.snapshot_id == session.snapshot_id
            && journal.old_files == old_files
            && journal.new_files == new_files,
        "backup journal is incomplete or changed"
    );
    let mut expected = HashMap::new();
    if let Some(base) = &base {
        for file in &base.manifest.files {
            if !snapshot::is_ignored_path(&file.relative_path) {
                expected.insert(format!("backup/{}", file.relative_path), file);
            }
        }
    }
    let mut result = BackupInventory {
        path: stage.clone(),
        bytes: 0,
        files: Vec::new(),
        directories: vec![stage.clone()],
    };
    let mut pending = vec![stage.clone()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)?;
            ensure!(
                !snapshot::is_reparse(&metadata),
                "backup contains a reparse point; deletion stopped"
            );
            let relative = path
                .strip_prefix(&stage)?
                .to_str()
                .context("non-Unicode backup path")?
                .replace('\\', "/");
            if metadata.is_dir() {
                ensure!(
                    relative == "backup"
                        || relative == "prepared"
                        || relative.starts_with("backup/")
                        || relative.starts_with("prepared/"),
                    "unrecognized backup directory; deletion stopped"
                );
                pending.push(path.clone());
                result.directories.push(path);
            } else {
                ensure!(metadata.is_file(), "unsupported backup entry");
                if relative != "journal.json" {
                    let file = expected
                        .remove(&relative)
                        .context("unrecognized backup file; deletion stopped")?;
                    ensure!(
                        metadata.len() == file.size,
                        "backup was modified; deletion stopped"
                    );
                    if verify {
                        snapshot::verify_file(&path, file)?;
                    }
                }
                result.bytes += metadata.len();
                result.files.push(path);
            }
        }
    }
    ensure!(
        expected.is_empty(),
        "backup files are missing; deletion stopped"
    );
    Ok(result)
}

pub fn delete_backup(inventory: BackupInventory) -> Result<u64> {
    // Remove individually after validation; never follow an arbitrary recursive tree.
    for file in &inventory.files {
        let relative = file
            .strip_prefix(&inventory.path)?
            .to_str()
            .context("non-Unicode backup path")?
            .replace('\\', "/");
        fs::remove_file(snapshot::safe_path(&inventory.path, &relative)?)?;
    }
    let mut directories = inventory.directories;
    directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for path in directories {
        snapshot::canonical_root(&path)?;
        fs::remove_dir(path)?;
    }
    Ok(inventory.bytes)
}
