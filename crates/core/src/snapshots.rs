//! Snapshot creation shares the existing lock and transactional commit boundary.
use crate::Engine;
use anyhow::{ensure, Context, Result};
use protocol::SignedSnapshot;
use snapshot::{ScanOptions, Stop};
use std::path::Path;
use uuid::Uuid;

impl Engine {
    pub fn snapshot(&self, project: Uuid) -> Result<SignedSnapshot> {
        self.snapshot_impl(project, false, Stop::default())
    }

    /// A stopped scan returns `snapshot::Stopped` before anything is committed.
    pub fn snapshot_until(&self, project: Uuid, stop: Stop) -> Result<SignedSnapshot> {
        self.snapshot_impl(project, false, stop)
    }

    pub fn snapshot_after_save(&self, project: Uuid) -> Result<SignedSnapshot> {
        self.snapshot_impl(project, true, Stop::default())
    }

    fn snapshot_impl(&self, project: Uuid, settled: bool, stop: Stop) -> Result<SignedSnapshot> {
        let lease = self.chunks.lease(false)?;
        let _lock = snapshot::ProjectLock::acquire(&self.directory, project)?;
        let project = self.store.project(project)?;
        ensure!(
            !project.recovery_required,
            "receive recovery must finish before snapshotting"
        );
        let parent = self.store.head(project.project_id)?;
        let scanner = if settled {
            snapshot::scan_after_save
        } else {
            snapshot::scan
        };
        let manifest = scanner(
            Path::new(&project.root),
            &self.chunks,
            project.project_id,
            self.identity.device_id,
            parent.as_ref().map(|s| &s.manifest),
            &ScanOptions {
                stop,
                ..ScanOptions::default()
            },
        )?;
        ensure!(
            !manifest.files.is_empty(),
            "Nothing to share yet: the project folder is empty."
        );
        if let Some(parent) = &parent {
            let previous = parent
                .manifest
                .files
                .iter()
                .filter(|file| !snapshot::is_ignored_path(&file.relative_path));
            if previous.eq(manifest.files.iter()) {
                return Ok(parent.clone());
            }
        }
        let signed = self.identity.sign_snapshot(manifest)?;
        // Stored with the legacy PENDING metadata status; nothing publishes it any more.
        self.store.commit_local(
            &signed,
            &self.identity.name(),
            parent.map(|s| s.manifest.snapshot_id),
            true,
        )?;
        drop(lease);
        self.maintain_storage_if_idle();
        Ok(signed)
    }
}

/// Which files a version added, removed or changed against the version it was made from.
#[derive(serde::Serialize, Default)]
pub struct SnapshotChanges {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<String>,
    /// More than `CHANGE_LIST_LIMIT` paths in total; the lists are cut.
    pub more: bool,
}
const CHANGE_LIST_LIMIT: usize = 200;

impl Engine {
    /// Compares a stored version's file list with its parent's. A first version lists every file as
    /// added; a version whose file list is not on this PC returns an error.
    pub fn snapshot_changes(&self, project: Uuid, snapshot: Uuid) -> Result<SnapshotChanges> {
        let current = self
            .store
            .snapshot(snapshot)?
            .context("this version's file list is not on this PC")?;
        anyhow::ensure!(
            current.manifest.project_id == project,
            "version belongs to another project"
        );
        let parent: std::collections::HashMap<String, String> =
            match current.manifest.parent_snapshot_id {
                Some(id) => self
                    .store
                    .snapshot(id)?
                    .map(|parent| {
                        parent
                            .manifest
                            .files
                            .into_iter()
                            .map(|file| (file.relative_path, file.blake3))
                            .collect()
                    })
                    .unwrap_or_default(),
                None => Default::default(),
            };
        let mut changes = SnapshotChanges::default();
        let mut seen = std::collections::HashSet::new();
        for file in &current.manifest.files {
            seen.insert(file.relative_path.as_str());
            match parent.get(&file.relative_path) {
                None => changes.added.push(file.relative_path.clone()),
                Some(hash) if *hash != file.blake3 => {
                    changes.changed.push(file.relative_path.clone())
                }
                Some(_) => {}
            }
        }
        changes.removed = parent
            .keys()
            .filter(|path| !seen.contains(path.as_str()))
            .cloned()
            .collect();
        changes.removed.sort();
        let total = changes.added.len() + changes.removed.len() + changes.changed.len();
        changes.more = total > CHANGE_LIST_LIMIT;
        for list in [
            &mut changes.changed,
            &mut changes.added,
            &mut changes.removed,
        ] {
            list.truncate(CHANGE_LIST_LIMIT);
        }
        Ok(changes)
    }
}
