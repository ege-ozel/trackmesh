//! Transport/behaviour construction. Keep protocol names, limits, and transport security here.
use crate::{codec, discovery::configure, identity::keypair, NetworkConfig};
use anyhow::Result;
use crypto::Identity;
use libp2p::{
    identify, noise, ping, request_response, swarm::NetworkBehaviour, tcp, yamux, StreamProtocol,
    SwarmBuilder,
};
use std::time::Duration;
use storage::Store;

pub(super) type PeerSwarm = libp2p::Swarm<Behaviour>;
#[derive(NetworkBehaviour)]
pub(super) struct Behaviour {
    pub(super) allow: crate::gate::Gate,
    pub(super) limits: libp2p::connection_limits::Behaviour,
    pub(super) identify: identify::Behaviour,
    pub(super) ping: ping::Behaviour,
    pub(super) transfer: request_response::Behaviour<codec::TransferCodec>,
    pub(super) pairing: request_response::Behaviour<codec::PairingCodec>,
}
pub(super) fn build_swarm(
    identity: &Identity,
    store: &Store,
    config: &NetworkConfig,
) -> Result<PeerSwarm> {
    let key = keypair(identity)?;
    let mut swarm = SwarmBuilder::with_existing_identity(key)
        .with_tokio()
        .with_tcp(
            tcp::Config::default().nodelay(true),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_quic_config(|mut c| {
            c.max_connection_data = 64 * 1024 * 1024;
            c.max_stream_data = 16 * 1024 * 1024;
            c.max_concurrent_stream_limit = 32;
            c
        })
        .with_dns()?
        .with_behaviour(|key| Behaviour {
            allow: Default::default(),
            limits: libp2p::connection_limits::Behaviour::new(
                libp2p::connection_limits::ConnectionLimits::default()
                    .with_max_pending_incoming(Some(8))
                    .with_max_pending_outgoing(Some(12))
                    .with_max_established_incoming(Some(12))
                    .with_max_established_outgoing(Some(12))
                    .with_max_established_per_peer(Some(3)),
            ),
            identify: identify::Behaviour::new(
                identify::Config::new("/trackmesh/1".into(), key.public())
                    .with_interval(Duration::from_secs(30)),
            ),
            ping: ping::Behaviour::new(
                ping::Config::new()
                    .with_interval(Duration::from_secs(10))
                    .with_timeout(Duration::from_secs(10)),
            ),
            pairing: request_response::Behaviour::with_codec(
                codec::PairingCodec,
                [(
                    StreamProtocol::new(protocol::PAIRING_PROTOCOL),
                    request_response::ProtocolSupport::Full,
                )],
                request_response::Config::default()
                    .with_request_timeout(Duration::from_secs(20))
                    .with_max_concurrent_streams(8),
            ),
            transfer: request_response::Behaviour::with_codec(
                codec::TransferCodec,
                [(
                    StreamProtocol::new("/trackmesh/transfer/1"),
                    request_response::ProtocolSupport::Full,
                )],
                request_response::Config::default()
                    .with_request_timeout(Duration::from_secs(75))
                    .with_max_concurrent_streams(24),
            ),
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(120)))
        .build();
    configure(&mut swarm, store)?;
    swarm.listen_on(format!("/ip4/0.0.0.0/udp/{}/quic-v1", config.port).parse()?)?;
    swarm.listen_on(format!("/ip4/0.0.0.0/tcp/{}", config.port).parse()?)?;
    for address in &config.external_addresses {
        swarm.add_external_address(address.clone());
    }
    Ok(swarm)
}
