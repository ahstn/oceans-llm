//! `oceans hook send`: delivers queued entries in sequence order, one sender at a time (ADR
//! "Delivery"). Entries leave the queue only when the gateway has applied, deduplicated or
//! refused them; anything else keeps them queued for the next run.

use std::{
    path::PathBuf,
    time::{Duration, SystemTime},
};

use reqwest::{Method, StatusCode};
use serde::Deserialize;

use super::{
    ownership::{LookupError, key_fingerprint, lookup},
    queue::{Entry, read_entry},
    state::{CachedOwnership, LOCK_TIMEOUT, State},
};
use crate::{client::Client, config::Connection};

pub const MAX_BATCH_ENTRIES: usize = 100;
pub const MAX_BATCH_BYTES: usize = 2 * 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
/// Passes over the queue per run, to pick up entries hooks queued while it sent.
const ROUNDS: usize = 3;
const ENVELOPE_PREFIX: &[u8] = br#"{"expected_ownership_scope":"#;
const ENVELOPE_ENTRIES: &[u8] = br#","entries":["#;
const ENVELOPE_SUFFIX: &[u8] = b"]}";

pub struct SendOptions {
    /// Waits between attempts after a transient failure; the sender exits once they run out.
    pub backoff: Vec<Duration>,
}

impl Default for SendOptions {
    fn default() -> Self {
        Self {
            backoff: [5, 15, 40].map(Duration::from_secs).to_vec(),
        }
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct SendReport {
    pub delivered: usize,
    pub quarantined: usize,
    pub remaining: usize,
    /// Why delivery stopped with entries still queued.
    pub stopped: Option<String>,
    /// Another sender held the lock, so this one did nothing.
    pub already_running: bool,
}

/// Delivers queued entries. `connect` resolves the gateway and key, and runs only when there
/// is something to send, so a key command doesn't run on every hook.
pub async fn send(
    state: &State,
    connect: impl FnOnce() -> anyhow::Result<Connection>,
    options: &SendOptions,
) -> anyhow::Result<SendReport> {
    let mut report = SendReport::default();
    let mut connect = Some(connect);
    let mut sender: Option<Sender<'_>> = None;
    for round in 0..ROUNDS {
        let Some(lock) = state.try_lock_sender()? else {
            report.already_running = round == 0;
            break;
        };
        state.cleanup(SystemTime::now())?;
        state.sequence_unsequenced(LOCK_TIMEOUT)?;
        if state.queued_files()?.is_empty() {
            break;
        }
        if sender.is_none() {
            // Read before the key is resolved, so a login that saves another key from here on
            // keeps its scope current.
            let seen_current = state.current_ownership();
            let connection = connect.take().expect("connects once")()?;
            sender = Some(Sender::new(state, connection, seen_current, options)?);
        }
        let active = sender.as_mut().expect("created above");
        if let Err(stop) = active.run(&mut report).await {
            report.stopped = Some(stop);
            break;
        }
        // Released before checking, so an entry queued by a hook that found this sender
        // running still goes out.
        drop(lock);
        if !state.has_pending()? {
            break;
        }
    }
    report.remaining = state.queued_files()?.len();
    Ok(report)
}

struct Sender<'a> {
    state: &'a State,
    client: Client,
    gateway_url: String,
    fingerprint: String,
    scope: Option<String>,
    /// The ownership cache's current entry as this sender last saw or set it. The sender
    /// replaces it only if nothing else, such as `oceans login`, changed it since.
    seen_current: Option<CachedOwnership>,
    options: &'a SendOptions,
    /// After the gateway refuses a whole batch, its entries go one at a time to find the one
    /// it refuses. Counts the entries left to send that way.
    one_at_a_time: usize,
    owner_changes: usize,
}

struct Pending {
    path: PathBuf,
    entry: Entry,
    json: Vec<u8>,
}

enum Step {
    Progress,
    Empty,
    Transient(String),
}

#[derive(Deserialize)]
struct BatchResponse {
    ownership_scope: String,
    results: Vec<EntryResult>,
}

#[derive(Deserialize)]
struct EntryResult {
    #[serde(default)]
    entry_id: String,
    status: String,
    #[serde(default)]
    reason: Option<String>,
}

impl<'a> Sender<'a> {
    fn new(
        state: &'a State,
        connection: Connection,
        seen_current: Option<CachedOwnership>,
        options: &'a SendOptions,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            state,
            fingerprint: key_fingerprint(&connection.api_key),
            gateway_url: connection.url.to_string(),
            client: Client::new(connection.url, &connection.api_key)?,
            scope: None,
            seen_current,
            options,
            one_at_a_time: 0,
            owner_changes: 0,
        })
    }

    /// Sends until the queue is empty. `Err` stops delivery and keeps what's left queued.
    async fn run(&mut self, report: &mut SendReport) -> Result<(), String> {
        let mut failures = 0;
        loop {
            let step = match self.scope.clone() {
                Some(scope) => self.send_batch(&scope, report).await?,
                None => self.resolve_scope().await?,
            };
            match step {
                Step::Progress => failures = 0,
                Step::Empty => return Ok(()),
                Step::Transient(reason) => {
                    let Some(wait) = self.options.backoff.get(failures) else {
                        return Err(reason);
                    };
                    tokio::time::sleep(*wait).await;
                    failures += 1;
                }
            }
        }
    }

    /// The key's scope, from the cache when this key on this gateway was looked up before.
    async fn resolve_scope(&mut self) -> Result<Step, String> {
        if let Some(cached) = self
            .state
            .cached_ownership(&self.gateway_url, &self.fingerprint)
        {
            self.scope = Some(cached.ownership_scope.clone());
            self.state
                .make_current_unless_changed(cached, &mut self.seen_current)
                .map_err(text)?;
            return Ok(Step::Progress);
        }
        match lookup(&self.client).await {
            Ok(ownership) => {
                self.state
                    .record_ownership_unless_changed(
                        &self.gateway_url,
                        &self.fingerprint,
                        &ownership,
                        &mut self.seen_current,
                    )
                    .map_err(text)?;
                self.scope = Some(ownership.ownership_scope);
                Ok(Step::Progress)
            }
            Err(LookupError::Transient(reason)) => Ok(Step::Transient(reason)),
            Err(LookupError::Fatal(reason)) => Err(reason),
        }
    }

    async fn send_batch(&mut self, scope: &str, report: &mut SendReport) -> Result<Step, String> {
        let limit = if self.one_at_a_time > 0 {
            1
        } else {
            MAX_BATCH_ENTRIES
        };
        let batch = self.next_batch(scope, limit, report).map_err(text)?;
        if batch.is_empty() {
            return Ok(Step::Empty);
        }
        let body = batch_body(scope, &batch);
        let Ok(response) = self
            .client
            .send_raw(
                Method::POST,
                &["attribution", "entries"],
                Some(body),
                MAX_RESPONSE_BYTES,
            )
            .await
        else {
            return Ok(Step::Transient("could not reach the gateway".into()));
        };
        let refused_batch = matches!(
            response.status,
            StatusCode::PAYLOAD_TOO_LARGE
                | StatusCode::BAD_REQUEST
                | StatusCode::UNPROCESSABLE_ENTITY
        );
        if refused_batch && batch.len() > 1 {
            self.one_at_a_time = batch.len();
            return Ok(Step::Progress);
        }
        let step = match response.status {
            StatusCode::OK => self.apply_results(scope, &batch, &response.body, report)?,
            StatusCode::CONFLICT => self.owner_changed(&batch, report)?,
            StatusCode::PAYLOAD_TOO_LARGE => {
                self.quarantine_all(&batch, "oversized_entry", report)?
            }
            _ if refused_batch => self.quarantine_all(&batch, "invalid_entry", report)?,
            status if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() => {
                return Ok(Step::Transient(format!(
                    "the gateway returned HTTP {status}"
                )));
            }
            status => return Err(fatal_status(status)),
        };
        if matches!(step, Step::Progress) {
            self.one_at_a_time = self.one_at_a_time.saturating_sub(batch.len());
        }
        Ok(step)
    }

    /// The next entries to send, in sequence order. Entries that can't be sent under `scope`,
    /// or that the gateway would refuse, are quarantined on the way.
    fn next_batch(
        &self,
        scope: &str,
        limit: usize,
        report: &mut SendReport,
    ) -> anyhow::Result<Vec<Pending>> {
        let mut batch = Vec::new();
        let mut size = envelope_bytes(scope);
        for path in self.state.queued_files()? {
            let Ok(entry) = read_entry(&path) else {
                self.state.drop_corrupt(&path)?;
                continue;
            };
            let local = match entry.ownership_scope.as_deref() {
                _ if entry.sequence.is_none() => Some("invalid_entry"),
                None => Some("missing_ownership_scope"),
                Some(stamped) if stamped != scope => Some("ownership_scope_mismatch"),
                Some(_) => entry.payload.local_rejection(),
            };
            if let Some(reason) = local {
                self.state.quarantine_file(&path, entry, reason)?;
                report.quarantined += 1;
                continue;
            }
            let json = serde_json::to_vec(&entry.wire().expect("sequence checked above"))?;
            if envelope_bytes(scope) + json.len() > MAX_BATCH_BYTES {
                self.state
                    .quarantine_file(&path, entry, "oversized_entry")?;
                report.quarantined += 1;
                continue;
            }
            if size + json.len() + 1 > MAX_BATCH_BYTES {
                break;
            }
            size += json.len() + 1;
            batch.push(Pending { path, entry, json });
            if batch.len() == limit {
                break;
            }
        }
        Ok(batch)
    }

    fn apply_results(
        &self,
        scope: &str,
        batch: &[Pending],
        body: &[u8],
        report: &mut SendReport,
    ) -> Result<Step, String> {
        let response: BatchResponse = serde_json::from_slice(body)
            .map_err(|_| "the gateway returned an invalid attribution response".to_string())?;
        let matches = response.ownership_scope == scope
            && response.results.len() == batch.len()
            && batch
                .iter()
                .zip(&response.results)
                .all(|(pending, result)| {
                    result.entry_id == pending.entry.entry_id
                        || (result.entry_id.is_empty() && result.status == "rejected")
                });
        if !matches {
            return Err("the gateway's attribution response doesn't match the entries sent".into());
        }
        let mut step = Step::Progress;
        for (pending, result) in batch.iter().zip(response.results) {
            match result.status.as_str() {
                "applied" | "duplicate" | "stale" => {
                    std::fs::remove_file(&pending.path).map_err(text)?;
                    report.delivered += 1;
                }
                "rejected" => {
                    let reason = result
                        .reason
                        .filter(|reason| is_reason_code(reason))
                        .unwrap_or_else(|| "rejected".into());
                    self.quarantine(pending, &reason, report)?;
                }
                // A status from a newer gateway: keep the entry and try again later.
                _ => step = Step::Transient("the gateway returned an unknown entry status".into()),
            }
        }
        Ok(step)
    }

    /// HTTP 409: the key now belongs to another owner. The batch was queued for the old one,
    /// so it is quarantined rather than sent under the new one.
    fn owner_changed(
        &mut self,
        batch: &[Pending],
        report: &mut SendReport,
    ) -> Result<Step, String> {
        self.owner_changes += 1;
        if self.owner_changes > 1 {
            return Err(
                "the gateway keeps reporting a different owner for this API key (HTTP 409)".into(),
            );
        }
        self.quarantine_all(batch, "owner_changed", report)?;
        self.state
            .forget_ownership(&self.gateway_url, &self.fingerprint)
            .map_err(text)?;
        self.scope = None;
        Ok(Step::Progress)
    }

    fn quarantine_all(
        &self,
        batch: &[Pending],
        reason: &str,
        report: &mut SendReport,
    ) -> Result<Step, String> {
        for pending in batch {
            self.quarantine(pending, reason, report)?;
        }
        Ok(Step::Progress)
    }

    fn quarantine(
        &self,
        pending: &Pending,
        reason: &str,
        report: &mut SendReport,
    ) -> Result<(), String> {
        self.state
            .quarantine_file(&pending.path, pending.entry.clone(), reason)
            .map_err(text)?;
        report.quarantined += 1;
        Ok(())
    }
}

fn fatal_status(status: StatusCode) -> String {
    match status {
        StatusCode::UNAUTHORIZED => "the gateway rejected the API key (HTTP 401)".into(),
        StatusCode::FORBIDDEN => "the API key can't send attribution (HTTP 403); use a full-access key or one with the attribution:write scope".into(),
        StatusCode::NOT_FOUND => {
            "this gateway has no attribution API (HTTP 404); it may need an upgrade".into()
        }
        status => format!("the gateway returned HTTP {status}"),
    }
}

fn text(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn scope_json(scope: &str) -> String {
    serde_json::to_string(scope).expect("strings serialize")
}

fn envelope_bytes(scope: &str) -> usize {
    ENVELOPE_PREFIX.len() + scope_json(scope).len() + ENVELOPE_ENTRIES.len() + ENVELOPE_SUFFIX.len()
}

/// `{"expected_ownership_scope":…,"entries":[…]}` from entries serialized once.
fn batch_body(scope: &str, batch: &[Pending]) -> Vec<u8> {
    let entries: usize = batch.iter().map(|pending| pending.json.len() + 1).sum();
    let mut body = Vec::with_capacity(envelope_bytes(scope) + entries);
    body.extend_from_slice(ENVELOPE_PREFIX);
    body.extend_from_slice(scope_json(scope).as_bytes());
    body.extend_from_slice(ENVELOPE_ENTRIES);
    for (index, pending) in batch.iter().enumerate() {
        if index > 0 {
            body.push(b',');
        }
        body.extend_from_slice(&pending.json);
    }
    body.extend_from_slice(ENVELOPE_SUFFIX);
    body
}

/// Reason codes stay short snake_case identifiers, so a gateway can't put text in listings.
fn is_reason_code(reason: &str) -> bool {
    (1..=64).contains(&reason.len())
        && reason
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

#[cfg(test)]
#[path = "sender_tests.rs"]
mod tests;
