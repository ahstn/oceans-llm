//! `POST /api/v1/attribution/entries`: validates each queued entry and applies it.
//!
//! A malformed entry is rejected on its own with a reason code; only an ownership mismatch,
//! an oversized batch or a store failure fails the whole request.

use std::collections::{BTreeSet, HashMap};

use gateway_core::{
    ATTRIBUTION_MAX_BATCH_ENTRIES, ApiKeyOwnerKind, AttributionApplyOutcome,
    AttributionCommitSession, AttributionCommitWrite, AttributionFreshness, AttributionOwner,
    AttributionPullRequestCommit, AttributionPullRequestWrite, AttributionRepository,
    AttributionSessionEventKind, AttributionSessionEventWrite, AuthError, AuthenticatedApiKey,
    GatewayError, StoreError,
};
use time::OffsetDateTime;
use uuid::Uuid;

use super::note::{self, NoteError, ParsedNote};
use super::validate::{self, MAX_BRANCH_BYTES, MAX_HARNESS_VERSION_BYTES, MAX_TITLE_BYTES};
use super::{AttributionEntryStatus, AttributionRejectReason, AttributionUserLookup};
use crate::agent_analysis::hash_lineage_candidate;
use crate::budget_scopes::usage_ownership_scope_key;
use crate::client_session::{SESSION_HASH_NAMESPACES, is_external_session_id};

/// Lineage namespace for author email hashes (ADR "Identity storage").
const HUMAN_HASH_NAMESPACE: &str = "git_ai_human";
const MAX_PULL_REQUESTS_PER_ENTRY: usize = 100;
const MAX_COMMITS_PER_PULL_REQUEST: usize = 1_000;

/// One ingestion request. Inputs carrying raw session ids or emails do not implement `Debug`.
pub struct AttributionBatchInput {
    /// The scope the client resolved through `GET /api/v1/me/ownership`.
    pub expected_ownership_scope: String,
    pub entries: Vec<AttributionEntryCandidate>,
}

pub enum AttributionEntryCandidate {
    Entry(Box<AttributionEntryInput>),
    /// JSON that does not have the entry shape. `entry_id` is whatever the raw entry held.
    Malformed {
        entry_id: Option<String>,
    },
}

pub struct AttributionEntryInput {
    pub entry_id: String,
    pub installation_id: String,
    pub sequence: i64,
    pub observed_at: OffsetDateTime,
    pub late_sequenced: bool,
    pub payload: AttributionPayloadInput,
}

pub enum AttributionPayloadInput {
    Note(AttributionNoteInput),
    Session(AttributionSessionInput),
    PullRequests(AttributionPullRequestsInput),
}

pub struct AttributionNoteInput {
    pub repository: String,
    pub commit_sha: String,
    pub branch: Option<String>,
    pub is_default_branch: bool,
    /// The raw git-ai note.
    pub note_content: String,
}

pub struct AttributionSessionInput {
    pub harness: String,
    /// The raw harness session id; only its lineage hash is stored.
    pub session_id: String,
    pub event: AttributionSessionEventKind,
    pub repository: Option<String>,
    pub branch: Option<String>,
    pub head_sha: Option<String>,
    pub harness_version: Option<String>,
}

pub struct AttributionPullRequestsInput {
    pub repository: String,
    pub pull_requests: Vec<AttributionPullRequestInput>,
}

pub struct AttributionPullRequestInput {
    pub number: i64,
    pub title: String,
    pub url: Option<String>,
    pub base_ref: Option<String>,
    pub head_ref: Option<String>,
    pub merge_commit_sha: Option<String>,
    pub merged_at: OffsetDateTime,
    pub commits: Vec<AttributionPullRequestCommitInput>,
}

pub struct AttributionPullRequestCommitInput {
    pub sha: String,
    pub subject: Option<String>,
    pub authored_at: Option<OffsetDateTime>,
    /// The raw author email; matched to a user or hashed, never stored.
    pub author_email: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributionEntryResult {
    /// The entry's id, or empty when the entry had no valid one.
    pub entry_id: String,
    pub status: AttributionEntryStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributionBatchOutcome {
    pub ownership_scope: String,
    /// One result per entry, in request order.
    pub results: Vec<AttributionEntryResult>,
}

/// The ownership columns attribution rows copy from the ingesting key.
pub fn attribution_owner(api_key: &AuthenticatedApiKey) -> Result<AttributionOwner, AuthError> {
    let ownership_scope_key = usage_ownership_scope_key(api_key)?;
    let (owner_user_id, owner_service_account_id) = match api_key.owner_kind {
        ApiKeyOwnerKind::User => (api_key.owner_user_id, None),
        ApiKeyOwnerKind::ServiceAccount => (None, api_key.owner_service_account_id),
    };
    Ok(AttributionOwner {
        ownership_scope_key,
        owner_user_id,
        owner_service_account_id,
    })
}

/// Applies a batch for `api_key`. Entries are applied in order; a store failure part-way
/// leaves the earlier entries applied, and the client's retry reads them as duplicates.
pub async fn ingest_attribution_batch<S, U>(
    store: &S,
    users: &U,
    api_key: &AuthenticatedApiKey,
    batch: AttributionBatchInput,
    now: OffsetDateTime,
) -> Result<AttributionBatchOutcome, GatewayError>
where
    S: AttributionRepository + Sync,
    U: AttributionUserLookup,
{
    if batch.entries.len() > ATTRIBUTION_MAX_BATCH_ENTRIES {
        return Err(GatewayError::InvalidRequest(format!(
            "a batch holds at most {ATTRIBUTION_MAX_BATCH_ENTRIES} entries"
        )));
    }
    let owner = attribution_owner(api_key)?;
    if batch.expected_ownership_scope != owner.ownership_scope_key {
        return Err(StoreError::Conflict(
            "expected_ownership_scope is not this API key's owner; look up the owner again"
                .to_string(),
        )
        .into());
    }
    let mut ingest = Ingest {
        store,
        authors: AuthorMatcher::new(users, &owner.ownership_scope_key),
        owner: &owner,
        now,
    };
    let mut results = Vec::with_capacity(batch.entries.len());
    for candidate in batch.entries {
        results.push(match candidate {
            AttributionEntryCandidate::Entry(entry) => ingest.entry(*entry).await?,
            AttributionEntryCandidate::Malformed { entry_id } => AttributionEntryResult {
                entry_id: entry_id
                    .filter(|id| validate::is_entry_id(id))
                    .unwrap_or_default(),
                status: AttributionEntryStatus::Rejected(AttributionRejectReason::InvalidEntry),
            },
        });
    }
    Ok(AttributionBatchOutcome {
        ownership_scope: owner.ownership_scope_key,
        results,
    })
}

enum EntryError {
    Rejected(AttributionRejectReason),
    Store(StoreError),
}

impl From<AttributionRejectReason> for EntryError {
    fn from(reason: AttributionRejectReason) -> Self {
        Self::Rejected(reason)
    }
}

impl From<StoreError> for EntryError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

fn note_reason(error: NoteError) -> AttributionRejectReason {
    match error {
        NoteError::Oversized => AttributionRejectReason::OversizedNote,
        NoteError::Invalid(_) => AttributionRejectReason::InvalidNote,
        NoteError::UnsupportedSchema => AttributionRejectReason::UnsupportedSchema,
    }
}

struct Ingest<'a, S, U> {
    store: &'a S,
    authors: AuthorMatcher<'a, U>,
    owner: &'a AttributionOwner,
    now: OffsetDateTime,
}

impl<S, U> Ingest<'_, S, U>
where
    S: AttributionRepository + Sync,
    U: AttributionUserLookup,
{
    async fn entry(
        &mut self,
        entry: AttributionEntryInput,
    ) -> Result<AttributionEntryResult, StoreError> {
        if !validate::is_entry_id(&entry.entry_id) {
            return Ok(AttributionEntryResult {
                entry_id: String::new(),
                status: AttributionEntryStatus::Rejected(AttributionRejectReason::InvalidEntry),
            });
        }
        let status = match self.apply(entry.freshness(), entry.payload).await {
            Ok(outcome) => outcome.into(),
            Err(EntryError::Rejected(reason)) => AttributionEntryStatus::Rejected(reason),
            Err(EntryError::Store(error)) => return Err(error),
        };
        Ok(AttributionEntryResult {
            entry_id: entry.entry_id,
            status,
        })
    }

    async fn apply(
        &mut self,
        freshness: Result<AttributionFreshness, AttributionRejectReason>,
        payload: AttributionPayloadInput,
    ) -> Result<AttributionApplyOutcome, EntryError> {
        let freshness = freshness?;
        match payload {
            AttributionPayloadInput::Note(note) => self.note(freshness, note).await,
            AttributionPayloadInput::Session(session) => self.session(&freshness, session).await,
            AttributionPayloadInput::PullRequests(input) => {
                self.pull_requests(&freshness, input).await
            }
        }
    }

    async fn note(
        &mut self,
        freshness: AttributionFreshness,
        input: AttributionNoteInput,
    ) -> Result<AttributionApplyOutcome, EntryError> {
        let repository = validate::repository(&input.repository)?;
        let commit_sha = validate::commit_sha(&input.commit_sha)?;
        let branch = validate::optional_text(input.branch, MAX_BRANCH_BYTES)?;
        let parsed = note::parse_note(&input.note_content).map_err(note_reason)?;
        let sessions = self.commit_sessions(&parsed).await?;
        let author = match self.note_author() {
            Some(owner) => owner,
            None => self.authors.first_user(note_authors(&parsed)).await?,
        };
        let write = AttributionCommitWrite {
            attribution_commit_id: Uuid::new_v4(),
            owner: self.owner.clone(),
            repository,
            commit_sha,
            branch,
            is_default_branch: input.is_default_branch,
            author_user_id: author.user_id,
            author_email_hash: author.email_hash,
            ai_lines: parsed.ai_lines(),
            human_lines: parsed.human_lines(),
            schema_version: parsed.schema_version,
            git_ai_version: parsed.git_ai_version,
            freshness,
            sessions,
            received_at: self.now,
        };
        Ok(self.store.apply_attribution_commit(&write).await?)
    }

    async fn commit_sessions(
        &mut self,
        parsed: &ParsedNote,
    ) -> Result<Vec<AttributionCommitSession>, StoreError> {
        let scope = self.owner.ownership_scope_key.as_str();
        let mut sessions = Vec::with_capacity(parsed.sessions.len());
        for session in &parsed.sessions {
            let tool = session.tool.as_deref().unwrap_or("unknown");
            let human = match (self.note_author(), session.human_author.as_deref()) {
                (Some(owner), _) => owner,
                (None, Some(author)) => self.authors.first_user([author].into_iter()).await?,
                (None, None) => AuthorMatch::default(),
            };
            sessions.push(AttributionCommitSession {
                note_session_key: session.key.clone(),
                agent_tool: tool.to_string(),
                agent_model: session.model.clone(),
                harness_key: display_harness(tool).map(str::to_string),
                session_hashes: session
                    .agent_session_id
                    .as_deref()
                    .filter(|id| is_external_session_id(id))
                    .map(|id| session_hashes(scope, id))
                    .unwrap_or_default(),
                ai_lines: session.lines,
                human_author_user_id: human.user_id,
                human_author_hash: human.email_hash,
            });
        }
        Ok(sessions)
    }

    /// The author of every commit and session in a note sent with a user's key: that user.
    /// git-ai runs the notes hook on the machine that made the commit, so the key owner is the
    /// developer, whatever email their Git config uses. `None` for a service account key,
    /// whose notes match author emails instead.
    fn note_author(&self) -> Option<AuthorMatch> {
        self.owner.owner_user_id.map(|user_id| AuthorMatch {
            user_id: Some(user_id),
            email_hash: None,
        })
    }

    async fn session(
        &mut self,
        freshness: &AttributionFreshness,
        input: AttributionSessionInput,
    ) -> Result<AttributionApplyOutcome, EntryError> {
        if !validate::is_harness_key(&input.harness) || !is_external_session_id(&input.session_id) {
            return Err(AttributionRejectReason::InvalidEntry.into());
        }
        let scope = self.owner.ownership_scope_key.as_str();
        let write = AttributionSessionEventWrite {
            owner: self.owner.clone(),
            session_hash: hash_lineage_candidate(scope, &input.harness, &input.session_id),
            session_hashes: session_hashes(scope, &input.session_id),
            harness_key: input.harness,
            event: input.event,
            repository: input
                .repository
                .map(|repository| validate::repository(&repository))
                .transpose()?,
            branch: validate::optional_text(input.branch, MAX_BRANCH_BYTES)?,
            head_sha: input
                .head_sha
                .map(|sha| validate::commit_sha(&sha))
                .transpose()?,
            harness_version: validate::optional_text(
                input.harness_version,
                MAX_HARNESS_VERSION_BYTES,
            )?,
            observed_at: freshness.observed_at,
            received_at: self.now,
        };
        Ok(self.store.apply_attribution_session_event(&write).await?)
    }

    async fn pull_requests(
        &mut self,
        freshness: &AttributionFreshness,
        input: AttributionPullRequestsInput,
    ) -> Result<AttributionApplyOutcome, EntryError> {
        let repository = validate::repository(&input.repository)?;
        if input.pull_requests.len() > MAX_PULL_REQUESTS_PER_ENTRY {
            return Err(AttributionRejectReason::InvalidEntry.into());
        }
        // Check every pull request before storing any, so a rejected entry stores nothing.
        let mut writes = Vec::with_capacity(input.pull_requests.len());
        for pull_request in input.pull_requests {
            writes.push(
                self.pull_request_write(&repository, freshness, pull_request)
                    .await?,
            );
        }
        let mut outcomes = Vec::with_capacity(writes.len());
        for write in &writes {
            outcomes.push(self.store.apply_attribution_pull_request(write).await?);
        }
        Ok(combined_outcome(&outcomes))
    }

    async fn pull_request_write(
        &mut self,
        repository: &str,
        freshness: &AttributionFreshness,
        input: AttributionPullRequestInput,
    ) -> Result<AttributionPullRequestWrite, EntryError> {
        if input.number < 1 || input.commits.len() > MAX_COMMITS_PER_PULL_REQUEST {
            return Err(AttributionRejectReason::InvalidEntry.into());
        }
        let title = validate::optional_text(Some(input.title), MAX_TITLE_BYTES)?;
        let url = input
            .url
            .map(|url| validate::pull_request_url(&url))
            .transpose()?;
        let base_ref = validate::optional_text(input.base_ref, MAX_BRANCH_BYTES)?;
        let head_ref = validate::optional_text(input.head_ref, MAX_BRANCH_BYTES)?;
        let merge_commit_sha = input
            .merge_commit_sha
            .map(|sha| validate::commit_sha(&sha))
            .transpose()?;
        let mut commits = Vec::with_capacity(input.commits.len());
        for commit in input.commits {
            let commit_sha = validate::commit_sha(&commit.sha)?;
            let subject = validate::optional_text(commit.subject, MAX_TITLE_BYTES)?;
            let author = match commit.author_email.as_deref() {
                Some(email) => self.authors.first_user([email].into_iter()).await?,
                None => AuthorMatch::default(),
            };
            commits.push(AttributionPullRequestCommit {
                commit_sha,
                subject,
                authored_at: commit.authored_at,
                author_user_id: author.user_id,
                author_email_hash: author.email_hash,
            });
        }
        // The first commit author who is an Oceans user, else the first author's hash.
        let author_user_id = commits.iter().find_map(|commit| commit.author_user_id);
        let author_email_hash = author_user_id
            .is_none()
            .then(|| {
                commits
                    .iter()
                    .find_map(|commit| commit.author_email_hash.clone())
            })
            .flatten();
        Ok(AttributionPullRequestWrite {
            attribution_pull_request_id: Uuid::new_v4(),
            owner: self.owner.clone(),
            repository: repository.to_string(),
            number: input.number,
            title: title.unwrap_or_default(),
            url,
            base_ref,
            head_ref,
            merge_commit_sha,
            merged_at: input.merged_at,
            author_user_id,
            author_email_hash,
            commits,
            observed_at: freshness.observed_at,
            received_at: self.now,
        })
    }
}

impl AttributionEntryInput {
    fn freshness(&self) -> Result<AttributionFreshness, AttributionRejectReason> {
        let installation_id = Uuid::parse_str(&self.installation_id)
            .map_err(|_| AttributionRejectReason::InvalidEntry)?;
        if self.sequence < 0 {
            return Err(AttributionRejectReason::InvalidEntry);
        }
        Ok(AttributionFreshness {
            installation_id: installation_id.to_string(),
            sequence: self.sequence,
            observed_at: self.observed_at,
            late_sequenced: self.late_sequenced,
        })
    }
}

/// The Oceans harness a git-ai `agent_id.tool` is shown as. Only a label: the join hashes the
/// session id under every namespace instead.
fn display_harness(tool: &str) -> Option<&'static str> {
    match tool {
        "claude" => Some("claude_code"),
        "codex" => Some("codex"),
        "opencode" => Some("opencode"),
        // git-ai's `pi` preset is driven by both pi and Oh My Pi; Oceans hooks OMP.
        "pi" => Some("oh_my_pi"),
        _ => None,
    }
}

/// A session id hashed under every namespace the gateway records request sessions under,
/// ascending, so the session finds its requests whichever harness the gateway classified them
/// as. Only ids the gateway would accept from a request can match.
fn session_hashes(scope: &str, session_id: &str) -> Vec<String> {
    SESSION_HASH_NAMESPACES
        .iter()
        .map(|namespace| hash_lineage_candidate(scope, namespace, session_id))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Author strings in the order that picks a commit's author: AI session and prompt authors by
/// key, then known-human authors by key.
fn note_authors(parsed: &ParsedNote) -> impl Iterator<Item = &str> {
    parsed
        .sessions
        .iter()
        .filter_map(|session| session.human_author.as_deref())
        .chain(
            parsed
                .humans
                .iter()
                .filter_map(|human| human.author.as_deref()),
        )
}

/// Any applied PR makes the entry applied; all duplicates make it a duplicate; else stale.
fn combined_outcome(outcomes: &[AttributionApplyOutcome]) -> AttributionApplyOutcome {
    if outcomes.is_empty() || outcomes.contains(&AttributionApplyOutcome::Applied) {
        AttributionApplyOutcome::Applied
    } else if outcomes
        .iter()
        .all(|outcome| *outcome == AttributionApplyOutcome::Duplicate)
    {
        AttributionApplyOutcome::Duplicate
    } else {
        AttributionApplyOutcome::Stale
    }
}

#[derive(Default)]
struct AuthorMatch {
    user_id: Option<Uuid>,
    email_hash: Option<String>,
}

/// Matches author emails to Oceans users, caching lookups for one batch. Holds raw emails in
/// memory only.
struct AuthorMatcher<'a, U> {
    users: &'a U,
    scope: &'a str,
    cache: HashMap<String, Option<Uuid>>,
}

impl<'a, U: AttributionUserLookup> AuthorMatcher<'a, U> {
    fn new(users: &'a U, scope: &'a str) -> Self {
        Self {
            users,
            scope,
            cache: HashMap::new(),
        }
    }

    /// The first author whose email belongs to an Oceans user, else the hash of the first
    /// author with an email.
    async fn first_user<'s>(
        &mut self,
        authors: impl Iterator<Item = &'s str>,
    ) -> Result<AuthorMatch, StoreError> {
        let mut first_unmatched = None;
        for email in authors.filter_map(note::author_email) {
            if let Some(user_id) = self.user_id(&email).await? {
                return Ok(AuthorMatch {
                    user_id: Some(user_id),
                    email_hash: None,
                });
            }
            first_unmatched.get_or_insert(email);
        }
        Ok(AuthorMatch {
            user_id: None,
            email_hash: first_unmatched
                .map(|email| hash_lineage_candidate(self.scope, HUMAN_HASH_NAMESPACE, &email)),
        })
    }

    async fn user_id(&mut self, email: &str) -> Result<Option<Uuid>, StoreError> {
        if let Some(found) = self.cache.get(email) {
            return Ok(*found);
        }
        let found = self.users.user_id_by_normalized_email(email).await?;
        self.cache.insert(email.to_string(), found);
        Ok(found)
    }
}
