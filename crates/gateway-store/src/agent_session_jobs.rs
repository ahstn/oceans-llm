//! Values prepared before acquiring a session or queue writer lock.
use gateway_core::{AgentAnalysisQueueRecord, StoreError};

pub(crate) struct PreparedAnalysis<'a> {
    pub record: &'a AgentAnalysisQueueRecord,
    pub id: String,
    pub session_id: String,
    pub versions: String,
    pub max_attempts: i32,
}

impl<'a> PreparedAnalysis<'a> {
    pub fn new(record: &'a AgentAnalysisQueueRecord) -> Result<Self, StoreError> {
        Ok(Self {
            record,
            id: record.queue_item_id.to_string(),
            session_id: record.agent_session_id.to_string(),
            versions: crate::shared::serialize_json(&record.desired_versions)?,
            max_attempts: i32::try_from(record.max_attempts)
                .map_err(|error| StoreError::Serialization(error.to_string()))?,
        })
    }

    pub fn for_session(
        record: &'a AgentAnalysisQueueRecord,
        session_id: gateway_core::AgentSessionId,
    ) -> Result<Self, StoreError> {
        if record.agent_session_id != session_id {
            return Err(StoreError::Serialization(
                "analysis job must share the input session".to_string(),
            ));
        }
        Self::new(record)
    }
}
