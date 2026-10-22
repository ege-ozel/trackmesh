//! Receive orchestration. A project lock survives any spawned blocking apply.
use crate::{
    apply::apply,
    download::{download, DownloadGate},
    verification::{validate_snapshot, verify_working_folder},
};
use anyhow::{ensure, Context, Result};
use crypto::Identity;
use network::Network;
use protocol::{DeviceCard, PeerRequest, PeerResponse, SignedSnapshot};
use snapshot::{ChunkStore, ProjectLock, Stop, Stopped};
use std::{future::Future, path::PathBuf, sync::Arc, time::Duration};
use storage::{Lineage, Store, TransferState};
use uuid::Uuid;

/// A user-actionable receive refusal. Display text matches the existing refusal messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiveRefusal {
    LocalChanges,
    Diverged,
}

impl std::fmt::Display for ReceiveRefusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LocalChanges => formatter.write_str(
                "local folder contains unsnapshotted changes; snapshot them before receiving",
            ),
            Self::Diverged => formatter.write_str("history is Diverged; receive is blocked"),
        }
    }
}

impl std::error::Error for ReceiveRefusal {}

/// Run `work` until it finishes or `stop` is requested. A stopped future is dropped at its
/// current await point, so wrap only work whose durable writes are each complete on their own.
pub async fn or_stop<T>(stop: &Stop, work: impl Future<Output = Result<T>>) -> Result<T> {
    tokio::select! {
        result = work => result,
        () = async {
            while !stop.requested() {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        } => Err(Stopped.into()),
    }
}

pub struct Receiver {
    pub store: Arc<Store>,
    pub chunks: ChunkStore,
    pub network: Network,
    pub identity: Arc<Identity>,
    pub directory: PathBuf,
    /// When to stop before downloading and ask; `DownloadGate::OPEN` never asks.
    pub gate: DownloadGate,
    /// Honoured before and during download and before apply; never while files are installed.
    pub stop: Stop,
}
impl Receiver {
    pub async fn receive(&self, snapshot: SignedSnapshot, source: DeviceCard) -> Result<Uuid> {
        self.receive_inner_with_override(snapshot, source, false)
            .await
    }

    /// Receive a snapshot while explicitly allowing a divergent remote history to replace the
    /// current local head. Unsnapshotted folder changes and other lineage states remain blocked.
    pub async fn receive_diverged(
        &self,
        snapshot: SignedSnapshot,
        source: DeviceCard,
    ) -> Result<Uuid> {
        self.receive_inner_with_override(snapshot, source, true)
            .await
    }

    async fn receive_inner_with_override(
        &self,
        snapshot: SignedSnapshot,
        source: DeviceCard,
        allow_diverged: bool,
    ) -> Result<Uuid> {
        let project = snapshot.manifest.project_id;
        let cache_lease = Arc::new(self.chunks.lease(false)?);
        let target = snapshot.manifest.snapshot_id;
        let result = self
            .receive_inner(snapshot, source, allow_diverged, cache_lease)
            .await;
        if let Err(error) = &result {
            if let Some(session) = self.store.transfers()?.into_iter().find(|t| {
                t.project_id == project
                    && t.snapshot_id == target
                    && t.state != TransferState::Complete
            }) {
                let state = if self.store.project(project)?.recovery_required {
                    TransferState::RecoveryRequired
                } else {
                    TransferState::Paused
                };
                // A requested stop, a refusal the card explains (unshared changes, both changed)
                // and a Get waiting for the person's answer are pauses, not failures to report.
                let quiet = error.is::<Stopped>()
                    || error.is::<ReceiveRefusal>()
                    || error.is::<crate::DownloadApproval>();
                let message = (!quiet).then(|| error.to_string());
                self.store
                    .transfer_state(session.transfer_id, state, message.as_deref())?;
            }
        }
        result
    }
    async fn receive_inner(
        &self,
        snapshot: SignedSnapshot,
        source: DeviceCard,
        allow_diverged: bool,
        cache_lease: Arc<snapshot::CacheLease>,
    ) -> Result<Uuid> {
        let project = snapshot.manifest.project_id;
        validate_snapshot(&self.store, &self.identity, &snapshot, project, None)?;
        let trusted = self
            .store
            .authorized(project, &source.peer_id)?
            .context("source peer is not paired with this project")?;
        ensure!(
            trusted.device_id == source.device_id && trusted.public_key == source.public_key,
            "source device identity mismatch"
        );
        let lock = Arc::new(ProjectLock::acquire(&self.directory, project)?);
        let current = self.store.project(project)?;
        if current.recovery_required {
            let pending: Vec<_> = self
                .store
                .transfers()?
                .into_iter()
                .filter(|t| t.project_id == project && t.state.requires_recovery())
                .collect();
            ensure!(
                pending.len() == 1 && pending[0].snapshot_id == snapshot.manifest.snapshot_id,
                "recover the existing interrupted apply before receiving a different snapshot"
            );
        }
        let lineage = self.store.lineage(
            project,
            current.latest_local_snapshot,
            snapshot.manifest.snapshot_id,
        )?;
        let lineage_allowed = matches!(lineage, Lineage::RemoteAhead | Lineage::Equal)
            || (allow_diverged && matches!(lineage, Lineage::Diverged));
        if !lineage_allowed {
            if matches!(lineage, Lineage::Diverged) {
                return Err(ReceiveRefusal::Diverged.into());
            }
            anyhow::bail!("history is {lineage:?}; receive is blocked");
        }
        let id = self.store.begin_transfer(&snapshot, source.device_id)?;
        let session = self
            .store
            .transfers()?
            .into_iter()
            .find(|t| t.transfer_id == id)
            .context("transfer record missing")?;
        if session.state == TransferState::Complete {
            ensure!(
                current.latest_local_snapshot == Some(snapshot.manifest.snapshot_id),
                "completed transfer belongs to an older local history"
            );
            let store = self.store.clone();
            let chunks = self.chunks.clone();
            let s = snapshot.clone();
            let guard = lock.clone();
            tokio::task::spawn_blocking(move || {
                let _guard = guard;
                verify_working_folder(&store, &chunks, project, Some(&s))
            })
            .await??;
        } else if current.recovery_required || session.state.requires_recovery() {
            // Local recovery is completed without downloading or regenerating already installed files.
            self.apply_locked(snapshot.clone(), id, lock.clone(), cache_lease.clone())
                .await?;
        } else {
            let store = self.store.clone();
            let chunks = self.chunks.clone();
            let guard = lock.clone();
            tokio::task::spawn_blocking(move || {
                let _guard = guard;
                let parent = store.head(project)?;
                verify_working_folder(&store, &chunks, project, parent.as_ref())
            })
            .await??;
            // A Get that waited (paused, or refused for unshared changes) while this PC shared a
            // newer version, or while Use version saved one, follows that version: nothing is
            // installed yet, and lineage and the folder were just checked against it.
            let head = self.store.project(project)?.latest_local_snapshot;
            if head != session.base_snapshot_id {
                self.store.rebase_transfer(id, head)?;
            }
            if self.stop.requested() {
                return Err(Stopped.into());
            }
            self.store
                .transfer_state(id, TransferState::Receiving, None)?;
            // Each verified chunk is cached and recorded on its own, so dropping the download
            // between chunks keeps all finished progress for Resume.
            if let Err(error) = or_stop(
                &self.stop,
                download(
                    &self.store,
                    &self.chunks,
                    &self.network,
                    &snapshot,
                    id,
                    &source.peer_id,
                    self.gate,
                ),
            )
            .await
            {
                let message = (!error.is::<Stopped>()).then(|| error.to_string());
                self.store
                    .transfer_state(id, TransferState::Paused, message.as_deref())?;
                return Err(error);
            }
            if self.stop.requested() {
                return Err(Stopped.into());
            }
            self.apply_locked(snapshot.clone(), id, lock.clone(), cache_lease.clone())
                .await?;
        }
        let confirmed = matches!(
            self.network
                .request(
                    &source.peer_id,
                    PeerRequest::Verified {
                        project_id: project,
                        snapshot_id: snapshot.manifest.snapshot_id,
                        manifest_hash: snapshot.manifest_hash.clone(),
                    }
                )
                .await,
            Ok(PeerResponse::Verified { equal: true })
        );
        if confirmed {
            self.store
                .mark_verified(project, snapshot.manifest.snapshot_id, &source.peer_id)?;
        }
        tracing::info!(transfer_id=%id, snapshot_id=%snapshot.manifest.snapshot_id, confirmed, "snapshot installed and verified locally");
        Ok(id)
    }
    async fn apply_locked(
        &self,
        snapshot: SignedSnapshot,
        id: Uuid,
        lock: Arc<ProjectLock>,
        cache_lease: Arc<snapshot::CacheLease>,
    ) -> Result<()> {
        let store = self.store.clone();
        let chunks = self.chunks.clone();
        let result = tokio::task::spawn_blocking(move || {
            let _guard = lock;
            let _cache_lease = cache_lease;
            apply(&store, &chunks, &snapshot, id)
        })
        .await?;
        if let Err(error) = result {
            let session = self
                .store
                .transfers()?
                .into_iter()
                .find(|t| t.transfer_id == id)
                .context("transfer missing")?;
            let recovery = self.store.project(session.project_id)?.recovery_required;
            self.store.transfer_state(
                id,
                if recovery {
                    TransferState::RecoveryRequired
                } else {
                    TransferState::Paused
                },
                Some(&error.to_string()),
            )?;
            return Err(error);
        }
        Ok(())
    }
}
