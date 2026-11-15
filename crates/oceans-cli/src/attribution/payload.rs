//! Entry payloads in the gateway's wire shape (`POST /api/v1/attribution/entries`).

use serde::{Deserialize, Serialize};

use super::repo::is_repository;

/// The gateway rejects notes over 1 MiB, so larger ones go straight to quarantine.
pub const MAX_NOTE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Payload {
    Note(NotePayload),
    Session(SessionPayload),
    PullRequests(PullRequestsPayload),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotePayload {
    /// Normalised `host/org/repo`, or empty when the remote couldn't be normalised.
    pub repository: String,
    pub commit_sha: String,
    pub branch: String,
    pub is_default_branch: bool,
    pub note_content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Harness {
    ClaudeCode,
    OhMyPi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionEvent {
    Start,
    End,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionPayload {
    pub harness: Harness,
    pub session_id: String,
    pub event: SessionEvent,
    pub repository: Option<String>,
    pub branch: Option<String>,
    pub head_sha: Option<String>,
    pub harness_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PullRequestsPayload {
    pub repository: String,
    pub pull_requests: Vec<PullRequest>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub base_ref: String,
    pub head_ref: String,
    pub merge_commit_sha: Option<String>,
    pub merged_at: String,
    pub commits: Vec<PullRequestCommit>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PullRequestCommit {
    pub sha: String,
    pub subject: String,
    pub authored_at: Option<String>,
    pub author_email: Option<String>,
}

impl Payload {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Note(_) => "note",
            Self::Session(_) => "session",
            Self::PullRequests(_) => "pull_requests",
        }
    }

    /// The repository and commit a queue listing may show. Nothing else leaves the payload.
    pub fn listing(&self) -> (Option<&str>, Option<&str>) {
        fn present(value: &str) -> Option<&str> {
            (!value.is_empty()).then_some(value)
        }
        match self {
            Self::Note(note) => (
                present(&note.repository),
                Some(note.commit_sha.as_str()).filter(|sha| is_commit_sha(sha)),
            ),
            Self::Session(session) => (session.repository.as_deref(), session.head_sha.as_deref()),
            Self::PullRequests(prs) => (present(&prs.repository), None),
        }
    }

    /// Checks the gateway would reject before sending, so a bad entry never blocks the queue.
    pub fn local_rejection(&self) -> Option<&'static str> {
        match self {
            Self::Note(note) if note.note_content.len() > MAX_NOTE_BYTES => Some("oversized_note"),
            Self::Note(note) if !is_repository(&note.repository) => Some("invalid_repository"),
            Self::Note(note) if !is_commit_sha(&note.commit_sha) => Some("invalid_entry"),
            Self::Session(session)
                if session
                    .repository
                    .as_deref()
                    .is_some_and(|repository| !is_repository(repository)) =>
            {
                Some("invalid_repository")
            }
            Self::PullRequests(prs) if !is_repository(&prs.repository) => {
                Some("invalid_repository")
            }
            _ => None,
        }
    }
}

/// 40 (SHA-1) or 64 (SHA-256) lowercase hex digits.
pub fn is_commit_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}
