//! Durable endpoint sequence allocation and accepted peer endpoint snapshots.
use crate::{EndpointWriteOutcome, PeerEndpointState, Store};
use anyhow::{ensure, Context, Result};
use chrono::{DateTime, Utc};
use protocol::{EndpointRecord, EndpointSequence};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

impl Store {
    /// Return the current saved endpoint using this process's UTC clock.
    pub fn current_peer_endpoint_state(
        &self,
        device_id: Uuid,
    ) -> Result<Option<PeerEndpointState>> {
        self.peer_endpoint_state(device_id, Utc::now())
    }

    /// Atomically reserve a durable, monotonically increasing endpoint sequence.
    /// The wall clock supplies a high starting point after backup restore; a
    /// clock rollback falls back to the previous sequence plus one.
    pub fn next_endpoint_sequence(&self, device_id: Uuid) -> Result<u64> {
        let mut db = self.lock()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let device = device_id.to_string();
        let previous: Option<String> = tx
            .query_row(
                "SELECT last_seq FROM endpoint_local_sequence WHERE device_id=?1",
                [&device],
                |row| row.get(0),
            )
            .optional()?;
        let previous = previous
            .map(|value| parse_sequence(&value))
            .transpose()?
            .unwrap_or(0);
        let incremented = previous
            .checked_add(1)
            .context("endpoint sequence exhausted")?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0);
        let now = u64::try_from(now).context("current Unix millisecond value exceeds u64")?;
        let next = incremented.max(now);
        tx.execute(
            "INSERT INTO endpoint_local_sequence(device_id,last_seq) VALUES(?1,?2)
             ON CONFLICT(device_id) DO UPDATE SET last_seq=excluded.last_seq",
            params![device, next.to_string()],
        )?;
        tx.commit()?;
        Ok(next)
    }

    /// Return the saved endpoint and replay watermark. Expired records are
    /// omitted as dial candidates while their watermark remains available.
    pub fn peer_endpoint_state(
        &self,
        device_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<Option<PeerEndpointState>> {
        let db = self.lock()?;
        let device = device_id.to_string();
        let row: Option<(String, String, String)> = db
            .query_row(
                "SELECT highest_seq,record_json,expires_at FROM peer_endpoints WHERE device_id=?1",
                [&device],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((highest_seq, record_json, expires_at)) = row else {
            return Ok(None);
        };
        let highest_seq = parse_sequence(&highest_seq)?;
        let record: EndpointRecord =
            serde_json::from_str(&record_json).context("invalid stored peer endpoint record")?;
        ensure!(
            record.device_id == device_id,
            "stored peer endpoint device mismatch"
        );
        ensure!(
            record.seq == highest_seq,
            "stored peer endpoint watermark mismatch"
        );
        let stored_expiry = DateTime::parse_from_rfc3339(&expires_at)
            .context("invalid stored peer endpoint expiry")?
            .with_timezone(&Utc);
        ensure!(
            stored_expiry == record.expires_at,
            "stored peer endpoint expiry mismatch"
        );
        Ok(Some(PeerEndpointState {
            record: (now < stored_expiry).then_some(record),
            highest_seq,
        }))
    }

    /// Persist a previously validated endpoint. A stale or equivocal sequence
    /// fails closed; an exact duplicate is an idempotent no-op.
    pub fn store_peer_endpoint(&self, record: &EndpointRecord) -> Result<EndpointWriteOutcome> {
        let mut db = self.lock()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let device = record.device_id.to_string();
        let existing: Option<(String, String, String)> = tx
            .query_row(
                "SELECT highest_seq,record_json,expires_at FROM peer_endpoints WHERE device_id=?1",
                [&device],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let outcome = if let Some((highest_seq, record_json, expires_at)) = existing {
            let highest_seq = parse_sequence(&highest_seq)?;
            let previous: EndpointRecord = serde_json::from_str(&record_json)
                .context("invalid stored peer endpoint record")?;
            ensure!(
                previous.device_id == record.device_id,
                "stored peer endpoint device mismatch"
            );
            ensure!(
                previous.seq == highest_seq,
                "stored peer endpoint watermark mismatch"
            );
            let stored_expiry = DateTime::parse_from_rfc3339(&expires_at)
                .context("invalid stored peer endpoint expiry")?
                .with_timezone(&Utc);
            ensure!(
                stored_expiry == previous.expires_at,
                "stored peer endpoint expiry mismatch"
            );
            match record.compare_sequence(&previous)? {
                EndpointSequence::Duplicate => EndpointWriteOutcome::Duplicate,
                EndpointSequence::Newer => {
                    write_endpoint(&tx, &device, record)?;
                    EndpointWriteOutcome::Stored
                }
            }
        } else {
            write_endpoint(&tx, &device, record)?;
            EndpointWriteOutcome::Stored
        };
        tx.commit()?;
        Ok(outcome)
    }
}

fn parse_sequence(value: &str) -> Result<u64> {
    let sequence = value
        .parse::<u64>()
        .context("invalid stored endpoint sequence")?;
    ensure!(
        sequence.to_string() == value,
        "non-canonical stored endpoint sequence"
    );
    Ok(sequence)
}

fn write_endpoint(
    tx: &rusqlite::Transaction<'_>,
    device: &str,
    record: &EndpointRecord,
) -> Result<()> {
    let record_json = serde_json::to_string(record)?;
    tx.execute(
        "INSERT INTO peer_endpoints(device_id,highest_seq,record_json,expires_at)
         VALUES(?1,?2,?3,?4)
         ON CONFLICT(device_id) DO UPDATE SET highest_seq=excluded.highest_seq,
             record_json=excluded.record_json,expires_at=excluded.expires_at",
        params![
            device,
            record.seq.to_string(),
            record_json,
            record.expires_at.to_rfc3339()
        ],
    )?;
    Ok(())
}
