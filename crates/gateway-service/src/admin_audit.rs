//! The single boundary for writing admin audit entries.
//!
//! Callers describe a change with an [`AuditEvent`] and hand it to [`AdminAudit`],
//! usually through [`AdminAudit::audited`], which runs the mutation and writes the
//! entry only when the mutation succeeds. Every summary and metadata value passes
//! through [`redact_audit_value`] before it is stored. See
//! `docs/adr/2026-10-15-admin-audit-log.md`.

use std::{
    future::Future,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

use gateway_core::{AuditActor, AuditLogRepository, NewAuditLogRecord};
use percent_encoding::percent_decode_str;
use serde::Serialize;
use serde_json::{Map, Value};
use time::OffsetDateTime;

use crate::redaction::is_sensitive_json_key;

pub const AUDIT_REDACTED: &str = "[REDACTED]";

/// Target labels longer than this are truncated.
const MAX_LABEL_CHARS: usize = 256;

/// Serialized summaries larger than this are replaced with a size marker.
const MAX_SUMMARY_BYTES: usize = 16 * 1024;

/// Field-name fragments that mark a value as secret, in addition to the request
/// log redaction keys.
const SECRET_KEY_FRAGMENTS: &[&str] = &[
    "secret",
    "password",
    "passwd",
    "token",
    "authorization",
    "cookie",
    "private_key",
    "api_key",
    "ciphertext",
    "nonce",
    "key_material",
    "invite_url",
];

/// Whole `_`-separated name segments that mark a secret, such as `github_pat`.
/// Matched as segments so names like `path` stay readable.
const SECRET_KEY_SEGMENTS: &[&str] = &["pat", "pwd", "passphrase"];

/// Field-name suffixes that describe a secret without holding it, such as
/// `api_key_id` or `token_expires_at`.
const SAFE_KEY_SUFFIXES: &[&str] = &[
    "_id",
    "_ids",
    "_at",
    "_kind",
    "_mode",
    "_type",
    "_status",
    "_count",
    "_prefix",
    "_configured",
    "_present",
    "_source",
    "_scope",
    "_ref",
];

/// Action names. Use `<target_type>.<past-tense verb>` for new actions.
pub mod actions {
    pub const API_KEY_CREATED: &str = "api_key.created";
    pub const API_KEY_UPDATED: &str = "api_key.updated";
    pub const API_KEY_REVOKED: &str = "api_key.revoked";
    pub const API_KEY_SECRET_REVEALED: &str = "api_key.secret_revealed";

    pub const USER_CREATED: &str = "user.created";
    pub const USER_UPDATED: &str = "user.updated";
    pub const USER_DEACTIVATED: &str = "user.deactivated";
    pub const USER_REACTIVATED: &str = "user.reactivated";
    pub const USER_ONBOARDING_RESET: &str = "user.onboarding_reset";
    pub const USER_INVITE_ISSUED: &str = "user.invite_issued";
    pub const USER_INVITE_REDEEMED: &str = "user.invite_redeemed";
    pub const USER_PASSWORD_CHANGED: &str = "user.password_changed";
    pub const USER_PROVISIONED: &str = "user.provisioned";
    /// Moves a user between teams; the target is the user.
    pub const USER_TEAM_TRANSFERRED: &str = "user.team_transferred";

    pub const PROVIDER_CREDENTIAL_SET: &str = "provider_credential.set";
    pub const PROVIDER_CREDENTIAL_DELETED: &str = "provider_credential.deleted";

    pub const SESSION_CREATED: &str = "session.created";
    pub const SESSION_REVOKED: &str = "session.revoked";

    pub const TEAM_CREATED: &str = "team.created";
    pub const TEAM_UPDATED: &str = "team.updated";
    pub const TEAM_MEMBERS_ADDED: &str = "team.members_added";
    pub const TEAM_MEMBER_REMOVED: &str = "team.member_removed";

    pub const SERVICE_ACCOUNT_CREATED: &str = "service_account.created";
    pub const SERVICE_ACCOUNT_UPDATED: &str = "service_account.updated";
    pub const SERVICE_ACCOUNT_DISABLED: &str = "service_account.disabled";

    pub const BUDGET_SET: &str = "budget.set";
    pub const BUDGET_DEACTIVATED: &str = "budget.deactivated";
    pub const BUDGET_INCREASE_CREATED: &str = "budget.increase_created";

    pub const MCP_SERVER_CREATED: &str = "mcp_server.created";
    pub const MCP_SERVER_UPDATED: &str = "mcp_server.updated";
    pub const MCP_SERVER_DISABLED: &str = "mcp_server.disabled";
    pub const MCP_SERVER_DISCOVERY_REFRESHED: &str = "mcp_server.discovery_refreshed";

    pub const MCP_TOOLSET_CREATED: &str = "mcp_toolset.created";
    pub const MCP_TOOLSET_UPDATED: &str = "mcp_toolset.updated";
    pub const MCP_TOOLSET_DISABLED: &str = "mcp_toolset.disabled";
    pub const MCP_TOOLSET_TOOLS_REPLACED: &str = "mcp_toolset.tools_replaced";

    pub const MCP_GRANT_SET: &str = "mcp_grant.set";
    pub const MCP_GRANT_REVOKED: &str = "mcp_grant.revoked";

    pub const MCP_CREDENTIAL_BINDING_SET: &str = "mcp_credential_binding.set";
    pub const MCP_CREDENTIAL_BINDING_REVOKED: &str = "mcp_credential_binding.revoked";

    pub const PRICING_CATALOG_REFRESHED: &str = "pricing_catalog.refreshed";

    pub const REVIEW_AGENT_REPOSITORY_CREATED: &str = "review_agent_repository.created";
    pub const REVIEW_AGENT_REPOSITORY_UPDATED: &str = "review_agent_repository.updated";
    pub const REVIEW_AGENT_REPOSITORY_DISABLED: &str = "review_agent_repository.disabled";
    pub const REVIEW_AGENT_REPOSITORY_REACTIVATED: &str = "review_agent_repository.reactivated";

    pub const RATE_LIMIT_POLICY_CREATED: &str = "rate_limit_policy.created";
    pub const RATE_LIMIT_POLICY_UPDATED: &str = "rate_limit_policy.updated";
    pub const RATE_LIMIT_POLICY_DELETED: &str = "rate_limit_policy.deleted";
}

/// Target resource types. An action's prefix names its target type.
pub mod targets {
    pub const API_KEY: &str = "api_key";
    pub const USER: &str = "user";
    pub const PROVIDER_CREDENTIAL: &str = "provider_credential";
    pub const SESSION: &str = "session";
    pub const TEAM: &str = "team";
    pub const SERVICE_ACCOUNT: &str = "service_account";
    pub const BUDGET: &str = "budget";
    pub const MCP_SERVER: &str = "mcp_server";
    pub const MCP_TOOLSET: &str = "mcp_toolset";
    pub const MCP_GRANT: &str = "mcp_grant";
    pub const MCP_CREDENTIAL_BINDING: &str = "mcp_credential_binding";
    pub const PRICING_CATALOG: &str = "pricing_catalog";
    pub const REVIEW_AGENT_REPOSITORY: &str = "review_agent_repository";
    pub const RATE_LIMIT_POLICY: &str = "rate_limit_policy";

    /// Every target type, for filter controls.
    pub const ALL: &[&str] = &[
        API_KEY,
        USER,
        PROVIDER_CREDENTIAL,
        SESSION,
        TEAM,
        SERVICE_ACCOUNT,
        BUDGET,
        MCP_SERVER,
        MCP_TOOLSET,
        MCP_GRANT,
        MCP_CREDENTIAL_BINDING,
        PRICING_CATALOG,
        REVIEW_AGENT_REPOSITORY,
        RATE_LIMIT_POLICY,
    ];
}

/// Who is acting, and where the request came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditContext {
    pub actor: AuditActor,
    pub request_ip: Option<String>,
    pub user_agent: Option<String>,
    /// The `x-request-id` of the admin request, kept in the entry's metadata.
    pub request_id: Option<String>,
}

impl AuditContext {
    #[must_use]
    pub fn new(actor: AuditActor) -> Self {
        Self {
            actor,
            request_ip: None,
            user_agent: None,
            request_id: None,
        }
    }

    #[must_use]
    pub fn with_request(
        mut self,
        request_ip: Option<String>,
        user_agent: Option<String>,
        request_id: Option<String>,
    ) -> Self {
        self.request_ip = request_ip;
        self.user_agent = user_agent;
        self.request_id = request_id;
        self
    }
}

/// One audited change. Build it with the target, then add summaries and metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct AuditEvent {
    action: String,
    target_type: String,
    target_id: Option<String>,
    target_label: Option<String>,
    before: Option<Value>,
    after: Option<Value>,
    metadata: Map<String, Value>,
}

impl AuditEvent {
    #[must_use]
    pub fn new(action: &str, target_type: &str) -> Self {
        Self {
            action: action.to_string(),
            target_type: target_type.to_string(),
            target_id: None,
            target_label: None,
            before: None,
            after: None,
            metadata: Map::new(),
        }
    }

    #[must_use]
    pub fn target(mut self, target_id: impl ToString) -> Self {
        self.target_id = Some(target_id.to_string());
        self
    }

    /// A display name for the target, kept so the entry reads well after deletion.
    #[must_use]
    pub fn label(mut self, target_label: impl Into<String>) -> Self {
        self.target_label = Some(target_label.into());
        self
    }

    /// The target's state before the change. Pass a summary, never a raw record
    /// that holds secrets; redaction is a safety net, not the contract.
    #[must_use]
    pub fn before(mut self, summary: impl Serialize) -> Self {
        self.before = Some(to_value(summary));
        self
    }

    #[must_use]
    pub fn after(mut self, summary: impl Serialize) -> Self {
        self.after = Some(to_value(summary));
        self
    }

    #[must_use]
    pub fn meta(mut self, key: &str, value: impl Serialize) -> Self {
        self.metadata.insert(key.to_string(), to_value(value));
        self
    }

    #[must_use]
    pub fn action(&self) -> &str {
        &self.action
    }

    fn into_record(
        mut self,
        context: &AuditContext,
        occurred_at: OffsetDateTime,
    ) -> NewAuditLogRecord {
        if let Some(request_id) = &context.request_id {
            self.metadata
                .insert("request_id".to_string(), Value::String(request_id.clone()));
        }
        NewAuditLogRecord {
            occurred_at,
            actor: AuditActor {
                label: context.actor.label.clone().map(redact_label),
                ..context.actor.clone()
            },
            action: self.action,
            target_type: self.target_type,
            // Some ids embed caller input, such as a budget scope's upstream model.
            target_id: self.target_id.map(redact_label),
            target_label: self.target_label.map(redact_label),
            before: self.before.map(redact_summary),
            after: self.after.map(redact_summary),
            metadata: redact_metadata(self.metadata),
            request_ip: context.request_ip.clone(),
            user_agent: context.user_agent.clone().map(redact_scalar),
        }
    }
}

/// Tracks whether a mutation made of several non-atomic writes has changed
/// anything yet. Call [`AuditProgress::landed`] after each write that succeeds.
#[derive(Debug, Default)]
pub struct AuditProgress {
    landed: AtomicBool,
    target_id: OnceLock<String>,
}

impl AuditProgress {
    pub fn landed(&self) {
        self.landed.store(true, Ordering::SeqCst);
    }

    /// Marks progress and remembers the target the first write created.
    pub fn landed_on(&self, target_id: impl ToString) {
        let _ = self.target_id.set(target_id.to_string());
        self.landed();
    }

    #[must_use]
    pub fn target_id(&self) -> Option<&str> {
        self.target_id.get().map(String::as_str)
    }

    #[must_use]
    pub fn has_landed(&self) -> bool {
        self.landed.load(Ordering::SeqCst)
    }
}

/// Writes admin audit entries.
pub struct AdminAudit<R: ?Sized> {
    repo: Arc<R>,
}

impl<R> AdminAudit<R>
where
    R: AuditLogRepository + ?Sized,
{
    #[must_use]
    pub fn new(repo: Arc<R>) -> Self {
        Self { repo }
    }

    /// Runs `mutation` and, only if it succeeds, writes the entry `describe`
    /// builds from its output. A failed mutation writes nothing.
    pub async fn audited<T, E, Fut, F>(
        &self,
        context: &AuditContext,
        mutation: Fut,
        describe: F,
    ) -> Result<T, E>
    where
        Fut: Future<Output = Result<T, E>>,
        F: FnOnce(&T) -> AuditEvent,
    {
        let output = mutation.await?;
        self.record(context, describe(&output)).await;
        Ok(output)
    }

    /// Like [`AdminAudit::audited`], for a mutation that reports whether it changed
    /// anything. `Ok(false)` is a no-op, such as revoking a grant that is already
    /// inactive, and records nothing.
    pub async fn audited_change<E, Fut, F>(
        &self,
        context: &AuditContext,
        mutation: Fut,
        describe: F,
    ) -> Result<bool, E>
    where
        Fut: Future<Output = Result<bool, E>>,
        F: FnOnce() -> AuditEvent,
    {
        let changed = mutation.await?;
        if changed {
            self.record(context, describe()).await;
        }
        Ok(changed)
    }

    /// Like [`AdminAudit::audited`], for a mutation made of several writes that are
    /// not in one transaction. If it fails after `progress` reports a write that
    /// landed, the change is partly applied, so `partial` describes it and the entry
    /// is marked `outcome: partial`. A failure before any write records nothing.
    ///
    /// `describe` may return `None` when the mutation succeeded without changing
    /// anything, such as a repeated revoke, so nothing is recorded.
    pub async fn audited_steps<T, E, Fut, F, D, P>(
        &self,
        context: &AuditContext,
        progress: &AuditProgress,
        mutation: Fut,
        describe: F,
        partial: P,
    ) -> Result<T, E>
    where
        Fut: Future<Output = Result<T, E>>,
        F: FnOnce(&T) -> D,
        D: Into<Option<AuditEvent>>,
        P: FnOnce(&AuditProgress, &E) -> AuditEvent,
    {
        match mutation.await {
            Ok(output) => {
                if let Some(event) = describe(&output).into() {
                    self.record(context, event).await;
                }
                Ok(output)
            }
            Err(error) => {
                if progress.has_landed() {
                    let event = partial(progress, &error).meta("outcome", "partial");
                    self.record(context, event).await;
                }
                Err(error)
            }
        }
    }

    /// Writes one entry for a change that has already succeeded. Returns the
    /// entry's id, or `None` if the write failed.
    ///
    /// The change is already committed, so a failed write is logged rather than
    /// returned: failing the request would invite a retry that repeats the change.
    pub async fn record(&self, context: &AuditContext, event: AuditEvent) -> Option<i64> {
        let record = event.into_record(context, now_millis());
        match self.repo.insert_audit_log(&record).await {
            Ok(id) => Some(id),
            Err(error) => {
                tracing::error!(
                    audit_action = %record.action,
                    audit_target_type = %record.target_type,
                    audit_target_id = record.target_id.as_deref(),
                    actor_kind = record.actor.kind.as_str(),
                    actor_user_id = ?record.actor.user_id,
                    error = %error,
                    "failed to write admin audit entry"
                );
                None
            }
        }
    }
}

/// Replaces secret-looking values in an audit summary with [`AUDIT_REDACTED`].
///
/// A value is secret when its field name looks secret (see
/// [`is_sensitive_audit_key`]) or the string itself looks like a credential.
/// Booleans and nulls are kept, so flags such as `secret_configured` survive.
#[must_use]
pub fn redact_audit_value(value: Value) -> Value {
    match value {
        // Key/value pairs such as entity tags name their field in `key`.
        Value::Object(mut map) if is_key_value_pair(&map) => {
            let key = map.get("key").and_then(Value::as_str).unwrap_or_default();
            // A token pasted in as a key is itself the secret.
            let key_is_credential = looks_like_credential(key);
            let sensitive = key_is_credential || is_secret_name(key);
            if key_is_credential {
                map.insert("key".to_string(), Value::String(AUDIT_REDACTED.to_string()));
            }
            if let Some(value) = map.remove("value") {
                let value = if sensitive && !is_flag(&value) {
                    Value::String(AUDIT_REDACTED.to_string())
                } else {
                    redact_audit_value(value)
                };
                map.insert("value".to_string(), value);
            }
            Value::Object(map)
        }
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| {
                    let value = if is_sensitive_audit_key(&key) && !is_flag(&value) {
                        Value::String(AUDIT_REDACTED.to_string())
                    } else {
                        redact_audit_value(value)
                    };
                    (key, value)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(redact_audit_value).collect()),
        Value::String(text) if looks_like_credential(&text) => {
            Value::String(AUDIT_REDACTED.to_string())
        }
        other => other,
    }
}

#[must_use]
pub fn is_sensitive_audit_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace('-', "_");
    if SAFE_KEY_SUFFIXES
        .iter()
        .any(|suffix| normalized.ends_with(suffix))
    {
        return false;
    }
    is_secret_name(&normalized)
}

/// Whether a name looks secret, without the safe-suffix allowance. Tag keys are
/// user-entered, so `password_ref` must not pass as a reference.
fn is_secret_name(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace('-', "_");
    is_sensitive_json_key(&normalized)
        || SECRET_KEY_FRAGMENTS
            .iter()
            .any(|fragment| normalized.contains(fragment))
        || normalized
            .split('_')
            .any(|segment| SECRET_KEY_SEGMENTS.contains(&segment))
}

fn is_key_value_pair(map: &Map<String, Value>) -> bool {
    map.len() == 2 && map.get("key").is_some_and(Value::is_string) && map.contains_key("value")
}

/// Prefixes of widely used provider tokens that carry no field name to judge by.
const TOKEN_PREFIXES: &[&str] = &[
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "github_pat_",
    "glpat-",
    "xoxb-",
    "xoxp-",
    "xoxa-",
    "sk-ant-",
    "sk-proj-",
    "sk_live_",
    "rk_live_",
    "AKIA",
];

fn is_flag(value: &Value) -> bool {
    matches!(value, Value::Bool(_) | Value::Null)
}

fn looks_like_credential(text: &str) -> bool {
    if looks_like_credential_as_written(text) {
        return true;
    }
    // `?api%5Fkey=…` hides a secret name behind percent-encoding, and a nested
    // URL can encode it twice (`api%255Fkey`), so each decoding is checked in
    // turn, up to a few layers.
    let mut current = text.to_string();
    for _ in 0..MAX_PERCENT_DECODES {
        if !current.contains('%') {
            return false;
        }
        let decoded = percent_decode_str(&current)
            .decode_utf8_lossy()
            .into_owned();
        if decoded == current {
            return false;
        }
        if looks_like_credential_as_written(&decoded) {
            return true;
        }
        current = decoded;
    }
    false
}

/// How many layers of percent-encoding the credential check looks through.
const MAX_PERCENT_DECODES: usize = 4;

fn looks_like_credential_as_written(text: &str) -> bool {
    // Split at anything that cannot be part of a token, so `token=gwk_…`,
    // `(ghp_…)` and `auth=Bearer …` are checked from the token's first character.
    let words: Vec<&str> = text
        .split(|character: char| {
            !(character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '-'))
        })
        .filter(|word| !word.is_empty())
        .collect();
    // `Bearer` and `Basic` also start ordinary phrases such as "bearer token" or
    // "Basic tier", so a short value counts only when it looks encoded.
    let scheme_with_value = words.windows(2).any(|pair| {
        (pair[0].eq_ignore_ascii_case("bearer") || pair[0].eq_ignore_ascii_case("basic"))
            && (pair[1].len() >= 8 || looks_encoded(pair[1]))
    });
    // A full gateway key is `gwk_<public id>.<secret>`; the bare `gwk_<public id>`
    // prefix is a display identifier and stays readable. Invite links carry their
    // token in the path.
    scheme_with_value
        || text.contains("/invite/")
        || has_url_userinfo(text)
        || has_secret_assignment(text)
        || words.iter().any(|word| is_token_word(word))
}

/// A digit or a capital after the first letter, as in `dTpw` (`u:p` in base64).
fn looks_encoded(word: &str) -> bool {
    word.chars().any(|character| character.is_ascii_digit())
        || word
            .chars()
            .skip(1)
            .any(|character| character.is_ascii_uppercase())
}

/// Whether the text assigns a value to a secret-looking name, such as
/// `password=hunter2`, `?api_key=…` or `Authorization: Basic …`.
fn has_secret_assignment(text: &str) -> bool {
    text.match_indices(['=', ':']).any(|(index, _)| {
        // `password = hunter2` puts spaces between the name and the separator,
        // and `{"password":"hunter2"}` a closing quote.
        let name = text[..index]
            .trim_end()
            .trim_end_matches(['"', '\''])
            .rsplit(|character: char| {
                !(character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
            })
            .next()
            .unwrap_or_default();
        // A quoted value counts once it has something inside the quotes.
        let mut value = text[index + 1..].trim_start().chars();
        let has_value = match value.next() {
            Some(quote @ ('"' | '\'')) => value.next().is_some_and(|next| next != quote),
            Some(character) => !matches!(character, '&' | ';' | ','),
            None => false,
        };
        !name.is_empty() && has_value && is_secret_name(name)
    })
}

/// Whether the text holds a URL with userinfo, such as `https://user:pass@host`
/// or `https://<token>@host`. Dedicated URL fields strip it; this catches URLs
/// written into free text.
fn has_url_userinfo(text: &str) -> bool {
    text.match_indices("://").any(|(index, _)| {
        let authority = text[index + 3..]
            .split(|character: char| {
                matches!(character, '/' | '?' | '#') || character.is_whitespace()
            })
            .next()
            .unwrap_or_default();
        authority.find('@').is_some_and(|at| at > 0)
    })
}

fn is_token_word(word: &str) -> bool {
    (word.starts_with("gwk_") && word.contains('.'))
        || TOKEN_PREFIXES
            .iter()
            .any(|prefix| word.starts_with(prefix) && word.len() > prefix.len() + 8)
}

/// Whether a URL path segment looks like an embedded credential: a known token
/// shape, or a long random-looking value. UUIDs are identifiers and are kept.
/// The segment is percent-decoded first, so an encoded token is caught too.
#[must_use]
pub fn is_secret_path_segment(segment: &str) -> bool {
    let decoded = percent_decode_str(segment).decode_utf8_lossy();
    let segment = decoded.as_ref();
    if is_token_word(segment) || looks_like_credential(segment) {
        return true;
    }
    // Only the hyphenated form is an identifier; 32 bare hex digits are as
    // likely to be a key.
    if segment.len() == 36 && uuid::Uuid::parse_str(segment).is_ok() {
        return false;
    }
    if segment.len() < 32 {
        return false;
    }
    // Letters, digits, and the URL-safe and base64 symbols tokens are built from.
    let token_charset = segment.chars().all(|character| {
        character.is_ascii_alphanumeric()
            || matches!(character, '_' | '-' | '.' | '~' | '+' | '/' | '=')
    });
    let mixed = segment.chars().any(|character| character.is_ascii_digit())
        && segment
            .chars()
            .any(|character| character.is_ascii_alphabetic());
    // A 32+ character run of only letters or only digits is not a path word or
    // a numeric ID; hyphenated words such as `oauth-authorization-server` stay
    // readable.
    let unbroken_letters = segment
        .chars()
        .all(|character| character.is_ascii_alphabetic());
    let unbroken_digits = segment.chars().all(|character| character.is_ascii_digit());
    token_charset && (mixed || unbroken_letters || unbroken_digits)
}

/// A caller-supplied scalar such as a user agent: redacted when it looks like a credential.
fn redact_scalar(text: String) -> String {
    if looks_like_credential(&text) {
        AUDIT_REDACTED.to_string()
    } else {
        text
    }
}

/// Labels are user-entered names, so they get the same credential check as summary
/// strings and a length cap.
fn redact_label(label: String) -> String {
    if looks_like_credential(&label) {
        return AUDIT_REDACTED.to_string();
    }
    if label.chars().count() > MAX_LABEL_CHARS {
        let mut truncated: String = label.chars().take(MAX_LABEL_CHARS).collect();
        truncated.push('…');
        return truncated;
    }
    label
}

fn redact_summary(value: Value) -> Value {
    let value = redact_audit_value(value);
    let size = serde_json::to_vec(&value).map_or(0, |bytes| bytes.len());
    if size > MAX_SUMMARY_BYTES {
        serde_json::json!({ "truncated": true, "bytes": size })
    } else {
        value
    }
}

/// Metadata keys that say what kind of entry this is. They survive truncation so
/// an oversized partial entry still reads as partial.
const RESERVED_METADATA_KEYS: [&str; 3] = ["outcome", "error", "request_id"];
const MAX_RESERVED_METADATA_BYTES: usize = 256;

fn redact_metadata(metadata: Map<String, Value>) -> Value {
    let reserved = RESERVED_METADATA_KEYS
        .iter()
        .filter_map(|&key| Some((key, metadata.get(key)?.clone())))
        .collect::<Vec<_>>();
    let mut value = redact_summary(Value::Object(metadata));
    if value.get("truncated") == Some(&Value::Bool(true))
        && let Value::Object(truncated) = &mut value
    {
        for (key, kept) in reserved {
            let kept = redact_audit_value(kept);
            let small = serde_json::to_vec(&kept)
                .is_ok_and(|bytes| bytes.len() <= MAX_RESERVED_METADATA_BYTES);
            if small && !kept.is_object() && !kept.is_array() {
                truncated.insert(key.to_string(), kept);
            }
        }
    }
    value
}

fn to_value(value: impl Serialize) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// Audit timestamps are stored in milliseconds, which the list cursor relies on.
fn now_millis() -> OffsetDateTime {
    let now = OffsetDateTime::now_utc();
    now.replace_nanosecond(now.nanosecond() / 1_000_000 * 1_000_000)
        .unwrap_or(now)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use gateway_core::{AuditLogPage, AuditLogQuery, AuditLogRecord, StoreError};
    use serde_json::json;
    use uuid::Uuid;

    use super::*;

    #[derive(Default)]
    struct RecordingRepo {
        records: Mutex<Vec<NewAuditLogRecord>>,
        fail: bool,
    }

    #[async_trait]
    impl AuditLogRepository for RecordingRepo {
        async fn insert_audit_log(&self, record: &NewAuditLogRecord) -> Result<i64, StoreError> {
            if self.fail {
                return Err(StoreError::Query("down".into()));
            }
            let mut records = self.records.lock().unwrap();
            records.push(record.clone());
            Ok(records.len() as i64)
        }

        async fn list_audit_logs(&self, _: &AuditLogQuery) -> Result<AuditLogPage, StoreError> {
            unreachable!()
        }

        async fn get_audit_log(&self, _: i64) -> Result<Option<AuditLogRecord>, StoreError> {
            unreachable!()
        }
    }

    fn context() -> AuditContext {
        AuditContext::new(AuditActor::user(Uuid::nil(), "admin@example.com")).with_request(
            Some("203.0.113.9".into()),
            Some("tests".into()),
            None,
        )
    }

    #[tokio::test]
    async fn audited_writes_after_success_and_skips_failures() {
        let repo = Arc::new(RecordingRepo::default());
        let audit = AdminAudit::new(repo.clone());

        let created: Result<u32, &str> = audit
            .audited(&context(), async { Ok(7) }, |value| {
                AuditEvent::new(actions::TEAM_CREATED, targets::TEAM)
                    .target(value)
                    .after(json!({ "name": "Platform" }))
            })
            .await;
        assert_eq!(created, Ok(7));

        let failed: Result<u32, &str> = audit
            .audited(&context(), async { Err("conflict") }, |_| {
                panic!("a failed mutation must not describe an entry")
            })
            .await;
        assert_eq!(failed, Err("conflict"));

        let records = repo.records.lock().unwrap();
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record.action, "team.created");
        assert_eq!(record.target_id.as_deref(), Some("7"));
        assert_eq!(record.request_ip.as_deref(), Some("203.0.113.9"));
        assert_eq!(record.after, Some(json!({ "name": "Platform" })));
        assert_eq!(record.occurred_at.nanosecond() % 1_000_000, 0);
    }

    #[tokio::test]
    async fn audited_steps_records_partial_changes_only_after_a_write_landed() {
        let repo = Arc::new(RecordingRepo::default());
        let audit = AdminAudit::new(repo.clone());

        let untouched = AuditProgress::default();
        let result: Result<(), &str> = audit
            .audited_steps(
                &context(),
                &untouched,
                async { Err("validation") },
                |_| -> AuditEvent { unreachable!() },
                |_, _| AuditEvent::new(actions::TEAM_UPDATED, targets::TEAM),
            )
            .await;
        assert_eq!(result, Err("validation"));
        assert!(repo.records.lock().unwrap().is_empty());

        let progress = AuditProgress::default();
        let result: Result<(), &str> = audit
            .audited_steps(
                &context(),
                &progress,
                async {
                    progress.landed_on("team-1");
                    Err("tags failed")
                },
                |_| -> AuditEvent { unreachable!() },
                |progress, error| {
                    AuditEvent::new(actions::TEAM_CREATED, targets::TEAM)
                        .target(progress.target_id().unwrap_or_default())
                        .meta("error", *error)
                },
            )
            .await;
        assert_eq!(result, Err("tags failed"));
        let records = repo.records.lock().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].target_id.as_deref(), Some("team-1"));
        assert_eq!(records[0].metadata["outcome"], json!("partial"));
        assert_eq!(records[0].metadata["error"], json!("tags failed"));
    }

    #[tokio::test]
    async fn audited_steps_skips_unchanged_success() {
        let repo = Arc::new(RecordingRepo::default());
        let audit = AdminAudit::new(repo.clone());
        for changed in [false, true] {
            let result: Result<bool, ()> = audit
                .audited_steps(
                    &context(),
                    &AuditProgress::default(),
                    async move { Ok(changed) },
                    |changed| {
                        changed.then(|| AuditEvent::new(actions::API_KEY_REVOKED, targets::API_KEY))
                    },
                    |_, _| unreachable!(),
                )
                .await;
            assert_eq!(result, Ok(changed));
        }
        assert_eq!(repo.records.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn oversized_partial_metadata_keeps_outcome_and_error() {
        let repo = Arc::new(RecordingRepo::default());
        let audit = AdminAudit::new(repo.clone());
        let context = context().with_request(None, None, Some("req-1".to_string()));
        let progress = AuditProgress::default();
        let requested = vec!["x".repeat(64); MAX_SUMMARY_BYTES / 32];
        let _: Result<(), &str> = audit
            .audited_steps(
                &context,
                &progress,
                async {
                    progress.landed();
                    Err("tags_failed")
                },
                |_| -> AuditEvent { unreachable!() },
                |_, error| {
                    AuditEvent::new(actions::TEAM_CREATED, targets::TEAM)
                        .meta("error", *error)
                        .meta("requested", &requested)
                },
            )
            .await;
        let records = repo.records.lock().unwrap();
        let metadata = &records[0].metadata;
        assert_eq!(metadata["truncated"], json!(true));
        assert_eq!(metadata["outcome"], json!("partial"));
        assert_eq!(metadata["error"], json!("tags_failed"));
        assert_eq!(metadata["request_id"], json!("req-1"));
        assert!(metadata.get("requested").is_none());
    }

    #[tokio::test]
    async fn audited_change_skips_no_op_mutations() {
        let repo = Arc::new(RecordingRepo::default());
        let audit = AdminAudit::new(repo.clone());
        for changed in [false, true] {
            let result: Result<bool, ()> = audit
                .audited_change(&context(), async move { Ok(changed) }, || {
                    AuditEvent::new(actions::MCP_GRANT_REVOKED, targets::MCP_GRANT)
                })
                .await;
            assert_eq!(result, Ok(changed));
        }
        assert_eq!(repo.records.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_failed_audit_write_does_not_fail_the_mutation() {
        let repo = Arc::new(RecordingRepo {
            fail: true,
            ..Default::default()
        });
        let audit = AdminAudit::new(repo);
        let result: Result<(), ()> = audit
            .audited(&context(), async { Ok(()) }, |_| {
                AuditEvent::new(actions::TEAM_CREATED, targets::TEAM)
            })
            .await;
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn redaction_removes_secrets_and_keeps_descriptive_fields() {
        let redacted = redact_audit_value(json!({
            "name": "ci",
            "api_key_id": "6f1c",
            "raw_key": "gwk_abc.def",
            "secret_hash": "argon2id$...",
            "client_secret": "s3cr3t",
            "headers": { "Authorization": "Bearer abc", "X-Trace": "ok" },
            "notes": ["gwk_leaked.value", "fine"],
            "prefix": "gwk_0f3c9a",
            "invite_url": "https://gateway.example/invite/abc",
            "secret_configured": true,
            "secret_ref": "env/UPSTREAM_TOKEN",
            "access_token_expires_at": "2026-10-15T00:00:00Z",
            "password": null,
            "tags": [
                { "key": "github_pat", "value": "plain-looking-value" },
                { "key": "password_ref", "value": "hunter2" },
                { "key": "glpat-0123456789abcdefghij", "value": "x" },
                { "key": "team", "value": "platform" },
            ],
            "pat": "ghp_0123456789abcdefghij",
            "description": "rotate token=gwk_0f3c9a.s3cr3t soon",
            "auth_note": "use auth=Bearer supersecret123 here",
            "plan_name": "Basic tier",
            "comment": "see (ghp_0123456789abcdefghij)",
            "path": "/srv/data",
            "mcp_note": "mirror at https://alice:hunter2@mcp.example/sse",
            "repo_note": "clone https://x-token@git.example/acme/api",
            "profile": "https://social.example/@alice and bob@example.com",
            "query_note": "see https://example.test/?api_key=hunter2",
            "setup": "set password=hunter2 first",
            "spaced": "set password = hunter2 first",
            "quoted": "config has password=\"hunter2\"",
            "single_quoted": "set api_key = 'secret' here",
            "empty_quoted": "password=\"\" means none",
            "json_config": "config={\"password\":\"hunter2\"}",
            "encoded_query": "see https://example.test/?api%5Fkey=hunter2",
            "double_encoded": "next=https%3A%2F%2Fexample.test%2F%3Fapi%255Fkey%3Dhunter2",
            "encoded_path": "docs at https://example.test/guides/100%25-uptime",
            "header_note": "send Authorization: Bearer abc",
            "basic_note": "use Basic dTpw",
            "requirements": "requires a bearer token",
            "rotation": "token rotation is weekly",
        }));
        assert_eq!(
            redacted,
            json!({
                "name": "ci",
                "api_key_id": "6f1c",
                "raw_key": AUDIT_REDACTED,
                "secret_hash": AUDIT_REDACTED,
                "client_secret": AUDIT_REDACTED,
                "headers": { "Authorization": AUDIT_REDACTED, "X-Trace": "ok" },
                "notes": [AUDIT_REDACTED, "fine"],
                "prefix": "gwk_0f3c9a",
                "invite_url": AUDIT_REDACTED,
                "secret_configured": true,
                "secret_ref": "env/UPSTREAM_TOKEN",
                "access_token_expires_at": "2026-10-15T00:00:00Z",
                "password": null,
                "tags": [
                    { "key": "github_pat", "value": AUDIT_REDACTED },
                    { "key": "password_ref", "value": AUDIT_REDACTED },
                    { "key": AUDIT_REDACTED, "value": AUDIT_REDACTED },
                    { "key": "team", "value": "platform" },
                ],
                "pat": AUDIT_REDACTED,
                "description": AUDIT_REDACTED,
                "auth_note": AUDIT_REDACTED,
                "plan_name": "Basic tier",
                "comment": AUDIT_REDACTED,
                "path": "/srv/data",
                "mcp_note": AUDIT_REDACTED,
                "repo_note": AUDIT_REDACTED,
                "profile": "https://social.example/@alice and bob@example.com",
                "query_note": AUDIT_REDACTED,
                "setup": AUDIT_REDACTED,
                "spaced": AUDIT_REDACTED,
                "quoted": AUDIT_REDACTED,
                "single_quoted": AUDIT_REDACTED,
                "empty_quoted": "password=\"\" means none",
                "json_config": AUDIT_REDACTED,
                "encoded_query": AUDIT_REDACTED,
                "double_encoded": AUDIT_REDACTED,
                "encoded_path": "docs at https://example.test/guides/100%25-uptime",
                "header_note": AUDIT_REDACTED,
                "basic_note": AUDIT_REDACTED,
                "requirements": "requires a bearer token",
                "rotation": "token rotation is weekly",
            })
        );
    }

    #[tokio::test]
    async fn target_labels_are_redacted_and_capped() {
        let repo = Arc::new(RecordingRepo::default());
        let audit = AdminAudit::new(repo.clone());
        for label in [
            "ci gwk_0f3c9a.s3cr3t".to_string(),
            "https://gateway.example/admin/invite/abc".to_string(),
            "x".repeat(MAX_LABEL_CHARS + 10),
            "gwk_0f3c9a".to_string(),
        ] {
            audit
                .record(
                    &context(),
                    AuditEvent::new(actions::API_KEY_CREATED, targets::API_KEY).label(label),
                )
                .await;
        }
        let records = repo.records.lock().unwrap();
        let labels: Vec<_> = records
            .iter()
            .map(|record| record.target_label.clone().unwrap())
            .collect();
        assert_eq!(labels[0], AUDIT_REDACTED);
        assert_eq!(labels[1], AUDIT_REDACTED);
        assert_eq!(labels[2].chars().count(), MAX_LABEL_CHARS + 1);
        assert_eq!(labels[3], "gwk_0f3c9a");
    }

    #[test]
    fn secret_path_segments_are_detected_but_identifiers_kept() {
        assert!(is_secret_path_segment("gwk_0f3c9a.s3cr3t"));
        assert!(is_secret_path_segment(
            "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8"
        ));
        assert!(!is_secret_path_segment(
            "6f1c9a2e-0d5b-4c1e-9f7a-1b2c3d4e5f60"
        ));
        assert!(is_secret_path_segment(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        ));
        assert!(!is_secret_path_segment(
            "oauth-authorization-server-metadata"
        ));
        assert!(!is_secret_path_segment("mcp"));
        assert!(!is_secret_path_segment("v1"));
        // Percent-encoded tokens are decoded before they are checked.
        assert!(is_secret_path_segment(&"%41".repeat(32)));
        assert!(is_secret_path_segment(
            "%61%31%62%32c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8"
        ));
        assert!(is_secret_path_segment("gwk_0f3c9a%2Es3cr3t"));
        // Base64 tokens carry `+`, `/` and `=`.
        assert!(is_secret_path_segment(
            "dGhpcyBpcyBhIHNlY3JldCB0b2tlbg+/xyz1=="
        ));
        assert!(!is_secret_path_segment("%7Euser"));
        assert!(is_secret_path_segment("0f3c9a2e0d5b4c1e9f7a1b2c3d4e5f60"));
        assert!(is_secret_path_segment("12345678901234567890123456789012"));
        // Shorter numeric IDs, such as 19-digit snowflakes, stay readable.
        assert!(!is_secret_path_segment("1234567890123456789"));
    }

    #[tokio::test]
    async fn credential_shaped_user_agents_are_redacted() {
        let repo = Arc::new(RecordingRepo::default());
        let audit = AdminAudit::new(repo.clone());
        for user_agent in ["gwk_0f3c9a.s3cr3t", "curl/8.0"] {
            let context = AuditContext::new(AuditActor::user(Uuid::nil(), "a@example.com"))
                .with_request(None, Some(user_agent.to_string()), None);
            audit
                .record(
                    &context,
                    AuditEvent::new(actions::TEAM_CREATED, targets::TEAM),
                )
                .await;
        }
        let records = repo.records.lock().unwrap();
        assert_eq!(records[0].user_agent.as_deref(), Some(AUDIT_REDACTED));
        assert_eq!(records[1].user_agent.as_deref(), Some("curl/8.0"));
    }

    #[tokio::test]
    async fn target_ids_are_redacted_and_capped() {
        let repo = Arc::new(RecordingRepo::default());
        let audit = AdminAudit::new(repo.clone());
        for target_id in [
            "user_model:1:upstream:password=hunter2".to_string(),
            "x".repeat(MAX_LABEL_CHARS + 50),
            "6f1c9a2e-0d5b-4c1e-9f7a-1b2c3d4e5f60".to_string(),
        ] {
            audit
                .record(
                    &context(),
                    AuditEvent::new(actions::BUDGET_SET, targets::BUDGET).target(target_id),
                )
                .await;
        }
        let records = repo.records.lock().unwrap();
        assert_eq!(records[0].target_id.as_deref(), Some(AUDIT_REDACTED));
        assert_eq!(
            records[1].target_id.as_ref().map(|id| id.chars().count()),
            Some(MAX_LABEL_CHARS + 1)
        );
        assert_eq!(
            records[2].target_id.as_deref(),
            Some("6f1c9a2e-0d5b-4c1e-9f7a-1b2c3d4e5f60")
        );
    }

    #[tokio::test]
    async fn actor_labels_are_redacted_and_capped() {
        let repo = Arc::new(RecordingRepo::default());
        let audit = AdminAudit::new(repo.clone());
        let context = AuditContext::new(AuditActor::user(
            Uuid::nil(),
            "gwk_0f3c9a.s3cr3t@example.com",
        ));
        audit
            .record(
                &context,
                AuditEvent::new(actions::TEAM_CREATED, targets::TEAM),
            )
            .await;
        let records = repo.records.lock().unwrap();
        assert_eq!(records[0].actor.label.as_deref(), Some(AUDIT_REDACTED));
        assert_eq!(records[0].actor.user_id, Some(Uuid::nil()));
    }

    #[tokio::test]
    async fn oversized_summaries_are_replaced_with_a_marker() {
        let repo = Arc::new(RecordingRepo::default());
        let audit = AdminAudit::new(repo.clone());
        audit
            .record(
                &context(),
                AuditEvent::new(actions::MCP_SERVER_UPDATED, targets::MCP_SERVER)
                    .after(json!({ "blob": "x".repeat(MAX_SUMMARY_BYTES) })),
            )
            .await;
        let records = repo.records.lock().unwrap();
        assert_eq!(records[0].after.as_ref().unwrap()["truncated"], json!(true));
    }
}
