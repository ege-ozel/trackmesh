use anyhow::{ensure, Context, Result};
use crypto::Identity;
use snapshot::ChunkStore;
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use storage::Store;
use uuid::Uuid;
mod auto_share;
mod presence;
pub use presence::PresenceWarning;
mod connection;
mod endpoints;
mod firewall_observation;
pub mod lang;
mod network_observation;
mod settings;
pub use auto_share::DawPluginSetting;
mod supabase;
pub use firewall_observation::{observe_firewall, FirewallObservation};
pub use network_observation::{observe_network, NetworkObservation};
mod pairing;
pub use connection::public_ipv4_addresses;
pub use pairing::{PairingCandidate, PairingStatus, PAIRING_WINDOW};
pub mod peer;
pub use settings::{
    GrantProblem, ManualPublicEndpoint, PendingGrant, PendingRevoke, ProjectBackend,
    ProjectBackendConfig, SupabaseDefaultConfig, SupabaseProjectState,
};
pub use supabase::SupabaseRuntime;
mod workflows;
pub use workflows::{
    peer_connection_state, port_observation_status, LocalStatus, PeerCheck, PeerConnectionState,
    PeerSnapshotStatus, PortObservationStatus, ProjectStatus,
};
mod status;
pub use status::{OfflineHead, PeerSummary, ProjectSummary, SummaryPage};
mod sending;
pub use sending::SendStatus;
#[cfg(windows)]
mod ipc;
mod retention;
pub use retention::{BackupSelection, BackupSummary, StorageSummary};
pub use snapshots::SnapshotChanges;
mod snapshots;
#[cfg(windows)]
pub use ipc::{plugin_pipe_name, PluginDesktopRequest, PluginRuntime};
#[cfg(windows)]
mod lifetime;
#[cfg(windows)]
pub use lifetime::SessionGrace;

pub struct Engine {
    presence: Mutex<presence::PresenceState>,
    pub directory: PathBuf,
    pub store: Arc<Store>,
    pub chunks: ChunkStore,
    pub identity: Arc<Identity>,
    pairing: Mutex<pairing::PairingState>,
    settings: Mutex<settings::SettingsState>,
    supabase_states: Mutex<std::collections::HashMap<Uuid, SupabaseProjectState>>,
    grant_problems: Mutex<std::collections::HashMap<Uuid, GrantProblem>>,
    revoke_problems: Mutex<std::collections::HashSet<Uuid>>,
    missing_supabase_functions:
        Mutex<std::collections::HashMap<Uuid, std::collections::HashSet<String>>>,
    upnp_mapping_enabled: AtomicBool,
    offline_heads: Mutex<std::collections::HashMap<(Uuid, Uuid), (u64, protocol::HeadRecord)>>,
    /// Each collaborator's head from the last pull; `None` when it had no version.
    peer_heads: Mutex<std::collections::HashMap<(Uuid, Uuid), Option<Uuid>>>,
}
impl Engine {
    pub fn open(directory: PathBuf, name: &str) -> Result<Self> {
        std::fs::create_dir_all(&directory)?;
        let directory = std::fs::canonicalize(directory)?;
        let identity = Arc::new(Identity::load_or_create(&directory.join("secrets"), name)?);
        let store = Arc::new(Store::open(&directory.join("trackmesh.db"))?);
        let chunks = ChunkStore::new(directory.join("chunks"))?;
        let mut settings = settings::SettingsState::load(&directory);
        let linked = store
            .projects()?
            .into_iter()
            .map(|project| project.project_id)
            .collect();
        settings.retain_auto_share_projects(&directory, &linked)?;
        // A project removed with its data waits for its queued access removals; once none are left,
        // its key file and last row go.
        for project in store.deleted_projects()? {
            if settings
                .pending_revokes(Some(project.project_id))
                .is_empty()
            {
                let key = directory
                    .join("secrets/projects")
                    .join(format!("{}.protected", project.project_id));
                if key.exists() {
                    std::fs::remove_file(&key)?;
                }
                store.forget_project(project.project_id)?;
            }
        }
        if let Some(device_name) = &settings.value.device_name {
            identity.set_name(device_name)?;
        }
        let upnp_mapping_enabled = settings.value.upnp_mapping_enabled();
        let peer_id = network::peer_id(&identity)?;
        store.add_device(
            identity.device_id,
            &identity.name(),
            &peer_id,
            &identity.public_key(),
        )?;
        Ok(Self {
            presence: Mutex::new(presence::PresenceState::default()),
            directory,
            store,
            chunks,
            identity,
            pairing: Mutex::new(pairing::PairingState::default()),
            settings: Mutex::new(settings),
            supabase_states: Mutex::new(std::collections::HashMap::new()),
            grant_problems: Mutex::new(std::collections::HashMap::new()),
            revoke_problems: Mutex::new(std::collections::HashSet::new()),
            missing_supabase_functions: Mutex::new(std::collections::HashMap::new()),
            upnp_mapping_enabled: AtomicBool::new(upnp_mapping_enabled),
            offline_heads: Mutex::new(std::collections::HashMap::new()),
            peer_heads: Mutex::new(std::collections::HashMap::new()),
        })
    }
    pub fn link(&self, project: Uuid, name: &str, root: &Path) -> Result<()> {
        let root = snapshot::canonical_root(root)?;
        ensure!(
            !self.directory.starts_with(&root) && !root.starts_with(&self.directory),
            "project folder and agent data directory must be separate"
        );
        crypto::project_key(&self.directory.join("secrets/projects"), project, None)?;
        self.store.link(project, name, &root)
    }

    pub fn project_root_differs(&self, project: Uuid, root: &Path) -> Result<bool> {
        let root = snapshot::canonical_root(root)?;
        self.store.project(project)?;
        let Some(head) = self.store.head(project)? else {
            return Ok(false);
        };
        let manifest = snapshot::scan_for_comparison(
            &root,
            project,
            head.manifest.author_device_id,
            Some(&head.manifest),
            &snapshot::ScanOptions::default(),
        )?;
        let expected = head
            .manifest
            .files
            .iter()
            .filter(|file| !snapshot::is_ignored_path(&file.relative_path))
            .collect::<Vec<_>>();
        Ok(!manifest.files.iter().eq(expected))
    }

    pub fn change_project_root(
        &self,
        project: Uuid,
        root: &Path,
        confirm_difference: bool,
    ) -> Result<()> {
        let _lock = snapshot::ProjectLock::acquire(&self.directory, project)?;
        let project_record = self.store.project(project)?;
        ensure!(
            !project_record.recovery_required,
            "finish project recovery before changing its folder"
        );
        for transfer in self.store.transfers()? {
            if transfer.project_id == project && transfer.state != storage::TransferState::Complete
            {
                anyhow::bail!("finish the active transfer or recovery before changing this folder");
            }
        }
        let root = snapshot::canonical_root(root)?;
        ensure!(
            !self.directory.starts_with(&root) && !root.starts_with(&self.directory),
            "project folder and agent data directory must be separate"
        );
        for other in self.store.projects()? {
            if other.project_id != project && Path::new(&other.root) == root {
                anyhow::bail!("folder is already linked to another project");
            }
        }
        if let Some(head) = self.store.head(project)? {
            let manifest = snapshot::scan_for_comparison(
                &root,
                project,
                head.manifest.author_device_id,
                Some(&head.manifest),
                &snapshot::ScanOptions::default(),
            )?;
            let expected = head
                .manifest
                .files
                .iter()
                .filter(|file| !snapshot::is_ignored_path(&file.relative_path))
                .collect::<Vec<_>>();
            let differs = !manifest.files.iter().eq(expected);
            ensure!(
                !differs || confirm_difference,
                "This folder differs from your last shared version"
            );
        }
        self.store.update_root(project, &root)
    }
}

impl Engine {
    pub fn upnp_mapping_enabled(&self) -> bool {
        self.upnp_mapping_enabled.load(Ordering::SeqCst)
    }
    pub fn set_upnp_mapping_enabled(&self, enabled: bool) -> Result<()> {
        let mut settings = self
            .settings
            .lock()
            .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?;
        settings.set_upnp_mapping(&self.directory, enabled)?;
        self.upnp_mapping_enabled.store(enabled, Ordering::SeqCst);
        Ok(())
    }
    pub fn upnp_mapping_asked(&self) -> Result<bool> {
        Ok(self
            .settings
            .lock()
            .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?
            .value
            .upnp_mapping_asked())
    }
    pub(crate) fn next_supabase_sequence(&self, project: Uuid) -> Result<u64> {
        self.settings
            .lock()
            .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?
            .next_sequence(&self.directory, project)
    }
    pub(crate) fn remember_offline_head(
        &self,
        project: Uuid,
        writer: Uuid,
        seq: u64,
        record: protocol::HeadRecord,
    ) -> Result<()> {
        let mut heads = self
            .offline_heads
            .lock()
            .map_err(|_| anyhow::anyhow!("offline head lock poisoned"))?;
        let key = (project, writer);
        if heads.get(&key).is_none_or(|(old_seq, _)| seq > *old_seq) {
            heads.insert(key, (seq, record));
        }
        Ok(())
    }

    pub fn project_backend_config(&self, project: Uuid) -> Result<ProjectBackendConfig> {
        self.settings
            .lock()
            .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?
            .value
            .project_backends
            .get(&project)
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| Ok(ProjectBackendConfig::default()))
    }

    pub fn set_project_backend_config(
        &self,
        project: Uuid,
        config: ProjectBackendConfig,
    ) -> Result<()> {
        self.store.project(project)?;
        match config.backend {
            ProjectBackend::Supabase => {
                let url = config.supabase_url.as_deref().unwrap_or_default();
                let key = config.supabase_anon_key.as_deref().unwrap_or_default();
                ::supabase_client::validate_supabase_project_url(url)?;
                ::supabase_client::validate_supabase_publishable_key(key)?;
            }
            ProjectBackend::None | ProjectBackend::SelfHostedMailbox => ensure!(
                config.supabase_url.is_none() && config.supabase_anon_key.is_none(),
                "Supabase credentials are only valid for the Supabase backend"
            ),
        }
        let is_supabase = matches!(config.backend, ProjectBackend::Supabase);
        {
            let mut settings = self
                .settings
                .lock()
                .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?;
            settings.set_project_backend(&self.directory, project, config)?;
        }
        self.set_supabase_project_state(
            project,
            if is_supabase {
                SupabaseProjectState::Unknown
            } else {
                SupabaseProjectState::NotConfigured
            },
        )?;
        // Only the creator grants membership; a device that joined (or joins again) only has the
        // creator as a peer, and the service refuses grants from anyone but the owner.
        if is_supabase && !self.project_backend_with_imported(project)?.1 {
            for card in self.store.peers(Some(project))? {
                if card.device_id != self.identity.device_id {
                    self.queue_supabase_grant(project, &card)?;
                }
            }
        }
        Ok(())
    }

    pub fn default_supabase_config(&self) -> Result<Option<SupabaseDefaultConfig>> {
        let config = self
            .settings
            .lock()
            .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?
            .value
            .default_supabase
            .clone();
        if let Some(config) = &config {
            ::supabase_client::validate_supabase_project_url(&config.url)?;
            ::supabase_client::validate_supabase_publishable_key(&config.publishable_key)?;
        }
        Ok(config)
    }

    pub fn set_device_name(&self, name: &str) -> Result<()> {
        let normalized = name.trim().chars().take(80).collect::<String>();
        ensure!(!normalized.is_empty(), "device name cannot be empty");
        self.settings
            .lock()
            .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?
            .set_device_name(&self.directory, normalized.clone())?;
        self.identity.set_name(&normalized)
    }

    pub fn projects_location(&self) -> Result<Option<String>> {
        Ok(self
            .settings
            .lock()
            .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?
            .value
            .projects_location
            .clone())
    }

    pub fn set_projects_location(&self, location: &Path) -> Result<()> {
        ensure!(
            location.is_absolute(),
            "Projects location must be an absolute path"
        );
        self.settings
            .lock()
            .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?
            .set_projects_location(&self.directory, location.to_string_lossy().into_owned())
    }

    pub fn manual_public_endpoint(&self) -> Result<Option<ManualPublicEndpoint>> {
        let endpoint = self
            .settings
            .lock()
            .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?
            .value
            .manual_public_endpoint
            .clone();
        if let Some(value) = &endpoint {
            public_ipv4_addresses(&value.public_ipv4, value.external_port)?;
        }
        Ok(endpoint)
    }

    pub fn set_manual_public_endpoint(
        &self,
        public_ipv4: Option<String>,
        external_port: Option<u16>,
    ) -> Result<()> {
        let endpoint = match (public_ipv4, external_port) {
            (None, None) => None,
            (Some(public_ipv4), Some(external_port)) => {
                public_ipv4_addresses(&public_ipv4, external_port)?;
                Some(ManualPublicEndpoint {
                    public_ipv4,
                    external_port,
                })
            }
            _ => {
                anyhow::bail!("Enter both the public IPv4 address and external port, or clear both")
            }
        };
        self.settings
            .lock()
            .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?
            .set_manual_public_endpoint(&self.directory, endpoint)
    }

    pub fn set_default_supabase_config(&self, config: Option<SupabaseDefaultConfig>) -> Result<()> {
        if let Some(config) = &config {
            ::supabase_client::validate_supabase_project_url(&config.url)?;
            ::supabase_client::validate_supabase_publishable_key(&config.publishable_key)?;
        }
        self.settings
            .lock()
            .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?
            .set_default_supabase(&self.directory, config)
    }

    pub(crate) fn queue_supabase_grant(
        &self,
        project: Uuid,
        card: &protocol::DeviceCard,
    ) -> Result<PendingGrant> {
        network::validate_card(card)?;
        let mut settings = self
            .settings
            .lock()
            .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?;
        ensure!(
            settings
                .value
                .project_backends
                .get(&project)
                .is_some_and(|config| matches!(config.backend, ProjectBackend::Supabase)),
            "Supabase backend is not selected for this project"
        );
        settings.enqueue_grant(
            &self.directory,
            project,
            card.device_id,
            card.public_key.clone(),
        )
    }

    pub(crate) fn pending_supabase_grants(
        &self,
        project: Option<Uuid>,
    ) -> Result<Vec<PendingGrant>> {
        Ok(self
            .settings
            .lock()
            .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?
            .pending_grants(project))
    }

    pub(crate) fn complete_supabase_grant(&self, grant: &PendingGrant) -> Result<()> {
        self.settings
            .lock()
            .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?
            .complete_grant(&self.directory, grant)
    }

    pub(crate) fn retry_supabase_grant(&self, grant: &PendingGrant) -> Result<()> {
        self.settings
            .lock()
            .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?
            .replace_grant_sequence(&self.directory, grant)
    }

    pub(crate) fn pending_supabase_revokes(&self, project: Uuid) -> Result<Vec<PendingRevoke>> {
        Ok(self
            .settings
            .lock()
            .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?
            .pending_revokes(Some(project)))
    }

    pub(crate) fn complete_supabase_revoke(&self, revoke: &PendingRevoke) -> Result<()> {
        self.settings
            .lock()
            .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?
            .complete_revoke(&self.directory, revoke)
    }

    /// Whether this PC joined the project from an invite instead of creating it.
    pub fn project_joined(&self, project: Uuid) -> Result<bool> {
        Ok(self.project_backend_with_imported(project)?.1)
    }

    pub(crate) fn project_backend_with_imported(
        &self,
        project: Uuid,
    ) -> Result<(ProjectBackendConfig, bool)> {
        let settings = self
            .settings
            .lock()
            .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?;
        Ok((
            settings
                .value
                .project_backends
                .get(&project)
                .cloned()
                .unwrap_or_default(),
            settings.value.imported_projects.contains(&project),
        ))
    }

    pub(crate) fn set_supabase_project_state(
        &self,
        project: Uuid,
        state: SupabaseProjectState,
    ) -> Result<()> {
        self.supabase_states
            .lock()
            .map_err(|_| anyhow::anyhow!("Supabase status lock poisoned"))?
            .insert(project, state);
        Ok(())
    }

    /// Records whether the creator removed this PC from a joined project, so a restart still says so.
    pub(crate) fn set_membership_removed(&self, project: Uuid, removed: bool) -> Result<()> {
        self.settings
            .lock()
            .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?
            .set_membership_removed(&self.directory, project, removed)
    }

    pub(crate) fn membership_removed(&self, project: Uuid) -> Result<bool> {
        Ok(self
            .settings
            .lock()
            .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?
            .value
            .removed_memberships
            .contains(&project))
    }

    pub fn supabase_project_state(&self, project: Uuid) -> Result<SupabaseProjectState> {
        if !matches!(
            self.project_backend_config(project)?.backend,
            ProjectBackend::Supabase
        ) {
            return Ok(SupabaseProjectState::NotConfigured);
        }
        Ok(self
            .supabase_states
            .lock()
            .map_err(|_| anyhow::anyhow!("Supabase status lock poisoned"))?
            .get(&project)
            .copied()
            .unwrap_or_default())
    }

    pub(crate) fn set_grant_problem(&self, project: Uuid, problem: GrantProblem) -> Result<()> {
        self.grant_problems
            .lock()
            .map_err(|_| anyhow::anyhow!("grant problem lock poisoned"))?
            .insert(project, problem);
        Ok(())
    }

    pub(crate) fn clear_grant_problem(&self, project: Uuid) -> Result<()> {
        self.grant_problems
            .lock()
            .map_err(|_| anyhow::anyhow!("grant problem lock poisoned"))?
            .remove(&project);
        Ok(())
    }

    pub fn grant_problem(&self, project: Uuid) -> Result<Option<GrantProblem>> {
        Ok(self
            .grant_problems
            .lock()
            .map_err(|_| anyhow::anyhow!("grant problem lock poisoned"))?
            .get(&project)
            .copied())
    }

    pub fn revoke_problem(&self, project: Uuid) -> Result<bool> {
        Ok(self
            .revoke_problems
            .lock()
            .map_err(|_| anyhow::anyhow!("revoke problem lock poisoned"))?
            .contains(&project))
    }

    pub(crate) fn set_revoke_problem(&self, project: Uuid) -> Result<()> {
        self.revoke_problems
            .lock()
            .map_err(|_| anyhow::anyhow!("revoke problem lock poisoned"))?
            .insert(project);
        Ok(())
    }

    pub(crate) fn clear_revoke_problem(&self, project: Uuid) -> Result<()> {
        self.revoke_problems
            .lock()
            .map_err(|_| anyhow::anyhow!("revoke problem lock poisoned"))?
            .remove(&project);
        Ok(())
    }

    pub fn missing_supabase_functions(&self, project: Uuid) -> Result<Vec<String>> {
        let mut names = self
            .missing_supabase_functions
            .lock()
            .map_err(|_| anyhow::anyhow!("missing function status lock poisoned"))?
            .get(&project)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect::<Vec<_>>();
        names.sort();
        Ok(names)
    }

    pub(crate) fn record_supabase_function(
        &self,
        project: Uuid,
        operation: &str,
        missing: bool,
    ) -> Result<()> {
        let mut state = self
            .missing_supabase_functions
            .lock()
            .map_err(|_| anyhow::anyhow!("missing function status lock poisoned"))?;
        let names = state.entry(project).or_default();
        let name = operation.to_owned();
        if missing {
            names.insert(name);
        } else {
            names.remove(&name);
        }
        let empty = names.is_empty();
        if empty {
            state.remove(&project);
        }
        Ok(())
    }
}

impl Engine {
    pub fn device_card(&self, addresses: Vec<String>) -> Result<protocol::DeviceCard> {
        let card = self
            .identity
            .sign_card(network::peer_id(&self.identity)?, addresses)?;
        network::validate_card(&card)?;
        Ok(card)
    }
    pub fn pair(&self, project: Uuid, card: protocol::DeviceCard) -> Result<()> {
        network::validate_card(&card)?;
        ensure!(
            card.device_id != self.identity.device_id,
            "cannot pair this device with itself"
        );
        self.store.trust(project, &card)
    }
    pub fn revoke(&self, project: Uuid, device: Uuid) -> Result<RevokeOutcome> {
        self.store.project(project)?;
        self.store.revoke(project, device)?;
        let mut settings = self
            .settings
            .lock()
            .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?;
        let config = settings
            .value
            .project_backends
            .get(&project)
            .cloned()
            .unwrap_or_default();
        if config.backend == ProjectBackend::Supabase
            && settings.value.imported_projects.contains(&project)
        {
            settings.cancel_grant(&self.directory, project, device)?;
            return Ok(RevokeOutcome::ImportedSupabase);
        }
        if config.backend == ProjectBackend::Supabase {
            settings.enqueue_revoke(&self.directory, project, device)?;
            Ok(RevokeOutcome::Queued)
        } else {
            settings.cancel_grant(&self.directory, project, device)?;
            Ok(RevokeOutcome::LocalOnly)
        }
    }
    /// Takes a project off the list; the folder and its files stay. Every collaborator loses access
    /// (on the invite service too when this PC created the project). With `delete_data` this PC's
    /// versions go now and the project key once the queued removals are sent; without it the
    /// project can be restored.
    pub fn remove_project(&self, project: Uuid, delete_data: bool) -> Result<()> {
        self.store.project(project)?;
        for peer in self.store.peers(Some(project))? {
            self.revoke(project, peer.device_id)?;
        }
        self.store.remove_project(project, delete_data)
    }
    pub fn restore_project(&self, project: Uuid) -> Result<()> {
        self.store.restore_project(project)
    }
    /// Only the device that created a project invites and approves; a device that joined it does not.
    pub(crate) fn ensure_project_creator(&self, project: Uuid) -> Result<()> {
        let (_, imported) = self.project_backend_with_imported(project)?;
        ensure!(
            !imported,
            "Only the device that created this project can invite or approve collaborators"
        );
        Ok(())
    }
    pub fn export_invite(
        &self,
        project: Uuid,
        addresses: Vec<String>,
    ) -> Result<protocol::ProjectInvite> {
        use base64::Engine as _;
        let p = self.store.project(project)?;
        self.ensure_project_creator(project)?;
        let key = crypto::project_key(&self.directory.join("secrets/projects"), project, None)?;
        let backend = self.project_backend_config(project)?;
        let backend = match backend.backend {
            ProjectBackend::Supabase => Some(protocol::InviteBackendConfig {
                backend: protocol::InviteBackend::Supabase,
                supabase_url: backend
                    .supabase_url
                    .context("Supabase URL is unavailable")?,
                supabase_anon_key: backend
                    .supabase_anon_key
                    .context("Supabase publishable key is unavailable")?,
            }),
            ProjectBackend::None | ProjectBackend::SelfHostedMailbox => None,
        };
        let mut invite = protocol::ProjectInvite {
            version: protocol::PROTOCOL_VERSION,
            project_id: project,
            project_name: p.name,
            project_key: base64::engine::general_purpose::STANDARD.encode(*key),
            // Mailbox and relay were removed; both fields stay empty in the signed format.
            mailbox_url: None,
            relay_address: None,
            backend,
            device: self.device_card(addresses)?,
            signature: None,
        };
        if let Err(error) = self.identity.sign_project_invite(&mut invite) {
            crypto::zeroize_project_invite(&mut invite);
            return Err(error);
        }
        Ok(invite)
    }
    /// Folder of a project this PC already had; an invite for it goes back there.
    pub fn rejoin_folder(&self, project: Uuid) -> Result<Option<PathBuf>> {
        Ok(self.store.known_root(project)?.map(PathBuf::from))
    }
    pub fn import_invite(&self, mut invite: protocol::ProjectInvite, folder: &Path) -> Result<()> {
        use base64::Engine as _;
        let validation = (|| -> Result<()> {
            protocol::require_protocol(invite.version)?;
            network::validate_card(&invite.device)?;
            ensure!(!invite.project_id.is_nil(), "invalid invitation project");
            if invite.signature.is_some() || invite.backend.is_some() {
                crypto::verify_project_invite(&invite)?;
            }
            if let Some(info) = &invite.backend {
                match info.backend {
                    protocol::InviteBackend::Supabase => {
                        ::supabase_client::validate_supabase_project_url(&info.supabase_url)?;
                        ::supabase_client::validate_supabase_publishable_key(
                            &info.supabase_anon_key,
                        )?;
                    }
                }
            }
            ensure!(
                invite.device.device_id != self.identity.device_id,
                "invitation is from this device"
            );
            Ok(())
        })();
        if let Err(error) = validation {
            crypto::zeroize_project_invite(&mut invite);
            return Err(error);
        }
        let encoded_key = zeroize::Zeroizing::new(std::mem::take(&mut invite.project_key));
        let invite_anon_key = invite
            .backend
            .as_mut()
            .map(|info| zeroize::Zeroizing::new(std::mem::take(&mut info.supabase_anon_key)));
        let key: [u8; 32] = base64::engine::general_purpose::STANDARD
            .decode(&*encoded_key)?
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid project key"))?;
        let key = zeroize::Zeroizing::new(key);
        let backend = match &invite.backend {
            Some(info) => Some(ProjectBackendConfig {
                backend: ProjectBackend::Supabase,
                supabase_url: Some(info.supabase_url.clone()),
                supabase_anon_key: Some(
                    invite_anon_key
                        .as_ref()
                        .context("invalid Supabase publishable key")?
                        .to_string(),
                ),
            }),
            None => None,
        };
        crypto::project_key(
            &self.directory.join("secrets/projects"),
            invite.project_id,
            Some(&key),
        )?;
        let (previous_backend, marked_imported) = {
            let mut settings = self
                .settings
                .lock()
                .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?;
            let previous_backend = settings
                .value
                .project_backends
                .get(&invite.project_id)
                .cloned();
            let marked_imported = settings.mark_imported(&self.directory, invite.project_id)?;
            (previous_backend, marked_imported)
        };
        let imported_project = invite.project_id;
        let result = (|| -> Result<()> {
            // A project this PC left or was removed from comes back in its old folder; the key
            // check above already required the same collaboration key.
            let folder = match self.rejoin_folder(invite.project_id)? {
                Some(root) => root,
                None => folder.to_path_buf(),
            };
            // Older invitations may carry mailbox_url/relay_address; they are ignored.
            self.link(invite.project_id, &invite.project_name, &folder)?;
            if let Some(backend) = backend {
                self.set_project_backend_config(invite.project_id, backend)?;
            }
            self.pair(invite.project_id, invite.device.clone())?;
            if self.membership_removed(invite.project_id)? {
                self.set_membership_removed(invite.project_id, false)?;
                self.set_supabase_project_state(
                    invite.project_id,
                    SupabaseProjectState::WaitingForGrant,
                )?;
            }
            self.store.rejoin_project(invite.project_id)
        })();
        if let Err(error) = result {
            self.settings
                .lock()
                .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?
                .rollback_import(
                    &self.directory,
                    imported_project,
                    previous_backend,
                    marked_imported,
                )
                .context("could not roll back imported-project backend and marker")?;
            return Err(error);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RevokeOutcome {
    LocalOnly,
    Queued,
    ImportedSupabase,
}

pub fn default_data_directory() -> Result<PathBuf> {
    Ok(PathBuf::from(
        std::env::var_os("LOCALAPPDATA")
            .ok_or_else(|| anyhow::anyhow!("LOCALAPPDATA is unavailable"))?,
    )
    .join("TrackMesh"))
}
