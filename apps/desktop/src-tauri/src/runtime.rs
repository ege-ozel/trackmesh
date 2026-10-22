//! One explicit desktop session, bounded jobs, and awaited shutdown.
use crate::documents;
use anyhow::{ensure, Context, Result};
use clap::Parser;
use network::{Network, NetworkConfig, NetworkStatus};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;
use trackmesh_core::{
    default_data_directory, observe_firewall, observe_network, peer::PeerRuntime,
    peer_connection_state, port_observation_status, Engine, FirewallObservation,
    NetworkObservation, PairingStatus, PeerCheck, PeerConnectionState, PluginRuntime,
    PortObservationStatus, ProjectBackend, ProjectBackendConfig, RevokeOutcome, SessionGrace,
    SummaryPage, SupabaseDefaultConfig, SupabaseProjectState,
};
use uuid::Uuid;

// The window sends its language on start and on change; native dialogs and the plugin follow it.
pub use trackmesh_core::lang::{is_turkish, tr};

fn receive_blocked_or_error(error: anyhow::Error) -> Result<Value> {
    if let Some(refusal) = error.downcast_ref::<transfer::ReceiveRefusal>() {
        // The core message advises snapshotting first; the card offers the explicit choice instead.
        let (code, message) = match refusal {
            transfer::ReceiveRefusal::LocalChanges => (
                "LOCAL_CHANGES",
                "This folder has changes you haven't shared. Choose which version to keep.",
            ),
            transfer::ReceiveRefusal::Diverged => (
                "DIVERGED",
                "Both versions changed. Choose which version to keep.",
            ),
        };
        Ok(json!({"receive_blocked": code, "message": message}))
    } else {
        Err(error)
    }
}

#[derive(Clone, Parser)]
#[command(
    name = "trackmesh-desktop",
    about = "Private Windows project snapshots and transfers"
)]
pub struct Options {
    /// Plugin-launched owner: start hidden and stay available in the system tray.
    #[arg(long)]
    plugin_owner: bool,
    #[arg(long, env = "TRACKMESH_DATA_DIR")]
    data_dir: Option<PathBuf>,
    #[arg(long, default_value = "My studio PC")]
    device_name: String,
    #[arg(long, default_value_t = 43101)]
    port: u16,
    #[arg(long)]
    external_address: Vec<String>,
    #[arg(long, env = "TRACKMESH_IPC_SUFFIX", default_value = "")]
    ipc_suffix: String,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    SetAutoShare {
        project_id: Uuid,
        enabled: bool,
    },
    DeleteBackups {
        selected: Vec<trackmesh_core::BackupSelection>,
    },
    Link {
        project_id: Uuid,
        name: String,
        folder: PathBuf,
    },
    ChangeProjectRoot {
        project_id: Uuid,
        folder: PathBuf,
    },
    SetProjectBackend {
        project_id: Uuid,
        backend: ProjectBackend,
        supabase_url: Option<String>,
        supabase_anon_key: Option<String>,
    },
    SetDefaultSupabase {
        url: Option<String>,
        publishable_key: Option<String>,
    },
    SetDeviceName {
        name: String,
    },
    SetProjectsLocation {
        location: PathBuf,
    },
    SetManualPublicEndpoint {
        public_ipv4: Option<String>,
        external_port: Option<u16>,
    },
    Join {
        folder: PathBuf,
    },
    CompleteInviteLink {
        supabase_url: Option<String>,
        supabase_anon_key: Option<String>,
    },
    Pair {
        project_id: Uuid,
    },
    Revoke {
        project_id: Uuid,
        device_id: Uuid,
    },
    RemoveProject {
        project_id: Uuid,
        delete_data: bool,
    },
    RestoreProject {
        project_id: Uuid,
    },
    ExportInvite {
        project_id: Uuid,
        public_ip: Option<String>,
        external_port: Option<u16>,
    },
    CopyInviteLink {
        project_id: Uuid,
    },
    ExportCard,
    OpenPairing {
        project_id: Uuid,
    },
    ClosePairing {
        project_id: Uuid,
    },
    ApprovePairing {
        project_id: Uuid,
        request_id: Uuid,
    },
    RejectPairing {
        project_id: Uuid,
        request_id: Uuid,
    },
    RequestPairing {
        project_id: Uuid,
        device_id: Uuid,
    },
    /// Offers a created project to a device this PC already shares another project with.
    OfferProject {
        project_id: Uuid,
        device_id: Uuid,
    },
    AcceptOffer {
        project_id: Uuid,
    },
    DeclineOffer {
        project_id: Uuid,
    },
    Send {
        project_id: Uuid,
    },
    Check {
        project_id: Uuid,
        device_id: Uuid,
    },
    Receive {
        project_id: Uuid,
        device_id: Uuid,
        snapshot_id: Option<Uuid>,
        daw_closed: bool,
        /// The automatic first version; never set by the window.
        #[serde(skip)]
        automatic: bool,
    },
    UsePeerVersion {
        project_id: Uuid,
        device_id: Uuid,
        daw_closed: bool,
    },
    Recover {
        project_id: Uuid,
        daw_closed: bool,
    },
}

impl Action {
    fn project(&self) -> Option<Uuid> {
        match self {
            Self::Join { .. }
            | Self::SetAutoShare { .. }
            | Self::DeleteBackups { .. }
            | Self::CompleteInviteLink { .. }
            | Self::SetDefaultSupabase { .. }
            | Self::SetDeviceName { .. }
            | Self::SetProjectsLocation { .. }
            | Self::SetManualPublicEndpoint { .. }
            | Self::AcceptOffer { .. }
            | Self::DeclineOffer { .. }
            | Self::ExportCard => None,
            Self::Link { project_id, .. }
            | Self::ChangeProjectRoot { project_id, .. }
            | Self::SetProjectBackend { project_id, .. }
            | Self::Pair { project_id }
            | Self::Revoke { project_id, .. }
            | Self::RemoveProject { project_id, .. }
            | Self::RestoreProject { project_id }
            | Self::ExportInvite { project_id, .. }
            | Self::CopyInviteLink { project_id }
            | Self::OpenPairing { project_id }
            | Self::ClosePairing { project_id }
            | Self::ApprovePairing { project_id, .. }
            | Self::RejectPairing { project_id, .. }
            | Self::RequestPairing { project_id, .. }
            | Self::OfferProject { project_id, .. }
            | Self::Send { project_id }
            | Self::Check { project_id, .. }
            | Self::Receive { project_id, .. }
            | Self::UsePeerVersion { project_id, .. }
            | Self::Recover { project_id, .. } => Some(*project_id),
        }
    }
    fn label(&self) -> &'static str {
        match self {
            Self::DeleteBackups { .. } => "Delete old backups",
            Self::Link { .. } => "Link folder",
            Self::ChangeProjectRoot { .. } => "Change project folder",
            Self::SetProjectBackend { .. } => "Save project backend",
            Self::SetDefaultSupabase { .. } => "Save default Supabase settings",
            Self::SetDeviceName { .. } => "Save device name",
            Self::SetAutoShare { .. } => "Save DAW auto-share setting",
            Self::SetProjectsLocation { .. } => "Save Projects location",
            Self::SetManualPublicEndpoint { .. } => "Save manual port forwarding",
            Self::Join { .. } => "Accept invitation and request pairing",
            Self::CompleteInviteLink { .. } => "Accept invite link",
            Self::Pair { .. } => "Complete pairing",
            Self::Revoke { .. } => "Revoke device",
            Self::RemoveProject { .. } => "Remove project",
            Self::RestoreProject { .. } => "Restore project",
            Self::ExportInvite { .. } => "Export invitation",
            Self::CopyInviteLink { .. } => "Copy invite link",
            Self::ExportCard => "Save pairing reply",
            Self::OpenPairing { .. } => "Open pairing window",
            Self::ClosePairing { .. } => "Close pairing window",
            Self::ApprovePairing { .. } => "Approve pairing",
            Self::RejectPairing { .. } => "Reject pairing",
            Self::RequestPairing { .. } => "Request automatic pairing",
            Self::OfferProject { .. } => "Add known device",
            Self::AcceptOffer { .. } => "Accept project offer",
            Self::DeclineOffer { .. } => "Decline project offer",
            Self::Send { .. } => "Share current version",
            Self::Check { .. } => "Check connection",
            Self::Receive { .. } => "Get collaborator's version",
            Self::UsePeerVersion { .. } => "Use collaborator's version",
            Self::Recover { .. } => "Recover installation",
        }
    }
    fn changes_setup(&self) -> bool {
        matches!(
            self,
            Self::Link { .. }
                | Self::Join { .. }
                | Self::Pair { .. }
                | Self::Revoke { .. }
                | Self::RemoveProject { .. }
                | Self::RestoreProject { .. }
                | Self::ApprovePairing { .. }
        )
    }

    fn is_parallel_projectless_setting(&self) -> bool {
        matches!(
            self,
            Self::SetDefaultSupabase { .. }
                | Self::SetAutoShare { .. }
                | Self::SetDeviceName { .. }
                | Self::SetProjectsLocation { .. }
                | Self::SetManualPublicEndpoint { .. }
        )
    }
}

#[derive(Clone, Serialize)]
pub struct Job {
    pub job_id: Uuid,
    pub project_id: Option<Uuid>,
    pub label: String,
    pub running: bool,
    pub outcome: Option<Value>,
    pub error: Option<String>,
    pub completed_at: Option<u64>,
    /// Check connection is aborted outright: it reads peer state and writes nothing durable.
    #[serde(skip)]
    pub abort: Option<tokio::task::AbortHandle>,
    /// Share and Get stop only at the core's safe points (before commit, between chunks,
    /// before install); installing files always finishes.
    #[serde(skip)]
    pub stop: Option<transfer::Stop>,
}

fn prune_finished_jobs(jobs: &mut Vec<Job>) {
    while jobs.iter().filter(|job| !job.running).count() > 32 {
        let oldest = jobs
            .iter()
            .enumerate()
            .filter(|(_, job)| !job.running)
            .min_by_key(|(_, job)| job.completed_at.unwrap_or(0))
            .map(|(index, _)| index);
        if let Some(index) = oldest {
            jobs.remove(index);
        } else {
            break;
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PairingApprovalState {
    #[default]
    Unknown,
    Pending,
    NotReachable,
    Approved,
}

#[derive(Clone, Serialize)]
pub struct PairingApproval {
    pub project_id: Uuid,
    pub device_id: Uuid,
    pub state: PairingApprovalState,
}

/// A collaborator's head that could not be pulled on two pulls in a row while this PC waits
/// for the first version; one failure right after approval is normal and is not shown.
#[derive(Clone, Serialize)]
pub struct HeadPullError {
    pub project_id: Uuid,
    pub device_id: Uuid,
    pub error: String,
}

#[derive(Clone, Serialize)]
pub struct RemovedProject {
    pub project_id: Uuid,
    pub name: String,
    pub root: String,
}

#[derive(Clone, Serialize)]
pub struct Device {
    device_id: Uuid,
    name: String,
    peer_id: String,
    /// Only for this PC: what an invite-service operator adds to its owner list.
    #[serde(skip_serializing_if = "Option::is_none")]
    public_key: Option<String>,
}

#[derive(Serialize)]
pub struct CheckView {
    pub device_id: Uuid,
    pub result: PeerCheck,
    pub fresh: bool,
    pub current_known: bool,
    pub age_seconds: u64,
}

#[derive(Serialize, Clone)]
pub struct PluginHandoff {
    pub revision: u64,
    pub project_id: Option<Uuid>,
    pub offset: u32,
}

#[derive(Serialize)]
pub struct DesktopStatus {
    pub presence: HashMap<Uuid, Vec<trackmesh_core::PresenceWarning>>,
    pub daw_plugin_settings: Vec<trackmesh_core::DawPluginSetting>,
    pub plugin_handoff: Option<PluginHandoff>,
    pub pairing: PairingStatus,
    pub listen_port: u16,
    pub configured_external_addresses: Vec<String>,
    pub ready: bool,
    pub closing: bool,
    pub device: Option<Device>,
    pub data_directory: Option<String>,
    pub projects_location: String,
    /// Projects removed with their data kept; Settings can bring them back.
    pub removed_projects: Vec<RemovedProject>,
    pub error: Option<String>,
    pub network: Option<NetworkStatus>,
    pub peer_states: HashMap<String, PeerConnectionState>,
    pub port_status: Option<PortObservationStatus>,
    pub network_observation: NetworkObservation,
    pub firewall_observation: FirewallObservation,
    pub page: Option<SummaryPage>,
    pub jobs: Vec<Job>,
    pub checks: Vec<CheckView>,
    pub pairing_approvals: Vec<PairingApproval>,
    pub head_pull_errors: Vec<HeadPullError>,
    pub project_backends: HashMap<Uuid, ProjectBackendConfig>,
    pub default_supabase: Option<SupabaseDefaultConfig>,
    pub supabase_states: HashMap<Uuid, SupabaseProjectState>,
    /// Projects this PC joined from an invite; the rest it created.
    pub joined_projects: Vec<Uuid>,
    /// For each created project, devices this PC shares another project with that can be added.
    pub offerable_devices: HashMap<Uuid, Vec<Device>>,
    pub invite_pending: bool,
    pub upnp_mapping_asked: bool,
    pub manual_public_endpoint: Option<trackmesh_core::ManualPublicEndpoint>,
    pub invite_needs_service_fields: bool,
}

struct Checked {
    result: PeerCheck,
    peer_id: String,
    frontier: Vec<(Uuid, Option<Uuid>)>,
    connected_since_check: bool,
    at: Instant,
}

#[derive(Default)]
struct Session {
    plugin_handoff: Option<PluginHandoff>,
    work_epoch: u64,
    window_session: bool,
    tray_session: bool,
    close_prompt_active: bool,
    engine: Option<Arc<Engine>>,
    device: Option<Device>,
    peer: Option<PeerRuntime>,
    network: Option<Network>,
    network_observation: NetworkObservation,
    firewall_observation: FirewallObservation,
    plugin: Option<PluginRuntime>,
    tasks: Vec<tauri::async_runtime::JoinHandle<()>>,
    jobs: Vec<Job>,
    checks: HashMap<(Uuid, Uuid), Checked>,
    ready: bool,
    closing: bool,
    closed: bool,
    error: Option<String>,
    pending_invite: Option<PendingInviteLink>,
    auto_receive_attempted: HashSet<(Uuid, Uuid)>,
    head_pulled_at: HashMap<(Uuid, Uuid), Instant>,
    /// Last failed first-version head pull and how many failed in a row.
    head_pull_errors: HashMap<(Uuid, Uuid), (String, u32)>,
    /// (project, device) pairs whose head was pulled during the current online period.
    heads_pulled_online: HashSet<(Uuid, Uuid)>,
    pairing_approval_states: HashMap<(Uuid, Uuid), PairingApprovalState>,
    pairing_approval_checked_at: HashMap<(Uuid, Uuid), Instant>,
    pairing_approval_in_flight: HashSet<(Uuid, Uuid)>,
}

struct PendingInviteLink {
    id: String,
    key: zeroize::Zeroizing<[u8; 32]>,
    service: Option<PendingInviteService>,
    parent_folder: Option<PathBuf>,
}

#[derive(Clone)]
struct PendingInviteService {
    url: String,
    publishable_key: String,
}

pub struct Desktop {
    options: Options,
    session: Mutex<Session>,
    // At most one slow status read per window; operation tasks never hold this gate.
    status_gate: tokio::sync::Mutex<()>,
}

impl Desktop {
    pub async fn snapshot_changes(
        &self,
        project: Uuid,
        snapshot: Uuid,
    ) -> Result<trackmesh_core::SnapshotChanges> {
        let engine = self.lock()?.engine.clone().context("Desktop is starting")?;
        tauri::async_runtime::spawn_blocking(move || engine.snapshot_changes(project, snapshot))
            .await?
    }
    pub async fn storage_summary(&self) -> Result<trackmesh_core::StorageSummary> {
        let engine = self.lock()?.engine.clone().context("Desktop is starting")?;
        tauri::async_runtime::spawn_blocking(move || engine.storage_summary()).await?
    }
    pub fn new(options: Options) -> Self {
        let window_session = !options.plugin_owner;
        Self {
            options,
            session: Mutex::new(Session {
                window_session,
                ..Session::default()
            }),
            status_gate: tokio::sync::Mutex::new(()),
        }
    }
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Session>> {
        self.session
            .lock()
            .map_err(|_| anyhow::anyhow!("Desktop session lock poisoned"))
    }
    fn record_head_pull(&self, project: Uuid, device: Uuid, error: Option<String>) {
        if let Ok(mut state) = self.lock() {
            match error {
                None => {
                    state.head_pull_errors.remove(&(project, device));
                }
                Some(error) => {
                    let entry = state
                        .head_pull_errors
                        .entry((project, device))
                        .or_insert((String::new(), 0));
                    *entry = (error, entry.1 + 1);
                }
            }
        }
    }
    fn set_pairing_approval(&self, project: Uuid, device: Uuid, approval: PairingApprovalState) {
        if let Ok(mut state) = self.lock() {
            state
                .pairing_approval_states
                .insert((project, device), approval);
        }
    }
    pub fn is_closed(&self) -> bool {
        self.session.lock().is_ok_and(|s| s.closed)
    }
    pub fn start(self: &Arc<Self>, app: AppHandle) {
        let desktop = self.clone();
        // Insert the handle while holding the same gate used by close requests.
        if let Ok(mut state) = self.lock() {
            state.tasks.push(tauri::async_runtime::spawn(async move {
                if let Err(error) = desktop.initialize().await {
                    if let Ok(mut state) = desktop.lock() {
                        state.error = Some(error.to_string());
                    }
                }
            }));
        }
        let desktop = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut grace = SessionGrace::default();
            let mut work_epoch = 0;
            let mut heartbeat = tokio::time::interval(Duration::from_secs(1));
            loop {
                heartbeat.tick().await;
                let request = desktop.lock().ok().and_then(|state| {
                    state
                        .plugin
                        .as_ref()
                        .and_then(PluginRuntime::take_desktop_request)
                });
                if let Some(request) = request {
                    desktop.reopen(&app);
                    let engine = desktop.lock().ok().and_then(|state| state.engine.clone());
                    let project_id = request.project_id;
                    let offset = if let (Some(engine), Some(project)) = (engine, project_id) {
                        tauri::async_runtime::spawn_blocking(move || -> Result<u32> {
                            let mut offset = 0;
                            loop {
                                let page = engine.store.projects_page(offset, 32)?;
                                if page.iter().any(|item| item.project_id == project)
                                    || page.len() < 32
                                {
                                    return Ok(offset);
                                }
                                offset = offset.checked_add(32).context("Project page overflow")?;
                            }
                        })
                        .await
                        .ok()
                        .and_then(Result::ok)
                        .unwrap_or(0)
                    } else {
                        0
                    };
                    if let Ok(mut state) = desktop.lock() {
                        if !state.closing && !state.closed {
                            state.plugin_handoff = Some(PluginHandoff {
                                revision: state
                                    .plugin_handoff
                                    .as_ref()
                                    .map_or(1, |old| old.revision.wrapping_add(1)),
                                project_id,
                                offset,
                            });
                        }
                    }
                }
                if desktop
                    .lock()
                    .map_or(true, |state| state.closing || state.closed)
                {
                    break;
                }
                let mut stay_in_tray = false;
                let should_stop = desktop.lock().is_ok_and(|mut state| {
                    if work_epoch != state.work_epoch {
                        grace = SessionGrace::default();
                        work_epoch = state.work_epoch;
                    }
                    if state.window_session
                        || state.tray_session
                        || state.close_prompt_active
                        || state.closing
                    {
                        grace = SessionGrace::default();
                        return false;
                    }
                    if state.error.is_some() {
                        state.closing = true;
                        return true;
                    }
                    let (Some(plugin), Some(peer)) = (&state.plugin, &state.peer) else {
                        return false;
                    };
                    let failed = plugin.is_finished() || peer.is_finished();
                    let busy = state.tasks.iter().any(|task| !task.inner().is_finished());
                    if !failed && grace.ready(plugin, peer, busy) {
                        stay_in_tray = true;
                        return false;
                    }
                    if failed {
                        // Same gate as desktop submit/reopen: no new operation can enter.
                        plugin.begin_close();
                        state.closing = true;
                        true
                    } else {
                        false
                    }
                });
                if stay_in_tray {
                    desktop.hide_to_tray(app.clone());
                }
                if should_stop {
                    desktop.finish_close(app.clone()).await;
                    break;
                }
            }
        });
    }
    async fn initialize(&self) -> Result<()> {
        let options = self.options.clone();
        let engine = tauri::async_runtime::spawn_blocking(move || -> Result<_> {
            let directory = options
                .data_dir
                .map(Ok)
                .unwrap_or_else(default_data_directory)?;
            // A new install starts with the Windows user name; the first screen lets the user change it.
            let device_name = if options.device_name == "My studio PC" {
                std::env::var("USERNAME")
                    .or_else(|_| std::env::var("COMPUTERNAME"))
                    .unwrap_or(options.device_name)
            } else {
                options.device_name
            };
            Ok(Arc::new(Engine::open(directory, &device_name)?))
        })
        .await??;
        let device = Device {
            device_id: engine.identity.device_id,
            name: engine.identity.name(),
            peer_id: network::peer_id(&engine.identity)?,
            public_key: Some(engine.identity.public_key()),
        };
        {
            let mut state = self.lock()?;
            state.engine = Some(engine.clone());
            state.device = Some(device);
            if state.closing {
                return Ok(());
            }
        }
        let engine_for_network = engine.clone();
        let options = self.options.clone();
        let (peer, network_observation, firewall_observation) =
            tauri::async_runtime::spawn_blocking(move || {
                let observation = observe_network();
                let firewall = observe_firewall(options.port);
                let config = NetworkConfig {
                    port: options.port,
                    external_addresses: options
                        .external_address
                        .iter()
                        .map(|v| v.parse())
                        .collect::<std::result::Result<_, _>>()?,
                };
                Ok::<_, anyhow::Error>((
                    PeerRuntime::start(engine_for_network, config, observation.clone(), true)?,
                    observation,
                    firewall,
                ))
            })
            .await??;
        let mut state = self.lock()?;
        state.network = Some(peer.network.clone());
        state.network_observation = network_observation;
        state.firewall_observation = firewall_observation;
        state.peer = Some(peer);
        state.plugin = Some(PluginRuntime::start_with_desktop(
            engine,
            state.network.clone().context("Peer runtime missing")?,
            &self.options.ipc_suffix,
        )?);
        if state.closing {
            if let Some(plugin) = &state.plugin {
                plugin.begin_close();
            }
        }
        state.ready = !state.closing;
        Ok(())
    }

    pub async fn status(&self, offset: u32) -> Result<DesktopStatus> {
        let _read = self.status_gate.lock().await;
        let engine = self.lock()?.engine.clone();
        let data_directory = engine
            .as_ref()
            .map(|engine| documents::display_path(&engine.directory));
        let projects_location = if let Some(engine) = engine.as_ref() {
            let disk = engine.clone();
            tauri::async_runtime::spawn_blocking(move || {
                Ok::<_, anyhow::Error>(disk.projects_location()?.map(PathBuf::from))
            })
            .await??
            .map(Ok)
            .unwrap_or_else(documents::default_projects_location)?
        } else {
            tauri::async_runtime::spawn_blocking(documents::default_projects_location).await??
        };
        let presence_network = self.lock()?.network.clone();
        let (
            page,
            pairing,
            project_backends,
            joined_projects,
            offerable_devices,
            default_supabase,
            daw_plugin_settings,
            presence,
        ) = match engine {
            Some(engine) => {
                tauri::async_runtime::spawn_blocking(move || -> Result<_> {
                    let page = engine.summary_page(offset)?;
                    let project_backends = page
                        .projects
                        .iter()
                        .map(|item| {
                            Ok((
                                item.project.project_id,
                                engine.project_backend_config(item.project.project_id)?,
                            ))
                        })
                        .collect::<Result<HashMap<_, _>>>()?;
                    let mut joined_projects = Vec::new();
                    let mut offerable_devices = HashMap::new();
                    for item in &page.projects {
                        let id = item.project.project_id;
                        if engine.project_joined(id)? {
                            joined_projects.push(id);
                        } else {
                            let devices = engine
                                .offerable_devices(id)?
                                .into_iter()
                                .map(|card| Device {
                                    device_id: card.device_id,
                                    name: card.name,
                                    peer_id: card.peer_id,
                                    public_key: None,
                                })
                                .collect::<Vec<_>>();
                            if !devices.is_empty() {
                                offerable_devices.insert(id, devices);
                            }
                        }
                    }
                    let default_supabase = engine.default_supabase_config()?;
                    let presence = if let Some(network) = presence_network {
                        page.projects
                            .iter()
                            .map(|project| {
                                let id = project.project.project_id;
                                Ok((id, engine.presence_warnings(id, &network)?))
                            })
                            .collect::<Result<HashMap<_, _>>>()?
                    } else {
                        HashMap::new()
                    };
                    Ok((
                        Some(page),
                        engine.pairing_status()?,
                        project_backends,
                        joined_projects,
                        offerable_devices,
                        default_supabase,
                        engine.daw_plugin_settings()?,
                        presence,
                    ))
                })
                .await??
            }
            None => (
                None,
                PairingStatus::default(),
                HashMap::new(),
                Vec::new(),
                HashMap::new(),
                None,
                Vec::new(),
                HashMap::new(),
            ),
        };
        let state_engine = { self.lock()?.engine.clone() };
        let supabase_states = match state_engine.clone() {
            Some(engine) => {
                let ids = project_backends.keys().copied().collect::<Vec<_>>();
                tauri::async_runtime::spawn_blocking(move || {
                    ids.into_iter()
                        .map(|id| Ok((id, engine.supabase_project_state(id)?)))
                        .collect::<Result<HashMap<_, _>>>()
                })
                .await??
            }
            None => HashMap::new(),
        };
        let upnp_mapping_asked = match state_engine.clone() {
            Some(engine) => {
                tauri::async_runtime::spawn_blocking(move || engine.upnp_mapping_asked()).await??
            }
            None => false,
        };
        let manual_public_endpoint = match state_engine {
            Some(engine) => {
                tauri::async_runtime::spawn_blocking(move || engine.manual_public_endpoint())
                    .await??
            }
            None => None,
        };
        let mut state = self.lock()?;
        let invite_needs_service_fields = state
            .pending_invite
            .as_ref()
            .is_some_and(|pending| pending.service.is_none())
            && default_supabase.is_none();
        let mut network = state.network.as_ref().map(Network::status);
        if state.peer.as_ref().is_some_and(PeerRuntime::is_finished) {
            if let Some(network) = &mut network {
                network.peers.clear();
                network.addresses.clear();
                network.error = Some("Peer runtime stopped".to_owned());
            }
        }
        // Keep known lineage visible. A disconnect invalidates equality even after reconnect.
        for checked in state.checks.values_mut() {
            checked.connected_since_check &= network
                .as_ref()
                .is_some_and(|net| net.peers.get(&checked.peer_id).is_some_and(|p| p.online));
        }
        let checks = state
            .checks
            .iter()
            .filter(|((project, _), _)| {
                page.as_ref().is_some_and(|page| {
                    page.projects
                        .iter()
                        .any(|p| p.project.project_id == *project)
                })
            })
            .map(|((_, device_id), checked)| {
                let head_matches = page.as_ref().is_some_and(|page| {
                    page.projects.iter().any(|p| {
                        p.project.project_id == checked.result.project_id
                            && p.project.latest_local_snapshot == checked.result.local_snapshot
                            && metadata_frontier(p) == checked.frontier
                    })
                });
                CheckView {
                    device_id: *device_id,
                    result: checked.result.clone(),
                    fresh: !state.closing
                        && head_matches
                        && checked.connected_since_check
                        && checked.at.elapsed() < Duration::from_secs(30),
                    current_known: head_matches,
                    age_seconds: checked.at.elapsed().as_secs(),
                }
            })
            .collect();
        if let Some(network) = &mut network {
            network.peers.retain(|id, _| {
                page.as_ref().is_some_and(|page| {
                    page.projects
                        .iter()
                        .any(|project| project.peers.iter().any(|peer| peer.device.peer_id == *id))
                })
            });
            network.addresses.truncate(32);
            network.received_snapshots.retain(|receipt| {
                page.as_ref().is_some_and(|page| {
                    page.projects.iter().any(|project| {
                        project.project.project_id == receipt.project_id
                            && project
                                .peers
                                .iter()
                                .any(|peer| peer.device.peer_id == receipt.peer_id)
                    })
                })
            });
        }
        let runtime_error = state
            .peer
            .as_ref()
            .filter(|peer| peer.is_finished())
            .map(|_| "Peer runtime stopped; reopen the desktop to reconnect".to_owned());
        let peer_states = page
            .as_ref()
            .zip(network.as_ref())
            .map(|(page, network)| {
                page.projects
                    .iter()
                    .flat_map(|project| project.peers.iter())
                    .map(|peer| {
                        (
                            peer.device.peer_id.clone(),
                            peer_connection_state(network, &peer.device.peer_id),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let port_status = network.as_ref().map(port_observation_status);
        let device = state.device.clone().map(|mut device| {
            if let Some(engine) = &state.engine {
                device.name = engine.identity.name();
            }
            device
        });
        let pairing_approvals = page
            .as_ref()
            .into_iter()
            .flat_map(|page| page.projects.iter())
            .flat_map(|project| {
                project.peers.iter().map(|peer| PairingApproval {
                    project_id: project.project.project_id,
                    device_id: peer.device.device_id,
                    state: state
                        .pairing_approval_states
                        .get(&(project.project.project_id, peer.device.device_id))
                        .copied()
                        .unwrap_or_default(),
                })
            })
            .collect();
        Ok(DesktopStatus {
            presence,
            daw_plugin_settings,
            plugin_handoff: state.plugin_handoff.clone(),
            pairing,
            listen_port: self.options.port,
            configured_external_addresses: self.options.external_address.clone(),
            ready: state.ready,
            closing: state.closing,
            device,
            data_directory,
            projects_location: documents::display_path(&projects_location),
            removed_projects: state
                .engine
                .as_ref()
                .and_then(|engine| engine.store.removed_projects().ok())
                .unwrap_or_default()
                .into_iter()
                .map(|project| RemovedProject {
                    project_id: project.project_id,
                    name: project.name,
                    root: documents::display_path(std::path::Path::new(&project.root)),
                })
                .collect(),
            error: state.error.clone().or(runtime_error),
            network,
            peer_states,
            port_status,
            network_observation: state
                .peer
                .as_ref()
                .map(PeerRuntime::network_observation)
                .unwrap_or_else(|| state.network_observation.clone()),
            firewall_observation: state.firewall_observation.clone(),
            page,
            jobs: state.jobs.clone(),
            checks,
            pairing_approvals,
            head_pull_errors: state
                .head_pull_errors
                .iter()
                .filter(|(_, (_, failures))| *failures >= 2)
                .map(|(&(project_id, device_id), (error, _))| HeadPullError {
                    project_id,
                    device_id,
                    error: error.clone(),
                })
                .collect(),
            project_backends,
            default_supabase,
            supabase_states,
            joined_projects,
            offerable_devices,
            invite_pending: state.pending_invite.is_some(),
            upnp_mapping_asked,
            invite_needs_service_fields,
            manual_public_endpoint,
        })
    }

    pub async fn try_auto_first_receive(
        self: &Arc<Self>,
        app: AppHandle,
        status: &DesktopStatus,
    ) -> Result<()> {
        let Some(page) = status.page.as_ref() else {
            return Ok(());
        };
        let (network, engine) = {
            let state = self.lock()?;
            let (Some(network), Some(engine)) = (&state.network, &state.engine) else {
                return Ok(());
            };
            (network.clone(), engine.clone())
        };
        for item in &page.projects {
            // A version shared while the PCs were disconnected is announced only at share time.
            // Pull each collaborator's head once whenever they come back online, so the card can
            // show their newer version. Once per online period keeps idle owners free to exit.
            if item.project.latest_local_snapshot.is_some()
                && !item.project.recovery_required
                && !item.active_transfer
            {
                for peer in &item.peers {
                    let key = (item.project.project_id, peer.device.device_id);
                    let online = network.online(&peer.device.peer_id);
                    let pull = {
                        let mut state = self.lock()?;
                        if !online {
                            state.heads_pulled_online.remove(&key);
                            false
                        } else if state.jobs.iter().any(|job| {
                            job.running && job.project_id == Some(item.project.project_id)
                        }) {
                            false
                        } else {
                            state.heads_pulled_online.insert(key)
                        }
                    };
                    if pull {
                        let (engine, network) = (engine.clone(), network.clone());
                        let (project, source) =
                            (item.project.project_id, peer.device.peer_id.clone());
                        tauri::async_runtime::spawn(async move {
                            let _ = tokio::time::timeout(
                                Duration::from_secs(30),
                                engine.fetch_snapshot_chain(&network, project, &source, None),
                            )
                            .await;
                        });
                    }
                }
            }
            if item.local_snapshot.is_some()
                || item.project.latest_local_snapshot.is_some()
                || item.project.recovery_required
                || !item.root_available
                || !item.root_empty
                || item.active_transfer
            {
                continue;
            }
            if self.lock()?.jobs.iter().any(|job| {
                job.running
                    && (job.project_id.is_none() || job.project_id == Some(item.project.project_id))
            }) {
                continue;
            }
            for peer in &item.peers {
                if !network.online(&peer.device.peer_id) {
                    // An offline collaborator is shown as offline, not as a failed pull.
                    self.record_head_pull(item.project.project_id, peer.device.device_id, None);
                    continue;
                }
                // The peer's head, not its newest own version: a head signed by another
                // collaborator would otherwise leave this folder on an older version.
                let Some(head) = peer.fetched_head else {
                    // Nothing pushes the collaborator's head; pull it with
                    // the existing Head request, at most every 10 s per project and peer.
                    let key = (item.project.project_id, peer.device.device_id);
                    {
                        let mut state = self.lock()?;
                        if state
                            .head_pulled_at
                            .get(&key)
                            .is_some_and(|at| at.elapsed() < Duration::from_secs(10))
                        {
                            continue;
                        }
                        state.head_pulled_at.insert(key, Instant::now());
                    }
                    let (engine, network) = (engine.clone(), network.clone());
                    let (project, source) = (item.project.project_id, peer.device.peer_id.clone());
                    let desktop = self.clone();
                    let device = peer.device.device_id;
                    tauri::async_runtime::spawn(async move {
                        let pulled = engine
                            .fetch_snapshot_chain(&network, project, &source, None)
                            .await;
                        if pulled.is_ok() {
                            desktop.set_pairing_approval(
                                project,
                                device,
                                PairingApprovalState::Approved,
                            );
                        }
                        desktop.record_head_pull(
                            project,
                            device,
                            pulled.err().map(|error| format!("{error:#}")),
                        );
                    });
                    continue;
                };
                let key = (item.project.project_id, head);
                {
                    let mut state = self.lock()?;
                    if state.auto_receive_attempted.contains(&key)
                        || state.jobs.iter().any(|job| {
                            job.running
                                && (job.project_id.is_none()
                                    || job.project_id == Some(item.project.project_id))
                        })
                    {
                        continue;
                    }
                    state.auto_receive_attempted.insert(key);
                }
                // Uses the same bounded receive job path as the manual Get action.
                let _ = self.submit(
                    app.clone(),
                    Action::Receive {
                        project_id: item.project.project_id,
                        device_id: peer.device.device_id,
                        snapshot_id: Some(head),
                        daw_closed: true,
                        automatic: true,
                    },
                );
                break;
            }
        }
        Ok(())
    }

    pub async fn try_pairing_approval_status(
        self: &Arc<Self>,
        status: &DesktopStatus,
    ) -> Result<()> {
        let Some(page) = status.page.as_ref() else {
            return Ok(());
        };
        let (network, engine) = {
            let state = self.lock()?;
            let (Some(network), Some(engine)) = (&state.network, &state.engine) else {
                return Ok(());
            };
            (network.clone(), engine.clone())
        };
        for item in &page.projects {
            if item.local_snapshot.is_some()
                || item.project.latest_local_snapshot.is_some()
                || item.active_transfer
            {
                continue;
            }
            for peer in &item.peers {
                if peer.head_fetched {
                    continue;
                }
                let key = (item.project.project_id, peer.device.device_id);
                {
                    let mut state = self.lock()?;
                    // An expired or closed window drops the connection, so a pending
                    // receiver must keep asking to learn that approval is no longer coming.
                    let approval = state
                        .pairing_approval_states
                        .get(&key)
                        .copied()
                        .unwrap_or_default();
                    if approval == PairingApprovalState::Approved
                        || (approval != PairingApprovalState::Pending
                            && !network.online(&peer.device.peer_id))
                        || state.pairing_approval_in_flight.contains(&key)
                        || state.jobs.iter().any(|job| {
                            job.running && job.project_id == Some(item.project.project_id)
                        })
                        || state
                            .pairing_approval_checked_at
                            .get(&key)
                            .is_some_and(|at| at.elapsed() < Duration::from_secs(10))
                    {
                        continue;
                    }
                    state.pairing_approval_in_flight.insert(key);
                }
                let desktop = self.clone();
                let engine = engine.clone();
                let network = network.clone();
                let (project, device) = key;
                tauri::async_runtime::spawn(async move {
                    let approval = match engine.pairing_approval(&network, project, device).await {
                        Ok(protocol::PairingResponse::Approved) => PairingApprovalState::Approved,
                        Ok(protocol::PairingResponse::Pending) => PairingApprovalState::Pending,
                        _ => PairingApprovalState::NotReachable,
                    };
                    if let Ok(mut state) = desktop.lock() {
                        // A Check, head pull or receive may have proven approval meanwhile.
                        let current = state
                            .pairing_approval_states
                            .entry((project, device))
                            .or_default();
                        if *current != PairingApprovalState::Approved {
                            *current = approval;
                        }
                        state.pairing_approval_in_flight.remove(&(project, device));
                        state
                            .pairing_approval_checked_at
                            .insert((project, device), Instant::now());
                    }
                });
            }
        }
        Ok(())
    }

    pub fn submit(self: &Arc<Self>, app: AppHandle, action: Action) -> Result<Uuid> {
        let mut state = self.lock()?;
        ensure!(
            state.ready && !state.closing && !state.close_prompt_active,
            "Desktop is starting or closing"
        );
        let project = action.project();
        let parallel_setting = action.is_parallel_projectless_setting();
        let conflicting = state.jobs.iter().any(|job| {
            job.running
                && if parallel_setting {
                    job.project_id.is_none()
                } else {
                    job.project_id.is_none() || project.is_none() || job.project_id == project
                }
        });
        if conflicting {
            let projectless_conflict = project.is_none()
                || state
                    .jobs
                    .iter()
                    .any(|job| job.running && job.project_id.is_none());
            anyhow::ensure!(
                !conflicting,
                if projectless_conflict {
                    "Wait for the current operation to finish"
                } else {
                    "This project already has an active operation"
                }
            );
        }
        ensure!(
            state.jobs.iter().filter(|job| job.running).count() < 8,
            "Too many active operations"
        );
        state.work_epoch = state.work_epoch.wrapping_add(1);
        state
            .jobs
            .retain(|job| job.running || job.project_id != project);
        prune_finished_jobs(&mut state.jobs);
        state.tasks.retain(|task| !task.inner().is_finished());
        let job_id = Uuid::new_v4();
        state.jobs.push(Job {
            job_id,
            project_id: project,
            label: action.label().to_owned(),
            running: true,
            outcome: None,
            error: None,
            completed_at: None,
            abort: None,
            stop: None,
        });
        let abortable = matches!(action, Action::Check { .. });
        let stop = transfer::Stop::default();
        let stopped_message = match action {
            Action::Check { .. } => Some("Connection check stopped"),
            Action::Send { .. } => Some("Sharing stopped. Nothing was shared."),
            Action::Receive { .. } | Action::UsePeerVersion { .. } => {
                Some("Getting stopped. Anything already downloaded is kept for Resume getting.")
            }
            _ => None,
        };
        if stopped_message.is_some() && !abortable {
            if let Some(job) = state.jobs.iter_mut().find(|job| job.job_id == job_id) {
                job.stop = Some(stop.clone());
            }
        }
        let desktop = self.clone();
        let engine = state.engine.clone().context("Engine is unavailable")?;
        let network = state.network.clone().context("Network is unavailable")?;
        state.tasks.push(tauri::async_runtime::spawn(async move {
            let worker_desktop = desktop.clone();
            // Join the inner task so a panic becomes a visible completed job, never a stuck gate.
            let worker = tokio::spawn(async move {
                worker_desktop
                    .execute(app, engine, network, action, stop)
                    .await
            });
            if abortable {
                if let Ok(mut state) = desktop.lock() {
                    if let Some(job) = state.jobs.iter_mut().find(|job| job.job_id == job_id) {
                        job.abort = Some(worker.abort_handle());
                    }
                }
            }
            let result = match worker.await {
                Ok(Err(error)) if error.downcast_ref::<transfer::Stopped>().is_some() => {
                    Ok(json!({"message": stopped_message, "stopped": true}))
                }
                Ok(result) => result,
                Err(error) if error.is_cancelled() => {
                    Ok(json!({"message": stopped_message, "stopped": true}))
                }
                Err(_) => Err(anyhow::anyhow!("Operation worker stopped unexpectedly")),
            };
            if let Ok(mut state) = desktop.lock() {
                if let Some(job) = state.jobs.iter_mut().find(|job| job.job_id == job_id) {
                    job.running = false;
                    job.abort = None;
                    job.stop = None;
                    job.completed_at = Some(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                    );
                    match result {
                        Ok(outcome) => job.outcome = Some(outcome),
                        Err(error) => job.error = Some(error.to_string()),
                    }
                }
                prune_finished_jobs(&mut state.jobs);
            }
        }));
        Ok(job_id)
    }

    pub async fn set_upnp_mapping(&self, enabled: bool) -> Result<()> {
        let (engine, network) = {
            let state = self.lock()?;
            ensure!(
                state.ready && !state.closing,
                "Desktop is starting or closing"
            );
            (
                state.engine.clone().context("Engine is unavailable")?,
                state.network.clone().context("Network is unavailable")?,
            )
        };
        let disk = engine.clone();
        tauri::async_runtime::spawn_blocking(move || disk.set_upnp_mapping_enabled(enabled))
            .await??;
        network.set_mapping_consent(enabled);
        Ok(())
    }

    pub fn stop_operation(&self, project_id: Uuid) -> Result<()> {
        let state = self.lock()?;
        let job = state
            .jobs
            .iter()
            .find(|job| job.running && job.project_id == Some(project_id))
            .context("Nothing is running for this project")?;
        if let Some(abort) = &job.abort {
            abort.abort();
        } else if let Some(stop) = &job.stop {
            stop.request();
        } else {
            anyhow::bail!("This operation can't be stopped");
        }
        Ok(())
    }

    pub fn cancel_invite_link(&self) -> Result<()> {
        let mut state = self.lock()?;
        ensure!(
            !state
                .jobs
                .iter()
                .any(|job| job.running && job.label == "Accept invite link"),
            "An invite is being accepted; wait for it to finish"
        );
        ensure!(
            state.pending_invite.take().is_some(),
            "No invite link is open"
        );
        Ok(())
    }

    pub async fn open_project_folder(&self, project_id: Uuid) -> Result<()> {
        let engine = self
            .lock()?
            .engine
            .clone()
            .context("Engine is unavailable")?;
        let path = tauri::async_runtime::spawn_blocking(move || {
            Ok::<_, anyhow::Error>(PathBuf::from(engine.store.project(project_id)?.root))
        })
        .await??;
        tauri::async_runtime::spawn_blocking(move || documents::open_in_explorer(&path)).await??;
        Ok(())
    }

    pub async fn open_previous_version(&self, project_id: Uuid, transfer_id: Uuid) -> Result<()> {
        let engine = self
            .lock()?
            .engine
            .clone()
            .context("Engine is unavailable")?;
        let path = tauri::async_runtime::spawn_blocking(move || -> Result<PathBuf> {
            // Same location the apply journal uses: a sibling of the project folder.
            let root = PathBuf::from(engine.store.project(project_id)?.root);
            let stage = transfer::stage_path(&root, project_id, transfer_id)?;
            let backup = stage.join("backup");
            ensure!(backup.is_dir(), "Previous version backup is not available");
            Ok(backup)
        })
        .await??;
        tauri::async_runtime::spawn_blocking(move || documents::open_in_explorer(&path)).await??;
        Ok(())
    }

    pub async fn open_projects_location(&self) -> Result<()> {
        let engine = self
            .lock()?
            .engine
            .clone()
            .context("Engine is unavailable")?;
        let path =
            tauri::async_runtime::spawn_blocking(move || match engine.projects_location()? {
                Some(path) => Ok(PathBuf::from(path)),
                None => documents::default_projects_location(),
            })
            .await??;
        tauri::async_runtime::spawn_blocking(move || documents::open_in_explorer(&path)).await??;
        Ok(())
    }

    pub async fn choose_projects_location(&self, app: AppHandle) -> Result<Option<String>> {
        let engine = self
            .lock()?
            .engine
            .clone()
            .context("Engine is unavailable")?;
        tauri::async_runtime::spawn_blocking(move || {
            let Some(path) = documents::choose_folder(&app)? else {
                return Ok(None);
            };
            engine.set_projects_location(std::path::Path::new(&path))?;
            Ok(Some(path))
        })
        .await?
    }

    pub fn set_invite_parent_folder(&self, folder: PathBuf) -> Result<String> {
        let folder =
            std::fs::canonicalize(folder).context("Selected folder could not be resolved")?;
        ensure!(folder.is_dir(), "Choose a folder for the project");
        let mut state = self.lock()?;
        let pending = state
            .pending_invite
            .as_mut()
            .context("No invitation link is waiting")?;
        pending.parent_folder = Some(folder.clone());
        Ok(documents::display_path(&folder))
    }

    pub async fn open_invite_parent_folder(&self) -> Result<()> {
        let (engine, selected) = {
            let state = self.lock()?;
            (
                state.engine.clone().context("Engine is unavailable")?,
                state
                    .pending_invite
                    .as_ref()
                    .and_then(|pending| pending.parent_folder.clone()),
            )
        };
        let path = if let Some(path) = selected {
            path
        } else {
            tauri::async_runtime::spawn_blocking(move || match engine.projects_location()? {
                Some(path) => Ok(PathBuf::from(path)),
                None => documents::default_projects_location(),
            })
            .await??
        };
        tauri::async_runtime::spawn_blocking(move || documents::open_in_explorer(&path)).await??;
        Ok(())
    }

    async fn execute(
        &self,
        app: AppHandle,
        engine: Arc<Engine>,
        network: Network,
        action: Action,
        stop: transfer::Stop,
    ) -> Result<Value> {
        let reload = action.changes_setup();
        let outcome = match action {
            Action::Join { folder } => {
                let disk = engine.clone();
                let (project, device) =
                    tauri::async_runtime::spawn_blocking(move || -> Result<_> {
                        let invite: protocol::ProjectInvite = documents::read_document(&app)?;
                        let project = invite.project_id;
                        let device = invite.device.device_id;
                        disk.import_invite(invite, &folder)?;
                        Ok((project, device))
                    })
                    .await??;
                network
                    .reload()
                    .await
                    .context("Invitation accepted, but network reload failed; reopen desktop")?;
                // Import is committed even when the inviter is offline or their window is closed.
                // Keep a retry action instead of asking the user to reimport the project.
                self.automatic_pairing(&engine, &network, project, device, true)
                    .await
            }
            Action::CompleteInviteLink {
                supabase_url,
                supabase_anon_key,
            } => {
                let (id, key, pending_parent, linked_service) = {
                    let state = self.lock()?;
                    let pending = state
                        .pending_invite
                        .as_ref()
                        .context("No invitation link is waiting")?;
                    (
                        pending.id.clone(),
                        pending.key.clone(),
                        pending.parent_folder.clone(),
                        pending.service.clone(),
                    )
                };
                let disk = engine.clone();
                let configured =
                    tauri::async_runtime::spawn_blocking(move || disk.default_supabase_config())
                        .await??;
                let save_default_after_validation = configured.is_none();
                let config = match (linked_service, configured) {
                    (Some(service), _) => SupabaseDefaultConfig {
                        url: service.url,
                        publishable_key: service.publishable_key,
                    },
                    (None, Some(config)) => config,
                    // Saved as the default only after the signed invite confirms this service.
                    (None, None) => SupabaseDefaultConfig {
                        url: supabase_url.context("Enter the Supabase Project URL")?,
                        publishable_key: supabase_anon_key
                            .context("Enter the Supabase publishable key")?,
                    },
                };
                let parent = if let Some(parent) = pending_parent {
                    parent
                } else if let Some(location) = engine.projects_location()? {
                    PathBuf::from(location)
                } else {
                    tauri::async_runtime::spawn_blocking(documents::default_projects_location)
                        .await??
                };
                let parent = tauri::async_runtime::spawn_blocking(move || -> Result<PathBuf> {
                    std::fs::create_dir_all(&parent)
                        .context("The selected Projects location could not be created")?;
                    let parent = std::fs::canonicalize(parent)
                        .context("The selected Projects location could not be resolved")?;
                    ensure!(parent.is_dir(), "Choose a folder for the project");
                    Ok(parent)
                })
                .await??;
                let downloaded = supabase_client::SupabaseClient::consume_invite_drop(
                    &config.url,
                    &config.publishable_key,
                    &id,
                )
                .await
                .map_err(|_| anyhow::anyhow!("Invite link is invalid, expired, or unavailable"))?;
                let ciphertext = zeroize::Zeroizing::new(downloaded.ciphertext);
                let mut invite =
                    crypto::decrypt_invite_drop(&id, &downloaded.expires_at, &ciphertext, &key)
                        .map_err(|_| {
                            anyhow::anyhow!("Invite link is invalid, expired, or unavailable")
                        })?;
                if crypto::verify_project_invite(&invite).is_err() {
                    crypto::zeroize_project_invite(&mut invite);
                    anyhow::bail!("Invite link is invalid, expired, or unavailable");
                }
                let signed_backend_matches = invite.backend.as_ref().is_some_and(|backend| {
                    backend.backend == protocol::InviteBackend::Supabase
                        && backend.supabase_anon_key.as_str() == config.publishable_key.as_str()
                        && supabase_client::supabase_project_host(&backend.supabase_url).ok()
                            == supabase_client::supabase_project_host(&config.url).ok()
                });
                if !signed_backend_matches {
                    crypto::zeroize_project_invite(&mut invite);
                    anyhow::bail!("Invite link is invalid, expired, or unavailable");
                }
                if save_default_after_validation {
                    let disk = engine.clone();
                    let saved = config.clone();
                    tauri::async_runtime::spawn_blocking(move || {
                        disk.set_default_supabase_config(Some(saved))
                    })
                    .await??;
                }
                if let Ok(mut state) = self.lock() {
                    state.pending_invite = None;
                }
                let name = sanitize_project_name(&invite.project_name);
                let disk = engine.clone();
                let known_project = invite.project_id;
                // A project this PC had before goes back to its old folder.
                let folder = tauri::async_runtime::spawn_blocking(move || {
                    match disk.rejoin_folder(known_project)? {
                        Some(root) => Ok(root),
                        None => create_unique_invite_folder(&parent, &name),
                    }
                })
                .await;
                let folder = match folder {
                    Ok(Ok(folder)) => folder,
                    Ok(Err(error)) => {
                        crypto::zeroize_project_invite(&mut invite);
                        return Err(error);
                    }
                    Err(error) => {
                        crypto::zeroize_project_invite(&mut invite);
                        return Err(error.into());
                    }
                };
                let project = invite.project_id;
                let device = invite.device.device_id;
                let disk = engine.clone();
                tauri::async_runtime::spawn_blocking(move || disk.import_invite(invite, &folder))
                    .await??;
                network
                    .reload()
                    .await
                    .context("Invite accepted, but network reload failed; reopen desktop")?;
                self.automatic_pairing(&engine, &network, project, device, true)
                    .await
            }
            Action::RequestPairing {
                project_id,
                device_id,
            } => {
                self.automatic_pairing(&engine, &network, project_id, device_id, false)
                    .await
            }
            Action::OfferProject {
                project_id,
                device_id,
            } => {
                let disk = engine.clone();
                let (had_snapshot, root_empty) = tauri::async_runtime::spawn_blocking(move || {
                    let summary = disk.project_summary(project_id)?;
                    Ok::<_, anyhow::Error>((summary.local_snapshot.is_some(), summary.root_empty))
                })
                .await??;
                let addresses = invite_addresses(
                    &engine,
                    &network,
                    &self.options.external_address,
                    Vec::new(),
                )
                .await?;
                let response = engine
                    .offer_project(&network, project_id, device_id, addresses)
                    .await
                    .context("The other PC could not be reached. Both PCs need TrackMesh open and this TrackMesh version.")?;
                ensure!(
                    matches!(response, protocol::PairingResponse::Pending),
                    "The other PC did not take the offer. Both PCs need this TrackMesh version, and the project must not already be on that PC."
                );
                // As with Approve: a project with files but no version yet shares one, so the
                // device gets a first version once it accepts.
                if !had_snapshot && !root_empty {
                    engine.send_snapshot(&network, project_id).await?;
                }
                json!({"message": "Offer sent. The other PC accepts it in TrackMesh.", "offered_device": device_id})
            }
            Action::AcceptOffer { project_id } => {
                let disk = engine.clone();
                let parent = match disk.projects_location()? {
                    Some(location) => PathBuf::from(location),
                    None => {
                        tauri::async_runtime::spawn_blocking(documents::default_projects_location)
                            .await??
                    }
                };
                let disk = engine.clone();
                let (project, device) =
                    tauri::async_runtime::spawn_blocking(move || -> Result<_> {
                        std::fs::create_dir_all(&parent)
                            .context("The selected Projects location could not be created")?;
                        let invite = disk.take_offer(project_id)?;
                        let name = sanitize_project_name(&invite.project_name);
                        // A project this PC had before goes back to its old folder.
                        let known = disk.rejoin_folder(invite.project_id);
                        let folder = match known.and_then(|known| match known {
                            Some(root) => Ok(root),
                            None => create_unique_invite_folder(&parent, &name),
                        }) {
                            Ok(folder) => folder,
                            Err(error) => {
                                let mut invite = invite;
                                crypto::zeroize_project_invite(&mut invite);
                                return Err(error);
                            }
                        };
                        let project = invite.project_id;
                        let device = invite.device.device_id;
                        disk.import_invite(invite, &folder)?;
                        Ok((project, device))
                    })
                    .await??;
                network
                    .reload()
                    .await
                    .context("Project added, but network reload failed; reopen desktop")?;
                self.automatic_pairing(&engine, &network, project, device, true)
                    .await
            }
            Action::DeclineOffer { project_id } => {
                let disk = engine.clone();
                tauri::async_runtime::spawn_blocking(move || disk.decline_offer(project_id))
                    .await??;
                json!({"message": "Offer declined"})
            }
            Action::Send { project_id } => {
                let disk = engine.clone();
                let net = network.clone();
                let dialog_app = app.clone();
                let accepted = tauri::async_runtime::spawn_blocking(move || -> Result<bool> {
                    let warnings = disk.presence_warnings(project_id, &net)?;
                    if warnings.is_empty() {
                        return Ok(true);
                    }
                    Ok(dialog_app
                        .dialog()
                        .message(
                            warnings
                                .iter()
                                .map(|warning| warning.share_confirmation.as_str())
                                .collect::<Vec<_>>()
                                .join("\n\n"),
                        )
                        .title(tr("Share current version?", "Güncel sürüm paylaşılsın mı?"))
                        .kind(tauri_plugin_dialog::MessageDialogKind::Warning)
                        .buttons(tauri_plugin_dialog::MessageDialogButtons::OkCancelCustom(
                            tr("Share", "Paylaş").to_owned(),
                            tr("Cancel", "Vazgeç").to_owned(),
                        ))
                        .blocking_show())
                })
                .await??;
                if accepted {
                    serde_json::to_value(
                        engine
                            .send_snapshot_until(&network, project_id, stop)
                            .await?,
                    )?
                } else {
                    json!({"message": "Sharing cancelled"})
                }
            }
            Action::ApprovePairing {
                project_id,
                request_id,
            } => {
                let disk = engine.clone();
                let (had_snapshot, root_empty) = tauri::async_runtime::spawn_blocking(move || {
                    let summary = disk.project_summary(project_id)?;
                    Ok::<_, anyhow::Error>((summary.local_snapshot.is_some(), summary.root_empty))
                })
                .await??;
                let disk = engine.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    disk.approve_pairing(project_id, request_id)
                })
                .await??;
                if !had_snapshot && root_empty {
                    json!({"message": "Collaborator approved. Add your project files, then Share."})
                } else if !had_snapshot {
                    let shared = engine.send_snapshot(&network, project_id).await?;
                    json!({"message": "Collaborator approved. First version shared and ready to receive.", "snapshot_id": shared.snapshot_id})
                } else {
                    json!({"message": "Collaborator approved for this project."})
                }
            }
            Action::CopyInviteLink { project_id } => {
                let disk = engine.clone();
                let config = tauri::async_runtime::spawn_blocking(move || {
                    disk.project_backend_config(project_id)
                })
                .await??;
                ensure!(
                    config.backend == ProjectBackend::Supabase,
                    "Select and save Supabase for this project before creating an invite link"
                );
                let url = config.supabase_url.context("Supabase is not configured")?;
                let anon_key = config
                    .supabase_anon_key
                    .context("Supabase is not configured")?;
                supabase_client::validate_supabase_project_url(&url)?;
                supabase_client::validate_supabase_publishable_key(&anon_key)?;
                let ref_name = invite_service_ref(&url)?;
                wait_for_mapping_if_requested(&network).await?;
                let addresses = invite_addresses(
                    &engine,
                    &network,
                    &self.options.external_address,
                    Vec::new(),
                )
                .await?;
                let disk = engine.clone();
                let mut invite = tauri::async_runtime::spawn_blocking(move || {
                    disk.export_invite(project_id, addresses)
                })
                .await??;
                let sealed_result = crypto::encrypt_invite_drop(&invite);
                crypto::zeroize_project_invite(&mut invite);
                let sealed = sealed_result?;
                let client = supabase_client::SupabaseClient::new(&url, &anon_key, project_id)?;
                let accepted = client
                    .create_invite_drop(
                        &engine.identity,
                        &sealed.id,
                        &sealed.expires_at,
                        &sealed.ciphertext,
                    )
                    .await
                    .map_err(|_| anyhow::anyhow!("Invite link could not be created"))?;
                ensure!(accepted, "Invite link could not be created");
                let encoded_key = zeroize::Zeroizing::new(base64::Engine::encode(
                    &base64::engine::general_purpose::URL_SAFE_NO_PAD,
                    sealed.key.as_slice(),
                ));
                let link = zeroize::Zeroizing::new(format!(
                    "trackmesh://invite/v2/{}/{}/{}#{}",
                    ref_name,
                    anon_key,
                    sealed.id,
                    encoded_key.as_str(),
                ));
                let disk = engine.clone();
                let replaced = tauri::async_runtime::spawn_blocking(move || {
                    disk.open_pairing_window(project_id)
                })
                .await??;
                let cleared_pending = match replaced {
                    Some(other) => Some(engine.store.project(other)?.name),
                    None => None,
                };
                network
                    .pairing_window(trackmesh_core::PAIRING_WINDOW)
                    .await?;
                let clipboard_value = zeroize::Zeroizing::new(link.to_string());
                let sequence = tauri::async_runtime::spawn_blocking(move || {
                    crate::clipboard::copy_invite_link(&clipboard_value)
                })
                .await??;
                let cleanup_value = zeroize::Zeroizing::new(link.to_string());
                let cleanup = tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    let _ = tauri::async_runtime::spawn_blocking(move || {
                        crate::clipboard::clear_if_matches(&cleanup_value, sequence)
                    })
                    .await;
                });
                if let Ok(mut state) = self.lock() {
                    state.tasks.push(cleanup);
                }
                json!({"message": tr(
                    "Link copied. Valid for 15 minutes and works once. For safety it is removed from the clipboard after 60 s.",
                    "Bağlantı kopyalandı. 15 dakika ve bir kez geçerli. Güvenlik için 60 sn sonra panodan silinir.",
                ), "invite_copied": true, "cleared_pending": cleared_pending})
            }
            Action::Check {
                project_id,
                device_id,
            } => {
                let disk = engine.clone();
                let peer = tauri::async_runtime::spawn_blocking(move || {
                    disk.paired_peer(project_id, device_id)
                })
                .await??;
                wait_for_peer(self, &network, &peer.peer_id).await?;
                let result = engine
                    .check_peer(&network, project_id, &peer.peer_id)
                    .await?;
                self.set_pairing_approval(project_id, device_id, PairingApprovalState::Approved);
                let outcome = serde_json::to_value(&result)?;
                let disk = engine.clone();
                let summary =
                    tauri::async_runtime::spawn_blocking(move || disk.project_summary(project_id))
                        .await??;
                let mut state = self.lock()?;
                if state.checks.len() >= 64 {
                    if let Some(oldest) = state
                        .checks
                        .iter()
                        .min_by_key(|(_, check)| check.at)
                        .map(|(key, _)| *key)
                    {
                        state.checks.remove(&oldest);
                    }
                }
                state.checks.insert(
                    (project_id, device_id),
                    Checked {
                        result,
                        peer_id: peer.peer_id,
                        frontier: metadata_frontier(&summary),
                        connected_since_check: true,
                        at: Instant::now(),
                    },
                );
                outcome
            }
            Action::Receive {
                project_id,
                device_id,
                snapshot_id,
                daw_closed,
                automatic,
            } => {
                ensure!(daw_closed, "Close the DAW project before receiving");
                let disk = engine.clone();
                let peer = tauri::async_runtime::spawn_blocking(move || {
                    disk.paired_peer(project_id, device_id)
                })
                .await??;
                transfer::or_stop(&stop, wait_for_peer(self, &network, &peer.peer_id)).await?;
                let peer_device = peer.device_id;
                match self
                    .receive_with_confirmation(
                        &app,
                        &engine,
                        &network,
                        project_id,
                        peer,
                        snapshot_id,
                        false,
                        !automatic,
                        &stop,
                    )
                    .await
                {
                    Ok(transfer_id) => {
                        self.set_pairing_approval(
                            project_id,
                            peer_device,
                            PairingApprovalState::Approved,
                        );
                        json!({"transfer_id": transfer_id})
                    }
                    Err(error) => receive_blocked_or_error(error)?,
                }
            }
            Action::UsePeerVersion {
                project_id,
                device_id,
                daw_closed,
            } => {
                ensure!(daw_closed, "Close the DAW project before receiving");
                let disk = engine.clone();
                let peer = tauri::async_runtime::spawn_blocking(move || {
                    disk.paired_peer(project_id, device_id)
                })
                .await??;
                transfer::or_stop(&stop, wait_for_peer(self, &network, &peer.peer_id)).await?;
                match self
                    .receive_with_confirmation(
                        &app, &engine, &network, project_id, peer, None, true, true, &stop,
                    )
                    .await
                {
                    Ok(transfer_id) => json!({"transfer_id": transfer_id}),
                    Err(error) => receive_blocked_or_error(error)?,
                }
            }
            Action::Recover {
                project_id,
                daw_closed,
            } => {
                ensure!(daw_closed, "Close the DAW project before recovery");
                let disk = engine.clone();
                tauri::async_runtime::spawn_blocking(move || disk.recover(project_id)).await??;
                json!({"message": "Local recovery completed"})
            }
            action => {
                let disk = engine.clone();
                let dialog_app = app.clone();
                let invite_export = matches!(&action, Action::ExportInvite { .. });
                if invite_export {
                    wait_for_mapping_if_requested(&network).await?;
                }
                let explicit = match &action {
                    Action::ExportInvite {
                        public_ip: Some(ip),
                        external_port: Some(port),
                        ..
                    } => trackmesh_core::public_ipv4_addresses(ip, *port)?,
                    Action::ExportInvite {
                        public_ip: None,
                        external_port: None,
                        ..
                    }
                    | Action::ExportCard => Vec::new(),
                    Action::ExportInvite { .. } => {
                        anyhow::bail!("Public IPv4 and external port must be supplied together")
                    }
                    _ => Vec::new(),
                };
                let addresses =
                    if matches!(&action, Action::ExportInvite { .. } | Action::ExportCard) {
                        invite_addresses(
                            &engine,
                            &network,
                            &self.options.external_address,
                            explicit,
                        )
                        .await?
                    } else {
                        Vec::new()
                    };
                tauri::async_runtime::spawn_blocking(move || -> Result<Value> {
                    match action {
                        Action::DeleteBackups { selected } => {
                            let total = selected.iter().try_fold(0u64, |sum, item| sum.checked_add(item.bytes)).context("backup size overflow")?;
                            let size = total as f64 / f64::from(1u32 << 30);
                            let message = if is_turkish() {
                                format!("{} eski yedek silinsin mi ({size:.2} GB)?\n\nHer projenin en yeni yedeği tutulur.", selected.len())
                            } else {
                                format!("Delete {} old backups ({size:.2} GB)?\n\nEach project's newest backup will be kept.", selected.len())
                            };
                            let accepted = dialog_app.dialog().message(message)
                                .title(tr("Delete old backups?", "Eski yedekler silinsin mi?")).buttons(tauri_plugin_dialog::MessageDialogButtons::OkCancelCustom(tr("Delete", "Sil").to_owned(), tr("Cancel", "Vazgeç").to_owned())).blocking_show();
                            ensure!(accepted, "Backup deletion cancelled");
                            let bytes = disk.delete_old_backups(&selected)?;
                            Ok(json!({"message": format!("Deleted {bytes} bytes of old backups")}))
                        }
                        Action::Link { project_id, name, folder } => {
                            ensure!(!project_id.is_nil(), "Project UUID cannot be nil");
                            let is_existing = disk.store.projects()?.iter().any(|project| project.project_id == project_id);
                            disk.link(project_id, &name, &folder)?;
                            if !is_existing {
                                if let Some(default) = disk.default_supabase_config()? {
                                    disk.set_project_backend_config(project_id, ProjectBackendConfig {
                                        backend: ProjectBackend::Supabase,
                                        supabase_url: Some(default.url),
                                        supabase_anon_key: Some(default.publishable_key),
                                    })?;
                                }
                            }
                            Ok(json!({"message": "Folder linked"}))
                        }
                        Action::ChangeProjectRoot { project_id, folder } => {
                            let differs = disk.project_root_differs(project_id, &folder)?;
                            if differs {
                                let accepted = dialog_app.dialog().message(tr("This folder differs from your last shared version.\n\nUse it as the project folder anyway?", "Bu klasör son paylaştığın sürümden farklı.\n\nYine de proje klasörü olarak kullanılsın mı?")).title(tr("Change project folder?", "Proje klasörü değiştirilsin mi?")).kind(tauri_plugin_dialog::MessageDialogKind::Warning)
                                    .buttons(tauri_plugin_dialog::MessageDialogButtons::OkCancelCustom(tr("Use this folder", "Bu klasörü kullan").to_owned(), tr("Cancel", "Vazgeç").to_owned())).blocking_show();
                                ensure!(accepted, "Folder change cancelled");
                            }
                            disk.change_project_root(project_id, &folder, differs)?;
                            Ok(json!({"message": "Project folder updated"}))
                        }
                        Action::SetProjectBackend { project_id, backend, supabase_url, supabase_anon_key } => {
                            disk.set_project_backend_config(project_id, ProjectBackendConfig { backend, supabase_url, supabase_anon_key })?;
                            Ok(json!({"message": "Project backend settings saved"}))
                        }
                        Action::SetDefaultSupabase { url, publishable_key } => {
                            let config = match (url, publishable_key) {
                                (None, None) => None,
                                (Some(url), Some(publishable_key)) => Some(SupabaseDefaultConfig { url, publishable_key }),
                                _ => anyhow::bail!("Enter both the Supabase Project URL and publishable key"),
                            };
                            disk.set_default_supabase_config(config)?;
                            Ok(json!({"message": "Default Supabase settings saved"}))
                        }
                        Action::SetDeviceName { name } => {
                            disk.set_device_name(&name)?;
                            Ok(json!({"message": "Device name saved"}))
                        }
                        Action::SetAutoShare { project_id, enabled } => {
                            disk.set_auto_share(project_id, enabled)?;
                            Ok(json!({"message": if enabled { "Automatic sharing is on" } else { "Automatic sharing is off" }}))
                        }
                        Action::SetProjectsLocation { location } => {
                            disk.set_projects_location(&location)?;
                            Ok(json!({"message": "Projects location saved"}))
                        }
                        Action::SetManualPublicEndpoint { public_ipv4, external_port } => {
                            disk.set_manual_public_endpoint(public_ipv4, external_port)?;
                            Ok(json!({"message": "Manual port forwarding settings saved"}))
                        }
                        Action::OpenPairing { project_id } => {
                            let replaced = disk.open_pairing_window(project_id)?;
                            let message = if let Some(other) = replaced {
                                format!("Pending approvals for {} were cleared; only one project can accept new devices at a time.", disk.store.project(other)?.name)
                            } else {
                                "Approval window open for 15 minutes. Ask your collaborator to press Ask for approval, then verify the arriving device before approving.".to_owned()
                            };
                            Ok(json!({"message": message}))
                        }
                        Action::ClosePairing { project_id } => {
                            ensure!(disk.pairing_status()?.project_id == Some(project_id), "No pairing window for this project");
                            disk.close_pairing_window()?;
                            Ok(json!({"message": "Pairing window closed; unapproved requests removed"}))
                        }
                        Action::RejectPairing { project_id, request_id } => {
                            disk.reject_pairing(project_id, request_id)?;
                            Ok(json!({"message": "Pairing request rejected; no project access was granted"}))
                        }
                        Action::Pair { project_id } => {
                            disk.pair(project_id, documents::read_document(&app)?)?;
                            Ok(json!({"message": "Pairing reply accepted. This device is authorized for the selected project. Tell your collaborator pairing is complete, create a snapshot, and keep TrackMesh open."}))
                        }
                        Action::Revoke { project_id, device_id } => {
                            let name = disk.store.peers(Some(project_id))?.into_iter()
                                .find(|peer| peer.device_id == device_id)
                                .map(|peer| peer.name)
                                .unwrap_or_else(|| "device".to_owned());
                            let outcome = disk.revoke(project_id, device_id)?;
                            let message = match outcome {
                                RevokeOutcome::Queued => format!("Removed {name}'s access on this PC. TrackMesh will also remove {name} from the invite service."),
                                RevokeOutcome::ImportedSupabase => format!("Removed {name}'s access on this PC. Only the device that created this project can remove {name} from the invite service."),
                                RevokeOutcome::LocalOnly => format!("Removed {name}'s access on this PC."),
                            };
                            Ok(json!({"message": message}))
                        }
                        Action::RemoveProject { project_id, delete_data } => {
                            disk.remove_project(project_id, delete_data)?;
                            Ok(json!({"message": "Removed from TrackMesh. The folder and its files are unchanged."}))
                        }
                        Action::RestoreProject { project_id } => {
                            disk.restore_project(project_id)?;
                            Ok(json!({"message": "Back in TrackMesh. Invite your collaborator again to share it."}))
                        }
                        Action::ExportInvite { project_id, .. } => {
                            let invite = disk.export_invite(project_id, addresses)?;
                            let path = documents::write_document(&app, "trackmesh-invite.json", &invite)?;
                            let replaced = disk.open_pairing_window(project_id)?;
                            let message = if let Some(other) = replaced {
                                format!("Private invitation saved. Pending approvals for {} were cleared; only one project can accept new devices at a time.", disk.store.project(other)?.name)
                            } else {
                                "Private invitation saved. Automatic pairing window open for 15 minutes; share the invitation privately, then verify and approve your collaborator here.".to_owned()
                            };
                            Ok(json!({"message": message, "path": path}))
                        }
                        Action::ExportCard => {
                            let card = disk.device_card(addresses)?;
                            let path = documents::write_document(&app, "trackmesh-pairing-reply.json", &card)?;
                            Ok(json!({"message": "Pairing reply saved. Send this file privately to the inviter to complete pairing.", "path": path}))
                        }
                        _ => anyhow::bail!("Unsupported document action"),
                    }
                }).await??
            }
        };
        let disk = engine.clone();
        let pairing = tauri::async_runtime::spawn_blocking(move || disk.pairing_status()).await??;
        network
            .pairing_window(Duration::from_secs(pairing.remaining_seconds))
            .await?;
        if reload {
            network
                .reload()
                .await
                .context("Project setup saved, but network reload failed; reopen desktop")?;
        }
        Ok(outcome)
    }

    async fn automatic_pairing(
        &self,
        engine: &Arc<Engine>,
        network: &Network,
        project: Uuid,
        device: Uuid,
        imported: bool,
    ) -> Value {
        let result = async {
            let disk = engine.clone();
            let peer =
                tauri::async_runtime::spawn_blocking(move || disk.paired_peer(project, device))
                    .await??;
            wait_for_peer(self, network, &peer.peer_id).await?;
            let addresses =
                invite_addresses(engine, network, &self.options.external_address, Vec::new())
                    .await?;
            engine
                .request_pairing(network, project, device, addresses)
                .await
        }
        .await;
        let (state, approval, message) = match result {
            Ok(protocol::PairingResponse::Pending) => ("pending", PairingApprovalState::Pending, "Pairing request sent automatically. The inviter must verify and approve your device. No card file needs to be sent; wait for their confirmation."),
            Ok(protocol::PairingResponse::Approved) => {
                (
                    "approved",
                    PairingApprovalState::Approved,
                    "The inviter approved this device for the project.",
                )
            },
            _ => ("retry", PairingApprovalState::NotReachable, "Automatic pairing could not complete. Your project remains imported. Ask the inviter to open the approval window and keep TrackMesh open, then press Ask for approval. Do not reimport or create a new UUID. Both devices need a build supporting automatic pairing."),
        };
        self.set_pairing_approval(project, device, approval);
        json!({"project_id": project, "invitation_imported": imported, "pairing_state": state, "message": message})
    }

    pub fn set_language(&self, lang: &str) {
        trackmesh_core::lang::set_turkish(lang == "tr");
    }

    pub fn hide_to_tray(self: &Arc<Self>, app: AppHandle) {
        let engine = {
            let Ok(mut state) = self.lock() else {
                return;
            };
            if state.closing || state.closed || state.close_prompt_active {
                return;
            }
            state.close_prompt_active = true;
            state.engine.clone()
        };
        let desktop = self.clone();
        tauri::async_runtime::spawn(async move {
            let dialog_app = app.clone();
            let result = tauri::async_runtime::spawn_blocking(move || -> Result<()> {
                let shown = engine.as_ref().map(|engine| engine.tray_notice_shown()).transpose()?.unwrap_or(false);
                if !shown {
                    let (title, message) = if is_turkish() {
                        ("TrackMesh açık kalıyor", "Pencere ya da DAW kapanınca TrackMesh saatin yanında çalışmaya devam eder; iş arkadaşların sürümlerini alabilir.\n\nYeniden açmak için saatin yanındaki TrackMesh simgesine tıkla.\n\nTamamen kapatmak için penceredeki ya da simge menüsündeki TrackMesh'ten çık'ı kullan.")
                    } else {
                        ("TrackMesh stays available", "TrackMesh keeps running next to the clock after the window or DAW closes, so collaborators can get your versions.\n\nClick the TrackMesh icon next to the clock to reopen it.\n\nTo exit, use Quit TrackMesh in the window or in the icon's menu.")
                    };
                    dialog_app.dialog().message(message).title(title).blocking_show();
                    if let Some(engine) = engine { engine.mark_tray_notice_shown()?; }
                }
                Ok(())
            }).await;
            if let Ok(mut state) = desktop.lock() {
                state.close_prompt_active = false;
                match result {
                    Ok(Ok(())) if !state.closing && !state.closed => {
                        if let Some(window) = app.get_webview_window("main") {
                            match window.hide() {
                                Ok(()) => {
                                    state.window_session = false;
                                    state.tray_session = true;
                                    state.work_epoch = state.work_epoch.wrapping_add(1);
                                }
                                Err(error) => state.error = Some(error.to_string()),
                            }
                        }
                    }
                    Ok(Err(error)) => state.error = Some(error.to_string()),
                    Err(_) => {
                        state.error =
                            Some("Could not show the tray notice; window kept open".to_owned())
                    }
                    _ => {}
                }
            }
        });
    }

    pub fn request_quit(self: &Arc<Self>, app: AppHandle) {
        let (engine, received) = {
            let Ok(mut state) = self.lock() else {
                return;
            };
            if state.closing || state.closed || state.close_prompt_active {
                return;
            }
            state.close_prompt_active = true;
            let received = state
                .network
                .as_ref()
                .map(|network| network.status().received_snapshots)
                .unwrap_or_default();
            (state.engine.clone(), received)
        };
        let desktop = self.clone();
        tauri::async_runtime::spawn(async move {
            let dialog_app = app.clone();
            let result = tauri::async_runtime::spawn_blocking(move || -> Result<bool> {
                let recipients = engine.as_ref().map(|engine| engine.shutdown_recipients(&received)).transpose()?.unwrap_or_default();
                // Everyone already has this PC's latest version (or there is nobody), so quit needs no confirmation.
                if recipients.is_empty() {
                    return Ok(true);
                }
                // One name per line so a long list stays readable.
                let names = recipients.iter().map(|name| format!("    • {name}")).collect::<Vec<_>>().join("\n");
                let (title, message, quit, cancel) = if is_turkish() {
                    ("TrackMesh kapatılsın mı?", format!("TrackMesh kapalıyken bu kişiler sürümlerini alamaz:\n\n{names}\n\nBaşlamış işler kapanmadan önce biter."), "Çık", "Vazgeç")
                } else {
                    ("Quit TrackMesh?", format!("While TrackMesh is closed, these collaborators can't get your versions:\n\n{names}\n\nWork that has already started finishes before TrackMesh closes."), "Quit", "Cancel")
                };
                Ok(dialog_app.dialog().message(message).title(title).buttons(tauri_plugin_dialog::MessageDialogButtons::OkCancelCustom(quit.to_owned(), cancel.to_owned())).blocking_show())
            }).await;
            let close = desktop.lock().is_ok_and(|mut state| {
                state.close_prompt_active = false;
                match result {
                    Ok(Ok(true)) => {
                        state.closing = true;
                        state.window_session = false;
                        state.tray_session = false;
                        if let Some(plugin) = &state.plugin {
                            plugin.begin_close();
                        }
                        true
                    }
                    Ok(Err(error)) => {
                        state.error = Some(error.to_string());
                        false
                    }
                    Err(_) => {
                        state.error =
                            Some("Could not confirm shutdown; TrackMesh kept running".to_owned());
                        false
                    }
                    _ => false,
                }
            });
            if close {
                desktop.finish_close(app).await;
            }
        });
    }
    pub fn reopen(&self, app: &AppHandle) {
        if let Ok(mut state) = self.lock() {
            if state.closing || state.closed {
                return;
            }
            state.window_session = true;
            state.work_epoch = state.work_epoch.wrapping_add(1);
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }
    }
    pub fn handle_invite_link(self: &Arc<Self>, app: AppHandle, value: String) {
        self.reopen(&app);
        let value = zeroize::Zeroizing::new(value);
        let parsed = crate::uri::parse_invite_link(&value);
        if let Ok(mut state) = self.lock() {
            state.work_epoch = state.work_epoch.wrapping_add(1);
            let accepting = state
                .jobs
                .iter()
                .any(|job| job.running && job.label == "Accept invite link");
            if let Ok(parsed) = parsed {
                if !accepting {
                    let service = parsed.service.map(|service| PendingInviteService {
                        url: format!("https://{}.supabase.co", service.ref_name),
                        publishable_key: service.publishable_key,
                    });
                    // A new project-less invite replaces only project-less notices.
                    state
                        .jobs
                        .retain(|job| job.running || job.project_id.is_some());
                    prune_finished_jobs(&mut state.jobs);
                    state.pending_invite = Some(PendingInviteLink {
                        id: parsed.id,
                        key: parsed.key,
                        service,
                        parent_folder: None,
                    });
                    return;
                }
            }
            state
                .jobs
                .retain(|job| job.running || job.project_id.is_some());
            prune_finished_jobs(&mut state.jobs);
            state.jobs.push(Job {
                job_id: Uuid::new_v4(),
                project_id: None,
                label: "Open invite link".to_owned(),
                running: false,
                outcome: None,
                error: Some(
                    if accepting {
                        "An invite is being accepted. Wait for it to finish before opening another link.".to_owned()
                    } else {
                        "This invite link could not be opened. Check the link and try again.".to_owned()
                    }
                ),
                completed_at: None,
                abort: None,
                stop: None,
            });
            prune_finished_jobs(&mut state.jobs);
        }
    }
    pub fn paste_invite_link(&self, app: &AppHandle) -> Result<()> {
        self.reopen(app);
        let mut state = self.lock()?;
        ensure!(
            !state
                .jobs
                .iter()
                .any(|job| job.running && job.label == "Accept invite link"),
            "An invite is being accepted; wait for it to finish"
        );
        let parsed = crate::clipboard::take_invite_link()?;
        state.work_epoch = state.work_epoch.wrapping_add(1);
        state
            .jobs
            .retain(|job| job.running || job.label != "Accept invite link");
        state.pending_invite = Some(PendingInviteLink {
            id: parsed.id,
            key: parsed.key,
            service: parsed.service.map(|service| PendingInviteService {
                url: format!("https://{}.supabase.co", service.ref_name),
                publishable_key: service.publishable_key,
            }),
            parent_folder: None,
        });
        Ok(())
    }
    async fn finish_close(&self, app: AppHandle) {
        let result = self.shutdown().await;
        if let Err(error) = &result {
            tracing::error!(%error, "Desktop shutdown finished with an error");
        }
        if let Ok(mut state) = self.lock() {
            state.closed = true;
        }
        app.exit(if result.is_ok() { 0 } else { 1 });
    }
    async fn shutdown(&self) -> Result<()> {
        let tasks = {
            let mut state = self.lock()?;
            std::mem::take(&mut state.tasks)
        };
        let mut errors = Vec::new();
        for task in tasks {
            if let Err(error) = task.await {
                errors.push(error.to_string());
            }
        }
        // Initialization may have installed the runtime while close was waiting for it.
        let (peer, plugin) = {
            let mut state = self.lock()?;
            (state.peer.take(), state.plugin.take())
        };
        if let Some(plugin) = plugin {
            if let Err(error) = plugin.stop().await {
                errors.push(error.to_string());
            }
        }
        if let Some(peer) = peer {
            if let Err(error) = peer.stop().await {
                errors.push(error.to_string());
            }
        }
        ensure!(errors.is_empty(), "{}", errors.join("; "));
        Ok(())
    }
}

fn create_unique_invite_folder(parent: &std::path::Path, name: &str) -> Result<PathBuf> {
    ensure!(!name.is_empty(), "Project folder name is empty");
    let mut suffix = 1u64;
    loop {
        let folder_name = if suffix == 1 {
            name.to_owned()
        } else {
            format!("{name} ({suffix})")
        };
        let path = parent.join(folder_name);
        match std::fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                suffix = suffix
                    .checked_add(1)
                    .context("No available project folder name")?;
            }
            Err(error) => return Err(error.into()),
        }
    }
}

fn sanitize_project_name(name: &str) -> String {
    let mut safe = name
        .chars()
        .map(|ch| {
            if ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
            {
                '_'
            } else {
                ch
            }
        })
        .take(120)
        .collect::<String>();
    safe = safe.trim().trim_end_matches([' ', '.']).to_owned();
    if safe.is_empty() {
        safe = "Shared Project".to_owned();
    }
    let stem = safe
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches([' ', '.'])
        .to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || matches!(stem.as_str(), "CONIN$" | "CONOUT$")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        || matches!(
            stem.as_str(),
            "COM¹" | "COM²" | "COM³" | "LPT¹" | "LPT²" | "LPT³"
        )
    {
        safe.insert(0, '_');
    }
    safe
}

fn metadata_frontier(summary: &trackmesh_core::ProjectSummary) -> Vec<(Uuid, Option<Uuid>)> {
    let mut frontier = summary
        .peers
        .iter()
        .map(|peer| {
            (
                peer.device.device_id,
                peer.announcement.as_ref().map(|s| s.snapshot_id),
            )
        })
        .collect::<Vec<_>>();
    frontier.sort();
    frontier
}

async fn wait_for_peer(desktop: &Desktop, network: &Network, peer: &str) -> Result<()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while !network.online(peer) {
        ensure!(!desktop.lock()?.closing, "Desktop session is closing");
        ensure!(
            tokio::time::Instant::now() < deadline && network.status().error.is_none(),
            "Source peer is offline or unreachable; retry when online"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Ok(())
}

fn invite_service_ref(url: &str) -> Result<String> {
    supabase_client::validate_supabase_project_url(url)?;
    let raw_host = url
        .strip_prefix("https://")
        .context("Invite links require a Supabase project URL of https://<ref>.supabase.co")?;
    ensure!(
        raw_host
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.'),
        "Invite links require a Supabase project URL of https://<ref>.supabase.co"
    );
    let raw_reference = raw_host
        .strip_suffix(".supabase.co")
        .context("Invite links require a Supabase project URL of https://<ref>.supabase.co")?;
    let host = supabase_client::supabase_project_host(url)?;
    let reference = host
        .strip_suffix(".supabase.co")
        .context("Invite links require a Supabase project URL of https://<ref>.supabase.co")?;
    ensure!(
        (1..=63).contains(&reference.len())
            && reference
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit()),
        "Invite links require a lowercase single-label Supabase project host"
    );
    ensure!(
        raw_reference == reference
            && raw_host == format!("{reference}.supabase.co")
            && host == format!("{reference}.supabase.co"),
        "Invite links require a single-label Supabase project host"
    );
    Ok(reference.to_owned())
}

async fn wait_for_mapping_if_requested(network: &Network) -> Result<()> {
    let status = network.status();
    if !status.port_mapping.consent_enabled
        || !matches!(
            status.port_mapping.state.as_str(),
            "DISCOVERING" | "NOT_ATTEMPTED"
        )
    {
        return Ok(());
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    let before = tokio::time::timeout_at(deadline, network.endpoint_addresses())
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default();
    let mut state = status.port_mapping.state;
    while tokio::time::Instant::now() < deadline {
        if !matches!(state.as_str(), "DISCOVERING" | "NOT_ATTEMPTED") {
            break;
        }
        tokio::time::sleep_until(std::cmp::min(
            tokio::time::Instant::now() + Duration::from_millis(150),
            deadline,
        ))
        .await;
        state = network.status().port_mapping.state;
    }
    if state == "MAPPED" {
        let initial = before
            .iter()
            .any(|address| network::is_public_endpoint_address(address));
        if !initial {
            let address_deadline = tokio::time::Instant::now() + Duration::from_secs(2);
            while tokio::time::Instant::now() < address_deadline {
                let current =
                    tokio::time::timeout_at(address_deadline, network.endpoint_addresses())
                        .await
                        .ok()
                        .and_then(Result::ok)
                        .unwrap_or_default();
                if current.iter().any(|address| {
                    network::is_public_endpoint_address(address) && !before.contains(address)
                }) {
                    break;
                }
                tokio::time::sleep_until(std::cmp::min(
                    tokio::time::Instant::now() + Duration::from_millis(100),
                    address_deadline,
                ))
                .await;
            }
        }
    }
    Ok(())
}

async fn invite_addresses(
    engine: &Arc<Engine>,
    network: &Network,
    startup_external: &[String],
    explicit: Vec<String>,
) -> Result<Vec<String>> {
    let disk = engine.clone();
    let manual =
        tauri::async_runtime::spawn_blocking(move || disk.manual_public_endpoint()).await??;
    let endpoint_addresses = network.endpoint_addresses().await?;
    let mut output: Vec<String> = Vec::new();
    let mut add = |address: String, validate: bool| {
        if (!validate || protocol::validate_endpoint_address(&address).is_ok())
            && !output.contains(&address)
        {
            output.push(address);
        }
    };
    for address in explicit {
        add(address, true);
    }
    if let Some(manual) = manual {
        for address in
            trackmesh_core::public_ipv4_addresses(&manual.public_ipv4, manual.external_port)?
        {
            add(address, true);
        }
    }
    // Operator-supplied startup addresses (possibly DNS) are kept as before this helper.
    for address in startup_external {
        add(address.clone(), false);
    }
    let mut dynamic = endpoint_addresses;
    dynamic.extend(network.status().addresses);
    dynamic.retain(|address| protocol::validate_endpoint_address(address).is_ok());
    dynamic.sort_by_key(|address| !network::is_public_endpoint_address(address));
    for address in dynamic {
        add(address, true);
    }
    output.truncate(16);
    output.sort();
    Ok(output)
}
