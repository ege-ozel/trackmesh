//! Persisted records and SQLite row decoding. Public types remain re-exported at crate root.
use crate::TransferState;
use protocol::EndpointRecord;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub project_id: Uuid,
    pub name: String,
    pub root: String,
    pub latest_local_snapshot: Option<Uuid>,
    pub last_verified_at: Option<String>,
    pub verified_peer_id: Option<String>,
    pub verified_snapshot_id: Option<Uuid>,
    pub error: Option<String>,
    pub recovery_required: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnapshotInfo {
    pub snapshot_id: Uuid,
    pub project_id: Uuid,
    pub parent_snapshot_id: Option<Uuid>,
    pub author_device_id: Uuid,
    pub created_at: String,
    pub manifest_hash: String,
    pub location: String,
    pub metadata_status: String,
    pub local_sequence: Option<u64>,
    pub total_size: u64,
    pub changed_bytes: u64,
    pub file_count: u32,
    pub author_name: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransferInfo {
    pub transfer_id: Uuid,
    pub project_id: Uuid,
    pub snapshot_id: Uuid,
    pub source_device_id: Uuid,
    pub state: TransferState,
    pub completed_bytes: u64,
    pub total_bytes: u64,
    pub error: Option<String>,
    pub base_snapshot_id: Option<Uuid>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerEndpointState {
    /// None when the stored endpoint has expired; the replay watermark remains available.
    pub record: Option<EndpointRecord>,
    pub highest_seq: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndpointWriteOutcome {
    Stored,
    Duplicate,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Lineage {
    Equal,
    RemoteAhead,
    LocalAhead,
    Diverged,
    Unknown,
}

pub(super) fn uuid_value(value: String) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(&value).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}
pub(super) fn optional_uuid(value: Option<String>) -> rusqlite::Result<Option<Uuid>> {
    value.map(uuid_value).transpose()
}
pub(super) fn project_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project {
        project_id: uuid_value(row.get(0)?)?,
        name: row.get(1)?,
        root: row.get(2)?,
        latest_local_snapshot: optional_uuid(row.get(3)?)?,
        last_verified_at: row.get(4)?,
        verified_peer_id: row.get(5)?,
        verified_snapshot_id: optional_uuid(row.get(6)?)?,
        error: row.get(7)?,
        recovery_required: row.get(8)?,
    })
}
pub(super) const PROJECT_COLUMNS: &str = "project_id,name,root,latest_local_snapshot,last_verified_at,verified_peer_id,verified_snapshot_id,error,recovery_required";
pub(super) fn info_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SnapshotInfo> {
    Ok(SnapshotInfo {
        snapshot_id: uuid_value(row.get(0)?)?,
        project_id: uuid_value(row.get(1)?)?,
        parent_snapshot_id: optional_uuid(row.get(2)?)?,
        author_device_id: uuid_value(row.get(3)?)?,
        created_at: row.get(4)?,
        manifest_hash: row.get(5)?,
        location: row.get(6)?,
        metadata_status: row.get(7)?,
        local_sequence: row.get(8)?,
        total_size: row.get(9)?,
        changed_bytes: row.get(10)?,
        file_count: row.get(11)?,
        author_name: row.get(12)?,
    })
}
pub(super) const INFO_COLUMNS: &str = "snapshot_id,project_id,parent_snapshot_id,author_device_id,created_at,manifest_hash,location,metadata_status,local_sequence,total_size,changed_bytes,file_count,author_name";
