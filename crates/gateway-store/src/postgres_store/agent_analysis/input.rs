use super::*;
use crate::agent_session_input::SessionWriteState;
use sqlx::{PgConnection, Postgres, QueryBuilder};

const WRITE_STATE_SQL: &str = "SELECT lifecycle, request_count, next_ordinal, nested_fact_count FROM agent_sessions WHERE agent_session_id = $1 FOR NO KEY UPDATE";

impl PostgresStore {
    pub(super) async fn append_session_input(
        &self,
        input: AgentSessionInput<'_>,
    ) -> Result<AgentSessionInputAppendResult, StoreError> {
        let prepared = PreparedInput::new(input)?;
        let mut transaction = self.pool.begin().await.map_err(to_query_error)?;
        // The primary key never changes. Unlike FOR UPDATE, this writer lock is compatible
        // with queue/request foreign-key KEY SHARE readers and avoids their upgrade cycle.
        let state = sqlx::query_as::<_, SessionWriteState>(WRITE_STATE_SQL)
            .bind(&prepared.session_id)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(to_query_error)?
            .ok_or_else(|| StoreError::NotFound("agent session not found".to_string()))?;
        let mut result = AgentSessionInputAppendResult::default();
        if let Some(request) = &prepared.request {
            match append_request(&mut transaction, &prepared.session_id, request, &state).await {
                Ok(inserted) => result.request_inserted = inserted,
                Err(error @ StoreError::AgentSessionWindowClosed(_)) => {
                    if state.lifecycle == SessionLifecycleState::Open {
                        schedule_changed(&mut transaction, &prepared).await?;
                    }
                    transaction.commit().await.map_err(to_query_error)?;
                    return Err(error);
                }
                Err(error) => return Err(error),
            }
        }
        let mut nested_delta = 0;
        if let Some(observations) = &prepared.observations {
            if let Some(existing) = load_set(&mut transaction, &observations.full.id).await? {
                result.nested_facts_truncated = observations.replay_kind(&existing)?;
            } else {
                let (selected, truncated) = observations.select(state.nested_fact_count)?;
                insert_set(&mut transaction, &prepared.session_id, selected).await?;
                result.observations_inserted = true;
                result.nested_facts_truncated = truncated;
                nested_delta = selected.nested_facts;
            }
        }
        if result.request_inserted || result.observations_inserted {
            update_counters(
                &mut transaction,
                &prepared,
                result.request_inserted,
                nested_delta,
            )
            .await?;
            schedule_changed(&mut transaction, &prepared).await?;
        }
        transaction.commit().await.map_err(to_query_error)?;
        Ok(result)
    }
}

async fn schedule_changed(
    connection: &mut PgConnection,
    prepared: &PreparedInput<'_>,
) -> Result<(), StoreError> {
    if let Some(analysis) = &prepared.analysis {
        super::queue::enqueue_on(connection, analysis).await?;
    } else {
        super::queue::wake_on(connection, &prepared.session_id, prepared.wake_at).await?;
    }
    Ok(())
}

async fn append_request(
    connection: &mut PgConnection,
    session_id: &str,
    request: &PreparedRequest<'_>,
    state: &SessionWriteState,
) -> Result<bool, StoreError> {
    let link = request.record;
    let can_append = state.lifecycle == SessionLifecycleState::Open
        && state.request_count
            < i64::try_from(gateway_core::MAX_AGENT_SESSION_REQUESTS).unwrap_or(i64::MAX);
    if can_append {
        let inserted = sqlx::query(INSERT_REQUEST_SQL)
            .bind(session_id)
            .bind(&link.request_id)
            .bind(request.log_id.as_deref())
            .bind(request.usage_id.as_deref())
            .bind(state.next_ordinal)
            .bind(link.execution_id.as_deref())
            .bind(link.parent_execution_id.as_deref())
            .bind(link.normalized_session_id.as_deref())
            .bind(&request.confidence)
            .bind(&request.limitations)
            .bind(request.occurred_at)
            .bind(request.completed_at)
            .bind(link.terminal_success)
            .execute(&mut *connection)
            .await
            .map_err(to_query_error)?;
        if inserted.rows_affected() > 0 {
            return Ok(true);
        }
    }
    let sql = format!(
        "SELECT {REQUEST_COLUMNS} FROM agent_session_requests WHERE agent_session_id = $1 AND request_id = $2"
    );
    let existing = sqlx::query(&sql)
        .bind(session_id)
        .bind(&link.request_id)
        .fetch_optional(&mut *connection)
        .await
        .map_err(to_query_error)?;
    if let Some(existing) = existing {
        if !agent_session_request_matches(&decode_request_link(&existing)?, link) {
            return Err(StoreError::Conflict(format!(
                "agent session request `{}` conflicts with the existing record",
                link.request_id
            )));
        }
        return Ok(false);
    }
    if can_append {
        return Err(StoreError::Unexpected(
            "conflicting session request disappeared".to_string(),
        ));
    }
    if state.lifecycle == SessionLifecycleState::Open {
        sqlx::query(FINALIZE_FULL_SESSION_SQL)
            .bind(session_id)
            .bind(request.activity_at / 1000)
            .execute(connection)
            .await
            .map_err(to_query_error)?;
    }
    Err(StoreError::AgentSessionWindowClosed(session_id.to_string()))
}

async fn update_counters(
    connection: &mut PgConnection,
    input: &PreparedInput<'_>,
    request_inserted: bool,
    nested_delta: i64,
) -> Result<(), StoreError> {
    let request = input.request.as_ref().filter(|_| request_inserted);
    sqlx::query(UPDATE_COUNTERS_SQL)
        .bind(&input.session_id)
        .bind(i64::from(request_inserted))
        .bind(nested_delta)
        .bind(request.map(|request| request.record.occurred_at.unix_timestamp()))
        .bind(request.map(|request| request.activity_at))
        .bind(request.map(|request| request.activity_at / 1000))
        .execute(connection)
        .await
        .map_err(to_query_error)?;
    Ok(())
}

async fn insert_set(
    connection: &mut PgConnection,
    session_id: &str,
    set: &PreparedSet<'_>,
) -> Result<(), StoreError> {
    sqlx::query(INSERT_OBSERVATION_SET_SQL)
        .bind(&set.id)
        .bind(session_id)
        .bind(&set.record.parser_version)
        .bind(set.watermark)
        .bind(&set.coverage)
        .bind(set.record.created_at.unix_timestamp())
        .execute(&mut *connection)
        .await
        .map_err(to_query_error)?;
    for rows in set.rows.chunks(100) {
        let mut query = QueryBuilder::<Postgres>::new(
            "INSERT INTO agent_inferred_observations (observation_id, observation_set_id, agent_session_id, kind, source_request_id, evidence, occurred_at, facts_json, limitation_codes_json) ",
        );
        query.push_values(rows, |mut row, observation| {
            row.push_bind(&observation.id)
                .push_bind(&set.id)
                .push_bind(session_id)
                .push_bind(&observation.kind)
                .push_bind(observation.request_id)
                .push_bind(&observation.evidence)
                .push_bind(observation.occurred_at)
                .push_bind(&observation.facts)
                .push_bind(&observation.limitations);
        });
        query
            .build()
            .execute(&mut *connection)
            .await
            .map_err(to_query_error)?;
    }
    Ok(())
}

async fn load_set(
    connection: &mut PgConnection,
    id: &str,
) -> Result<Option<AgentObservationSetRecord>, StoreError> {
    let row = sqlx::query(LOAD_OBSERVATION_SET_SQL)
        .bind(id)
        .fetch_optional(&mut *connection)
        .await
        .map_err(to_query_error)?;
    let Some(row) = row else { return Ok(None) };
    let observations = sqlx::query(LOAD_OBSERVATIONS_SQL)
        .bind(id)
        .fetch_all(connection)
        .await
        .map_err(to_query_error)?
        .iter()
        .map(decode_observation)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(AgentObservationSetRecord {
        observation_set_id: parse_uuid(id)?,
        agent_session_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        parser_version: row.try_get(1).map_err(to_query_error)?,
        source_watermark_at: unix_millis_to_datetime(row.try_get(2).map_err(to_query_error)?)?,
        coverage: serde_json::from_str(&row.try_get::<String, _>(3).map_err(to_query_error)?)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        created_at: unix_to_datetime(row.try_get(4).map_err(to_query_error)?)?,
        observations,
    }))
}

const INSERT_REQUEST_SQL: &str = r"
INSERT INTO agent_session_requests (
    agent_session_id, request_id, request_log_id, usage_event_id, ordinal, execution_id,
    parent_execution_id, normalized_session_id, correlation_confidence, limitation_codes_json,
    occurred_at, completed_at, terminal_success
) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
ON CONFLICT (agent_session_id, request_id) DO NOTHING
";

const FINALIZE_FULL_SESSION_SQL: &str = r"
UPDATE agent_sessions
SET lifecycle = 'finalized', ended_at = input_watermark_at / 1000,
    finalized_reason = 'request_limit', input_generation = input_generation + 1,
    updated_at = GREATEST(updated_at, $2)
WHERE agent_session_id = $1
";

const UPDATE_COUNTERS_SQL: &str = r"
UPDATE agent_sessions
SET request_count = request_count + $2, next_ordinal = next_ordinal + $2,
    nested_fact_count = nested_fact_count + $3, input_generation = input_generation + 1,
    started_at = LEAST(started_at, COALESCE($4, started_at)),
    input_watermark_at = GREATEST(input_watermark_at, COALESCE($5, input_watermark_at)),
    updated_at = GREATEST(updated_at, COALESCE($6, updated_at))
WHERE agent_session_id = $1
";

const INSERT_OBSERVATION_SET_SQL: &str = r"
INSERT INTO agent_inferred_observation_sets (
    observation_set_id, agent_session_id, parser_version, source_watermark_at, coverage_json, created_at
) VALUES ($1, $2, $3, $4, $5, $6)
";

const LOAD_OBSERVATION_SET_SQL: &str = r"
SELECT agent_session_id, parser_version, source_watermark_at, coverage_json, created_at
FROM agent_inferred_observation_sets WHERE observation_set_id = $1
";

const LOAD_OBSERVATIONS_SQL: &str = r"
SELECT o.observation_id, o.kind, o.source_request_id, o.evidence, o.occurred_at,
       o.facts_json, o.limitation_codes_json, s.parser_version
FROM agent_inferred_observations o
JOIN agent_inferred_observation_sets s ON s.observation_set_id = o.observation_set_id
WHERE o.observation_set_id = $1 ORDER BY o.occurred_at, o.observation_id
";
