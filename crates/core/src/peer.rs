use crate::Engine;
use anyhow::{ensure, Context, Result};
use network::{Network, NetworkConfig};
use network::{PortMappingRuntime, UpnpTarget};
use protocol::{PeerRequest, PeerResponse, SignedSnapshot};
use std::{
    collections::HashSet,
    net::{IpAddr, Ipv4Addr},
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, RwLock,
    },
    time::Duration,
};
use tokio::task::{JoinHandle, JoinSet};
use uuid::Uuid;

pub struct PeerRuntime {
    pub network: Network,
    actor: JoinHandle<Result<()>>,
    responder: JoinHandle<Result<()>>,
    // Windows closes this exclusive handle only after this runtime is drained.
    _lease: Arc<std::fs::File>,
    activity: Arc<ResponderActivity>,
    mapping_task: Option<(tokio::sync::watch::Sender<bool>, JoinHandle<()>)>,
    observation: Arc<RwLock<crate::NetworkObservation>>,
    monitor: Option<JoinHandle<()>>,
    monitor_cancel: Option<tokio::sync::watch::Sender<bool>>,
    endpoint_publisher: JoinHandle<()>,
    endpoint_publisher_cancel: tokio::sync::watch::Sender<bool>,
    supabase: Option<crate::SupabaseRuntime>,
    online_pull: JoinHandle<()>,
    online_pull_cancel: tokio::sync::watch::Sender<bool>,
    presence: JoinHandle<()>,
    presence_cancel: tokio::sync::watch::Sender<bool>,
}
#[derive(Default)]
struct ResponderActivity {
    active: AtomicUsize,
    epoch: AtomicU64,
}
struct ResponderWork(Arc<ResponderActivity>);
impl Drop for ResponderWork {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
}
impl PeerRuntime {
    pub fn start(
        engine: Arc<Engine>,
        config: NetworkConfig,
        observation: crate::NetworkObservation,
        start_mapping: bool,
    ) -> Result<Self> {
        let lease = Arc::new(runtime_lease(&engine.directory)?);
        let (network, mut incoming, actor) =
            network::start(engine.identity.clone(), engine.store.clone(), config)?;
        let initial_target = upnp_target(&observation);
        let observation = Arc::new(RwLock::new(observation.clone()));
        let (endpoint_publisher_cancel, endpoint_publisher_rx) = tokio::sync::watch::channel(false);
        let endpoint_publisher = tokio::spawn(crate::endpoints::run_endpoint_publisher(
            engine.clone(),
            network.clone(),
            observation.clone(),
            endpoint_publisher_rx,
            // Short-lived CLI commands may share this device identity; only the
            // long-lived runtime may advertise its listeners as the device endpoint.
            start_mapping,
        ));
        let supabase =
            start_mapping.then(|| crate::SupabaseRuntime::start(engine.clone(), network.clone()));
        let (online_pull_cancel, online_pull_rx) = tokio::sync::watch::channel(false);
        let online_pull =
            spawn_online_pull_monitor(engine.clone(), network.clone(), online_pull_rx);
        let (presence_cancel, presence_rx) = tokio::sync::watch::channel(false);
        let presence = tokio::spawn(crate::presence::run(
            engine.clone(),
            network.clone(),
            presence_rx,
        ));
        if start_mapping {
            network.set_mapping_consent(engine.upnp_mapping_enabled());
        }
        let (mapping_updates, _) = tokio::sync::watch::channel((0u64, initial_target));
        let mapping_task = if start_mapping {
            let mut updates = mapping_updates.subscribe();
            let network = network.clone();
            let device_id = engine.identity.device_id.to_string();
            let mut mapping =
                PortMappingRuntime::start(network.clone(), initial_target, device_id.clone());
            let (shutdown, mut shutdown_rx) = tokio::sync::watch::channel(false);
            let task = tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = shutdown_rx.changed() => {
                            mapping.stop(&network).await;
                            return;
                        }
                        changed = updates.changed() => {
                            if changed.is_err() { mapping.stop(&network).await; return; }
                            mapping.stop_for_target_change(&network).await;
                            if *shutdown_rx.borrow() { return; }
                            let (_, target) = *updates.borrow_and_update();
                            mapping = PortMappingRuntime::start(network.clone(), target, device_id.clone());
                        }
                    }
                }
            });
            Some((shutdown, task))
        } else {
            None
        };
        let (monitor_cancel, monitor) = if start_mapping {
            let (cancel, mut cancelled) = tokio::sync::watch::channel(false);
            let monitor_observation = observation.clone();
            let monitor_network = network.clone();
            let updates = mapping_updates;
            let initial_observation = observation
                .read()
                .map(|value| value.clone())
                .unwrap_or_default();
            let monitor = tokio::spawn(async move {
                let mut previous = observation_signature(&initial_observation);
                let mut previous_listeners =
                    listener_signature(&monitor_network.status().addresses);
                let mut current_observation = initial_observation;
                let mut generation = 0u64;
                loop {
                    tokio::select! {
                        _ = cancelled.changed() => return,
                        _ = tokio::time::sleep(Duration::from_secs(10)) => {}
                    }
                    let observed = match tokio::task::spawn_blocking(crate::observe_network).await {
                        Ok(observed) => observed,
                        Err(error) => {
                            tracing::warn!(%error, "network observation worker failed");
                            continue;
                        }
                    };
                    let signature = observation_signature(&observed);
                    let listeners = listener_signature(&monitor_network.status().addresses);
                    let observation_changed = signature != previous;
                    let listeners_changed = listeners != previous_listeners;
                    if !observation_changed && !listeners_changed {
                        continue;
                    }
                    if observation_changed {
                        let notified = tokio::select! {
                            _ = cancelled.changed() => return,
                            result = tokio::time::timeout(Duration::from_secs(2), monitor_network.network_changed()) => result,
                        };
                        match notified {
                            Ok(Ok(())) => {}
                            Ok(Err(error)) => {
                                tracing::debug!(%error, "network actor is unavailable for network-change notification");
                                return;
                            }
                            Err(_) => {
                                tracing::warn!("network-change command queue remained full; observation will be retried");
                                continue;
                            }
                        }
                        current_observation = observed.clone();
                        if let Ok(mut current) = monitor_observation.write() {
                            *current = observed.clone();
                        }
                    }
                    generation = generation.wrapping_add(1);
                    previous = signature;
                    previous_listeners = listeners;
                    let _ = updates.send((generation, upnp_target(&current_observation)));
                }
            });
            (Some(cancel), Some(monitor))
        } else {
            (None, None)
        };
        // The lease outlives an accidentally dropped adapter handle as well.
        let actor_lease = lease.clone();
        let actor = tokio::spawn(async move {
            let _lease = actor_lease;
            actor.await.context("network actor panicked")?
        });
        let network_copy = network.clone();
        let responder_lease = lease.clone();
        let activity = Arc::new(ResponderActivity::default());
        let responder_activity = activity.clone();
        let responder = tokio::spawn(async move {
            let _lease = responder_lease;
            let mut workers = JoinSet::new();
            let mut notice_pulls = std::collections::HashMap::new();
            let mut closed = false;
            loop {
                if closed && workers.is_empty() {
                    break;
                }
                tokio::select! {
                    message=incoming.recv(),if !closed && workers.len()<8=>{
                        let Some(message)=message else {closed=true;continue;};
                        let pull_notice = match &message.request {
                            network::IncomingRequest::Transfer(protocol::PeerRequest::SnapshotAvailable { project_id, .. }) => {
                                let key = (message.peer.to_string(), *project_id);
                                notice_pulls.retain(|_, at: &mut std::time::Instant| at.elapsed() < Duration::from_secs(5));
                                let allowed = !notice_pulls.contains_key(&key) && notice_pulls.len() < 512;
                                if allowed { notice_pulls.insert(key, std::time::Instant::now()); }
                                allowed
                            }
                            _ => false,
                        };
                        // Advisory refreshes must not keep a CLI-only owner alive.
                        let work = if matches!(&message.request, network::IncomingRequest::Transfer(protocol::PeerRequest::Presence { .. })) {
                            None
                        } else {
                            responder_activity.active.fetch_add(1, Ordering::SeqCst);
                            responder_activity.epoch.fetch_add(1, Ordering::SeqCst);
                            Some(ResponderWork(responder_activity.clone()))
                        };
                        let engine=engine.clone();let network=network_copy.clone();
                        workers.spawn(async move {
                            let _work = work;
                            let peer=message.peer.to_string();let request=message.request.clone();
                            match request {
                                network::IncomingRequest::EndpointUpdate { project_id, record } => {
                                    let response = match engine.apply_endpoint_record(&network, project_id, peer.clone(), record).await {
                                        Ok(_) => PeerResponse::EndpointUpdated,
                                        Err(_) => PeerResponse::Error { message: "endpoint update rejected".into() },
                                    };
                                    let _ = network.respond(message, response).await;
                                }
                                network::IncomingRequest::Transfer(request) => {
                                    if matches!(&request, protocol::PeerRequest::Presence { .. }) {
                                        let status_network = network.clone();
                                        let response = tokio::task::spawn_blocking(move || engine.accept_presence(&status_network, &peer, &request)).await;
                                        let response = match response {
                                            Ok(Ok(response)) => response,
                                            _ => PeerResponse::Error { message: "presence rejected".into() },
                                        };
                                        let _ = network.respond(message, response).await;
                                        return;
                                    }
                                    if let protocol::PeerRequest::SnapshotAvailable { project_id, .. } = &request {
                                        let project_id = *project_id;
                                        let store = engine.store.clone();
                                        let auth_peer = peer.clone();
                                        let authorized = tokio::task::spawn_blocking(move || store.authorized(project_id, &auth_peer).map(|record| record.is_some()))
                                            .await
                                            .ok()
                                            .and_then(Result::ok)
                                            .unwrap_or(false);
                                        let response = if authorized { PeerResponse::SnapshotNoticed } else { PeerResponse::Error { message: "project access denied".into() } };
                                        let _ = network.respond(message, response).await;
                                        if authorized && pull_notice {
                                            let _ = tokio::time::timeout(Duration::from_secs(30), engine.fetch_snapshot_chain(&network, project_id, &peer, None)).await;
                                        }
                                        return;
                                    }
                                    let receipt = match &request {
                                        protocol::PeerRequest::Verified { project_id, snapshot_id, .. } => Some((*project_id, *snapshot_id)),
                                        _ => None,
                                    };
                                    let receipt_peer = peer.clone();
                                    let response=tokio::task::spawn_blocking(move||transfer::serve(&engine.store,&engine.chunks,&peer,&request)).await;
                                    let response=match response {Ok(Ok(r))=>r,Ok(Err(e))=>PeerResponse::Error {message:e.to_string()},Err(_)=>PeerResponse::Error {message:"peer disk operation failed".into()}};
                                    if matches!(&response, PeerResponse::Verified { equal: true }) {
                                        if let Some((project_id, snapshot_id)) = receipt {
                                            network.record_received_snapshot(receipt_peer, project_id, snapshot_id);
                                        }
                                    }
                                    let _=network.respond(message,response).await;
                                }
                                network::IncomingRequest::Pairing(request) => {
                                    let response = tokio::task::spawn_blocking(move || engine.handle_pairing(&peer, request)).await.unwrap_or(protocol::PairingResponse::Unavailable);
                                    let _ = network.respond_pairing(message, response).await;
                                }
                            }
                        });
                    },
                    result=workers.join_next(),if !workers.is_empty()=>{if let Some(Err(error))=result {tracing::warn!(%error,"peer responder worker stopped");}},
                }
            }
            Ok(())
        });
        Ok(Self {
            network,
            actor,
            responder,
            _lease: lease,
            activity,
            observation,
            monitor,
            monitor_cancel,
            endpoint_publisher,
            endpoint_publisher_cancel,
            supabase,
            online_pull,
            online_pull_cancel,
            mapping_task,
            presence,
            presence_cancel,
        })
    }
    pub fn is_finished(&self) -> bool {
        // A failed Supabase worker is reported as UNREACHABLE; it must not end direct sessions.
        self.actor.is_finished()
    }
    pub fn network_observation(&self) -> crate::NetworkObservation {
        self.observation
            .read()
            .map(|value| value.clone())
            .unwrap_or_default()
    }
    pub fn idle_epoch(&self) -> Option<u64> {
        let epoch = self.activity.epoch.load(Ordering::SeqCst);
        (self.activity.active.load(Ordering::SeqCst) == 0).then_some(epoch)
    }
    pub async fn wait_for_peer(&self, peer: &str, timeout: Duration) -> Result<()> {
        let deadline = tokio::time::Instant::now() + timeout;
        while !self.network.online(peer) {
            ensure!(
                !self.actor.is_finished(),
                "network actor stopped before connecting"
            );
            ensure!(
                tokio::time::Instant::now() < deadline,
                "source peer is offline or unreachable; no files can be downloaded"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Ok(())
    }
    pub async fn stop(self) -> Result<()> {
        self.presence_cancel.send_replace(true);
        self.presence.await.context("presence worker panicked")?;
        let supabase_result = if let Some(supabase) = self.supabase {
            supabase.stop().await
        } else {
            Ok(())
        };
        self.endpoint_publisher_cancel.send_replace(true);
        let mut endpoint_publisher = self.endpoint_publisher;
        if tokio::time::timeout(Duration::from_secs(15), &mut endpoint_publisher)
            .await
            .is_err()
        {
            endpoint_publisher.abort();
            let _ = tokio::time::timeout(Duration::from_secs(1), &mut endpoint_publisher).await;
        }
        self.online_pull_cancel.send_replace(true);
        let mut online_pull = self.online_pull;
        if tokio::time::timeout(Duration::from_secs(15), &mut online_pull)
            .await
            .is_err()
        {
            online_pull.abort();
            let _ = tokio::time::timeout(Duration::from_secs(1), &mut online_pull).await;
        }
        if let Some(cancel) = self.monitor_cancel {
            cancel.send_replace(true);
        }
        if let Some(mut monitor) = self.monitor {
            if tokio::time::timeout(Duration::from_secs(15), &mut monitor)
                .await
                .is_err()
            {
                monitor.abort();
                let _ = monitor.await;
            }
        }
        if let Some((shutdown, mut task)) = self.mapping_task {
            shutdown.send_replace(true);
            if tokio::time::timeout(Duration::from_secs(20), &mut task)
                .await
                .is_err()
            {
                task.abort();
                let _ = tokio::time::timeout(Duration::from_secs(1), &mut task).await;
            }
        }
        if tokio::time::timeout(Duration::from_secs(5), self.network.shutdown())
            .await
            .is_err()
        {
            tracing::error!("Network shutdown request timed out");
        }
        let mut actor = self.actor;
        let actor_result = match tokio::time::timeout(Duration::from_secs(15), &mut actor).await {
            Ok(result) => Some(result.context("network actor panicked")?),
            Err(_) => {
                actor.abort();
                let _ = tokio::time::timeout(Duration::from_secs(1), &mut actor).await;
                None
            }
        };
        let mut responder = self.responder;
        match tokio::time::timeout(Duration::from_secs(20), &mut responder).await {
            Ok(result) => result.context("peer responder panicked")??,
            Err(_) => {
                responder.abort();
                let _ = tokio::time::timeout(Duration::from_secs(1), &mut responder).await;
                anyhow::bail!("Peer responder did not stop within 20 seconds");
            }
        }
        supabase_result?;
        if let Some(actor) = actor_result {
            actor
        } else {
            anyhow::bail!("Network actor did not stop within 15 seconds")
        }
    }
}

type NetworkSignature = (Vec<(u32, Vec<Ipv4Addr>)>, Vec<(u32, Ipv4Addr)>);

fn listener_signature(addresses: &[String]) -> Vec<String> {
    let mut listeners = addresses.to_vec();
    listeners.sort();
    listeners.dedup();
    listeners
}

fn observation_signature(observation: &crate::NetworkObservation) -> NetworkSignature {
    let mut interfaces = observation
        .interfaces
        .iter()
        .map(|interface| {
            let mut addresses = interface
                .ipv4
                .iter()
                .filter_map(|address| address.parse::<Ipv4Addr>().ok())
                .collect::<Vec<_>>();
            addresses.sort_unstable();
            addresses.dedup();
            (interface.interface_index, addresses)
        })
        .collect::<Vec<_>>();
    interfaces.sort_unstable();
    interfaces.dedup();
    let mut gateways = observation
        .default_gateways
        .iter()
        .filter_map(|gateway| {
            gateway
                .address
                .parse::<Ipv4Addr>()
                .ok()
                .map(|address| (gateway.interface_index, address))
        })
        .collect::<Vec<_>>();
    gateways.sort_unstable();
    gateways.dedup();
    (interfaces, gateways)
}

fn upnp_target(observation: &crate::NetworkObservation) -> Option<UpnpTarget> {
    let mut routes = observation.default_gateways.iter().collect::<Vec<_>>();
    routes.sort_by_key(|route| (route.interface_index, route.address.clone()));
    routes.dedup_by_key(|route| (route.interface_index, route.address.clone()));
    let [route] = routes.as_slice() else {
        return None;
    };
    let gateway = route.address.parse::<IpAddr>().ok()?;
    let interface = observation
        .interfaces
        .iter()
        .find(|item| item.interface_index == route.interface_index)?;
    let mut addresses = interface
        .ipv4
        .iter()
        .filter_map(|value| value.parse::<Ipv4Addr>().ok())
        .filter(|ip| {
            !ip.is_unspecified() && !ip.is_loopback() && !ip.is_link_local() && !ip.is_multicast()
        })
        .collect::<Vec<_>>();
    addresses.sort_unstable();
    addresses.dedup();
    let mut addresses = addresses.into_iter();
    let local = addresses.next()?;
    addresses
        .next()
        .is_none()
        .then_some(UpnpTarget { gateway, local })
}

fn runtime_lease(directory: &std::path::Path) -> Result<std::fs::File> {
    let locks = directory.join("locks");
    std::fs::create_dir_all(&locks)?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    #[cfg(not(windows))]
    anyhow::bail!("peer runtime ownership requires Windows");
    options
        .open(locks.join("peer-runtime.lock"))
        .context("another TrackMesh peer runtime already owns this data directory")
}

fn spawn_online_pull_monitor(
    engine: Arc<Engine>,
    network: Network,
    mut cancelled: tokio::sync::watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut previously_online = std::collections::HashSet::new();
        let mut pulls = JoinSet::new();
        loop {
            tokio::select! {
                _ = cancelled.changed() => return,
                _ = tokio::time::sleep(Duration::from_millis(250)) => {}
            }
            let online = network
                .status()
                .peers
                .into_iter()
                .filter_map(|(peer, status)| status.online.then_some(peer))
                .collect::<std::collections::HashSet<_>>();
            let newly_online = online
                .difference(&previously_online)
                .cloned()
                .collect::<Vec<_>>();
            previously_online = online;
            for peer_id in newly_online {
                while pulls.len() >= 8 {
                    let _ = pulls.join_next().await;
                }
                let engine = engine.clone();
                let network = network.clone();
                pulls.spawn(async move {
                    let disk = engine.clone();
                    let peer_for_query = peer_id.clone();
                    let projects = tokio::task::spawn_blocking(move || -> Result<Vec<Uuid>> {
                        let mut authorized = Vec::new();
                        for project in disk.store.projects()? {
                            if disk
                                .store
                                .peers(Some(project.project_id))?
                                .iter()
                                .any(|peer| peer.peer_id == peer_for_query)
                            {
                                authorized.push(project.project_id);
                            }
                        }
                        Ok(authorized)
                    })
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .unwrap_or_default();
                    for project in projects {
                        let _ = tokio::time::timeout(
                            Duration::from_secs(30),
                            engine.fetch_snapshot_chain(&network, project, &peer_id, None),
                        )
                        .await;
                    }
                });
            }
        }
    })
}

impl Engine {
    pub async fn fetch_snapshot_chain(
        &self,
        network: &Network,
        project: Uuid,
        source: &str,
        target: Option<Uuid>,
    ) -> Result<Option<SignedSnapshot>> {
        let source_device = self
            .store
            .authorized(project, source)?
            .context("peer is not paired with this project")?
            .device_id;
        let response = network
            .request(
                source,
                if let Some(snapshot_id) = target {
                    PeerRequest::Manifest {
                        project_id: project,
                        snapshot_id,
                    }
                } else {
                    PeerRequest::Head {
                        project_id: project,
                    }
                },
            )
            .await?;
        let snapshot = match response {
            PeerResponse::Head { snapshot: None } if target.is_none() => {
                self.peer_heads
                    .lock()
                    .map_err(|_| anyhow::anyhow!("peer head lock poisoned"))?
                    .insert((project, source_device), None);
                return Ok(None);
            }
            PeerResponse::Head { snapshot: Some(s) } if target.is_none() => s,
            PeerResponse::Manifest { snapshot: s } if target.is_some() => s,
            PeerResponse::Error { message } => anyhow::bail!(message),
            _ => anyhow::bail!("unexpected peer manifest response"),
        };
        self.vouch_served_author(&snapshot, project)?;
        transfer::validate_snapshot(&self.store, &self.identity, &snapshot, project, target)?;
        if target.is_none() {
            self.peer_heads
                .lock()
                .map_err(|_| anyhow::anyhow!("peer head lock poisoned"))?
                .insert(
                    (project, source_device),
                    Some(snapshot.manifest.snapshot_id),
                );
        }
        self.remember_author_manifest(&snapshot)?;
        let local = self.store.project(project)?.latest_local_snapshot;
        let mut seen = HashSet::from([snapshot.manifest.snapshot_id]);
        let mut parent = snapshot.manifest.parent_snapshot_id;
        for _ in 0..1024 {
            let Some(id) = parent else {
                return Ok(Some(snapshot));
            };
            ensure!(seen.insert(id), "cyclic snapshot ancestry");
            if Some(id) == local {
                return Ok(Some(snapshot));
            }
            let ancestor = if let Some(cached) = self.store.snapshot(id)? {
                cached
            } else {
                match network
                    .request(
                        source,
                        PeerRequest::Manifest {
                            project_id: project,
                            snapshot_id: id,
                        },
                    )
                    .await?
                {
                    PeerResponse::Manifest { snapshot } => snapshot,
                    PeerResponse::Error { message } => {
                        anyhow::bail!("snapshot ancestry is incomplete: {message}")
                    }
                    _ => anyhow::bail!("unexpected ancestor response"),
                }
            };
            self.vouch_served_author(&ancestor, project)?;
            transfer::validate_snapshot(&self.store, &self.identity, &ancestor, project, Some(id))?;
            parent = ancestor.manifest.parent_snapshot_id;
            self.remember_author_manifest(&ancestor)?;
        }
        anyhow::bail!("snapshot ancestry exceeds the MVP history limit; receive blocked")
    }
    /// `source` is paired with this project and serves only versions it accepted. A version signed
    /// by a collaborator this PC never paired with (removed before this PC joined, or a third
    /// device) would otherwise block the first receive; its author is recorded once the signature
    /// verifies. Devices this PC paired with are never vouched for, so a removal stays final.
    fn vouch_served_author(&self, snapshot: &SignedSnapshot, project: Uuid) -> Result<()> {
        let m = &snapshot.manifest;
        if m.project_id != project
            || m.author_device_id == self.identity.device_id
            || self.store.trusted_author(
                project,
                m.author_device_id,
                &snapshot.author_public_key,
            )?
        {
            return Ok(());
        }
        crypto::verify_snapshot(snapshot)?;
        self.store
            .vouch_author(project, m.author_device_id, &snapshot.author_public_key)?;
        Ok(())
    }
    fn remember_author_manifest(&self, snapshot: &SignedSnapshot) -> Result<()> {
        let name = if snapshot.manifest.author_device_id == self.identity.device_id {
            self.identity.name()
        } else {
            // A vouched author has no device card here, so no name.
            self.store
                .peers(Some(snapshot.manifest.project_id))?
                .into_iter()
                .find(|p| p.device_id == snapshot.manifest.author_device_id)
                .map_or_else(|| "Another collaborator".to_owned(), |p| p.name)
        };
        self.store.remember_manifest(snapshot, &name)
    }
    pub fn receiver(&self, network: Network) -> transfer::Receiver {
        transfer::Receiver {
            store: self.store.clone(),
            chunks: self.chunks.clone(),
            network,
            identity: self.identity.clone(),
            directory: self.directory.clone(),
            gate: transfer::DownloadGate::OPEN,
            stop: transfer::Stop::default(),
        }
    }
}
