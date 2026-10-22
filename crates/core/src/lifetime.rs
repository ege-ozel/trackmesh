//! Work-based owner retention; no filesystem observation or permanent service.
use crate::{peer::PeerRuntime, Engine, PluginRuntime};
use anyhow::Result;
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct SessionGrace {
    idle_since: Option<Instant>,
    epochs: Option<(u64, u64)>,
}

impl SessionGrace {
    /// Brief sessions/requests between polls reset grace just like long-running work.
    pub fn ready(&mut self, plugin: &PluginRuntime, peer: &PeerRuntime, busy: bool) -> bool {
        let epochs = plugin.idle_epoch().zip(peer.idle_epoch());
        if busy || epochs.is_none() || self.epochs != epochs {
            self.idle_since = None;
        }
        self.epochs = epochs;
        if busy || epochs.is_none() {
            return false;
        }
        self.idle_since.get_or_insert_with(Instant::now).elapsed() >= Duration::from_secs(60)
    }
}

impl Engine {
    pub fn tray_notice_shown(&self) -> Result<bool> {
        Ok(self
            .settings
            .lock()
            .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?
            .value
            .tray_notice_shown)
    }

    pub fn mark_tray_notice_shown(&self) -> Result<()> {
        self.settings
            .lock()
            .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?
            .mark_tray_notice_shown(&self.directory)
    }

    /// Collaborators who would miss this PC's latest version while TrackMesh is closed. Someone known
    /// to have it already (the same or a newer version, or a receipt for it) is left out, so quitting
    /// asks nothing when everyone is up to date.
    pub fn shutdown_recipients(
        &self,
        received: &[network::ReceivedSnapshot],
    ) -> Result<Vec<String>> {
        let mut recipients = std::collections::BTreeSet::new();
        for project in self.store.projects()? {
            let Some(latest) = project.latest_local_snapshot else {
                continue;
            };
            for peer in self.project_summary(project.project_id)?.peers {
                let has_latest = matches!(
                    peer.known_lineage,
                    Some(storage::Lineage::Equal | storage::Lineage::RemoteAhead)
                ) || peer.delivered_snapshot == Some(latest)
                    || received.iter().any(|item| {
                        item.project_id == project.project_id
                            && item.peer_id == peer.device.peer_id
                            && item.snapshot_id == latest
                    });
                if !has_latest {
                    recipients.insert(peer.device.name);
                }
            }
        }
        Ok(recipients.into_iter().collect())
    }
}
