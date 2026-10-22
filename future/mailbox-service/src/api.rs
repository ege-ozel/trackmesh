use crate::store::Store;
use anyhow::{ensure, Result};
use axum::{
    body::Bytes,
    extract::{Path, Query, Request, State as AppState},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::Utc;
use protocol::{mailbox_scope, DeviceCard, Envelope, MailboxAuth};
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    io::Read,
    path::{Path as FilePath, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::Semaphore;
use uuid::Uuid;

pub struct State {
    pub store: Store,
    pub acl: PathBuf,
    pub workers: Arc<Semaphore>,
    pub requests: Semaphore,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    project_id: Uuid,
    device: DeviceCard,
}

pub fn read_acl(path: &FilePath) -> Result<Vec<Grant>> {
    let mut data = Vec::new();
    std::fs::File::open(path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut data)?;
    ensure!(data.len() <= 1024 * 1024, "ACL document too large");
    let grants: Vec<Grant> = serde_json::from_slice(&data)?;
    ensure!(grants.len() <= 1024, "too many ACL grants");
    let mut bindings = HashMap::new();
    let mut projects: HashMap<Uuid, HashSet<Uuid>> = HashMap::new();
    for grant in &grants {
        ensure!(!grant.project_id.is_nil(), "invalid ACL project");
        ensure!(!grant.device.device_id.is_nil(), "invalid ACL device");
        network::validate_card(&grant.device)?;
        let binding = (&grant.device.public_key, &grant.device.peer_id);
        if let Some(previous) = bindings.insert(grant.device.device_id, binding) {
            ensure!(previous == binding, "conflicting ACL device identity");
        }
        let devices = projects.entry(grant.project_id).or_default();
        ensure!(
            devices.insert(grant.device.device_id),
            "duplicate ACL grant"
        );
        ensure!(devices.len() <= 3, "MVP supports three devices per project");
    }
    Ok(grants)
}

fn authenticate(
    state: &State,
    headers: &HeaderMap,
    project: Uuid,
    scope: &str,
    body: &[u8],
) -> Result<MailboxAuth, StatusCode> {
    let header = headers
        .get("x-trackmesh-auth")
        .ok_or(StatusCode::UNAUTHORIZED)?;
    if headers.get_all("x-trackmesh-auth").iter().count() != 1
        || header.as_bytes().len() > protocol::MAX_MAILBOX_AUTH_BYTES
    {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let bytes = STANDARD
        .decode(header.as_bytes())
        .map_err(|_| StatusCode::UNAUTHORIZED)?;
    let auth: MailboxAuth = serde_json::from_slice(&bytes).map_err(|_| StatusCode::UNAUTHORIZED)?;
    crypto::verify_mailbox_auth(&auth, project, scope, body, Utc::now())
        .map_err(|_| StatusCode::UNAUTHORIZED)?;
    let grants = read_acl(&state.acl).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    if !grants.iter().any(|grant| {
        grant.project_id == project
            && grant.device.device_id == auth.device_id
            && grant.device.public_key == auth.public_key
    }) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(auth)
}

pub async fn publish(
    AppState(state): AppState<Arc<State>>,
    Path(project): Path<Uuid>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, StatusCode> {
    let permit = state
        .workers
        .clone()
        .try_acquire_owned()
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let auth = authenticate(&state, &headers, project, &mailbox_scope(None), &body)?;
        let validated = (|| {
            let envelope: Envelope =
                serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
            if envelope.project_id != project
                || envelope.author_device_id != auth.device_id
                || envelope.public_key != auth.public_key
                || envelope.snapshot_id.is_nil()
            {
                return Err(StatusCode::BAD_REQUEST);
            }
            crypto::verify_envelope(&envelope).map_err(|_| StatusCode::BAD_REQUEST)?;
            Ok(envelope)
        })();
        // Even a malformed envelope consumes authenticated replay/rate state.
        state.store.publish(&auth, validated).map(Json)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cursor {
    after: i64,
}

pub async fn page(
    AppState(state): AppState<Arc<State>>,
    Path(project): Path<Uuid>,
    Query(cursor): Query<Cursor>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, StatusCode> {
    if cursor.after < 0 || !body.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let permit = state
        .workers
        .clone()
        .try_acquire_owned()
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let auth = authenticate(
            &state,
            &headers,
            project,
            &mailbox_scope(Some(cursor.after)),
            &[],
        )?;
        state.store.page(&auth, cursor.after).map(Json)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
}

pub async fn request_deadline(
    AppState(state): AppState<Arc<State>>,
    request: Request,
    next: Next,
) -> Response {
    let Ok(_permit) = state.requests.try_acquire() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let mut response = match tokio::time::timeout(Duration::from_secs(30), next.run(request)).await
    {
        Ok(response) => response,
        Err(_) => StatusCode::REQUEST_TIMEOUT.into_response(),
    };
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().expect("static header"));
    response
}
