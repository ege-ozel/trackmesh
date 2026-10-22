use crate::{
    lang::{genitive, is_turkish, tr},
    Engine, ProjectSummary,
};
use anyhow::Result;
use network::Network;
use protocol::{PluginProject, PluginStatus};
use storage::Lineage;
use uuid::Uuid;

/// What Get in the plugin asks for; TrackMesh opens on the project with the Get button.
pub(super) fn get_guidance() -> String {
    tr(
        "TrackMesh is open on this project. Save and close the project in your DAW, then press Get there.",
        "TrackMesh bu projede açıldı. Projeyi DAW'da kaydedip kapat, sonra orada Al'a bas.",
    )
    .into()
}

/// The plugin's Get button names the collaborator, as the Desktop card does.
fn get_label(name: &str) -> String {
    match (name.is_empty(), is_turkish()) {
        (true, false) => "Get collaborator's version".into(),
        (true, true) => "İş arkadaşının sürümünü al".into(),
        (false, false) => format!("Get {name}'s version"),
        (false, true) => format!("{} sürümünü al", genitive(name)),
    }
}

#[derive(Clone)]
pub(super) enum Notice {
    Queued,
    Saving,
    Committed,
    Failed,
    Receive,
    Saved,
}
impl Notice {
    fn message(&self) -> (bool, String) {
        match self {
            Self::Queued => (true, queued()),
            Self::Saving => (true, saving()),
            Self::Committed => (true, tr("Shared. Your collaborator can get this version while TrackMesh is open.", "Paylaşıldı. TrackMesh açıkken iş arkadaşın bu sürümü alabilir.").into()),
            Self::Failed => (false, tr("Sharing didn't finish. Open TrackMesh to see why, then press Share current version again.", "Paylaşım bitmedi. Nedenini görmek için TrackMesh'i aç, sonra Güncel sürümü paylaş'a yeniden bas.").into()),
            Self::Receive => (true, get_guidance()),
            Self::Saved => (true, saved()),
        }
    }
}
pub(super) fn queued() -> String {
    tr(
        "Sharing… waiting for the DAW to finish saving.",
        "Paylaşılıyor… DAW'ın kaydetmeyi bitirmesi bekleniyor.",
    )
    .into()
}
pub(super) fn saving() -> String {
    tr(
        "Saved. Sharing once the DAW finishes writing files…",
        "Kaydedildi. DAW dosyaları yazmayı bitirince paylaşılacak…",
    )
    .into()
}
pub(super) fn saved() -> String {
    tr(
        "Saved. Press Share current version to send it.",
        "Kaydedildi. Göndermek için Güncel sürümü paylaş'a bas.",
    )
    .into()
}
// Only local names and last folder components leave Rust; never full paths.
pub(super) fn projects(engine: &Engine) -> Result<(Vec<PluginProject>, bool)> {
    let mut records = engine.store.projects_page(0, 33)?;
    records.extend(engine.store.projects_page(33, 32)?);
    let more = records.len() > 64;
    records.truncate(64);
    Ok((
        records
            .into_iter()
            .map(|project| PluginProject {
                project_id: project.project_id,
                name: project
                    .name
                    .chars()
                    .take(96)
                    .map(|ch| if ch.is_control() { ' ' } else { ch })
                    .collect(),
                folder_name: std::path::Path::new(&project.root)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("")
                    .chars()
                    .take(48)
                    .map(|ch| if ch.is_control() { ' ' } else { ch })
                    .collect(),
            })
            .collect(),
        more,
    ))
}
pub fn unavailable(project_id: Uuid, message: &str) -> PluginStatus {
    PluginStatus {
        ok: false,
        project_id,
        project_name: String::new(),
        local_snapshot: None,
        remote_snapshot: None,
        author: None,
        created_at: None,
        peer_online: false,
        connection: "OFFLINE".into(),
        state: "UNLINKED".into(),
        message: bounded(message),
        projects: Vec::new(),
        projects_more: false,
        picker_supported: true,
        desktop_available: false,
        status_line: String::new(),
        share_enabled: false,
        get_enabled: false,
        pending_publications: 0,
        auto_share_supported: false,
        auto_share: false,
        presence_warning: String::new(),
        share_confirmation: String::new(),
        presence_danger: false,
        presence_since: 0,
        presence_get_line: String::new(),
        language: tr("en", "tr").into(),
        get_label: get_label(""),
        collaborators: String::new(),
    }
}
pub fn summarize(
    summary: ProjectSummary,
    network: &Network,
    notice: Option<Notice>,
) -> PluginStatus {
    let connection = network.status();
    let online = |peer: &crate::status::PeerSummary| {
        connection.error.is_none()
            && connection
                .peers
                .get(&peer.device.peer_id)
                .is_some_and(|p| p.online)
    };
    // As on the Desktop card: whoever is online, with a newer version first, then the newest.
    let selected = summary.peers.iter().max_by_key(|p| {
        (
            online(p),
            p.known_lineage == Some(Lineage::RemoteAhead),
            p.offline_head
                .as_ref()
                .map(|s| s.created_at.as_str())
                .or_else(|| p.announcement.as_ref().map(|s| s.created_at.as_str())),
        )
    });
    let remote = selected.and_then(|p| p.announcement.as_ref());
    let hint = selected.and_then(|p| p.offline_head.as_ref());
    let route = selected
        .and_then(|p| connection.peers.get(&p.device.peer_id))
        .filter(|p| p.online && connection.error.is_none());
    let any_online = summary.peers.iter().any(&online);
    let collaborators = bounded(
        &summary
            .peers
            .iter()
            .filter(|p| !any_online || online(p))
            .map(|p| p.device.name.as_str())
            .collect::<Vec<_>>()
            .join(", "),
    );
    let lineage = selected.and_then(|p| p.known_lineage.as_ref());
    let name = selected
        .map(|p| bounded(&p.device.name))
        .unwrap_or_default();
    let transfer = summary
        .transfer
        .as_ref()
        .filter(|t| t.state != storage::TransferState::Complete);
    let turkish = is_turkish();
    let delivered = lineage == Some(&Lineage::LocalAhead)
        && selected
            .and_then(|p| p.delivered_snapshot)
            .is_some_and(|delivered| {
                summary.local_snapshot.as_ref().map(|s| s.snapshot_id) == Some(delivered)
            });
    let (state, line) = if summary.project.recovery_required {
        (
            storage::TransferState::RecoveryRequired.as_str(),
            tr(
                "The last Get did not finish. Close the project in your DAW and press Recover in TrackMesh.",
                "Son alma işlemi bitmedi. Projeyi DAW'da kapat ve TrackMesh'te Kurtar'a bas.",
            )
            .into(),
        )
    } else if let Some(transfer) = transfer {
        let message = match transfer.state {
            storage::TransferState::Receiving | storage::TransferState::Applying => {
                let percent = transfer
                    .completed_bytes
                    .saturating_mul(100)
                    .checked_div(transfer.total_bytes)
                    .unwrap_or(0)
                    .min(100);
                if turkish {
                    format!("{} sürümü alınıyor… %{percent}", genitive(&name))
                } else {
                    format!("Getting {name}'s version… {percent} %")
                }
            }
            storage::TransferState::Paused => tr(
                "Get paused. Press Resume getting in TrackMesh.",
                "Alma duraklatıldı. TrackMesh'te Almaya devam et'e bas.",
            )
            .into(),
            _ => tr("Getting a version…", "Sürüm alınıyor…").into(),
        };
        (transfer.state.as_str(), message)
    } else if lineage == Some(&Lineage::Diverged) {
        (
            "DIVERGED",
            if turkish {
                format!("Senin sürümün de {} sürümü de değişti. Hangisinin kalacağını TrackMesh'te seç.", genitive(&name))
            } else {
                format!("You and {name} both changed the project. Choose which version to keep in TrackMesh.")
            },
        )
    } else if selected.is_none() {
        (
            "LAST_KNOWN",
            tr(
                "Invite a collaborator in TrackMesh.",
                "TrackMesh'te bir iş arkadaşını davet et.",
            )
            .into(),
        )
    } else if route.is_none() {
        (
            "OFFLINE",
            if turkish {
                format!("{name} bağlı değil. Bağlanınca sürümü alınabilir.")
            } else {
                format!("{name} is not connected. You can get their version when they are.")
            },
        )
    } else if hint.is_some() || lineage == Some(&Lineage::RemoteAhead) {
        (
            "REMOTE_AHEAD",
            if turkish {
                format!("{name} yeni bir sürüm paylaştı")
            } else {
                format!("{name} shared a new version")
            },
        )
    } else if delivered {
        // The collaborator confirmed getting this PC's version, as the Desktop card's "Received by".
        // Still LOCAL_AHEAD: the plugin then shows the collaborator's own version time on its own line.
        (
            "LOCAL_AHEAD",
            if turkish {
                format!("{name} son sürümünü aldı ✓")
            } else {
                format!("{name} has your latest version ✓")
            },
        )
    } else if lineage == Some(&Lineage::LocalAhead) {
        (
            "LOCAL_AHEAD",
            if turkish {
                format!("Senin sürümün {} sürümünden daha yeni", genitive(&name))
            } else {
                format!("Your version is newer than {name}'s")
            },
        )
    } else if lineage == Some(&Lineage::Equal) {
        (
            "UP_TO_DATE",
            if turkish {
                format!("{} son sürümü sende ✓", genitive(&name))
            } else {
                format!("You have {name}'s latest version ✓")
            },
        )
    } else {
        (
            "LAST_KNOWN",
            if turkish {
                format!("{} bir sürüm paylaşması bekleniyor", genitive(&name))
            } else {
                format!("Waiting for {name} to share a version")
            },
        )
    };
    // "Shared, your collaborator can get it" is stale once they have it or have a newer or split version.
    let notice = notice.filter(|notice| {
        !matches!(notice, Notice::Committed)
            || !(delivered || matches!(state, "REMOTE_AHEAD" | "DIVERGED"))
    });
    let first_share_blocked = summary.local_snapshot.is_none()
        && (summary.root_empty
            || summary.peers.iter().any(|peer| {
                peer.announcement.is_some()
                    || peer.offline_head.is_some()
                    || peer.known_lineage.is_some()
            }));
    let share_enabled = !first_share_blocked
        && summary.root_available
        && !summary.root_empty
        && !summary.project.recovery_required
        && !summary.active_transfer
        // A share already waiting for the DAW's save would refuse a second press.
        && !matches!(notice, Some(Notice::Queued | Notice::Saving));
    let get_enabled = route.is_some()
        && (remote.is_some() || hint.is_some())
        // Nothing to get; the plugin says so instead.
        && !matches!(state, "UP_TO_DATE" | "LOCAL_AHEAD")
        && !summary.project.recovery_required
        && !transfer.is_some_and(|item| {
            matches!(
                item.state,
                storage::TransferState::Receiving | storage::TransferState::Applying
            )
        })
        && lineage != Some(&Lineage::Diverged);
    let (ok, message) = notice
        .map(|notice| notice.message())
        .unwrap_or((true, String::new()));
    PluginStatus {
        ok,
        project_id: summary.project.project_id,
        project_name: bounded(&summary.project.name),
        local_snapshot: summary
            .local_snapshot
            .as_ref()
            .map(|s| s.snapshot_id.to_string()),
        remote_snapshot: hint
            .map(|s| s.snapshot_id.to_string())
            .or_else(|| remote.map(|s| s.snapshot_id.to_string())),
        get_label: get_label(&name),
        collaborators,
        language: tr("en", "tr").into(),
        author: selected.map(|_| name),
        created_at: hint
            .map(|s| s.created_at.clone())
            .or_else(|| remote.map(|s| s.created_at.clone())),
        peer_online: route.is_some(),
        connection: route.map_or("OFFLINE", |p| p.route.as_str()).into(),
        state: state.into(),
        message: bounded(&message),
        projects: Vec::new(),
        projects_more: false,
        picker_supported: true,
        desktop_available: false,
        status_line: bounded(&line),
        share_enabled,
        get_enabled,
        // Kept in the plugin protocol; nothing is queued since the mailbox was removed.
        pending_publications: 0,
        auto_share_supported: true,
        auto_share: false,
        presence_warning: String::new(),
        share_confirmation: String::new(),
        presence_danger: false,
        presence_since: 0,
        presence_get_line: String::new(),
    }
}
pub fn bounded(text: &str) -> String {
    text.chars()
        .take(512)
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect()
}
