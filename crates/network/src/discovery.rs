//! Private discovery and retry ordering: QUIC, then TCP. Never dial public peers.
use crate::{
    behaviour::{Behaviour, PeerSwarm},
    status::Connections,
    validate_card,
};
use anyhow::Result;
use libp2p::{
    identify,
    multiaddr::Protocol,
    swarm::dial_opts::{DialOpts, PeerCondition},
    Multiaddr, PeerId,
};
use std::{
    collections::{HashMap, HashSet},
    time::Instant,
};
use storage::Store;
pub(super) fn configure(swarm: &mut libp2p::Swarm<Behaviour>, store: &Store) -> Result<()> {
    let mut allowed = HashSet::new();
    for card in store.peers(None)? {
        validate_card(&card)?;
        let peer: PeerId = card.peer_id.parse()?;
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
    learned: HashMap<PeerId, Vec<Multiaddr>>,
    learned_endpoints: HashMap<PeerId, (Vec<Multiaddr>, u64, Instant)>,
    learned_endpoint_sequences: HashMap<PeerId, u64>,
}

impl Discovery {
    pub(super) fn network_changed(&mut self) {
        self.direct_attempts.clear();
    }

    pub(super) fn tick(
        &mut self,
        swarm: &mut PeerSwarm,
        store: &Store,
        connections: &Connections,
    ) -> Result<()> {
        let now = Instant::now();
        self.learned_endpoints
            .retain(|_, (_, _, expiry)| *expiry > now);
        for card in store.peers(None)? {
            let peer: PeerId = card.peer_id.parse()?;
            if connections.values().any(|(p, _)| *p == peer) {
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
            // QUIC gets its own attempt; TCP is tried after a QUIC timeout.
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

    pub(super) fn identified(&mut self, swarm: &mut PeerSwarm, peer: PeerId, info: identify::Info) {
        let addresses = self.learned.entry(peer).or_default();
        for address in info.listen_addrs.into_iter().take(16) {
            if addresses.len() < 32 && !addresses.contains(&address) {
                addresses.push(address.clone());
            }
            swarm.add_peer_address(peer, address);
        }
    }
}
