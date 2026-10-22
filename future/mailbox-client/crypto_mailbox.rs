//! Short-lived signed HTTP requests; no device or project secret enters a header.
use crate::{verify, Identity};
use anyhow::{ensure, Result};
use chrono::{DateTime, Utc};
use protocol::{MailboxAuth, PROTOCOL_VERSION};
use uuid::Uuid;

impl Identity {
    pub fn mailbox_auth(&self, project: Uuid, scope: String, body: &[u8]) -> Result<MailboxAuth> {
        let mut auth = MailboxAuth {
            version: PROTOCOL_VERSION,
            project_id: project,
            device_id: self.device_id,
            public_key: self.public_key(),
            issued_at: Utc::now(),
            nonce: Uuid::new_v4(),
            scope,
            body_hash: protocol::hash(body),
            signature: String::new(),
        };
        auth.signature = self.sign(&auth.signing_bytes()?);
        Ok(auth)
    }
}

pub fn verify_mailbox_auth(
    auth: &MailboxAuth,
    project: Uuid,
    scope: &str,
    body: &[u8],
    now: DateTime<Utc>,
) -> Result<()> {
    protocol::require_protocol(auth.version)?;
    ensure!(
        auth.project_id == project && auth.scope == scope,
        "request binding mismatch"
    );
    ensure!(
        auth.body_hash == protocol::hash(body),
        "request body mismatch"
    );
    ensure!(!auth.nonce.is_nil(), "missing request nonce");
    let age = now.signed_duration_since(auth.issued_at);
    let skew = chrono::Duration::seconds(120);
    ensure!(
        age >= -skew && age <= skew,
        "request expired or clock differs"
    );
    verify(&auth.public_key, &auth.signing_bytes()?, &auth.signature)
}
