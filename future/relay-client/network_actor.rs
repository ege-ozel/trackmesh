//! The sole swarm owner. Application requests, retry ticks, and events are serialized here.
use crate::{
    behaviour::{build_swarm, BehaviourEvent, PeerSwarm},
    discovery::{configure, Discovery},
    handle::{Command, ExternalAddressSource, IncomingChannel},
    status::{refresh_status, Connections},
    Incoming, IncomingRequest, Network, NetworkConfig, NetworkStatus,
};
use anyhow::Result;
use crypto::Identity;
use futures::StreamExt;
use libp2p::{
    core::ConnectedPoint,
    identify,
    multiaddr::Protocol,
    ping, request_response,
    swarm::{ConnectionId, SwarmEvent},
    Multiaddr, PeerId,
};
use protocol::{PairingResponse, PeerRequest, PeerResponse};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};
use storage::Store;
use tokio::sync::{mpsc, oneshot};

pub fn start(
    identity: Arc<Identity>,
    store: Arc<Store>,
    config: NetworkConfig,
) -> Result<(
    Network,
    mpsc::Receiver<Incoming>,
    tokio::task::JoinHandle<Result<()>>,
)> {
    let manual_external_addresses = config
        .external_addresses
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    let swarm = build_swarm(&identity, &store, &config)?;
    let id = *swarm.local_peer_id();
    let (tx, commands) = mpsc::channel(128);
    let (incoming_tx, incoming_rx) = mpsc::channel(24);
    let status = Arc::new(RwLock::new(NetworkStatus {
        peer_id: id.to_string(),
        ..Default::default()
    }));
    let (mapping_tx, _mapping_rx) = tokio::sync::watch::channel(false);
    let handle = Network {
        command: tx,
        status: status.clone(),
        mapping_consent: Arc::new(mapping_tx),
    };
    let actor = Actor {
        swarm,
        store,
        namespaces: config.rendezvous,
        commands,
        incoming_tx,
        status,
        pending: HashMap::new(),
        pairing_pending: HashMap::new(),
        pairing_rates: HashMap::new(),
        connections: HashMap::new(),
        inbound_connections: HashMap::new(),
        discovery: Discovery::default(),
        ip_source: manual_external_addresses
            .into_iter()
            .map(|address| (address, HashSet::from([ExternalAddressSource::Manual])))
            .collect(),
        gateway_external_addresses: HashSet::new(),
    };
    let task = tokio::spawn(actor.run());
    Ok((handle, incoming_rx, task))
}

struct Actor {
    swarm: PeerSwarm,
    store: Arc<Store>,
    namespaces: Vec<(Multiaddr, String)>,
    commands: mpsc::Receiver<Command>,
    incoming_tx: mpsc::Sender<Incoming>,
    status: Arc<RwLock<NetworkStatus>>,
    pending: HashMap<request_response::OutboundRequestId, oneshot::Sender<Result<PeerResponse>>>,
    pairing_pending:
        HashMap<request_response::OutboundRequestId, oneshot::Sender<Result<PairingResponse>>>,
    pairing_rates: HashMap<PeerId, (Instant, u8)>,
    connections: Connections,
    inbound_connections: HashMap<ConnectionId, InboundConnection>,
    discovery: Discovery,
    ip_source: HashMap<Multiaddr, HashSet<ExternalAddressSource>>,
    gateway_external_addresses: HashSet<Multiaddr>,
}

struct InboundConnection {
    peer: PeerId,
    transport: String,
    local_addr: Multiaddr,
}

impl Actor {
    async fn run(mut self) -> Result<()> {
        let result = self.run_loop().await;
        let stopped = result
            .as_ref()
            .err()
            .map(|e| format!("network stopped: {e:#}"))
            .unwrap_or_else(|| "network stopped".into());
        for (_, reply) in self.pending {
            let _ = reply.send(Err(anyhow::anyhow!(stopped.clone())));
        }
        for (_, reply) in self.pairing_pending {
            let _ = reply.send(Err(anyhow::anyhow!("pairing network stopped")));
        }
        if let Ok(mut state) = self.status.write() {
            state.error = result.as_ref().err().map(|e| format!("{e:#}"));
            state.addresses.clear();
            state.reachability = crate::ReachabilityStatus::default();
            for peer in state.peers.values_mut() {
                peer.online = false;
                peer.route = "OFFLINE".into();
                peer.transport.clear();
            }
        }
        result
    }

    async fn run_loop(&mut self) -> Result<()> {
        // Tokio's first interval tick is immediate; Discovery starts existing trusted-peer dials at startup.
        let mut retry = tokio::time::interval(Duration::from_secs(6));
        loop {
            tokio::select! {
                command = self.commands.recv() => {
                    if !self.command(command)? { break; }
                },
                _ = retry.tick() => {
                    configure(&mut self.swarm, &self.store, &self.namespaces)?;
                    self.pending.retain(|_, reply| !reply.is_closed());
                    self.pairing_pending.retain(|_, reply| !reply.is_closed());
                    self.pairing_rates.retain(|_, (at, _)| at.elapsed() < Duration::from_secs(60));
                    for peer in self.swarm.behaviour().allow.expired_peers() {
                        let _ = self.swarm.disconnect_peer_id(peer);
                    }
                    self.discovery.tick(&mut self.swarm, &self.store, &self.namespaces, &self.connections)?;
                },
                event = self.swarm.select_next_some() => self.event(event)?,
            }
        }
        Ok(())
    }

    fn command(&mut self, command: Option<Command>) -> Result<bool> {
        match command {
            Some(Command::Request(peer, request, reply)) => {
                if self
                    .store
                    .authorized(protocol::request_project(&request), &peer.to_string())?
                    .is_none()
                {
                    let _ =
                        reply.send(Err(anyhow::anyhow!("peer is not trusted for this project")));
                    return Ok(true);
                }
                if matches!(&request, protocol::PeerRequest::EndpointUpdate { .. })
                    && !self
                        .connections
                        .values()
                        .any(|(connected, relayed, _)| *connected == peer && !*relayed)
                {
                    let _ = reply.send(Err(anyhow::anyhow!(
                        "endpoint updates require an existing direct connection"
                    )));
                    return Ok(true);
                }
                let id = self
                    .swarm
                    .behaviour_mut()
                    .transfer
                    .send_request(&peer, request);
                self.pending.insert(id, reply);
            }
            Some(Command::Respond(incoming, response)) => {
                let IncomingChannel::Transfer(channel) = incoming.channel else {
                    return Ok(true);
                };
                let _ = self
                    .swarm
                    .behaviour_mut()
                    .transfer
                    .send_response(channel, *response);
            }
            Some(Command::PairingWindow(duration)) => {
                self.swarm.behaviour_mut().allow.window(duration);
                for peer in self.swarm.behaviour().allow.expired_peers() {
                    let _ = self.swarm.disconnect_peer_id(peer);
                }
            }
            Some(Command::PairingRequest(peer, request, reply)) => {
                // Outbound enrollment is only to a locally paired inviter, never a public peer.
                if !self.swarm.behaviour().allow.allowed_peers().contains(&peer)
                    || self.pairing_pending.len() >= 8
                {
                    let _ = reply.send(Err(anyhow::anyhow!("inviter is unavailable for pairing")));
                    return Ok(true);
                }
                let id = self
                    .swarm
                    .behaviour_mut()
                    .pairing
                    .send_request(&peer, request);
                self.pairing_pending.insert(id, reply);
            }
            Some(Command::PairingRespond(incoming, response)) => {
                if let IncomingChannel::Pairing(channel) = incoming.channel {
                    let _ = self
                        .swarm
                        .behaviour_mut()
                        .pairing
                        .send_response(channel, response);
                }
            }
            Some(Command::Reload(new)) => {
                self.namespaces = new;
                configure(&mut self.swarm, &self.store, &self.namespaces)?;
                self.discovery.reload();
            }
            Some(Command::SetExternalAddresses(addresses)) => {
                let new = addresses.into_iter().collect::<HashSet<_>>();
                let previous = std::mem::replace(&mut self.gateway_external_addresses, new.clone());
                for address in previous.difference(&new) {
                    self.remove_external_source(address, ExternalAddressSource::Gateway);
                }
                for address in new.difference(&previous) {
                    self.add_external_source(address.clone(), ExternalAddressSource::Gateway);
                }
                self.inbound_connections.clear();
                self.reset_reachability();
            }
            Some(Command::EndpointAddresses(reply)) => {
                let _ = reply.send(self.local_endpoint_addresses());
            }
            Some(Command::LearnPeerAddresses(peer, addresses, sequence, expiry)) => {
                if self.swarm.behaviour().allow.allowed_peers().contains(&peer) {
                    self.discovery
                        .learn_endpoint_addresses(peer, addresses, sequence, expiry);
                }
            }
            Some(Command::NetworkChanged) => {
                self.inbound_connections.clear();
                self.reset_reachability();
                self.discovery.network_changed();
            }
            Some(Command::Shutdown) | None => return Ok(false),
        }
        Ok(true)
    }

    fn local_endpoint_addresses(&self) -> Vec<String> {
        let local_peer = self.swarm.local_peer_id().to_string();
        let mut candidates = HashSet::new();
        for address in self.ip_source.keys() {
            let value = without_local_peer_suffix(address, &local_peer);
            if protocol::validate_endpoint_address(&value).is_ok() {
                candidates.insert(value);
            }
        }
        if let Ok(status) = self.status.read() {
            for address in &status.addresses {
                let Ok(address) = address.parse::<Multiaddr>() else {
                    continue;
                };
                let value = without_local_peer_suffix(&address, &local_peer);
                if protocol::validate_endpoint_address(&value).is_ok() {
                    candidates.insert(value);
                }
            }
        }
        select_endpoint_candidates(candidates.into_iter().collect())
    }

    fn add_external_source(&mut self, address: Multiaddr, source: ExternalAddressSource) {
        let sources = self.ip_source.entry(address.clone()).or_default();
        if sources.insert(source) && sources.len() == 1 {
            self.swarm.add_external_address(address);
        }
    }

    fn remove_external_source(&mut self, address: &Multiaddr, source: ExternalAddressSource) {
        let Some(sources) = self.ip_source.get_mut(address) else {
            return;
        };
        sources.remove(&source);
        if sources.is_empty() {
            self.ip_source.remove(address);
            self.swarm.remove_external_address(address);
        }
    }

    fn event(&mut self, event: SwarmEvent<BehaviourEvent>) -> Result<()> {
        match event {
            SwarmEvent::NewListenAddr { address, .. } => {
                let id = *self.swarm.local_peer_id();
                let address = if address.iter().last() == Some(Protocol::P2p(id)) {
                    address
                } else {
                    address.with(Protocol::P2p(id))
                };
                tracing::info!(%address, "listening");
                if let Ok(mut state) = self.status.write() {
                    let address = address.to_string();
                    if !state.addresses.contains(&address) {
                        state.addresses.push(address);
                    }
                }
            }
            SwarmEvent::ExpiredListenAddr { address, .. } => {
                if let Ok(mut state) = self.status.write() {
                    state
                        .addresses
                        .retain(|a| !a.starts_with(&address.to_string()));
                }
            }
            SwarmEvent::ConnectionEstablished {
                peer_id,
                connection_id,
                endpoint,
                ..
            } => {
                if let ConnectedPoint::Listener {
                    local_addr,
                    send_back_addr,
                } = &endpoint
                {
                    let relayed = send_back_addr
                        .iter()
                        .any(|p| matches!(p, Protocol::P2pCircuit));
                    let transport = if send_back_addr.iter().any(|p| matches!(p, Protocol::QuicV1))
                    {
                        "QUIC"
                    } else {
                        "TCP"
                    };
                    if !relayed && is_public_remote(send_back_addr) {
                        self.inbound_connections.insert(
                            connection_id,
                            InboundConnection {
                                peer: peer_id,
                                transport: transport.into(),
                                local_addr: local_addr.clone(),
                            },
                        );
                    }
                }
                self.connected(peer_id, connection_id, endpoint.get_remote_address())
            }
            SwarmEvent::ConnectionClosed {
                connection_id,
                peer_id,
                ..
            } => {
                self.connections.remove(&connection_id);
                self.inbound_connections.remove(&connection_id);
                refresh_status(&self.status, &self.connections);
                self.discovery.disconnected(peer_id, &self.namespaces);
            }
            SwarmEvent::Behaviour(BehaviourEvent::Identify(identify::Event::Received {
                peer_id,
                info,
                ..
            })) => {
                // Enrollment-only identities must not populate discovery or learned addresses.
                if self
                    .swarm
                    .behaviour()
                    .allow
                    .allowed_peers()
                    .contains(&peer_id)
                {
                    if let Some(address) =
                        self.discovery
                            .identified(&mut self.swarm, peer_id, info, &self.namespaces)
                    {
                        self.add_external_source(address, ExternalAddressSource::PeerObserved);
                    }
                }
            }
            SwarmEvent::Behaviour(BehaviourEvent::Pairing(request_response::Event::Message {
                peer,
                message,
                ..
            })) => match message {
                request_response::Message::Request {
                    request, channel, ..
                } => {
                    let allow = &self.swarm.behaviour().allow;
                    let available = allow.is_open() || allow.allowed_peers().contains(&peer);
                    let within_limit = if let Some((_, count)) = self.pairing_rates.get_mut(&peer) {
                        *count = count.saturating_add(1);
                        *count <= 16
                    } else if self.pairing_rates.len() < 64 {
                        self.pairing_rates.insert(peer, (Instant::now(), 1));
                        true
                    } else {
                        false
                    };
                    if !available || !within_limit {
                        let _ = self
                            .swarm
                            .behaviour_mut()
                            .pairing
                            .send_response(channel, PairingResponse::Unavailable);
                        return Ok(());
                    }
                    let incoming = Incoming {
                        peer,
                        request: IncomingRequest::Pairing(request),
                        channel: IncomingChannel::Pairing(channel),
                    };
                    if let Err(error) = self.incoming_tx.try_send(incoming) {
                        if let IncomingChannel::Pairing(channel) = error.into_inner().channel {
                            let _ = self
                                .swarm
                                .behaviour_mut()
                                .pairing
                                .send_response(channel, PairingResponse::Unavailable);
                        }
                    }
                }
                request_response::Message::Response {
                    request_id,
                    response,
                } => {
                    if let Some(reply) = self.pairing_pending.remove(&request_id) {
                        let _ = reply.send(Ok(response));
                    }
                }
            },
            SwarmEvent::Behaviour(BehaviourEvent::Pairing(
                request_response::Event::OutboundFailure { request_id, .. },
            )) => {
                if let Some(reply) = self.pairing_pending.remove(&request_id) {
                    let _ = reply.send(Err(anyhow::anyhow!("automatic pairing failed; check the inviter's pairing window and connection")));
                }
            }
            SwarmEvent::Behaviour(BehaviourEvent::Rendezvous(event)) => {
                self.discovery.event(&mut self.swarm, &self.store, event)?
            }
            SwarmEvent::Behaviour(BehaviourEvent::Transfer(request_response::Event::Message {
                peer,
                connection_id,
                message,
                ..
            })) => match message {
                request_response::Message::Request {
                    request, channel, ..
                } => {
                    if matches!(&request, PeerRequest::EndpointUpdate { .. })
                        && !self
                            .connections
                            .get(&connection_id)
                            .is_some_and(|(connected, relayed, _)| *connected == peer && !*relayed)
                    {
                        let _ = self.swarm.behaviour_mut().transfer.send_response(
                            channel,
                            PeerResponse::Error {
                                message: "endpoint updates require a direct connection".into(),
                            },
                        );
                        return Ok(());
                    }
                    if self
                        .store
                        .authorized(protocol::request_project(&request), &peer.to_string())?
                        .is_none()
                    {
                        let _ = self.swarm.behaviour_mut().transfer.send_response(
                            channel,
                            PeerResponse::Error {
                                message: "project access denied".into(),
                            },
                        );
                        return Ok(());
                    }
                    self.record_inbound_observation(peer, connection_id);
                    let request = match request {
                        PeerRequest::EndpointUpdate { project_id, record } => {
                            IncomingRequest::EndpointUpdate { project_id, record }
                        }
                        request => IncomingRequest::Transfer(request),
                    };
                    let incoming = Incoming {
                        peer,
                        request,
                        channel: IncomingChannel::Transfer(channel),
                    };
                    if let Err(error) = self.incoming_tx.try_send(incoming) {
                        let incoming = error.into_inner();
                        if let IncomingChannel::Transfer(channel) = incoming.channel {
                            let _ = self.swarm.behaviour_mut().transfer.send_response(
                                channel,
                                PeerResponse::Error {
                                    message: "peer busy or shutting down; retry".into(),
                                },
                            );
                        }
                    }
                }
                request_response::Message::Response {
                    request_id,
                    response,
                } => {
                    if let Some(reply) = self.pending.remove(&request_id) {
                        let _ = reply.send(Ok(response));
                    }
                }
            },
            SwarmEvent::Behaviour(BehaviourEvent::Transfer(
                request_response::Event::OutboundFailure {
                    request_id, error, ..
                },
            )) => {
                if let Some(reply) = self.pending.remove(&request_id) {
                    let _ = reply.send(Err(anyhow::anyhow!("peer request failed: {error}")));
                }
            }
            SwarmEvent::Behaviour(BehaviourEvent::Ping(ping::Event {
                peer,
                result: Err(_),
                ..
            })) => {
                let _ = self.swarm.disconnect_peer_id(peer);
            }
            SwarmEvent::OutgoingConnectionError { error, .. } => {
                tracing::debug!(%error, "dial failed")
            }
            _ => {}
        }
        Ok(())
    }

    fn connected(&mut self, peer: PeerId, connection: ConnectionId, address: &Multiaddr) {
        let relayed = address.iter().any(|p| matches!(p, Protocol::P2pCircuit));
        let transport = if relayed {
            "NOISE/YAMUX"
        } else if address.iter().any(|p| matches!(p, Protocol::QuicV1)) {
            "QUIC"
        } else {
            "TCP"
        }
        .to_owned();
        let better = self
            .connections
            .values()
            .any(|(p, r, t)| *p == peer && !r && (relayed || (t == "QUIC" && transport != "QUIC")));
        if better {
            self.swarm.close_connection(connection);
            return;
        }
        tracing::info!(peer=%peer, route=if relayed {"RELAYED"} else {"DIRECT"}, %transport, "peer connected");
        if !relayed && transport == "QUIC" {
            let close: Vec<_> = self
                .connections
                .iter()
                .filter_map(|(cid, (p, _, t))| {
                    if *p == peer && t == "TCP" {
                        Some(*cid)
                    } else {
                        None
                    }
                })
                .collect();
            for cid in close {
                self.swarm.close_connection(cid);
            }
        }
        self.connections
            .insert(connection, (peer, relayed, transport));
        // Request/response then uses the remaining direct peer connection.
        if !relayed {
            let close: Vec<_> = self
                .connections
                .iter()
                .filter_map(|(cid, (p, r, _))| if *p == peer && *r { Some(*cid) } else { None })
                .collect();
            for cid in close {
                self.swarm.close_connection(cid);
            }
        }
        refresh_status(&self.status, &self.connections);
    }

    fn record_inbound_observation(&mut self, peer: PeerId, connection: ConnectionId) {
        let Some(inbound) = self.inbound_connections.get(&connection) else {
            return;
        };
        if inbound.peer != peer
            || !listener_matches(&self.status, &inbound.local_addr, &inbound.transport)
        {
            return;
        }
        let Ok(mut status) = self.status.write() else {
            return;
        };
        status.reachability = crate::ReachabilityStatus {
            state: "UNVERIFIED".into(),
            peer: Some(
                peer.to_string()
                    .chars()
                    .rev()
                    .take(8)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect(),
            ),
            transport: Some(inbound.transport.clone()),
            at_unix_seconds: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|value| value.as_secs()),
        };
    }

    fn reset_reachability(&mut self) {
        if let Ok(mut status) = self.status.write() {
            status.reachability = crate::ReachabilityStatus::default();
        }
    }
}

fn listener_matches(status: &RwLock<NetworkStatus>, local: &Multiaddr, transport: &str) -> bool {
    let Some(port) = multiaddr_transport_port(local, transport) else {
        return false;
    };
    status.read().ok().is_some_and(|state| {
        state.addresses.iter().any(|address| {
            address
                .parse::<Multiaddr>()
                .ok()
                .and_then(|value| multiaddr_transport_port(&value, transport))
                == Some(port)
        })
    })
}

fn multiaddr_transport_port(address: &Multiaddr, transport: &str) -> Option<u16> {
    let mut protocols = address.iter();
    while let Some(protocol) = protocols.next() {
        match (transport, protocol) {
            ("TCP", Protocol::Tcp(port)) => return Some(port),
            ("QUIC", Protocol::Udp(port)) if matches!(protocols.next(), Some(Protocol::QuicV1)) => {
                return Some(port)
            }
            _ => {}
        }
    }
    None
}

fn without_local_peer_suffix(address: &Multiaddr, local_peer: &str) -> String {
    let address = address.to_string();
    let suffix = format!("/p2p/{local_peer}");
    address.strip_suffix(&suffix).unwrap_or(&address).to_owned()
}

pub fn is_public_endpoint_address(address: &str) -> bool {
    address
        .parse::<Multiaddr>()
        .is_ok_and(|address| is_public_remote(&address))
}

fn select_endpoint_candidates(mut candidates: Vec<String>) -> Vec<String> {
    candidates.sort();
    candidates.dedup();
    let (mut public, private): (Vec<_>, Vec<_>) = candidates
        .into_iter()
        .partition(|address| is_public_endpoint_address(address));
    public.extend(private);
    public.truncate(protocol::MAX_ENDPOINT_ADDRESSES);
    public.sort();
    public
}

fn is_public_remote(address: &Multiaddr) -> bool {
    if address
        .iter()
        .any(|protocol| matches!(protocol, Protocol::P2pCircuit))
    {
        return false;
    }
    address
        .iter()
        .find_map(|protocol| match protocol {
            Protocol::Ip4(ip) => Some(is_public_ipv4(ip)),
            Protocol::Ip6(ip) => Some(is_public_ipv6(ip)),
            _ => None,
        })
        .unwrap_or(false)
}

fn is_public_ipv4(ip: std::net::Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !ip.is_private()
        && !ip.is_loopback()
        && !ip.is_link_local()
        && !ip.is_documentation()
        && !ip.is_broadcast()
        && a != 0
        && a < 224
        && !(a == 100 && (64..=127).contains(&b))
        && !(a == 198 && (b == 18 || b == 19))
        && !(a == 192 && ((b == 0 && c == 0) || (b == 88 && c == 99)))
}

fn is_public_ipv6(ip: std::net::Ipv6Addr) -> bool {
    let segments = ip.segments();
    (segments[0] & 0xe000) == 0x2000
        && !(segments[0] == 0x2001 && segments[1] <= 0x01ff)
        && !(segments[0] == 0x2001 && segments[1] == 0x0db8)
        && !(segments[0] == 0x2002)
        && !(segments[0] == 0x3fff && (segments[1] & 0xf000) == 0x0000)
        && ip.to_ipv4_mapped().is_none_or(is_public_ipv4)
}
