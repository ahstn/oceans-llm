use std::{collections::BTreeMap, net::IpAddr};

use gateway_core::{ExternalMcpAuthMode, ExternalMcpServerRecord, GatewayError};
use serde_json::{Map, Value};
use url::Url;

const DISCOVERY_SECRET_ENV_PREFIX: &str = "OCEANS_MCP_DISCOVERY_";

pub fn validate_mcp_auth_config(
    auth_mode: ExternalMcpAuthMode,
    auth_config: &Map<String, Value>,
) -> Result<(), GatewayError> {
    match auth_mode {
        ExternalMcpAuthMode::None => ensure_allowed_auth_fields(auth_config, &[]),
        ExternalMcpAuthMode::GatewayStaticHeader => {
            ensure_allowed_auth_fields(auth_config, &["header_name", "secret_ref"])?;
            validate_static_header_name(required_string(auth_config, "header_name")?)?;
            validate_secret_ref(required_secret_ref(auth_config)?)?;
            Ok(())
        }
        ExternalMcpAuthMode::GatewayBearerToken => {
            ensure_allowed_auth_fields(auth_config, &["secret_ref"])?;
            validate_secret_ref(required_secret_ref(auth_config)?)?;
            Ok(())
        }
        ExternalMcpAuthMode::UserPassthrough => {
            ensure_allowed_auth_fields(auth_config, &["header", "token_type"])
        }
        ExternalMcpAuthMode::OauthObo => {
            ensure_allowed_auth_fields(auth_config, &["token_exchange", "token_type"])
        }
    }
}

pub fn validate_gateway_managed_server_url(
    value: &str,
    auth_mode: ExternalMcpAuthMode,
) -> Result<(), GatewayError> {
    if !auth_mode.supports_gateway_discovery() || auth_mode == ExternalMcpAuthMode::None {
        return Ok(());
    }
    let url = Url::parse(value)
        .map_err(|error| GatewayError::InvalidRequest(format!("server_url is invalid: {error}")))?;
    if url.scheme() != "https" && !is_loopback_http_url(&url) {
        return Err(GatewayError::InvalidRequest(
            "gateway-managed MCP credentials require an https server_url unless the host is loopback"
                .to_string(),
        ));
    }
    Ok(())
}

fn is_loopback_http_url(url: &Url) -> bool {
    if url.scheme() != "http" {
        return false;
    }
    url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    })
}

pub fn gateway_mcp_upstream_headers(
    server: &ExternalMcpServerRecord,
) -> Result<Option<BTreeMap<String, String>>, GatewayError> {
    validate_gateway_managed_server_url(&server.server_url, server.auth_mode)?;
    match server.auth_mode {
        ExternalMcpAuthMode::None => Ok(None),
        ExternalMcpAuthMode::GatewayStaticHeader => {
            let header_name = required_string(&server.auth_config, "header_name")?;
            let secret = resolve_secret_ref(required_secret_ref(&server.auth_config)?)?;
            Ok(Some(BTreeMap::from([(header_name.to_string(), secret)])))
        }
        ExternalMcpAuthMode::GatewayBearerToken => {
            let secret = resolve_secret_ref(required_secret_ref(&server.auth_config)?)?;
            Ok(Some(BTreeMap::from([(
                "Authorization".to_string(),
                format!("Bearer {secret}"),
            )])))
        }
        ExternalMcpAuthMode::UserPassthrough | ExternalMcpAuthMode::OauthObo => Ok(None),
    }
}

pub fn normalize_mcp_server_key(value: &str) -> Result<String, GatewayError> {
    let key = value.trim().to_ascii_lowercase();
    if !(3..=64).contains(&key.len()) {
        return Err(GatewayError::InvalidRequest(
            "server_key must be 3-64 characters".to_string(),
        ));
    }
    if !key.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
    }) {
        return Err(GatewayError::InvalidRequest(
            "server_key may only contain lowercase letters, digits, hyphen, and underscore"
                .to_string(),
        ));
    }
    Ok(key)
}

fn ensure_allowed_auth_fields(
    auth_config: &Map<String, Value>,
    allowed_fields: &[&str],
) -> Result<(), GatewayError> {
    for key in auth_config.keys() {
        if !allowed_fields.contains(&key.as_str()) {
            return Err(GatewayError::InvalidRequest(format!(
                "auth_config.{key} is not allowed for this auth mode"
            )));
        }
    }
    Ok(())
}

fn required_string<'a>(
    auth_config: &'a Map<String, Value>,
    field: &str,
) -> Result<&'a str, GatewayError> {
    auth_config
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| GatewayError::InvalidRequest(format!("auth_config.{field} is required")))
}

fn required_secret_ref(auth_config: &Map<String, Value>) -> Result<&str, GatewayError> {
    required_string(auth_config, "secret_ref")
}

fn validate_static_header_name(header_name: &str) -> Result<&str, GatewayError> {
    if header_name.trim() != header_name {
        return Err(GatewayError::InvalidRequest(
            "auth_config.header_name must not contain leading or trailing whitespace".to_string(),
        ));
    }
    reqwest::header::HeaderName::from_bytes(header_name.as_bytes()).map_err(|error| {
        GatewayError::InvalidRequest(format!("auth_config.header_name is invalid: {error}"))
    })?;
    Ok(header_name)
}

fn validate_secret_ref(secret_ref: &str) -> Result<&str, GatewayError> {
    if secret_ref.trim() != secret_ref {
        return Err(GatewayError::InvalidRequest(
            "auth_config.secret_ref must not contain leading or trailing whitespace".to_string(),
        ));
    }
    let env_name = secret_env_name(secret_ref)?;
    if !env_name.starts_with(DISCOVERY_SECRET_ENV_PREFIX) {
        return Err(GatewayError::InvalidRequest(format!(
            "secret_ref environment variable must start with {DISCOVERY_SECRET_ENV_PREFIX}"
        )));
    }
    Ok(secret_ref)
}

fn resolve_secret_ref(secret_ref: &str) -> Result<String, GatewayError> {
    validate_secret_ref(secret_ref)?;
    let env_name = secret_env_name(secret_ref)?;
    std::env::var(env_name).map_err(|_| {
        GatewayError::InvalidRequest(format!(
            "secret_ref env/{env_name} is not available for MCP use"
        ))
    })
}

fn secret_env_name(secret_ref: &str) -> Result<&str, GatewayError> {
    let env_name = secret_ref.strip_prefix("env/").ok_or_else(|| {
        GatewayError::InvalidRequest(
            "secret_ref must reference an environment variable as env/NAME".to_string(),
        )
    })?;
    if env_name.is_empty() {
        return Err(GatewayError::InvalidRequest(
            "secret_ref environment variable name cannot be empty".to_string(),
        ));
    }
    if !env_name
        .bytes()
        .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(GatewayError::InvalidRequest(
            "secret_ref environment variable name may only contain uppercase letters, digits, and underscore".to_string(),
        ));
    }
    Ok(env_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_core::{ExternalMcpAuthMode, ExternalMcpServerStatus, ExternalMcpTransport};
    use serde_json::{Map, json};
    use time::OffsetDateTime;
    use uuid::Uuid;

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(key);
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(previous) => unsafe {
                    std::env::set_var(self.key, previous);
                },
                None => unsafe {
                    std::env::remove_var(self.key);
                },
            }
        }
    }

    #[test]
    fn gateway_managed_auth_requires_secret_refs() {
        let mode = ExternalMcpAuthMode::GatewayBearerToken;
        let mut config = Map::new();
        assert!(validate_mcp_auth_config(mode, &config).is_err());
        config.insert("secret_ref".to_string(), json!("env/MCP_TOKEN"));
        assert!(validate_mcp_auth_config(mode, &config).is_err());
        config.insert(
            "secret_ref".to_string(),
            json!("env/OCEANS_MCP_DISCOVERY_MCP_TOKEN"),
        );
        assert!(validate_mcp_auth_config(mode, &config).is_ok());
    }

    #[test]
    fn gateway_static_header_auth_validates_header_name() {
        let mode = ExternalMcpAuthMode::GatewayStaticHeader;
        let mut config = Map::from_iter([
            ("header_name".to_string(), json!(" X-Api-Key ")),
            (
                "secret_ref".to_string(),
                json!("env/OCEANS_MCP_DISCOVERY_API_KEY"),
            ),
        ]);
        assert!(validate_mcp_auth_config(mode, &config).is_err());

        config.insert("header_name".to_string(), json!("bad header"));
        assert!(validate_mcp_auth_config(mode, &config).is_err());

        config.insert("header_name".to_string(), json!("X-Api-Key"));
        assert!(validate_mcp_auth_config(mode, &config).is_ok());
    }

    #[test]
    fn upstream_header_resolver_only_returns_configured_gateway_credentials() {
        let mut config = Map::from_iter([
            ("header_name".to_string(), json!("X-Upstream-Key")),
            (
                "secret_ref".to_string(),
                json!("env/OCEANS_MCP_DISCOVERY_STATIC_HEADER_TEST_KEY"),
            ),
        ]);
        let _env_guard = EnvVarGuard::set(
            "OCEANS_MCP_DISCOVERY_STATIC_HEADER_TEST_KEY",
            "upstream-secret",
        );
        let server = server_record(ExternalMcpAuthMode::GatewayStaticHeader, config.clone());
        let headers = gateway_mcp_upstream_headers(&server)
            .expect("headers")
            .expect("some headers");
        assert_eq!(
            headers.get("X-Upstream-Key"),
            Some(&"upstream-secret".to_string())
        );
        assert!(!headers.contains_key("Authorization"));
        assert!(!headers.contains_key("x-oceans-api-key"));

        config.clear();
        let server = server_record(ExternalMcpAuthMode::None, config);
        assert!(
            gateway_mcp_upstream_headers(&server)
                .expect("headers")
                .is_none()
        );
    }

    #[test]
    fn upstream_header_resolver_revalidates_gateway_managed_https() {
        let _env_guard = EnvVarGuard::set(
            "OCEANS_MCP_DISCOVERY_BEARER_URL_TEST_KEY",
            "upstream-secret",
        );
        let mut server = server_record(
            ExternalMcpAuthMode::GatewayBearerToken,
            Map::from_iter([(
                "secret_ref".to_string(),
                json!("env/OCEANS_MCP_DISCOVERY_BEARER_URL_TEST_KEY"),
            )]),
        );
        server.server_url = "http://example.test/mcp".to_string();

        let error = gateway_mcp_upstream_headers(&server).expect_err("http must fail");
        assert_eq!(error.error_code(), "invalid_request");
    }

    fn server_record(
        auth_mode: ExternalMcpAuthMode,
        auth_config: Map<String, Value>,
    ) -> ExternalMcpServerRecord {
        let now = OffsetDateTime::now_utc();
        ExternalMcpServerRecord {
            mcp_server_id: Uuid::new_v4(),
            server_key: "github".to_string(),
            display_name: "GitHub".to_string(),
            description: None,
            transport: ExternalMcpTransport::StreamableHttp,
            server_url: "https://example.test/mcp".to_string(),
            auth_mode,
            auth_config,
            timeout_ms: 30_000,
            status: ExternalMcpServerStatus::Active,
            last_discovery_status: None,
            last_discovery_at: None,
            last_successful_discovery_at: None,
            last_error_summary: None,
            last_tool_count: None,
            created_at: now,
            updated_at: now,
            disabled_at: None,
        }
    }
}
