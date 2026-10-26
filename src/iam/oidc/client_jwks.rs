//! JWK Sets that `private_key_jwt` clients publish at their `jwks_uri`.
//!
//! Documents are cached in the process for [`CACHE_TTL`]. A key the cached set
//! does not have (the client rotated) triggers a refetch, at most once per
//! [`REFETCH_INTERVAL`] per URL so bad assertions cannot drive fetches.

use super::http_client;
use jwt::JwkSet;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(300);
const REFETCH_INTERVAL: Duration = Duration::from_secs(10);
const MAX_DOCUMENT_BYTES: usize = 64 * 1024;

struct Cached {
    jwks: JwkSet,
    fetched_at: Instant,
}

static CACHE: Lazy<Mutex<HashMap<String, Cached>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// Which copy of the client's keys to use.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// The cached set while it is younger than the TTL.
    Cached,
    /// A fresh download, unless the cached set is younger than the refetch
    /// interval (the keys just rotated, or the assertion is bad).
    Refetch,
}

/// The JWK Set at `uri`.
pub async fn jwk_set(uri: &str, freshness: Freshness) -> Result<JwkSet, String> {
    let max_age = match freshness {
        Freshness::Cached => CACHE_TTL,
        Freshness::Refetch => REFETCH_INTERVAL,
    };
    if let Some(cached) = cached(uri, max_age) {
        return Ok(cached);
    }
    let jwks = fetch(uri).await?;
    CACHE.lock().expect("client JWKS cache").insert(
        uri.to_string(),
        Cached {
            jwks: jwks.clone(),
            fetched_at: Instant::now(),
        },
    );
    Ok(jwks)
}

fn cached(uri: &str, max_age: Duration) -> Option<JwkSet> {
    let cache = CACHE.lock().expect("client JWKS cache");
    cache
        .get(uri)
        .filter(|cached| cached.fetched_at.elapsed() < max_age)
        .map(|cached| cached.jwks.clone())
}

async fn fetch(uri: &str) -> Result<JwkSet, String> {
    let response = http_client::client()
        .get(uri)
        .header(
            http::header::ACCEPT,
            "application/jwk-set+json, application/json",
        )
        .send()
        .await
        .map_err(|error| http_client::error_chain(&error))?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    if response
        .content_length()
        .is_some_and(|length| length as usize > MAX_DOCUMENT_BYTES)
    {
        return Err("JWK Set document too large".to_string());
    }
    let body = response
        .bytes()
        .await
        .map_err(|error| http_client::error_chain(&error))?;
    if body.len() > MAX_DOCUMENT_BYTES {
        return Err("JWK Set document too large".to_string());
    }
    let value: serde_json::Value =
        serde_json::from_slice(&body).map_err(|error| format!("not JSON: {error}"))?;
    jwt::validate_public_jwk_set(&value)
}
