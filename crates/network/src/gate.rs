//! Normal allow-list, with a short-lived inbound-only enrollment exception.
use libp2p::{
    allow_block_list,
    core::{transport::PortUse, Endpoint},
    swarm::{
        dummy, ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandler,
        THandlerInEvent, THandlerOutEvent, ToSwarm,
    },
    Multiaddr, PeerId,
};
use std::{
    collections::HashMap,
    convert::Infallible,
    task::{Context, Poll},
    time::{Duration, Instant},
};

#[derive(Default)]
pub(super) struct Gate {
    allow: allow_block_list::Behaviour<allow_block_list::AllowedPeers>,
    until: Option<Instant>,
    temporary: HashMap<ConnectionId, (PeerId, Instant)>,
}

impl Gate {
    pub(super) fn allow_peer(&mut self, peer: PeerId) {
        self.temporary.retain(|_, (p, _)| *p != peer);
        self.allow.allow_peer(peer);
    }
    pub(super) fn disallow_peer(&mut self, peer: PeerId) {
        self.allow.disallow_peer(peer);
    }
    pub(super) fn allowed_peers(&self) -> &std::collections::HashSet<PeerId> {
        self.allow.allowed_peers()
    }
    pub(super) fn window(&mut self, duration: Duration) {
        self.until =
            (!duration.is_zero()).then(|| Instant::now() + duration.min(Duration::from_secs(900)));
    }
    pub(super) fn is_open(&self) -> bool {
        self.until.is_some_and(|until| until > Instant::now())
    }
    pub(super) fn expired_peers(&self) -> Vec<PeerId> {
        self.temporary
            .values()
            .filter_map(|(peer, deadline)| {
                (!self.allowed_peers().contains(peer)
                    && (!self.is_open() || *deadline <= Instant::now()))
                .then_some(*peer)
            })
            .collect()
    }
}

impl NetworkBehaviour for Gate {
    type ConnectionHandler = dummy::ConnectionHandler;
    type ToSwarm = Infallible;
    fn handle_established_inbound_connection(
        &mut self,
        id: ConnectionId,
        peer: PeerId,
        local: &Multiaddr,
        remote: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        if self.allowed_peers().contains(&peer) {
            return self
                .allow
                .handle_established_inbound_connection(id, peer, local, remote);
        }
        if self.is_open()
            && self.temporary.len() < 4
            && !self.temporary.values().any(|(p, _)| *p == peer)
        {
            self.temporary
                .insert(id, (peer, Instant::now() + Duration::from_secs(60)));
            return Ok(dummy::ConnectionHandler);
        }
        self.allow
            .handle_established_inbound_connection(id, peer, local, remote)
    }
    fn handle_pending_outbound_connection(
        &mut self,
        id: ConnectionId,
        peer: Option<PeerId>,
        addresses: &[Multiaddr],
        role: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        self.allow
            .handle_pending_outbound_connection(id, peer, addresses, role)
    }
    fn handle_established_outbound_connection(
        &mut self,
        id: ConnectionId,
        peer: PeerId,
        address: &Multiaddr,
        role: Endpoint,
        port: PortUse,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.allow
            .handle_established_outbound_connection(id, peer, address, role, port)
    }
    fn on_swarm_event(&mut self, event: FromSwarm) {
        match &event {
            FromSwarm::ConnectionClosed(e) => {
                self.temporary.remove(&e.connection_id);
            }
            FromSwarm::ListenFailure(e) => {
                self.temporary.remove(&e.connection_id);
            }
            _ => {}
        }
        self.allow.on_swarm_event(event);
    }
    fn on_connection_handler_event(
        &mut self,
        peer: PeerId,
        id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        self.allow.on_connection_handler_event(peer, id, event);
    }
    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        self.allow.poll(cx)
    }
}
