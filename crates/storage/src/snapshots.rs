//! Immutable history, transactional local commits, lineage, and authorized chunk index.
use crate::{
    models::{info_row, INFO_COLUMNS},
    Lineage, SnapshotInfo, Store,
};
use anyhow::{ensure, Result};
use chrono::Utc;
use protocol::SignedSnapshot;
use rusqlite::{params, OptionalExtension, Transaction};
use uuid::Uuid;

const STATUS_HISTORY_LIMIT: u32 = 8;
const STATUS_LINEAGE_DEPTH: u32 = 64;

impl Store {
    pub fn snapshot_info(&self, id: Uuid) -> Result<Option<SnapshotInfo>> {
        Ok(self
            .lock()?
            .query_row(
                &format!("SELECT {INFO_COLUMNS} FROM snapshots WHERE snapshot_id=?1"),
                [id.to_string()],
                info_row,
            )
            .optional()?)
    }
    pub fn latest_author_info(&self, project: Uuid, author: Uuid) -> Result<Option<SnapshotInfo>> {
        Ok(self.lock()?.query_row(&format!("SELECT {INFO_COLUMNS} FROM snapshots WHERE project_id=?1 AND author_device_id=?2 ORDER BY created_at DESC,snapshot_id DESC LIMIT 1"), params![project.to_string(), author.to_string()], info_row).optional()?)
    }
    /// A compact, newest-first history for adapter cards. Complete CLI history stays separate.
    pub fn author_history(&self, project: Uuid, author: Uuid) -> Result<Vec<SnapshotInfo>> {
        let db = self.lock()?;
        let mut stmt = db.prepare(&format!("SELECT {INFO_COLUMNS} FROM snapshots WHERE project_id=?1 AND author_device_id=?2 ORDER BY created_at DESC,snapshot_id DESC LIMIT ?3"))?;
        let history = stmt
            .query_map(
                params![
                    project.to_string(),
                    author.to_string(),
                    STATUS_HISTORY_LIMIT
                ],
                info_row,
            )?
            .collect::<rusqlite::Result<_>>()?;
        Ok(history)
    }
    /// Resolve a last-known relationship with bounded recursive ancestry reads.
    /// Missing or deeper history is UNKNOWN rather than an unsafe divergence claim.
    pub fn known_lineage(
        &self,
        project: Uuid,
        local: Option<Uuid>,
        remote: Uuid,
    ) -> Result<Lineage> {
        let Some(local) = local else {
            return Ok(Lineage::RemoteAhead);
        };
        if local == remote {
            return Ok(Lineage::Equal);
        }
        let db = self.lock()?;
        let mut stmt = db.prepare(
            "WITH RECURSIVE ancestry(origin,snapshot_id,parent_snapshot_id,depth) AS (
                SELECT snapshot_id,snapshot_id,parent_snapshot_id,0 FROM snapshots
                    WHERE project_id=?1 AND snapshot_id IN (?2,?3)
                UNION ALL
                SELECT ancestry.origin,s.snapshot_id,s.parent_snapshot_id,ancestry.depth+1
                    FROM ancestry JOIN snapshots s ON s.project_id=?1 AND s.snapshot_id=ancestry.parent_snapshot_id
                    WHERE ancestry.parent_snapshot_id IS NOT NULL AND ancestry.depth<?4
            ) SELECT origin,snapshot_id,parent_snapshot_id,depth FROM ancestry ORDER BY origin,depth",
        )?;
        let rows = stmt
            .query_map(
                params![
                    project.to_string(),
                    local.to_string(),
                    remote.to_string(),
                    STATUS_LINEAGE_DEPTH
                ],
                |row| {
                    Ok((
                        crate::models::uuid_value(row.get(0)?)?,
                        crate::models::uuid_value(row.get(1)?)?,
                        crate::models::optional_uuid(row.get(2)?)?,
                        row.get::<_, u32>(3)?,
                    ))
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let chain = |origin| {
            rows.iter()
                .filter(|(row_origin, _, _, _)| *row_origin == origin)
                .collect::<Vec<_>>()
        };
        let local_chain = chain(local);
        let remote_chain = chain(remote);
        if remote_chain.iter().any(|(_, id, _, _)| *id == local) {
            return Ok(Lineage::RemoteAhead);
        }
        if local_chain.iter().any(|(_, id, _, _)| *id == remote) {
            return Ok(Lineage::LocalAhead);
        }
        let complete = |values: &[&(Uuid, Uuid, Option<Uuid>, u32)]| {
            values.last().is_some_and(|(_, _, parent, depth)| {
                parent.is_none() && *depth < STATUS_LINEAGE_DEPTH
            })
        };
        Ok(if complete(&local_chain) && complete(&remote_chain) {
            Lineage::Diverged
        } else {
            Lineage::Unknown
        })
    }
    pub fn snapshot(&self, id: Uuid) -> Result<Option<SignedSnapshot>> {
        let value: Option<Option<String>> = self
            .lock()?
            .query_row(
                "SELECT snapshot_json FROM snapshots WHERE snapshot_id=?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        value
            .flatten()
            .map(|s| Ok(serde_json::from_str(&s)?))
            .transpose()
    }
    pub fn head(&self, project: Uuid) -> Result<Option<SignedSnapshot>> {
        self.project(project)?
            .latest_local_snapshot
            .map(|id| self.snapshot(id))
            .transpose()
            .map(Option::flatten)
    }
    pub fn history(&self, project: Uuid) -> Result<Vec<SnapshotInfo>> {
        let db = self.lock()?;
        let mut stmt = db.prepare(&format!("SELECT {INFO_COLUMNS} FROM snapshots WHERE project_id=?1 ORDER BY created_at DESC,snapshot_id"))?;
        let rows = stmt
            .query_map([project.to_string()], info_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
    pub fn commit_local(
        &self,
        snapshot: &SignedSnapshot,
        name: &str,
        expected_head: Option<Uuid>,
        publish: bool,
    ) -> Result<()> {
        snapshot.validate_structure()?;
        let m = &snapshot.manifest;
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        let current: Option<String> = tx.query_row(
            "SELECT latest_local_snapshot FROM projects WHERE project_id=?1",
            [m.project_id.to_string()],
            |r| r.get(0),
        )?;
        ensure!(
            current == expected_head.map(|v| v.to_string()),
            "local head changed during snapshot; retry"
        );
        let previous_hash: Option<String> = tx
            .query_row(
                "SELECT manifest_hash FROM snapshots WHERE snapshot_id=?1",
                [m.snapshot_id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(hash) = previous_hash {
            ensure!(
                hash == snapshot.manifest_hash,
                "immutable snapshot collision"
            );
        }
        let sequence: u64 = tx.query_row(
            "SELECT COALESCE(MAX(local_sequence),0)+1 FROM snapshots WHERE project_id=?1",
            [m.project_id.to_string()],
            |r| r.get(0),
        )?;
        tx.execute("INSERT INTO snapshots(snapshot_id,project_id,parent_snapshot_id,author_device_id,created_at,manifest_hash,snapshot_json,location,metadata_status,local_sequence,total_size,changed_bytes,file_count,author_name) VALUES(?1,?2,?3,?4,?5,?6,?7,'LOCAL',?8,?9,?10,?11,?12,?13) ON CONFLICT(snapshot_id) DO UPDATE SET snapshot_json=excluded.snapshot_json,location='LOCAL',local_sequence=COALESCE(snapshots.local_sequence,excluded.local_sequence)", params![m.snapshot_id.to_string(),m.project_id.to_string(),m.parent_snapshot_id.map(|v|v.to_string()),m.author_device_id.to_string(),m.created_at.to_rfc3339(),snapshot.manifest_hash,serde_json::to_string(snapshot)?,if publish {"PENDING"} else {"REMOTE"},sequence,m.total_size,m.changed_bytes,m.files.len() as u32,name])?;
        insert_files(&tx, snapshot)?;
        tx.execute("UPDATE projects SET latest_local_snapshot=?2,error=NULL,last_verified_at=NULL,verified_snapshot_id=NULL,verified_peer_id=NULL WHERE project_id=?1",params![m.project_id.to_string(),m.snapshot_id.to_string()])?;
        tx.commit()?;
        Ok(())
    }
    pub fn remember_manifest(&self, s: &SignedSnapshot, name: &str) -> Result<()> {
        s.validate_structure()?;
        let m = &s.manifest;
        let db = self.lock()?;
        let existing: Option<String> = db
            .query_row(
                "SELECT manifest_hash FROM snapshots WHERE snapshot_id=?1",
                [m.snapshot_id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(hash) = existing {
            ensure!(
                hash == s.manifest_hash,
                "announced manifest differs from received manifest"
            );
        }
        db.execute("INSERT INTO snapshots(snapshot_id,project_id,parent_snapshot_id,author_device_id,created_at,manifest_hash,snapshot_json,location,metadata_status,total_size,changed_bytes,file_count,author_name) VALUES(?1,?2,?3,?4,?5,?6,?7,'REMOTE','REMOTE',?8,?9,?10,?11) ON CONFLICT(snapshot_id) DO UPDATE SET snapshot_json=excluded.snapshot_json", params![m.snapshot_id.to_string(),m.project_id.to_string(),m.parent_snapshot_id.map(|v|v.to_string()),m.author_device_id.to_string(),m.created_at.to_rfc3339(),s.manifest_hash,serde_json::to_string(s)?,m.total_size,m.changed_bytes,m.files.len() as u32,name])?;
        Ok(())
    }
    pub fn record_chunk(&self, hash: &str, size: u32) -> Result<()> {
        self.lock()?.execute("INSERT INTO chunks(blake3,size,verified_at) VALUES(?1,?2,?3) ON CONFLICT(blake3) DO UPDATE SET verified_at=excluded.verified_at",params![hash,size,Utc::now().to_rfc3339()])?;
        Ok(())
    }
    pub fn mark_verified(&self, project: Uuid, snapshot: Uuid, peer: &str) -> Result<()> {
        let db = self.lock()?;
        let args = params![
            project.to_string(),
            snapshot.to_string(),
            Utc::now().to_rfc3339(),
            peer
        ];
        db.execute("UPDATE projects SET last_verified_at=?3,verified_peer_id=?4,verified_snapshot_id=?2 WHERE project_id=?1 AND latest_local_snapshot=?2", args)?;
        // One row per collaborator, so a second or third one is not overwritten.
        db.execute("INSERT INTO deliveries(project_id,peer_id,snapshot_id,at) SELECT ?1,?4,?2,?3 WHERE EXISTS (SELECT 1 FROM projects WHERE project_id=?1 AND latest_local_snapshot=?2) ON CONFLICT(project_id,peer_id) DO UPDATE SET snapshot_id=excluded.snapshot_id,at=excluded.at", args)?;
        Ok(())
    }
    /// The last version of this project the collaborator confirmed having, if any.
    pub fn delivered(&self, project: Uuid, peer: &str) -> Result<Option<Uuid>> {
        let value: Option<String> = self
            .lock()?
            .query_row(
                "SELECT snapshot_id FROM deliveries WHERE project_id=?1 AND peer_id=?2",
                params![project.to_string(), peer],
                |row| row.get(0),
            )
            .optional()?;
        value
            .map(|id| Uuid::parse_str(&id).map_err(Into::into))
            .transpose()
    }
    pub fn lineage(&self, project: Uuid, local: Option<Uuid>, remote: Uuid) -> Result<Lineage> {
        let Some(local) = local else {
            return Ok(Lineage::RemoteAhead);
        };
        if local == remote {
            return Ok(Lineage::Equal);
        }
        let history = self.history(project)?;
        let parents: std::collections::HashMap<_, _> = history
            .into_iter()
            .map(|s| (s.snapshot_id, s.parent_snapshot_id))
            .collect();
        fn walk(
            parents: &std::collections::HashMap<Uuid, Option<Uuid>>,
            mut id: Uuid,
            target: Uuid,
        ) -> (bool, bool) {
            let mut seen = std::collections::HashSet::new();
            while seen.insert(id) {
                if id == target {
                    return (true, true);
                }
                match parents.get(&id) {
                    Some(Some(parent)) => id = *parent,
                    Some(None) => return (false, true),
                    None => return (false, false),
                }
            }
            (false, false)
        }
        let (remote_descends, remote_complete) = walk(&parents, remote, local);
        if remote_descends {
            return Ok(Lineage::RemoteAhead);
        }
        let (local_descends, local_complete) = walk(&parents, local, remote);
        if local_descends {
            return Ok(Lineage::LocalAhead);
        }
        Ok(if remote_complete && local_complete {
            Lineage::Diverged
        } else {
            Lineage::Unknown
        })
    }
    pub fn local_snapshot(&self, project: Uuid, id: Uuid) -> Result<bool> {
        Ok(self.lock()?.query_row("SELECT EXISTS(SELECT 1 FROM snapshots WHERE project_id=?1 AND snapshot_id=?2 AND location='LOCAL')",params![project.to_string(),id.to_string()],|r|r.get(0))?)
    }
    pub fn authorized_chunk_size(
        &self,
        project: Uuid,
        id: Uuid,
        hash: &str,
    ) -> Result<Option<u32>> {
        Ok(self.lock()?.query_row("SELECT c.size FROM snapshot_chunks c JOIN snapshots s ON s.snapshot_id=c.snapshot_id WHERE s.project_id=?1 AND s.snapshot_id=?2 AND s.location='LOCAL' AND c.blake3=?3",params![project.to_string(),id.to_string(),hash],|r|r.get(0)).optional()?)
    }
}
fn insert_files(tx: &Transaction<'_>, s: &SignedSnapshot) -> Result<()> {
    let id = s.manifest.snapshot_id.to_string();
    let verified_at = Utc::now().to_rfc3339();
    let mut files = tx.prepare_cached("INSERT OR IGNORE INTO snapshot_files(snapshot_id,relative_path,size,blake3,chunks_json) VALUES(?1,?2,?3,?4,?5)")?;
    let mut snapshot_chunks = tx.prepare_cached(
        "INSERT OR IGNORE INTO snapshot_chunks(snapshot_id,blake3,size) VALUES(?1,?2,?3)",
    )?;
    let mut chunks = tx.prepare_cached("INSERT INTO chunks(blake3,size,verified_at) VALUES(?1,?2,?3) ON CONFLICT(blake3) DO UPDATE SET verified_at=excluded.verified_at WHERE chunks.size=excluded.size")?;
    let mut seen = std::collections::HashSet::new();
    for file in &s.manifest.files {
        files.execute(params![
            id,
            file.relative_path,
            file.size,
            file.blake3,
            serde_json::to_string(&file.chunks)?
        ])?;
        for chunk in &file.chunks {
            if seen.insert(&chunk.blake3) {
                snapshot_chunks.execute(params![id, chunk.blake3, chunk.size])?;
                ensure!(
                    chunks.execute(params![chunk.blake3, chunk.size, verified_at])? == 1,
                    "chunk catalog has a conflicting size"
                );
            }
        }
    }
    Ok(())
}
