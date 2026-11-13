//! Parser for git-ai authorship notes (`authorship/3.x`, see the git-ai standard v3.0.0).
//!
//! A note is an attestation section (unindented file paths, then two-space `<key> <ranges>`
//! lines), a `---` line, and a JSON metadata object. Only what attribution stores is kept:
//! attested line counts per session or human key, the agent tool and model, and the raw agent
//! session id and author strings for the caller to hash or match. The raw values are redacted
//! from `Debug` output and never appear in [`NoteError`].

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use serde::Deserialize;
use serde_json::Value;

/// Largest note accepted.
pub const MAX_NOTE_BYTES: usize = 1024 * 1024;
/// Largest JSON metadata section accepted.
pub const MAX_METADATA_BYTES: usize = 256 * 1024;
const MAX_FILE_PATHS: usize = 10_000;
const MAX_ATTESTATION_LINES: usize = 100_000;
const MAX_RANGES_PER_LINE: usize = 10_000;
const MAX_LINE_NUMBER: u32 = 10_000_000;
const MAX_KEY_BYTES: usize = 64;
const MAX_TOOL_BYTES: usize = 64;
const MAX_MODEL_BYTES: usize = 256;
const MAX_VERSION_BYTES: usize = 64;
const SUPPORTED_SCHEMA_PREFIX: &str = "authorship/3.";

/// Why a note was refused. Details are fixed text, never note content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteError {
    /// A size or count bound was exceeded.
    Oversized,
    /// The note is not a well-formed authorship log.
    Invalid(&'static str),
    /// `schema_version` is not `authorship/3.*`.
    UnsupportedSchema,
}

/// What a note says about one commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedNote {
    pub schema_version: String,
    pub git_ai_version: Option<String>,
    /// AI sessions and legacy prompts, sorted by key: every metadata record plus any attested
    /// key the metadata lacks.
    pub sessions: Vec<NoteSession>,
    /// Known-human records, sorted by key, likewise.
    pub humans: Vec<NoteHuman>,
}

impl ParsedNote {
    #[must_use]
    pub fn ai_lines(&self) -> i64 {
        self.sessions.iter().map(|session| session.lines).sum()
    }

    #[must_use]
    pub fn human_lines(&self) -> i64 {
        self.humans.iter().map(|human| human.lines).sum()
    }
}

/// An AI session (`s_` key) or legacy prompt (bare hex key).
#[derive(Clone, PartialEq, Eq)]
pub struct NoteSession {
    pub key: String,
    /// `agent_id.tool`, such as `claude` or `pi`.
    pub tool: Option<String>,
    /// Raw `agent_id.id`. Hash it; never store or log it.
    pub agent_session_id: Option<String>,
    /// `agent_id.model`; `None` when `unknown` or empty.
    pub model: Option<String>,
    /// Raw `human_author`, `Name <email>` or an email. Never store or log it.
    pub human_author: Option<String>,
    /// Distinct attested lines, counted per file.
    pub lines: i64,
}

/// A known-human attestation (`h_` key).
#[derive(Clone, PartialEq, Eq)]
pub struct NoteHuman {
    pub key: String,
    /// Raw `author`, `Name <email>`. Never store or log it.
    pub author: Option<String>,
    pub lines: i64,
}

impl fmt::Debug for NoteSession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NoteSession")
            .field("key", &self.key)
            .field("tool", &self.tool)
            .field(
                "agent_session_id",
                &redacted(self.agent_session_id.as_ref()),
            )
            .field("model", &self.model)
            .field("human_author", &redacted(self.human_author.as_ref()))
            .field("lines", &self.lines)
            .finish()
    }
}

impl fmt::Debug for NoteHuman {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NoteHuman")
            .field("key", &self.key)
            .field("author", &redacted(self.author.as_ref()))
            .field("lines", &self.lines)
            .finish()
    }
}

fn redacted(value: Option<&String>) -> Option<&'static str> {
    value.map(|_| "<redacted>")
}

/// Parses a raw note, as git-ai passes it to `post_notes_updated` hooks.
pub fn parse_note(content: &str) -> Result<ParsedNote, NoteError> {
    if content.len() > MAX_NOTE_BYTES {
        return Err(NoteError::Oversized);
    }
    let (attestations, metadata) = split_note(content)?;
    if metadata.len() > MAX_METADATA_BYTES {
        return Err(NoteError::Oversized);
    }
    let metadata = parse_metadata(metadata)?;
    let lines = count_attested_lines(attestations)?;
    assemble(metadata, &lines)
}

/// The lowercased email in `Name <email>` or a bare email, matching how
/// `users.email_normalized` is stored; `None` when there is no plausible email.
#[must_use]
pub fn author_email(author: &str) -> Option<String> {
    let author = author.trim();
    let email = match author.rfind('<') {
        Some(open) if author.ends_with('>') => &author[open + 1..author.len() - 1],
        _ => author,
    };
    let email = email.trim();
    let plausible = email.len() <= 320
        && email.contains('@')
        && !email
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control());
    plausible.then(|| email.to_ascii_lowercase())
}

/// Splits at the first line that is exactly `---`, as git-ai's own parser does.
fn split_note(content: &str) -> Result<(&str, &str), NoteError> {
    let mut offset = 0;
    for line in content.split_inclusive('\n') {
        let text = line.strip_suffix('\n').unwrap_or(line);
        if text.strip_suffix('\r').unwrap_or(text) == "---" {
            return Ok((&content[..offset], &content[offset + line.len()..]));
        }
        offset += line.len();
    }
    Err(NoteError::Invalid("missing `---` divider"))
}

#[derive(Deserialize)]
struct Metadata {
    schema_version: String,
    #[serde(default)]
    git_ai_version: Option<String>,
    #[serde(default)]
    prompts: BTreeMap<String, AgentRecord>,
    #[serde(default)]
    sessions: BTreeMap<String, AgentRecord>,
    #[serde(default)]
    humans: BTreeMap<String, HumanRecord>,
}

#[derive(Deserialize)]
struct AgentRecord {
    #[serde(default)]
    agent_id: Option<AgentId>,
    #[serde(default)]
    human_author: Option<String>,
}

#[derive(Deserialize)]
struct AgentId {
    #[serde(default)]
    tool: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    model: Option<String>,
}

#[derive(Deserialize)]
struct HumanRecord {
    #[serde(default)]
    author: Option<String>,
}

fn parse_metadata(json: &str) -> Result<Metadata, NoteError> {
    let value: Value =
        serde_json::from_str(json).map_err(|_| NoteError::Invalid("metadata is not JSON"))?;
    // Check the version before the shape, so a future schema reads as unsupported.
    let schema_version = value
        .get("schema_version")
        .and_then(Value::as_str)
        .ok_or(NoteError::Invalid("metadata has no schema_version"))?;
    if !schema_version.starts_with(SUPPORTED_SCHEMA_PREFIX) {
        return Err(NoteError::UnsupportedSchema);
    }
    serde_json::from_value(value)
        .map_err(|_| NoteError::Invalid("metadata has an unexpected shape"))
}

/// An attestation key, borrowed from the note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum AttestedKey<'a> {
    /// `s_<hex>` (the session part of `s_<hex>::t_<hex>`) or a legacy 16- or 7-character hex
    /// prompt hash. The two never collide, so they share one namespace.
    Ai(&'a str),
    /// `h_<hex>`.
    Human(&'a str),
}

/// Distinct attested lines per key.
#[derive(Debug, Default)]
struct AttestedLines<'a> {
    ai: HashMap<&'a str, i64>,
    human: HashMap<&'a str, i64>,
}

/// Distinct attested lines per key. Overlapping ranges for one key in one file count once.
fn count_attested_lines(section: &str) -> Result<AttestedLines<'_>, NoteError> {
    // Paths are interned when their header is read, so each entry hashes a small id rather than
    // the whole path: a long path followed by many entries would otherwise be hashed per entry.
    let mut path_ids: HashMap<&str, usize> = HashMap::new();
    let mut spans: HashMap<(usize, AttestedKey<'_>), Vec<(u32, u32)>> = HashMap::new();
    let mut path = None;
    let (mut paths, mut entries) = (0_usize, 0_usize);
    for line in section.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        let Some(entry) = line.strip_prefix("  ") else {
            paths += 1;
            if paths > MAX_FILE_PATHS {
                return Err(NoteError::Oversized);
            }
            let next_id = path_ids.len();
            path = Some(*path_ids.entry(unquote_path(line)).or_insert(next_id));
            continue;
        };
        entries += 1;
        if entries > MAX_ATTESTATION_LINES {
            return Err(NoteError::Oversized);
        }
        let file = path.ok_or(NoteError::Invalid("attestation entry before any file path"))?;
        let (key, ranges) = entry
            .split_once(' ')
            .ok_or(NoteError::Invalid("attestation entry has no line ranges"))?;
        let key = parse_attested_key(key)?;
        parse_line_ranges(ranges, spans.entry((file, key)).or_default())?;
    }
    let mut lines = AttestedLines::default();
    for ((_, key), mut ranges) in spans {
        let (counts, key) = match key {
            AttestedKey::Ai(key) => (&mut lines.ai, key),
            AttestedKey::Human(key) => (&mut lines.human, key),
        };
        *counts.entry(key).or_insert(0) += merged_length(&mut ranges);
    }
    Ok(lines)
}

/// git-ai quotes paths containing whitespace and never escapes inside the quotes.
fn unquote_path(line: &str) -> &str {
    line.strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(line)
}

fn parse_attested_key(key: &str) -> Result<AttestedKey<'_>, NoteError> {
    if let Some(rest) = key.strip_prefix("s_") {
        let (session, trace) = rest.split_once("::").unwrap_or((rest, ""));
        let trace_ok = trace.is_empty() || trace.strip_prefix("t_").is_some_and(is_hex_id);
        return if is_hex_id(session) && trace_ok {
            Ok(AttestedKey::Ai(&key[..2 + session.len()]))
        } else {
            Err(NoteError::Invalid("malformed session attestation key"))
        };
    }
    if let Some(rest) = key.strip_prefix("h_") {
        return if is_hex_id(rest) {
            Ok(AttestedKey::Human(key))
        } else {
            Err(NoteError::Invalid("malformed human attestation key"))
        };
    }
    if matches!(key.len(), 7 | 16) && key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Ok(AttestedKey::Ai(key));
    }
    Err(NoteError::Invalid("unknown attestation key format"))
}

/// The standard uses 14 hex characters today and reserves the right to change the length.
fn is_hex_id(value: &str) -> bool {
    (1..=MAX_KEY_BYTES - 2).contains(&value.len())
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Appends `n`, `a-b` and comma-separated lists of them to `out`.
fn parse_line_ranges(text: &str, out: &mut Vec<(u32, u32)>) -> Result<(), NoteError> {
    for (index, part) in text.split(',').enumerate() {
        if index >= MAX_RANGES_PER_LINE {
            return Err(NoteError::Oversized);
        }
        let (start, end) = match part.split_once('-') {
            Some((start, end)) => (parse_line_number(start)?, parse_line_number(end)?),
            None => {
                let line = parse_line_number(part)?;
                (line, line)
            }
        };
        if start > end {
            return Err(NoteError::Invalid("line range ends before it starts"));
        }
        out.push((start, end));
    }
    Ok(())
}

fn parse_line_number(text: &str) -> Result<u32, NoteError> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(NoteError::Invalid("line number is not a positive integer"));
    }
    // Nine digits always fit a u32; anything longer is past the bound anyway.
    if text.len() > 9 {
        return Err(NoteError::Oversized);
    }
    let line: u32 = text
        .parse()
        .map_err(|_| NoteError::Invalid("line number is not a positive integer"))?;
    match line {
        0 => Err(NoteError::Invalid("line numbers start at 1")),
        line if line > MAX_LINE_NUMBER => Err(NoteError::Oversized),
        line => Ok(line),
    }
}

fn merged_length(ranges: &mut [(u32, u32)]) -> i64 {
    ranges.sort_unstable();
    let mut total = 0;
    let mut open: Option<(u32, u32)> = None;
    for &(start, end) in ranges.iter() {
        open = match open {
            Some((open_start, open_end)) if start <= open_end => {
                Some((open_start, open_end.max(end)))
            }
            Some((open_start, open_end)) => {
                total += i64::from(open_end - open_start + 1);
                Some((start, end))
            }
            None => Some((start, end)),
        };
    }
    if let Some((start, end)) = open {
        total += i64::from(end - start + 1);
    }
    total
}

fn assemble(metadata: Metadata, lines: &AttestedLines<'_>) -> Result<ParsedNote, NoteError> {
    let ai_lines = |key: &str| lines.ai.get(key).copied().unwrap_or(0);
    let mut sessions = BTreeMap::new();
    for (key, record) in metadata.sessions.into_iter().chain(metadata.prompts) {
        if !sessions.contains_key(&key) {
            let count = ai_lines(&key);
            sessions.insert(key.clone(), note_session(key, record, count)?);
        }
    }
    let mut humans = BTreeMap::new();
    for (key, record) in metadata.humans {
        bounded_key(&key)?;
        let count = lines.human.get(key.as_str()).copied().unwrap_or(0);
        humans.insert(
            key.clone(),
            NoteHuman {
                key,
                author: record.author,
                lines: count,
            },
        );
    }
    // Attested keys the metadata does not describe still count their lines.
    for (&key, &count) in &lines.ai {
        sessions
            .entry(key.to_string())
            .or_insert_with(|| unknown_session(key, count));
    }
    for (&key, &count) in &lines.human {
        humans.entry(key.to_string()).or_insert_with(|| NoteHuman {
            key: key.to_string(),
            author: None,
            lines: count,
        });
    }
    Ok(ParsedNote {
        schema_version: bounded(Some(metadata.schema_version), MAX_VERSION_BYTES)?
            .unwrap_or_default(),
        git_ai_version: bounded(metadata.git_ai_version, MAX_VERSION_BYTES)?,
        sessions: sessions.into_values().collect(),
        humans: humans.into_values().collect(),
    })
}

fn note_session(key: String, record: AgentRecord, lines: i64) -> Result<NoteSession, NoteError> {
    bounded_key(&key)?;
    let agent = record.agent_id.unwrap_or(AgentId {
        tool: None,
        id: None,
        model: None,
    });
    let model = agent
        .model
        .filter(|model| !model.is_empty() && !model.eq_ignore_ascii_case("unknown"));
    Ok(NoteSession {
        key,
        tool: bounded(agent.tool.filter(|tool| !tool.is_empty()), MAX_TOOL_BYTES)?,
        agent_session_id: agent.id.filter(|id| !id.is_empty()),
        model: bounded(model, MAX_MODEL_BYTES)?,
        human_author: record.human_author,
        lines,
    })
}

fn unknown_session(key: &str, lines: i64) -> NoteSession {
    NoteSession {
        key: key.to_string(),
        tool: None,
        agent_session_id: None,
        model: None,
        human_author: None,
        lines,
    }
}

fn bounded_key(key: &str) -> Result<(), NoteError> {
    if key.is_empty() || key.len() > MAX_KEY_BYTES {
        return Err(NoteError::Invalid(
            "metadata record key is empty or too long",
        ));
    }
    Ok(())
}

fn bounded(value: Option<String>, max_bytes: usize) -> Result<Option<String>, NoteError> {
    match value {
        Some(value) if value.len() > max_bytes || value.chars().any(char::is_control) => Err(
            NoteError::Invalid("metadata text field is too long or has control characters"),
        ),
        value => Ok(value),
    }
}

#[cfg(test)]
mod tests;
