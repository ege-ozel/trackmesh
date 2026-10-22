//! Device identity bindings and explicit per-project authorization.
use crate::Store;
use anyhow::{ensure, Result};
use protocol::DeviceCard;
use rusqlite::{params, OptionalExtension};
use uuid::Uuid;

impl Store {
    pub fn add_device(&self, id: Uuid, name: &str, peer_id: &str, key: &str) -> Result<()> {
        self.lock()?.execute("INSERT INTO devices(device_id,name,peer_id,public_key) VALUES(?1,?2,?3,?4) ON CONFLICT(device_id) DO UPDATE SET name=excluded.name WHERE devices.peer_id=excluded.peer_id AND devices.public_key=excluded.public_key", params![id.to_string(),name,peer_id,key])?;
        Ok(())
    }
    pub fn trust(&self, project: Uuid, card: &DeviceCard) -> Result<()> {
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        let bound: Option<(String, String)> = tx
            .query_row(
                "SELECT peer_id,public_key FROM devices WHERE device_id=?1",
                [card.device_id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((peer, key)) = bound {
            ensure!(
                peer == card.peer_id && key == card.public_key,
                "device identity cannot be replaced"
            );
        }
        let count: u32 = tx.query_row(
            "SELECT COUNT(*) FROM project_peers WHERE project_id=?1 AND device_id<>?2",
            params![project.to_string(), card.device_id.to_string()],
            |r| r.get(0),
        )?;
        ensure!(count < 2, "MVP supports at most three devices per project");
        tx.execute("INSERT INTO devices(device_id,name,peer_id,public_key) VALUES(?1,?2,?3,?4) ON CONFLICT(device_id) DO UPDATE SET name=excluded.name", params![card.device_id.to_string(),card.name,card.peer_id,card.public_key])?;
        tx.execute("INSERT INTO trusted_peers(device_id,card_json,trust_status) VALUES(?1,?2,'TRUSTED') ON CONFLICT(device_id) DO UPDATE SET card_json=excluded.card_json,trust_status='TRUSTED'", params![card.device_id.to_string(),serde_json::to_string(card)?])?;
        tx.execute(
            "INSERT OR IGNORE INTO project_peers(project_id,device_id) VALUES(?1,?2)",
            params![project.to_string(), card.device_id.to_string()],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn peers(&self, project: Option<Uuid>) -> Result<Vec<DeviceCard>> {
        let db = self.lock()?;
        let mut stmt = db.prepare("SELECT DISTINCT t.card_json FROM trusted_peers t JOIN project_peers p ON p.device_id=t.device_id WHERE t.trust_status='TRUSTED' AND (?1 IS NULL OR p.project_id=?1)")?;
        let json = stmt
            .query_map([project.map(|p| p.to_string())], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        json.into_iter()
            .map(|s| Ok(serde_json::from_str(&s)?))
            .collect()
    }
    pub fn authorized(&self, project: Uuid, peer_id: &str) -> Result<Option<DeviceCard>> {
        Ok(self
            .peers(Some(project))?
            .into_iter()
            .find(|card| card.peer_id == peer_id))
    }
    /// A paired collaborator, or an author recorded by `vouch_author` that this PC never paired
    /// with. A device bound here (paired once, possibly removed since) counts only while paired.
    pub fn trusted_author(&self, project: Uuid, device: Uuid, key: &str) -> Result<bool> {
        if self
            .peers(Some(project))?
            .iter()
            .any(|c| c.device_id == device && c.public_key == key)
        {
            return Ok(true);
        }
        Ok(self.lock()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM snapshot_authors a WHERE a.project_id=?1 AND a.device_id=?2 AND a.public_key=?3 AND NOT EXISTS(SELECT 1 FROM devices d WHERE d.device_id=a.device_id))",
            params![project.to_string(), device.to_string(), key],
            |r| r.get(0),
        )?)
    }
    /// Records the author of a version a paired peer served when this PC never paired with that
    /// device (removed before this PC joined, or a third collaborator). The caller has verified the
    /// signature. Returns false for a device bound here: removing a collaborator stays final.
    pub fn vouch_author(&self, project: Uuid, device: Uuid, key: &str) -> Result<bool> {
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        let bound: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM devices WHERE device_id=?1)",
            [device.to_string()],
            |r| r.get(0),
        )?;
        if bound {
            return Ok(false);
        }
        tx.execute(
            "INSERT OR IGNORE INTO snapshot_authors(project_id,device_id,public_key) VALUES(?1,?2,?3)",
            params![project.to_string(), device.to_string(), key],
        )?;
        let stored: String = tx.query_row(
            "SELECT public_key FROM snapshot_authors WHERE project_id=?1 AND device_id=?2",
            params![project.to_string(), device.to_string()],
            |r| r.get(0),
        )?;
        ensure!(stored == key, "device identity cannot be replaced");
        tx.commit()?;
        Ok(true)
    }
    pub fn revoke(&self, project: Uuid, device: Uuid) -> Result<()> {
        self.lock()?.execute(
            "DELETE FROM project_peers WHERE project_id=?1 AND device_id=?2",
            params![project.to_string(), device.to_string()],
        )?;
        Ok(())
    }
}
