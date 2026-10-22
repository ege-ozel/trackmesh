//! Durable apply journal format. These names and values are recovery compatibility contracts.
use anyhow::{Context, Result};
use std::{
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
};
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum ApplyPhase {
    Backup,
    Install,
    Commit,
    Done,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ApplyJournal {
    pub(crate) transfer_id: Uuid,
    pub(crate) project_id: Uuid,
    pub(crate) snapshot_id: Uuid,
    pub(crate) old_files: Vec<String>,
    pub(crate) new_files: Vec<String>,
    pub(crate) phase: ApplyPhase,
}

pub fn stage_path(root: &Path, project: Uuid, transfer: Uuid) -> Result<PathBuf> {
    Ok(root
        .parent()
        .context("project folder must have a parent")?
        .join(format!(".trackmesh-{project}-{transfer}")))
}
pub(crate) fn write_journal(directory: &Path, journal: &ApplyJournal) -> Result<()> {
    let path = snapshot::safe_path(directory, "journal.json")?;
    let temp = snapshot::safe_path(directory, "journal.partial")?;
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temp)?;
    file.write_all(&serde_json::to_vec(journal)?)?;
    file.sync_all()?;
    drop(file);
    snapshot::atomic_replace(&temp, &path)
}
