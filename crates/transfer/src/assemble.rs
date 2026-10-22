//! Assemble and fsync complete, full-file-verified data outside the working folder.
use anyhow::{ensure, Context, Result};
use protocol::SignedSnapshot;
use snapshot::ChunkStore;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

pub(crate) fn assemble(directory: &Path, chunks: &ChunkStore, s: &SignedSnapshot) -> Result<()> {
    fs::create_dir_all(directory)?;
    for expected in &s.manifest.files {
        let path = snapshot::safe_path(directory, &expected.relative_path)?;
        if path.exists() && snapshot::verify_file(&path, expected).is_ok() {
            continue;
        }
        fs::create_dir_all(path.parent().context("file has no parent")?)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)?;
        let mut hasher = blake3::Hasher::new();
        for chunk in &expected.chunks {
            let data = chunks.read(&chunk.blake3, chunk.size)?;
            hasher.update(&data);
            file.write_all(&data)?;
        }
        ensure!(
            hasher.finalize().to_hex().as_str() == expected.blake3
                && file.metadata()?.len() == expected.size,
            "assembled file failed full BLAKE3 verification"
        );
        file.sync_all()?;
    }
    Ok(())
}
