//! Native confirmation based on verified missing chunks, pinned to the requested snapshot.
use crate::runtime::{is_turkish, tr, Desktop};
use anyhow::{ensure, Result};
use network::Network;
use protocol::DeviceCard;
use std::sync::Arc;
use tauri::AppHandle;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use trackmesh_core::Engine;
use uuid::Uuid;

/// How many file names the dialog lists before "…and N more".
const LISTED: usize = 12;

fn gb(bytes: u64) -> f64 {
    bytes as f64 / f64::from(1u32 << 30)
}

/// "12 KB", "3.4 MB" or "1.25 GB": small changes don't read as "0.00 GB".
fn size(bytes: u64) -> String {
    const MB: u64 = 1 << 20;
    if bytes < MB {
        format!("{} KB", bytes.div_ceil(1024).max(1))
    } else if bytes < 1 << 30 {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else {
        format!("{:.2} GB", gb(bytes))
    }
}

fn message(
    approval: &transfer::DownloadApproval,
    peer: &str,
    review: bool,
    replace: bool,
) -> String {
    let turkish = is_turkish();
    let mut text = String::new();
    let (added, changed, removed) = (
        approval.added.len(),
        approval.changed.len(),
        approval.removed.len(),
    );
    if added + changed + removed == 0 {
        text.push_str(tr("No files change.", "Hiçbir dosya değişmiyor."));
    } else {
        text.push_str(&if turkish {
            format!("Eklenen: {added} · Değişen: {changed} · Silinen: {removed}")
        } else {
            format!("Added: {added} · Changed: {changed} · Removed: {removed}")
        });
        let lines: Vec<String> = approval
            .added
            .iter()
            .map(|path| format!("+ {path}"))
            .chain(approval.changed.iter().map(|path| format!("~ {path}")))
            .chain(approval.removed.iter().map(|path| format!("− {path}")))
            .collect();
        for line in lines.iter().take(LISTED) {
            text.push('\n');
            text.push_str(line);
        }
        if lines.len() > LISTED {
            let more = lines.len() - LISTED;
            text.push_str(&if turkish {
                format!("\n…ve {more} dosya daha")
            } else {
                format!("\n…and {more} more")
            });
        }
    }
    if !approval.executables.is_empty() {
        text.push_str("\n\n");
        text.push_str(&if turkish {
            format!("⚠ Program veya betik dosyaları geliyor. Açılırsa ya da bir program onları yüklerse kod çalıştırabilirler. Yalnızca {peer} adlı iş arkadaşına güveniyorsan aç; bir DAW projesinde normalde bulunmazlar:")
        } else {
            format!("⚠ Program or script files are coming. They can run code if opened or loaded by a program. Open them only if you trust {peer}; a DAW project doesn't normally contain them:")
        });
        for path in approval.executables.iter().take(LISTED) {
            text.push_str("\n• ");
            text.push_str(path);
        }
        if approval.executables.len() > LISTED {
            let more = approval.executables.len() - LISTED;
            text.push_str(&if turkish {
                format!("\n…ve {more} dosya daha")
            } else {
                format!("\n…and {more} more")
            });
        }
    }
    text.push_str("\n\n");
    text.push_str(&match (approval.free_bytes, turkish) {
        (Some(free), true) => format!(
            "Yeni veri: yaklaşık {} · bu sürücüde boş: {:.1} GB",
            size(approval.new_bytes),
            gb(free)
        ),
        (Some(free), false) => format!(
            "New data: about {} · free on this drive: {:.1} GB",
            size(approval.new_bytes),
            gb(free)
        ),
        (None, true) => format!("Yeni veri: yaklaşık {}", size(approval.new_bytes)),
        (None, false) => format!("New data: about {}", size(approval.new_bytes)),
    });
    text.push('\n');
    // A person pressed Get or Use version: this one window also asks that the DAW project is
    // closed, so there is no separate question before it.
    text.push_str(if review && replace {
        tr(
            "Close this project in your DAW first. Your current files were saved as your latest version, and TrackMesh keeps a backup of the files it replaces.\n\nUse this version?",
            "Önce projeyi DAW'da kapat. Şu anki dosyaların son sürümün olarak kaydedildi; TrackMesh değiştirdiği dosyaların yedeğini de tutar.\n\nBu sürüm kullanılsın mı?",
        )
    } else if review {
        tr(
            "Close this project in your DAW first. TrackMesh keeps a backup of the files it replaces; Open my previous files opens them.\n\nGet this version?",
            "Önce projeyi DAW'da kapat. TrackMesh değiştirdiği dosyaların yedeğini tutar; Önceki dosyalarımı aç ile açabilirsin.\n\nBu sürüm alınsın mı?",
        )
    } else {
        tr(
            "TrackMesh also keeps a backup of the files it replaces.\n\nDownload it?",
            "TrackMesh değiştirdiği dosyaların yedeğini de tutar.\n\nİndirilsin mi?",
        )
    });
    text
}

impl Desktop {
    /// `review` asks before every download with the changed files (a person pressed Get or Use
    /// version). Without it only large downloads or program/script files ask (automatic first
    /// version). Either way a drive without room refuses before anything is fetched.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn receive_with_confirmation(
        &self,
        app: &AppHandle,
        engine: &Arc<Engine>,
        network: &Network,
        project: Uuid,
        peer: DeviceCard,
        target: Option<Uuid>,
        replace: bool,
        review: bool,
        stop: &transfer::Stop,
    ) -> Result<Uuid> {
        let mut target = target;
        let mut gate = transfer::DownloadGate {
            limit: 1 << 30,
            review,
            allow_executables: false,
        };
        loop {
            let result = if replace {
                engine
                    .receive_replacing_local_with_gate(
                        network,
                        project,
                        peer.clone(),
                        target,
                        gate,
                        stop.clone(),
                    )
                    .await
            } else {
                engine
                    .receive_from_peer_with_gate(
                        network,
                        project,
                        peer.clone(),
                        target,
                        gate,
                        stop.clone(),
                    )
                    .await
            };
            match result {
                Err(error) if error.downcast_ref::<transfer::DownloadApproval>().is_some() => {
                    let approval = error.downcast_ref::<transfer::DownloadApproval>().unwrap();
                    target = Some(approval.snapshot_id);
                    // The same pinned version comes back next time, so these answers cover it.
                    gate = transfer::DownloadGate {
                        limit: approval.new_bytes,
                        review: false,
                        allow_executables: true,
                    };
                    let text = message(approval, &peer.name, review, replace);
                    let warn = !approval.executables.is_empty();
                    let title = if review && replace {
                        if is_turkish() {
                            format!("{} adlı iş arkadaşının sürümü kullanılsın mı?", peer.name)
                        } else {
                            format!("Use {}'s version?", peer.name)
                        }
                    } else if review {
                        if is_turkish() {
                            format!("{} adlı iş arkadaşının sürümü alınsın mı?", peer.name)
                        } else {
                            format!("Get {}'s version?", peer.name)
                        }
                    } else {
                        tr("Confirm download", "İndirmeyi onayla").to_owned()
                    };
                    let accept = if review && replace {
                        tr("Use", "Kullan")
                    } else {
                        tr("Get", "Al")
                    };
                    let app = app.clone();
                    let accepted = tauri::async_runtime::spawn_blocking(move || {
                        app.dialog()
                            .message(text)
                            .title(title)
                            .kind(if warn {
                                MessageDialogKind::Warning
                            } else {
                                MessageDialogKind::Info
                            })
                            .buttons(MessageDialogButtons::OkCancelCustom(
                                accept.to_owned(),
                                tr("Cancel", "Vazgeç").to_owned(),
                            ))
                            .blocking_show()
                    })
                    .await?;
                    ensure!(
                        accepted,
                        "Download cancelled; verified progress is kept for Resume"
                    );
                }
                result => return result,
            }
        }
    }
}
