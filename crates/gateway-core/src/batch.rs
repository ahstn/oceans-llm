use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::Money4;

pub const MAX_BATCH_PAGE_SIZE: u32 = 500;
pub const MAX_BATCH_RESULT_PAGE_SIZE: u32 = 1_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BatchEndpoint {
    ChatCompletions,
    Responses,
    Embeddings,
}

impl BatchEndpoint {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ChatCompletions => "chat_completions",
            Self::Responses => "responses",
            Self::Embeddings => "embeddings",
        }
    }

    #[must_use]
    pub const fn provider_path(self) -> &'static str {
        match self {
            Self::ChatCompletions => "/v1/chat/completions",
            Self::Responses => "/v1/responses",
            Self::Embeddings => "/v1/embeddings",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "chat_completions" => Some(Self::ChatCompletions),
            "responses" => Some(Self::Responses),
            "embeddings" => Some(Self::Embeddings),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BatchStatus {
    Queued,
    Submitting,
    SubmissionUnknown,
    Validating,
    InProgress,
    Finalizing,
    Completed,
    Failed,
    Expired,
    CancelRequested,
    Cancelling,
    Cancelled,
}

impl BatchStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Submitting => "submitting",
            Self::SubmissionUnknown => "submission_unknown",
            Self::Validating => "validating",
            Self::InProgress => "in_progress",
            Self::Finalizing => "finalizing",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Expired => "expired",
            Self::CancelRequested => "cancel_requested",
            Self::Cancelling => "cancelling",
            Self::Cancelled => "cancelled",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "queued" => Some(Self::Queued),
            "submitting" => Some(Self::Submitting),
            "submission_unknown" => Some(Self::SubmissionUnknown),
            "validating" => Some(Self::Validating),
            "in_progress" => Some(Self::InProgress),
            "finalizing" => Some(Self::Finalizing),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "expired" => Some(Self::Expired),
            "cancel_requested" => Some(Self::CancelRequested),
            "cancelling" => Some(Self::Cancelling),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }

    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::SubmissionUnknown
                | Self::Completed
                | Self::Failed
                | Self::Expired
                | Self::Cancelled
        )
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BatchItemStatus {
    Pending,
    Succeeded,
    Failed,
}

impl BatchItemStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "succeeded" => Some(Self::Succeeded),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BatchPricingStatus {
    Pending,
    Priced,
    PartiallyPriced,
    Unpriced,
    ProviderReported,
}

impl BatchPricingStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Priced => "priced",
            Self::PartiallyPriced => "partially_priced",
            Self::Unpriced => "unpriced",
            Self::ProviderReported => "provider_reported",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "priced" => Some(Self::Priced),
            "partially_priced" => Some(Self::PartiallyPriced),
            "unpriced" => Some(Self::Unpriced),
            "provider_reported" => Some(Self::ProviderReported),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchAccessScope {
    All,
    ApiKey(Uuid),
    User(Uuid),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchJobRecord {
    pub batch_id: Uuid,
    pub idempotency_key: String,
    pub request_hash: String,
    pub api_key_id: Uuid,
    pub user_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
    pub service_account_id: Option<Uuid>,
    pub model_id: Uuid,
    pub model_key: String,
    pub resolved_model_key: String,
    pub route_id: Uuid,
    pub provider_key: String,
    pub upstream_model: String,
    pub endpoint: BatchEndpoint,
    pub status: BatchStatus,
    pub provider_batch_id: Option<String>,
    pub request_count: i64,
    pub completed_count: i64,
    pub failed_count: i64,
    pub cost_usd: Option<Money4>,
    pub pricing_status: BatchPricingStatus,
    pub provider_usage: Option<Value>,
    pub error: Option<Value>,
    pub created_at: OffsetDateTime,
    pub submitted_at: Option<OffsetDateTime>,
    pub completed_at: Option<OffsetDateTime>,
    pub updated_at: OffsetDateTime,
    pub next_poll_at: Option<OffsetDateTime>,
    pub lease_owner: Option<String>,
    pub lease_expires_at: Option<OffsetDateTime>,
    pub provider_context: crate::ProviderRequestContext,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewBatchJob {
    pub job: BatchJobRecord,
    pub items: Vec<NewBatchItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewBatchItem {
    pub batch_item_id: Uuid,
    pub custom_id: String,
    pub request_body: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchItemRecord {
    pub batch_item_id: Uuid,
    pub batch_id: Uuid,
    pub custom_id: String,
    pub status: BatchItemStatus,
    pub request_body: Value,
    pub response_body: Option<Value>,
    pub error: Option<Value>,
    pub provider_request_id: Option<String>,
    pub provider_usage: Option<Value>,
    pub cost_usd: Option<Money4>,
    pub completed_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BatchQuery {
    pub page: u32,
    pub page_size: u32,
    pub status: Option<BatchStatus>,
    pub model_key: Option<String>,
    pub provider_key: Option<String>,
    pub user_id: Option<Uuid>,
    pub service_account_id: Option<Uuid>,
    pub created_at_start: Option<OffsetDateTime>,
    pub created_at_end: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchPage {
    pub items: Vec<BatchJobRecord>,
    pub page: u32,
    pub page_size: u32,
    pub total: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BatchItemQuery {
    pub page: u32,
    pub page_size: u32,
    pub status: Option<BatchItemStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchItemPage {
    pub items: Vec<BatchItemRecord>,
    pub page: u32,
    pub page_size: u32,
    pub total: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchCapabilities {
    pub chat_completions: bool,
    pub responses: bool,
    pub embeddings: bool,
    pub cancel: bool,
}

impl BatchCapabilities {
    pub const NONE: Self = Self {
        chat_completions: false,
        responses: false,
        embeddings: false,
        cancel: false,
    };

    #[must_use]
    pub const fn supports(self, endpoint: BatchEndpoint) -> bool {
        match endpoint {
            BatchEndpoint::ChatCompletions => self.chat_completions,
            BatchEndpoint::Responses => self.responses,
            BatchEndpoint::Embeddings => self.embeddings,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProviderBatchRequest {
    pub batch_id: Uuid,
    pub endpoint: BatchEndpoint,
    pub upstream_model: String,
    pub items: Vec<ProviderBatchRequestItem>,
    pub context: crate::ProviderRequestContext,
}

#[derive(Debug, Clone)]
pub struct ProviderBatchRequestItem {
    pub custom_id: String,
    pub body: Value,
}

#[derive(Debug, Clone)]
pub struct ProviderBatchState {
    pub provider_batch_id: String,
    pub status: BatchStatus,
    pub request_count: i64,
    pub completed_count: i64,
    pub failed_count: i64,
    pub provider_usage: Option<Value>,
    pub provider_cost_usd: Option<Money4>,
    pub error: Option<Value>,
    pub submitted_at: Option<OffsetDateTime>,
    pub completed_at: Option<OffsetDateTime>,
}

#[derive(Debug)]
pub enum ProviderBatchSubmission {
    Submitted(ProviderBatchState),
    NotSubmitted(crate::ProviderError),
    SubmissionUnknown(crate::ProviderError),
}

#[derive(Debug, Clone)]
pub struct ProviderBatchResult {
    pub custom_id: String,
    pub response_body: Option<Value>,
    pub error: Option<Value>,
    pub provider_request_id: Option<String>,
    pub provider_usage: Option<Value>,
    pub completed_at: Option<OffsetDateTime>,
    pub cost_usd: Option<Money4>,
}

#[derive(Debug, Clone)]
pub struct BatchPollUpdate {
    pub state: ProviderBatchState,
    pub results: Vec<ProviderBatchResult>,
    pub next_poll_at: Option<OffsetDateTime>,
    pub pricing_status: Option<BatchPricingStatus>,
}
