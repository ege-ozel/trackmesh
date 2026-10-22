//! Durable transfer sessions and progress; apply markers are transactional.
use crate::{
    models::{optional_uuid, uuid_value},
    Store, TransferInfo, TransferState,
};
use anyhow::Result;
use chrono::Utc;
use protocol::SignedSnapshot;
use rusqlite::{params, OptionalExtension};
use uuid::Uuid;

impl Store {
    /// Existing completion metadata only; advisory callers must not write presence to storage.
    pub fn last_completed_apply(&self, project: Uuid) -> Result<Option<std::time::SystemTime>> {
        let completed: Option<String> = self.lock()?.query_row(
            "SELECT MAX(updated_at) FROM transfer_sessions WHERE project_id=?1 AND state=?2",
            params![project.to_string(), TransferState::Complete],
            |row| row.get(0),
        )?;
        completed
            .map(|value| Ok(chrono::DateTime::parse_from_rfc3339(&value)?.into()))
            .transpose()
    }

    pub fn has_active_transfer(&self, project: Uuid) -> Result<bool> {
        Ok(self.lock()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM transfer_sessions WHERE project_id=?1 AND state<>?2)",
            params![project.to_string(), TransferState::Complete],
            |row| row.get(0),
        )?)
    }

    pub fn latest_transfer(&self, project: Uuid) -> Result<Option<TransferInfo>> {
        Ok(self.lock()?.query_row("SELECT transfer_id,project_id,snapshot_id,source_device_id,state,completed_bytes,total_bytes,error,base_snapshot_id FROM transfer_sessions WHERE project_id=?1 ORDER BY updated_at DESC,transfer_id DESC LIMIT 1", [project.to_string()], transfer_row).optional()?)
    }
    pub fn begin_transfer(&self, snapshot: &SignedSnapshot, source: Uuid) -> Result<Uuid> {
        let m = &snapshot.manifest;
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT transfer_id FROM transfer_sessions WHERE project_id=?1 AND snapshot_id=?2",
                params![m.project_id.to_string(), m.snapshot_id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = existing {
            tx.execute(
                "UPDATE transfer_sessions SET source_device_id=?2 WHERE transfer_id=?1",
                params![id, source.to_string()],
            )?;
            tx.commit()?;
            return Ok(Uuid::parse_str(&id)?);
        }
        let base: Option<String> = tx.query_row(
            "SELECT latest_local_snapshot FROM projects WHERE project_id=?1",
            [m.project_id.to_string()],
            |r| r.get(0),
        )?;
        let id = Uuid::new_v4();
        let chunks: std::collections::HashMap<_, _> = m
            .files
            .iter()
            .flat_map(|f| &f.chunks)
            .map(|c| (&c.blake3, c.size))
            .collect();
        let total_bytes: u64 = chunks.values().map(|s| *s as u64).sum();
        tx.execute("INSERT INTO transfer_sessions(transfer_id,project_id,snapshot_id,source_device_id,state,total_bytes,updated_at,snapshot_json,base_snapshot_id) VALUES(?1,?2,?3,?4,?9,?5,?6,?7,?8)",params![id.to_string(),m.project_id.to_string(),m.snapshot_id.to_string(),source.to_string(),total_bytes,Utc::now().to_rfc3339(),serde_json::to_string(snapshot)?,base,TransferState::Queued])?;
        tx.commit()?;
        Ok(id)
    }
    /// Points a transfer that has not started installing at the current local head. Its
    /// verified chunks stay valid; the caller has just checked lineage and the working folder
    /// against `base`. Refuses once the transfer is applying or needs recovery.
    pub fn rebase_transfer(&self, id: Uuid, base: Option<Uuid>) -> Result<()> {
        let updated = self.lock()?.execute(
            "UPDATE transfer_sessions SET base_snapshot_id=?2 WHERE transfer_id=?1 AND state IN (?3,?4,?5)",
            params![
                id.to_string(),
                base.map(|b| b.to_string()),
                TransferState::Queued,
                TransferState::Receiving,
                TransferState::Paused
            ],
        )?;
        anyhow::ensure!(updated == 1, "transfer is already being installed");
        Ok(())
    }
    pub fn transfer_manifest(&self, id: Uuid) -> Result<SignedSnapshot> {
        let s: String = self.lock()?.query_row(
            "SELECT snapshot_json FROM transfer_sessions WHERE transfer_id=?1",
            [id.to_string()],
            |r| r.get(0),
        )?;
        Ok(serde_json::from_str(&s)?)
    }
    pub fn transfers(&self) -> Result<Vec<TransferInfo>> {
        let db = self.lock()?;
        let mut stmt = db.prepare("SELECT transfer_id,project_id,snapshot_id,source_device_id,state,completed_bytes,total_bytes,error,base_snapshot_id FROM transfer_sessions ORDER BY updated_at DESC")?;
        let rows = stmt
            .query_map([], |r| {
                Ok(TransferInfo {
                    transfer_id: uuid_value(r.get(0)?)?,
                    project_id: uuid_value(r.get(1)?)?,
                    snapshot_id: uuid_value(r.get(2)?)?,
                    source_device_id: uuid_value(r.get(3)?)?,
                    state: r.get(4)?,
                    completed_bytes: r.get(5)?,
                    total_bytes: r.get(6)?,
                    error: r.get(7)?,
                    base_snapshot_id: optional_uuid(r.get(8)?)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
    pub fn transfer_state(
        &self,
        id: Uuid,
        state: TransferState,
        error: Option<&str>,
    ) -> Result<()> {
        self.lock()?.execute(
            "UPDATE transfer_sessions SET state=?2,error=?3,updated_at=?4 WHERE transfer_id=?1",
            params![id.to_string(), state, error, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }
    pub fn transfer_chunk(
        &self,
        id: Uuid,
        hash: &str,
        size: u32,
        completed_bytes: u64,
    ) -> Result<()> {
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        tx.execute(
            "INSERT OR IGNORE INTO transfer_chunks(transfer_id,blake3,size) VALUES(?1,?2,?3)",
            params![id.to_string(), hash, size],
        )?;
        tx.execute(
            "UPDATE transfer_sessions SET completed_bytes=?2,updated_at=?3 WHERE transfer_id=?1",
            params![id.to_string(), completed_bytes, Utc::now().to_rfc3339()],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn reset_transfer_progress(&self, id: Uuid, total_bytes: u64) -> Result<()> {
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        tx.execute(
            "DELETE FROM transfer_chunks WHERE transfer_id=?1",
            [id.to_string()],
        )?;
        tx.execute(
            "UPDATE transfer_sessions SET completed_bytes=0,total_bytes=?2 WHERE transfer_id=?1",
            params![id.to_string(), total_bytes],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn begin_apply(&self, id: Uuid) -> Result<()> {
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        tx.execute("UPDATE projects SET recovery_required=1 WHERE project_id=(SELECT project_id FROM transfer_sessions WHERE transfer_id=?1)",[id.to_string()])?;
        tx.execute(
            "UPDATE transfer_sessions SET state=?3,error=NULL,updated_at=?2 WHERE transfer_id=?1",
            params![
                id.to_string(),
                Utc::now().to_rfc3339(),
                TransferState::Applying
            ],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn finish_apply(&self, id: Uuid) -> Result<()> {
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        tx.execute("UPDATE projects SET recovery_required=0,error=NULL WHERE project_id=(SELECT project_id FROM transfer_sessions WHERE transfer_id=?1)",[id.to_string()])?;
        tx.execute("UPDATE transfer_sessions SET state=?3,error=NULL,completed_bytes=total_bytes,updated_at=?2 WHERE transfer_id=?1",params![id.to_string(),Utc::now().to_rfc3339(),TransferState::Complete])?;
        tx.commit()?;
        Ok(())
    }
}

fn transfer_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<TransferInfo> {
    Ok(TransferInfo {
        transfer_id: uuid_value(r.get(0)?)?,
        project_id: uuid_value(r.get(1)?)?,
        snapshot_id: uuid_value(r.get(2)?)?,
        source_device_id: uuid_value(r.get(3)?)?,
        state: r.get(4)?,
        completed_bytes: r.get(5)?,
        total_bytes: r.get(6)?,
        error: r.get(7)?,
        base_snapshot_id: optional_uuid(r.get(8)?)?,
    })
}
