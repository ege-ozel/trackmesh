use anyhow::Result;
use axum::http::StatusCode;
use chrono::Utc;
use protocol::{
    Envelope, MailboxAck, MailboxAuth, MailboxPage, MailboxRecord, MAX_MAILBOX_PAGE_RECORDS,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use std::{path::Path, sync::Mutex, time::Duration};

pub struct Store {
    db: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let db = Connection::open(path)?;
        db.busy_timeout(Duration::from_secs(5))?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS announcements(sequence INTEGER PRIMARY KEY AUTOINCREMENT,project_id TEXT NOT NULL,snapshot_id TEXT NOT NULL UNIQUE,envelope_json TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS announcements_project ON announcements(project_id,sequence);
            CREATE TABLE IF NOT EXISTS requests(device_id TEXT NOT NULL,nonce TEXT NOT NULL,seen_at INTEGER NOT NULL,PRIMARY KEY(device_id,nonce));
            CREATE INDEX IF NOT EXISTS requests_time ON requests(seen_at);")?;
        Ok(Self { db: Mutex::new(db) })
    }

    pub fn publish(
        &self,
        auth: &MailboxAuth,
        validated: Result<Envelope, StatusCode>,
    ) -> Result<MailboxAck, StatusCode> {
        let mut db = self
            .db
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let tx = db.transaction().map_err(internal)?;
        admit(&tx, auth)?;
        let result = validated.and_then(|envelope| publish_record(&tx, &envelope));
        // Authentication replay state survives rejected records and lost HTTP responses.
        tx.commit().map_err(internal)?;
        result
    }

    pub fn page(&self, auth: &MailboxAuth, cursor: i64) -> Result<MailboxPage, StatusCode> {
        let mut db = self
            .db
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let tx = db.transaction().map_err(internal)?;
        admit(&tx, auth)?;
        let records = {
            let mut stmt = tx.prepare("SELECT sequence,envelope_json FROM announcements WHERE project_id=?1 AND sequence>?2 ORDER BY sequence LIMIT ?3").map_err(internal)?;
            let rows = stmt
                .query_map(
                    params![
                        auth.project_id.to_string(),
                        cursor,
                        MAX_MAILBOX_PAGE_RECORDS
                    ],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
                )
                .map_err(internal)?;
            let mut records = Vec::new();
            for row in rows {
                let (sequence, json) = row.map_err(internal)?;
                records.push(MailboxRecord {
                    sequence,
                    envelope: serde_json::from_str(&json).map_err(internal)?,
                });
            }
            records
        };
        tx.commit().map_err(internal)?;
        Ok(MailboxPage {
            next_cursor: records.last().map_or(cursor, |record| record.sequence),
            records,
        })
    }
}

fn admit(tx: &Transaction<'_>, auth: &MailboxAuth) -> Result<(), StatusCode> {
    let now = Utc::now().timestamp();
    tx.execute("DELETE FROM requests WHERE seen_at<?1", [now - 240])
        .map_err(internal)?;
    let device = auth.device_id.to_string();
    let replay: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM requests WHERE device_id=?1 AND nonce=?2)",
            params![device, auth.nonce.to_string()],
            |r| r.get(0),
        )
        .map_err(internal)?;
    if replay {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let count: u32 = tx
        .query_row(
            "SELECT COUNT(*) FROM requests WHERE device_id=?1 AND seen_at>=?2",
            params![device, now - 60],
            |r| r.get(0),
        )
        .map_err(internal)?;
    if count >= 120 {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
    tx.execute(
        "INSERT INTO requests(device_id,nonce,seen_at) VALUES(?1,?2,?3)",
        params![device, auth.nonce.to_string(), now],
    )
    .map_err(internal)?;
    Ok(())
}

fn publish_record(tx: &Transaction<'_>, envelope: &Envelope) -> Result<MailboxAck, StatusCode> {
    let json = serde_json::to_string(envelope).map_err(internal)?;
    let existing: Option<(i64, String)> = tx
        .query_row(
            "SELECT sequence,envelope_json FROM announcements WHERE snapshot_id=?1",
            [envelope.snapshot_id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(internal)?;
    let sequence = if let Some((sequence, previous)) = existing {
        if previous != json {
            return Err(StatusCode::CONFLICT);
        }
        sequence
    } else {
        let count: u32 = tx
            .query_row(
                "SELECT COUNT(*) FROM announcements WHERE project_id=?1",
                [envelope.project_id.to_string()],
                |r| r.get(0),
            )
            .map_err(internal)?;
        if count >= 10_000 {
            return Err(StatusCode::INSUFFICIENT_STORAGE);
        }
        tx.execute(
            "INSERT INTO announcements(project_id,snapshot_id,envelope_json) VALUES(?1,?2,?3)",
            params![
                envelope.project_id.to_string(),
                envelope.snapshot_id.to_string(),
                json
            ],
        )
        .map_err(internal)?;
        tx.last_insert_rowid()
    };
    Ok(MailboxAck {
        snapshot_id: envelope.snapshot_id,
        sequence,
    })
}

fn internal(_: impl std::fmt::Display) -> StatusCode {
    StatusCode::INTERNAL_SERVER_ERROR
}
