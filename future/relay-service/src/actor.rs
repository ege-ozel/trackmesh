use crate::behaviour::Behaviour;
use anyhow::{ensure, Result};
use futures::StreamExt;
use libp2p::{swarm::SwarmEvent, PeerId, Swarm};
use protocol::DeviceCard;
use std::{
    collections::{HashMap, HashSet},
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

pub fn read_allowed(path: &Path) -> Result<HashSet<PeerId>> {
    let mut data = Vec::new();
    std::fs::File::open(path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut data)?;
    ensure!(data.len() <= 1024 * 1024, "allow-list too large");
    let cards: Vec<DeviceCard> = serde_json::from_slice(&data)?;
    ensure!(
        cards.len() <= 3,
        "use a private relay instance for each collaboration group of at most three devices"
    );
    let mut bindings = HashMap::new();
    let mut peers = HashSet::new();
    for card in cards {
        network::validate_card(&card)?;
        ensure!(
            bindings.insert(card.device_id, card.public_key).is_none(),
            "duplicate device identity"
        );
        ensure!(
            peers.insert(card.peer_id.parse()?),
            "duplicate peer identity"
        );
    }
    Ok(peers)
}

pub async fn run(
    mut swarm: Swarm<Behaviour>,
    allow_file: PathBuf,
    mut allowed: HashSet<PeerId>,
) -> Result<()> {
    let mut refresh = tokio::time::interval(Duration::from_secs(6));
    refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut reload: Option<tokio::task::JoinHandle<Result<HashSet<PeerId>>>> = None;
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => return Ok(()),
            _ = refresh.tick(), if reload.is_none() => {
                let path = allow_file.clone();
                reload = Some(tokio::task::spawn_blocking(move || read_allowed(&path)));
            },
            result = async { reload.as_mut().expect("guarded reload").await }, if reload.is_some() => {
                reload = None;
                let next = match result {
                    Ok(Ok(peers)) => peers,
                    _ => { eprintln!("Invalid or unavailable allow-list; all client access disabled"); HashSet::new() },
                };
                for peer in allowed.difference(&next) {
                    swarm.behaviour_mut().allow.disallow_peer(*peer);
                    let _ = swarm.disconnect_peer_id(*peer);
                }
                for peer in next.difference(&allowed) { swarm.behaviour_mut().allow.allow_peer(*peer); }
                allowed = next;
            },
            event = swarm.select_next_some() => match event {
                SwarmEvent::NewListenAddr { address, .. } => println!("Listening: {address}/p2p/{}", swarm.local_peer_id()),
                SwarmEvent::ListenerError { .. } => eprintln!("Relay listener error"),
                SwarmEvent::ListenerClosed { .. } => anyhow::bail!("relay listener closed"),
                _ => {},
            },
        }
    }
}
