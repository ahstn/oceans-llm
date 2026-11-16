//! `GET /api/v1/me/ownership` and `POST /api/v1/attribution/entries`. The key is checked
//! before the body is read, and serde errors never reach a response: they can quote raw
//! session ids or emails.

use std::collections::HashMap;

use async_trait::async_trait;
use axum::{
    Json,
    extract::{FromRequestParts, State, rejection::JsonRejection},
    http::{StatusCode, header::AUTHORIZATION, request::Parts},
};
use gateway_core::{
    ATTRIBUTION_MAX_BATCH_BYTES, ApiKeyOwnerKind, AttributionPullRequestDetails,
    AttributionPullRequestLifecycle, AttributionPullRequestState, AttributionRevertEvidence,
    AttributionRevertLink, AttributionSessionEventKind, AuthError, AuthenticatedApiKey,
    GatewayError, SCOPE_ATTRIBUTION_WRITE, StoreError,
};
use gateway_service::attribution::{
    AttributionBatchInput, AttributionBatchOutcome, AttributionEntryCandidate,
    AttributionEntryInput, AttributionEntryStatus, AttributionNoteInput, AttributionPayloadInput,
    AttributionPullRequestCommitInput, AttributionPullRequestInput, AttributionPullRequestsInput,
    AttributionRejectReason, AttributionRequestSessionInput, AttributionSessionInput,
    AttributionUserLookup, attribution_owner, ingest_attribution_batch,
};
use gateway_store::{AnyStore, GatewayStore};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use utoipa::ToSchema;
use uuid::Uuid;

use super::AttributionOwnerKind;
use crate::http::{error::AppError, state::AppState};

/// A key with `attribution:write` or full access.
pub struct AttributionKey(AuthenticatedApiKey);

impl FromRequestParts<AppState> for AttributionKey {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let authorization = parts
            .headers
            .get(AUTHORIZATION)
            .map(|value| {
                value
                    .to_str()
                    .map_err(|_| AuthError::InvalidAuthorizationHeader)
            })
            .transpose()?;
        let key = state
            .service
            .authenticate_for_scope(authorization, SCOPE_ATTRIBUTION_WRITE)
            .await?;
        Ok(Self(key))
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AttributionOwnershipView {
    /// Send back as `expected_ownership_scope` with each batch.
    pub ownership_scope: String,
    pub owner_kind: AttributionOwnerKind,
    pub owner_id: String,
}

#[derive(Deserialize, ToSchema)]
pub struct AttributionBatchRequest {
    /// The `ownership_scope` from `GET /api/v1/me/ownership`.
    pub expected_ownership_scope: String,
    /// At most 100 entries. An entry without this shape is rejected on its own as
    /// `invalid_entry`.
    #[schema(value_type = Vec<AttributionEntryRequest>)]
    pub entries: Vec<Value>,
}

#[derive(Deserialize, ToSchema)]
pub struct AttributionEntryRequest {
    /// 1-64 characters of `[A-Za-z0-9._-]`, unique per installation.
    pub entry_id: String,
    pub installation_id: String,
    pub sequence: i64,
    /// RFC 3339.
    pub observed_at: String,
    #[serde(default)]
    pub late_sequenced: bool,
    pub payload: AttributionPayloadRequest,
}

#[derive(Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AttributionPayloadRequest {
    /// The git-ai note of one commit.
    Note {
        /// Normalised `host[:port]/owner/repo`, without scheme, userinfo or `.git`.
        repository: String,
        /// 40 or 64 lowercase hex characters.
        commit_sha: String,
        branch: Option<String>,
        #[serde(default)]
        is_default_branch: bool,
        /// The raw git-ai note, at most 1 MiB.
        note_content: String,
    },
    /// A harness session start or end.
    Session {
        /// The hook's harness, such as `oh_my_pi`: 1-64 characters of `[a-z0-9_-]`.
        harness: String,
        /// The raw harness session id. Only a hash bound to the key's owner is stored.
        session_id: String,
        event: AttributionSessionEventRequest,
        repository: Option<String>,
        branch: Option<String>,
        head_sha: Option<String>,
        harness_version: Option<String>,
    },
    /// Associate a gateway request with the journal that actually received its response.
    RequestSession {
        /// The gateway's request UUID, not a provider response id.
        request_id: String,
        /// The raw journal session id. Only an owner-scoped hash is stored.
        session_id: String,
    },
    /// Pull requests of one repository, open, closed or merged, with their commits.
    PullRequests {
        repository: String,
        pull_requests: Vec<AttributionPullRequestRequest>,
    },
}

#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AttributionSessionEventRequest {
    Start,
    End,
}

#[derive(Deserialize, ToSchema)]
pub struct AttributionPullRequestRequest {
    pub number: i64,
    pub title: String,
    pub url: Option<String>,
    pub base_ref: Option<String>,
    pub head_ref: Option<String>,
    pub merge_commit_sha: Option<String>,
    /// RFC 3339. Required for a merged pull request; absent for an open or closed one.
    pub merged_at: Option<String>,
    #[serde(default)]
    pub commits: Vec<AttributionPullRequestCommitRequest>,
    /// Defaults to `merged` when absent, as older hooks sent only merged pull requests.
    pub state: Option<AttributionPullRequestStateRequest>,
    pub is_draft: Option<bool>,
    /// RFC 3339.
    pub opened_at: Option<String>,
    /// RFC 3339; when it closed, merged or not.
    pub closed_at: Option<String>,
    /// RFC 3339; GitHub's last change to the pull request.
    pub updated_at: Option<String>,
    /// RFC 3339; the first review by a person other than the author.
    pub first_review_at: Option<String>,
    pub review_count: Option<i64>,
    pub approval_count: Option<i64>,
    pub changes_requested_count: Option<i64>,
    pub reviewer_count: Option<i64>,
    pub comment_count: Option<i64>,
    pub additions: Option<i64>,
    pub deletions: Option<i64>,
    pub changed_files: Option<i64>,
    /// What this pull request reverts, as the hook detected it.
    pub reverts: Option<AttributionRevertLinkRequest>,
}

#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AttributionPullRequestStateRequest {
    Open,
    /// Closed without merging.
    Closed,
    Merged,
}

#[derive(Deserialize, ToSchema)]
pub struct AttributionRevertLinkRequest {
    /// The reverted pull request in the same repository.
    pub pull_request_number: Option<i64>,
    /// Reverted commits, full or abbreviated (at least 7 characters).
    #[serde(default)]
    pub commit_shas: Vec<String>,
    pub evidence: AttributionRevertEvidenceRequest,
}

/// Strongest first; `title` is a possible revert only.
#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AttributionRevertEvidenceRequest {
    RevertCommit,
    GithubRevert,
    ChangeMatch,
    Title,
}

#[derive(Deserialize, ToSchema)]
pub struct AttributionPullRequestCommitRequest {
    pub sha: String,
    pub subject: Option<String>,
    /// RFC 3339.
    pub authored_at: Option<String>,
    /// Matched to an Oceans user or stored as a hash bound to the key's owner.
    pub author_email: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AttributionBatchResponse {
    pub ownership_scope: String,
    /// One result per entry, in request order.
    pub results: Vec<AttributionEntryResultView>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AttributionEntryResultView {
    /// Empty when the entry had no valid id.
    pub entry_id: String,
    pub status: AttributionEntryStatusView,
    /// Set only when `status` is `rejected`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<AttributionRejectReasonView>,
}

/// `rejected` entries are quarantined by the client; the others are done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AttributionEntryStatusView {
    Applied,
    Duplicate,
    Stale,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AttributionRejectReasonView {
    OversizedNote,
    InvalidNote,
    UnsupportedSchema,
    InvalidRepository,
    InvalidEntry,
}

#[utoipa::path(
    get,
    path = "/api/v1/me/ownership",
    tag = "crate::http::attribution",
    responses(
        (status = 200, body = AttributionOwnershipView),
        (status = 401, description = "Missing or invalid gateway API key"),
        (status = 403, description = "The key has neither `attribution:write` nor full access")
    ),
    security(("gateway_api_key" = []))
)]
pub async fn get_my_attribution_ownership(
    AttributionKey(key): AttributionKey,
) -> Result<Json<AttributionOwnershipView>, AppError> {
    let owner = attribution_owner(&key)?;
    let (owner_kind, owner_id) = match key.owner_kind {
        ApiKeyOwnerKind::User => (AttributionOwnerKind::User, owner.owner_user_id),
        ApiKeyOwnerKind::ServiceAccount => (
            AttributionOwnerKind::ServiceAccount,
            owner.owner_service_account_id,
        ),
    };
    let owner_id = owner_id.ok_or(AuthError::ApiKeyOwnerInvalid)?;
    Ok(Json(AttributionOwnershipView {
        ownership_scope: owner.ownership_scope_key,
        owner_kind,
        owner_id: owner_id.to_string(),
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/attribution/entries",
    tag = "crate::http::attribution",
    request_body = AttributionBatchRequest,
    responses(
        (status = 200, body = AttributionBatchResponse),
        (status = 400, description = "Not a batch, or more than 100 entries"),
        (status = 401, description = "Missing or invalid gateway API key"),
        (status = 403, description = "The key has neither `attribution:write` nor full access"),
        (status = 409, description = "`expected_ownership_scope` is not the key's owner; nothing was applied"),
        (status = 413, description = "Request body exceeds 2 MiB")
    ),
    security(("gateway_api_key" = []))
)]
pub async fn post_attribution_entries(
    State(state): State<AppState>,
    AttributionKey(key): AttributionKey,
    body: Result<Json<AttributionBatchRequest>, JsonRejection>,
) -> Result<Json<AttributionBatchResponse>, AppError> {
    let Json(body) = body.map_err(|rejection| {
        if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
            GatewayError::PayloadTooLarge {
                limit_bytes: ATTRIBUTION_MAX_BATCH_BYTES,
            }
        } else {
            GatewayError::InvalidRequest(
                "request must be JSON with `expected_ownership_scope` and an `entries` list"
                    .to_string(),
            )
        }
    })?;
    let batch = AttributionBatchInput {
        expected_ownership_scope: body.expected_ownership_scope,
        entries: body.entries.into_iter().map(entry_candidate).collect(),
    };
    let outcome = ingest_attribution_batch(
        state.store.as_ref(),
        &StoreUsers {
            store: &state.store,
            aliases: &state.attribution_email_aliases,
        },
        &key,
        batch,
        OffsetDateTime::now_utc(),
    )
    .await?;
    Ok(Json(outcome.into()))
}

/// Author matching over the store's normalized-email lookup: a user's Oceans email first,
/// then the `additional_attribution_emails` config gives them.
struct StoreUsers<'a> {
    store: &'a AnyStore,
    aliases: &'a HashMap<String, String>,
}

#[async_trait]
impl AttributionUserLookup for StoreUsers<'_> {
    async fn user_id_by_normalized_email(
        &self,
        email_normalized: &str,
    ) -> Result<Option<Uuid>, StoreError> {
        if let Some(user) = self
            .store
            .get_user_by_email_normalized(email_normalized)
            .await?
        {
            return Ok(Some(user.user_id));
        }
        let Some(oceans_email) = self.aliases.get(email_normalized) else {
            return Ok(None);
        };
        Ok(self
            .store
            .get_user_by_email_normalized(oceans_email)
            .await?
            .map(|user| user.user_id))
    }
}

fn entry_candidate(entry: Value) -> AttributionEntryCandidate {
    let entry_id = entry
        .get("entry_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    match serde_json::from_value::<AttributionEntryRequest>(entry)
        .ok()
        .and_then(AttributionEntryRequest::into_input)
    {
        Some(input) => AttributionEntryCandidate::Entry(Box::new(input)),
        None => AttributionEntryCandidate::Malformed { entry_id },
    }
}

fn parse_time(value: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(value, &Rfc3339).ok()
}

impl AttributionEntryRequest {
    fn into_input(self) -> Option<AttributionEntryInput> {
        Some(AttributionEntryInput {
            observed_at: parse_time(&self.observed_at)?,
            payload: self.payload.into_input()?,
            entry_id: self.entry_id,
            installation_id: self.installation_id,
            sequence: self.sequence,
            late_sequenced: self.late_sequenced,
        })
    }
}

impl AttributionPayloadRequest {
    fn into_input(self) -> Option<AttributionPayloadInput> {
        Some(match self {
            Self::Note {
                repository,
                commit_sha,
                branch,
                is_default_branch,
                note_content,
            } => AttributionPayloadInput::Note(AttributionNoteInput {
                repository,
                commit_sha,
                branch,
                is_default_branch,
                note_content,
            }),
            Self::Session {
                harness,
                session_id,
                event,
                repository,
                branch,
                head_sha,
                harness_version,
            } => AttributionPayloadInput::Session(AttributionSessionInput {
                harness,
                session_id,
                event: match event {
                    AttributionSessionEventRequest::Start => AttributionSessionEventKind::Start,
                    AttributionSessionEventRequest::End => AttributionSessionEventKind::End,
                },
                repository,
                branch,
                head_sha,
                harness_version,
            }),
            Self::RequestSession {
                request_id,
                session_id,
            } => AttributionPayloadInput::RequestSession(AttributionRequestSessionInput {
                request_id,
                session_id,
            }),
            Self::PullRequests {
                repository,
                pull_requests,
            } => AttributionPayloadInput::PullRequests(AttributionPullRequestsInput {
                repository,
                pull_requests: pull_requests
                    .into_iter()
                    .map(AttributionPullRequestRequest::into_input)
                    .collect::<Option<_>>()?,
            }),
        })
    }
}

impl AttributionPullRequestRequest {
    fn into_input(self) -> Option<AttributionPullRequestInput> {
        let commits = self
            .commits
            .into_iter()
            .map(AttributionPullRequestCommitRequest::into_input)
            .collect::<Option<_>>()?;
        let optional_time = |value: Option<String>| match value.as_deref() {
            Some(value) => parse_time(value).map(Some),
            None => Some(None),
        };
        let merged_at = optional_time(self.merged_at)?;
        // Hooks that send a lifecycle always send its state.
        let lifecycle_reported = self.state.is_some();
        let state = match self.state {
            Some(AttributionPullRequestStateRequest::Open) => AttributionPullRequestState::Open,
            Some(AttributionPullRequestStateRequest::Closed) => AttributionPullRequestState::Closed,
            Some(AttributionPullRequestStateRequest::Merged) | None => {
                AttributionPullRequestState::Merged
            }
        };
        let reverts = self.reverts.map(|link| AttributionRevertLink {
            pull_request_number: link.pull_request_number,
            commit_shas: link.commit_shas,
            evidence: match link.evidence {
                AttributionRevertEvidenceRequest::RevertCommit => {
                    AttributionRevertEvidence::RevertCommit
                }
                AttributionRevertEvidenceRequest::GithubRevert => {
                    AttributionRevertEvidence::GithubRevert
                }
                AttributionRevertEvidenceRequest::ChangeMatch => {
                    AttributionRevertEvidence::ChangeMatch
                }
                AttributionRevertEvidenceRequest::Title => AttributionRevertEvidence::Title,
            },
        });
        let lifecycle = AttributionPullRequestLifecycle {
            state,
            opened_at: optional_time(self.opened_at)?,
            closed_at: optional_time(self.closed_at)?,
            last_activity_at: optional_time(self.updated_at)?,
            details: AttributionPullRequestDetails {
                is_draft: self.is_draft,
                first_review_at: optional_time(self.first_review_at)?,
                review_count: self.review_count,
                approval_count: self.approval_count,
                changes_requested_count: self.changes_requested_count,
                reviewer_count: self.reviewer_count,
                comment_count: self.comment_count,
                additions: self.additions,
                deletions: self.deletions,
                changed_files: self.changed_files,
                reverts,
            },
        };
        Some(AttributionPullRequestInput {
            merged_at,
            number: self.number,
            title: self.title,
            url: self.url,
            base_ref: self.base_ref,
            head_ref: self.head_ref,
            merge_commit_sha: self.merge_commit_sha,
            commits,
            lifecycle,
            lifecycle_reported,
        })
    }
}

impl AttributionPullRequestCommitRequest {
    fn into_input(self) -> Option<AttributionPullRequestCommitInput> {
        let authored_at = match self.authored_at.as_deref() {
            Some(value) => Some(parse_time(value)?),
            None => None,
        };
        Some(AttributionPullRequestCommitInput {
            sha: self.sha,
            subject: self.subject,
            authored_at,
            author_email: self.author_email,
        })
    }
}

impl From<AttributionRejectReason> for AttributionRejectReasonView {
    fn from(reason: AttributionRejectReason) -> Self {
        match reason {
            AttributionRejectReason::OversizedNote => Self::OversizedNote,
            AttributionRejectReason::InvalidNote => Self::InvalidNote,
            AttributionRejectReason::UnsupportedSchema => Self::UnsupportedSchema,
            AttributionRejectReason::InvalidRepository => Self::InvalidRepository,
            AttributionRejectReason::InvalidEntry => Self::InvalidEntry,
        }
    }
}

impl From<AttributionBatchOutcome> for AttributionBatchResponse {
    fn from(outcome: AttributionBatchOutcome) -> Self {
        let results = outcome
            .results
            .into_iter()
            .map(|result| AttributionEntryResultView {
                entry_id: result.entry_id,
                status: match result.status {
                    AttributionEntryStatus::Applied => AttributionEntryStatusView::Applied,
                    AttributionEntryStatus::Duplicate => AttributionEntryStatusView::Duplicate,
                    AttributionEntryStatus::Stale => AttributionEntryStatusView::Stale,
                    AttributionEntryStatus::Rejected(_) => AttributionEntryStatusView::Rejected,
                },
                reason: result.status.reason().map(Into::into),
            })
            .collect();
        Self {
            ownership_scope: outcome.ownership_scope,
            results,
        }
    }
}
