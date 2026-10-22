//! Empty-message AEAD proof: no project key is transmitted, and verification is constant-time.
use crate::fill_random;
use anyhow::{ensure, Result};
use chacha20poly1305::{
    aead::{Aead, Payload},
    KeyInit, XChaCha20Poly1305, XNonce,
};

pub fn pairing_challenge() -> String {
    let mut nonce = [0; 24];
    fill_random(&mut nonce);
    hex::encode(nonce)
}

fn nonce(value: &str) -> Result<[u8; 24]> {
    ensure!(value.len() == 48, "invalid pairing challenge");
    hex::decode(value)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid pairing challenge"))
}

pub fn pairing_proof(key: &[u8; 32], challenge: &str, scope: &[u8]) -> Result<String> {
    let nonce = nonce(challenge)?;
    let tag = XChaCha20Poly1305::new(key.into())
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: &[],
                aad: scope,
            },
        )
        .map_err(|_| anyhow::anyhow!("pairing proof failed"))?;
    Ok(hex::encode(tag))
}

pub fn verify_pairing_proof(
    key: &[u8; 32],
    challenge: &str,
    scope: &[u8],
    proof: &str,
) -> Result<()> {
    ensure!(proof.len() == 32, "invalid pairing proof");
    let nonce = nonce(challenge)?;
    let tag = hex::decode(proof)?;
    XChaCha20Poly1305::new(key.into())
        .decrypt(
            &XNonce::from(nonce),
            Payload {
                msg: &tag,
                aad: scope,
            },
        )
        .map_err(|_| anyhow::anyhow!("invalid pairing proof"))?;
    Ok(())
}
