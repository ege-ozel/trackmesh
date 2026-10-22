//! Network startup settings.
use libp2p::Multiaddr;
#[derive(Default)]
pub struct NetworkConfig {
    pub port: u16,
    pub external_addresses: Vec<Multiaddr>,
}
