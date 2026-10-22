//! Project-authorized inbound requests; chunk availability is checked separately.
use anyhow::{ensure, Context, Result};
use protocol::{PeerRequest, PeerResponse};
use snapshot::ChunkStore;
use storage::Store;

pub fn serve(
    store: &Store,
    chunks: &ChunkStore,
    peer: &str,
    request: &PeerRequest,
) -> Result<PeerResponse> {
    let project = protocol::request_project(request);
    ensure!(
        store.authorized(project, peer)?.is_some(),
        "project access denied"
    );
    match request {
        PeerRequest::Presence { .. } => {
            anyhow::bail!("presence notices are handled by the core runtime")
        }
        PeerRequest::SnapshotAvailable { .. } => {
            anyhow::bail!("snapshot availability notices are handled by the core runtime")
        }
        PeerRequest::EndpointUpdate { .. } => {
            anyhow::bail!("endpoint updates are handled by the core runtime")
        }
        PeerRequest::Head { .. } => Ok(PeerResponse::Head {
            snapshot: store.head(project)?,
        }),
        PeerRequest::Manifest { snapshot_id, .. } => {
            let snapshot = store
                .snapshot(*snapshot_id)?
                .context("snapshot manifest is unavailable on this device")?;
            ensure!(
                snapshot.manifest.project_id == project,
                "snapshot belongs to another project"
            );
            // Ancestors can be forwarded without retaining their files. A chunk request has a separate local-availability check.
            Ok(PeerResponse::Manifest { snapshot })
        }
        PeerRequest::Chunk {
            snapshot_id, hash, ..
        } => {
            ensure!(protocol::valid_hash(hash), "invalid chunk digest");
            let size = store
                .authorized_chunk_size(project, *snapshot_id, hash)?
                .context("chunk is not in a locally available, authorized snapshot")?;
            Ok(PeerResponse::Chunk {
                hash: hash.clone(),
                data: chunks.read(hash, size)?,
            })
        }
        PeerRequest::Verified {
            snapshot_id,
            manifest_hash,
            ..
        } => {
            let equal = store.head(project)?.is_some_and(|s| {
                s.manifest.snapshot_id == *snapshot_id && s.manifest_hash == *manifest_hash
            });
            if equal {
                store.mark_verified(project, *snapshot_id, peer)?;
            }
            Ok(PeerResponse::Verified { equal })
        }
    }
}
