//! Rehash cached chunks, fetch only missing chunks, then persist verified progress.
use anyhow::{ensure, Result};
use futures::{stream, StreamExt, TryStreamExt};
use network::Network;
use protocol::{PeerRequest, PeerResponse, SignedSnapshot};
use snapshot::ChunkStore;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use storage::Store;
use uuid::Uuid;

/// What a receive may do before it asks the person.
#[derive(Clone, Copy, Debug)]
pub struct DownloadGate {
    /// New data above this many bytes needs confirmation.
    pub limit: u64,
    /// Ask before every download and show which files change.
    pub review: bool,
    /// Program, library, plugin or script files may arrive without asking.
    pub allow_executables: bool,
}
impl DownloadGate {
    /// No question asked; the free-space check still applies.
    pub const OPEN: Self = Self {
        limit: u64::MAX,
        review: false,
        allow_executables: true,
    };
}

/// The receive stopped before downloading so the person can decide. Paths are project-relative
/// file names for the person's own dialog; never log them.
pub struct DownloadApproval {
    pub snapshot_id: Uuid,
    pub new_bytes: u64,
    pub added: Vec<String>,
    pub changed: Vec<String>,
    pub removed: Vec<String>,
    /// Added or changed files that can run code (see `snapshot::is_executable_path`).
    pub executables: Vec<String>,
    /// Free bytes on the project's drive, when Windows reports it.
    pub free_bytes: Option<u64>,
}
impl std::fmt::Debug for DownloadApproval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DownloadApproval")
            .field("snapshot_id", &self.snapshot_id)
            .field("new_bytes", &self.new_bytes)
            .field(
                "files",
                &(self.added.len() + self.changed.len() + self.removed.len()),
            )
            .field("executables", &self.executables.len())
            .finish()
    }
}
impl std::fmt::Display for DownloadApproval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Download of {} new bytes requires confirmation",
            self.new_bytes
        )
    }
}
impl std::error::Error for DownloadApproval {}

/// The drive does not have room for this version; nothing was downloaded.
#[derive(Debug)]
pub struct InsufficientSpace {
    pub needed: u64,
    pub free: u64,
}
impl std::fmt::Display for InsufficientSpace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let gb = |bytes: u64| bytes as f64 / f64::from(1u32 << 30);
        write!(
            f,
            "Not enough free disk space: this version needs about {:.2} GB, {:.2} GB is free",
            gb(self.needed),
            gb(self.free)
        )
    }
}
impl std::error::Error for InsufficientSpace {}

/// Room kept free beyond the estimate, for the database, journal and the DAW.
const SPACE_MARGIN: u64 = 256 << 20;

struct Changes {
    added: Vec<String>,
    changed: Vec<String>,
    removed: Vec<String>,
    staged_bytes: u64,
}

fn changes(base: Option<&SignedSnapshot>, target: &SignedSnapshot) -> Changes {
    let old: std::collections::HashMap<&str, &str> = base
        .map(|base| {
            base.manifest
                .files
                .iter()
                .filter(|file| !snapshot::is_ignored_path(&file.relative_path))
                .map(|file| (file.relative_path.as_str(), file.blake3.as_str()))
                .collect()
        })
        .unwrap_or_default();
    // assemble() writes every file of the target manifest into staging, unchanged ones
    // included and without the ignored-path filter, so the space estimate counts that same set.
    let staged_bytes = target
        .manifest
        .files
        .iter()
        .fold(0u64, |sum, file| sum.saturating_add(file.size));
    let mut out = Changes {
        added: Vec::new(),
        changed: Vec::new(),
        removed: Vec::new(),
        staged_bytes,
    };
    let mut seen = std::collections::HashSet::new();
    for file in &target.manifest.files {
        if snapshot::is_ignored_path(&file.relative_path) {
            continue;
        }
        seen.insert(file.relative_path.as_str());
        match old.get(file.relative_path.as_str()) {
            None => out.added.push(file.relative_path.clone()),
            Some(hash) if *hash != file.blake3 => out.changed.push(file.relative_path.clone()),
            Some(_) => {}
        }
    }
    out.removed = old
        .keys()
        .filter(|path| !seen.contains(*path))
        .map(|path| (*path).to_owned())
        .collect();
    out.removed.sort();
    out
}

/// Refuse before any chunk is fetched when the drives can't hold the new chunks plus the
/// staged copy of every target file (staging sits next to the project).
fn check_space(
    project_root: &Path,
    chunk_root: &Path,
    new_bytes: u64,
    staged: u64,
) -> Result<Option<u64>> {
    let project_free = snapshot::free_bytes(project_root);
    let chunk_free = snapshot::free_bytes(chunk_root);
    let same = match (
        snapshot::volume_root(project_root),
        snapshot::volume_root(chunk_root),
    ) {
        (Some(a), Some(b)) => a.as_os_str().eq_ignore_ascii_case(b.as_os_str()),
        _ => false,
    };
    let mut demands = Vec::new();
    if same {
        demands.push((project_free, new_bytes.saturating_add(staged)));
    } else {
        demands.push((project_free, staged));
        demands.push((chunk_free, new_bytes));
    }
    for (free, needed) in demands {
        let Some(free) = free else { continue };
        let needed = needed.saturating_add(SPACE_MARGIN);
        if needed > free {
            return Err(InsufficientSpace { needed, free }.into());
        }
    }
    Ok(project_free)
}

pub(crate) async fn download(
    store: &Arc<Store>,
    chunks: &ChunkStore,
    network: &Network,
    s: &SignedSnapshot,
    id: Uuid,
    peer: &str,
    gate: DownloadGate,
) -> Result<()> {
    let mut unique = BTreeMap::new();
    for file in &s.manifest.files {
        for chunk in &file.chunks {
            unique.insert(chunk.blake3.clone(), chunk.size);
        }
    }
    let total: u64 = unique.values().map(|s| *s as u64).sum();
    store.reset_transfer_progress(id, total)?;
    let mut completed = 0u64;
    let mut missing = Vec::new();
    for (hash, size) in unique {
        let cache = chunks.clone();
        let h = hash.clone();
        if tokio::task::spawn_blocking(move || cache.has_verified(&h, size)).await? {
            completed += size as u64;
            store.transfer_chunk(id, &hash, size, completed)?;
        } else {
            missing.push((hash, size));
        }
    }
    tracing::info!(transfer_id=%id, reused_bytes=completed, missing_bytes=total-completed, missing_chunks=missing.len(), "verified resume cache; requesting only missing chunks");
    let new_bytes = total - completed;
    let base = store.head(s.manifest.project_id)?;
    let changes = changes(base.as_ref(), s);
    let root = PathBuf::from(store.project(s.manifest.project_id)?.root);
    let free_bytes = check_space(&root, chunks.root(), new_bytes, changes.staged_bytes)?;
    let executables: Vec<String> = changes
        .added
        .iter()
        .chain(&changes.changed)
        .filter(|path| snapshot::is_executable_path(path))
        .cloned()
        .collect();
    if gate.review || new_bytes > gate.limit || (!gate.allow_executables && !executables.is_empty())
    {
        return Err(DownloadApproval {
            snapshot_id: s.manifest.snapshot_id,
            new_bytes,
            added: changes.added,
            changed: changes.changed,
            removed: changes.removed,
            executables,
            free_bytes,
        }
        .into());
    }
    let project = s.manifest.project_id;
    let snapshot = s.manifest.snapshot_id;
    let mut requests = stream::iter(missing.into_iter().map(|(hash, size)| {
        let network = network.clone();
        let chunks = chunks.clone();
        let peer = peer.to_owned();
        async move {
            let mut last_error = None;
            for attempt in 0..3 {
                match network
                    .request(
                        &peer,
                        PeerRequest::Chunk {
                            project_id: project,
                            snapshot_id: snapshot,
                            hash: hash.clone(),
                        },
                    )
                    .await
                {
                    Ok(PeerResponse::Chunk {
                        hash: returned,
                        data,
                    }) => {
                        ensure!(
                            returned == hash
                                && data.len() == size as usize
                                && protocol::hash(&data) == hash,
                            "received chunk failed BLAKE3 verification"
                        );
                        let h = hash.clone();
                        let cache = chunks.clone();
                        tokio::task::spawn_blocking(move || cache.put(&h, &data)).await??;
                        return Ok((hash, size));
                    }
                    Ok(PeerResponse::Error { message }) => {
                        last_error = Some(anyhow::anyhow!(message))
                    }
                    Ok(_) => anyhow::bail!("unexpected chunk response"),
                    Err(error) => last_error = Some(error),
                }
                if !network.online(&peer) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(250 * (attempt + 1))).await;
            }
            Err(last_error.unwrap_or_else(|| anyhow::anyhow!("chunk transfer failed")))
        }
    }))
    .buffer_unordered(8);
    while let Some((hash, size)) = requests.try_next().await? {
        completed += size as u64;
        store.record_chunk(&hash, size)?;
        store.transfer_chunk(id, &hash, size, completed)?;
        tracing::debug!(transfer_id=%id, completed_bytes=completed, total_bytes=total, "chunk committed");
    }
    Ok(())
}
