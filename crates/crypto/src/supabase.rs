//! Project-key encrypted wrappers for the opaque Supabase endpoint mailbox.
use crate::{fill_random, verify};
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chacha20poly1305::{
    aead::{Aead, Payload},
    KeyInit, XChaCha20Poly1305, XNonce,
};
use chrono::{SecondsFormat, Utc};
use protocol::{
    EndpointMailboxWrapper, EndpointRecord, HeadMailboxWrapper, HeadRecord, ENDPOINT_MAILBOX_AAD,
    HEAD_MAILBOX_AAD,
};
use sha2::{Digest, Sha256};

const WRAPPER_VERSION: u32 = 1;
const MAX_WRAPPER_BYTES: usize = 64 * 1024;
const MAX_HEAD_CIPHERTEXT: usize = 4096;
// A signed record is about 490 bytes; leave room so a valid head is never rejected.
const MAX_HEAD_PLAINTEXT: usize = 1024;

pub fn endpoint_record_expires_at(record: &EndpointRecord) -> String {
    record
        .expires_at
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}

impl super::Identity {
    /// Sign the exact M5d request tuple consumed by Supabase Edge Functions.
    pub fn supabase_auth(
        &self,
        project: uuid::Uuid,
        scope: &str,
        body: &[u8],
    ) -> Result<protocol::SupabaseAuth> {
        ensure!(!project.is_nil(), "Supabase project route cannot be nil");
        ensure!(
            matches!(
                scope,
                "project-claim"
                    | "member-grant"
                    | "member-revoke"
                    | "endpoint-publish"
                    | "endpoint-list"
                    | "head-publish"
                    | "head-list"
                    | "invite-create"
            ),
            "invalid Supabase request scope"
        );
        let mut auth = protocol::SupabaseAuth {
            version: protocol::PROTOCOL_VERSION,
            project_route: project.to_string(),
            device_id: self.device_id.to_string(),
            public_key: self.public_key(),
            issued_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            nonce: uuid::Uuid::new_v4().to_string(),
            scope: scope.to_owned(),
            body_hash: hex::encode(Sha256::digest(body)),
            signature: String::new(),
        };
        auth.signature = self.sign(&auth.signing_bytes()?);
        Ok(auth)
    }
}

pub fn wrap_head_record(
    project_route: uuid::Uuid,
    identity: &super::Identity,
    record: &HeadRecord,
    seq: u64,
    project_key: &[u8; 32],
) -> Result<String> {
    ensure!(
        !project_route.is_nil() && seq > 0,
        "invalid head mailbox metadata"
    );
    let mut signed = record.clone();
    signed.signature = identity.sign(&signed.signing_bytes()?);
    let plaintext = serde_json::to_vec(&signed)?;
    ensure!(
        plaintext.len() <= MAX_HEAD_PLAINTEXT,
        "head record plaintext too large"
    );
    let mut nonce = [0u8; 24];
    fill_random(&mut nonce);
    let ciphertext = XChaCha20Poly1305::new(project_key.into())
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: &plaintext,
                aad: HEAD_MAILBOX_AAD,
            },
        )
        .map_err(|_| anyhow::anyhow!("head wrapper encryption failed"))?;
    let wrapper = HeadMailboxWrapper {
        version: WRAPPER_VERSION,
        project_route: project_route.to_string(),
        writer_device_id: identity.device_id.to_string(),
        seq: seq.to_string(),
        nonce: URL_SAFE_NO_PAD.encode(nonce),
        ciphertext: URL_SAFE_NO_PAD.encode(ciphertext),
    };
    let encoded = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&wrapper)?);
    ensure!(
        encoded.len() <= MAX_HEAD_CIPHERTEXT,
        "head ciphertext exceeds 4 KiB"
    );
    Ok(encoded)
}

pub fn unwrap_head_record(
    encoded_wrapper: &str,
    project_route: uuid::Uuid,
    writer_device_id: uuid::Uuid,
    seq: &str,
    project_key: &[u8; 32],
) -> Result<HeadRecord> {
    ensure!(
        !encoded_wrapper.is_empty() && encoded_wrapper.len() <= MAX_HEAD_CIPHERTEXT,
        "head ciphertext exceeds 4 KiB"
    );
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded_wrapper)
        .context("invalid head wrapper encoding")?;
    let wrapper: HeadMailboxWrapper =
        serde_json::from_slice(&bytes).context("invalid head wrapper")?;
    ensure!(
        wrapper.version == WRAPPER_VERSION
            && wrapper.project_route == project_route.to_string()
            && wrapper.writer_device_id == writer_device_id.to_string()
            && wrapper.seq == seq,
        "head wrapper metadata mismatch"
    );
    let nonce: [u8; 24] = URL_SAFE_NO_PAD
        .decode(wrapper.nonce)
        .context("invalid head nonce")?
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid head nonce length"))?;
    let ciphertext = URL_SAFE_NO_PAD
        .decode(wrapper.ciphertext)
        .context("invalid head body")?;
    let plaintext = XChaCha20Poly1305::new(project_key.into())
        .decrypt(
            &XNonce::from(nonce),
            Payload {
                msg: &ciphertext,
                aad: HEAD_MAILBOX_AAD,
            },
        )
        .map_err(|_| anyhow::anyhow!("head wrapper authentication failed"))?;
    ensure!(
        plaintext.len() <= MAX_HEAD_PLAINTEXT,
        "head record plaintext too large"
    );
    let record: HeadRecord = serde_json::from_slice(&plaintext).context("invalid head record")?;
    ensure!(
        !record.snapshot_id.is_nil()
            && record.parent_id != Some(record.snapshot_id)
            && protocol::valid_hash(&record.manifest_hash)
            && record.total_size <= protocol::MAX_PROJECT_BYTES
            && record.file_count as usize <= protocol::MAX_FILES,
        "invalid head record metadata"
    );
    Ok(record)
}

pub fn verify_head_record(record: &HeadRecord, public_key: &str) -> Result<()> {
    verify(public_key, &record.signing_bytes()?, &record.signature)
}

pub fn wrap_endpoint_record(
    project_route: uuid::Uuid,
    record: &EndpointRecord,
    project_key: &[u8; 32],
) -> Result<String> {
    record.validate(Utc::now())?;
    let mut nonce = [0u8; 24];
    fill_random(&mut nonce);
    let plaintext = serde_json::to_vec(record)?;
    let ciphertext = XChaCha20Poly1305::new(project_key.into())
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: &plaintext,
                aad: ENDPOINT_MAILBOX_AAD,
            },
        )
        .map_err(|_| anyhow::anyhow!("endpoint wrapper encryption failed"))?;
    let wrapper = EndpointMailboxWrapper {
        version: WRAPPER_VERSION,
        project_route: project_route.to_string(),
        writer_device_id: record.device_id.to_string(),
        seq: record.seq.to_string(),
        expires_at: endpoint_record_expires_at(record),
        nonce: URL_SAFE_NO_PAD.encode(nonce),
        ciphertext: URL_SAFE_NO_PAD.encode(ciphertext),
    };
    let serialized = serde_json::to_vec(&wrapper)?;
    ensure!(
        serialized.len() <= MAX_WRAPPER_BYTES,
        "endpoint mailbox wrapper too large"
    );
    Ok(URL_SAFE_NO_PAD.encode(serialized))
}

pub fn unwrap_endpoint_record(
    encoded_wrapper: &str,
    project_route: uuid::Uuid,
    writer_device_id: uuid::Uuid,
    seq: &str,
    expires_at: &str,
    project_key: &[u8; 32],
) -> Result<EndpointRecord> {
    ensure!(
        encoded_wrapper.len() <= MAX_WRAPPER_BYTES * 2,
        "endpoint mailbox wrapper too large"
    );
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded_wrapper)
        .context("invalid endpoint wrapper encoding")?;
    ensure!(
        bytes.len() <= MAX_WRAPPER_BYTES,
        "endpoint mailbox wrapper too large"
    );
    let wrapper: EndpointMailboxWrapper =
        serde_json::from_slice(&bytes).context("invalid endpoint wrapper")?;
    ensure!(
        wrapper.version == WRAPPER_VERSION
            && wrapper.project_route == project_route.to_string()
            && wrapper.writer_device_id == writer_device_id.to_string()
            && wrapper.seq == seq
            && wrapper.expires_at == expires_at,
        "endpoint wrapper metadata mismatch"
    );
    let nonce: [u8; 24] = URL_SAFE_NO_PAD
        .decode(wrapper.nonce)
        .context("invalid endpoint wrapper nonce")?
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid endpoint wrapper nonce length"))?;
    let ciphertext = URL_SAFE_NO_PAD
        .decode(wrapper.ciphertext)
        .context("invalid endpoint wrapper ciphertext")?;
    let plaintext = XChaCha20Poly1305::new(project_key.into())
        .decrypt(
            &XNonce::from(nonce),
            Payload {
                msg: &ciphertext,
                aad: ENDPOINT_MAILBOX_AAD,
            },
        )
        .map_err(|_| anyhow::anyhow!("endpoint wrapper authentication failed"))?;
    let record: EndpointRecord =
        serde_json::from_slice(&plaintext).context("invalid wrapped endpoint record")?;
    ensure!(
        record.device_id == writer_device_id
            && record.seq.to_string() == seq
            && record
                .expires_at
                .to_rfc3339_opts(SecondsFormat::Millis, true)
                == expires_at,
        "wrapped endpoint metadata mismatch"
    );
    Ok(record)
}
