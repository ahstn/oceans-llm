//! Hook inputs from git-ai, Claude Code and OMP. Each hook reads stdin, queues entries and
//! returns; a detached `oceans hook send` delivers them.

use std::{
    fmt,
    io::{BufReader, Read},
    path::PathBuf,
};

use anyhow::{anyhow, bail};
use serde::{
    Deserialize,
    de::{DeserializeOwned, Deserializer as _, SeqAccess, Visitor},
};

use super::{
    payload::{Harness, NotePayload, Payload, SessionEvent, SessionPayload},
    queue::EnqueueReport,
    repo::{git_context, normalize_repository},
    state::State,
};

/// git-ai sends every note updated by one git operation at once; past this the rest is lost.
const MAX_NOTES_INPUT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SESSION_INPUT_BYTES: u64 = 1024 * 1024;
/// The gateway's limits for display text it stores.
const MAX_BRANCH_BYTES: usize = 256;
const MAX_HARNESS_VERSION_BYTES: usize = 128;
const MAX_SESSION_ID_BYTES: usize = 256;

/// One element of the `post_notes_updated` payload. git-ai also sends `repo_name`, which isn't
/// needed.
#[derive(Deserialize)]
struct HookNote {
    commit_sha: String,
    #[serde(default)]
    repo_url: Option<String>,
    #[serde(default)]
    branch: Option<String>,
    #[serde(default)]
    is_default_branch: Option<bool>,
    note_content: String,
}

#[derive(Deserialize)]
struct ClaudeInput {
    session_id: String,
    #[serde(default)]
    cwd: Option<PathBuf>,
    hook_event_name: String,
}

/// What the OMP extension writes: `{"session_id","cwd","event","harness_version"}`.
#[derive(Deserialize)]
struct OmpInput {
    session_id: String,
    #[serde(default)]
    cwd: Option<PathBuf>,
    event: SessionEvent,
    #[serde(default)]
    harness_version: Option<String>,
}

/// A queued session event, so the caller can start PR sync and delivery.
#[derive(Debug)]
pub struct QueuedSession {
    pub event: SessionEvent,
    pub directory: PathBuf,
    pub has_repository: bool,
}

/// Queues the notes in git-ai's `post_notes_updated` payload. Notes the gateway would refuse
/// (over 1 MiB, no usable repository, a malformed SHA) go to quarantine instead.
pub fn notes(state: &State, input: impl Read) -> anyhow::Result<EnqueueReport> {
    let mut queue = Vec::new();
    let mut quarantine = Vec::new();
    for note in read_notes(input, MAX_NOTES_INPUT_BYTES) {
        let payload = note_payload(note);
        match payload.local_rejection() {
            Some(reason) => quarantine.push((payload, reason)),
            None => queue.push(payload),
        }
    }
    let quarantined = state.quarantine_payloads(quarantine)?;
    let mut report = state.enqueue(queue)?;
    report.quarantined += quarantined.quarantined;
    report.dropped += quarantined.dropped;
    Ok(report)
}

fn note_payload(note: HookNote) -> Payload {
    Payload::Note(NotePayload {
        // A remote that doesn't normalise is never stored: it may hold credentials.
        repository: note
            .repo_url
            .as_deref()
            .and_then(normalize_repository)
            .unwrap_or_default(),
        commit_sha: note.commit_sha,
        branch: clean_text(note.branch, MAX_BRANCH_BYTES).unwrap_or_else(|| "unknown".into()),
        is_default_branch: note.is_default_branch.unwrap_or(false),
        note_content: note.note_content,
    })
}

/// Streams the JSON array, keeping each element that has the note shape. Notes read before
/// the size cap or a malformed tail are kept.
fn read_notes(input: impl Read, max_bytes: u64) -> Vec<HookNote> {
    let reader = BufReader::new(input.take(max_bytes));
    let mut notes = Vec::new();
    let mut deserializer = serde_json::Deserializer::from_reader(reader);
    let _ = deserializer.deserialize_seq(NotesVisitor(&mut notes));
    notes
}

struct NotesVisitor<'a>(&'a mut Vec<HookNote>);

impl<'de> Visitor<'de> for NotesVisitor<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an array of git-ai notes")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut elements: A) -> Result<(), A::Error> {
        while let Some(element) = elements.next_element::<serde_json::Value>()? {
            if let Ok(note) = serde_json::from_value(element) {
                self.0.push(note);
            }
        }
        Ok(())
    }
}

/// Queues a Claude Code `SessionStart` or `SessionEnd` event; other events are ignored.
/// Claude Code's hook input carries no version, so none is reported.
pub fn claude(state: &State, input: impl Read) -> anyhow::Result<Option<QueuedSession>> {
    let input: ClaudeInput = read_json(input, "a Claude Code hook event")?;
    let event = match input.hook_event_name.as_str() {
        "SessionStart" => SessionEvent::Start,
        "SessionEnd" => SessionEvent::End,
        _ => return Ok(None),
    };
    queue_session(
        state,
        Harness::ClaudeCode,
        input.session_id,
        event,
        input.cwd,
        None,
    )
}

pub fn omp(state: &State, input: impl Read) -> anyhow::Result<Option<QueuedSession>> {
    let input: OmpInput = read_json(input, "an OMP session event")?;
    queue_session(
        state,
        Harness::OhMyPi,
        input.session_id,
        input.event,
        input.cwd,
        input.harness_version,
    )
}

fn queue_session(
    state: &State,
    harness: Harness,
    session_id: String,
    event: SessionEvent,
    cwd: Option<PathBuf>,
    harness_version: Option<String>,
) -> anyhow::Result<Option<QueuedSession>> {
    if !is_session_id(&session_id) {
        // The id itself is never echoed.
        bail!("the session id isn't one the gateway accepts; nothing was queued");
    }
    let directory = match cwd.filter(|cwd| cwd.is_absolute()) {
        Some(directory) => directory,
        None => std::env::current_dir()?,
    };
    let context = git_context(&directory);
    let payload = SessionPayload {
        harness,
        session_id,
        event,
        repository: context.repository,
        branch: clean_text(context.branch, MAX_BRANCH_BYTES),
        head_sha: context.head_sha,
        harness_version: clean_text(harness_version, MAX_HARNESS_VERSION_BYTES),
    };
    let has_repository = payload.repository.is_some();
    state.enqueue(vec![Payload::Session(payload)])?;
    Ok(Some(QueuedSession {
        event,
        directory,
        has_repository,
    }))
}

/// Parses small hook input. Errors name what was expected, never the input.
fn read_json<T: DeserializeOwned>(input: impl Read, expected: &str) -> anyhow::Result<T> {
    let mut bytes = Vec::new();
    input
        .take(MAX_SESSION_INPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| anyhow!("could not read stdin"))?;
    if bytes.len() as u64 > MAX_SESSION_INPUT_BYTES {
        bail!("stdin is larger than 1 MiB; expected {expected}");
    }
    serde_json::from_slice(&bytes).map_err(|_| anyhow!("stdin isn't {expected}"))
}

/// The session ids the gateway accepts: 1-256 bytes of `[A-Za-z0-9._:-]`.
fn is_session_id(value: &str) -> bool {
    (1..=MAX_SESSION_ID_BYTES).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

/// Optional display text the gateway accepts: non-empty, short, no control characters.
fn clean_text(value: Option<String>, max_bytes: usize) -> Option<String> {
    value.filter(|value| {
        !value.is_empty() && value.len() <= max_bytes && !value.chars().any(char::is_control)
    })
}

#[cfg(test)]
#[path = "hooks_tests.rs"]
mod tests;
