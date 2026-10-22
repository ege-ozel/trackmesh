//! Advisory activity only. Never used for lineage, installation or owner retention.
use crate::Engine;
use anyhow::Result;
use network::Network;
use protocol::{PeerRequest, PeerResponse, PresenceSource};
use serde::Serialize;
use std::{
    collections::HashMap,
    fs::{self, ReadDir},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

#[derive(Default)]
pub(crate) struct PresenceState {
    sessions: HashMap<Uuid, Uuid>,
    local: HashMap<Uuid, Local>,
    received: HashMap<(Uuid, String), Received>,
}
#[derive(Default)]
struct Local {
    plugin_ended: Option<SystemTime>,
    applied: Option<SystemTime>,
    last_check: Option<Instant>,
    baseline: Option<SystemTime>,
    modified: Option<SystemTime>,
    cursor: Vec<(PathBuf, ReadDir)>,
    root: PathBuf,
    signal: Option<Signal>,
}
struct Received {
    since: u64,
    refreshed: Instant,
    generation: u64,
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct Signal {
    working: bool,
    since: u64,
    source: PresenceSource,
}
#[derive(Clone, Serialize)]
pub struct PresenceWarning {
    pub device_id: Uuid,
    pub name: String,
    pub since: u64,
    pub both_working: bool,
    pub line: String,
    pub share_confirmation: String,
    pub get_line: String,
}
struct Silence {
    generation: u64,
    failed: Instant,
    retried: bool,
}

fn unix(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn recent(time: SystemTime, now: SystemTime, seconds: u64) -> bool {
    now.duration_since(time)
        .is_ok_and(|age| age < Duration::from_secs(seconds))
}

impl Engine {
    #[cfg(windows)]
    pub(crate) fn presence_bind(&self, session: Uuid, project: Option<Uuid>) {
        let Ok(mut state) = self.presence.lock() else {
            return;
        };
        let old = state.sessions.remove(&session);
        if let Some(project) = project {
            state.sessions.insert(session, project);
        }
        if let Some(old) = old.filter(|old| Some(*old) != project) {
            if !state.sessions.values().any(|bound| *bound == old) {
                state.local.entry(old).or_default().plugin_ended = Some(SystemTime::now());
            }
        }
    }

    pub(crate) fn presence_applied(&self, project: Uuid) {
        if let Ok(mut state) = self.presence.lock() {
            let local = state.local.entry(project).or_default();
            local.applied = Some(SystemTime::now());
            local.modified = None;
            local.cursor.clear();
        }
    }

    pub(crate) fn accept_presence(
        &self,
        network: &Network,
        peer: &str,
        request: &PeerRequest,
    ) -> Result<PeerResponse> {
        let PeerRequest::Presence {
            project_id,
            working,
            since,
            ..
        } = request
        else {
            anyhow::bail!("not a presence request")
        };
        anyhow::ensure!(
            self.store.authorized(*project_id, peer)?.is_some(),
            "project access denied"
        );
        let now = unix(SystemTime::now());
        anyhow::ensure!(*since <= now.saturating_add(60), "invalid presence time");
        let mut state = self
            .presence
            .lock()
            .map_err(|_| anyhow::anyhow!("presence lock poisoned"))?;
        let key = (*project_id, peer.to_owned());
        state
            .received
            .retain(|_, received| received.refreshed.elapsed() < Duration::from_secs(180));
        if *working {
            if let Some(connection) = network
                .status()
                .peers
                .get(peer)
                .filter(|connection| connection.online)
            {
                state.received.insert(
                    key,
                    Received {
                        since: (*since).min(now),
                        refreshed: Instant::now(),
                        generation: connection.generation,
                    },
                );
            }
        } else {
            state.received.remove(&key);
        }
        // Reuse an existing acknowledgement; Presence is the only added peer message.
        Ok(PeerResponse::SnapshotNoticed)
    }

    pub fn presence_warnings(
        &self,
        project: Uuid,
        network: &Network,
    ) -> Result<Vec<PresenceWarning>> {
        let peers = self.store.peers(Some(project))?;
        let net = network.status();
        let state = self
            .presence
            .lock()
            .map_err(|_| anyhow::anyhow!("presence lock poisoned"))?;
        let both = state.sessions.values().any(|bound| *bound == project)
            || state
                .local
                .get(&project)
                .and_then(|local| local.signal)
                .is_some_and(|signal| signal.working);
        Ok(peers.into_iter().filter_map(|peer| {
            let received = state.received.get(&(project, peer.peer_id.clone()))?;
            let connection = net.peers.get(&peer.peer_id)?;
            if !connection.online || connection.generation != received.generation || received.refreshed.elapsed() >= Duration::from_secs(180) { return None; }
            let name: String = peer.name.chars().take(80).map(|ch| if ch.is_control() { ' ' } else { ch }).collect();
            Some(PresenceWarning {
                device_id: peer.device_id, name: name.clone(), since: received.since, both_working: both,
                line: if both {
                    if crate::lang::is_turkish() { format!("{name} de bu projede çalışıyor. İkiniz de kaydederseniz sürümleriniz ayrılır.") }
                    else { format!("{name} is also working on this project. If you both save changes, your versions will split.") }
                } else if crate::lang::is_turkish() { format!("{name} şu an bu projede çalışıyor") }
                    else { format!("{name} is working on this project now") },
                share_confirmation: if crate::lang::is_turkish() { format!("{name} şu an bu projede çalışıyor. Yine de paylaşılsın mı?") }
                    else { format!("{name} is working on this project now. Share anyway?") },
                get_line: if crate::lang::is_turkish() { format!("{name} yakında daha yeni bir sürüm paylaşabilir.") }
                    else { format!("{name} may share a newer version soon.") },
            })
        }).collect())
    }

    fn presence_tick(&self) -> Result<Vec<(Uuid, Signal, Vec<String>)>> {
        let projects = self.store.projects()?;
        let mut state = self
            .presence
            .lock()
            .map_err(|_| anyhow::anyhow!("presence lock poisoned"))?;
        state
            .local
            .retain(|id, _| projects.iter().any(|project| project.project_id == *id));
        let now = SystemTime::now();
        let mut signals = Vec::new();
        for project in projects {
            let id = project.project_id;
            let plugin = state.sessions.values().any(|bound| *bound == id);
            let local = state.local.entry(id).or_default();
            if !plugin
                && local
                    .last_check
                    .is_none_or(|at| at.elapsed() >= Duration::from_secs(60))
            {
                local.last_check = Some(Instant::now());
                // No project lock: it is exclusive, so holding it would fail a concurrent Share/Get.
                // The walk is read-only and advisory; a race only yields a stale warning.
                if !project.recovery_required && !self.store.has_active_transfer(id)? {
                    {
                        let head_time = self.store.head(id)?.map(|head| {
                            UNIX_EPOCH
                                + Duration::from_secs(
                                    head.manifest.created_at.timestamp().max(0) as u64
                                )
                        });
                        let baseline = head_time
                            .into_iter()
                            .chain(local.applied)
                            .chain(self.store.last_completed_apply(id)?)
                            .max()
                            .unwrap_or(UNIX_EPOCH);
                        let root = PathBuf::from(&project.root);
                        if local.baseline != Some(baseline) || local.root != root {
                            local.baseline = Some(baseline);
                            local.modified = None;
                            local.cursor.clear();
                            local.root = root;
                        }
                        local.walk(baseline, now);
                    }
                } else {
                    local.cursor.clear();
                }
            }
            let folder = local.modified.is_some_and(|modified| {
                recent(modified, now, 600)
                    && local.baseline.is_some_and(|baseline| modified > baseline)
                    && local
                        .plugin_ended
                        .is_none_or(|ended| modified > ended || recent(ended, now, 120))
            });
            let plugin_grace = local
                .plugin_ended
                .is_some_and(|ended| recent(ended, now, 120))
                && local
                    .signal
                    .is_some_and(|old| old.working && old.source == PresenceSource::Plugin);
            let source = if plugin || plugin_grace {
                PresenceSource::Plugin
            } else {
                PresenceSource::Folder
            };
            let working = plugin || plugin_grace || folder;
            let since = local
                .signal
                .filter(|old| old.working == working && old.source == source)
                .map_or_else(|| unix(now), |old| old.since);
            let signal = Signal {
                working,
                since,
                source,
            };
            local.signal = Some(signal);
            let peers = self
                .store
                .peers(Some(id))?
                .into_iter()
                .filter(|peer| peer.device_id != self.identity.device_id)
                .map(|peer| peer.peer_id)
                .collect();
            signals.push((id, signal, peers));
        }
        Ok(signals)
    }
}

impl Local {
    fn walk(&mut self, baseline: SystemTime, now: SystemTime) {
        let started = Instant::now();
        if snapshot::canonical_root(&self.root).is_err() {
            self.cursor.clear();
            return;
        }
        if self.cursor.is_empty() {
            if let Ok(entries) = fs::read_dir(&self.root) {
                self.cursor.push((self.root.clone(), entries));
            }
        }
        // ReadDir is retained so a large single directory also resumes, without rereading entries.
        for _ in 0..5000 {
            if started.elapsed() >= Duration::from_millis(200) {
                break;
            }
            let Some((directory, entries)) = self.cursor.last_mut() else {
                break;
            };
            // Revalidate retained directory paths in case a directory became a junction between ticks.
            if directory
                .ancestors()
                .take_while(|path| path.starts_with(&self.root))
                .any(|path| {
                    fs::symlink_metadata(path)
                        .map_or(true, |metadata| snapshot::is_reparse(&metadata))
                })
            {
                self.cursor.pop();
                continue;
            }
            let Some(entry) = entries.next() else {
                self.cursor.pop();
                continue;
            };
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            let Ok(relative) = path.strip_prefix(&self.root) else {
                continue;
            };
            let Some(relative) = relative.to_str() else {
                continue;
            };
            let relative = relative.replace('\\', "/");
            if snapshot::is_ignored_path(&relative)
                || relative
                    .split('/')
                    .any(|part| part.eq_ignore_ascii_case(".trackmesh"))
            {
                continue;
            }
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if snapshot::is_reparse(&metadata) {
                continue;
            }
            if metadata.is_dir() {
                if self.cursor.len() < 128 {
                    if let Ok(entries) = fs::read_dir(&path) {
                        self.cursor.push((path, entries));
                    }
                }
            } else if metadata.is_file() {
                if let Ok(modified) = metadata.modified() {
                    if modified > baseline && recent(modified, now, 600) {
                        self.modified =
                            Some(self.modified.map_or(modified, |old| old.max(modified)));
                    }
                }
            }
        }
    }
}

pub(crate) async fn run(
    engine: Arc<Engine>,
    network: Network,
    mut cancel: tokio::sync::watch::Receiver<bool>,
) {
    let mut sent: HashMap<(Uuid, String), (Signal, Instant, u64)> = HashMap::new();
    let mut silenced: HashMap<String, Silence> = HashMap::new();
    let mut timer = tokio::time::interval(Duration::from_secs(1));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! { biased; _ = cancel.changed() => return, _ = timer.tick() => {} }
        let disk = engine.clone();
        let Ok(Ok(signals)) = tokio::task::spawn_blocking(move || disk.presence_tick()).await
        else {
            continue;
        };
        let net = network.status();
        silenced.retain(|peer, silence| {
            net.peers.get(peer).is_some_and(|connection| {
                connection.online && connection.generation == silence.generation
            })
        });
        sent.retain(|(project, peer), (_, _, generation)| {
            signals
                .iter()
                .any(|(id, _, peers)| id == project && peers.contains(peer))
                && net.peers.get(peer).is_some_and(|connection| {
                    connection.online && connection.generation == *generation
                })
        });
        for (project_id, signal, peers) in signals {
            for peer in peers {
                let Some(connection) = net.peers.get(&peer).filter(|connection| connection.online)
                else {
                    continue;
                };
                if let Some(silence) = silenced.get_mut(&peer) {
                    if silence.retried || silence.failed.elapsed() < Duration::from_secs(600) {
                        continue;
                    }
                    silence.retried = true;
                }
                let key = (project_id, peer.clone());
                let due = sent.get(&key).is_none_or(|(old, at, _)| {
                    *old != signal || (signal.working && at.elapsed() >= Duration::from_secs(60))
                });
                if !due {
                    continue;
                }
                let request = PeerRequest::Presence {
                    project_id,
                    working: signal.working,
                    since: signal.since,
                    source: signal.source,
                };
                let result = tokio::select! {
                    _ = cancel.changed() => return,
                    result = tokio::time::timeout(Duration::from_secs(2), network.request(&peer, request)) => result,
                };
                if matches!(result, Ok(Ok(PeerResponse::SnapshotNoticed))) {
                    sent.insert(key, (signal, Instant::now(), connection.generation));
                    silenced.remove(&peer);
                } else {
                    silenced.entry(peer.clone()).or_insert(Silence {
                        generation: connection.generation,
                        failed: Instant::now(),
                        retried: false,
                    });
                    sent.retain(|(_, recipient), _| *recipient != peer);
                    if let Ok(mut state) = engine.presence.lock() {
                        state.received.retain(|(_, sender), _| *sender != peer);
                    }
                }
            }
        }
    }
}
