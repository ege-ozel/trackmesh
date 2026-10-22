//! Event-triggered save settlement; never runs as a filesystem watcher.
use super::{inventory, safe_path, scan, ChunkStore, FileStamp, ScanOptions};
use anyhow::{ensure, Result};
use protocol::{Manifest, MAX_PROJECT_BYTES};
use std::{
    fs::OpenOptions,
    path::Path,
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

const QUIET: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(500);
const WAIT: Duration = Duration::from_secs(60);

fn readable(root: &Path, files: &[FileStamp]) -> Result<bool> {
    for file in files {
        let path = safe_path(root, &file.path)?;
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1);
        }
        match options.open(path) {
            Ok(handle) => {
                let metadata = handle.metadata()?;
                if metadata.len() != file.size || metadata.modified()? != file.modified {
                    return Ok(false);
                }
            }
            Err(error)
                if matches!(error.raw_os_error(), Some(32 | 33))
                    || error.kind() == std::io::ErrorKind::NotFound =>
            {
                return Ok(false)
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(true)
}

/// Wait for a quiet inventory, then use the existing guarded scanner. A failed
/// scan may retry three times before any manifest reaches the commit boundary.
/// The wait is bounded; hashing a large, stable project can take longer.
pub fn scan_after_save(
    root: &Path,
    chunks: &ChunkStore,
    project_id: Uuid,
    author_device_id: Uuid,
    parent: Option<&Manifest>,
    options: &ScanOptions,
) -> Result<Manifest> {
    let deadline = Instant::now() + WAIT;
    let mut previous = inventory(root)?;
    let mut quiet_since = Instant::now();
    let mut attempts = 0;
    loop {
        ensure!(
            Instant::now() < deadline,
            "project did not settle within 60 seconds; retry Send Snapshot"
        );
        thread::sleep(POLL);
        let current = inventory(root)?;
        let total = current
            .iter()
            .try_fold(0u64, |total, file| total.checked_add(file.size));
        ensure!(
            total.is_some_and(|size| size <= MAX_PROJECT_BYTES),
            "project exceeds MVP size limit"
        );
        if current != previous {
            previous = current;
            quiet_since = Instant::now();
            continue;
        }
        if quiet_since.elapsed() < QUIET {
            continue;
        }
        // Refuse active writers before spending time hashing the whole folder.
        // The scanner still opens each file with the same restrictive sharing.
        if !readable(root, &previous)? {
            continue;
        }
        attempts += 1;
        match scan(root, chunks, project_id, author_device_id, parent, options) {
            Ok(manifest) if inventory(root)? == previous => return Ok(manifest),
            Ok(_) => ensure!(
                attempts < 3,
                "project changed while saving; retry Send Snapshot"
            ),
            Err(error) => {
                if attempts >= 3 {
                    return Err(error);
                }
            }
        }
        previous = inventory(root)?;
        quiet_since = Instant::now();
    }
}
