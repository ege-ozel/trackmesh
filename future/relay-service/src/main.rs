mod actor;
mod behaviour;

use anyhow::Result;
use clap::Parser;
use crypto::Identity;
use libp2p::Multiaddr;
use std::{net::Ipv4Addr, path::PathBuf};

#[derive(Parser)]
#[command(about = "Allow-listed TrackMesh circuit relay and private rendezvous")]
struct Cli {
    #[arg(long)]
    data_dir: PathBuf,
    /// JSON array of authorized signed public device cards.
    #[arg(long)]
    allow_file: PathBuf,
    #[arg(long, default_value = "0.0.0.0")]
    listen_ip: Ipv4Addr,
    #[arg(long, default_value_t = 4001)]
    port: u16,
    #[arg(long)]
    external_address: Vec<Multiaddr>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let allowed = actor::read_allowed(&cli.allow_file)?;
    let identity = Identity::load_or_create(&cli.data_dir.join("secrets"), "TrackMesh relay")?;
    let mut swarm = behaviour::build(&identity)?;
    for peer in &allowed {
        swarm.behaviour_mut().allow.allow_peer(*peer);
    }
    swarm.listen_on(format!("/ip4/{}/udp/{}/quic-v1", cli.listen_ip, cli.port).parse()?)?;
    swarm.listen_on(format!("/ip4/{}/tcp/{}", cli.listen_ip, cli.port).parse()?)?;
    for address in cli.external_address {
        swarm.add_external_address(address);
    }
    println!("Relay PeerId: {}", swarm.local_peer_id());
    actor::run(swarm, cli.allow_file, allowed).await
}
