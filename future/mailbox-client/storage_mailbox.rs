//! Announcement persistence, pending publication, acknowledgements, and cursors.
use crate::{
    models::{info_row, INFO_COLUMNS},
    SnapshotInfo, Store,
};
use anyhow::{ensure, Result};
use protocol::{Announcement, Envelope, MailboxAck, SignedSnapshot};
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

impl Store {
    pub fn has_pending_publication(&self, author: Uuid) -> Result<bool> {
        Ok(self.lock()?.query_row(
            // Only a project with a mailbox can ever clear PENDING; without one, waiting would keep the app alive forever.
            "SELECT EXISTS(SELECT 1 FROM snapshots s JOIN projects p ON p.project_id=s.project_id WHERE s.author_device_id=?1 AND s.metadata_status='PENDING' AND s.location='LOCAL' AND p.mailbox_url IS NOT NULL AND p.mailbox_url<>'')",
            [author.to_string()],
            |row| row.get(0),
        )?)
    }
    pub fn ingest_remote(&self, envelope: &Envelope, announcement: &Announcement) -> Result<()> {
        let db = self.lock()?;
        ingest(&db, envelope, announcement)
    }
    pub fn pending_publications(&self) -> Result<Vec<SignedSnapshot>> {
        let db = self.lock()?;
        let mut stmt = db.prepare("SELECT snapshot_json FROM snapshots WHERE metadata_status='PENDING' AND location='LOCAL' ORDER BY created_at")?;
        let values = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        values
            .into_iter()
            .map(|s| Ok(serde_json::from_str(&s)?))
            .collect()
    }
    pub fn publication_envelope(&self, id: Uuid) -> Result<Option<Envelope>> {
        let json: Option<Option<String>> = self
            .lock()?
            .query_row(
                "SELECT envelope_json FROM snapshots WHERE snapshot_id=?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        json.flatten()
            .map(|s| Ok(serde_json::from_str(&s)?))
            .transpose()
    }
    pub fn save_envelope(&self, envelope: &Envelope) -> Result<()> {
        let db = self.lock()?;
        let bound: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM snapshots WHERE snapshot_id=?1 AND project_id=?2 AND author_device_id=?3 AND location='LOCAL' AND metadata_status IN ('PENDING','PUBLISHED'))",params![envelope.snapshot_id.to_string(),envelope.project_id.to_string(),envelope.author_device_id.to_string()], |r| r.get(0))?;
        ensure!(
            bound,
            "publication does not match a local authored snapshot"
        );
        db.execute(
            "UPDATE snapshots SET envelope_json=COALESCE(envelope_json,?2) WHERE snapshot_id=?1",
            params![
                envelope.snapshot_id.to_string(),
                serde_json::to_string(envelope)?
            ],
        )?;
        Ok(())
    }
    pub fn published(&self, id: Uuid) -> Result<()> {
        self.lock()?.execute("UPDATE snapshots SET metadata_status='PUBLISHED' WHERE snapshot_id=?1 AND metadata_status='PENDING'", [id.to_string()])?;
        Ok(())
    }
    pub fn cursor(&self, project: Uuid) -> Result<i64> {
        Ok(self.lock()?.query_row(
            "SELECT cursor FROM mailbox_state WHERE project_id=?1",
            [project.to_string()],
            |r| r.get(0),
        )?)
    }
    pub fn set_cursor(&self, project: Uuid, cursor: i64) -> Result<()> {
        self.lock()?.execute(
            "UPDATE mailbox_state SET cursor=?2 WHERE project_id=?1 AND cursor<=?2",
            params![project.to_string(), cursor],
        )?;
        Ok(())
    }

    pub fn pending_publication_ids(&self, project: Uuid, author: Uuid) -> Result<Vec<Uuid>> {
        let db = self.lock()?;
        let mut stmt = db.prepare("SELECT snapshot_id FROM snapshots WHERE project_id=?1 AND author_device_id=?2 AND metadata_status='PENDING' AND location='LOCAL' ORDER BY created_at,snapshot_id LIMIT 64")?;
        let ids = stmt
            .query_map(params![project.to_string(), author.to_string()], |row| {
                crate::models::uuid_value(row.get(0)?)
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(ids)
    }

    pub fn pending_publication_count(&self, project: Uuid, author: Uuid) -> Result<u64> {
        Ok(self.lock()?.query_row("SELECT COUNT(*) FROM snapshots WHERE project_id=?1 AND author_device_id=?2 AND metadata_status='PENDING' AND location='LOCAL'", params![project.to_string(),author.to_string()], |r| r.get(0))?)
    }

    pub fn publication_ack(&self, envelope: &Envelope, ack: &MailboxAck) -> Result<()> {
        ensure!(
            ack.snapshot_id == envelope.snapshot_id && ack.sequence > 0,
            "invalid publication ACK"
        );
        let db = self.lock()?;
        let matches: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM snapshots WHERE snapshot_id=?1 AND envelope_json=?2 AND location='LOCAL' AND metadata_status IN ('PENDING','PUBLISHED'))", params![envelope.snapshot_id.to_string(),serde_json::to_string(envelope)?], |r| r.get(0))?;
        ensure!(
            matches,
            "ACK does not match the durable publication envelope"
        );
        db.execute("UPDATE snapshots SET metadata_status='PUBLISHED' WHERE snapshot_id=?1 AND metadata_status='PENDING'", [envelope.snapshot_id.to_string()])?;
        Ok(())
    }

    pub fn ingest_mailbox_page(
        &self,
        project: Uuid,
        local_device: Uuid,
        expected_cursor: i64,
        records: &[(i64, Envelope, Announcement)],
        next_cursor: i64,
    ) -> Result<()> {
        ensure!(
            expected_cursor >= 0 && records.len() <= protocol::MAX_MAILBOX_PAGE_RECORDS,
            "invalid mailbox page bounds"
        );
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        let cursor: i64 = tx.query_row(
            "SELECT cursor FROM mailbox_state WHERE project_id=?1",
            [project.to_string()],
            |r| r.get(0),
        )?;
        ensure!(
            cursor == expected_cursor,
            "mailbox cursor changed; refresh again"
        );
        let mut previous = expected_cursor;
        for (sequence, envelope, announcement) in records {
            ensure!(
                *sequence > previous && envelope.project_id == project,
                "invalid mailbox sequence or project"
            );
            let author = envelope.author_device_id;
            let trusted: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM devices d WHERE d.device_id=?1 AND d.public_key=?2 AND (d.device_id=?3 OR EXISTS(SELECT 1 FROM project_peers p JOIN trusted_peers t ON t.device_id=p.device_id WHERE p.project_id=?4 AND p.device_id=d.device_id AND t.trust_status='TRUSTED')))", params![author.to_string(),envelope.public_key,local_device.to_string(),project.to_string()], |r| r.get(0))?;
            ensure!(trusted, "mailbox author is not authorized for this project");
            ingest(&tx, envelope, announcement)?;
            previous = *sequence;
        }
        ensure!(
            next_cursor == previous,
            "mailbox cursor does not match page"
        );
        tx.execute(
            "UPDATE mailbox_state SET cursor=?2 WHERE project_id=?1",
            params![project.to_string(), next_cursor],
        )?;
        tx.commit()?;
        Ok(())
    }
}

fn ingest(db: &Connection, envelope: &Envelope, announcement: &Announcement) -> Result<()> {
    let previous: Option<(SnapshotInfo, Option<String>)> = db
        .query_row(
            &format!("SELECT {INFO_COLUMNS},envelope_json FROM snapshots WHERE snapshot_id=?1"),
            [envelope.snapshot_id.to_string()],
            |r| Ok((info_row(r)?, r.get(13)?)),
        )
        .optional()?;
    let json = serde_json::to_string(envelope)?;
    if let Some((info, stored_envelope)) = previous {
        ensure!(
            info.manifest_hash == announcement.manifest_hash
                && info.author_device_id == envelope.author_device_id
                && info.project_id == envelope.project_id
                && info.parent_snapshot_id == announcement.parent_snapshot_id
                && info.created_at == envelope.created_at.to_rfc3339()
                && info.total_size == announcement.total_size
                && info.changed_bytes == announcement.changed_bytes
                && info.file_count == announcement.file_count,
            "remote immutable snapshot collision"
        );
        ensure!(
            stored_envelope
                .as_ref()
                .is_none_or(|stored| stored == &json),
            "remote immutable envelope collision"
        );
        db.execute("UPDATE snapshots SET announcement_json=COALESCE(announcement_json,?2),envelope_json=COALESCE(envelope_json,?3) WHERE snapshot_id=?1", params![envelope.snapshot_id.to_string(),serde_json::to_string(announcement)?,json])?;
    } else {
        db.execute("INSERT INTO snapshots(snapshot_id,project_id,parent_snapshot_id,author_device_id,created_at,manifest_hash,announcement_json,envelope_json,location,metadata_status,total_size,changed_bytes,file_count,author_name) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'REMOTE','REMOTE',?9,?10,?11,?12)", params![envelope.snapshot_id.to_string(),envelope.project_id.to_string(),announcement.parent_snapshot_id.map(|v|v.to_string()),envelope.author_device_id.to_string(),envelope.created_at.to_rfc3339(),announcement.manifest_hash,serde_json::to_string(announcement)?,json,announcement.total_size,announcement.changed_bytes,announcement.file_count,announcement.author_name])?;
    }
    Ok(())
}
