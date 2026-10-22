//! Signed, bounded HTTP transport for the Supabase endpoint mailbox.
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use crypto::Identity;
use protocol::{SupabaseAccepted, SupabaseEndpointRow, SupabaseHeadRow, MAX_MAILBOX_AUTH_BYTES};
use reqwest::{redirect, Client, Response, StatusCode, Url};
use serde::{de::DeserializeOwned, Serialize};
use std::time::Duration;
use uuid::Uuid;

const MAX_REQUEST_BYTES: usize = 64 * 1024;
const MAX_INVITE_REQUEST_BYTES: usize = 70 * 1024;
const MAX_RESPONSE_BYTES: usize = 8 * 64 * 1024 + 4096;
const MAX_INVITE_RESPONSE_BYTES: usize = 64 * 1024 + 1024;

pub fn validate_supabase_publishable_key(key: &str) -> Result<()> {
    ensure!(
        key.starts_with("sb_publishable_")
            && (key.len() - "sb_publishable_".len()) >= 16
            && key.len() <= 256
            && key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'),
        if key.starts_with("sb_secret_") {
            "secret key istemciye girilmez"
        } else {
            "invalid Supabase publishable key"
        }
    );
    Ok(())
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupabaseInviteDrop {
    pub ciphertext: String,
    pub expires_at: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupabaseError {
    Rejected,
    Unavailable,
    NotDeployed,
}

pub struct SupabaseClient {
    client: Client,
    project_url: Url,
    project: Uuid,
    anon_key: String,
}

impl SupabaseClient {
    pub fn new(project_url: &str, anon_key: &str, project: Uuid) -> Result<Self> {
        protocol::require_protocol(protocol::PROTOCOL_VERSION)?;
        ensure!(!project.is_nil(), "Supabase route cannot be nil");
        super::validate_supabase_project_url(project_url)?;
        super::validate_supabase_publishable_key(anon_key)?;
        let project_url = Url::parse(project_url)?;
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
            project_url,
            project,
            anon_key: anon_key.to_owned(),
        })
    }

    pub async fn claim(&self, identity: &Identity) -> std::result::Result<bool, SupabaseError> {
        let response: SupabaseAccepted = self
            .post(
                identity,
                "project-claim",
                &serde_json::json!({"actor_seq": "1"}),
                4096,
            )
            .await?;
        Ok(response.accepted)
    }

    pub async fn create_invite_drop(
        &self,
        identity: &Identity,
        id: &str,
        expires_at: &str,
        ciphertext: &str,
    ) -> std::result::Result<bool, SupabaseError> {
        let response: SupabaseAccepted = self
            .post(
                identity,
                "invite-create",
                &serde_json::json!({"id": id, "expires_at": expires_at, "ciphertext": ciphertext}),
                4096,
            )
            .await?;
        Ok(response.accepted)
    }

    pub async fn consume_invite_drop(
        project_url: &str,
        anon_key: &str,
        id: &str,
    ) -> std::result::Result<SupabaseInviteDrop, SupabaseError> {
        let endpoint = async {
            super::validate_supabase_project_url(project_url)
                .map_err(|_| SupabaseError::Rejected)?;
            if super::validate_supabase_publishable_key(anon_key).is_err() {
                return Err(SupabaseError::Rejected);
            }
            if id.len() != 43
                || !id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
            {
                return Err(SupabaseError::Rejected);
            }
            let project_url = Url::parse(project_url).map_err(|_| SupabaseError::Rejected)?;
            let _ = rustls::crypto::ring::default_provider().install_default();
            let client = Client::builder()
                .redirect(redirect::Policy::none())
                .no_proxy()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(20))
                .user_agent("TrackMesh/1")
                .build()
                .map_err(|_| SupabaseError::Unavailable)?;
            let mut endpoint = project_url;
            endpoint.set_path("/functions/v1/invite-consume");
            let response = client
                .post(endpoint)
                .header("apikey", anon_key)
                .header("content-type", "application/json")
                .body(
                    serde_json::to_vec(&serde_json::json!({"id": id}))
                        .map_err(|_| SupabaseError::Unavailable)?,
                )
                .send()
                .await
                .map_err(|_| SupabaseError::Unavailable)?;
            if response.status() == StatusCode::BAD_REQUEST {
                return Err(SupabaseError::Rejected);
            }
            if !response.status().is_success() {
                return Err(SupabaseError::Unavailable);
            }
            bounded_json(response, MAX_INVITE_RESPONSE_BYTES)
                .await
                .map_err(|_| SupabaseError::Unavailable)
        }
        .await;
        endpoint
    }

    pub async fn grant(
        &self,
        identity: &Identity,
        actor_seq: u64,
        target_device_id: Uuid,
        target_public_key: &str,
    ) -> std::result::Result<bool, SupabaseError> {
        let response: SupabaseAccepted = self
            .post(
                identity,
                "member-grant",
                &serde_json::json!({
                    "actor_seq": actor_seq.to_string(),
                    "target_device_id": target_device_id.to_string(),
                    "target_public_key": target_public_key,
                }),
                4096,
            )
            .await?;
        Ok(response.accepted)
    }

    pub async fn revoke(
        &self,
        identity: &Identity,
        actor_seq: u64,
        target_device_id: Uuid,
    ) -> std::result::Result<bool, SupabaseError> {
        let response: SupabaseAccepted = self
            .post(
                identity,
                "member-revoke",
                &serde_json::json!({
                    "actor_seq": actor_seq.to_string(),
                    "target_device_id": target_device_id.to_string(),
                }),
                4096,
            )
            .await?;
        Ok(response.accepted)
    }

    pub async fn publish_endpoint(
        &self,
        identity: &Identity,
        seq: u64,
        expires_at: &str,
        ciphertext: &str,
    ) -> std::result::Result<bool, SupabaseError> {
        let response: SupabaseAccepted = self
            .post(
                identity,
                "endpoint-publish",
                &serde_json::json!({
                    "seq": seq.to_string(),
                    "expires_at": expires_at,
                    "ciphertext": ciphertext,
                }),
                4096,
            )
            .await?;
        Ok(response.accepted)
    }

    pub async fn endpoint_page(
        &self,
        identity: &Identity,
        cursor: Option<&str>,
    ) -> std::result::Result<Vec<SupabaseEndpointRow>, SupabaseError> {
        let value: serde_json::Value = self
            .post(
                identity,
                "endpoint-list",
                &serde_json::json!({ "after_cursor": cursor }),
                MAX_RESPONSE_BYTES,
            )
            .await?;
        if value
            .as_object()
            .is_some_and(|object| object.get("accepted") == Some(&serde_json::Value::Bool(false)))
        {
            return Err(SupabaseError::Rejected);
        }
        let rows: Vec<SupabaseEndpointRow> =
            serde_json::from_value(value).map_err(|_| SupabaseError::Unavailable)?;
        if rows.len() > 8 {
            return Err(SupabaseError::Rejected);
        }
        let mut previous = cursor
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        for row in &rows {
            let Ok(sequence) = row.cursor.parse::<u64>() else {
                return Err(SupabaseError::Rejected);
            };
            if sequence.to_string() != row.cursor
                || sequence <= previous
                || row.ciphertext.is_empty()
                || row.ciphertext.len() > 64 * 1024
                || !row
                    .ciphertext
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
            {
                return Err(SupabaseError::Rejected);
            }
            previous = sequence;
        }
        Ok(rows)
    }

    pub async fn publish_head(
        &self,
        identity: &Identity,
        seq: u64,
        ciphertext: &str,
    ) -> std::result::Result<bool, SupabaseError> {
        if ciphertext.is_empty()
            || ciphertext.len() > 4096
            || !ciphertext
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(SupabaseError::Rejected);
        }
        let response: SupabaseAccepted = self
            .post(
                identity,
                "head-publish",
                &serde_json::json!({"seq":seq.to_string(),"ciphertext":ciphertext}),
                4096,
            )
            .await?;
        Ok(response.accepted)
    }

    pub async fn head_list(
        &self,
        identity: &Identity,
    ) -> std::result::Result<Vec<SupabaseHeadRow>, SupabaseError> {
        let rows: Vec<SupabaseHeadRow> = self
            .post(identity, "head-list", &serde_json::json!({}), 160 * 1024)
            .await?;
        if rows.len() > 32 {
            return Err(SupabaseError::Rejected);
        }
        let mut writers = std::collections::HashSet::new();
        for row in &rows {
            let Ok(writer) = uuid::Uuid::parse_str(&row.writer_device_id) else {
                return Err(SupabaseError::Rejected);
            };
            let Ok(seq) = row.seq.parse::<u64>() else {
                return Err(SupabaseError::Rejected);
            };
            if writer.is_nil()
                || writer.to_string() != row.writer_device_id
                || seq == 0
                || seq.to_string() != row.seq
                || !writers.insert(writer)
                || row.ciphertext.is_empty()
                || row.ciphertext.len() > 4096
                || !row
                    .ciphertext
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            {
                return Err(SupabaseError::Rejected);
            }
            let time = row.updated_at.as_bytes();
            let valid_time = time.len() == 24
                && time.iter().enumerate().all(|(index, byte)| match index {
                    4 | 7 => *byte == b'-',
                    10 => *byte == b'T',
                    13 | 16 => *byte == b':',
                    19 => *byte == b'.',
                    23 => *byte == b'Z',
                    _ => byte.is_ascii_digit(),
                });
            if !valid_time {
                return Err(SupabaseError::Rejected);
            }
        }
        Ok(rows)
    }

    async fn post<T: DeserializeOwned, B: Serialize>(
        &self,
        identity: &Identity,
        operation: &str,
        body_value: &B,
        response_limit: usize,
    ) -> std::result::Result<T, SupabaseError> {
        let body = serde_json::to_vec(body_value).map_err(|_| SupabaseError::Unavailable)?;
        let body_limit = if operation == "invite-create" {
            MAX_INVITE_REQUEST_BYTES
        } else {
            MAX_REQUEST_BYTES
        };
        if body.len() > body_limit {
            return Err(SupabaseError::Rejected);
        }
        let auth = identity
            .supabase_auth(self.project, operation, &body)
            .map_err(|_| SupabaseError::Unavailable)?;
        let auth =
            STANDARD.encode(serde_json::to_vec(&auth).map_err(|_| SupabaseError::Unavailable)?);
        if auth.len() > MAX_MAILBOX_AUTH_BYTES {
            return Err(SupabaseError::Rejected);
        }
        let mut endpoint = self.project_url.clone();
        endpoint.set_path(&format!("/functions/v1/{operation}"));
        let response = self
            .client
            .post(endpoint)
            .header("apikey", &self.anon_key)
            .header("x-trackmesh-auth", auth)
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await
            .map_err(|_| SupabaseError::Unavailable)?;
        if response.status() == StatusCode::NOT_FOUND {
            return Err(SupabaseError::NotDeployed);
        }
        if response.status() == StatusCode::BAD_REQUEST {
            return Err(SupabaseError::Rejected);
        }
        if !response.status().is_success() {
            return Err(SupabaseError::Unavailable);
        }
        bounded_json(response, response_limit)
            .await
            .map_err(|_| SupabaseError::Unavailable)
    }
}

async fn bounded_json<T: DeserializeOwned>(mut response: Response, limit: usize) -> Result<T> {
    ensure!(
        response
            .content_length()
            .is_none_or(|length| length <= limit as u64),
        "Supabase response too large"
    );
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .context("Supabase response read failed")?
    {
        ensure!(
            chunk.len() <= limit.saturating_sub(body.len()),
            "Supabase response too large"
        );
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).context("invalid Supabase response")
}
