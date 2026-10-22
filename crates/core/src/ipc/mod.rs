//! Bounded Windows IPC adapter. Shares the owner's Engine and network handle.
mod jobs;
mod security;
mod status;
use crate::{lang::tr, Engine};
use anyhow::{ensure, Context, Result};
use network::Network;
use protocol::{PluginEvent, PluginStatus, MAX_IPC_BYTES};
pub use security::plugin_pipe_name;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::windows::named_pipe::NamedPipeServer,
    sync::watch,
    task::{JoinHandle, JoinSet},
    time::timeout,
};
use uuid::Uuid;

#[derive(Default)]
struct State {
    epoch: u64,
    sessions: HashMap<Uuid, Uuid>,
    messages: HashMap<Uuid, status::Notice>,
    jobs: HashMap<Uuid, jobs::Job>,
    setting_tasks: Vec<JoinHandle<()>>,
    desktop_request: Option<PluginDesktopRequest>,
}
#[derive(Clone, Copy)]
pub struct PluginDesktopRequest {
    pub project_id: Option<Uuid>,
}
struct Shared {
    engine: Arc<Engine>,
    network: Network,
    closing: AtomicBool,
    state: Mutex<State>,
    desktop_available: bool,
}
impl Shared {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, State>> {
        self.state
            .lock()
            .map_err(|_| anyhow::anyhow!("Plugin session lock poisoned"))
    }
    async fn status(&self, project: Uuid) -> Result<PluginStatus> {
        let engine = self.engine.clone();
        let network = self.network.clone();
        let (summary, projects, more, auto_share, warnings) =
            tokio::task::spawn_blocking(move || -> Result<_> {
                let (projects, more) = status::projects(&engine)?;
                // A project removed in TrackMesh reads as unlinked, so the plugin offers the list again.
                let summary = if engine.store.listed(project)? {
                    engine.project_summary(project)
                } else {
                    Err(anyhow::anyhow!("project was removed from TrackMesh"))
                };
                Ok((
                    summary,
                    projects,
                    more,
                    engine.auto_share_enabled(project)?,
                    engine.presence_warnings(project, &network)?,
                ))
            })
            .await??;
        let mut reply = match summary {
            Ok(summary) => status::summarize(
                summary,
                &self.network,
                self.lock()?.messages.get(&project).cloned(),
            ),
            Err(_) => status::unavailable(
                project,
                tr(
                    "Add this project's folder in TrackMesh first (Add project).",
                    "Önce bu projenin klasörünü TrackMesh'e ekle (Proje ekle).",
                ),
            ),
        };
        reply.projects = projects;
        reply.projects_more = more;
        reply.desktop_available = self.desktop_available;
        reply.get_enabled &= self.desktop_available;
        reply.auto_share = auto_share && reply.auto_share_supported;
        if let Some(warning) = warnings.first() {
            reply.presence_warning = status::bounded(&warning.line);
            reply.presence_since = warning.since;
            reply.share_confirmation = status::bounded(&warning.share_confirmation);
            reply.presence_danger = warning.both_working;
            reply.presence_get_line = status::bounded(&warning.get_line);
            if reply.message == status::get_guidance() {
                reply.message.push(' ');
                reply.message.push_str(&reply.presence_get_line);
            }
        }
        Ok(reply)
    }
}

pub struct PluginRuntime {
    stop: watch::Sender<bool>,
    task: JoinHandle<Result<()>>,
    shared: Arc<Shared>,
}
impl PluginRuntime {
    pub fn start(engine: Arc<Engine>, network: Network, suffix: &str) -> Result<Self> {
        Self::start_impl(engine, network, suffix, false)
    }
    pub fn start_with_desktop(engine: Arc<Engine>, network: Network, suffix: &str) -> Result<Self> {
        Self::start_impl(engine, network, suffix, true)
    }
    pub fn take_desktop_request(&self) -> Option<PluginDesktopRequest> {
        self.shared.lock().ok()?.desktop_request.take()
    }
    fn start_impl(
        engine: Arc<Engine>,
        network: Network,
        suffix: &str,
        desktop_available: bool,
    ) -> Result<Self> {
        let name = plugin_pipe_name(suffix)?;
        let pipe = security::create_pipe(&name, true)
            .context("Cannot own the per-user plugin pipe; another bridge may be active")?;
        let (stop, receiver) = watch::channel(false);
        let shared = Arc::new(Shared {
            engine,
            network,
            closing: AtomicBool::new(false),
            state: Mutex::new(State::default()),
            desktop_available,
        });
        let task = tokio::spawn(serve(pipe, name, shared.clone(), receiver));
        Ok(Self { stop, task, shared })
    }
    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }
    pub fn is_idle(&self) -> bool {
        self.idle_epoch().is_some()
    }
    pub fn idle_epoch(&self) -> Option<u64> {
        let state = self.shared.lock().ok()?;
        (state.sessions.is_empty()
            && state.desktop_request.is_none()
            && state.setting_tasks.iter().all(|task| task.is_finished())
            && state.jobs.values().all(|job| job.task.is_finished()))
        .then_some(state.epoch)
    }
    /// Claim idle shutdown under the same gate used to accept connections/jobs.
    pub fn close_if_idle(&self) -> bool {
        let Ok(state) = self.shared.lock() else {
            return false;
        };
        if !state.sessions.is_empty()
            || state.desktop_request.is_some()
            || state.setting_tasks.iter().any(|task| !task.is_finished())
            || state.jobs.values().any(|job| !job.task.is_finished())
        {
            return false;
        }
        self.begin_close();
        true
    }
    /// Stop accepting commands immediately; `stop` still awaits started local work.
    pub fn begin_close(&self) {
        self.shared.closing.store(true, Ordering::Release);
        let _ = self.stop.send(true);
    }
    pub async fn stop(self) -> Result<()> {
        self.begin_close();
        let server = self.task.await;
        let (tasks, setting_tasks) = {
            let mut state = self.shared.lock()?;
            (
                std::mem::take(&mut state.jobs),
                std::mem::take(&mut state.setting_tasks),
            )
        };
        let mut errors = Vec::new();
        for task in setting_tasks {
            if let Err(error) = task.await {
                errors.push(error.to_string());
            }
        }
        for job in tasks.into_values() {
            if let Err(e) = job.task.await {
                errors.push(e.to_string());
            }
        }
        server??;
        ensure!(
            errors.is_empty(),
            "Plugin snapshot worker failed: {}",
            errors.join("; ")
        );
        Ok(())
    }
}
impl Drop for Shared {
    fn drop(&mut self) {
        self.closing.store(true, Ordering::Release);
    }
}

async fn serve(
    mut pipe: NamedPipeServer,
    name: String,
    shared: Arc<Shared>,
    mut stop: watch::Receiver<bool>,
) -> Result<()> {
    let mut clients = JoinSet::new();
    let outcome = loop {
        tokio::select! {
            biased;
            _ = stop.changed() => break Ok(()),
            Some(result) = clients.join_next(), if !clients.is_empty() => {
                if let Err(error) = result { tracing::warn!(%error, "Plugin IPC worker ended"); }
            },
            connected = pipe.connect(), if clients.len() < 16 => {
                if let Err(error) = connected { break Err(error.into()); }
                let next = match security::create_pipe(&name, false) { Ok(next) => next, Err(error) => break Err(error) };
                let connected = std::mem::replace(&mut pipe, next);
                let shared = shared.clone();
                let mut stop = stop.clone();
                clients.spawn(async move {
                    let mut binding = None;
                    tokio::select! {
                        _ = stop.changed() => {},
                        _ = connection(connected, &shared, &mut binding) => {},
                    }
                    if let Some((session, _)) = binding { if let Ok(mut state) = shared.lock() { state.sessions.remove(&session); shared.engine.presence_bind(session, None); } }
                });
            },
        }
    };
    shared.closing.store(true, Ordering::Release);
    // Connection tasks also listen to this channel. On an internal listener error,
    // abort only pipe tasks; snapshot workers are owned separately and awaited by stop.
    clients.abort_all();
    while clients.join_next().await.is_some() {}
    let mut state = shared.lock()?;
    for session in state.sessions.keys() {
        shared.engine.presence_bind(*session, None);
    }
    state.sessions.clear();
    drop(state);
    drop(pipe);
    outcome
}

async fn connection(
    mut pipe: NamedPipeServer,
    shared: &Arc<Shared>,
    binding: &mut Option<(Uuid, Uuid)>,
) -> Result<()> {
    loop {
        ensure!(
            !shared.closing.load(Ordering::Acquire),
            "Plugin bridge is closing"
        );
        let event = timeout(Duration::from_secs(15), async {
            let length = pipe.read_u32_le().await? as usize;
            ensure!(
                length > 0 && length <= MAX_IPC_BYTES,
                "Invalid plugin frame size"
            );
            let mut bytes = vec![0; length];
            pipe.read_exact(&mut bytes).await?;
            Ok::<PluginEvent, anyhow::Error>(serde_json::from_slice(&bytes)?)
        })
        .await??;
        let (session, project) = event.ids();
        ensure!(
            !session.is_nil() && !project.is_nil(),
            "Nil plugin identifiers"
        );
        match &event {
            PluginEvent::PluginConnected { .. } => {
                ensure!(binding.is_none(), "Plugin connection already bound");
                let mut state = shared.lock()?;
                ensure!(
                    !shared.closing.load(Ordering::Acquire),
                    "Plugin bridge is closing"
                );
                ensure!(
                    !state.sessions.contains_key(&session),
                    "Plugin session already connected"
                );
                state.sessions.insert(session, project);
                shared.engine.presence_bind(session, Some(project));
                state.epoch = state.epoch.wrapping_add(1);
                *binding = Some((session, project));
            }
            PluginEvent::ProjectId { .. } => {
                ensure!(
                    binding.is_some_and(|(id, _)| id == session),
                    "Plugin session is not bound"
                );
                shared.lock()?.sessions.insert(session, project);
                shared.engine.presence_bind(session, Some(project));
                *binding = Some((session, project));
            }
            _ => ensure!(
                *binding == Some((session, project)),
                "Plugin session/project mismatch"
            ),
        }
        let mut reply = shared.status(project).await?;
        match event {
            PluginEvent::SetAutoShare { enabled, .. } => {
                ensure!(
                    !shared.closing.load(Ordering::Acquire),
                    "Plugin bridge is closing"
                );
                let changed = match shared.schedule_setting(project, enabled) {
                    Ok(receiver) => receiver.await?,
                    Err(error) => Err(error),
                };
                match changed {
                    Ok(()) => {
                        shared.lock()?.messages.remove(&project);
                        reply = shared.status(project).await?;
                        reply.message = if enabled {
                            tr("Every DAW save is now shared.", "Artık her DAW kaydı paylaşılıyor.")
                        } else {
                            tr("Saves are no longer shared by themselves. Use Share current version.", "Kayıtlar artık kendiliğinden paylaşılmıyor. Güncel sürümü paylaş'ı kullan.")
                        }
                        .into();
                    }
                    Err(_) => {
                        reply.ok = false;
                        reply.message = tr(
                            "Could not save this setting. Open TrackMesh and try again.",
                            "Bu ayar kaydedilemedi. TrackMesh'i aç ve tekrar dene.",
                        )
                        .into();
                    }
                }
            }
            PluginEvent::SendSnapshot { .. } if reply.share_enabled => {
                match shared.schedule(project, false) {
                    Ok(()) => reply.message = status::queued(),
                    Err(_) => {
                        reply.ok = false;
                        reply.message = tr(
                            "TrackMesh is busy or closing. Try again in a moment.",
                            "TrackMesh meşgul ya da kapanıyor. Biraz sonra tekrar dene.",
                        )
                        .into();
                    }
                }
            }
            PluginEvent::SendSnapshot { .. } => {
                reply.ok = false;
                reply.message = tr(
                    "Sharing isn't possible yet. Open TrackMesh to see what it needs.",
                    "Şu an paylaşılamıyor. Neyin gerektiğini görmek için TrackMesh'i aç.",
                )
                .into();
            }
            PluginEvent::ReceiveSnapshot { .. } => {
                if !shared.desktop_available {
                    reply.ok = false;
                    reply.message = tr(
                        "Open TrackMesh to get your collaborator's version.",
                        "İş arkadaşının sürümünü almak için TrackMesh'i aç.",
                    )
                    .into();
                } else {
                    let mut state = shared.lock()?;
                    ensure!(
                        !shared.closing.load(Ordering::Acquire),
                        "Plugin bridge is closing"
                    );
                    state.desktop_request = Some(PluginDesktopRequest {
                        project_id: (!reply.project_name.is_empty()).then_some(project),
                    });
                    state.epoch = state.epoch.wrapping_add(1);
                    reply.message = status::get_guidance();
                    if !reply.presence_get_line.is_empty() {
                        reply.message.push(' ');
                        reply.message.push_str(&reply.presence_get_line);
                    }
                    if state.messages.contains_key(&project) || state.messages.len() < 64 {
                        state.messages.insert(project, status::Notice::Receive);
                    }
                }
            }
            PluginEvent::SaveObserved { .. }
                if !reply.project_name.is_empty() && !reply.auto_share =>
            {
                reply.ok = true;
                reply.message = status::saved();
                let mut state = shared.lock()?;
                if state.messages.contains_key(&project) || state.messages.len() < 64 {
                    state.messages.insert(project, status::Notice::Saved);
                }
            }
            PluginEvent::SaveObserved { .. } if !reply.project_name.is_empty() => {
                match shared.schedule(project, true) {
                    Ok(()) => reply.message = status::saving(),
                    Err(_) => {
                        reply.ok = false;
                        reply.message = tr("TrackMesh is busy or closing, so this save wasn't shared. Press Share current version.", "TrackMesh meşgul ya da kapanıyor, bu kayıt paylaşılmadı. Güncel sürümü paylaş'a bas.").into();
                    }
                }
            }
            PluginEvent::SessionEnding { .. } => {
                write_reply(&mut pipe, &reply).await?;
                return Ok(());
            }
            _ => {}
        }
        write_reply(&mut pipe, &reply).await?;
        // A client cannot turn compact polling into an unbounded SQLite job stream.
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
async fn write_reply(pipe: &mut NamedPipeServer, reply: &PluginStatus) -> Result<()> {
    let bytes = serde_json::to_vec(reply)?;
    ensure!(bytes.len() <= MAX_IPC_BYTES, "Plugin reply is too large");
    timeout(Duration::from_secs(5), async {
        pipe.write_u32_le(bytes.len() as u32).await?;
        pipe.write_all(&bytes).await
    })
    .await??;
    Ok(())
}
