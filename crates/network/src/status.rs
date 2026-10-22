//! Connection reporting. Online equality is recorded by core workflows, not inferred here.
use libp2p::{swarm::ConnectionId, PeerId};
use serde::Serialize;
use std::{collections::HashMap, sync::RwLock};

pub(super) type Connections = HashMap<ConnectionId, (PeerId, String)>;
#[derive(Clone, Debug, Serialize, Default)]
pub struct NetworkStatus {
    pub peer_id: String,
    pub addresses: Vec<String>,
    pub peers: HashMap<String, ConnectionStatus>,
    pub error: Option<String>,
    pub port_mapping: PortMappingStatus,
    pub reachability: ReachabilityStatus,
    #[serde(default)]
    pub received_snapshots: Vec<ReceivedSnapshot>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ReceivedSnapshot {
    pub peer_id: String,
    pub project_id: uuid::Uuid,
    pub snapshot_id: uuid::Uuid,
}
#[derive(Clone, Debug, Serialize)]
pub struct ReachabilityStatus {
    pub state: String,
    pub peer: Option<String>,
    pub transport: Option<String>,
    pub at_unix_seconds: Option<u64>,
}
impl Default for ReachabilityStatus {
    fn default() -> Self {
        Self {
            state: "UNVERIFIED".into(),
            peer: None,
            transport: None,
            at_unix_seconds: None,
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct PortMappingStatus {
    pub consent_enabled: bool,
    pub state: String,
    pub tcp: String,
    pub udp: String,
    pub detail: Option<String>,
    pub external_ip_state: String,
}
impl Default for PortMappingStatus {
    fn default() -> Self {
        Self {
            consent_enabled: false,
            state: "NOT_ATTEMPTED".into(),
            tcp: "NOT_ATTEMPTED".into(),
            udp: "NOT_ATTEMPTED".into(),
            detail: None,
            external_ip_state: "UNKNOWN".into(),
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct ConnectionStatus {
    /// Changes only when this peer comes online after having no live connection.
    pub generation: u64,
    pub online: bool,
    pub route: String,
    pub transport: String,
}
pub(super) fn refresh_status(status: &RwLock<NetworkStatus>, connections: &Connections) {
    if let Ok(mut status) = status.write() {
        let previously_online = status
            .peers
            .iter()
            .filter_map(|(peer, state)| state.online.then_some(peer.clone()))
            .collect::<std::collections::HashSet<_>>();
        for state in status.peers.values_mut() {
            state.online = false;
            state.route = "OFFLINE".into();
        }
        for (peer, transport) in connections.values() {
            let state = status
                .peers
                .entry(peer.to_string())
                .or_insert(ConnectionStatus {
                    generation: 0,
                    online: false,
                    route: String::new(),
                    transport: String::new(),
                });
            if !state.online && !previously_online.contains(&peer.to_string()) {
                state.generation = state.generation.wrapping_add(1);
            }
            state.online = true;
            state.route = "DIRECT".into();
            state.transport = transport.clone();
        }
    }
}
