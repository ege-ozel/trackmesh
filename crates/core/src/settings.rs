//! Versioned, atomic local settings that do not alter the project database.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

const VERSION: u32 = 6;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
pub enum ProjectBackend {
    #[default]
    None,
    /// Read-only compatibility value from older settings files; behaves like `None`.
    SelfHostedMailbox,
    Supabase,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SupabaseProjectState {
    NotConfigured,
    #[default]
    Unknown,
    Claimed,
    Member,
    RouteTaken,
    WaitingForGrant,
    Unreachable,
    /// The invite service stopped accepting this member: the creator removed it.
    Removed,
}

/// In-memory-only status for the most recent invite-service grant attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GrantProblem {
    Rejected,
    Unreachable,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectBackendConfig {
    pub backend: ProjectBackend,
    pub supabase_url: Option<String>,
    pub supabase_anon_key: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupabaseDefaultConfig {
    pub url: String,
    pub publishable_key: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManualPublicEndpoint {
    pub public_ipv4: String,
    pub external_port: u16,
}

fn compiled_default_supabase() -> Option<SupabaseDefaultConfig> {
    let url = option_env!("TRACKMESH_DEFAULT_SUPABASE_URL")?;
    let publishable_key = option_env!("TRACKMESH_DEFAULT_SUPABASE_PUBLISHABLE_KEY")?;
    Some(SupabaseDefaultConfig {
        url: url.to_owned(),
        publishable_key: publishable_key.to_owned(),
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingGrant {
    pub project_id: Uuid,
    pub target_device_id: Uuid,
    pub target_public_key: String,
    pub actor_seq: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingRevoke {
    pub project_id: Uuid,
    pub target_device_id: Uuid,
    pub actor_seq: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Settings {
    version: u32,
    upnp_mapping_enabled: bool,
    #[serde(default)]
    upnp_mapping_asked: bool,
    #[serde(default)]
    pub project_backends: HashMap<Uuid, ProjectBackendConfig>,
    #[serde(default)]
    pub actor_sequences: HashMap<Uuid, u64>,
    #[serde(default)]
    pub pending_grants: Vec<PendingGrant>,
    #[serde(default)]
    pub pending_revokes: Vec<PendingRevoke>,
    #[serde(default)]
    pub imported_projects: std::collections::HashSet<Uuid>,
    #[serde(default = "compiled_default_supabase")]
    pub default_supabase: Option<SupabaseDefaultConfig>,
    #[serde(default)]
    pub device_name: Option<String>,
    #[serde(default)]
    pub projects_location: Option<String>,
    #[serde(default)]
    pub manual_public_endpoint: Option<ManualPublicEndpoint>,
    #[serde(default)]
    pub tray_notice_shown: bool,
    #[serde(default)]
    pub auto_share_projects: std::collections::BTreeSet<Uuid>,
    /// Joined projects whose creator removed this PC; left out while empty so older builds still read the file.
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub removed_memberships: std::collections::BTreeSet<Uuid>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: VERSION,
            upnp_mapping_enabled: false,
            upnp_mapping_asked: false,
            project_backends: HashMap::new(),
            actor_sequences: HashMap::new(),
            pending_grants: Vec::new(),
            pending_revokes: Vec::new(),
            imported_projects: std::collections::HashSet::new(),
            default_supabase: compiled_default_supabase(),
            device_name: None,
            projects_location: None,
            manual_public_endpoint: None,
            tray_notice_shown: false,
            auto_share_projects: std::collections::BTreeSet::new(),
            removed_memberships: std::collections::BTreeSet::new(),
        }
    }
}

impl Settings {
    pub fn upnp_mapping_enabled(&self) -> bool {
        self.upnp_mapping_enabled
    }
    pub fn set_upnp_mapping_enabled(&mut self, enabled: bool) {
        self.upnp_mapping_enabled = enabled;
        self.upnp_mapping_asked = true;
    }
    pub fn upnp_mapping_asked(&self) -> bool {
        self.upnp_mapping_asked
    }
}

#[derive(Clone)]
pub(super) struct SettingsState {
    pub value: Settings,
    writable: bool,
}

impl SettingsState {
    pub fn retain_auto_share_projects(
        &mut self,
        directory: &Path,
        linked: &std::collections::BTreeSet<Uuid>,
    ) -> Result<()> {
        if self.value.auto_share_projects.is_subset(linked) {
            return Ok(());
        }
        let mut candidate = self.value.clone();
        candidate
            .auto_share_projects
            .retain(|id| linked.contains(id));
        self.commit(directory, candidate)
    }
    pub fn set_auto_share(
        &mut self,
        directory: &Path,
        project: Uuid,
        enabled: bool,
        linked: &std::collections::BTreeSet<Uuid>,
    ) -> Result<()> {
        ensure!(linked.contains(&project), "project is no longer linked");
        let mut candidate = self.value.clone();
        candidate
            .auto_share_projects
            .retain(|id| linked.contains(id));
        if enabled {
            candidate.auto_share_projects.insert(project);
        } else {
            candidate.auto_share_projects.remove(&project);
        }
        self.commit(directory, candidate)
    }
    pub fn set_membership_removed(
        &mut self,
        directory: &Path,
        project: Uuid,
        removed: bool,
    ) -> Result<()> {
        if self.value.removed_memberships.contains(&project) == removed {
            return Ok(());
        }
        let mut candidate = self.value.clone();
        if removed {
            candidate.removed_memberships.insert(project);
        } else {
            candidate.removed_memberships.remove(&project);
        }
        self.commit(directory, candidate)
    }
    pub fn mark_tray_notice_shown(&mut self, directory: &Path) -> Result<()> {
        let mut candidate = self.value.clone();
        candidate.tray_notice_shown = true;
        self.commit(directory, candidate)
    }
    pub fn load(directory: &Path) -> Self {
        let path = directory.join("settings.json");
        if !path.exists() {
            return Self {
                value: Settings::default(),
                writable: true,
            };
        }
        let loaded = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Settings>(&bytes).ok());
        match loaded {
            Some(mut value) if (1..=VERSION).contains(&value.version) => {
                value.version = VERSION;
                Self {
                    value,
                    writable: true,
                }
            }
            _ => Self {
                value: Settings::default(),
                writable: false,
            },
        }
    }

    fn commit(&mut self, directory: &Path, mut candidate: Settings) -> Result<()> {
        ensure!(
            self.writable,
            "settings are invalid or from a newer version; refusing to overwrite them"
        );
        candidate.version = VERSION;
        store(directory, &candidate)?;
        self.value = candidate;
        Ok(())
    }

    pub fn set_upnp_mapping(&mut self, directory: &Path, enabled: bool) -> Result<()> {
        let mut candidate = self.value.clone();
        candidate.set_upnp_mapping_enabled(enabled);
        self.commit(directory, candidate)
    }

    pub fn set_project_backend(
        &mut self,
        directory: &Path,
        project: Uuid,
        config: ProjectBackendConfig,
    ) -> Result<()> {
        let mut candidate = self.value.clone();
        candidate.project_backends.insert(project, config);
        self.commit(directory, candidate)
    }

    pub fn set_default_supabase(
        &mut self,
        directory: &Path,
        config: Option<SupabaseDefaultConfig>,
    ) -> Result<()> {
        let mut candidate = self.value.clone();
        candidate.default_supabase = config;
        self.commit(directory, candidate)
    }

    pub fn set_device_name(&mut self, directory: &Path, name: String) -> Result<()> {
        let mut candidate = self.value.clone();
        candidate.device_name = Some(name);
        self.commit(directory, candidate)
    }

    pub fn set_projects_location(&mut self, directory: &Path, location: String) -> Result<()> {
        let mut candidate = self.value.clone();
        candidate.projects_location = Some(location);
        self.commit(directory, candidate)
    }

    pub fn set_manual_public_endpoint(
        &mut self,
        directory: &Path,
        endpoint: Option<ManualPublicEndpoint>,
    ) -> Result<()> {
        let mut candidate = self.value.clone();
        candidate.manual_public_endpoint = endpoint;
        self.commit(directory, candidate)
    }

    pub fn enqueue_grant(
        &mut self,
        directory: &Path,
        project_id: Uuid,
        target_device_id: Uuid,
        target_public_key: String,
    ) -> Result<PendingGrant> {
        if let Some(existing) = self.value.pending_grants.iter().find(|grant| {
            grant.project_id == project_id && grant.target_device_id == target_device_id
        }) {
            ensure!(
                existing.target_public_key == target_public_key,
                "pending grant identity changed"
            );
            return Ok(existing.clone());
        }
        let mut candidate = self.value.clone();
        let actor_seq = next_sequence(&mut candidate, project_id)?;
        let grant = PendingGrant {
            project_id,
            target_device_id,
            target_public_key,
            actor_seq,
        };
        candidate.pending_grants.push(grant.clone());
        self.commit(directory, candidate)?;
        Ok(grant)
    }

    pub fn pending_grants(&self, project_id: Option<Uuid>) -> Vec<PendingGrant> {
        self.value
            .pending_grants
            .iter()
            .filter(|grant| project_id.is_none_or(|project| grant.project_id == project))
            .cloned()
            .collect()
    }

    pub fn next_sequence(&mut self, directory: &Path, project_id: Uuid) -> Result<u64> {
        let mut candidate = self.value.clone();
        let sequence = next_sequence(&mut candidate, project_id)?;
        self.commit(directory, candidate)?;
        Ok(sequence)
    }

    pub fn complete_grant(&mut self, directory: &Path, grant: &PendingGrant) -> Result<()> {
        let mut candidate = self.value.clone();
        candidate.pending_grants.retain(|item| {
            item.project_id != grant.project_id
                || item.target_device_id != grant.target_device_id
                || item.actor_seq != grant.actor_seq
        });
        self.commit(directory, candidate)
    }

    pub fn enqueue_revoke(
        &mut self,
        directory: &Path,
        project_id: Uuid,
        target_device_id: Uuid,
    ) -> Result<PendingRevoke> {
        let mut candidate = self.value.clone();
        candidate.pending_grants.retain(|grant| {
            grant.project_id != project_id || grant.target_device_id != target_device_id
        });
        candidate.pending_revokes.retain(|revoke| {
            revoke.project_id != project_id || revoke.target_device_id != target_device_id
        });
        let actor_seq = next_sequence(&mut candidate, project_id)?;
        let revoke = PendingRevoke {
            project_id,
            target_device_id,
            actor_seq,
        };
        candidate.pending_revokes.push(revoke.clone());
        self.commit(directory, candidate)?;
        Ok(revoke)
    }

    pub fn cancel_grant(
        &mut self,
        directory: &Path,
        project_id: Uuid,
        target_device_id: Uuid,
    ) -> Result<()> {
        if !self
            .value
            .pending_grants
            .iter()
            .any(|item| item.project_id == project_id && item.target_device_id == target_device_id)
        {
            return Ok(());
        }
        let mut candidate = self.value.clone();
        candidate.pending_grants.retain(|item| {
            item.project_id != project_id || item.target_device_id != target_device_id
        });
        self.commit(directory, candidate)
    }

    pub fn pending_revokes(&self, project_id: Option<Uuid>) -> Vec<PendingRevoke> {
        self.value
            .pending_revokes
            .iter()
            .filter(|item| project_id.is_none_or(|project| item.project_id == project))
            .cloned()
            .collect()
    }

    pub fn complete_revoke(&mut self, directory: &Path, revoke: &PendingRevoke) -> Result<()> {
        let mut candidate = self.value.clone();
        candidate.pending_revokes.retain(|item| {
            item.project_id != revoke.project_id
                || item.target_device_id != revoke.target_device_id
                || item.actor_seq != revoke.actor_seq
        });
        self.commit(directory, candidate)
    }

    pub fn replace_grant_sequence(&mut self, directory: &Path, grant: &PendingGrant) -> Result<()> {
        let mut candidate = self.value.clone();
        let Some(index) = candidate.pending_grants.iter().position(|item| {
            item.project_id == grant.project_id
                && item.target_device_id == grant.target_device_id
                && item.actor_seq == grant.actor_seq
        }) else {
            return Ok(());
        };
        let sequence = next_sequence(&mut candidate, grant.project_id)?;
        candidate.pending_grants[index].actor_seq = sequence;
        self.commit(directory, candidate)
    }

    pub fn mark_imported(&mut self, directory: &Path, project: Uuid) -> Result<bool> {
        if self.value.imported_projects.contains(&project) {
            return Ok(false);
        }
        let mut candidate = self.value.clone();
        candidate.imported_projects.insert(project);
        self.commit(directory, candidate)?;
        Ok(true)
    }

    pub fn rollback_import(
        &mut self,
        directory: &Path,
        project: Uuid,
        previous_backend: Option<ProjectBackendConfig>,
        marked_imported: bool,
    ) -> Result<()> {
        let mut candidate = self.value.clone();
        if let Some(config) = previous_backend {
            candidate.project_backends.insert(project, config);
        } else {
            candidate.project_backends.remove(&project);
        }
        if marked_imported {
            candidate.imported_projects.remove(&project);
        }
        // Restore the backend and marker together. A failed write retains the
        // imported marker, so a partially imported project cannot claim a route.
        self.commit(directory, candidate)
    }
}

fn next_sequence(candidate: &mut Settings, project_id: Uuid) -> Result<u64> {
    let previous = candidate
        .actor_sequences
        .get(&project_id)
        .copied()
        .unwrap_or(0);
    let next = previous
        .checked_add(1)
        .context("Supabase actor sequence exhausted")?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let now = u64::try_from(now).context("Unix millisecond value exceeds u64")?;
    let sequence = next.max(now);
    candidate.actor_sequences.insert(project_id, sequence);
    Ok(sequence)
}

fn store(directory: &Path, settings: &Settings) -> Result<()> {
    let destination = directory.join("settings.json");
    let temporary = directory.join(format!("settings.json.{}.tmp", Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let bytes = serde_json::to_vec(settings)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .context("could not create temporary settings file")?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        snapshot::atomic_replace(&temporary, &destination)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
