//! Explicit, ephemeral enrollment. No project-file permission until local approval.
use crate::Engine;
use anyhow::{ensure, Context, Result};
use network::Network;
use protocol::{DeviceCard, PairingRequest, PairingResponse};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};
use uuid::Uuid;

pub const PAIRING_WINDOW: Duration = Duration::from_secs(900);
const CHALLENGE_TTL: Duration = Duration::from_secs(60);
/// How long an offer to a known device stays open on both PCs; memory only, so a restart drops it.
const OFFER_TTL: Duration = Duration::from_secs(3600);
#[derive(Default)]
pub(crate) struct PairingState {
    window: Option<Window>,
    /// Offers this PC made: (project, device) until.
    offers_made: HashMap<(Uuid, Uuid), Instant>,
    /// Offers this PC received, by project, waiting for its user.
    offers_received: HashMap<Uuid, ReceivedOffer>,
}
struct ReceivedOffer {
    invite: protocol::ProjectInvite,
    until: Instant,
}
impl Drop for ReceivedOffer {
    fn drop(&mut self) {
        crypto::zeroize_project_invite(&mut self.invite);
    }
}
struct Window {
    project: Uuid,
    until: Instant,
    challenges: HashMap<String, Challenge>,
    pending: HashMap<Uuid, Pending>,
    rejected: HashSet<String>,
}
struct Challenge {
    card: DeviceCard,
    nonce: String,
    until: Instant,
}
struct Pending {
    card: DeviceCard,
}

#[derive(Default, Serialize)]
pub struct PairingStatus {
    pub project_id: Option<Uuid>,
    pub remaining_seconds: u64,
    pub requests: Vec<PairingCandidate>,
    /// Projects a known device offered to this PC.
    pub offers: Vec<ProjectOffer>,
}
#[derive(Serialize)]
pub struct ProjectOffer {
    pub project_id: Uuid,
    pub project_name: String,
    pub device_id: Uuid,
    pub name: String,
    /// The folder the project goes back to when this PC had it before.
    pub existing_root: Option<String>,
}
#[derive(Serialize)]
pub struct PairingCandidate {
    pub request_id: Uuid,
    pub device_id: Uuid,
    pub name: String,
    pub peer_id: String,
}
impl PairingState {
    fn prune_offers(&mut self) {
        let now = Instant::now();
        self.offers_made.retain(|_, until| *until > now);
        self.offers_received.retain(|_, offer| offer.until > now);
    }
    fn active(&mut self) -> Option<&mut Window> {
        if self
            .window
            .as_ref()
            .is_some_and(|w| w.until <= Instant::now())
        {
            self.window = None;
        }
        let window = self.window.as_mut()?;
        window.challenges.retain(|_, c| c.until > Instant::now());
        Some(window)
    }
}
impl Engine {
    pub async fn pairing_approval(
        &self,
        network: &Network,
        project: Uuid,
        inviter_device: Uuid,
    ) -> Result<PairingResponse> {
        let inviter = self.paired_peer(project, inviter_device)?;
        network
            .pairing_request(
                &inviter.peer_id,
                PairingRequest::Status {
                    project_id: project,
                },
            )
            .await
    }

    pub fn open_pairing_window(&self, project: Uuid) -> Result<Option<Uuid>> {
        self.store.project(project)?;
        self.ensure_project_creator(project)?;
        let mut state = self
            .pairing
            .lock()
            .map_err(|_| anyhow::anyhow!("pairing state unavailable"))?;
        if let Some(window) = state.active() {
            if window.project == project {
                window.until = Instant::now() + PAIRING_WINDOW;
                return Ok(None);
            }
        }
        let lost_project = state.window.take().map(|window| window.project);
        state.window = Some(Window {
            project,
            until: Instant::now() + PAIRING_WINDOW,
            challenges: HashMap::new(),
            pending: HashMap::new(),
            rejected: HashSet::new(),
        });
        Ok(lost_project)
    }
    pub fn close_pairing_window(&self) -> Result<()> {
        self.pairing
            .lock()
            .map_err(|_| anyhow::anyhow!("pairing state unavailable"))?
            .window = None;
        Ok(())
    }
    pub fn pairing_status(&self) -> Result<PairingStatus> {
        let mut state = self
            .pairing
            .lock()
            .map_err(|_| anyhow::anyhow!("pairing state unavailable"))?;
        state.prune_offers();
        let offers = state
            .offers_received
            .iter()
            .map(|(project, offer)| ProjectOffer {
                project_id: *project,
                project_name: offer.invite.project_name.clone(),
                device_id: offer.invite.device.device_id,
                name: offer.invite.device.name.clone(),
                existing_root: self.store.known_root(*project).ok().flatten(),
            })
            .collect();
        let Some(window) = state.active() else {
            return Ok(PairingStatus {
                offers,
                ..PairingStatus::default()
            });
        };
        Ok(PairingStatus {
            offers,
            project_id: Some(window.project),
            remaining_seconds: window
                .until
                .saturating_duration_since(Instant::now())
                .as_secs(),
            requests: window
                .pending
                .iter()
                .map(|(id, pending)| PairingCandidate {
                    request_id: *id,
                    device_id: pending.card.device_id,
                    name: pending.card.name.clone(),
                    peer_id: pending.card.peer_id.clone(),
                })
                .collect(),
        })
    }
    pub fn approve_pairing(&self, project: Uuid, request: Uuid) -> Result<()> {
        self.ensure_project_creator(project)?;
        let mut state = self
            .pairing
            .lock()
            .map_err(|_| anyhow::anyhow!("pairing state unavailable"))?;
        let window = state
            .active()
            .context("pairing window expired; reopen it and ask the peer to retry")?;
        ensure!(
            window.project == project,
            "pairing request belongs to another project"
        );
        let pending = window
            .pending
            .get(&request)
            .context("pairing request expired or removed")?;
        let card = pending.card.clone();
        self.pair(project, card.clone())?;
        if self.project_backend_config(project)?.backend == crate::ProjectBackend::Supabase {
            // Persist the grant before removing the approval request so an I/O
            // failure can be retried by approving again without losing the card.
            self.queue_supabase_grant(project, &card)?;
        }
        window.pending.remove(&request);
        if window.pending.is_empty() {
            // The invite link opens once: with its device approved, nobody else is coming.
            // An approved device still gets `Approved` from `Status` without a window.
            state.window = None;
        }
        Ok(())
    }
    pub fn reject_pairing(&self, project: Uuid, request: Uuid) -> Result<()> {
        let mut state = self
            .pairing
            .lock()
            .map_err(|_| anyhow::anyhow!("pairing state unavailable"))?;
        let window = state.active().context("pairing window expired")?;
        ensure!(
            window.project == project,
            "pairing request belongs to another project"
        );
        let pending = window
            .pending
            .remove(&request)
            .context("pairing request expired or removed")?;
        if window.rejected.len() < 8 {
            window.rejected.insert(pending.card.peer_id);
        } else {
            state.window = None;
        }
        Ok(())
    }
    /// Called only in a blocking responder worker; malformed requests reveal no private details.
    pub fn handle_pairing(&self, peer: &str, request: PairingRequest) -> PairingResponse {
        self.handle_pairing_inner(peer, request)
            .unwrap_or(PairingResponse::Unavailable)
    }
    fn handle_pairing_inner(&self, peer: &str, request: PairingRequest) -> Result<PairingResponse> {
        let mut state = self
            .pairing
            .lock()
            .map_err(|_| anyhow::anyhow!("pairing state unavailable"))?;
        match request {
            PairingRequest::Status { project_id } => {
                if self.store.authorized(project_id, peer)?.is_some() {
                    return Ok(PairingResponse::Approved);
                }
                let window = state.active().context("pairing closed")?;
                ensure!(
                    window.project == project_id
                        && window.pending.values().any(|p| p.card.peer_id == peer),
                    "pairing unavailable"
                );
                Ok(PairingResponse::Pending)
            }
            PairingRequest::Begin { project_id, card } => {
                network::validate_card(&card)?;
                ensure!(
                    card.peer_id == peer && card.device_id != self.identity.device_id,
                    "pairing identity mismatch"
                );
                if let Some(known) = self.store.authorized(project_id, peer)? {
                    ensure!(
                        known.device_id == card.device_id && known.public_key == card.public_key,
                        "pairing identity mismatch"
                    );
                    return Ok(PairingResponse::Approved);
                }
                // A device this PC offered the project to was chosen here already: no window,
                // no second approval. It must be the same device, key and transport identity.
                state.prune_offers();
                if state
                    .offers_made
                    .contains_key(&(project_id, card.device_id))
                {
                    let known = self.known_device(card.device_id)?;
                    ensure!(
                        known.peer_id == peer && known.public_key == card.public_key,
                        "pairing identity mismatch"
                    );
                    self.ensure_project_creator(project_id)?;
                    self.pair(project_id, card.clone())?;
                    if self.project_backend_config(project_id)?.backend
                        == crate::ProjectBackend::Supabase
                    {
                        self.queue_supabase_grant(project_id, &card)?;
                    }
                    state.offers_made.remove(&(project_id, card.device_id));
                    return Ok(PairingResponse::Approved);
                }
                let window = state.active().context("pairing closed")?;
                ensure!(window.project == project_id, "pairing closed");
                ensure!(!window.rejected.contains(peer), "pairing rejected");
                if window.pending.values().any(|p| p.card.peer_id == peer) {
                    return Ok(PairingResponse::Pending);
                }
                ensure!(
                    window.challenges.len() < 8 && window.pending.len() < 8,
                    "pairing busy"
                );
                let nonce = crypto::pairing_challenge();
                window.challenges.insert(
                    peer.to_owned(),
                    Challenge {
                        card,
                        nonce: nonce.clone(),
                        until: Instant::now() + CHALLENGE_TTL,
                    },
                );
                Ok(PairingResponse::Challenge { nonce })
            }
            PairingRequest::Prove {
                project_id,
                challenge,
                proof,
            } => {
                let window = state.active().context("pairing closed")?;
                ensure!(window.project == project_id, "pairing closed");
                ensure!(!window.rejected.contains(peer), "pairing rejected");
                // Consume before validation: even invalid proofs cannot reuse this nonce.
                let issued = window
                    .challenges
                    .remove(peer)
                    .context("pairing challenge expired")?;
                ensure!(
                    issued.until > Instant::now() && issued.nonce == challenge,
                    "pairing challenge expired"
                );
                let key = crypto::project_key(
                    &self.directory.join("secrets/projects"),
                    project_id,
                    None,
                )?;
                let inviter = network::peer_id(&self.identity)?;
                let scope = protocol::pairing_scope(project_id, &inviter, &issued.card)?;
                crypto::verify_pairing_proof(&key, &challenge, &scope, &proof)?;
                ensure!(window.pending.len() < 8, "pairing busy");
                window
                    .pending
                    .insert(Uuid::new_v4(), Pending { card: issued.card });
                Ok(PairingResponse::Pending)
            }
            PairingRequest::Offer { mut invite } => {
                let checked = (|| -> Result<()> {
                    protocol::require_protocol(invite.version)?;
                    network::validate_card(&invite.device)?;
                    crypto::verify_project_invite(&invite)?;
                    // Only from a device this PC already shares a project with, over that
                    // device's own connection.
                    let known = self.known_device(invite.device.device_id)?;
                    ensure!(
                        invite.device.peer_id == peer
                            && known.peer_id == peer
                            && known.public_key == invite.device.public_key,
                        "offer identity mismatch"
                    );
                    ensure!(!invite.project_id.is_nil(), "invalid offer project");
                    // A project this PC left, or whose creator removed it, can be offered again.
                    ensure!(
                        !self.store.listed(invite.project_id)?
                            || self.membership_removed(invite.project_id)?,
                        "project already on this PC"
                    );
                    Ok(())
                })();
                if let Err(error) = checked {
                    crypto::zeroize_project_invite(&mut invite);
                    return Err(error);
                }
                state.prune_offers();
                if !state.offers_received.contains_key(&invite.project_id)
                    && state.offers_received.len() >= 8
                {
                    crypto::zeroize_project_invite(&mut invite);
                    anyhow::bail!("too many offers");
                }
                state.offers_received.insert(
                    invite.project_id,
                    ReceivedOffer {
                        invite,
                        until: Instant::now() + OFFER_TTL,
                    },
                );
                Ok(PairingResponse::Pending)
            }
        }
    }

    /// A device this PC shares at least one project with.
    fn known_device(&self, device: Uuid) -> Result<DeviceCard> {
        self.store
            .peers(None)?
            .into_iter()
            .find(|card| card.device_id == device)
            .context("device is not known on this PC")
    }

    /// Devices this PC shares some project with but not `project`.
    pub fn offerable_devices(&self, project: Uuid) -> Result<Vec<DeviceCard>> {
        let members = self.store.peers(Some(project))?;
        let mut devices = self.store.peers(None)?;
        devices.retain(|card| {
            !members
                .iter()
                .any(|member| member.device_id == card.device_id)
        });
        devices.sort_by_key(|card| card.device_id);
        devices.dedup_by(|a, b| a.device_id == b.device_id);
        Ok(devices)
    }

    /// Offers `project` to a device this PC already shares another project with. When that
    /// device accepts, its pairing request is approved without a window.
    pub async fn offer_project(
        self: &Arc<Self>,
        network: &Network,
        project: Uuid,
        device: Uuid,
        addresses: Vec<String>,
    ) -> Result<PairingResponse> {
        let disk = self.clone();
        let (peer, mut invite) = tokio::task::spawn_blocking(move || -> Result<_> {
            disk.ensure_project_creator(project)?;
            let card = disk
                .offerable_devices(project)?
                .into_iter()
                .find(|card| card.device_id == device)
                .context("this device is already in the project or not known on this PC")?;
            let invite = disk.export_invite(project, addresses)?;
            disk.pairing
                .lock()
                .map_err(|_| anyhow::anyhow!("pairing state unavailable"))?
                .offers_made
                .insert((project, device), Instant::now() + OFFER_TTL);
            Ok((card.peer_id, invite))
        })
        .await??;
        let request = PairingRequest::Offer {
            invite: invite.clone(),
        };
        crypto::zeroize_project_invite(&mut invite);
        network.pairing_request(&peer, request).await
    }

    /// Takes a received offer to accept it; the caller imports the invitation it carries.
    pub fn take_offer(&self, project: Uuid) -> Result<protocol::ProjectInvite> {
        let mut state = self
            .pairing
            .lock()
            .map_err(|_| anyhow::anyhow!("pairing state unavailable"))?;
        state.prune_offers();
        let offer = state
            .offers_received
            .remove(&project)
            .context("the offer expired; ask for it again")?;
        // The copy goes to import, which zeroizes it; dropping `offer` zeroizes the original.
        Ok(offer.invite.clone())
    }

    pub fn decline_offer(&self, project: Uuid) -> Result<()> {
        self.pairing
            .lock()
            .map_err(|_| anyhow::anyhow!("pairing state unavailable"))?
            .offers_received
            .remove(&project);
        Ok(())
    }
    pub async fn request_pairing(
        self: &Arc<Self>,
        network: &Network,
        project: Uuid,
        inviter_device: Uuid,
        addresses: Vec<String>,
    ) -> Result<PairingResponse> {
        let disk = self.clone();
        let (inviter, card) = tokio::task::spawn_blocking(move || -> Result<_> {
            let inviter = disk.paired_peer(project, inviter_device)?;
            Ok((inviter.peer_id, disk.device_card(addresses)?))
        })
        .await??;
        let response = network
            .pairing_request(
                &inviter,
                PairingRequest::Begin {
                    project_id: project,
                    card: card.clone(),
                },
            )
            .await?;
        let nonce = match response {
            PairingResponse::Challenge { nonce } => nonce,
            PairingResponse::Pending => {
                return self.wait_pairing_approval(network, project, &inviter).await
            }
            other => return Ok(other),
        };
        let disk = self.clone();
        let peer = inviter.clone();
        let challenge = nonce.clone();
        let proof = tokio::task::spawn_blocking(move || -> Result<_> {
            let key = crypto::project_key(&disk.directory.join("secrets/projects"), project, None)?;
            let scope = protocol::pairing_scope(project, &peer, &card)?;
            crypto::pairing_proof(&key, &challenge, &scope)
        })
        .await??;
        let response = network
            .pairing_request(
                &inviter,
                PairingRequest::Prove {
                    project_id: project,
                    challenge: nonce,
                    proof,
                },
            )
            .await?;
        if matches!(response, PairingResponse::Pending) {
            self.wait_pairing_approval(network, project, &inviter).await
        } else {
            Ok(response)
        }
    }

    async fn wait_pairing_approval(
        &self,
        network: &Network,
        project: Uuid,
        inviter: &str,
    ) -> Result<PairingResponse> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        loop {
            tokio::time::sleep(Duration::from_secs(6)).await;
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Ok(PairingResponse::Pending);
            }
            match tokio::time::timeout(
                remaining,
                network.pairing_request(
                    inviter,
                    PairingRequest::Status {
                        project_id: project,
                    },
                ),
            )
            .await
            {
                Ok(Ok(PairingResponse::Pending)) => {}
                Ok(Ok(response)) => return Ok(response),
                _ => return Ok(PairingResponse::Unavailable),
            }
        }
    }
}
