//! Bounded project jobs; save observations survive client disconnect/rebind.
use super::{status::Notice, Shared};
use anyhow::{ensure, Result};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use tokio::{
    task::JoinHandle,
    time::{sleep, Duration, Instant},
};
use uuid::Uuid;

pub(super) struct Job {
    pub task: JoinHandle<()>,
    revision: Arc<AtomicU64>,
}

impl Shared {
    /// Accepted settings writes also outlive disconnect and drain before owner exit.
    pub(super) fn schedule_setting(
        &self,
        project: Uuid,
        enabled: bool,
    ) -> Result<tokio::sync::oneshot::Receiver<Result<()>>> {
        let mut state = self.lock()?;
        ensure!(
            !self.closing.load(Ordering::Acquire),
            "Plugin bridge is closing"
        );
        state.setting_tasks.retain(|task| !task.is_finished());
        ensure!(
            state.setting_tasks.len() < 16,
            "Plugin settings are busy; retry later"
        );
        let engine = self.engine.clone();
        let (sender, receiver) = tokio::sync::oneshot::channel();
        state.setting_tasks.push(tokio::spawn(async move {
            let result =
                tokio::task::spawn_blocking(move || engine.set_auto_share(project, enabled))
                    .await
                    .map_err(anyhow::Error::from)
                    .and_then(|result| result);
            let _ = sender.send(result);
        }));
        Ok(receiver)
    }
    pub(super) fn schedule(self: &Arc<Self>, project: Uuid, observed: bool) -> Result<()> {
        let mut state = self.lock()?;
        ensure!(
            !self.closing.load(Ordering::Acquire),
            "Plugin bridge is closing"
        );
        if let Some(job) = state.jobs.get(&project) {
            ensure!(observed, "Snapshot work is already active; retry later");
            job.revision.fetch_add(1, Ordering::AcqRel);
            return Ok(());
        }
        ensure!(
            state.jobs.len() < 8,
            "Snapshot job limit reached; retry later"
        );
        ensure!(
            state.messages.contains_key(&project) || state.messages.len() < 64,
            "Plugin project limit reached; restart the session"
        );
        state.messages.insert(
            project,
            if observed {
                Notice::Saving
            } else {
                Notice::Queued
            },
        );
        let revision = Arc::new(AtomicU64::new(0));
        let worker_revision = revision.clone();
        let shared = self.clone();
        let task = tokio::spawn(run(shared, project, observed, worker_revision));
        state.jobs.insert(project, Job { task, revision });
        Ok(())
    }
}

async fn debounce(revision: &AtomicU64) -> Result<u64> {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let current = revision.load(Ordering::Acquire);
        sleep(Duration::from_secs(2)).await;
        ensure!(
            Instant::now() < deadline,
            "save observations did not settle"
        );
        if revision.load(Ordering::Acquire) == current {
            return Ok(current);
        }
    }
}

async fn run(shared: Arc<Shared>, project: Uuid, mut observed: bool, revision: Arc<AtomicU64>) {
    loop {
        let baseline = if observed {
            debounce(&revision).await
        } else {
            Ok(revision.load(Ordering::Acquire))
        };
        let handled = baseline.as_ref().ok().copied();
        let result = match baseline {
            Ok(_) if observed && !shared.engine.auto_share_enabled(project).unwrap_or(false) => {
                Ok(None)
            }
            Ok(_) => shared
                .engine
                .send_saved_snapshot(&shared.network, project)
                .await
                .map(Some),
            Err(error) => Err(error),
        };
        let notice = match result {
            Ok(Some(_)) => Notice::Committed,
            Ok(None) => Notice::Saved,
            Err(_) => Notice::Failed,
        };
        // Scheduling and removal share this lock: an observation cannot fall
        // between the final revision check and removal of a finishing job.
        let Ok(mut state) = shared.lock() else {
            return;
        };
        if handled.is_none() || handled.is_some_and(|v| revision.load(Ordering::Acquire) == v) {
            state.messages.insert(project, notice);
            state.jobs.remove(&project);
            return;
        }
        state.messages.insert(project, Notice::Saving);
        observed = true;
    }
}
