use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;
use gateway_core::ProviderError;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};
use tokio::sync::Mutex;

use crate::http::map_reqwest_error;

const DEFAULT_OAUTH_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const METADATA_TOKEN_URL: &str =
    "http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token";
pub const CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";

#[derive(Debug, Clone)]
pub struct AccessToken {
    pub token: String,
    pub expires_at: OffsetDateTime,
}

impl AccessToken {
    #[must_use]
    pub fn is_usable(&self, refresh_before: Duration) -> bool {
        self.expires_at > OffsetDateTime::now_utc() + refresh_before
    }
}

#[async_trait]
pub trait AccessTokenSource: Send + Sync {
    async fn fetch_token(&self) -> Result<AccessToken, ProviderError>;
}

#[derive(Clone)]
pub struct CachedAccessTokenSource {
    inner: Arc<dyn AccessTokenSource>,
    cached: Arc<Mutex<Option<AccessToken>>>,
    refresh_before: Duration,
}

impl CachedAccessTokenSource {
    #[must_use]
    pub fn new(inner: Arc<dyn AccessTokenSource>) -> Self {
        Self {
            inner,
            cached: Arc::new(Mutex::new(None)),
            refresh_before: Duration::seconds(60),
        }
    }

    pub async fn token(&self) -> Result<String, ProviderError> {
        let mut guard = self.cached.lock().await;

        if let Some(token) = &*guard
            && token.is_usable(self.refresh_before)
        {
            return Ok(token.token.clone());
        }

        let fresh = self.inner.fetch_token().await?;
        let token = fresh.token.clone();
        *guard = Some(fresh);

        Ok(token)
    }
}

pub struct StaticBearerTokenSource {
    token: String,
}

impl StaticBearerTokenSource {
    #[must_use]
    pub fn new(token: String) -> Self {
        Self { token }
    }
}

#[async_trait]
impl AccessTokenSource for StaticBearerTokenSource {
    async fn fetch_token(&self) -> Result<AccessToken, ProviderError> {
        Ok(AccessToken {
            token: self.token.clone(),
            expires_at: OffsetDateTime::now_utc() + Duration::days(365 * 10),
        })
    }
}

pub struct ServiceAccountTokenSource {
    credentials_path: PathBuf,
    scope: String,
    client: reqwest::Client,
}

impl ServiceAccountTokenSource {
    pub fn new(credentials_path: PathBuf, scope: String) -> Result<Self, ProviderError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .map_err(map_reqwest_error)?;

        Ok(Self {
            credentials_path,
            scope,
            client,
        })
    }
}

#[async_trait]
impl AccessTokenSource for ServiceAccountTokenSource {
    async fn fetch_token(&self) -> Result<AccessToken, ProviderError> {
        let raw = fs::read_to_string(&self.credentials_path).map_err(|error| {
            ProviderError::Transport(format!(
                "failed to read service account credentials `{}`: {error}",
                self.credentials_path.display()
            ))
        })?;

        let credentials: ServiceAccountCredentials =
            serde_json::from_str(&raw).map_err(|error| {
                ProviderError::Transport(format!(
                    "invalid service account credentials JSON: {error}"
                ))
            })?;

        if credentials.kind != "service_account" {
            return Err(ProviderError::InvalidRequest(format!(
                "credentials file `{}` is not a service_account credential",
                self.credentials_path.display()
            )));
        }

        fetch_service_account_token(&self.client, &credentials, &self.scope).await
    }
}

pub struct AdcTokenSource {
    scope: String,
    client: reqwest::Client,
}

impl AdcTokenSource {
    pub fn new(scope: String) -> Result<Self, ProviderError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .map_err(map_reqwest_error)?;

        Ok(Self { scope, client })
    }
}

#[async_trait]
impl AccessTokenSource for AdcTokenSource {
    async fn fetch_token(&self) -> Result<AccessToken, ProviderError> {
        if let Some(path) = env::var("GOOGLE_APPLICATION_CREDENTIALS")
            .ok()
            .map(PathBuf::from)
        {
            return self.fetch_from_adc_file(&path).await;
        }

        if let Some(path) = default_adc_file_path() {
            return self.fetch_from_adc_file(&path).await;
        }

        fetch_metadata_server_token(&self.client, &self.scope).await
    }
}

impl AdcTokenSource {
    async fn fetch_from_adc_file(&self, path: &Path) -> Result<AccessToken, ProviderError> {
        let raw = fs::read_to_string(path).map_err(|error| {
            ProviderError::Transport(format!(
                "failed to read ADC credentials file `{}`: {error}",
                path.display()
            ))
        })?;

        let detected_type = serde_json::from_str::<serde_json::Value>(&raw)
            .ok()
            .and_then(|value| {
                value
                    .get("type")
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
            });

        match detected_type.as_deref() {
            Some("service_account") => {
                let credentials: ServiceAccountCredentials =
                    serde_json::from_str(&raw).map_err(|error| {
                        ProviderError::Transport(format!(
                            "invalid service account ADC credentials JSON: {error}"
                        ))
                    })?;
                fetch_service_account_token(&self.client, &credentials, &self.scope).await
            }
            Some("authorized_user") => {
                let credentials: AuthorizedUserCredentials =
                    serde_json::from_str(&raw).map_err(|error| {
                        ProviderError::Transport(format!(
                            "invalid authorized_user ADC credentials JSON: {error}"
                        ))
                    })?;
                fetch_authorized_user_token(&self.client, &credentials, &self.scope).await
            }
            Some(other) => Err(ProviderError::InvalidRequest(format!(
                "unsupported ADC credential type `{other}`"
            ))),
            None => Err(ProviderError::InvalidRequest(format!(
                "ADC credential file `{}` is missing `type`",
                path.display()
            ))),
        }
    }
}

fn default_adc_file_path() -> Option<PathBuf> {
    let home = env::var("HOME").ok()?;
    let path = PathBuf::from(home).join(".config/gcloud/application_default_credentials.json");
    if path.exists() { Some(path) } else { None }
}

#[derive(Debug, Deserialize)]
struct ServiceAccountCredentials {
    #[serde(rename = "type")]
    kind: String,
    client_email: String,
    private_key: String,
    #[serde(default)]
    private_key_id: Option<String>,
    #[serde(default)]
    token_uri: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AuthorizedUserCredentials {
    #[serde(rename = "type")]
    kind: String,
    client_id: String,
    client_secret: String,
    refresh_token: String,
    #[serde(default)]
    token_uri: Option<String>,
}

#[derive(Debug, Serialize)]
struct ServiceAccountClaims<'a> {
    iss: &'a str,
    scope: &'a str,
    aud: &'a str,
    iat: i64,
    exp: i64,
}

#[derive(Debug, Deserialize)]
struct OAuthAccessTokenResponse {
    access_token: String,
    #[serde(default)]
    expires_in: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct MetadataTokenResponse {
    access_token: String,
    expires_in: i64,
}

async fn fetch_service_account_token(
    client: &reqwest::Client,
    credentials: &ServiceAccountCredentials,
    scope: &str,
) -> Result<AccessToken, ProviderError> {
    let token_uri = credentials
        .token_uri
        .clone()
        .unwrap_or_else(|| DEFAULT_OAUTH_TOKEN_URL.to_string());
    let now = OffsetDateTime::now_utc();

    let claims = ServiceAccountClaims {
        iss: &credentials.client_email,
        scope,
        aud: &token_uri,
        iat: now.unix_timestamp(),
        exp: (now + Duration::hours(1)).unix_timestamp(),
    };

    let mut header = Header::new(Algorithm::RS256);
    if let Some(key_id) = &credentials.private_key_id {
        header.kid = Some(key_id.clone());
    }

    let key = EncodingKey::from_rsa_pem(credentials.private_key.as_bytes()).map_err(|error| {
        ProviderError::Transport(format!(
            "failed to parse service account private key: {error}"
        ))
    })?;

    let assertion = jsonwebtoken::encode(&header, &claims, &key).map_err(|error| {
        ProviderError::Transport(format!("failed to sign service account JWT: {error}"))
    })?;

    let response = client
        .post(&token_uri)
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
            ("assertion", assertion.as_str()),
        ])
        .send()
        .await
        .map_err(map_reqwest_error)?;

    parse_oauth_token_response(response).await
}

async fn fetch_authorized_user_token(
    client: &reqwest::Client,
    credentials: &AuthorizedUserCredentials,
    scope: &str,
) -> Result<AccessToken, ProviderError> {
    if credentials.kind != "authorized_user" {
        return Err(ProviderError::InvalidRequest(
            "expected authorized_user credentials".to_string(),
        ));
    }

    let token_uri = credentials
        .token_uri
        .clone()
        .unwrap_or_else(|| DEFAULT_OAUTH_TOKEN_URL.to_string());

    let response = client
        .post(token_uri)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", credentials.refresh_token.as_str()),
            ("client_id", credentials.client_id.as_str()),
            ("client_secret", credentials.client_secret.as_str()),
            ("scope", scope),
        ])
        .send()
        .await
        .map_err(map_reqwest_error)?;

    parse_oauth_token_response(response).await
}

async fn fetch_metadata_server_token(
    client: &reqwest::Client,
    scope: &str,
) -> Result<AccessToken, ProviderError> {
    let response = client
        .get(METADATA_TOKEN_URL)
        .query(&[("scopes", scope)])
        .header("Metadata-Flavor", "Google")
        .send()
        .await
        .map_err(map_reqwest_error)?;

    let status = response.status();
    let text = response.text().await.map_err(map_reqwest_error)?;
    if !status.is_success() {
        return Err(ProviderError::UpstreamHttp {
            status: status.as_u16(),
            body: text,
        });
    }

    let parsed: MetadataTokenResponse = serde_json::from_str(&text).map_err(|error| {
        ProviderError::Transport(format!("invalid metadata token response: {error}"))
    })?;

    Ok(AccessToken {
        token: parsed.access_token,
        expires_at: OffsetDateTime::now_utc() + Duration::seconds(parsed.expires_in.into()),
    })
}

async fn parse_oauth_token_response(
    response: reqwest::Response,
) -> Result<AccessToken, ProviderError> {
    let status = response.status();
    let text = response.text().await.map_err(map_reqwest_error)?;

    if !status.is_success() {
        return Err(ProviderError::UpstreamHttp {
            status: status.as_u16(),
            body: text,
        });
    }

    let parsed: OAuthAccessTokenResponse = serde_json::from_str(&text).map_err(|error| {
        ProviderError::Transport(format!("invalid OAuth token response: {error}"))
    })?;

    let expires_in = parsed.expires_in.unwrap_or(3600).max(60);
    Ok(AccessToken {
        token: parsed.access_token,
        expires_at: OffsetDateTime::now_utc() + Duration::seconds(expires_in),
    })
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        path::PathBuf,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use async_trait::async_trait;
    use axum::{Json, Router, routing::post};
    use serde_json::json;
    use serial_test::serial;
    use tempfile::tempdir;
    use tokio::net::TcpListener;

    use super::{
        AccessToken, AccessTokenSource, AdcTokenSource, CachedAccessTokenSource, Duration,
        OffsetDateTime, ServiceAccountTokenSource, StaticBearerTokenSource,
    };
    use crate::token::CLOUD_PLATFORM_SCOPE;

    const TEST_RSA_PRIVATE_KEY: &str = r#"-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQCo1WHWzHdosbKK
WTlCf4nJS2wANN4n/lXEc+7E/2OEoS8co9upp4NVgH0wcLjfSYXz1bmrCdnj7ppW
Vbg+mZpW4r9JKncCTtKjXq2Qt/a4tYj6WxakcLk42pnhAx8PNbmYHp2bZhPv4eJf
BqZKu5nXyZ6BFwoIWkj/3MGIq+e7pA3bGEa/vW3U+1YRAA3+0WsE5abPcJKMLgF6
1PFfjKuw9x//yenMHkL4gjZOINac1nHyTQI/4Km/84IyztNBwyxncGwW4g/x0Neb
nSgOKs8Nndg3Rh82R6IkOEbH8Eopgs2y0/7rBPrqSHuaPwoDbo9ryOx/bBhhF40g
yDHRzteFAgMBAAECggEADt8a2uWQSBXM9QBGeavLyoIc/Yiqn+m4itEucUZQsQxU
nsh1LyS8/hFPFa78LdjnVnLXQ7Bes8Pe7udmjwcPMAORl2OI75hbV/4dOj/mGN+O
tQTEghAW1KH2x3nzqK6SDkr2FYvgijMCsl2e1LrhIn+VOWg630D6qKT8nCoOQ7ob
G2aCWsrdFLkGnF/OyzN9HvA1cIi2QSloLYX0cxfoa5nILevbzPL5JFYphzXF7V3T
OSzueatw54K9o7Pywn6zb5pG/fri9jxFugojZlSG08vnamaFJjdjW/k76DVBgLi+
hlmvOmQ08hdIk4q05L9OzEbSZgw0bOPFCe/PECVRAQKBgQDlf1zRI0uI6YKxhDX3
B2hNWQuiqk2CXy5qb8EH+3omFTmDrShnhjdvYOtxUs8Sys+/W8h8ONOfnR1moBtI
ysoNno6E1AtpL0563CJhRo9H+XT8spwMMFolgQy37Eg/Jh/6be9/B4t5HUUVl9C4
m9IHV1DGrYLnW7UF5mcMISEghQKBgQC8VJwKM76m9ADQvtZ6H9SvimCzhhq/Hcqi
9uTzPqaBPWg7S5ErJCGDydHd68vBZbiqlVuYqNogdJq+WTxZzuy00/Wk87U6XGno
MS2pNpZdi5Lpzm/vIvNe18K9ZBSdlCS9mgzibDJ13pvMUvkw9NWXlhlqktW2rTGS
tzW+SagLAQKBgCNcdn67A357DGoxxubjO0z/tW1A9GRsKgi4Y3PJac7IYm5Jlfot
kgkVU/HIIqPwoAYKLGAHmYP0f306mjmjFXL3xVnuGjwA0ATaOmnmp1kdtMri8mxm
Xt18fusv+wnP5AmAOvDFxtXIjsZ+9+gaCkibSZTzU0I2vTPFhoc165bJAoGBALGZ
rKkmUPWKhzZTsVjrqZt9CGJj5dczFgQGhrQo8cZRDXlVcunXIc/xQ/tewQB5l+Mu
BHn7SfBvZfp5lqMusyR3+l/6/32w5qLztZashrJizEG2zvIZ6J4ZJGmL9rD/ooI2
w03HMPLc4dmWqa6URNS11PQe0nF59JTiN0lilpkBAoGBAJSUsh5qGGyAE+gHZXYn
yPy48bBninSJZBa7aUm5PxbZLLG5FQoyBDZPUyOvsKJc7UBjpwDe0jMkJmjpvW+r
GgkfTd4qdOaEI8ljZxJM7plf5ZHfJND9xz+SJ3PqpNejzDeD4xQkwKAzeMQyl1z6
UQ2sSTSfuLHz2F1jr5+pRNL2
-----END PRIVATE KEY-----"#;

    #[tokio::test]
    async fn static_token_source_passthrough() {
        let source = StaticBearerTokenSource::new("tok-123".to_string());
        let token = source.fetch_token().await.expect("token");
        assert_eq!(token.token, "tok-123");
    }

    #[tokio::test]
    async fn cached_source_reuses_token_until_refresh_window() {
        struct CountingSource {
            calls: Arc<AtomicUsize>,
            expires_in: i64,
        }

        #[async_trait]
        impl AccessTokenSource for CountingSource {
            async fn fetch_token(&self) -> Result<AccessToken, gateway_core::ProviderError> {
                let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
                Ok(AccessToken {
                    token: format!("tok-{call}"),
                    expires_at: OffsetDateTime::now_utc() + Duration::seconds(self.expires_in),
                })
            }
        }

        let calls = Arc::new(AtomicUsize::new(0));
        let source = Arc::new(CountingSource {
            calls: calls.clone(),
            expires_in: 3600,
        });
        let cached = CachedAccessTokenSource::new(source);

        let a = cached.token().await.expect("token a");
        let b = cached.token().await.expect("token b");

        assert_eq!(a, "tok-1");
        assert_eq!(b, "tok-1");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cached_source_refreshes_near_expiry() {
        struct CountingSource {
            calls: Arc<AtomicUsize>,
        }

        #[async_trait]
        impl AccessTokenSource for CountingSource {
            async fn fetch_token(&self) -> Result<AccessToken, gateway_core::ProviderError> {
                let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
                Ok(AccessToken {
                    token: format!("tok-{call}"),
                    expires_at: OffsetDateTime::now_utc() + Duration::seconds(30),
                })
            }
        }

        let calls = Arc::new(AtomicUsize::new(0));
        let source = Arc::new(CountingSource {
            calls: calls.clone(),
        });
        let cached = CachedAccessTokenSource::new(source);

        let _ = cached.token().await.expect("token a");
        let _ = cached.token().await.expect("token b");

        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn service_account_file_loading_path() {
        let app = Router::new().route(
            "/token",
            post(|| async { Json(json!({"access_token": "sa-token", "expires_in": 3600})) }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });

        let tmp = tempdir().expect("tempdir");
        let credentials_path = tmp.path().join("service-account.json");
        fs::write(
            &credentials_path,
            json!({
                "type": "service_account",
                "client_email": "gateway-test@example.iam.gserviceaccount.com",
                "private_key": TEST_RSA_PRIVATE_KEY,
                "token_uri": format!("http://{addr}/token")
            })
            .to_string(),
        )
        .expect("write credentials");

        let source = ServiceAccountTokenSource::new(
            PathBuf::from(&credentials_path),
            CLOUD_PLATFORM_SCOPE.to_string(),
        )
        .expect("source");
        let token = source.fetch_token().await.expect("token");
        assert_eq!(token.token, "sa-token");
    }

    #[tokio::test]
    #[serial]
    async fn adc_token_acquisition_path_from_authorized_user_file() {
        let app = Router::new().route(
            "/token",
            post(|| async { Json(json!({"access_token": "adc-token", "expires_in": 3600})) }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });

        let tmp = tempdir().expect("tempdir");
        let credentials_path = tmp.path().join("adc.json");
        fs::write(
            &credentials_path,
            json!({
                "type": "authorized_user",
                "client_id": "cid",
                "client_secret": "csecret",
                "refresh_token": "rtok",
                "token_uri": format!("http://{addr}/token")
            })
            .to_string(),
        )
        .expect("write adc");

        let old = env::var("GOOGLE_APPLICATION_CREDENTIALS").ok();
        unsafe {
            env::set_var(
                "GOOGLE_APPLICATION_CREDENTIALS",
                credentials_path.as_os_str(),
            )
        };

        let source = AdcTokenSource::new(CLOUD_PLATFORM_SCOPE.to_string()).expect("source");
        let token = source.fetch_token().await.expect("token");
        assert_eq!(token.token, "adc-token");

        if let Some(old) = old {
            unsafe { env::set_var("GOOGLE_APPLICATION_CREDENTIALS", old) };
        } else {
            unsafe { env::remove_var("GOOGLE_APPLICATION_CREDENTIALS") };
        }
    }
}
