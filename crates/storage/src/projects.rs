//! Project-folder links and local project status.
use crate::{
    models::{project_row, PROJECT_COLUMNS},
    Project, Store,
};
use anyhow::{ensure, Context, Result};
use rusqlite::{params, OptionalExtension};
use std::path::Path;
use uuid::Uuid;

impl Store {
    pub fn projects_page(&self, offset: u32, limit: u32) -> Result<Vec<Project>> {
        ensure!(
            (1..=33).contains(&limit),
            "project page must contain at most 33 records"
        );
        let db = self.lock()?;
        let mut statement = db.prepare(&format!(
            "SELECT {PROJECT_COLUMNS} FROM projects WHERE removed=0 ORDER BY name,project_id LIMIT ?1 OFFSET ?2"
        ))?;
        let rows = statement
            .query_map(params![limit, offset], project_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
    /// Listed projects; removed ones are left out everywhere except `projects_with_removed`.
    pub fn projects(&self) -> Result<Vec<Project>> {
        self.projects_where("removed=0")
    }
    /// Listed and removed projects, for the invite-service loop that still owes removals.
    pub fn projects_with_removed(&self) -> Result<Vec<Project>> {
        self.projects_where("1=1")
    }
    /// Removed projects whose data was kept, so they can be restored.
    pub fn removed_projects(&self) -> Result<Vec<Project>> {
        self.projects_where("removed=1")
    }
    /// Removed projects whose data was deleted; their rows wait for queued removals and the key file.
    pub fn deleted_projects(&self) -> Result<Vec<Project>> {
        self.projects_where("removed=2")
    }
    fn projects_where(&self, filter: &str) -> Result<Vec<Project>> {
        let db = self.lock()?;
        let mut statement = db.prepare(&format!(
            "SELECT {PROJECT_COLUMNS} FROM projects WHERE {filter} ORDER BY name"
        ))?;
        let rows = statement
            .query_map([], project_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
    /// Takes a project off the list. With `delete_history` its versions, transfers and collaborator
    /// links on this PC are deleted too; the folder and its files are never touched.
    pub fn remove_project(&self, id: Uuid, delete_history: bool) -> Result<()> {
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        let id = id.to_string();
        if delete_history {
            tx.execute_batch("CREATE TEMP TABLE IF NOT EXISTS gone(id TEXT); DELETE FROM gone;")?;
            tx.execute(
                "INSERT INTO gone SELECT snapshot_id FROM snapshots WHERE project_id=?1",
                [&id],
            )?;
            tx.execute("DELETE FROM transfer_chunks WHERE transfer_id IN (SELECT transfer_id FROM transfer_sessions WHERE project_id=?1)", [&id])?;
            tx.execute("DELETE FROM transfer_sessions WHERE project_id=?1", [&id])?;
            tx.execute_batch("DELETE FROM snapshot_chunks WHERE snapshot_id IN (SELECT id FROM gone); DELETE FROM snapshot_files WHERE snapshot_id IN (SELECT id FROM gone);")?;
            tx.execute("DELETE FROM snapshots WHERE project_id=?1", [&id])?;
            tx.execute("DELETE FROM project_peers WHERE project_id=?1", [&id])?;
            tx.execute("DELETE FROM mailbox_state WHERE project_id=?1", [&id])?;
            tx.execute("DELETE FROM deliveries WHERE project_id=?1", [&id])?;
        }
        let changed = tx.execute(
            "UPDATE projects SET removed=?2, latest_local_snapshot=CASE WHEN ?2=2 THEN NULL ELSE latest_local_snapshot END WHERE project_id=?1 AND removed=0",
            params![id, if delete_history { 2 } else { 1 }],
        )?;
        ensure!(changed == 1, "project is not on the list");
        tx.commit()?;
        Ok(())
    }
    /// Whether the project is on the list (linked and not removed).
    pub fn listed(&self, id: Uuid) -> Result<bool> {
        Ok(self
            .lock()?
            .query_row(
                "SELECT removed=0 FROM projects WHERE project_id=?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(false))
    }
    /// Folder of a project this PC has a row for, listed or removed.
    pub fn known_root(&self, id: Uuid) -> Result<Option<String>> {
        Ok(self
            .lock()?
            .query_row(
                "SELECT root FROM projects WHERE project_id=?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?)
    }
    /// Puts a removed project back on the list when its creator invites this PC again.
    pub fn rejoin_project(&self, id: Uuid) -> Result<()> {
        self.lock()?.execute(
            "UPDATE projects SET removed=0 WHERE project_id=?1",
            [id.to_string()],
        )?;
        Ok(())
    }
    pub fn restore_project(&self, id: Uuid) -> Result<()> {
        let changed = self.lock()?.execute(
            "UPDATE projects SET removed=0 WHERE project_id=?1 AND removed=1",
            [id.to_string()],
        )?;
        ensure!(changed == 1, "project cannot be restored");
        Ok(())
    }
    /// Drops the last row of a project whose data was deleted.
    pub fn forget_project(&self, id: Uuid) -> Result<()> {
        self.lock()?.execute(
            "DELETE FROM projects WHERE project_id=?1 AND removed=2",
            [id.to_string()],
        )?;
        Ok(())
    }
    pub fn project(&self, id: Uuid) -> Result<Project> {
        self.lock()?
            .query_row(
                &format!("SELECT {PROJECT_COLUMNS} FROM projects WHERE project_id=?1"),
                [id.to_string()],
                project_row,
            )
            .optional()?
            .context("project UUID is not linked to a folder")
    }
    pub fn link(&self, id: Uuid, name: &str, root: &Path) -> Result<()> {
        ensure!(
            !name.trim().is_empty() && name.len() <= 320,
            "invalid project name"
        );
        let root = root.to_str().context("project folder must be Unicode")?;
        let db = self.lock()?;
        let existing: Option<String> = db
            .query_row(
                "SELECT root FROM projects WHERE project_id=?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            ensure!(existing == root, "UUID already linked to another folder");
        }
        // A folder removed with its data kept comes back through Restore; one whose data was deleted is free again.
        let removed: Option<i64> = db
            .query_row(
                "SELECT removed FROM projects WHERE root=?1 AND project_id<>?2 AND removed<>0",
                params![root, id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        ensure!(
            removed != Some(1),
            "This folder was removed from TrackMesh. Bring it back in Settings, Removed projects."
        );
        if removed == Some(2) {
            db.execute("DELETE FROM projects WHERE root=?1 AND removed=2", [root])?;
        }
        // The old mailbox_url and relay_address columns stay in the schema unread; mailbox_url is cleared here.
        db.execute("INSERT INTO projects(project_id,name,root) VALUES(?1,?2,?3) ON CONFLICT(project_id) DO UPDATE SET name=excluded.name,mailbox_url=NULL", params![id.to_string(),name,root])?;
        Ok(())
    }
    pub fn update_root(&self, id: Uuid, root: &Path) -> Result<()> {
        let root = root.to_str().context("project folder must be Unicode")?;
        let db = self.lock()?;
        let conflicts: i64 = db.query_row(
            "SELECT COUNT(*) FROM projects WHERE project_id<>?1 AND root=?2",
            params![id.to_string(), root],
            |row| row.get(0),
        )?;
        ensure!(
            conflicts == 0,
            "folder is already linked to another project"
        );
        let changed = db.execute(
            "UPDATE projects SET root=?2 WHERE project_id=?1",
            params![id.to_string(), root],
        )?;
        ensure!(changed == 1, "project UUID is not linked");
        Ok(())
    }
    pub fn set_project_error(&self, id: Uuid, error: Option<&str>) -> Result<()> {
        self.lock()?.execute(
            "UPDATE projects SET error=?2 WHERE project_id=?1",
            params![id.to_string(), error],
        )?;
        Ok(())
    }
    pub fn mark_recovery(&self, id: Uuid, required: bool) -> Result<()> {
        self.lock()?.execute(
            "UPDATE projects SET recovery_required=?2 WHERE project_id=?1",
            params![id.to_string(), required],
        )?;
        Ok(())
    }
}
