//! Durable publication and verified metadata ingestion. No file transfer or head updates.
use crate::Engine;
use anyhow::{ensure, Context, Result};
use mailbox::MailboxClient;
use protocol::Envelope;
use serde::Serialize;
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

#[derive(Serialize)]
pub struct PublicationStatus {
    pub project_id: Uuid,
    pub published: usize,
    pub pending: u64,
}

#[derive(Serialize)]
pub struct RefreshStatus {
    pub project_id: Uuid,
    pub records: usize,
    pub cursor: i64,
    pub more: bool,
}

impl Engine {
    /// Legacy mailbox metadata is independent of Supabase endpoint/head hints.
    pub fn mailbox_enabled(&self, project: Uuid) -> Result<bool> {
        Ok(
            self.project_backend_config(project)?.backend != crate::ProjectBackend::Supabase
                && self.store.project(project)?.mailbox_url.is_some(),
        )
    }
    fn metadata_gate(&self, project: Uuid) -> Result<Arc<tokio::sync::Mutex<()>>> {
        let mut jobs = self
            .metadata_jobs
            .lock()
            .map_err(|_| anyhow::anyhow!("metadata job lock poisoned"))?;
        Ok(jobs.entry(project).or_default().clone())
    }
    fn mailbox_client(&self, project: Uuid, allow_loopback_http: bool) -> Result<MailboxClient> {
        ensure!(
            self.mailbox_enabled(project)?,
            "legacy mailbox is disabled for this project"
        );
        let project = self.store.project(project)?;
        let url = project.mailbox_url.context("project has no mailbox URL")?;
        MailboxClient::new(&url, project.project_id, allow_loopback_http)
    }

    fn prepare_publication(&self, project: Uuid, id: Uuid) -> Result<Envelope> {
        let signed = self
            .store
            .snapshot(id)?
            .context("pending snapshot has no manifest")?;
        crypto::verify_snapshot(&signed)?;
        ensure!(
            signed.manifest.project_id == project
                && signed.manifest.author_device_id == self.identity.device_id
                && signed.author_public_key == self.identity.public_key(),
            "cannot publish another device's snapshot"
        );
        let key = crypto::project_key(&self.directory.join("secrets/projects"), project, None)?;
        if self.store.publication_envelope(id)?.is_none() {
            let envelope = self.identity.seal(&signed, &key)?;
            self.store.save_envelope(&envelope)?;
        }
        // Read the winning persisted envelope if another process prepared it concurrently.
        let envelope = self
            .store
            .publication_envelope(id)?
            .context("publication envelope was not persisted")?;
        let announcement = crypto::open(&envelope, &key)?;
        ensure!(
            envelope.project_id == project
                && envelope.snapshot_id == id
                && envelope.author_device_id == self.identity.device_id
                && envelope.public_key == signed.author_public_key
                && envelope.created_at == signed.manifest.created_at
                && announcement.manifest_hash == signed.manifest_hash,
            "durable envelope does not match authored snapshot"
        );
        Ok(envelope)
    }

    pub async fn publish_metadata(
        self: &Arc<Self>,
        project: Uuid,
        allow_loopback_http: bool,
    ) -> Result<PublicationStatus> {
        let _job = self.metadata_gate(project)?.lock_owned().await;
        let engine = self.clone();
        let (client, ids) = tokio::task::spawn_blocking(move || -> Result<_> {
            Ok((
                engine.mailbox_client(project, allow_loopback_http)?,
                engine
                    .store
                    .pending_publication_ids(project, engine.identity.device_id)?,
            ))
        })
        .await??;
        let mut published = 0;
        for id in ids {
            let engine = self.clone();
            let envelope =
                tokio::task::spawn_blocking(move || engine.prepare_publication(project, id))
                    .await??;
            let ack = client.publish(&self.identity, &envelope).await?;
            let engine = self.clone();
            tokio::task::spawn_blocking(move || engine.store.publication_ack(&envelope, &ack))
                .await??;
            published += 1;
        }
        let engine = self.clone();
        let pending = tokio::task::spawn_blocking(move || {
            engine
                .store
                .pending_publication_count(project, engine.identity.device_id)
        })
        .await??;
        Ok(PublicationStatus {
            project_id: project,
            published,
            pending,
        })
    }

    pub async fn refresh_metadata(
        self: &Arc<Self>,
        project: Uuid,
        allow_loopback_http: bool,
    ) -> Result<RefreshStatus> {
        let _job = self.metadata_gate(project)?.lock_owned().await;
        let engine = self.clone();
        let (client, mut cursor) = tokio::task::spawn_blocking(move || -> Result<_> {
            Ok((
                engine.mailbox_client(project, allow_loopback_http)?,
                engine.store.cursor(project)?,
            ))
        })
        .await??;
        let mut records = 0;
        let mut more = false;
        for _ in 0..8 {
            let page = client.page(&self.identity, project, cursor).await?;
            more = page.records.len() == protocol::MAX_MAILBOX_PAGE_RECORDS;
            let next = page.next_cursor;
            let engine = self.clone();
            records += tokio::task::spawn_blocking(move || -> Result<usize> {
                let key =
                    crypto::project_key(&engine.directory.join("secrets/projects"), project, None)?;
                let mut verified = Vec::new();
                for record in page.records {
                    let envelope = record.envelope;
                    ensure!(
                        !envelope.snapshot_id.is_nil(),
                        "invalid announced snapshot ID"
                    );
                    let own = envelope.author_device_id == engine.identity.device_id
                        && envelope.public_key == engine.identity.public_key();
                    ensure!(
                        own || engine.store.trusted_author(
                            project,
                            envelope.author_device_id,
                            &envelope.public_key
                        )?,
                        "mailbox author is not paired with this project"
                    );
                    let announcement = crypto::open(&envelope, &key)?;
                    verified.push((record.sequence, envelope, announcement));
                }
                engine.store.ingest_mailbox_page(
                    project,
                    engine.identity.device_id,
                    cursor,
                    &verified,
                    next,
                )?;
                Ok(verified.len())
            })
            .await??;
            cursor = next;
            if !more {
                break;
            }
        }
        Ok(RefreshStatus {
            project_id: project,
            records,
            cursor,
            more,
        })
    }
}

/// Retries metadata only while an explicitly started adapter session remains active.
pub struct MailboxRuntime {
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<Result<()>>,
}

impl MailboxRuntime {
    pub fn start(engine: Arc<Engine>, allow_loopback_http: bool) -> Self {
        let (stop, mut stopped) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            // The first interval tick is immediate, so configured mailboxes get a startup pass.
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! { _ = &mut stopped => break, _ = interval.tick() => {} }
                let disk = engine.clone();
                tokio::task::spawn_blocking(move || disk.maintain_storage_if_idle()).await?;
                let engine_for_disk = engine.clone();
                let projects =
                    tokio::task::spawn_blocking(move || engine_for_disk.store.projects()).await??;
                for project in projects {
                    // One unreadable project config must not stop the other projects' passes.
                    if !engine.mailbox_enabled(project.project_id).unwrap_or(false) {
                        continue;
                    }
                    let operation = async {
                        if engine
                            .publish_metadata(project.project_id, allow_loopback_http)
                            .await
                            .is_err()
                        {
                            tracing::warn!(project_id=%project.project_id, "Mailbox publication failed; pending envelopes will retry");
                        }
                        if engine
                            .refresh_metadata(project.project_id, allow_loopback_http)
                            .await
                            .is_err()
                        {
                            tracing::warn!(project_id=%project.project_id, "Mailbox refresh failed; current page cursor retained");
                        }
                    };
                    tokio::select! { _ = &mut stopped => return Ok(()), _ = operation => {} }
                }
            }
            Ok(())
        });
        Self { stop, task }
    }

    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }

    pub async fn stop(self) -> Result<()> {
        let _ = self.stop.send(());
        let mut task = self.task;
        match tokio::time::timeout(Duration::from_secs(20), &mut task).await {
            Ok(result) => result??,
            Err(_) => {
                task.abort();
                let _ = tokio::time::timeout(Duration::from_secs(1), &mut task).await;
                anyhow::bail!("Mailbox runtime did not stop within 20 seconds");
            }
        }
        Ok(())
    }
}
