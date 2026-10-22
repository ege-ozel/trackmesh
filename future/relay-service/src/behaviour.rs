use anyhow::Result;
use crypto::Identity;
use libp2p::{
    allow_block_list, connection_limits, identify, noise, ping, relay, rendezvous,
    swarm::NetworkBehaviour, tcp, yamux, SwarmBuilder,
};
use std::time::Duration;

#[derive(NetworkBehaviour)]
pub struct Behaviour {
    pub allow: allow_block_list::Behaviour<allow_block_list::AllowedPeers>,
    limits: connection_limits::Behaviour,
    identify: identify::Behaviour,
    ping: ping::Behaviour,
    relay: relay::Behaviour,
    rendezvous: rendezvous::server::Behaviour,
}

pub fn build(identity: &Identity) -> Result<libp2p::Swarm<Behaviour>> {
    let key = network::keypair(identity)?;
    let id = key.public().to_peer_id();
    Ok(SwarmBuilder::with_existing_identity(key)
        .with_tokio()
        .with_tcp(
            tcp::Config::default().nodelay(true),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_quic()
        .with_dns()?
        .with_behaviour(|key| Behaviour {
            allow: Default::default(),
            limits: connection_limits::Behaviour::new(
                connection_limits::ConnectionLimits::default()
                    .with_max_pending_incoming(Some(16))
                    .with_max_established_incoming(Some(128))
                    .with_max_established_per_peer(Some(3)),
            ),
            identify: identify::Behaviour::new(identify::Config::new(
                "/trackmesh/1".into(),
                key.public(),
            )),
            ping: ping::Behaviour::new(ping::Config::new()),
            relay: relay::Behaviour::new(
                id,
                relay::Config {
                    max_reservations: 128,
                    max_reservations_per_peer: 2,
                    max_circuits: 128,
                    max_circuits_per_peer: 16,
                    max_circuit_bytes: 256 * 1024 * 1024,
                    max_circuit_duration: Duration::from_secs(120),
                    ..Default::default()
                },
            ),
            rendezvous: rendezvous::server::Behaviour::new(
                rendezvous::server::Config::default()
                    .with_max_registration_per_peer(16)
                    .with_max_registration_total(1024)
                    .with_max_stored_cookies(1024)
                    .with_min_ttl(30)
                    .with_max_ttl(3600),
            ),
        })?
        .with_swarm_config(|config| config.with_idle_connection_timeout(Duration::from_secs(120)))
        .build())
}
