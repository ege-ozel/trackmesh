//! SQLite connection ownership, schema initialization, and public storage API.
//! Domain modules implement `Store`; transactions never cross module boundaries.
mod endpoints;
mod models;
mod projects;
mod retention;
mod snapshots;
mod transfer_state;
mod transfers;
mod trust;

pub use models::{
    EndpointWriteOutcome, Lineage, PeerEndpointState, Project, SnapshotInfo, TransferInfo,
};
pub use transfer_state::TransferState;

use anyhow::{ensure, Result};
use rusqlite::Connection;
use std::{path::Path, sync::Mutex};

pub struct Store {
    connection: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(10))?;
        let version: u32 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
        ensure!(version <= 6, "database is from a newer TrackMesh version");
        connection.execute_batch(include_str!("schema.sql"))?;
        if version < 2 {
            let tx = connection.transaction()?;
            tx.execute("INSERT OR IGNORE INTO snapshot_chunks(snapshot_id,blake3,size) SELECT f.snapshot_id,json_extract(c.value,'$.blake3'),json_extract(c.value,'$.size') FROM snapshot_files f,json_each(f.chunks_json) c",[])?;
            tx.pragma_update(None, "user_version", 2)?;
            tx.commit()?;
        }
        if version < 3 {
            let tx =
                connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            tx.execute_batch(
                "CREATE TABLE endpoint_local_sequence (
                    device_id TEXT PRIMARY KEY,
                    last_seq TEXT NOT NULL
                );
                CREATE TABLE peer_endpoints (
                    device_id TEXT PRIMARY KEY REFERENCES devices(device_id),
                    highest_seq TEXT NOT NULL,
                    record_json TEXT NOT NULL,
                    expires_at TEXT NOT NULL
                );",
            )?;
            tx.pragma_update(None, "user_version", 3)?;
            tx.commit()?;
        }
        if version < 4 {
            // 0 listed, 1 removed with its data kept (can be restored), 2 removed with its data deleted
            // (the row stays only until queued access removals and the key file are gone).
            let tx =
                connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            tx.execute_batch(
                "ALTER TABLE projects ADD COLUMN removed INTEGER NOT NULL DEFAULT 0;",
            )?;
            tx.pragma_update(None, "user_version", 4)?;
            tx.commit()?;
        }
        if version < 5 {
            // The last version each collaborator confirmed having; projects.verified_* kept only one.
            let tx =
                connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            tx.execute_batch(
                "CREATE TABLE deliveries (
                    project_id TEXT NOT NULL REFERENCES projects(project_id),
                    peer_id TEXT NOT NULL,
                    snapshot_id TEXT NOT NULL,
                    at TEXT NOT NULL,
                    PRIMARY KEY(project_id, peer_id)
                );
                INSERT INTO deliveries SELECT project_id,verified_peer_id,verified_snapshot_id,last_verified_at
                    FROM projects WHERE verified_peer_id IS NOT NULL AND verified_snapshot_id IS NOT NULL AND last_verified_at IS NOT NULL;",
            )?;
            tx.pragma_update(None, "user_version", 5)?;
            tx.commit()?;
        }
        if version < 6 {
            // Authors of versions a paired peer served, for devices this PC never paired with.
            let tx =
                connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            tx.execute_batch(
                "CREATE TABLE snapshot_authors (
                    project_id TEXT NOT NULL REFERENCES projects(project_id),
                    device_id TEXT NOT NULL,
                    public_key TEXT NOT NULL,
                    PRIMARY KEY(project_id, device_id)
                );",
            )?;
            tx.pragma_update(None, "user_version", 6)?;
            tx.commit()?;
        }
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))
    }
}
