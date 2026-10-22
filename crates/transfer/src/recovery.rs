//! Explicit local recovery with the DAW closed; no network connection is required.
use crate::{apply::apply, validate_snapshot};
use anyhow::{ensure, Result};
use crypto::Identity;
use snapshot::{ChunkStore, ProjectLock};
use std::path::Path;
use storage::{Store, TransferState};
use uuid::Uuid;

pub fn recover_project(
    store: &Store,
    chunks: &ChunkStore,
    own: &Identity,
    directory: &Path,
    project: Uuid,
) -> Result<()> {
    let _cache_lease = chunks.lease(false)?;
    let _lock = ProjectLock::acquire(directory, project)?;
    let sessions: Vec<_> = store
        .transfers()?
        .into_iter()
        .filter(|t| t.project_id == project && t.state.requires_recovery())
        .collect();
    ensure!(
        !store.project(project)?.recovery_required || sessions.len() == 1,
        "recovery needs exactly one pending apply journal"
    );
    for session in sessions {
        let snapshot = store.transfer_manifest(session.transfer_id)?;
        validate_snapshot(store, own, &snapshot, project, Some(session.snapshot_id))?;
        if let Err(error) = apply(store, chunks, &snapshot, session.transfer_id) {
            store.transfer_state(
                session.transfer_id,
                TransferState::RecoveryRequired,
                Some(&error.to_string()),
            )?;
            store.set_project_error(
                project,
                Some(&format!("Receive recovery needs attention: {error}")),
            )?;
            return Err(error);
        }
    }
    Ok(())
}
