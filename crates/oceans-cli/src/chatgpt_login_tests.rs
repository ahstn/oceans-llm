use super::*;
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use jsonwebtoken::{EncodingKey, Header, encode};
use serde_json::{Value, json};
use std::sync::Mutex as StdMutex;

// Public modulus for the throwaway RSA test fixture.
const MODULUS: &str = "qNVh1sx3aLGyilk5Qn-JyUtsADTeJ_5VxHPuxP9jhKEvHKPbqaeDVYB9MHC430mF89W5qwnZ4-6aVlW4PpmaVuK_SSp3Ak7So16tkLf2uLWI-lsWpHC5ONqZ4QMfDzW5mB6dm2YT7-HiXwamSruZ18megRcKCFpI_9zBiKvnu6QN2xhGv71t1PtWEQAN_tFrBOWmz3CSjC4BetTxX4yrsPcf_8npzB5C-II2TiDWnNZx8k0CP-Cpv_OCMs7TQcMsZ3BsFuIP8dDXm50oDirPDZ3YN0YfNkeiJDhGx_BKKYLNstP-6wT66kh7mj8KA26Pa8jsf2wYYReNIMgx0c7XhQ";
const TEST_PRIVATE_KEY: &str = include_str!("chatgpt_test_key.pem");

#[derive(Clone, Copy, PartialEq, Eq)]
enum Case {
    Good,
    State,
    Denied,
    Scope,
    Signature,
    Nonce,
    Subject,
    Client,
    InvalidGrant,
}

struct Fake {
    base: String,
    nonce: StdMutex<String>,
    case: Case,
    exchanges: StdMutex<usize>,
}

async fn discovery(State(fake): State<Arc<Fake>>) -> Json<Value> {
    Json(json!({"jwks_uri": format!("{}/jwks", fake.base)}))
}

async fn jwks(State(fake): State<Arc<Fake>>) -> Json<Value> {
    let modulus = if fake.case == Case::Signature {
        MODULUS.replace('q', "r")
    } else {
        MODULUS.to_owned()
    };
    Json(
        json!({"keys": [{"kty": "RSA", "kid": "test", "alg": "RS256", "use": "sig", "n": modulus, "e": "AQAB"}]}),
    )
}

async fn token(
    State(fake): State<Arc<Fake>>,
    axum::Form(form): axum::Form<HashMap<String, String>>,
) -> Response {
    *fake.exchanges.lock().unwrap() += 1;
    assert_eq!(form["client_id"], "oaiapp_test");
    assert_eq!(form["grant_type"], "authorization_code");
    assert_eq!(form["resource"], RESOURCE);
    if fake.case == Case::InvalidGrant {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid_grant", "error_description": "secret-verifier"})),
        )
            .into_response();
    }
    let nonce = fake.nonce.lock().unwrap().clone();
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("test".into());
    let id_token = encode(&header, &json!({
        "iss": ISSUER, "aud": "oaiapp_test", "exp": time::OffsetDateTime::now_utc().unix_timestamp() + 3600,
        "sub": if fake.case == Case::Subject { "other" } else { "subject" },
        "email": "user@example.com", "nonce": if fake.case == Case::Nonce { "wrong" } else { &nonce }
    }), &EncodingKey::from_rsa_pem(TEST_PRIVATE_KEY.as_bytes()).unwrap()).unwrap();
    Json(json!({"id_token": id_token, "access_token": "access", "refresh_token": "refresh", "token_type": "Bearer", "expires_in": 3600,
        "scope": if fake.case == Case::Scope { "openid email" } else { "chatgpt.tokens.use.direct email openid" }})).into_response()
}

async fn attempt(path: &Path, case: Case, returning: bool) -> (anyhow::Result<()>, Arc<Fake>) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let fake = Arc::new(Fake {
        base: base.clone(),
        nonce: StdMutex::new(String::new()),
        case,
        exchanges: StdMutex::new(0),
    });
    let app = Router::new()
        .route("/discovery", get(discovery))
        .route("/jwks", get(jwks))
        .route("/token", post(token))
        .with_state(fake.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let endpoints = Endpoints {
        authorize: Url::parse(&format!("{base}/authorize")).unwrap(),
        token: Url::parse(&format!("{base}/token")).unwrap(),
        discovery: Url::parse(&format!("{base}/discovery")).unwrap(),
    };
    let (ready, url) = oneshot::channel::<Url>();
    let drive = async {
        let authorize = url.await.unwrap();
        let params: HashMap<String, String> = authorize.query_pairs().into_owned().collect();
        assert_eq!(params["scope"], SCOPE);
        assert_eq!(params["response_type"], "code");
        assert_eq!(params["resource"], RESOURCE);
        assert_eq!(params["code_challenge_method"], "S256");
        if returning {
            assert_eq!(params["client_id"], "oaiapp_test");
            assert_eq!(params["ext_agent_host_id"], "urn:uuid:test");
            assert_eq!(params["id_token_hint"], "old-id-token");
            assert_eq!(params["login_hint"], "user@example.com");
            assert!(!params.contains_key("agent_name_hint"));
        } else {
            assert_eq!(params["client_id"], "dynamic_agent_client");
            assert_eq!(params["agent_name_hint"], "Oceans");
            assert!(params["ext_agent_host_id"].starts_with("urn:uuid:"));
        }
        *fake.nonce.lock().unwrap() = params["nonce"].clone();
        let mut callback = Url::parse(&params["redirect_uri"]).unwrap();
        assert_eq!(callback.host_str(), Some("127.0.0.1"));
        assert_eq!(callback.path(), "/auth/callback");
        callback
            .query_pairs_mut()
            .append_pair(
                "state",
                if case == Case::State {
                    "wrong"
                } else {
                    &params["state"]
                },
            )
            .append_pair("code", "test-code")
            .append_pair(
                "client_id",
                if case == Case::Client {
                    "oaiapp_other"
                } else {
                    "oaiapp_test"
                },
            );
        if case == Case::Denied {
            callback
                .query_pairs_mut()
                .append_pair("error", "access_denied");
        }
        reqwest::get(callback)
            .await
            .unwrap()
            .error_for_status()
            .unwrap();
    };
    let (result, ()) = tokio::join!(login(path, 0, true, true, &endpoints, Some(ready)), drive);
    server.abort();
    (result, fake)
}

fn existing(path: &Path) -> Vec<u8> {
    let value = json!({"email":"user@example.com", "subject":"subject", "client_id":"oaiapp_test", "ext_agent_host_id":"urn:uuid:test", "id_token":"old-id-token", "future_field": 42});
    let bytes = serde_json::to_vec(&value).unwrap();
    fs::write(path, &bytes).unwrap();
    bytes
}

#[tokio::test]
async fn registration_and_reauthorization() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("private/credential.json");
    let (result, _) = attempt(&path, Case::Good, false).await;
    result.unwrap();
    let value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(value["client_id"], "oaiapp_test");
    assert_eq!(value["subject"], "subject");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    existing(&path);
    let (result, _) = attempt(&path, Case::Good, true).await;
    result.unwrap();
    let value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(value["future_field"], 42);
    assert_eq!(value["ext_agent_host_id"], "urn:uuid:test");
}

#[tokio::test]
async fn reauthorization_omits_empty_hints_after_terminal_failure() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("credential.json");
    for (id_token, email) in [("", ""), ("old-id-token", ""), ("", "user@example.com")] {
        let value = json!({
            "client_id": "oaiapp_test",
            "ext_agent_host_id": "urn:uuid:test",
            "subject": "subject",
            "id_token": id_token,
            "email": email,
            "access_token": "",
            "refresh_token": "",
            "expires_in": 0
        });
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let pending = prepare_login(&path, 0, &Endpoints::default())
            .await
            .unwrap();
        let params: HashMap<_, _> = pending.authorize.query_pairs().into_owned().collect();
        assert_eq!(params["client_id"], "oaiapp_test");
        assert_eq!(
            params.get("id_token_hint").map(String::as_str),
            (!id_token.is_empty()).then_some(id_token)
        );
        assert_eq!(
            params.get("login_hint").map(String::as_str),
            (!email.is_empty()).then_some(email)
        );
        assert!(!params.contains_key("agent_name_hint"));
    }
}

#[tokio::test]
async fn failures_never_replace_credentials() {
    for case in [
        Case::State,
        Case::Denied,
        Case::Scope,
        Case::Signature,
        Case::Nonce,
        Case::Subject,
        Case::Client,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credential.json");
        let before = existing(&path);
        let (result, fake) = attempt(&path, case, true).await;
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        if matches!(case, Case::State | Case::Denied | Case::Client) {
            assert_eq!(*fake.exchanges.lock().unwrap(), 0);
        }
    }
}

#[tokio::test]
async fn first_registration_failures_leave_no_file() {
    for case in [
        Case::State,
        Case::Denied,
        Case::Scope,
        Case::Signature,
        Case::Nonce,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credential.json");
        let (result, _) = attempt(&path, case, false).await;
        assert!(result.is_err());
        assert!(!path.exists());
    }
}

#[tokio::test]
async fn token_errors_report_code_without_description() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("credential.json");
    let (result, _) = attempt(&path, Case::InvalidGrant, false).await;
    let error = format!("{:#}", result.unwrap_err());
    assert!(error.contains("invalid_grant"));
    assert!(error.contains("start a new login"));
    assert!(!error.contains("secret-verifier"));
    assert!(!path.exists());
}
