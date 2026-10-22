use anyhow::{ensure, Context, Result};
use chrono::Utc;
use protocol::{
    Chunk, Manifest, SnapshotFile, DEFAULT_CHUNK_SIZE, MAX_FILES, MAX_PROJECT_BYTES,
    PROTOCOL_VERSION,
};
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::SystemTime,
};
use uuid::Uuid;
mod disk;
mod retention;
mod settlement;
pub use disk::{free_bytes, is_executable_path, volume_root};
pub use retention::CacheLease;
pub use settlement::scan_after_save;

/// A person's request to stop long work. Holders check it only at points where
/// stopping leaves no partly written project state behind.
#[derive(Clone, Default)]
pub struct Stop(Arc<AtomicBool>);
impl Stop {
    pub fn request(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn requested(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Work ended at a safe point because a `Stop` was requested.
#[derive(Debug, Clone, Copy)]
pub struct Stopped;
impl std::fmt::Display for Stopped {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Stopped")
    }
}
impl std::error::Error for Stopped {}

/// Whether a project-relative file is generated metadata that should not be
/// included in TrackMesh snapshots. Paths use `/` separators, as manifests do.
pub fn is_ignored_path(relative: &str) -> bool {
    // Ableton's automatic set backup tree is local generated state.
    if is_backup_file_path(relative) {
        return true;
    }

    let name = relative.rsplit('/').next().unwrap_or(relative);
    // Ableton regenerates analysis cache files when loading samples.
    if name.to_ascii_lowercase().ends_with(".asd") {
        return true;
    }
    match name.to_ascii_lowercase().as_str() {
        "desktop.ini" => true, // Windows folder-view settings.
        "thumbs.db" => true,   // Windows thumbnail cache.
        "ehthumbs.db" => true, // Windows legacy thumbnail cache.
        ".ds_store" => true,   // macOS Finder metadata.
        _ => false,
    }
}

fn is_backup_file_path(relative: &str) -> bool {
    relative
        .split_once('/')
        .is_some_and(|(first, _)| first.eq_ignore_ascii_case("Backup"))
}

fn is_backup_tree_path(relative: &str) -> bool {
    relative.eq_ignore_ascii_case("Backup") || is_backup_file_path(relative)
}

#[derive(Clone)]
pub struct ChunkStore {
    root: PathBuf,
    /// Unix seconds of the last chunk read; retention skips pruning while peers are fetching.
    last_read: Arc<AtomicU64>,
}
impl ChunkStore {
    pub fn new(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root)?;
        Ok(Self {
            root,
            last_read: Arc::default(),
        })
    }
    /// The folder holding the chunk files, for free-space checks.
    pub fn root(&self) -> &Path {
        &self.root
    }
    fn path(&self, hash: &str) -> Result<PathBuf> {
        ensure!(protocol::valid_hash(hash), "invalid chunk hash");
        Ok(self.root.join(&hash[..2]).join(hash))
    }
    pub fn read(&self, hash: &str, size: u32) -> Result<Vec<u8>> {
        let _lease = self.lease(false)?;
        self.mark_read();
        let path = self.path(hash)?;
        let file = File::open(&path)?;
        ensure!(
            file.metadata()?.len() == size as u64,
            "chunk store length mismatch"
        );
        let mut bytes = Vec::with_capacity(size as usize);
        file.take(size as u64 + 1).read_to_end(&mut bytes)?;
        ensure!(
            protocol::hash(&bytes) == hash,
            "chunk store digest mismatch"
        );
        Ok(bytes)
    }
    pub fn has_verified(&self, hash: &str, size: u32) -> bool {
        self.read(hash, size).is_ok()
    }
    pub fn put(&self, hash: &str, bytes: &[u8]) -> Result<()> {
        let _lease = self.lease(false)?;
        ensure!(
            bytes.len() <= protocol::MAX_CHUNK_SIZE as usize && protocol::hash(bytes) == hash,
            "chunk integrity failure"
        );
        let path = self.path(hash)?;
        fs::create_dir_all(path.parent().context("chunk parent missing")?)?;
        if self.has_verified(hash, bytes.len() as u32) {
            return Ok(());
        }
        let temp = path.with_extension(format!("{}.partial", Uuid::new_v4()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        if let Err(error) = atomic_replace(&temp, &path) {
            let _ = fs::remove_file(&temp);
            return Err(error);
        }
        Ok(())
    }
}

pub fn is_reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}
pub fn canonical_root(path: &Path) -> Result<PathBuf> {
    let meta = fs::symlink_metadata(path).context("project folder does not exist")?;
    ensure!(
        meta.is_dir() && !is_reparse(&meta),
        "project root must be a real directory"
    );
    let root = fs::canonicalize(path)?;
    // Also disallow a junction/symlink in a parent supplied by the user.
    for ancestor in path.ancestors().filter(|p| !p.as_os_str().is_empty()) {
        ensure!(
            !is_reparse(&fs::symlink_metadata(ancestor)?),
            "project path contains a reparse point"
        );
    }
    Ok(root)
}

#[derive(Clone, PartialEq, Eq)]
struct FileStamp {
    path: String,
    size: u64,
    modified: SystemTime,
}
fn inventory(root: &Path) -> Result<Vec<FileStamp>> {
    let mut files = Vec::new();
    let mut directories = vec![root.to_owned()];
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)?;
            ensure!(
                !is_reparse(&metadata),
                "symlinks and Windows reparse points are not supported: {}",
                path.display()
            );
            let relative = path
                .strip_prefix(root)?
                .to_str()
                .context("non-Unicode project filename")?
                .replace('\\', "/");
            protocol::validate_relative_path(&relative)?;
            // Reserved exclusively for local recovery/staging, never part of a DAW project.
            ensure!(
                relative
                    .split('/')
                    .next()
                    .is_none_or(|s| !s.eq_ignore_ascii_case(".trackmesh")),
                "project folder contains reserved .trackmesh directory"
            );
            if metadata.is_dir() {
                if is_backup_tree_path(&relative) {
                    continue;
                }
                directories.push(path);
            } else {
                ensure!(metadata.is_file(), "unsupported filesystem entry");
                if is_ignored_path(&relative) {
                    continue;
                }
                files.push(FileStamp {
                    path: relative,
                    size: metadata.len(),
                    modified: metadata.modified()?,
                });
                ensure!(files.len() <= MAX_FILES, "too many project files");
            }
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// Check whether a project contains any files visible to TrackMesh without hashing them.
/// Stops at the first visible entry, so status polling does not walk a full sample tree.
/// Anything a scan would reject (reparse point, odd name) counts as content, not as empty.
pub fn is_empty(root: &Path) -> Result<bool> {
    let mut directories = vec![root.to_owned()];
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory)? {
            let path = entry?.path();
            let metadata = fs::symlink_metadata(&path)?;
            let Some(relative) = path
                .strip_prefix(root)?
                .to_str()
                .map(|value| value.replace('\\', "/"))
            else {
                return Ok(false);
            };
            if is_reparse(&metadata) {
                return Ok(false);
            }
            if metadata.is_dir() {
                if !is_backup_tree_path(&relative) {
                    directories.push(path);
                }
            } else if !is_ignored_path(&relative) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

pub struct ScanOptions {
    pub chunk_size: u32,
    /// Checked between chunks; a stopped scan commits nothing and its cached chunks
    /// stay unreferenced until storage maintenance prunes them.
    pub stop: Stop,
}
impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            chunk_size: DEFAULT_CHUNK_SIZE,
            stop: Stop::default(),
        }
    }
}

pub fn scan(
    root: &Path,
    chunks: &ChunkStore,
    project_id: Uuid,
    author_device_id: Uuid,
    parent: Option<&Manifest>,
    options: &ScanOptions,
) -> Result<Manifest> {
    scan_impl(
        root,
        Some(chunks),
        project_id,
        author_device_id,
        parent,
        options,
    )
}

/// Scan a folder for comparison without adding chunks to the local chunk store.
pub fn scan_for_comparison(
    root: &Path,
    project_id: Uuid,
    author_device_id: Uuid,
    parent: Option<&Manifest>,
    options: &ScanOptions,
) -> Result<Manifest> {
    scan_impl(root, None, project_id, author_device_id, parent, options)
}

fn scan_impl(
    root: &Path,
    chunks: Option<&ChunkStore>,
    project_id: Uuid,
    author_device_id: Uuid,
    parent: Option<&Manifest>,
    options: &ScanOptions,
) -> Result<Manifest> {
    ensure!(
        options.chunk_size > 0 && options.chunk_size <= protocol::MAX_CHUNK_SIZE,
        "invalid scan chunk size"
    );
    let before = inventory(root)?;
    let previous: HashMap<_, _> = parent
        .map(|p| {
            p.files
                .iter()
                .filter(|f| !is_ignored_path(&f.relative_path))
                .map(|f| (f.relative_path.as_str(), f))
                .collect()
        })
        .unwrap_or_default();
    let mut files = Vec::with_capacity(before.len());
    let mut total_size = 0u64;
    let mut changed_bytes = 0u64;
    let mut buffer = vec![0u8; options.chunk_size as usize];
    let stop = &options.stop;
    for stamp in &before {
        let path = safe_path(root, &stamp.path)?;
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1);
        } // Refuse writers/deleters while hashing this file.
        let mut file = options.open(&path).with_context(|| {
            format!(
                "project file is still being written or locked: {}",
                stamp.path
            )
        })?;
        let metadata = file.metadata()?;
        ensure!(
            !is_reparse(&metadata)
                && metadata.len() == stamp.size
                && metadata.modified()? == stamp.modified,
            "project changed before scan; retry"
        );
        let mut hasher = blake3::Hasher::new();
        let mut parts = Vec::new();
        let mut offset = 0u64;
        loop {
            if stop.requested() {
                return Err(Stopped.into());
            }
            let mut filled = 0;
            while filled < buffer.len() {
                let n = file.read(&mut buffer[filled..])?;
                if n == 0 {
                    break;
                }
                filled += n;
            }
            if filled == 0 {
                break;
            }
            hasher.update(&buffer[..filled]);
            let digest = protocol::hash(&buffer[..filled]);
            if let Some(chunks) = chunks {
                chunks.put(&digest, &buffer[..filled])?;
            }
            parts.push(Chunk {
                offset,
                size: filled as u32,
                blake3: digest,
            });
            offset += filled as u64;
        }
        let after = file.metadata()?;
        ensure!(
            after.len() == stamp.size
                && after.modified()? == stamp.modified
                && offset == stamp.size,
            "project changed during scan; retry"
        );
        total_size = total_size
            .checked_add(offset)
            .context("project size overflow")?;
        ensure!(
            total_size <= MAX_PROJECT_BYTES,
            "project exceeds MVP size limit"
        );
        let digest = hasher.finalize().to_hex().to_string();
        if let Some(old) = previous.get(stamp.path.as_str()) {
            let old_chunks: std::collections::HashSet<_> =
                old.chunks.iter().map(|c| (&c.blake3, c.size)).collect();
            changed_bytes += parts
                .iter()
                .filter(|c| !old_chunks.contains(&(&c.blake3, c.size)))
                .map(|c| c.size as u64)
                .sum::<u64>();
        } else {
            changed_bytes += offset;
        }
        files.push(SnapshotFile {
            relative_path: stamp.path.clone(),
            size: offset,
            blake3: digest,
            chunks: parts,
        });
    }
    ensure!(
        inventory(root)? == before,
        "project directory changed during scan; retry after the DAW finishes saving"
    );
    let manifest = Manifest {
        version: PROTOCOL_VERSION,
        snapshot_id: Uuid::new_v4(),
        project_id,
        parent_snapshot_id: parent.map(|p| p.snapshot_id),
        author_device_id,
        created_at: Utc::now(),
        chunk_size: options.chunk_size,
        files,
        total_size,
        changed_bytes,
    };
    manifest.validate()?;
    Ok(manifest)
}

pub fn safe_path(root: &Path, relative: &str) -> Result<PathBuf> {
    protocol::validate_relative_path(relative)?;
    ensure!(root.is_absolute(), "root must be absolute");
    for parent in root.ancestors() {
        ensure!(
            !is_reparse(&fs::symlink_metadata(parent)?),
            "root path became a reparse point"
        );
    }
    let mut path = root.to_owned();
    for part in relative.split('/') {
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(meta) => ensure!(
                !is_reparse(&meta),
                "refusing reparse point at {}",
                path.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(path)
}

pub fn verify_file(path: &Path, expected: &SnapshotFile) -> Result<()> {
    let _file = open_verified_file(path, expected)?;
    Ok(())
}
// Keep this handle alive when moving a verified original into a recovery backup.
pub fn open_verified_file(path: &Path, expected: &SnapshotFile) -> Result<File> {
    ensure!(
        !is_reparse(&fs::symlink_metadata(path)?),
        "file became a reparse point"
    );
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1 | 4).custom_flags(0x0020_0000);
    }
    let mut file = options.open(path)?;
    ensure!(
        !is_reparse(&file.metadata()?),
        "refusing a reparse file handle"
    );
    ensure!(
        file.metadata()?.len() == expected.size,
        "received file size mismatch"
    );
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    ensure!(
        hasher.finalize().to_hex().as_str() == expected.blake3,
        "full-file integrity check failed"
    );
    Ok(file)
}

pub struct ProjectLock {
    _file: File,
}
impl ProjectLock {
    pub fn acquire(directory: &Path, project: Uuid) -> Result<Self> {
        let locks = directory.join("locks");
        fs::create_dir_all(&locks)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(0);
        }
        #[cfg(not(windows))]
        {
            anyhow::bail!("project operations require Windows file locking");
        }
        let file = options
            .open(locks.join(format!("{project}.lock")))
            .context("another snapshot or receive is already active for this project")?;
        Ok(Self { _file: file })
    }
}

#[cfg(windows)]
pub fn atomic_replace(source: &Path, destination: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    unsafe {
        ensure!(
            MoveFileExW(
                source.as_ptr(),
                destination.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH
            ) != 0,
            "atomic file replacement failed: {}",
            std::io::Error::last_os_error()
        );
    }
    Ok(())
}
#[cfg(not(windows))]
pub fn atomic_replace(source: &Path, destination: &Path) -> Result<()> {
    fs::rename(source, destination)?;
    if let Some(parent) = destination.parent() {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}
