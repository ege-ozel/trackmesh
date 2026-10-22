//! Bounded HTTPS transport for encrypted announcements. Core owns project decisions.
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use crypto::Identity;
use protocol::{
    mailbox_scope, Envelope, MailboxAck, MailboxPage, MAX_MAILBOX_AUTH_BYTES,
    MAX_MAILBOX_ENVELOPE_BYTES, MAX_MAILBOX_PAGE_BYTES, MAX_MAILBOX_PAGE_RECORDS,
};
use reqwest::{redirect, Client, Response, Url};
use serde::de::DeserializeOwned;
use std::{net::IpAddr, time::Duration};
use uuid::Uuid;

pub struct MailboxClient {
    client: Client,
    endpoint: Url,
    project: Uuid,
}

fn parse_url(base: &str, allow_loopback_http: bool) -> Result<Url> {
    let url = Url::parse(base).context("invalid mailbox URL")?;
    ensure!(
        url.username().is_empty() && url.password().is_none(),
        "mailbox URL cannot contain credentials"
    );
    ensure!(
        url.query().is_none() && url.fragment().is_none(),
        "mailbox URL cannot contain a query or fragment"
    );
    let loopback = url.host_str().is_some_and(|host| {
        host.trim_matches(['[', ']'])
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
    });
    ensure!(url.scheme() == "https" || (allow_loopback_http && url.scheme() == "http" && loopback), "mailbox requires HTTPS; development HTTP requires literal loopback and explicit permission");
    ensure!(url.host_str().is_some(), "mailbox host missing");
    Ok(url)
}

/// Validate stored configuration. Plain HTTP is useful only on literal loopback;
/// requests still require the explicit development switch.
pub fn validate_mailbox_url(base: &str) -> Result<()> {
    parse_url(base, true)?;
    Ok(())
}

/// Supabase project URLs use the hosted HTTPS origin; custom domains require a
/// separately configured origin and are intentionally not accepted in M5d.
pub fn validate_supabase_project_url(base: &str) -> Result<()> {
    let url = Url::parse(base).context("invalid Supabase project URL")?;
    let host = url.host_str().unwrap_or_default();
    ensure!(
        url.scheme() == "https"
            && host.ends_with(".supabase.co")
            && host.len() > ".supabase.co".len()
            && url.username().is_empty()
            && url.password().is_none()
            && url.port_or_known_default() == Some(443)
            && (url.path().is_empty() || url.path() == "/")
            && url.query().is_none()
            && url.fragment().is_none(),
        "Supabase project URL must be an HTTPS *.supabase.co origin"
    );
    Ok(())
}

/// Return the parsed, normalized host of a hosted Supabase project URL.
pub fn supabase_project_host(base: &str) -> Result<String> {
    validate_supabase_project_url(base)?;
    let url = Url::parse(base).context("invalid Supabase project URL")?;
    Ok(url.host_str().unwrap_or_default().to_ascii_lowercase())
}

impl MailboxClient {
    pub fn new(base: &str, project: Uuid, allow_loopback_http: bool) -> Result<Self> {
        let mut url = parse_url(base, allow_loopback_http)?;
        let prefix = url.path().trim_end_matches('/');
        url.set_path(&format!("{prefix}/v1/projects/{project}/announcements"));
        // All TLS dependencies use ring; do not disable certificate/hostname validation.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = Client::builder()
            .redirect(redirect::Policy::none())
            .no_proxy()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(20))
            .user_agent("TrackMesh/1")
            .build()?;
        Ok(Self {
            client,
            endpoint: url,
            project,
        })
    }

    pub async fn publish(&self, identity: &Identity, envelope: &Envelope) -> Result<MailboxAck> {
        crypto::verify_envelope(envelope)?;
        ensure!(
            envelope.project_id == self.project,
            "publication project does not match endpoint"
        );
        ensure!(
            envelope.author_device_id == identity.device_id
                && envelope.public_key == identity.public_key(),
            "cannot publish another device's envelope"
        );
        let body = serde_json::to_vec(envelope)?;
        ensure!(
            body.len() <= MAX_MAILBOX_ENVELOPE_BYTES,
            "announcement envelope too large"
        );
        let auth = identity.mailbox_auth(envelope.project_id, mailbox_scope(None), &body)?;
        let response = self
            .client
            .post(self.endpoint.clone())
            .header("x-trackmesh-auth", auth_header(&auth)?)
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await
            .context("mailbox publish request failed")?;
        let ack: MailboxAck = bounded_json(response, MAX_MAILBOX_AUTH_BYTES).await?;
        ensure!(
            ack.snapshot_id == envelope.snapshot_id && ack.sequence > 0,
            "mailbox ACK does not match snapshot"
        );
        Ok(ack)
    }

    pub async fn page(
        &self,
        identity: &Identity,
        project: Uuid,
        cursor: i64,
    ) -> Result<MailboxPage> {
        ensure!(
            cursor >= 0 && project == self.project,
            "invalid mailbox cursor or endpoint project"
        );
        let auth = identity.mailbox_auth(project, mailbox_scope(Some(cursor)), &[])?;
        let mut url = self.endpoint.clone();
        url.query_pairs_mut()
            .append_pair("after", &cursor.to_string());
        let response = self
            .client
            .get(url)
            .header("x-trackmesh-auth", auth_header(&auth)?)
            .send()
            .await
            .context("mailbox refresh request failed")?;
        let page: MailboxPage = bounded_json(response, MAX_MAILBOX_PAGE_BYTES).await?;
        ensure!(
            page.records.len() <= MAX_MAILBOX_PAGE_RECORDS,
            "mailbox page contains too many records"
        );
        let mut next = cursor;
        for record in &page.records {
            ensure!(
                record.sequence > next && record.envelope.project_id == project,
                "invalid mailbox record order or project"
            );
            next = record.sequence;
        }
        ensure!(
            page.next_cursor == next,
            "mailbox cursor does not match its records"
        );
        Ok(page)
    }
}

fn auth_header(auth: &protocol::MailboxAuth) -> Result<String> {
    let header = STANDARD.encode(serde_json::to_vec(auth)?);
    ensure!(
        header.len() <= MAX_MAILBOX_AUTH_BYTES,
        "mailbox auth header too large"
    );
    Ok(header)
}

async fn bounded_json<T: DeserializeOwned>(mut response: Response, limit: usize) -> Result<T> {
    ensure!(
        response.status().is_success(),
        "mailbox returned HTTP {}",
        response.status().as_u16()
    );
    ensure!(
        response
            .content_length()
            .is_none_or(|length| length <= limit as u64),
        "mailbox response too large"
    );
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            chunk.len() <= limit.saturating_sub(body.len()),
            "mailbox response too large"
        );
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).context("invalid mailbox response")
}
