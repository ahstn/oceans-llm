//! S3 archive storage for AWS and compatible services such as RustFS.

use std::time::Duration;

use async_trait::async_trait;
use aws_config::{BehaviorVersion, Region, timeout::TimeoutConfig};
use aws_credential_types::Credentials;
use aws_sdk_s3::{Client, primitives::ByteStream};
use gateway_core::{SkillObjectStore, StoreError};
use url::Url;

/// Resolved settings. Deliberately not `Debug`: explicit credentials are secrets.
pub struct S3SkillStorageConfig {
    pub bucket: String,
    pub region: String,
    pub endpoint: Option<String>,
    pub prefix: String,
    pub force_path_style: bool,
    pub allow_http: bool,
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
    pub session_token: Option<String>,
    pub max_object_bytes: usize,
    pub request_timeout: Duration,
}

/// Stores archives under a configured prefix in an existing bucket.
/// Bucket creation and policy management belong to deployment tooling.
pub struct S3SkillObjectStore {
    client: Client,
    bucket: String,
    prefix: String,
    max_object_bytes: usize,
    request_timeout: Duration,
}

impl S3SkillObjectStore {
    pub async fn new(config: S3SkillStorageConfig) -> Result<Self, StoreError> {
        validate_config(&config)?;
        let mut loader = aws_config::defaults(BehaviorVersion::latest())
            .region(Region::new(config.region))
            .timeout_config(
                TimeoutConfig::builder()
                    .operation_timeout(config.request_timeout)
                    .build(),
            );
        if let (Some(access_key), Some(secret_key)) =
            (config.access_key_id, config.secret_access_key)
        {
            loader = loader.credentials_provider(Credentials::new(
                access_key,
                secret_key,
                config.session_token,
                None,
                "oceans-skills",
            ));
        }
        let shared_config = loader.load().await;
        let mut builder = aws_sdk_s3::config::Builder::from(&shared_config)
            .force_path_style(config.force_path_style);
        // Oceans owns the destination. Clear any endpoint inherited from the
        // AWS environment/profile, while retaining its credential chain.
        builder.set_endpoint_url(config.endpoint);
        Ok(Self {
            client: Client::from_conf(builder.build()),
            bucket: config.bucket,
            prefix: config.prefix.trim_end_matches('/').to_owned(),
            max_object_bytes: config.max_object_bytes,
            request_timeout: config.request_timeout,
        })
    }

    fn object_key(&self, key: &str) -> Result<String, StoreError> {
        validate_key(key)?;
        let key = if self.prefix.is_empty() {
            key.to_owned()
        } else {
            format!("{}/{key}", self.prefix)
        };
        if key.len() > 1024 {
            return Err(invalid_config("skill object key exceeds 1024 bytes"));
        }
        Ok(key)
    }

    async fn read_archive(&self, key: String) -> Result<Vec<u8>, StoreError> {
        let object = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|error| {
                if error
                    .as_service_error()
                    .is_some_and(|error| error.is_no_such_key())
                {
                    StoreError::NotFound("skill archive".to_owned())
                } else {
                    storage_error("read", error.raw_response().map(|r| r.status().as_u16()))
                }
            })?;
        if object.content_length().is_some_and(|length| {
            length < 0 || u64::try_from(length).unwrap_or(u64::MAX) > self.max_object_bytes as u64
        }) {
            return Err(archive_too_large());
        }
        read_bounded_body(object.body, self.max_object_bytes).await
    }
}

#[async_trait]
impl SkillObjectStore for S3SkillObjectStore {
    async fn put(&self, key: &str, bytes: &[u8]) -> Result<(), StoreError> {
        let key = self.object_key(key)?;
        if bytes.len() > self.max_object_bytes {
            return Err(archive_too_large());
        }
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .if_none_match("*")
            .content_type("application/zip")
            .body(ByteStream::from(bytes.to_vec()))
            .send()
            .await
            .map_err(|error| {
                let status = error.raw_response().map(|r| r.status().as_u16());
                if status == Some(412) {
                    StoreError::Conflict("skill archive object already exists".to_owned())
                } else {
                    storage_error("write", status)
                }
            })?;
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Vec<u8>, StoreError> {
        let key = self.object_key(key)?;
        // SDK operation timeouts end when the response body starts. Bound the
        // complete download too, including a stalled or indefinitely slow body.
        tokio::time::timeout(self.request_timeout, self.read_archive(key))
            .await
            .map_err(|_| StoreError::Unavailable("skill archive read timed out".to_owned()))?
    }

    async fn delete(&self, key: &str) -> Result<(), StoreError> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(self.object_key(key)?)
            .send()
            .await
            .map_err(|error| {
                storage_error("delete", error.raw_response().map(|r| r.status().as_u16()))
            })?;
        Ok(())
    }
}

async fn read_bounded_body(mut body: ByteStream, limit: usize) -> Result<Vec<u8>, StoreError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = body
        .try_next()
        .await
        .map_err(|_| storage_error("read body", None))?
    {
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return Err(archive_too_large());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn validate_config(config: &S3SkillStorageConfig) -> Result<(), StoreError> {
    if config.bucket.trim().is_empty() || config.region.trim().is_empty() {
        return Err(invalid_config(
            "skill storage bucket and region must not be empty",
        ));
    }
    if config.max_object_bytes == 0 || config.request_timeout.is_zero() {
        return Err(invalid_config(
            "skill storage limits must be greater than zero",
        ));
    }
    match (&config.access_key_id, &config.secret_access_key) {
        (Some(key), Some(secret)) if !key.is_empty() && !secret.is_empty() => {}
        (None, None) if config.session_token.is_none() => {}
        _ => {
            return Err(invalid_config(
                "skill storage requires both access key and secret key",
            ));
        }
    }
    let prefix = config.prefix.trim_end_matches('/');
    if !config.prefix.is_empty() {
        validate_key(prefix)?;
    }
    if let Some(endpoint) = &config.endpoint {
        validate_endpoint(endpoint, config.allow_http)?;
    }
    Ok(())
}

fn validate_endpoint(endpoint: &str, allow_http: bool) -> Result<(), StoreError> {
    let url = Url::parse(endpoint).map_err(|_| invalid_config("invalid skill storage endpoint"))?;
    if !matches!(url.scheme(), "https" | "http")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid_config(
            "skill storage endpoint must be an HTTP(S) URL without credentials, query, or fragment",
        ));
    }
    if url.scheme() == "http" && !allow_http {
        return Err(invalid_config(
            "HTTP skill storage requires allow_http = true",
        ));
    }
    Ok(())
}

fn validate_key(key: &str) -> Result<(), StoreError> {
    if key.len() > 1024
        || key.contains('\\')
        || key.chars().any(char::is_control)
        || key.split('/').any(|part| matches!(part, "" | "." | ".."))
    {
        return Err(invalid_config("invalid skill object key or prefix"));
    }
    Ok(())
}

fn invalid_config(message: &str) -> StoreError {
    StoreError::Unexpected(message.to_owned())
}

fn archive_too_large() -> StoreError {
    StoreError::Unexpected("skill archive exceeds configured storage limit".to_owned())
}

fn storage_error(operation: &str, status: Option<u16>) -> StoreError {
    // Never return SDK error text: it can include request headers, signed URLs,
    // endpoint credentials, or service-controlled response bodies.
    let message = match status {
        Some(status) => format!("S3 skill archive {operation} failed (HTTP {status})"),
        None => format!("S3 skill archive {operation} failed"),
    };
    StoreError::Unavailable(message)
}

#[cfg(test)]
mod tests;
