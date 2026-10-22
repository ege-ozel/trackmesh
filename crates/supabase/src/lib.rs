//! Bounded HTTPS client for the optional per-user Supabase service. Core owns project decisions.
use anyhow::{ensure, Context, Result};
use reqwest::Url;
mod client;
pub use client::{validate_supabase_publishable_key, SupabaseClient, SupabaseError};

/// Supabase project URLs use the hosted HTTPS origin; custom domains require a
/// separately configured origin and are intentionally not accepted.
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
