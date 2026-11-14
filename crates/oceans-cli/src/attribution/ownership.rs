//! The API key's ownership scope, from `GET /api/v1/me/ownership`.

use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::client::Client;

const MAX_OWNERSHIP_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ownership {
    pub ownership_scope: String,
    pub owner_kind: String,
    pub owner_id: String,
}

#[derive(Debug)]
pub enum LookupError {
    /// Worth retrying: the network, `429` or `5xx`.
    Transient(String),
    Fatal(String),
}

impl std::fmt::Display for LookupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transient(message) | Self::Fatal(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for LookupError {}

/// The first 16 hex digits of the key's SHA-256: a cache key that never reveals the key.
pub fn key_fingerprint(api_key: &str) -> String {
    let digest = format!("{:x}", Sha256::digest(api_key.as_bytes()));
    digest[..16].to_owned()
}

pub async fn lookup(client: &Client) -> Result<Ownership, LookupError> {
    let response = client
        .send_raw(Method::GET, &["me", "ownership"], None, MAX_OWNERSHIP_BYTES)
        .await
        .map_err(|_| LookupError::Transient("could not reach the gateway".into()))?;
    match response.status {
        StatusCode::OK => {}
        StatusCode::UNAUTHORIZED => {
            return Err(LookupError::Fatal(
                "the gateway rejected the API key (HTTP 401)".into(),
            ));
        }
        StatusCode::FORBIDDEN => {
            return Err(LookupError::Fatal(
                "the API key can't read its ownership (HTTP 403); use a full-access key or one with the attribution:write scope".into(),
            ));
        }
        StatusCode::NOT_FOUND => {
            return Err(LookupError::Fatal(
                "this gateway has no attribution API (HTTP 404); it may need an upgrade".into(),
            ));
        }
        status if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() => {
            return Err(LookupError::Transient(format!(
                "the gateway returned HTTP {status}"
            )));
        }
        status => {
            return Err(LookupError::Fatal(format!(
                "the gateway returned HTTP {status}"
            )));
        }
    }
    let ownership: Ownership = serde_json::from_slice(&response.body).map_err(|_| {
        LookupError::Fatal("the gateway returned an invalid ownership response".into())
    })?;
    let valid = matches!(ownership.owner_kind.as_str(), "user" | "service_account")
        && Uuid::parse_str(&ownership.owner_id).is_ok()
        && ownership.ownership_scope == format!("{}:{}", ownership.owner_kind, ownership.owner_id);
    if !valid {
        return Err(LookupError::Fatal(
            "the gateway returned an invalid ownership scope".into(),
        ));
    }
    Ok(ownership)
}
