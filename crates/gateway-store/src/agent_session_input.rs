//! Serialization and bounds shared by both session-input transaction implementations.
use std::collections::HashSet;

use gateway_core::{
    AgentObservationSetRecord, AgentSessionInput, AgentSessionRequestLinkRecord,
    MAX_AGENT_SESSION_NESTED_FACTS, SessionLifecycleState, StoreError,
};
use serde::Serialize;

use crate::shared::{datetime_to_unix_millis, serialize_json};

#[derive(sqlx::FromRow)]
pub(crate) struct SessionWriteState {
    // FromRow supplies 'a for the row borrow; the enum conversion retains no borrow.
    #[sqlx(try_from = "&'a str")]
    pub lifecycle: SessionLifecycleState,
    pub request_count: i64,
    pub next_ordinal: i64,
    pub nested_fact_count: i64,
}

pub(crate) struct PreparedInput<'a> {
    pub session_id: String,
    pub request: Option<PreparedRequest<'a>>,
    pub observations: Option<PreparedObservations<'a>>,
    pub analysis: Option<crate::agent_session_jobs::PreparedAnalysis<'a>>,
    pub wake_at: i64,
}

pub(crate) struct PreparedRequest<'a> {
    pub record: &'a AgentSessionRequestLinkRecord,
    pub log_id: Option<String>,
    pub usage_id: Option<String>,
    pub confidence: String,
    pub limitations: String,
    pub occurred_at: i64,
    pub completed_at: Option<i64>,
    pub activity_at: i64,
}

pub(crate) struct PreparedObservations<'a> {
    pub full: PreparedSet<'a>,
    pub truncated: Option<PreparedSet<'a>>,
    pub maximum_nested_facts: i64,
}

pub(crate) struct PreparedSet<'a> {
    pub record: &'a AgentObservationSetRecord,
    pub id: String,
    pub coverage: String,
    pub watermark: i64,
    pub nested_facts: i64,
    pub rows: Vec<PreparedObservation<'a>>,
}

pub(crate) struct PreparedObservation<'a> {
    pub id: String,
    pub kind: String,
    pub request_id: &'a str,
    pub evidence: String,
    pub occurred_at: i64,
    pub facts: String,
    pub limitations: String,
}

impl<'a> PreparedInput<'a> {
    pub fn new(input: AgentSessionInput<'a>) -> Result<Self, StoreError> {
        let session_id = input
            .request
            .map(|request| request.agent_session_id)
            .or_else(|| input.observations.map(|sets| sets.set.agent_session_id))
            .ok_or_else(|| {
                StoreError::Serialization("session input must contain facts".to_string())
            })?;
        let request = input.request.map(PreparedRequest::new).transpose()?;
        let observations = input
            .observations
            .map(|sets| {
                if sets.set.agent_session_id != session_id
                    || sets.truncated_set.agent_session_id != session_id
                    || sets.set.observation_set_id != sets.truncated_set.observation_set_id
                {
                    return Err(StoreError::Serialization(
                        "session input must share session and observation-set IDs".to_string(),
                    ));
                }
                Ok(PreparedObservations {
                    full: PreparedSet::new(sets.set)?,
                    truncated: (!std::ptr::eq(sets.set, sets.truncated_set))
                        .then(|| PreparedSet::new(sets.truncated_set))
                        .transpose()?,
                    maximum_nested_facts: i64::try_from(
                        sets.maximum_nested_facts
                            .min(MAX_AGENT_SESSION_NESTED_FACTS),
                    )
                    .map_err(|error| StoreError::Serialization(error.to_string()))?,
                })
            })
            .transpose()?;
        let analysis = input
            .analysis
            .map(|item| crate::agent_session_jobs::PreparedAnalysis::for_session(item, session_id))
            .transpose()?;
        let wake_at = input.observations.map_or_else(
            || {
                request
                    .as_ref()
                    .map_or(0, |request| request.activity_at / 1000)
            },
            |sets| sets.set.created_at.unix_timestamp(),
        );
        Ok(Self {
            session_id: session_id.to_string(),
            request,
            observations,
            analysis,
            wake_at,
        })
    }
}

impl<'a> PreparedRequest<'a> {
    fn new(record: &'a AgentSessionRequestLinkRecord) -> Result<Self, StoreError> {
        Ok(Self {
            record,
            log_id: record.request_log_id.map(|id| id.to_string()),
            usage_id: record.usage_event_id.map(|id| id.to_string()),
            confidence: enum_name(record.correlation_confidence)?,
            limitations: serialize_json(&record.limitation_codes)?,
            occurred_at: datetime_to_unix_millis(record.occurred_at)?,
            completed_at: record
                .completed_at
                .map(datetime_to_unix_millis)
                .transpose()?,
            activity_at: datetime_to_unix_millis(
                record.completed_at.unwrap_or(record.occurred_at),
            )?,
        })
    }
}

impl<'a> PreparedSet<'a> {
    fn new(record: &'a AgentObservationSetRecord) -> Result<Self, StoreError> {
        let mut ids = HashSet::with_capacity(record.observations.len());
        let mut nested_facts = 0_i64;
        let mut rows = Vec::with_capacity(record.observations.len());
        for observation in &record.observations {
            if !ids.insert(observation.observation_id)
                || observation.parser_version != record.parser_version
            {
                return Err(StoreError::Serialization(
                    "observation IDs must be unique and parser versions must match".to_string(),
                ));
            }
            let facts = &observation.facts;
            let count = facts
                .supplied_tools
                .len()
                .saturating_add(facts.supplied_skills.len())
                .saturating_add(facts.file_interactions.len());
            nested_facts = nested_facts.saturating_add(i64::try_from(count).unwrap_or(i64::MAX));
            rows.push(PreparedObservation {
                id: observation.observation_id.to_string(),
                kind: enum_name(observation.kind)?,
                request_id: &observation.source_request_id,
                evidence: enum_name(observation.evidence)?,
                occurred_at: observation.occurred_at.unix_timestamp(),
                facts: serialize_json(facts)?,
                limitations: serialize_json(&observation.limitations)?,
            });
        }
        rows.sort_unstable_by(|left, right| left.id.cmp(&right.id));
        Ok(Self {
            record,
            id: record.observation_set_id.to_string(),
            coverage: serialize_json(&record.coverage)?,
            watermark: datetime_to_unix_millis(record.source_watermark_at)?,
            nested_facts,
            rows,
        })
    }
}

impl PreparedObservations<'_> {
    pub fn replay_kind(&self, existing: &AgentObservationSetRecord) -> Result<bool, StoreError> {
        if crate::shared::agent_observation_set_matches(existing, self.full.record) {
            return Ok(false);
        }
        if self
            .truncated
            .as_ref()
            .is_some_and(|set| crate::shared::agent_observation_set_matches(existing, set.record))
        {
            return Ok(true);
        }
        Err(StoreError::Conflict(format!(
            "agent observation set `{}` conflicts with the existing record",
            existing.observation_set_id
        )))
    }

    pub fn select(&self, prior_nested_facts: i64) -> Result<(&PreparedSet<'_>, bool), StoreError> {
        if prior_nested_facts.saturating_add(self.full.nested_facts) <= self.maximum_nested_facts {
            return Ok((&self.full, false));
        }
        let truncated = self.truncated.as_ref().unwrap_or(&self.full);
        if truncated.nested_facts
            > self
                .maximum_nested_facts
                .saturating_sub(prior_nested_facts)
                .max(0)
        {
            return Err(StoreError::Serialization(
                "truncated observations exceed the nested-fact budget".to_string(),
            ));
        }
        Ok((truncated, true))
    }
}

fn enum_name(value: impl Serialize) -> Result<String, StoreError> {
    serde_json::to_value(value)
        .map_err(|error| StoreError::Serialization(error.to_string()))?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| StoreError::Serialization("enum must serialize as a string".to_string()))
}
