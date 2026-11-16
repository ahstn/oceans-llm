//! Sign in with ChatGPT's public-client registration and authorization flow.

use std::{collections::HashMap, fs, io::Write, net::Ipv4Addr, path::Path, sync::Arc};

use anyhow::{Context, anyhow, bail, ensure};
use axum::{
    Router,
    extract::{Query, State},
    response::Html,
    routing::get,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::{
    net::TcpListener,
    sync::{Mutex, oneshot},
};
use url::Url;
use uuid::Uuid;

use crate::{cli::ChatgptLoginArgs, config::Env};

const ISSUER: &str = "https://auth.openai.com";
const RESOURCE: &str = "https://api.openai.com/v1";
const SCOPE: &str = "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
const DIRECT: &str = "chatgpt.tokens.use.direct";

struct Endpoints {
    authorize: Url,
    token: Url,
    discovery: Url,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            authorize: Url::parse("https://auth.openai.com/api/accounts/authorize").unwrap(),
            token: Url::parse("https://auth.openai.com/api/accounts/oauth/token").unwrap(),
            discovery: Url::parse("https://auth.openai.com/.well-known/openid-configuration")
                .unwrap(),
        }
    }
}

#[derive(Deserialize)]
struct Existing {
    client_id: String,
    ext_agent_host_id: String,
    id_token: String,
    email: String,
    subject: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    id_token: String,
    access_token: String,
    refresh_token: String,
    token_type: String,
    expires_in: u64,
    scope: String,
}

#[derive(Deserialize)]
struct Claims {
    sub: String,
    email: String,
    nonce: String,
}

#[derive(Deserialize)]
struct Discovery {
    jwks_uri: String,
}

#[derive(Serialize)]
struct Report<'a> {
    email: &'a str,
    subject: &'a str,
    client_id: &'a str,
    scopes: &'a [String],
    path: &'a Path,
}

type Callback = Arc<Mutex<Option<oneshot::Sender<HashMap<String, String>>>>>;

async fn callback(
    State(sender): State<Callback>,
    Query(query): Query<HashMap<String, String>>,
) -> Html<&'static str> {
    if let Some(sender) = sender.lock().await.take() {
        let _ = sender.send(query);
    }
    Html("<!doctype html><title>Oceans sign-in</title><p>You may return to the Oceans CLI.</p>")
}

pub async fn run(args: ChatgptLoginArgs, env: &Env, json: bool) -> anyhow::Result<()> {
    let path = match args.output {
        Some(path) => path,
        None => env
            .dir_var("XDG_CONFIG_HOME")
            .or_else(|| env.home().map(|home| home.join(".config")))
            .context("could not find a config directory; set HOME or XDG_CONFIG_HOME")?
            .join("oceans/chatgpt-plan/credential.json"),
    };
    login(
        &path,
        args.port,
        args.no_browser,
        json,
        &Endpoints::default(),
        None,
    )
    .await
}

struct PendingLogin {
    existing_value: Option<serde_json::Value>,
    existing: Option<Existing>,
    host_id: String,
    state: String,
    nonce: String,
    verifier: String,
    redirect: String,
    authorize: Url,
    listener: TcpListener,
}

struct Authorization<'a> {
    code: &'a str,
    client_id: &'a str,
}

async fn login(
    path: &Path,
    port: u16,
    no_browser: bool,
    json: bool,
    endpoints: &Endpoints,
    ready: Option<oneshot::Sender<Url>>,
) -> anyhow::Result<()> {
    let mut pending = prepare_login(path, port, endpoints).await?;
    let existing_value = pending.existing_value.take();
    let query = wait_for_callback(pending.listener, &pending.authorize, no_browser, ready).await?;
    let authorization = validate_callback(&query, &pending.state, pending.existing.as_ref())?;
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let tokens = exchange_code(
        &http,
        endpoints,
        &authorization,
        &pending.verifier,
        &pending.redirect,
    )
    .await?;
    let scopes: Vec<String> = tokens.scope.split_whitespace().map(str::to_owned).collect();
    ensure!(
        scopes.iter().any(|scope| scope == DIRECT),
        "ChatGPT plan usage was not granted (missing chatgpt.tokens.use.direct scope)"
    );
    let claims = verify_id_token(
        &http,
        endpoints,
        &tokens.id_token,
        authorization.client_id,
        &pending.nonce,
    )
    .await?;
    if let Some(old) = &pending.existing {
        ensure!(
            claims.sub == old.subject,
            "ChatGPT account subject does not match existing credential"
        );
    }
    save_login(
        path,
        existing_value,
        &pending.host_id,
        &authorization,
        &tokens,
        &claims,
        &scopes,
    )?;
    print_report(path, &authorization, &claims, &scopes, json)
}

async fn prepare_login(
    path: &Path,
    port: u16,
    endpoints: &Endpoints,
) -> anyhow::Result<PendingLogin> {
    let existing_value: Option<serde_json::Value> = match fs::read(path) {
        Ok(bytes) => Some(
            serde_json::from_slice(&bytes).context("invalid existing ChatGPT credential file")?,
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error).context("could not read existing ChatGPT credential file"),
    };
    let existing: Option<Existing> = existing_value
        .as_ref()
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()
        .context("invalid existing ChatGPT credential file")?;
    if let Some(old) = &existing {
        ensure!(
            old.client_id.starts_with("oaiapp_") && old.client_id.len() > 7,
            "existing credential has an invalid issued client_id"
        );
    }
    let host_id = existing.as_ref().map_or_else(
        || format!("urn:uuid:{}", Uuid::new_v4()),
        |old| old.ext_agent_host_id.clone(),
    );
    let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?
        }
        Err(error) => return Err(error).context("could not start callback listener"),
    };
    let redirect = format!(
        "http://127.0.0.1:{}/auth/callback",
        listener.local_addr()?.port()
    );
    let state = random_value();
    let nonce = random_value();
    let verifier = random_value();
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut authorize = endpoints.authorize.clone();
    {
        let mut query = authorize.query_pairs_mut();
        query
            .append_pair("response_type", "code")
            .append_pair(
                "client_id",
                existing
                    .as_ref()
                    .map_or("dynamic_agent_client", |old| old.client_id.as_str()),
            )
            .append_pair("redirect_uri", &redirect)
            .append_pair("scope", SCOPE)
            .append_pair("resource", RESOURCE)
            .append_pair("state", &state)
            .append_pair("nonce", &nonce)
            .append_pair("code_challenge_method", "S256")
            .append_pair("code_challenge", &challenge)
            .append_pair("ext_agent_host_id", &host_id);
        if let Some(old) = &existing {
            if !old.id_token.is_empty() {
                query.append_pair("id_token_hint", &old.id_token);
            }
            if !old.email.is_empty() {
                query.append_pair("login_hint", &old.email);
            }
        } else {
            query.append_pair("agent_name_hint", "Oceans");
        }
    }
    Ok(PendingLogin {
        existing_value,
        existing,
        host_id,
        state,
        nonce,
        verifier,
        redirect,
        authorize,
        listener,
    })
}

async fn wait_for_callback(
    listener: TcpListener,
    authorize: &Url,
    no_browser: bool,
    ready: Option<oneshot::Sender<Url>>,
) -> anyhow::Result<HashMap<String, String>> {
    let (sender, receiver) = oneshot::channel();
    let app = Router::new()
        .route("/auth/callback", get(callback))
        .with_state(Arc::new(Mutex::new(Some(sender))));
    let (shutdown, shutdown_rx) = oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
    });
    eprintln!("Open this URL to sign in with ChatGPT:\n{authorize}");
    if let Some(ready) = ready {
        let _ = ready.send(authorize.clone());
    }
    if !no_browser {
        let command = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        if let Err(error) = std::process::Command::new(command)
            .arg(authorize.as_str())
            .spawn()
        {
            eprintln!("Could not open the browser ({error}); open the URL above manually.");
        }
    }
    let result = match tokio::time::timeout(std::time::Duration::from_secs(600), receiver).await {
        Ok(result) => result.context("callback listener stopped before authorization"),
        Err(_) => {
            server.abort();
            bail!(
                "ChatGPT authorization timed out waiting for the browser callback; start a new login"
            );
        }
    };
    let _ = shutdown.send(());
    server.await.context("callback server failed")??;
    result
}

fn validate_callback<'a>(
    query: &'a HashMap<String, String>,
    state: &str,
    existing: Option<&'a Existing>,
) -> anyhow::Result<Authorization<'a>> {
    ensure!(
        query.get("state").is_some_and(|actual| actual == state),
        "authorization callback state mismatch"
    );
    if let Some(error) = query.get("error") {
        bail!("ChatGPT authorization failed: {error}");
    }
    let code = query
        .get("code")
        .filter(|code| !code.is_empty())
        .context("authorization callback has no code")?;
    let supplied_id = query.get("client_id");
    let client_id = if let Some(old) = existing {
        ensure!(
            supplied_id.is_none_or(|id| id == &old.client_id),
            "authorization callback changed the registered client_id"
        );
        &old.client_id
    } else {
        supplied_id
            .filter(|id| id.starts_with("oaiapp_") && id.len() > 7)
            .context("registration callback did not return an issued client_id")?
    };
    Ok(Authorization { code, client_id })
}

async fn exchange_code(
    http: &reqwest::Client,
    endpoints: &Endpoints,
    authorization: &Authorization<'_>,
    verifier: &str,
    redirect: &str,
) -> anyhow::Result<TokenResponse> {
    let response = http
        .post(endpoints.token.clone())
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", authorization.client_id),
            ("code", authorization.code),
            ("code_verifier", verifier),
            ("redirect_uri", redirect),
            ("resource", RESOURCE),
        ])
        .send()
        .await
        .context("ChatGPT token exchange failed; start a new login")?;
    if !response.status().is_success() {
        let status = response.status();
        let body: serde_json::Value = response.json().await.unwrap_or_default();
        let error = body.get("error").and_then(serde_json::Value::as_str);
        let error = match error {
            Some(
                "invalid_request"
                | "invalid_client"
                | "invalid_grant"
                | "unauthorized_client"
                | "unsupported_grant_type"
                | "invalid_scope"
                | "server_error"
                | "temporarily_unavailable",
            ) => error.unwrap(),
            _ => "unknown_oauth_error",
        };
        bail!("ChatGPT token exchange failed (HTTP {status}, {error}); start a new login");
    }
    response
        .json()
        .await
        .context("invalid ChatGPT token response")
}

fn save_login(
    path: &Path,
    existing_value: Option<serde_json::Value>,
    host_id: &str,
    authorization: &Authorization<'_>,
    tokens: &TokenResponse,
    claims: &Claims,
    scopes: &[String],
) -> anyhow::Result<()> {
    let mut credential = existing_value.unwrap_or_else(|| serde_json::json!({}));
    let object = credential
        .as_object_mut()
        .context("existing credential must be a JSON object")?;
    let saved_at =
        time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339)?;
    for (key, value) in [
        ("email", serde_json::json!(claims.email)),
        ("issuer", serde_json::json!(ISSUER)),
        ("subject", serde_json::json!(claims.sub)),
        ("client_id", serde_json::json!(authorization.client_id)),
        ("ext_agent_host_id", serde_json::json!(host_id)),
        ("id_token", serde_json::json!(tokens.id_token)),
        ("access_token", serde_json::json!(tokens.access_token)),
        ("refresh_token", serde_json::json!(tokens.refresh_token)),
        ("token_type", serde_json::json!(tokens.token_type)),
        ("expires_in", serde_json::json!(tokens.expires_in)),
        ("scopes", serde_json::json!(scopes)),
        ("saved_at", serde_json::json!(saved_at)),
    ] {
        object.insert(key.to_owned(), value);
    }
    write_credential(path, &credential)
}

fn print_report(
    path: &Path,
    authorization: &Authorization<'_>,
    claims: &Claims,
    scopes: &[String],
    json: bool,
) -> anyhow::Result<()> {
    let report = Report {
        email: &claims.email,
        subject: &claims.sub,
        client_id: authorization.client_id,
        scopes,
        path,
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "Email: {}\nSubject: {}\nClient ID: {}\nScopes: {}\nSaved: {}",
            report.email,
            report.subject,
            report.client_id,
            report.scopes.join(" "),
            path.display()
        );
    }
    Ok(())
}

fn random_value() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

async fn verify_id_token(
    http: &reqwest::Client,
    endpoints: &Endpoints,
    token: &str,
    client_id: &str,
    nonce: &str,
) -> anyhow::Result<Claims> {
    let discovery: Discovery = http
        .get(endpoints.discovery.clone())
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
        .context("invalid OpenID discovery document")?;
    let keys: JwkSet = http
        .get(discovery.jwks_uri)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
        .context("invalid OpenID JWKS")?;
    let header = decode_header(token).context("invalid ID token header")?;
    ensure!(
        header.alg == Algorithm::RS256,
        "unsupported ID token signing algorithm"
    );
    let kid = header.kid.context("ID token has no key ID")?;
    let key = keys
        .find(&kid)
        .ok_or_else(|| anyhow!("ID token signing key not found"))?;
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_issuer(&[ISSUER]);
    validation.set_audience(&[client_id]);
    validation.leeway = 0;
    let claims = decode::<Claims>(token, &DecodingKey::from_jwk(key)?, &validation)
        .context("ID token verification failed")?
        .claims;
    ensure!(claims.nonce == nonce, "ID token nonce mismatch");
    ensure!(
        !claims.sub.is_empty() && !claims.email.is_empty(),
        "ID token has empty identity claims"
    );
    Ok(claims)
}

fn write_credential(path: &Path, credential: &serde_json::Value) -> anyhow::Result<()> {
    let directory = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    create_private_dir(directory)?;
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    serde_json::to_writer(&mut file, credential)?;
    file.write_all(b"\n")?;
    file.as_file().sync_all()?;
    #[cfg(all(test, unix))]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            file.as_file().metadata()?.permissions().mode() & 0o777,
            0o600
        );
    }
    file.persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("could not write {}", path.display()))?;
    Ok(())
}

#[cfg(unix)]
fn create_private_dir(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn create_private_dir(path: &Path) -> std::io::Result<()> {
    fs::create_dir_all(path)
}

#[cfg(test)]
#[path = "chatgpt_login_tests.rs"]
mod tests;
