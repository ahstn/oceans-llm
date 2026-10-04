use std::time::Duration;

use anyhow::{Context, bail};
use reqwest::{
    Method, Response, StatusCode,
    header::{AUTHORIZATION, HeaderValue},
};
use serde::{Serialize, de::DeserializeOwned};
use url::{Host, Url};

const MAX_JSON_BYTES: usize = 4 * 1024 * 1024;
const MAX_ERROR_BYTES: usize = 8 * 1024;

// This client owns transport only. Authorization and ownership stay on the server.
pub struct Client {
    http: reqwest::Client,
    base_url: Url,
    authorization: HeaderValue,
}

impl Client {
    pub fn new(mut base_url: Url, api_key: &str) -> anyhow::Result<Self> {
        if !matches!(base_url.scheme(), "http" | "https") || base_url.host_str().is_none() {
            bail!("gateway URL must use HTTP or HTTPS and have a host");
        }
        if !base_url.username().is_empty()
            || base_url.password().is_some()
            || base_url.query().is_some()
            || base_url.fragment().is_some()
        {
            bail!("gateway URL must not contain credentials, a query, or a fragment");
        }
        let plaintext = base_url.scheme() == "http";
        if plaintext {
            let loopback = match base_url.host() {
                Some(Host::Ipv4(address)) => address.is_loopback(),
                Some(Host::Ipv6(address)) => address.is_loopback(),
                _ => false,
            };
            if !loopback {
                bail!("gateway URL must use HTTPS unless the host is a loopback IP address");
            }
        }
        if api_key.trim().is_empty() {
            bail!("OCEANS_API_KEY is empty");
        }
        base_url
            .path_segments_mut()
            .map_err(|_| anyhow::anyhow!("invalid gateway URL"))?
            .pop_if_empty()
            .extend(["api", "v1", "skills"]);
        let mut authorization = HeaderValue::from_str(&format!("Bearer {}", api_key.trim()))
            .map_err(|_| anyhow::anyhow!("OCEANS_API_KEY contains invalid header characters"))?;
        authorization.set_sensitive(true);
        let http = reqwest::Client::builder()
            .user_agent(concat!("oceans/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            // Registry APIs serve archives directly. Never follow a credential-bearing redirect.
            .redirect(reqwest::redirect::Policy::none());
        // A proxy could forward a plaintext loopback request outside this machine.
        let http = if plaintext { http.no_proxy() } else { http };
        let http = http.build().context("could not initialize HTTP client")?;
        Ok(Self {
            http,
            base_url,
            authorization,
        })
    }

    fn request(&self, method: Method, path: &[&str]) -> reqwest::RequestBuilder {
        let mut url = self.base_url.clone();
        url.path_segments_mut()
            .expect("validated HTTP base URL")
            .extend(path.iter().copied());
        self.http
            .request(method, url)
            .header(AUTHORIZATION, self.authorization.clone())
    }

    pub async fn get<T: DeserializeOwned>(&self, path: &[&str]) -> anyhow::Result<T> {
        self.decode(self.request(Method::GET, path).send().await?)
            .await
    }

    pub async fn get_with_limit<T: DeserializeOwned>(
        &self,
        path: &[&str],
        max_bytes: usize,
    ) -> anyhow::Result<T> {
        let response = check_status(
            self.request(Method::GET, path).send().await?,
            &self.authorization,
        )
        .await?;
        serde_json::from_slice(&read_bounded(response, max_bytes).await?)
            .context("gateway returned an invalid JSON response")
    }

    pub async fn get_optional<T: DeserializeOwned>(
        &self,
        path: &[&str],
    ) -> anyhow::Result<Option<T>> {
        let response = self.request(Method::GET, path).send().await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        self.decode(response).await.map(Some)
    }

    pub async fn get_query<T: DeserializeOwned>(
        &self,
        path: &[&str],
        query: &[(&str, &str)],
    ) -> anyhow::Result<T> {
        self.decode(self.request(Method::GET, path).query(query).send().await?)
            .await
    }

    pub async fn post<T: DeserializeOwned>(
        &self,
        path: &[&str],
        body: &impl Serialize,
    ) -> anyhow::Result<T> {
        self.decode(self.request(Method::POST, path).json(body).send().await?)
            .await
    }

    pub async fn put<T: DeserializeOwned>(
        &self,
        path: &[&str],
        body: &impl Serialize,
    ) -> anyhow::Result<T> {
        self.decode(self.request(Method::PUT, path).json(body).send().await?)
            .await
    }

    pub async fn upload<T: DeserializeOwned>(
        &self,
        path: &[&str],
        archive: Vec<u8>,
    ) -> anyhow::Result<T> {
        self.decode(
            self.request(Method::POST, path)
                .header("content-type", "application/zip")
                .body(archive)
                .send()
                .await?,
        )
        .await
    }

    pub async fn download(&self, path: &[&str], max_bytes: usize) -> anyhow::Result<Vec<u8>> {
        let response = self.request(Method::GET, path).send().await?;
        read_bounded(
            check_status(response, &self.authorization).await?,
            max_bytes,
        )
        .await
    }

    async fn decode<T: DeserializeOwned>(&self, response: Response) -> anyhow::Result<T> {
        let response = check_status(response, &self.authorization).await?;
        let bytes = read_bounded(response, MAX_JSON_BYTES).await?;
        serde_json::from_slice(&bytes).context("gateway returned an invalid JSON response")
    }
}

async fn check_status(
    mut response: Response,
    authorization: &HeaderValue,
) -> anyhow::Result<Response> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        let count = chunk.len().min(MAX_ERROR_BYTES - bytes.len());
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.len() == MAX_ERROR_BYTES {
            break;
        }
    }
    let message: String = String::from_utf8_lossy(&bytes)
        .chars()
        .filter(|value| !value.is_control() || matches!(value, '\n' | '\t'))
        .collect();
    let key = authorization
        .to_str()
        .unwrap_or("")
        .strip_prefix("Bearer ")
        .unwrap_or("");
    let message = if key.is_empty() {
        message
    } else {
        message.replace(key, "[redacted]")
    };
    bail!("gateway returned HTTP {status}: {}", message.trim());
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;

async fn read_bounded(mut response: Response, max_bytes: usize) -> anyhow::Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        bail!("gateway response exceeds the {max_bytes}-byte limit");
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if chunk.len() > max_bytes.saturating_sub(bytes.len()) {
            bail!("gateway response exceeds the {max_bytes}-byte limit");
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
