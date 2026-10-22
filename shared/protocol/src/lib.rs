use anyhow::{bail, ensure, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;
mod endpoint;
mod pairing;
mod supabase;
pub use endpoint::{
    validate_endpoint_address, EndpointRecord, EndpointSequence, ENDPOINT_DOMAIN,
    MAX_ENDPOINT_ADDRESSES, MAX_ENDPOINT_RECORD_BYTES, MAX_ENDPOINT_REQUEST_BYTES,
    MAX_ENDPOINT_TTL, PREFERRED_ENDPOINT_TTL,
};
pub use pairing::{
    pairing_scope, PairingRequest, PairingResponse, MAX_PAIRING_BYTES, PAIRING_PROTOCOL,
};
pub use supabase::{
    EndpointMailboxWrapper, HeadMailboxWrapper, HeadRecord, SupabaseAccepted, SupabaseAuth,
    SupabaseEndpointRow, SupabaseHeadRow, ENDPOINT_MAILBOX_AAD, HEAD_MAILBOX_AAD,
    MAX_MAILBOX_AUTH_BYTES, SUPABASE_REQUEST_DOMAIN,
};

pub const PROTOCOL_VERSION: u32 = 1;
pub const DEFAULT_CHUNK_SIZE: u32 = 4 * 1024 * 1024;
pub const MAX_CHUNK_SIZE: u32 = 8 * 1024 * 1024;
pub const MAX_MANIFEST_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_FILES: usize = 100_000;
pub const MAX_PROJECT_BYTES: u64 = 1024 * 1024 * 1024 * 1024;
pub const MAX_IPC_BYTES: usize = 64 * 1024;

pub fn hash(data: &[u8]) -> String {
    blake3::hash(data).to_hex().to_string()
}
pub fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

// Reject Windows aliases as well as traversal: the same manifest must name one unambiguous path.
pub fn validate_relative_path(path: &str) -> Result<()> {
    ensure!(
        !path.is_empty() && path.len() <= 4096,
        "invalid relative path length"
    );
    ensure!(
        !path.contains('\\') && !path.starts_with('/'),
        "path must use relative forward slashes"
    );
    for component in path.split('/') {
        ensure!(
            !component.is_empty() && component != "." && component != "..",
            "invalid path component"
        );
        ensure!(
            !component.ends_with('.') && !component.ends_with(' '),
            "ambiguous Windows filename"
        );
        ensure!(
            !component.chars().any(|c| c < ' ' || "<>:\"|?*".contains(c)),
            "invalid Windows filename"
        );
        let stem = component.split('.').next().unwrap_or("").to_uppercase();
        let reserved = ["CON", "PRN", "AUX", "NUL", "CLOCK$", "CONIN$", "CONOUT$"];
        ensure!(
            !reserved.contains(&stem.as_str()),
            "reserved Windows filename"
        );
        let numbered = stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"));
        ensure!(
            !numbered.is_some_and(
                |s| ["1", "2", "3", "4", "5", "6", "7", "8", "9", "¹", "²", "³"].contains(&s)
            ),
            "reserved Windows filename"
        );
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Chunk {
    pub offset: u64,
    pub size: u32,
    pub blake3: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SnapshotFile {
    pub relative_path: String,
    pub size: u64,
    pub blake3: String,
    pub chunks: Vec<Chunk>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u32,
    pub snapshot_id: Uuid,
    pub project_id: Uuid,
    pub parent_snapshot_id: Option<Uuid>,
    pub author_device_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub chunk_size: u32,
    pub files: Vec<SnapshotFile>,
    pub total_size: u64,
    pub changed_bytes: u64,
}

impl Manifest {
    pub fn digest(&self) -> Result<String> {
        Ok(hash(&serde_json::to_vec(self)?))
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == PROTOCOL_VERSION,
            "unsupported manifest version"
        );
        ensure!(
            self.chunk_size > 0 && self.chunk_size <= MAX_CHUNK_SIZE,
            "invalid chunk size"
        );
        ensure!(self.files.len() <= MAX_FILES, "too many files");
        ensure!(
            self.parent_snapshot_id != Some(self.snapshot_id),
            "snapshot cannot parent itself"
        );
        let mut names = HashSet::new();
        let mut chunk_sizes = HashMap::new();
        let mut total = 0u64;
        let mut previous = None;
        for file in &self.files {
            validate_relative_path(&file.relative_path)?;
            ensure!(
                names.insert(file.relative_path.to_lowercase()),
                "Windows filename collision"
            );
            ensure!(
                previous.is_none_or(|p: &str| p < file.relative_path.as_str()),
                "manifest paths must be sorted"
            );
            previous = Some(file.relative_path.as_str());
            ensure!(valid_hash(&file.blake3), "invalid file digest");
            ensure!(
                file.size != 0 || file.blake3 == hash(&[]),
                "empty file digest mismatch"
            );
            let mut offset = 0u64;
            for (index, chunk) in file.chunks.iter().enumerate() {
                ensure!(valid_hash(&chunk.blake3), "invalid chunk digest");
                if let Some(size) = chunk_sizes.insert(&chunk.blake3, chunk.size) {
                    ensure!(size == chunk.size, "one chunk digest has conflicting sizes");
                }
                ensure!(
                    chunk.offset == offset && chunk.size > 0 && chunk.size <= self.chunk_size,
                    "invalid chunk bounds"
                );
                ensure!(
                    index + 1 == file.chunks.len() || chunk.size == self.chunk_size,
                    "nonfinal chunk must have fixed size"
                );
                offset = offset
                    .checked_add(chunk.size as u64)
                    .ok_or_else(|| anyhow::anyhow!("size overflow"))?;
            }
            ensure!(offset == file.size, "chunks do not cover file");
            total = total
                .checked_add(file.size)
                .ok_or_else(|| anyhow::anyhow!("size overflow"))?;
            ensure!(total <= MAX_PROJECT_BYTES, "project too large");
        }
        for name in &names {
            for (index, _) in name.match_indices('/') {
                ensure!(
                    !names.contains(&name[..index]),
                    "file/directory path collision"
                );
            }
        }
        ensure!(
            total == self.total_size && self.changed_bytes <= total,
            "invalid aggregate sizes"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedSnapshot {
    pub manifest: Manifest,
    pub manifest_hash: String,
    pub author_public_key: String,
    pub signature: String,
}
impl SignedSnapshot {
    pub fn validate_structure(&self) -> Result<()> {
        self.manifest.validate()?;
        ensure!(
            self.manifest.digest()? == self.manifest_hash,
            "manifest digest mismatch"
        );
        ensure!(
            serde_json::to_vec(self)?.len() <= MAX_MANIFEST_BYTES,
            "manifest too large"
        );
        Ok(())
    }
    pub fn signing_bytes(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(&(
            "trackmesh-snapshot-v1",
            &self.manifest_hash,
            self.manifest.project_id,
            self.manifest.snapshot_id,
        ))?)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceCard {
    pub version: u32,
    pub device_id: Uuid,
    pub name: String,
    pub peer_id: String,
    pub public_key: String,
    pub addresses: Vec<String>,
    pub signature: String,
}
impl DeviceCard {
    pub fn signing_bytes(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(&(
            "trackmesh-device-v1",
            self.version,
            self.device_id,
            &self.name,
            &self.peer_id,
            &self.public_key,
            &self.addresses,
        ))?)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectInvite {
    pub version: u32,
    pub project_id: Uuid,
    pub project_name: String,
    pub project_key: String,
    pub mailbox_url: Option<String>,
    pub relay_address: Option<String>,
    #[serde(default)]
    pub backend: Option<InviteBackendConfig>,
    pub device: DeviceCard,
    #[serde(default)]
    pub signature: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InviteBackendConfig {
    pub backend: InviteBackend,
    pub supabase_url: String,
    pub supabase_anon_key: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InviteBackend {
    Supabase,
}

impl ProjectInvite {
    pub fn signing_bytes(&self) -> anyhow::Result<Vec<u8>> {
        Ok(serde_json::to_vec(&(
            "trackmesh-project-invite-v1",
            self.version,
            self.project_id,
            &self.project_name,
            &self.project_key,
            &self.mailbox_url,
            &self.relay_address,
            &self.backend,
            self.device.signing_bytes()?,
            &self.device.signature,
        ))?)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "event",
    rename_all = "SCREAMING_SNAKE_CASE",
    deny_unknown_fields
)]
pub enum PluginEvent {
    PluginConnected {
        session_id: Uuid,
        project_id: Uuid,
    },
    ProjectId {
        session_id: Uuid,
        project_id: Uuid,
    },
    SaveObserved {
        session_id: Uuid,
        project_id: Uuid,
    },
    SendSnapshot {
        session_id: Uuid,
        project_id: Uuid,
    },
    ReceiveSnapshot {
        session_id: Uuid,
        project_id: Uuid,
    },
    SessionEnding {
        session_id: Uuid,
        project_id: Uuid,
    },
    GetStatus {
        session_id: Uuid,
        project_id: Uuid,
    },
    SetAutoShare {
        session_id: Uuid,
        project_id: Uuid,
        enabled: bool,
    },
}
impl PluginEvent {
    pub fn ids(&self) -> (Uuid, Uuid) {
        match *self {
            Self::PluginConnected {
                session_id,
                project_id,
            }
            | Self::ProjectId {
                session_id,
                project_id,
            }
            | Self::SaveObserved {
                session_id,
                project_id,
            }
            | Self::SendSnapshot {
                session_id,
                project_id,
            }
            | Self::ReceiveSnapshot {
                session_id,
                project_id,
            }
            | Self::SessionEnding {
                session_id,
                project_id,
            }
            | Self::GetStatus {
                session_id,
                project_id,
            }
            | Self::SetAutoShare {
                session_id,
                project_id,
                ..
            } => (session_id, project_id),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginProject {
    pub project_id: Uuid,
    pub name: String,
    pub folder_name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginStatus {
    pub ok: bool,
    pub project_id: Uuid,
    pub project_name: String,
    pub local_snapshot: Option<String>,
    pub remote_snapshot: Option<String>,
    pub author: Option<String>,
    pub created_at: Option<String>,
    pub peer_online: bool,
    pub connection: String,
    pub state: String,
    pub message: String,
    #[serde(default)]
    pub projects: Vec<PluginProject>,
    #[serde(default)]
    pub projects_more: bool,
    #[serde(default)]
    pub picker_supported: bool,
    #[serde(default)]
    pub desktop_available: bool,
    #[serde(default)]
    pub status_line: String,
    #[serde(default)]
    pub share_enabled: bool,
    #[serde(default)]
    pub get_enabled: bool,
    #[serde(default)]
    pub pending_publications: u64,
    #[serde(default)]
    pub auto_share_supported: bool,
    #[serde(default)]
    pub auto_share: bool,
    #[serde(default)]
    pub presence_warning: String,
    #[serde(default)]
    pub share_confirmation: String,
    #[serde(default)]
    pub presence_danger: bool,
    #[serde(default)]
    pub presence_since: u64,
    #[serde(default)]
    pub presence_get_line: String,
    /// "en" or "tr": the plugin's own labels follow the Desktop language.
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub get_label: String,
    /// Names for the connection badge: the collaborators online now, or all of them when none is.
    #[serde(default)]
    pub collaborators: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresenceSource {
    Plugin,
    Folder,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum PeerRequest {
    Presence {
        project_id: Uuid,
        working: bool,
        since: u64,
        source: PresenceSource,
    },
    SnapshotAvailable {
        project_id: Uuid,
        snapshot_id: Uuid,
    },
    EndpointUpdate {
        project_id: Uuid,
        record: EndpointRecord,
    },
    Head {
        project_id: Uuid,
    },
    Manifest {
        project_id: Uuid,
        snapshot_id: Uuid,
    },
    Chunk {
        project_id: Uuid,
        snapshot_id: Uuid,
        hash: String,
    },
    Verified {
        project_id: Uuid,
        snapshot_id: Uuid,
        manifest_hash: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum PeerResponse {
    SnapshotNoticed,
    EndpointUpdated,
    Head { snapshot: Option<SignedSnapshot> },
    Manifest { snapshot: SignedSnapshot },
    Chunk { hash: String, data: Vec<u8> },
    Verified { equal: bool },
    Error { message: String },
}

pub fn request_project(request: &PeerRequest) -> Uuid {
    match request {
        PeerRequest::Presence { project_id, .. }
        | PeerRequest::SnapshotAvailable { project_id, .. }
        | PeerRequest::EndpointUpdate { project_id, .. }
        | PeerRequest::Head { project_id }
        | PeerRequest::Manifest { project_id, .. }
        | PeerRequest::Chunk { project_id, .. }
        | PeerRequest::Verified { project_id, .. } => *project_id,
    }
}

pub fn require_protocol(version: u32) -> Result<()> {
    if version != PROTOCOL_VERSION {
        bail!("incompatible TrackMesh version");
    }
    Ok(())
}
