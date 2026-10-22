//! Automatic application-cache retention; recovery folders require explicit user cleanup.
use crate::Engine;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize)]
pub struct BackupSummary {
    pub project_id: Uuid,
    pub project_name: String,
    pub transfer_id: Uuid,
    pub path: String,
    pub bytes: u64,
    /// Unix seconds when the Get that left this backup finished (journal write time).
    pub modified_at: Option<u64>,
    pub protected: bool,
}
#[derive(Serialize)]
pub struct StorageSummary {
    pub cache_bytes: Option<u64>,
    pub backups: Vec<BackupSummary>,
    pub problems: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupSelection {
    pub transfer_id: Uuid,
    pub path: String,
    pub bytes: u64,
}

impl Engine {
    pub fn storage_summary(&self) -> Result<StorageSummary> {
        let sessions = self.store.transfers()?;
        let mut result = StorageSummary {
            cache_bytes: None,
            backups: Vec::new(),
            problems: Vec::new(),
        };
        let _cache_lease = self.chunks.lease(false)?;
        match self.chunks.stored_bytes() {
            Ok(bytes) => result.cache_bytes = Some(bytes),
            Err(_) => result.problems.push("Unrecognized cache data: automatic cleanup stopped. Inspect it and ask before deleting anything.".to_owned()),
        }
        for project in self.store.projects()? {
            let mut backups = Vec::new();
            let mut unreadable = 0;
            for session in sessions.iter().filter(|s| {
                s.project_id == project.project_id && s.state == storage::TransferState::Complete
            }) {
                let stage = transfer::stage_path(
                    std::path::Path::new(&project.root),
                    project.project_id,
                    session.transfer_id,
                )?;
                if !stage.try_exists()? {
                    continue;
                }
                match transfer::inspect_backup(&self.store, &project, session, false) {
                    Ok(inventory) => backups.push(BackupSummary {
                        project_id: project.project_id,
                        project_name: project.name.clone(),
                        transfer_id: session.transfer_id,
                        path: inventory.path.to_string_lossy().into_owned(),
                        bytes: inventory.bytes,
                        modified_at: std::fs::metadata(inventory.path.join("journal.json"))
                            .and_then(|m| m.modified())
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs()),
                        protected: backups.is_empty(),
                    }),
                    // An unreadable folder is never offered for deletion; the others stay usable.
                    Err(_) => unreadable += 1,
                }
            }
            if unreadable > 0 {
                result.problems.push(format!("{}: {unreadable} backup folder(s) are unfinished, unavailable or changed and are not listed. Inspect them before deleting anything by hand.", project.name));
            }
            result.backups.extend(backups);
        }
        Ok(result)
    }

    pub fn delete_old_backups(&self, selected: &[BackupSelection]) -> Result<u64> {
        anyhow::ensure!(
            !selected.is_empty() && selected.len() <= 1024,
            "choose up to 1024 old backups"
        );
        let sessions = self.store.transfers()?;
        let mut projects = std::collections::BTreeSet::new();
        for item in selected {
            let session = sessions
                .iter()
                .find(|s| s.transfer_id == item.transfer_id)
                .ok_or_else(|| anyhow::anyhow!("backup transfer is missing"))?;
            projects.insert(session.project_id);
        }
        let _locks = projects
            .into_iter()
            .map(|id| snapshot::ProjectLock::acquire(&self.directory, id))
            .collect::<Result<Vec<_>>>()?;
        let summary = self.storage_summary()?;
        let mut inventories = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for item in selected {
            anyhow::ensure!(seen.insert(item.transfer_id), "duplicate backup selection");
            let backup = summary
                .backups
                .iter()
                .find(|b| b.transfer_id == item.transfer_id)
                .ok_or_else(|| anyhow::anyhow!("backup is no longer available; refresh Storage"))?;
            anyhow::ensure!(
                !backup.protected && backup.path == item.path && backup.bytes == item.bytes,
                "backup changed or is protected; refresh Storage before deletion"
            );
            let session = sessions
                .iter()
                .find(|s| s.transfer_id == item.transfer_id)
                .unwrap();
            let project = self.store.project(session.project_id)?;
            inventories.push(transfer::inspect_backup(
                &self.store,
                &project,
                session,
                true,
            )?);
        }
        let mut bytes = 0;
        for inventory in inventories {
            bytes += transfer::delete_backup(inventory)?;
        }
        Ok(bytes)
    }
    pub fn maintain_storage(&self) -> Result<u64> {
        prune_cache(&self.chunks, &self.store)
    }
    pub(crate) fn maintain_storage_if_idle(&self) {
        // A busy cache is retried on the next completed operation/session pass.
        if self.maintain_storage().is_err() {
            tracing::debug!("Cache retention deferred; busy or unrecognized storage entry");
        }
    }
    pub(crate) async fn maintain_storage_after_transfer(&self) {
        let chunks = self.chunks.clone();
        let store = self.store.clone();
        let _ = tokio::task::spawn_blocking(move || prune_cache(&chunks, &store)).await;
    }
}

/// Peers fetching from this device keep their chunks readable; pruning waits for a quiet pass.
const SERVING_QUIET: std::time::Duration = std::time::Duration::from_secs(120);

fn prune_cache(chunks: &snapshot::ChunkStore, store: &storage::Store) -> Result<u64> {
    anyhow::ensure!(
        !chunks.recently_read(SERVING_QUIET),
        "a collaborator is fetching versions; retention deferred"
    );
    let _lease = chunks.lease(true)?;
    chunks.stored_bytes()?;
    let retained = store.retain_current_chunks()?;
    chunks.prune(&retained)
}
