//! Signature/author validation and comparison of working files to committed history.
use crate::receiver::ReceiveRefusal;
use anyhow::{ensure, Result};
use crypto::Identity;
use protocol::SignedSnapshot;
use snapshot::{ChunkStore, ScanOptions};
use std::path::Path;
use storage::Store;
use uuid::Uuid;

pub fn validate_snapshot(
    store: &Store,
    own: &Identity,
    snapshot: &SignedSnapshot,
    project: Uuid,
    requested: Option<Uuid>,
) -> Result<()> {
    crypto::verify_snapshot(snapshot)?;
    let m = &snapshot.manifest;
    ensure!(
        m.project_id == project && requested.is_none_or(|id| id == m.snapshot_id),
        "peer returned a different project or snapshot"
    );
    let own_author =
        m.author_device_id == own.device_id && snapshot.author_public_key == own.public_key();
    ensure!(
        own_author
            || store.trusted_author(project, m.author_device_id, &snapshot.author_public_key)?,
        "snapshot author identity is not trusted for this project"
    );
    Ok(())
}
pub(crate) fn verify_working_folder(
    store: &Store,
    chunks: &ChunkStore,
    project: Uuid,
    parent: Option<&SignedSnapshot>,
) -> Result<()> {
    let project = store.project(project)?;
    ensure!(
        !project.recovery_required,
        "an interrupted apply must be recovered first"
    );
    let chunk_size = parent
        .map(|p| p.manifest.chunk_size)
        .unwrap_or(protocol::DEFAULT_CHUNK_SIZE);
    let current = snapshot::scan(
        Path::new(&project.root),
        chunks,
        project.project_id,
        parent
            .map(|p| p.manifest.author_device_id)
            .unwrap_or(Uuid::nil()),
        parent.map(|p| &p.manifest),
        &ScanOptions {
            chunk_size,
            ..ScanOptions::default()
        },
    )?;
    let expected = parent
        .into_iter()
        .flat_map(|snapshot| snapshot.manifest.files.iter())
        .filter(|file| !snapshot::is_ignored_path(&file.relative_path));
    if !expected.eq(current.files.iter()) {
        return Err(ReceiveRefusal::LocalChanges.into());
    }
    Ok(())
}
