// Removed from crates/crypto/src/lib.rs in d447de1. Identity::seal was an impl method;
// verify_envelope and open were free functions. Imports they used:
// use anyhow::{ensure, Context, Result};
// use base64::{engine::general_purpose::STANDARD, Engine};
// use chacha20poly1305::{
//     aead::{Aead, Payload},
//     KeyInit, XChaCha20Poly1305, XNonce,
// };
// use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
// use protocol::{
//     Announcement, DeviceCard, EndpointRecord, Envelope, Manifest, SignedSnapshot, PROTOCOL_VERSION,
// };
// use rand::{rngs::OsRng, RngCore};
// use serde::{Deserialize, Serialize};

impl Identity {
    pub fn seal(&self, snapshot: &SignedSnapshot, key: &[u8; 32]) -> Result<Envelope> {
        let m = &snapshot.manifest;
        let announcement = Announcement {
            parent_snapshot_id: m.parent_snapshot_id,
            manifest_hash: snapshot.manifest_hash.clone(),
            total_size: m.total_size,
            changed_bytes: m.changed_bytes,
            file_count: m.files.len() as u32,
            author_name: self.name(),
            description: "Saved project snapshot".into(),
        };
        let mut nonce = [0u8; 24];
        OsRng.fill_bytes(&mut nonce);
        let mut envelope = Envelope {
            version: PROTOCOL_VERSION,
            project_id: m.project_id,
            snapshot_id: m.snapshot_id,
            author_device_id: self.device_id,
            created_at: m.created_at,
            nonce: STANDARD.encode(nonce),
            encrypted_payload: String::new(),
            public_key: self.public_key(),
            signature: String::new(),
        };
        let plaintext = Zeroizing::new(serde_json::to_vec(&announcement)?);
        let ciphertext = XChaCha20Poly1305::new(key.into())
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &plaintext,
                    aad: &envelope.aad()?,
                },
            )
            .map_err(|_| anyhow::anyhow!("metadata encryption failed"))?;
        envelope.encrypted_payload = STANDARD.encode(ciphertext);
        envelope.signature = self.sign(&envelope.signing_bytes()?);
        Ok(envelope)
    }
}

pub fn verify_envelope(envelope: &Envelope) -> Result<()> {
    ensure!(
        envelope.version == PROTOCOL_VERSION,
        "unsupported metadata version"
    );
    ensure!(
        envelope.encrypted_payload.len() <= protocol::MAX_MAILBOX_PAYLOAD,
        "metadata too large"
    );
    ensure!(
        STANDARD.decode(&envelope.nonce)?.len() == 24,
        "invalid nonce"
    );
    verify(
        &envelope.public_key,
        &envelope.signing_bytes()?,
        &envelope.signature,
    )
}
pub fn open(envelope: &Envelope, key: &[u8; 32]) -> Result<Announcement> {
    verify_envelope(envelope)?;
    let nonce = STANDARD.decode(&envelope.nonce)?;
    let ciphertext = STANDARD.decode(&envelope.encrypted_payload)?;
    let plaintext = Zeroizing::new(
        XChaCha20Poly1305::new(key.into())
            .decrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad: &envelope.aad()?,
                },
            )
            .map_err(|_| anyhow::anyhow!("metadata authentication failed"))?,
    );
    let announcement: Announcement = serde_json::from_slice(&plaintext)?;
    ensure!(
        protocol::valid_hash(&announcement.manifest_hash),
        "invalid announced manifest hash"
    );
    ensure!(
        announcement.parent_snapshot_id != Some(envelope.snapshot_id),
        "invalid announced parent"
    );
    ensure!(
        announcement.changed_bytes <= announcement.total_size
            && announcement.total_size <= protocol::MAX_PROJECT_BYTES
            && announcement.file_count as usize <= protocol::MAX_FILES,
        "invalid announced size"
    );
    ensure!(
        announcement.author_name.len() <= 320 && announcement.description.len() <= 4096,
        "metadata text too long"
    );
    Ok(announcement)
}
