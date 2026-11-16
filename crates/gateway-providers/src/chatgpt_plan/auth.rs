use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

use async_trait::async_trait;
use gateway_core::ProviderError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::sync::Mutex;

use crate::token::{AccessToken, AccessTokenSource};

const TOKEN_URL: &str = "https://auth.openai.com/api/accounts/oauth/token";
const RESOURCE: &str = "https://api.openai.com/v1";
const DIRECT_SCOPE: &str = "chatgpt.tokens.use.direct";
const REFRESH_BEFORE: Duration = Duration::minutes(4);
const LOGIN_HINT: &str = "run `oceans providers login-chatgpt` to sign in again";

// The on-disk schema is shared with `oceans providers login-chatgpt`. Retain fields
// introduced by future CLI versions when replacing the rotating token set.
#[derive(Deserialize, Serialize)]
struct Credentials {
    email: String,
    issuer: String,
    subject: String,
    client_id: String,
    ext_agent_host_id: String,
    id_token: String,
    access_token: String,
    refresh_token: String,
    token_type: String,
    expires_in: u64,
    scopes: Vec<String>,
    saved_at: String,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

impl Credentials {
    fn token(&self) -> Result<AccessToken, ProviderError> {
        require_direct_scope(&self.scopes)?;
        if self.access_token.is_empty() || self.refresh_token.is_empty() {
            return Err(sign_in_error(
                "ChatGPT plan credentials have no usable token set",
            ));
        }
        if self.client_id.is_empty() || self.client_id == "dynamic_agent_client" {
            return Err(ProviderError::InvalidRequest(
                "ChatGPT plan credentials must contain the issued client_id, not dynamic_agent_client; fix the saved client configuration".into(),
            ));
        }
        if !self.token_type.eq_ignore_ascii_case("Bearer") {
            return Err(ProviderError::InvalidRequest(
                "ChatGPT plan credentials must use token_type Bearer".into(),
            ));
        }
        let saved_at = OffsetDateTime::parse(&self.saved_at, &Rfc3339).map_err(|_| {
            sign_in_error("ChatGPT plan credentials contain an invalid RFC3339 saved_at")
        })?;
        let lifetime = i64::try_from(self.expires_in)
            .ok()
            .and_then(|seconds| saved_at.checked_add(Duration::seconds(seconds)))
            .ok_or_else(|| {
                sign_in_error("ChatGPT plan credentials contain an invalid expires_in")
            })?;
        Ok(AccessToken {
            token: self.access_token.clone(),
            expires_at: lifetime,
        })
    }
}

fn require_direct_scope(scopes: &[String]) -> Result<(), ProviderError> {
    if !scopes.iter().any(|scope| scope == DIRECT_SCOPE) {
        return Err(sign_in_error(
            "ChatGPT plan usage is disabled: missing chatgpt.tokens.use.direct permission; enable ChatGPT plan usage when signing in",
        ));
    }
    Ok(())
}

fn sign_in_error(message: &str) -> ProviderError {
    ProviderError::InvalidRequest(format!("{message}; {LOGIN_HINT}"))
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct CredentialVersion {
    modified: SystemTime,
    length: u64,
}

impl CredentialVersion {
    fn from_metadata(metadata: &fs::Metadata) -> Result<Self, ProviderError> {
        Ok(Self {
            modified: metadata.modified().map_err(|_| {
                sign_in_error("could not inspect ChatGPT plan credential modification time")
            })?,
            length: metadata.len(),
        })
    }
}

struct CachedToken {
    version: CredentialVersion,
    token: AccessToken,
}

/// Reads the selected SIWC credential record and safely persists rotating tokens.
/// Refreshes are detached from the requesting task once started. The sibling lock
/// file also serializes refreshes across provider instances and gateway processes.
#[derive(Clone)]
pub struct ChatGptPlanTokenSource {
    credentials_file: PathBuf,
    client: reqwest::Client,
    token_url: String,
    refreshing: Arc<Mutex<()>>,
    cached: Arc<Mutex<Option<CachedToken>>>,
    #[cfg(test)]
    credential_reads: Arc<std::sync::atomic::AtomicUsize>,
}

impl ChatGptPlanTokenSource {
    pub fn new(credentials_file: PathBuf) -> Result<Self, ProviderError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .build()
            .map_err(|_| {
                ProviderError::Transport("could not construct ChatGPT plan token client".into())
            })?;
        Ok(Self {
            credentials_file,
            client,
            token_url: TOKEN_URL.into(),
            refreshing: Arc::new(Mutex::new(())),
            cached: Arc::new(Mutex::new(None)),
            #[cfg(test)]
            credential_reads: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        })
    }

    #[cfg(test)]
    fn with_token_url(credentials_file: PathBuf, token_url: String) -> Self {
        let mut source = Self::new(credentials_file).unwrap();
        source.token_url = token_url;
        source
    }

    async fn cached_token(&self) -> Result<Option<AccessToken>, ProviderError> {
        if !self
            .cached
            .lock()
            .await
            .as_ref()
            .is_some_and(|cached| cached.token.is_usable(REFRESH_BEFORE))
        {
            return Ok(None);
        }
        let path = self.credentials_file.clone();
        let version = tokio::task::spawn_blocking(move || {
            let metadata = fs::metadata(path)
                .map_err(|_| sign_in_error("could not inspect ChatGPT plan credential file"))?;
            CredentialVersion::from_metadata(&metadata)
        })
        .await
        .map_err(|_| {
            ProviderError::Transport("ChatGPT plan credential metadata task failed".into())
        })??;
        // Only compare metadata and clone the parsed token under this short lock.
        // Fresh requests never acquire the refresh lock or parse the JSON again.
        let cached = self.cached.lock().await;
        Ok(cached.as_ref().and_then(|cached| {
            (cached.version == version && cached.token.is_usable(REFRESH_BEFORE))
                .then(|| cached.token.clone())
        }))
    }

    async fn load_credentials(&self) -> Result<(Credentials, CredentialVersion), ProviderError> {
        let path = self.credentials_file.clone();
        #[cfg(test)]
        let reads = Arc::clone(&self.credential_reads);
        tokio::task::spawn_blocking(move || {
            #[cfg(test)]
            reads.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            read_credentials(&path)
        })
        .await
        .map_err(|_| ProviderError::Transport("ChatGPT plan credential read task failed".into()))?
    }

    async fn remember_token(
        &self,
        credentials: &Credentials,
        version: CredentialVersion,
    ) -> Result<AccessToken, ProviderError> {
        let token = credentials.token();
        *self.cached.lock().await = token.as_ref().ok().map(|token| CachedToken {
            version,
            token: token.clone(),
        });
        token
    }

    async fn persist_credentials(
        &self,
        credentials: Credentials,
    ) -> Result<(Credentials, CredentialVersion), ProviderError> {
        let path = self.credentials_file.clone();
        tokio::task::spawn_blocking(move || {
            let version = write_credentials(&path, &credentials)?;
            Ok((credentials, version))
        })
        .await
        .map_err(|_| ProviderError::Transport("ChatGPT plan credential write task failed".into()))?
    }

    async fn refresh(&self) -> Result<AccessToken, ProviderError> {
        let path = self.credentials_file.clone();
        // File::lock may block behind another process, so never run it on a Tokio
        // executor thread. Hold the file until the replacement is fully persisted.
        let _file_lock = tokio::task::spawn_blocking(move || lock_credentials(&path))
            .await
            .map_err(|_| {
                ProviderError::Transport("ChatGPT plan credential lock task failed".into())
            })??;
        let (mut credentials, version) = self.load_credentials().await?;
        let token = self.remember_token(&credentials, version).await?;
        // Another source/process may already have renewed this token while we
        // waited. Always use the latest refresh token read under the file lock.
        if token.is_usable(REFRESH_BEFORE) {
            return Ok(token);
        }

        let response = self
            .client
            .post(&self.token_url)
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", credentials.client_id.as_str()),
                ("refresh_token", credentials.refresh_token.as_str()),
                ("resource", RESOURCE),
            ])
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    ProviderError::Timeout
                } else {
                    // Do not expose the token endpoint URL or its form body.
                    ProviderError::Transport(
                        "ChatGPT plan token refresh failed; credentials were preserved".into(),
                    )
                }
            })?;
        let status = response.status();
        let body = response.bytes().await.map_err(|_| {
            ProviderError::Transport(
                "could not read ChatGPT plan token refresh response; credentials were preserved"
                    .into(),
            )
        })?;
        if status != reqwest::StatusCode::OK {
            let code = refresh_error_code(&body);
            if matches!(
                code.as_deref(),
                Some(
                    "invalid_grant"
                        | "invalid_refresh_token"
                        | "token_expired"
                        | "refresh_token_expired"
                        | "refresh_token_invalidated"
                        | "refresh_token_reused"
                )
            ) {
                credentials.access_token.clear();
                credentials.refresh_token.clear();
                credentials.expires_in = 0;
                // Keep even an expired ID token: the CLI can use it as a hint
                // when starting the next browser sign-in.
                self.persist_credentials(credentials).await?;
                *self.cached.lock().await = None;
                return Err(sign_in_error(&format!(
                    "ChatGPT plan session is no longer usable ({})",
                    code.as_deref().unwrap_or("invalid_refresh_token")
                )));
            }
            if code.as_deref() == Some("invalid_client") {
                return Err(ProviderError::InvalidRequest(
                    "ChatGPT plan refresh failed with invalid_client; fix the issued client_id in the credential configuration".into(),
                ));
            }
            // Token endpoint bodies can include credentials. Surface only status;
            // never blindly retry a POST whose refresh token may have been spent.
            return Err(ProviderError::UpstreamHttp {
                status: status.as_u16(),
                body: "ChatGPT plan token refresh failed; credentials were preserved".into(),
                request_id: None,
            });
        }
        let fresh: RefreshResponse = serde_json::from_slice(&body).map_err(|_| {
            ProviderError::Transport(
                "invalid ChatGPT plan token refresh response; credentials were preserved".into(),
            )
        })?;
        if fresh.access_token.is_empty() || fresh.refresh_token.is_empty() || fresh.expires_in == 0
        {
            return Err(ProviderError::Transport(
                "ChatGPT plan token refresh response is missing a usable replacement token set"
                    .into(),
            ));
        }
        credentials.access_token = fresh.access_token;
        credentials.refresh_token = fresh.refresh_token;
        credentials.expires_in = fresh.expires_in;
        credentials.saved_at = OffsetDateTime::now_utc().format(&Rfc3339).map_err(|_| {
            ProviderError::Transport("could not timestamp ChatGPT plan token refresh".into())
        })?;
        if let Some(scope) = fresh.scope {
            credentials.scopes = scope.split_whitespace().map(str::to_owned).collect();
        }
        if let Some(id_token) = fresh.id_token {
            credentials.id_token = id_token;
        }
        if let Some(token_type) = fresh.token_type {
            credentials.token_type = token_type;
        }
        // Even if the new grant lacks direct permission, persist its replacement
        // refresh token before rejecting inference, so rotation is never lost.
        let (credentials, version) = self.persist_credentials(credentials).await?;
        self.remember_token(&credentials, version).await
    }
}

#[async_trait]
impl AccessTokenSource for ChatGptPlanTokenSource {
    async fn fetch_token(&self) -> Result<AccessToken, ProviderError> {
        if let Some(token) = self.cached_token().await? {
            return Ok(token);
        }
        let guard = Arc::clone(&self.refreshing).lock_owned().await;
        // A preceding refresh may have filled the cache while we waited.
        if let Some(token) = self.cached_token().await? {
            return Ok(token);
        }
        let source = self.clone();
        // Dropping a JoinHandle detaches the task instead of cancelling it. Move
        // the owned mutex guard into it to serialize callers even after abort.
        tokio::spawn(async move {
            let _guard = guard;
            source.refresh().await
        })
        .await
        .map_err(|_| ProviderError::Transport("ChatGPT plan token refresh task failed".into()))?
    }
}

#[derive(Deserialize)]
struct RefreshResponse {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
    scope: Option<String>,
    id_token: Option<String>,
    token_type: Option<String>,
}

fn refresh_error_code(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    value
        .get("error")
        .and_then(|error| {
            error
                .as_str()
                .or_else(|| error.get("code").and_then(Value::as_str))
        })
        .or_else(|| value.get("code").and_then(Value::as_str))
        .map(str::to_owned)
}

fn read_credentials(path: &Path) -> Result<(Credentials, CredentialVersion), ProviderError> {
    let file = File::open(path)
        .map_err(|_| sign_in_error("could not open ChatGPT plan credential file"))?;
    // Read metadata from the opened file, not the path: a concurrent atomic
    // login replacement must never label old contents with the new file's key.
    let metadata = file
        .metadata()
        .map_err(|_| sign_in_error("could not inspect ChatGPT plan credential file"))?;
    let version = CredentialVersion::from_metadata(&metadata)?;
    let credentials = serde_json::from_reader(file).map_err(|_| {
        sign_in_error("invalid ChatGPT plan credential file; expected the SIWC credential schema")
    })?;
    Ok((credentials, version))
}

fn lock_credentials(path: &Path) -> Result<File, ProviderError> {
    let filename = path.file_name().ok_or_else(|| {
        ProviderError::InvalidRequest("ChatGPT plan credentials_file must name a file".into())
    })?;
    let lock_path = path.with_file_name(format!(".{}.lock", filename.to_string_lossy()));
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(lock_path).map_err(|_| {
        ProviderError::Transport("could not open ChatGPT plan credential lock file".into())
    })?;
    file.lock()
        .map_err(|_| ProviderError::Transport("could not lock ChatGPT plan credentials".into()))?;
    Ok(file)
}

fn write_credentials(
    path: &Path,
    credentials: &Credentials,
) -> Result<CredentialVersion, ProviderError> {
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let temporary = parent.join(format!(".chatgpt-plan-{}.tmp", uuid::Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary).map_err(|_| {
        ProviderError::Transport(
            "could not create protected ChatGPT plan credential replacement".into(),
        )
    })?;
    let result = (|| -> Result<CredentialVersion, ProviderError> {
        serde_json::to_writer_pretty(&mut file, credentials).map_err(|_| {
            ProviderError::Transport("could not serialize ChatGPT plan credentials".into())
        })?;
        file.write_all(b"\n")
            .and_then(|()| file.sync_all())
            .map_err(|_| {
                ProviderError::Transport(
                    "could not flush ChatGPT plan credential replacement".into(),
                )
            })?;
        let metadata = file.metadata().map_err(|_| {
            ProviderError::Transport("could not inspect ChatGPT plan credential replacement".into())
        })?;
        let version = CredentialVersion::from_metadata(&metadata)?;
        fs::rename(&temporary, path).map_err(|_| {
            ProviderError::Transport("could not atomically replace ChatGPT plan credentials".into())
        })?;
        #[cfg(unix)]
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| {
                ProviderError::Transport("could not sync ChatGPT plan credential directory".into())
            })?;
        Ok(version)
    })();
    drop(file);
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;
