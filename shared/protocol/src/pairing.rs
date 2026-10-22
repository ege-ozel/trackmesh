//! Additive enrollment channel; existing invitation/card/transfer serialization is unchanged.
use crate::DeviceCard;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PAIRING_PROTOCOL: &str = "/trackmesh/pairing/1";
pub const MAX_PAIRING_BYTES: usize = 32 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PairingRequest {
    Status {
        project_id: Uuid,
    },
    Begin {
        project_id: Uuid,
        card: DeviceCard,
    },
    Prove {
        project_id: Uuid,
        challenge: String,
        proof: String,
    },
    /// The creator of a project offers it to a device it already shares another project with.
    /// The receiver keeps it until its user accepts or declines; Accept then sends `Begin`.
    Offer {
        invite: crate::ProjectInvite,
    },
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PairingResponse {
    Challenge { nonce: String },
    Pending,
    Approved,
    Unavailable,
}

// Behaviour events require Debug; never print cards, challenges or capability proofs.
impl std::fmt::Debug for PairingRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Begin { .. } => "PairingRequest::Begin",
            Self::Status { .. } => "PairingRequest::Status",
            Self::Prove { .. } => "PairingRequest::Prove",
            Self::Offer { .. } => "PairingRequest::Offer",
        })
    }
}
impl std::fmt::Debug for PairingResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Challenge { .. } => "PairingResponse::Challenge",
            Self::Pending => "PairingResponse::Pending",
            Self::Approved => "PairingResponse::Approved",
            Self::Unavailable => "PairingResponse::Unavailable",
        })
    }
}

/// Bind capability possession to this project, transport identities and exact signed card.
pub fn pairing_scope(project: Uuid, inviter: &str, card: &DeviceCard) -> anyhow::Result<Vec<u8>> {
    Ok(serde_json::to_vec(&(
        "trackmesh-pairing-proof-v1",
        project,
        inviter,
        &card.peer_id,
        card.signing_bytes()?,
        &card.signature,
    ))?)
}
