//! Private authenticated networking. `Network` is a bounded handle; `actor` owns the swarm.
mod actor;
mod behaviour;
mod codec;
mod config;
mod discovery;
mod gate;
mod handle;
mod identity;
mod portmap;
mod status;

pub use actor::{is_public_endpoint_address, start};
pub use config::NetworkConfig;
pub use handle::{Incoming, IncomingRequest, Network};
pub use identity::{keypair, peer_id, validate_card, validate_endpoint_record};
pub use portmap::{PortMappingRuntime, UpnpTarget};
pub use status::{
    ConnectionStatus, NetworkStatus, PortMappingStatus, ReachabilityStatus, ReceivedSnapshot,
};
