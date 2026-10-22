//! Bounded commands and responses between application tasks and the single swarm owner.
use crate::NetworkStatus;
use anyhow::{ensure, Context, Result};
use libp2p::{request_response, Multiaddr, PeerId};
use protocol::{EndpointRecord, PairingRequest, PairingResponse, PeerRequest, PeerResponse};
use std::{
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};
use tokio::sync::watch;
use tokio::sync::{mpsc, oneshot};
pub struct Incoming {
    pub peer: PeerId,
    pub request: IncomingRequest,
    pub(super) channel: IncomingChannel,
}
#[derive(Clone)]
pub enum IncomingRequest {
    EndpointUpdate {
        project_id: uuid::Uuid,
        record: EndpointRecord,
    },
    Transfer(PeerRequest),
    Pairing(PairingRequest),
}
pub(super) enum IncomingChannel {
    Transfer(request_response::ResponseChannel<PeerResponse>),
    Pairing(request_response::ResponseChannel<PairingResponse>),
}
pub(super) enum Command {
    Request(PeerId, PeerRequest, oneshot::Sender<Result<PeerResponse>>),
    Respond(Incoming, Box<PeerResponse>),
    PairingRequest(
        PeerId,
        PairingRequest,
        oneshot::Sender<Result<PairingResponse>>,
    ),
    PairingRespond(Incoming, PairingResponse),
    PairingWindow(Duration),
    Reload,
    SetExternalAddresses(Vec<Multiaddr>),
    EndpointAddresses(oneshot::Sender<Vec<String>>),
    LearnPeerAddresses(PeerId, Vec<Multiaddr>, u64, Instant),
    NetworkChanged,
    Shutdown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum ExternalAddressSource {
    Gateway,
    Manual,
}
#[derive(Clone)]
pub struct Network {
    pub(super) command: mpsc::Sender<Command>,
    pub(super) status: Arc<RwLock<NetworkStatus>>,
    pub(super) mapping_consent: Arc<watch::Sender<bool>>,
}
impl Network {
    pub async fn network_changed(&self) -> Result<()> {
        self.command
            .send(Command::NetworkChanged)
            .await
            .context("network stopped")
    }

    pub async fn set_external_addresses(&self, addresses: Vec<Multiaddr>) -> Result<()> {
        self.command
            .send(Command::SetExternalAddresses(addresses))
            .await
            .context("network stopped")
    }

    /// Snapshot currently observed, mapped, and listening addresses suitable for endpoint records.
    pub async fn endpoint_addresses(&self) -> Result<Vec<String>> {
        let (tx, rx) = oneshot::channel();
        self.command
            .send(Command::EndpointAddresses(tx))
            .await
            .context("network stopped")?;
        rx.await.context("network actor stopped")
    }

    /// Learn a validated peer endpoint until its record expiry; does not dial immediately.
    pub async fn learn_peer_addresses(
        &self,
        peer: &str,
        addresses: Vec<String>,
        sequence: u64,
        ttl: Duration,
    ) -> Result<()> {
        ensure!(
            addresses.len() <= protocol::MAX_ENDPOINT_ADDRESSES,
            "too many endpoint addresses"
        );
        let peer: PeerId = peer.parse()?;
        let mut parsed = Vec::with_capacity(addresses.len());
        for address in addresses {
            protocol::validate_endpoint_address(&address)?;
            let multiaddr: Multiaddr = address.parse()?;
            ensure!(
                multiaddr.to_string() == address,
                "endpoint address is not canonical"
            );
            parsed.push(multiaddr);
        }
        parsed.sort_by_key(ToString::to_string);
        parsed.dedup();
        let expiry = Instant::now()
            .checked_add(ttl)
            .context("endpoint expiry is out of range")?;
        self.command
            .send(Command::LearnPeerAddresses(peer, parsed, sequence, expiry))
            .await
            .context("network stopped")
    }

    pub fn set_mapping_consent(&self, enabled: bool) {
        self.mapping_consent.send_if_modified(|current| {
            if *current == enabled {
                false
            } else {
                *current = enabled;
                true
            }
        });
        if let Ok(mut status) = self.status.write() {
            if enabled {
                status.port_mapping.consent_enabled = true;
            } else if status.port_mapping.state == "MAPPED"
                || status.port_mapping.state == "REMOVING"
            {
                status.port_mapping.consent_enabled = false;
                status.port_mapping.state = "REMOVING".into();
            } else if status.port_mapping.state == "FAILED"
                && status
                    .port_mapping
                    .detail
                    .as_deref()
                    .is_some_and(|detail| detail.contains("cleanup"))
            {
                status.port_mapping.consent_enabled = false;
            } else {
                status.port_mapping = crate::PortMappingStatus::default();
            }
        }
    }
    pub async fn pairing_window(&self, duration: Duration) -> Result<()> {
        self.command
            .send(Command::PairingWindow(duration))
            .await
            .context("network stopped")
    }
    pub async fn pairing_request(
        &self,
        peer: &str,
        request: PairingRequest,
    ) -> Result<PairingResponse> {
        let peer: PeerId = peer.parse()?;
        ensure!(
            self.online(&peer.to_string()),
            "inviter is offline or unreachable"
        );
        let (tx, rx) = oneshot::channel();
        self.command
            .send(Command::PairingRequest(peer, request, tx))
            .await
            .context("network stopped")?;
        tokio::time::timeout(Duration::from_secs(25), rx)
            .await
            .context("pairing request timed out")??
    }
    pub async fn respond_pairing(
        &self,
        incoming: Incoming,
        response: PairingResponse,
    ) -> Result<()> {
        self.command
            .send(Command::PairingRespond(incoming, response))
            .await
            .context("network stopped")
    }
    pub fn status(&self) -> NetworkStatus {
        self.status.read().map(|s| s.clone()).unwrap_or_default()
    }
    pub fn record_received_snapshot(
        &self,
        peer_id: String,
        project_id: uuid::Uuid,
        snapshot_id: uuid::Uuid,
    ) {
        if let Ok(mut status) = self.status.write() {
            status
                .received_snapshots
                .push(crate::status::ReceivedSnapshot {
                    peer_id,
                    project_id,
                    snapshot_id,
                });
            if status.received_snapshots.len() > 128 {
                status.received_snapshots.drain(..64);
            }
        }
    }
    pub fn online(&self, peer: &str) -> bool {
        self.status().peers.get(peer).is_some_and(|p| p.online)
    }
    pub async fn request(&self, peer: &str, request: PeerRequest) -> Result<PeerResponse> {
        let peer: PeerId = peer.parse()?;
        ensure!(
            self.online(&peer.to_string()),
            "source peer is offline; retry when it is online"
        );
        let (tx, rx) = oneshot::channel();
        self.command
            .send(Command::Request(peer, request, tx))
            .await
            .context("network stopped")?;
        tokio::time::timeout(Duration::from_secs(90), rx)
            .await
            .context("peer request timed out")??
    }
    pub async fn respond(&self, incoming: Incoming, response: PeerResponse) -> Result<()> {
        self.command
            .send(Command::Respond(incoming, Box::new(response)))
            .await
            .context("network stopped")
    }
    /// Re-reads the trusted-peer allow-list now instead of at the next retry tick.
    pub async fn reload(&self) -> Result<()> {
        self.command
            .send(Command::Reload)
            .await
            .context("network stopped")
    }
    pub async fn shutdown(&self) {
        let _ = self.command.send(Command::Shutdown).await;
    }
}
