use anyhow::{ensure, Context, Result};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use protocol::{DeviceCard, EndpointRecord, Manifest, SignedSnapshot, PROTOCOL_VERSION};
use rand::TryRng;
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};
mod invite_drop;
mod pairing;
mod supabase;
pub use invite_drop::{decrypt_invite_drop, encrypt_invite_drop, EncryptedInviteDrop};

pub fn zeroize_project_invite(invite: &mut protocol::ProjectInvite) {
    invite.project_key.zeroize();
    if let Some(backend) = &mut invite.backend {
        backend.supabase_anon_key.zeroize();
    }
}
pub use pairing::{pairing_challenge, pairing_proof, verify_pairing_proof};
pub use supabase::{
    endpoint_record_expires_at, unwrap_endpoint_record, unwrap_head_record, verify_head_record,
    wrap_endpoint_record, wrap_head_record,
};

pub struct Identity {
    pub device_id: Uuid,
    name: std::sync::RwLock<String>,
    signing_key: SigningKey,
}
#[derive(Serialize, Deserialize)]
struct IdentityData {
    device_id: Uuid,
    name: String,
    secret: [u8; 32],
}

impl Identity {
    pub fn sign_project_invite(&self, invite: &mut protocol::ProjectInvite) -> Result<()> {
        ensure!(
            invite.device.device_id == self.device_id,
            "invitation signer mismatch"
        );
        invite.signature = None;
        invite.signature = Some(self.sign(&invite.signing_bytes()?));
        Ok(())
    }

    pub fn load_or_create(directory: &Path, name: &str) -> Result<Self> {
        fs::create_dir_all(directory)?;
        let path = directory.join("identity.protected");
        if !path.exists() {
            // Same as `SigningKey::generate`: 32 random bytes are the secret key.
            let mut secret = Zeroizing::new([0u8; 32]);
            fill_random(&mut *secret);
            let key = SigningKey::from_bytes(&secret);
            let mut data = IdentityData {
                device_id: Uuid::new_v4(),
                name: name.chars().take(80).collect(),
                secret: key.to_bytes(),
            };
            let serialized = Zeroizing::new(serde_json::to_vec(&data)?);
            data.secret.zeroize();
            write_secret(&path, &serialized)?;
        }
        let serialized = read_secret(&path)?;
        let mut data: IdentityData = serde_json::from_slice(&serialized)?;
        let signing_key = SigningKey::from_bytes(&data.secret);
        data.secret.zeroize();
        Ok(Self {
            device_id: data.device_id,
            name: std::sync::RwLock::new(data.name),
            signing_key,
        })
    }
    pub fn name(&self) -> String {
        self.name
            .read()
            .map(|name| name.clone())
            .unwrap_or_default()
    }
    pub fn set_name(&self, name: &str) -> Result<()> {
        let name = name.trim().chars().take(80).collect::<String>();
        ensure!(!name.is_empty(), "device name cannot be empty");
        *self
            .name
            .write()
            .map_err(|_| anyhow::anyhow!("identity name lock poisoned"))? = name;
        Ok(())
    }
    pub fn secret_bytes(&self) -> Zeroizing<[u8; 32]> {
        Zeroizing::new(self.signing_key.to_bytes())
    }
    pub fn public_key(&self) -> String {
        hex::encode(self.signing_key.verifying_key().as_bytes())
    }
    pub fn sign(&self, message: &[u8]) -> String {
        hex::encode(self.signing_key.sign(message).to_bytes())
    }
    pub fn sign_card(&self, peer_id: String, addresses: Vec<String>) -> Result<DeviceCard> {
        let mut card = DeviceCard {
            version: PROTOCOL_VERSION,
            device_id: self.device_id,
            name: self.name(),
            peer_id,
            public_key: self.public_key(),
            addresses,
            signature: String::new(),
        };
        card.signature = self.sign(&card.signing_bytes()?);
        Ok(card)
    }
    pub fn sign_endpoint(&self, mut record: EndpointRecord) -> Result<EndpointRecord> {
        ensure!(
            record.device_id == self.device_id,
            "endpoint device id does not match identity"
        );
        record.signature = self.sign(&record.signing_bytes()?);
        Ok(record)
    }
    pub fn sign_current_endpoint(
        &self,
        peer_id: String,
        addresses: Vec<String>,
        seq: u64,
    ) -> Result<EndpointRecord> {
        let issued_at = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(
            chrono::Utc::now().timestamp_millis(),
        )
        .context("could not format endpoint issue time to milliseconds")?;
        self.sign_endpoint(EndpointRecord {
            device_id: self.device_id,
            peer_id,
            addresses,
            issued_at,
            expires_at: issued_at + protocol::PREFERRED_ENDPOINT_TTL,
            seq,
            signature: String::new(),
        })
    }
    pub fn sign_snapshot(&self, manifest: Manifest) -> Result<SignedSnapshot> {
        manifest.validate()?;
        let mut snapshot = SignedSnapshot {
            manifest_hash: manifest.digest()?,
            manifest,
            author_public_key: self.public_key(),
            signature: String::new(),
        };
        snapshot.signature = self.sign(&snapshot.signing_bytes()?);
        snapshot.validate_structure()?;
        Ok(snapshot)
    }
}

/// Fills `buf` from the operating system's random source. Panics if the OS can't provide
/// randomness, as `OsRng::fill_bytes` did before rand 0.10.
pub(crate) fn fill_random(buf: &mut [u8]) {
    rand::rngs::SysRng
        .try_fill_bytes(buf)
        .expect("operating system random source failed");
}

pub fn verify_project_invite(invite: &protocol::ProjectInvite) -> Result<()> {
    let signature = invite
        .signature
        .as_deref()
        .context("invitation signature missing")?;
    let key_bytes: [u8; 32] = hex::decode(&invite.device.public_key)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid invitation signer key"))?;
    let key = VerifyingKey::from_bytes(&key_bytes)?;
    let signature_bytes: [u8; 64] = hex::decode(signature)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid invitation signature"))?;
    key.verify_strict(
        &invite.signing_bytes()?,
        &Signature::from_bytes(&signature_bytes),
    )?;
    Ok(())
}

pub fn verify(public_key: &str, message: &[u8], signature: &str) -> Result<()> {
    let key: [u8; 32] = hex::decode(public_key)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid public key"))?;
    let sig: [u8; 64] = hex::decode(signature)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid signature"))?;
    VerifyingKey::from_bytes(&key)?
        .verify_strict(message, &Signature::from_bytes(&sig))
        .context("signature verification failed")
}

pub fn verify_endpoint(
    record: &EndpointRecord,
    public_key: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<()> {
    record.validate(now)?;
    verify(public_key, &record.signing_bytes()?, &record.signature)
}
pub fn verify_endpoint_current(record: &EndpointRecord, public_key: &str) -> Result<()> {
    verify_endpoint(record, public_key, chrono::Utc::now())
}
pub fn verify_snapshot(snapshot: &SignedSnapshot) -> Result<()> {
    snapshot.validate_structure()?;
    verify(
        &snapshot.author_public_key,
        &snapshot.signing_bytes()?,
        &snapshot.signature,
    )
}
pub fn project_key(
    directory: &Path,
    project: Uuid,
    supplied: Option<&[u8; 32]>,
) -> Result<Zeroizing<[u8; 32]>> {
    fs::create_dir_all(directory)?;
    let path = directory.join(format!("{project}.protected"));
    if path.exists() {
        let bytes = read_secret(&path)?;
        let key: [u8; 32] = bytes
            .as_slice()
            .try_into()
            .context("invalid protected project key")?;
        if let Some(supplied) = supplied {
            ensure!(
                &key == supplied,
                "project key differs from the existing collaboration key"
            );
        }
        return Ok(Zeroizing::new(key));
    }
    let mut key = Zeroizing::new([0u8; 32]);
    if let Some(supplied) = supplied {
        *key = *supplied;
    } else {
        fill_random(&mut *key);
    }
    write_secret(&path, &*key)?;
    Ok(key)
}

pub fn write_secret(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let protected = protect(bytes)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(&protected)?;
    file.sync_all()?;
    Ok(())
}
pub fn read_secret(path: &Path) -> Result<Zeroizing<Vec<u8>>> {
    unprotect(&fs::read(path)?)
}

#[cfg(windows)]
fn protect(bytes: &[u8]) -> Result<Vec<u8>> {
    use windows_sys::Win32::{Foundation::LocalFree, Security::Cryptography::*};
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len().try_into()?,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // Current-user DPAPI, UI forbidden; never use CRYPTPROTECT_LOCAL_MACHINE.
    unsafe {
        ensure!(
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output
            ) != 0,
            "DPAPI protect failed: {}",
            std::io::Error::last_os_error()
        );
        let result = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData as _);
        Ok(result)
    }
}
#[cfg(windows)]
fn unprotect(bytes: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    use windows_sys::Win32::{Foundation::LocalFree, Security::Cryptography::*};
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len().try_into()?,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    unsafe {
        ensure!(
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output
            ) != 0,
            "DPAPI unlock failed: {}",
            std::io::Error::last_os_error()
        );
        let result = Zeroizing::new(
            std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec(),
        );
        std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize).zeroize();
        LocalFree(output.pbData as _);
        Ok(result)
    }
}
#[cfg(not(windows))]
fn protect(_: &[u8]) -> Result<Vec<u8>> {
    anyhow::bail!("TrackMesh device secrets require Windows DPAPI")
}
#[cfg(not(windows))]
fn unprotect(_: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    anyhow::bail!("TrackMesh device secrets require Windows DPAPI")
}
