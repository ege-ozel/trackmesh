// Mailbox-only protocol types removed from shared/protocol (lib.rs and mailbox.rs).
// They were still exported after d447de1 but nothing in the product used them.
// Wire/domain strings ("trackmesh-mailbox-v1", "trackmesh-mailbox-request-v1") must stay
// unchanged if this comes back, or older mailbox data stops verifying.

pub const MAX_MAILBOX_PAYLOAD: usize = 64 * 1024;
pub const MAX_MAILBOX_ENVELOPE_BYTES: usize = 128 * 1024;
pub const MAX_MAILBOX_PAGE_RECORDS: usize = 32;
pub const MAX_MAILBOX_PAGE_BYTES: usize = 5 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Announcement {
    pub parent_snapshot_id: Option<Uuid>,
    pub manifest_hash: String,
    pub total_size: u64,
    pub changed_bytes: u64,
    pub file_count: u32,
    pub author_name: String,
    pub description: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub version: u32,
    pub project_id: Uuid,
    pub snapshot_id: Uuid,
    pub author_device_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub nonce: String,
    pub encrypted_payload: String,
    pub public_key: String,
    pub signature: String,
}
impl Envelope {
    pub fn aad(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(&(
            "trackmesh-mailbox-v1",
            self.version,
            self.project_id,
            self.snapshot_id,
            self.author_device_id,
            self.created_at,
            &self.public_key,
        ))?)
    }
    pub fn signing_bytes(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(&(
            self.aad()?,
            &self.nonce,
            &self.encrypted_payload,
        ))?)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MailboxRecord {
    pub sequence: i64,
    pub envelope: Envelope,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MailboxPage {
    pub records: Vec<MailboxRecord>,
    pub next_cursor: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MailboxAck {
    pub snapshot_id: Uuid,
    pub sequence: i64,
}


#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MailboxAuth {
    pub version: u32,
    pub project_id: Uuid,
    pub device_id: Uuid,
    pub public_key: String,
    pub issued_at: DateTime<Utc>,
    pub nonce: Uuid,
    pub scope: String,
    pub body_hash: String,
    pub signature: String,
}

impl MailboxAuth {
    pub fn signing_bytes(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(&(
            "trackmesh-mailbox-request-v1",
            self.version,
            self.project_id,
            self.device_id,
            &self.public_key,
            self.issued_at,
            self.nonce,
            &self.scope,
            &self.body_hash,
        ))?)
    }
}

pub fn mailbox_scope(cursor: Option<i64>) -> String {
    match cursor {
        Some(cursor) => format!("GET announcements after {cursor}"),
        None => "POST announcement".into(),
    }
}
