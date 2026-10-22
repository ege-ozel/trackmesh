//! Compact adapter status. Signed manifests and complete history stay outside polling.
use crate::{Engine, GrantProblem};
use anyhow::Result;
use protocol::DeviceCard;
use serde::Serialize;
use storage::{Lineage, Project, SnapshotInfo, TransferInfo};

#[derive(Serialize)]
pub struct PeerSummary {
    pub device: DeviceCard,
    /// Last-known authored metadata, not a claim about the peer's current head.
    pub announcement: Option<SnapshotInfo>,
    /// Bounded newest-first history for adapter presentation.
    pub history: Vec<SnapshotInfo>,
    /// Conservative relationship from stored ancestry; never a live equality claim.
    pub known_lineage: Option<Lineage>,
    pub offline_head: Option<OfflineHead>,
    /// Whether this Engine has fetched a head from this peer during its lifetime, including an
    /// answer that the peer has no version.
    pub head_fetched: bool,
    /// The peer's head from the last fetch; its author may be another collaborator.
    pub fetched_head: Option<uuid::Uuid>,
    /// Stored details of `fetched_head`, for showing the collaborator's current version.
    pub head: Option<SnapshotInfo>,
    /// The last version of this PC's the collaborator confirmed having.
    pub delivered_snapshot: Option<uuid::Uuid>,
}

#[derive(Serialize)]
pub struct OfflineHead {
    pub snapshot_id: uuid::Uuid,
    pub created_at: String,
    pub total_size: u64,
    pub file_count: u32,
}

#[derive(Serialize)]
pub struct ProjectSummary {
    pub project: Project,
    pub root_available: bool,
    pub root_empty: bool,
    pub active_transfer: bool,
    pub local_snapshot: Option<SnapshotInfo>,
    pub peers: Vec<PeerSummary>,
    pub grant_problem: Option<GrantProblem>,
    pub revoke_problem: bool,
    pub missing_functions: Vec<String>,
    pub transfer: Option<TransferInfo>,
}

#[derive(Serialize)]
pub struct SummaryPage {
    pub projects: Vec<ProjectSummary>,
    pub offset: u32,
    pub has_more: bool,
}

impl Engine {
    pub fn project_summary(&self, project: uuid::Uuid) -> Result<ProjectSummary> {
        self.summarize_project(self.store.project(project)?)
    }

    fn summarize_project(&self, project: Project) -> Result<ProjectSummary> {
        let root_empty = snapshot::is_empty(std::path::Path::new(&project.root)).unwrap_or(false);
        let active_transfer = self.store.has_active_transfer(project.project_id)?;
        let fetched_heads = self
            .peer_heads
            .lock()
            .map_err(|_| anyhow::anyhow!("peer head lock poisoned"))?;
        let peers = self
            .store
            .peers(Some(project.project_id))?
            .into_iter()
            .map(|device| {
                let history = self
                    .store
                    .author_history(project.project_id, device.device_id)?;
                let announcement = history.first().cloned();
                let offline_head = self
                    .offline_heads
                    .lock()
                    .map_err(|_| anyhow::anyhow!("offline head lock poisoned"))?
                    .get(&(project.project_id, device.device_id))
                    .map(|(_, head)| head)
                    .filter(|head| {
                        Some(head.snapshot_id) != project.latest_local_snapshot
                            && !history
                                .iter()
                                .any(|known| known.snapshot_id == head.snapshot_id)
                            && self
                                .store
                                .snapshot(head.snapshot_id)
                                .ok()
                                .flatten()
                                .is_none()
                    })
                    .map(|head| OfflineHead {
                        snapshot_id: head.snapshot_id,
                        created_at: head.created_at.to_rfc3339(),
                        total_size: head.total_size,
                        file_count: head.file_count,
                    });
                Ok(PeerSummary {
                    delivered_snapshot: self
                        .store
                        .delivered(project.project_id, &device.peer_id)?,
                    head_fetched: fetched_heads
                        .contains_key(&(project.project_id, device.device_id)),
                    fetched_head: fetched_heads
                        .get(&(project.project_id, device.device_id))
                        .copied()
                        .flatten(),
                    head: match fetched_heads.get(&(project.project_id, device.device_id)) {
                        Some(&Some(head)) => self.store.snapshot_info(head)?,
                        _ => None,
                    },
                    // A fetched empty head means the collaborator has no version yet.
                    known_lineage: match fetched_heads.get(&(project.project_id, device.device_id))
                    {
                        Some(&head) => head,
                        None => announcement.as_ref().map(|snapshot| snapshot.snapshot_id),
                    }
                    .map(|head| {
                        self.store.known_lineage(
                            project.project_id,
                            project.latest_local_snapshot,
                            head,
                        )
                    })
                    .transpose()?,
                    announcement,
                    history,
                    offline_head,
                    device,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(ProjectSummary {
            root_available: std::fs::metadata(&project.root)
                .is_ok_and(|metadata| metadata.is_dir()),
            root_empty,
            active_transfer,
            local_snapshot: project
                .latest_local_snapshot
                .map(|id| self.store.snapshot_info(id))
                .transpose()?
                .flatten(),
            grant_problem: self.grant_problem(project.project_id)?,
            revoke_problem: self.revoke_problem(project.project_id)?,
            missing_functions: self.missing_supabase_functions(project.project_id)?,
            transfer: self.store.latest_transfer(project.project_id)?,
            project,
            peers,
        })
    }
    pub fn summary_page(&self, offset: u32) -> Result<SummaryPage> {
        let mut projects = self.store.projects_page(offset, 33)?;
        let has_more = projects.len() > 32;
        projects.truncate(32);
        let projects = projects
            .into_iter()
            .map(|project| self.summarize_project(project))
            .collect::<Result<Vec<_>>>()?;
        Ok(SummaryPage {
            projects,
            offset,
            has_more,
        })
    }
}
