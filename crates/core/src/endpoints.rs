//! Source-independent validation, durable acceptance, and network application of endpoint records.
use crate::Engine;
use anyhow::{ensure, Context, Result};
use network::Network;
use protocol::{EndpointRecord, PeerRequest, MAX_ENDPOINT_REQUEST_BYTES};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use storage::EndpointWriteOutcome;
use tokio::sync::watch;
use uuid::Uuid;

impl Engine {
    pub(crate) fn issue_endpoint_record(
        &self,
        peer_id: String,
        addresses: Vec<String>,
    ) -> Result<EndpointRecord> {
        let seq = self.store.next_endpoint_sequence(self.identity.device_id)?;
        self.identity.sign_current_endpoint(peer_id, addresses, seq)
    }

    /// Validate and persist an endpoint update from any authenticated delivery source.
    /// Supabase endpoint delivery (M5d) calls this same function as direct delivery (M5c).
    pub async fn apply_endpoint_record(
        &self,
        network: &Network,
        project_id: Uuid,
        source_peer: String,
        record: EndpointRecord,
    ) -> Result<EndpointWriteOutcome> {
        let store = self.store.clone();
        let (outcome, peer_id, addresses, sequence, expires_at_millis) =
            tokio::task::spawn_blocking(move || {
                let request = PeerRequest::EndpointUpdate {
                    project_id,
                    record: record.clone(),
                };
                ensure!(
                    serde_json::to_vec(&request)?.len() <= MAX_ENDPOINT_REQUEST_BYTES,
                    "endpoint request exceeds protocol limit"
                );
                let trusted = store
                    .authorized(project_id, &source_peer)?
                    .context("endpoint sender is not authorized for this project")?;
                network::validate_endpoint_record(&record, &trusted)?;
                let outcome = store.store_peer_endpoint(&record)?;
                Ok::<_, anyhow::Error>((
                    outcome,
                    trusted.peer_id,
                    record.addresses,
                    record.seq,
                    record.expires_at.timestamp_millis(),
                ))
            })
            .await
            .context("endpoint validation worker failed")??;

        if outcome == EndpointWriteOutcome::Stored {
            let now_millis = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let expiry_millis = u128::try_from(expires_at_millis).unwrap_or(0);
            let ttl_millis = expiry_millis.saturating_sub(now_millis);
            let ttl = Duration::from_millis(u64::try_from(ttl_millis).unwrap_or(u64::MAX));
            network
                .learn_peer_addresses(&peer_id, addresses, sequence, ttl)
                .await?;
        }
        Ok(outcome)
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct EndpointRecipient {
    project_id: Uuid,
    peer_id: String,
}

#[derive(Default)]
struct PublisherState {
    addresses: Vec<String>,
    recipients: HashSet<EndpointRecipient>,
    supabase_projects: HashSet<Uuid>,
    record: Option<EndpointRecord>,
    issued_at: Option<Instant>,
    acknowledged: HashSet<EndpointRecipient>,
    last_attempt: HashMap<EndpointRecipient, Instant>,
    supabase_acknowledged: HashSet<Uuid>,
    supabase_last_attempt: HashMap<Uuid, Instant>,
}

/// Publishes only to already connected, directly authorized project peers.
pub(crate) async fn run_endpoint_publisher(
    engine: Arc<Engine>,
    network: Network,
    observation: Arc<std::sync::RwLock<crate::NetworkObservation>>,
    mut cancel: watch::Receiver<bool>,
    publish: bool,
) {
    restore_endpoint_candidates(engine.clone(), network.clone()).await;
    if !publish {
        return;
    }
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    let mut state = PublisherState::default();
    loop {
        tokio::select! {
            _ = cancel.changed() => return,
            _ = interval.tick() => {}
        }
        if *cancel.borrow() {
            return;
        }
        let mut addresses = match tokio::time::timeout(
            Duration::from_secs(2),
            network.endpoint_addresses(),
        )
        .await
        {
            Ok(Ok(addresses)) => addresses,
            _ => continue,
        };
        // Listener addresses are bound to wildcard IPv4s. Expand them using
        // the current read-only IP Helper observation so peers get usable LAN
        // candidates without querying the router or firewall.
        let interface_ips = observation
            .read()
            .map(|observation| {
                observation
                    .interfaces
                    .iter()
                    .flat_map(|interface| interface.ipv4.iter().cloned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let listener_addresses = network.status().addresses;
        for listener in listener_addresses {
            let listener = listener
                .split_once("/p2p/")
                .map_or(listener.as_str(), |(address, _)| address);
            if !listener.starts_with("/ip4/0.0.0.0/") {
                continue;
            }
            for ip in &interface_ips {
                let address = listener.replacen("/ip4/0.0.0.0/", &format!("/ip4/{ip}/"), 1);
                if protocol::validate_endpoint_address(&address).is_ok() {
                    addresses.push(address);
                }
            }
        }
        addresses.sort();
        addresses.dedup();
        let (mut public, private): (Vec<_>, Vec<_>) = addresses
            .into_iter()
            .partition(|address| network::is_public_endpoint_address(address));
        public.extend(private);
        public.truncate(protocol::MAX_ENDPOINT_ADDRESSES);
        public.sort();
        let addresses = public;

        let direct_peers = network
            .status()
            .peers
            .into_iter()
            .filter(|(_, status)| status.online && status.route == "DIRECT")
            .map(|(peer, _)| peer)
            .collect::<HashSet<_>>();
        let engine_for_recipients = engine.clone();
        let (recipients, supabase_projects) = match tokio::task::spawn_blocking(move || {
            let mut recipients = HashSet::new();
            let mut supabase_projects = HashSet::new();
            for project in engine_for_recipients.store.projects()? {
                if matches!(
                    engine_for_recipients
                        .project_backend_config(project.project_id)?
                        .backend,
                    crate::ProjectBackend::Supabase
                ) {
                    supabase_projects.insert(project.project_id);
                }
                for card in engine_for_recipients
                    .store
                    .peers(Some(project.project_id))?
                {
                    if direct_peers.contains(&card.peer_id) {
                        recipients.insert(EndpointRecipient {
                            project_id: project.project_id,
                            peer_id: card.peer_id,
                        });
                    }
                }
            }
            Ok::<_, anyhow::Error>((recipients, supabase_projects))
        })
        .await
        {
            Ok(Ok(recipients)) => recipients,
            _ => continue,
        };

        let changed = addresses != state.addresses
            || recipients != state.recipients
            || supabase_projects != state.supabase_projects;
        let refresh = state
            .issued_at
            .is_some_and(|issued| issued.elapsed() >= Duration::from_secs(8 * 60));
        if (!recipients.is_empty() || !supabase_projects.is_empty())
            && (state.record.is_none() || changed || refresh)
        {
            let local_peer = network.status().peer_id;
            let engine_for_record = engine.clone();
            let address_list = addresses.clone();
            let record = match tokio::task::spawn_blocking(move || {
                engine_for_record.issue_endpoint_record(local_peer, address_list)
            })
            .await
            {
                Ok(Ok(record)) => record,
                _ => continue,
            };
            state.record = Some(record);
            state.issued_at = Some(Instant::now());
            state.acknowledged.clear();
            state.last_attempt.clear();
            state.supabase_acknowledged.clear();
            state.supabase_last_attempt.clear();
        }
        state.addresses = addresses;
        state.recipients = recipients;
        state.supabase_projects = supabase_projects;

        let Some(record) = state.record.clone() else {
            continue;
        };
        let now = Instant::now();
        let pending = state
            .recipients
            .iter()
            .filter(|recipient| !state.acknowledged.contains(*recipient))
            .filter(|recipient| {
                state
                    .last_attempt
                    .get(*recipient)
                    .is_none_or(|attempt| attempt.elapsed() >= Duration::from_secs(30))
            })
            .cloned()
            .collect::<Vec<_>>();
        if !pending.is_empty() {
            for recipient in &pending {
                state.last_attempt.insert(recipient.clone(), now);
            }
            let requests = pending.into_iter().map(|recipient| {
                let network = network.clone();
                let record = record.clone();
                async move {
                    let response = tokio::time::timeout(
                        Duration::from_secs(15),
                        network.request(
                            &recipient.peer_id,
                            PeerRequest::EndpointUpdate {
                                project_id: recipient.project_id,
                                record,
                            },
                        ),
                    )
                    .await;
                    (
                        recipient,
                        matches!(response, Ok(Ok(protocol::PeerResponse::EndpointUpdated))),
                    )
                }
            });
            let results = tokio::select! {
                _ = cancel.changed() => return,
                results = futures::future::join_all(requests) => results,
            };
            for (recipient, accepted) in results {
                if accepted {
                    state.acknowledged.insert(recipient);
                }
            }
        }

        let supabase_pending = state
            .supabase_projects
            .iter()
            .filter(|project| !state.supabase_acknowledged.contains(project))
            .filter(|project| {
                state
                    .supabase_last_attempt
                    .get(project)
                    .is_none_or(|attempt| attempt.elapsed() >= Duration::from_secs(30))
            })
            .copied()
            .collect::<Vec<_>>();
        for project in supabase_pending {
            state.supabase_last_attempt.insert(project, Instant::now());
            let engine_for_config = engine.clone();
            let project_config = tokio::task::spawn_blocking(move || -> Result<_> {
                let config = engine_for_config.project_backend_config(project)?;
                let key = crypto::project_key(
                    &engine_for_config.directory.join("secrets/projects"),
                    project,
                    None,
                )?;
                Ok((config, *key))
            })
            .await;
            let Ok(Ok((config, project_key))) = project_config else {
                continue;
            };
            let Some(url) = config.supabase_url else {
                continue;
            };
            let Some(anon_key) = config.supabase_anon_key else {
                continue;
            };
            let wrapper_record = record.clone();
            let wrapper = tokio::task::spawn_blocking(move || {
                crypto::wrap_endpoint_record(project, &wrapper_record, &project_key)
            })
            .await;
            let Ok(Ok(ciphertext)) = wrapper else {
                continue;
            };
            let client = match ::supabase_client::SupabaseClient::new(&url, &anon_key, project) {
                Ok(client) => client,
                Err(_) => continue,
            };
            let expires_at = crypto::endpoint_record_expires_at(&record);
            match client
                .publish_endpoint(&engine.identity, record.seq, &expires_at, &ciphertext)
                .await
            {
                Ok(true) => {
                    let _ = engine.record_supabase_function(project, "endpoint-publish", false);
                    state.supabase_acknowledged.insert(project);
                }
                Ok(false) | Err(::supabase_client::SupabaseError::Rejected) => {
                    let _ = engine.record_supabase_function(project, "endpoint-publish", false);
                }
                Err(::supabase_client::SupabaseError::NotDeployed) => {
                    let _ = engine.record_supabase_function(project, "endpoint-publish", true);
                }
                Err(::supabase_client::SupabaseError::Unavailable) => {}
            }
        }
    }
}

async fn restore_endpoint_candidates(engine: Arc<Engine>, network: Network) {
    let candidates = tokio::task::spawn_blocking(move || {
        let mut candidates = HashMap::<String, (Vec<String>, u64, Duration)>::new();
        for project in engine.store.projects()? {
            for trusted in engine.store.peers(Some(project.project_id))? {
                if candidates.contains_key(&trusted.peer_id) {
                    continue;
                }
                let Some(state) = engine
                    .store
                    .current_peer_endpoint_state(trusted.device_id)?
                else {
                    continue;
                };
                let Some(record) = state.record else {
                    continue;
                };
                if network::validate_endpoint_record(&record, &trusted).is_err() {
                    continue;
                }
                let now_millis = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let expiry_millis =
                    u128::try_from(record.expires_at.timestamp_millis()).unwrap_or_default();
                let ttl_millis = expiry_millis.saturating_sub(now_millis);
                let ttl = Duration::from_millis(u64::try_from(ttl_millis).unwrap_or(u64::MAX));
                if !ttl.is_zero() {
                    candidates.insert(trusted.peer_id, (record.addresses, state.highest_seq, ttl));
                }
            }
        }
        Ok::<_, anyhow::Error>(candidates)
    })
    .await;
    let Ok(Ok(candidates)) = candidates else {
        return;
    };
    for (peer, (addresses, sequence, ttl)) in candidates {
        let _ = network
            .learn_peer_addresses(&peer, addresses, sequence, ttl)
            .await;
    }
}
