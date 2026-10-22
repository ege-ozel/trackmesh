//! Supabase request authentication and encrypted endpoint/head wrappers. Wire strings are stable.
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const MAX_MAILBOX_AUTH_BYTES: usize = 4096;
pub const SUPABASE_REQUEST_DOMAIN: &str = "trackmesh-supabase-request-v1";
pub const ENDPOINT_MAILBOX_AAD: &[u8] = b"trackmesh-endpoint-mailbox-v1";
pub const HEAD_MAILBOX_AAD: &[u8] = b"trackmesh-head-mailbox-v1";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupabaseAuth {
    pub version: u32,
    pub project_route: String,
    pub device_id: String,
    pub public_key: String,
    pub issued_at: String,
    pub nonce: String,
    pub scope: String,
    pub body_hash: String,
    pub signature: String,
}

impl SupabaseAuth {
    pub fn signing_bytes(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(&(
            SUPABASE_REQUEST_DOMAIN,
            self.version,
            &self.project_route,
            &self.device_id,
            &self.public_key,
            &self.issued_at,
            &self.nonce,
            &self.scope,
            &self.body_hash,
        ))?)
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointMailboxWrapper {
    pub version: u32,
    pub project_route: String,
    pub writer_device_id: String,
    pub seq: String,
    pub expires_at: String,
    pub nonce: String,
    pub ciphertext: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupabaseEndpointRow {
    pub cursor: String,
    pub writer_device_id: String,
    pub seq: String,
    pub ciphertext: String,
    pub expires_at: String,
    pub created_at: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeadMailboxWrapper {
    pub version: u32,
    pub project_route: String,
    pub writer_device_id: String,
    pub seq: String,
    pub nonce: String,
    pub ciphertext: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupabaseHeadRow {
    pub writer_device_id: String,
    pub seq: String,
    pub ciphertext: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeadRecord {
    pub snapshot_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub author_device_id: Uuid,
    pub total_size: u64,
    pub file_count: u32,
    pub manifest_hash: String,
    pub signature: String,
}

impl HeadRecord {
    pub fn signing_bytes(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(&(
            "trackmesh-head-record-v1",
            self.snapshot_id,
            self.parent_id,
            self.created_at,
            self.author_device_id,
            self.total_size,
            self.file_count,
            &self.manifest_hash,
        ))?)
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupabaseAccepted {
    pub accepted: bool,
    #[serde(default)]
    pub role: Option<String>,
}
