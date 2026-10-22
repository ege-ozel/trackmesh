//! Windows cache lease and bounded deletion of recognized content-addressed files.
use crate::{is_reparse, ChunkStore};
use anyhow::{ensure, Result};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    sync::atomic::Ordering,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// A prune holds the exclusive lease briefly; shared users wait instead of failing a transfer.
const SHARED_LEASE_WAIT: Duration = Duration::from_secs(30);

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

pub struct CacheLease {
    _file: File,
}

impl ChunkStore {
    /// Shared across scans/receives, including cancelled blocking applies, and across processes.
    pub fn lease(&self, exclusive: bool) -> Result<CacheLease> {
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            let path = self.root.join("retention.lock");
            ensure!(
                !is_reparse(&fs::symlink_metadata(&self.root)?),
                "chunk store is a reparse point"
            );
            if path.exists() {
                ensure!(
                    !is_reparse(&fs::symlink_metadata(&path)?),
                    "cache lease is a reparse point"
                );
            }
            let open = || {
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .share_mode(if exclusive { 0 } else { 3 })
                    .open(&path)
            };
            let started = Instant::now();
            loop {
                match open() {
                    Ok(file) => return Ok(CacheLease { _file: file }),
                    Err(error) if exclusive || started.elapsed() >= SHARED_LEASE_WAIT => {
                        return Err(error.into())
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(100)),
                }
            }
        }
        #[cfg(not(windows))]
        anyhow::bail!("cache retention requires Windows file locking")
    }

    pub(crate) fn mark_read(&self) {
        self.last_read.store(unix_now(), Ordering::Relaxed);
    }

    /// True while a peer fetched chunks within `window`; retention then waits for a later pass.
    pub fn recently_read(&self, window: Duration) -> bool {
        unix_now().saturating_sub(self.last_read.load(Ordering::Relaxed)) < window.as_secs()
    }

    /// Caller holds the exclusive lease. Unknown files and reparse points fail closed.
    pub fn prune(&self, retained: &HashSet<String>) -> Result<u64> {
        let files = self.cache_files()?;
        let mut removed = 0;
        for (path, hash, size) in files {
            if !retained.contains(&hash) {
                let relative = path
                    .strip_prefix(&self.root)?
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("non-Unicode cache path"))?
                    .replace('\\', "/");
                fs::remove_file(crate::safe_path(&self.root, &relative)?)?;
                removed += size;
            }
        }
        Ok(removed)
    }

    pub fn stored_bytes(&self) -> Result<u64> {
        Ok(self.cache_files()?.iter().map(|(_, _, size)| size).sum())
    }

    fn cache_files(&self) -> Result<Vec<(std::path::PathBuf, String, u64)>> {
        let mut candidates = Vec::new();
        for prefix in fs::read_dir(&self.root)? {
            let prefix = prefix?;
            let name = prefix.file_name().to_string_lossy().into_owned();
            if name == "retention.lock" {
                continue;
            }
            let metadata = fs::symlink_metadata(prefix.path())?;
            ensure!(
                !is_reparse(&metadata)
                    && metadata.is_dir()
                    && name.len() == 2
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
                "unrecognized entry in chunk store; cleanup stopped"
            );
            for entry in fs::read_dir(prefix.path())? {
                let entry = entry?;
                let hash = entry.file_name().to_string_lossy().into_owned();
                let metadata = fs::symlink_metadata(entry.path())?;
                ensure!(
                    !is_reparse(&metadata)
                        && metadata.is_file()
                        && protocol::valid_hash(&hash)
                        && hash.starts_with(&name),
                    "unrecognized cache file; cleanup stopped"
                );
                candidates.push((entry.path(), hash, metadata.len()));
            }
        }
        Ok(candidates)
    }
}
