mod api;
mod store;

use anyhow::{ensure, Result};
use axum::{extract::DefaultBodyLimit, middleware, routing::get, Router};
use clap::Parser;
use std::{net::SocketAddr, path::PathBuf, sync::Arc};

#[derive(Parser)]
#[command(about = "Private encrypted metadata mailbox; use HTTPS reverse proxy in production")]
struct Cli {
    #[arg(long)]
    database: PathBuf,
    #[arg(long)]
    acl: PathBuf,
    #[arg(long, default_value = "127.0.0.1:8787")]
    listen: SocketAddr,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    ensure!(
        cli.listen.ip().is_loopback(),
        "mailbox backend must bind loopback behind HTTPS"
    );
    api::read_acl(&cli.acl)?;
    let state = Arc::new(api::State {
        store: store::Store::open(&cli.database)?,
        acl: cli.acl,
        workers: Arc::new(tokio::sync::Semaphore::new(16)),
        requests: tokio::sync::Semaphore::new(64),
    });
    let app = Router::new()
        .route(
            "/v1/projects/{project}/announcements",
            get(api::page).post(api::publish),
        )
        .layer(DefaultBodyLimit::max(protocol::MAX_MAILBOX_ENVELOPE_BYTES))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            api::request_deadline,
        ))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(cli.listen).await?;
    println!("Mailbox backend listening on {}", listener.local_addr()?);
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
