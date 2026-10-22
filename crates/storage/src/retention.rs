//! Retain file data for current versions; immutable signed history stays in SQLite.
use crate::{Store, TransferState};
use anyhow::Result;
use std::collections::HashSet;

impl Store {
    /// Caller owns the exclusive cache lease; no scanner/receiver can create unindexed data.
    pub fn retain_current_chunks(&self) -> Result<HashSet<String>> {
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        tx.execute_batch("CREATE TEMP TABLE IF NOT EXISTS retained_versions(snapshot_id TEXT PRIMARY KEY); DELETE FROM retained_versions;
            INSERT OR IGNORE INTO retained_versions SELECT latest_local_snapshot FROM projects WHERE latest_local_snapshot IS NOT NULL;
            INSERT OR IGNORE INTO retained_versions SELECT snapshot_id FROM (
                SELECT snapshot_id,ROW_NUMBER() OVER (PARTITION BY project_id,author_device_id ORDER BY local_sequence DESC,created_at DESC,snapshot_id DESC) AS rank
                FROM snapshots WHERE location='LOCAL') WHERE rank=1;
            INSERT OR IGNORE INTO retained_versions SELECT snapshot_id FROM snapshots WHERE project_id IN (SELECT project_id FROM projects WHERE recovery_required=1);")?;
        tx.execute("INSERT OR IGNORE INTO retained_versions SELECT base_snapshot_id FROM transfer_sessions WHERE state<>?1 AND base_snapshot_id IS NOT NULL", [TransferState::Complete])?;
        tx.execute("INSERT OR IGNORE INTO retained_versions SELECT snapshot_id FROM transfer_sessions WHERE state<>?1", [TransferState::Complete])?;
        let mut hashes = {
            let mut stmt = tx.prepare("SELECT blake3 FROM snapshot_chunks WHERE snapshot_id IN (SELECT snapshot_id FROM retained_versions)")?;
            let values = stmt
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<HashSet<_>>>()?;
            values
        };
        {
            let mut stmt =
                tx.prepare("SELECT snapshot_json FROM transfer_sessions WHERE state<>?1")?;
            let manifests = stmt
                .query_map([TransferState::Complete], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for json in manifests {
                let signed: protocol::SignedSnapshot = serde_json::from_str(&json)?;
                signed.validate_structure()?;
                hashes.extend(
                    signed
                        .manifest
                        .files
                        .iter()
                        .flat_map(|file| file.chunks.iter().map(|chunk| chunk.blake3.clone())),
                );
            }
        }
        tx.execute_batch("DELETE FROM snapshot_files WHERE snapshot_id NOT IN (SELECT snapshot_id FROM retained_versions);
            DELETE FROM snapshot_chunks WHERE snapshot_id NOT IN (SELECT snapshot_id FROM retained_versions);
            DROP TABLE retained_versions;")?;
        tx.commit()?;
        Ok(hashes)
    }
}
