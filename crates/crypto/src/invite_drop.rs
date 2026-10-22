//! Client-side encryption for short-lived Supabase invitation drops.
use crate::fill_random;
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chacha20poly1305::{
    aead::{Aead, Payload},
    KeyInit, XChaCha20Poly1305, XNonce,
};
use chrono::{SecondsFormat, Utc};
use protocol::ProjectInvite;
use zeroize::Zeroizing;

const DOMAIN: &[u8] = b"trackmesh-invite-drop-v1";
pub const INVITE_DROP_TTL_SECONDS: i64 = 15 * 60;
pub const MAX_INVITE_DROP_BYTES: usize = 64 * 1024;

pub struct EncryptedInviteDrop {
    pub id: String,
    pub expires_at: String,
    pub ciphertext: String,
    pub key: Zeroizing<[u8; 32]>,
}

pub fn encrypt_invite_drop(invite: &ProjectInvite) -> Result<EncryptedInviteDrop> {
    super::verify_project_invite(invite)?;
    let mut id_bytes = [0u8; 32];
    let mut key = Zeroizing::new([0u8; 32]);
    let mut nonce = [0u8; 24];
    fill_random(&mut id_bytes);
    fill_random(key.as_mut());
    fill_random(&mut nonce);
    let id = URL_SAFE_NO_PAD.encode(id_bytes);
    let expires_at = (Utc::now() + chrono::Duration::seconds(INVITE_DROP_TTL_SECONDS))
        .to_rfc3339_opts(SecondsFormat::Millis, true);
    let plaintext = Zeroizing::new(serde_json::to_vec(invite)?);
    ensure!(
        plaintext.len() < MAX_INVITE_DROP_BYTES,
        "invitation is too large"
    );
    let aad = aad(&id, &expires_at);
    let ciphertext = XChaCha20Poly1305::new((&*key).into())
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: &plaintext,
                aad: &aad,
            },
        )
        .map_err(|_| anyhow::anyhow!("invitation encryption failed"))?;
    ensure!(
        ciphertext.len() < MAX_INVITE_DROP_BYTES,
        "invitation is too large"
    );
    let mut sealed = Vec::with_capacity(nonce.len() + ciphertext.len());
    sealed.extend_from_slice(&nonce);
    sealed.extend_from_slice(&ciphertext);
    let ciphertext = URL_SAFE_NO_PAD.encode(sealed);
    ensure!(
        ciphertext.len() < MAX_INVITE_DROP_BYTES,
        "invitation is too large"
    );
    Ok(EncryptedInviteDrop {
        id,
        expires_at,
        ciphertext,
        key,
    })
}

pub fn decrypt_invite_drop(
    id: &str,
    expires_at: &str,
    ciphertext: &str,
    key: &[u8; 32],
) -> Result<ProjectInvite> {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    ensure!(
        ciphertext.len() <= MAX_INVITE_DROP_BYTES * 2,
        "invalid invitation"
    );
    ensure!(
        chrono::DateTime::parse_from_rfc3339(expires_at)
            .context("invalid invitation")?
            .with_timezone(&Utc)
            > Utc::now(),
        "invalid invitation"
    );
    let sealed = URL_SAFE_NO_PAD
        .decode(ciphertext)
        .context("invalid invitation")?;
    ensure!(
        sealed.len() >= 24 + 16 && sealed.len() < MAX_INVITE_DROP_BYTES,
        "invalid invitation"
    );
    let (nonce, ciphertext) = sealed.split_at(24);
    let plaintext = Zeroizing::new(
        XChaCha20Poly1305::new(key.into())
            .decrypt(
                &XNonce::try_from(nonce).map_err(|_| anyhow::anyhow!("invalid invitation"))?,
                Payload {
                    msg: ciphertext,
                    aad: &aad(id, expires_at),
                },
            )
            .map_err(|_| anyhow::anyhow!("invalid invitation"))?,
    );
    serde_json::from_slice(&plaintext).map_err(|_| anyhow::anyhow!("invalid invitation"))
}

fn aad(id: &str, expires_at: &str) -> Vec<u8> {
    let mut aad = Vec::with_capacity(DOMAIN.len() + id.len() + expires_at.len() + 2);
    aad.extend_from_slice(DOMAIN);
    aad.push(0);
    aad.extend_from_slice(id.as_bytes());
    aad.push(0);
    aad.extend_from_slice(expires_at.as_bytes());
    aad
}
