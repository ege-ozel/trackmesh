//! Live-folder operations called only in durable apply-journal order.
//! Caller holds ProjectLock; backups are verified and retained, never recursively deleted.
use anyhow::{ensure, Context, Result};
use protocol::SignedSnapshot;
use snapshot::{ChunkStore, ScanOptions};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};
use storage::{Project, Store};
use uuid::Uuid;

pub(crate) fn backup_originals(
    root: &Path,
    backup: &Path,
    base: Option<&SignedSnapshot>,
    old_files: &[String],
    new_files: &[String],
) -> Result<()> {
    if let Some(base) = base {
        for expected in &base.manifest.files {
            if snapshot::is_ignored_path(&expected.relative_path) {
                continue;
            }
            let saved = snapshot::safe_path(backup, &expected.relative_path)?;
            let original = snapshot::safe_path(root, &expected.relative_path)?;
            if saved.exists() {
                snapshot::verify_file(&saved, expected)?;
                ensure!(!original.exists(), "original changed after being backed up");
                continue;
            }
            fs::create_dir_all(saved.parent().context("backup parent missing")?)?;
            let _hold = snapshot::open_verified_file(&original, expected)
                .context("original changed or is locked; apply paused with backups retained")?;
            snapshot::atomic_replace(&original, &saved)?;
        }
    }
    // Folders the new version also uses stay. Removing one that File Explorer or a DAW browser has
    // open only marks it for deletion, and creating it again then fails with "Access is denied".
    let ancestors = |files: &[String]| -> HashSet<PathBuf> {
        let mut directories = HashSet::new();
        for relative in files {
            if snapshot::is_ignored_path(relative) {
                continue;
            }
            let path = root.join(relative);
            for parent in path.ancestors().skip(1).take_while(|p| *p != root) {
                directories.insert(parent.to_owned());
            }
        }
        directories
    };
    let kept = ancestors(new_files);
    let mut directories = ancestors(old_files);
    directories.retain(|directory| !kept.contains(directory));
    let mut directories: Vec<_> = directories.into_iter().collect();
    directories.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    for directory in directories {
        if directory.exists() {
            ensure!(
                !snapshot::is_reparse(&fs::symlink_metadata(&directory)?),
                "directory became a reparse point"
            );
            // Remove only empty directories; no recursive delete is ever used to apply a snapshot.
            match fs::remove_dir(&directory) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::DirectoryNotEmpty => {}
                Err(e) => return Err(e.into()),
            }
        }
    }

    Ok(())
}

pub(crate) fn install_files(root: &Path, prepared: &Path, s: &SignedSnapshot) -> Result<()> {
    for expected in &s.manifest.files {
        if snapshot::is_ignored_path(&expected.relative_path) {
            continue;
        }
        let target = snapshot::safe_path(root, &expected.relative_path)?;
        let staged = snapshot::safe_path(prepared, &expected.relative_path)?;
        if !staged.exists() {
            snapshot::verify_file(&target, expected)?;
            continue;
        }
        snapshot::verify_file(&staged, expected)?;
        ensure!(
            !target.exists(),
            "unexpected file appeared during apply; backups retained"
        );
        fs::create_dir_all(target.parent().context("target parent missing")?)?;
        snapshot::atomic_replace(&staged, &target)?;
    }

    Ok(())
}

pub(crate) fn commit_snapshot(
    store: &Store,
    chunks: &ChunkStore,
    root: &Path,
    project: &Project,
    s: &SignedSnapshot,
    base_snapshot_id: Option<Uuid>,
) -> Result<()> {
    let actual = snapshot::scan(
        root,
        chunks,
        project.project_id,
        s.manifest.author_device_id,
        Some(&s.manifest),
        &ScanOptions {
            chunk_size: s.manifest.chunk_size,
            ..ScanOptions::default()
        },
    )?;
    let expected = s
        .manifest
        .files
        .iter()
        .filter(|file| !snapshot::is_ignored_path(&file.relative_path));
    ensure!(
        expected.eq(actual.files.iter()),
        "installed project differs from received snapshot; backups retained"
    );
    if project.latest_local_snapshot != Some(s.manifest.snapshot_id) {
        let name = store
            .history(project.project_id)?
            .into_iter()
            .find(|i| i.snapshot_id == s.manifest.snapshot_id)
            .map(|i| i.author_name)
            .unwrap_or_default();
        store.commit_local(s, &name, base_snapshot_id, false)?;
    }

    Ok(())
}
