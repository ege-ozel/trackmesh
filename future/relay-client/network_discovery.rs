//! Private discovery and retry ordering: QUIC, TCP, then circuits. Never dial public peers.
use crate::{
    behaviour::{Behaviour, PeerSwarm},
    status::Connections,
    validate_card,
};
use anyhow::{ensure, Result};
use libp2p::{
    identify,
    multiaddr::Protocol,
    rendezvous,
    swarm::dial_opts::{DialOpts, PeerCondition},
    Multiaddr, PeerId,
};
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};
use storage::Store;
pub(super) fn infrastructure_id(address: &Multiaddr) -> Result<PeerId> {
    ensure!(
        !address.iter().any(|p| matches!(p, Protocol::P2pCircuit)),
        "infrastructure address must be direct"
    );
    match address.iter().last() {
        Some(Protocol::P2p(peer)) => Ok(peer),
        _ => anyhow::bail!("relay address must end in its pinned /p2p/PeerId"),
    }
}
pub(super) fn configure(
    swarm: &mut libp2p::Swarm<Behaviour>,
    store: &Store,
    namespaces: &[(Multiaddr, String)],
) -> Result<()> {
    let mut allowed = HashSet::new();
    for card in store.peers(None)? {
        validate_card(&card)?;
        let peer: PeerId = card.peer_id.parse()?;
        allowed.insert(peer);
        swarm.behaviour_mut().allow.allow_peer(peer);
    }
    for (address, _) in namespaces {
        let peer = infrastructure_id(address)?;
        allowed.insert(peer);
        swarm.behaviour_mut().allow.allow_peer(peer);
    }
    let existing: Vec<_> = swarm
        .behaviour()
        .allow
        .allowed_peers()
        .iter()
        .copied()
        .collect();
    for peer in existing {
        if !allowed.contains(&peer) {
            swarm.behaviour_mut().allow.disallow_peer(peer);
            let _ = swarm.disconnect_peer_id(peer);
        }
    }
    Ok(())
}
#[derive(Default)]
pub(super) struct Discovery {
    direct_attempts: HashMap<PeerId, u32>,
    registered: HashMap<(PeerId, String), Instant>,
    learned: HashMap<PeerId, Vec<Multiaddr>>,
    learned_endpoints: HashMap<PeerId, (Vec<Multiaddr>, u64, Instant)>,
    learned_endpoint_sequences: HashMap<PeerId, u64>,
    relay_listeners: HashSet<Multiaddr>,
}

impl Discovery {
    pub(super) fn network_changed(&mut self) {
        self.direct_attempts.clear();
    }

    pub(super) fn reload(&mut self) {
        self.registered.clear();
    }

    pub(super) fn tick(
        &mut self,
        swarm: &mut PeerSwarm,
        store: &Store,
        namespaces: &[(Multiaddr, String)],
        connections: &Connections,
    ) -> Result<()> {
        let now = Instant::now();
        self.learned_endpoints
            .retain(|_, (_, _, expiry)| *expiry > now);
        for card in store.peers(None)? {
            let peer: PeerId = card.peer_id.parse()?;
            let direct = connections.values().any(|(p, r, _)| *p == peer && !r);
            if direct {
                self.direct_attempts.remove(&peer);
                continue;
            }
            let attempts = self.direct_attempts.entry(peer).or_default();
            *attempts += 1;
            let mut addresses: Vec<Multiaddr> = card
                .addresses
                .iter()
                .filter_map(|a| a.parse().ok())
                .chain(self.learned.get(&peer).into_iter().flatten().cloned())
                .chain(
                    self.learned_endpoints
                        .get(&peer)
                        .into_iter()
                        .flat_map(|(addresses, _, _)| addresses.iter().cloned()),
                )
                .filter(|a: &Multiaddr| !a.iter().any(|p| matches!(p, Protocol::P2pCircuit)))
                .collect();
            addresses.sort_by_key(ToString::to_string);
            addresses.dedup();
            let has_quic = addresses
                .iter()
                .any(|a| a.iter().any(|p| matches!(p, Protocol::QuicV1)));
            // QUIC gets its own attempt; TCP is tried after a QUIC timeout, then circuits.
            let prefer_quic = has_quic && (*attempts == 1 || *attempts % 3 == 1);
            addresses.retain(|a| a.iter().any(|p| matches!(p, Protocol::QuicV1)) == prefer_quic);
            if !addresses.is_empty() {
                let _ = swarm.dial(
                    DialOpts::peer_id(peer)
                        .condition(PeerCondition::Always)
                        .addresses(addresses)
                        .build(),
                );
            }
            // Give direct dialing one full interval before the circuit fallback.
            if *attempts >= 3 && !swarm.is_connected(&peer) {
                for (relay, _) in namespaces {
                    let _ = swarm.dial(
                        relay
                            .clone()
                            .with(Protocol::P2pCircuit)
                            .with(Protocol::P2p(peer)),
                    );
                }
            }
        }
        for (address, namespace) in namespaces {
            let relay = infrastructure_id(address)?;
            if !swarm.is_connected(&relay) {
                let _ = swarm.dial(address.clone());
                continue;
            }
            if self.relay_listeners.insert(address.clone()) {
                if let Err(e) = swarm.listen_on(address.clone().with(Protocol::P2pCircuit)) {
                    self.relay_listeners.remove(address);
                    tracing::warn!(%e,"relay reservation failed");
                }
            }
            let ns = rendezvous::Namespace::new(namespace.clone())?;
            if self
                .registered
                .get(&(relay, namespace.clone()))
                .is_none_or(|renew| *renew <= Instant::now())
            {
                if let Err(e) = swarm
                    .behaviour_mut()
                    .rendezvous
                    .register(ns.clone(), relay, None)
                {
                    tracing::debug!(%e,"registration waiting for external address");
                }
            }
            swarm
                .behaviour_mut()
                .rendezvous
                .discover(Some(ns), None, Some(8), relay);
        }
        Ok(())
    }

    pub(super) fn learn_endpoint_addresses(
        &mut self,
        peer: PeerId,
        addresses: Vec<Multiaddr>,
        sequence: u64,
        expiry: Instant,
    ) {
        if self
            .learned_endpoint_sequences
            .get(&peer)
            .is_some_and(|current_sequence| *current_sequence > sequence)
        {
            return;
        }
        self.learned_endpoint_sequences.insert(peer, sequence);
        if expiry <= Instant::now() {
            self.learned_endpoints.remove(&peer);
        } else if addresses.is_empty() {
            self.learned_endpoints
                .insert(peer, (Vec::new(), sequence, expiry));
            self.direct_attempts.remove(&peer);
        } else {
            self.learned_endpoints
                .insert(peer, (addresses, sequence, expiry));
            self.direct_attempts.remove(&peer);
        }
    }

    pub(super) fn disconnected(&mut self, peer: PeerId, namespaces: &[(Multiaddr, String)]) {
        self.registered.retain(|(p, _), _| *p != peer);
        // Reservations are recreated after an infrastructure disconnect.
        if namespaces
            .iter()
            .any(|(a, _)| infrastructure_id(a).ok() == Some(peer))
        {
            self.relay_listeners
                .retain(|a| infrastructure_id(a).ok() != Some(peer));
        }
    }

    pub(super) fn identified(
        &mut self,
        swarm: &mut PeerSwarm,
        peer: PeerId,
        info: identify::Info,
        namespaces: &[(Multiaddr, String)],
    ) -> Option<Multiaddr> {
        let observed = if namespaces
            .iter()
            .any(|(a, _)| infrastructure_id(a).ok() == Some(peer))
        {
            Some(info.observed_addr)
        } else {
            None
        };
        let addresses = self.learned.entry(peer).or_default();
        for address in info.listen_addrs.into_iter().take(16) {
            if addresses.len() < 32 && !addresses.contains(&address) {
                addresses.push(address.clone());
            }
            swarm.add_peer_address(peer, address);
        }
        observed
    }

    pub(super) fn event(
        &mut self,
        swarm: &mut PeerSwarm,
        store: &Store,
        event: rendezvous::client::Event,
    ) -> Result<()> {
        match event {
            rendezvous::client::Event::Registered {
                rendezvous_node,
                namespace,
                ttl,
            } => {
                self.registered.insert(
                    (rendezvous_node, namespace.to_string()),
                    Instant::now() + Duration::from_secs(ttl.saturating_mul(2) / 3),
                );
            }
            rendezvous::client::Event::Expired { peer, .. } => {
                self.learned.remove(&peer);
            }
            rendezvous::client::Event::Discovered { registrations, .. } => {
                let allowed: HashSet<_> =
                    store.peers(None)?.into_iter().map(|c| c.peer_id).collect();
                for registration in registrations {
                    let peer = registration.record.peer_id();
                    if !allowed.contains(&peer.to_string()) {
                        continue;
                    }
                    let mut addresses = registration.record.addresses().to_vec();
                    addresses.truncate(16);
                    let known = self.learned.entry(peer).or_default();
                    let mut changed = false;
                    for address in &addresses {
                        if known.len() < 32 && !known.contains(address) {
                            known.push(address.clone());
                            changed = true;
                        }
                    }
                    for address in &addresses {
                        swarm.add_peer_address(peer, address.clone());
                    }
                    if changed {
                        self.direct_attempts.remove(&peer);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}
