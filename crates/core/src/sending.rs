//! Explicit manual send: commit locally, then tell online collaborators.
use crate::Engine;
use anyhow::Result;
use network::Network;
use protocol::PeerRequest;
use serde::Serialize;
use snapshot::Stop;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Serialize)]
pub struct SendStatus {
    pub snapshot_id: Uuid,
}

impl Engine {
    pub async fn send_snapshot(
        self: &Arc<Self>,
        network: &Network,
        project: Uuid,
    ) -> Result<SendStatus> {
        self.send_impl(network, project, false, Stop::default())
            .await
    }

    /// Like `send_snapshot`, but the folder scan ends with `snapshot::Stopped` when `stop` is
    /// requested. Nothing is shared then; once a version is committed it is always announced.
    pub async fn send_snapshot_until(
        self: &Arc<Self>,
        network: &Network,
        project: Uuid,
        stop: Stop,
    ) -> Result<SendStatus> {
        self.send_impl(network, project, false, stop).await
    }

    pub async fn send_saved_snapshot(
        self: &Arc<Self>,
        network: &Network,
        project: Uuid,
    ) -> Result<SendStatus> {
        self.send_impl(network, project, true, Stop::default())
            .await
    }

    async fn send_impl(
        self: &Arc<Self>,
        network: &Network,
        project: Uuid,
        settled: bool,
        stop: Stop,
    ) -> Result<SendStatus> {
        let engine = self.clone();
        let snapshot_id = tokio::task::spawn_blocking(move || {
            let snapshot = if settled {
                engine.snapshot_after_save(project)
            } else {
                engine.snapshot_until(project, stop)
            };
            snapshot.map(|s| s.manifest.snapshot_id)
        })
        .await??;
        self.notify_snapshot_available(network.clone(), project, snapshot_id);
        Ok(SendStatus { snapshot_id })
    }

    fn notify_snapshot_available(&self, network: Network, project: Uuid, snapshot_id: Uuid) {
        let store = self.store.clone();
        tokio::spawn(async move {
            let trusted =
                match tokio::task::spawn_blocking(move || store.peers(Some(project))).await {
                    Ok(Ok(peers)) => peers,
                    _ => return,
                };
            let mut requests = tokio::task::JoinSet::new();
            for peer in trusted
                .into_iter()
                .filter(|peer| network.online(&peer.peer_id))
            {
                let network = network.clone();
                let peer_id = peer.peer_id;
                requests.spawn(async move {
                    let _ = tokio::time::timeout(
                        std::time::Duration::from_secs(2),
                        network.request(
                            &peer_id,
                            PeerRequest::SnapshotAvailable {
                                project_id: project,
                                snapshot_id,
                            },
                        ),
                    )
                    .await;
                });
                if requests.len() >= 8 {
                    let _ = requests.join_next().await;
                }
            }
            while requests.join_next().await.is_some() {}
        });
    }
}
