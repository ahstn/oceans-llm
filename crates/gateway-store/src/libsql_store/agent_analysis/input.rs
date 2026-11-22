use super::*;

impl LibsqlStore {
    pub(super) async fn append_session_input(
        &self,
        input: AgentSessionInput<'_>,
    ) -> Result<AgentSessionInputAppendResult, StoreError> {
        let mut prepared = PreparedInput::new(input)?;
        let mut writes = prepared.observations.as_mut().map(|sets| {
            (
                SetWrites::new(&mut sets.full, &prepared.session_id),
                sets.truncated
                    .as_mut()
                    .map(|set| SetWrites::new(set, &prepared.session_id)),
            )
        });
        let transaction = self.begin_transaction().await?;
        let state = load_state(&transaction, &prepared.session_id).await?;
        let mut result = AgentSessionInputAppendResult::default();
        if let Some(request) = &prepared.request {
            match append_request(&transaction, &prepared.session_id, request, &state).await {
                Ok(inserted) => result.request_inserted = inserted,
                Err(error @ StoreError::AgentSessionWindowClosed(_)) => {
                    transaction.commit().await.map_err(to_query_error)?;
                    return Err(error);
                }
                Err(error) => return Err(error),
            }
        }
        let mut nested_delta = 0;
        if let Some(observations) = &prepared.observations {
            if let Some(existing) = load_set(&transaction, &observations.full.id).await? {
                result.nested_facts_truncated = observations.replay_kind(&existing)?;
            } else {
                let (selected, truncated) = observations.select(state.3)?;
                let (full, fallback) = writes.take().ok_or_else(|| {
                    StoreError::Unexpected("observation writes were not prepared".to_string())
                })?;
                let selected_writes = if truncated {
                    fallback.unwrap_or(full)
                } else {
                    full
                };
                insert_set(&transaction, selected_writes).await?;
                result.observations_inserted = true;
                result.nested_facts_truncated = truncated;
                nested_delta = selected.nested_facts;
            }
        }
        if result.request_inserted || result.observations_inserted {
            update_counters(
                &transaction,
                &prepared,
                result.request_inserted,
                nested_delta,
            )
            .await?;
        }
        transaction.commit().await.map_err(to_query_error)?;
        Ok(result)
    }
}

async fn load_state(
    transaction: &libsql::Transaction,
    session_id: &str,
) -> Result<(String, i64, i64, i64), StoreError> {
    let mut rows = transaction.query(
        "SELECT lifecycle, request_count, next_ordinal, nested_fact_count FROM agent_sessions WHERE agent_session_id = ?1", [session_id],
    ).await.map_err(to_query_error)?;
    let row = rows
        .next()
        .await
        .map_err(to_query_error)?
        .ok_or_else(|| StoreError::NotFound("agent session not found".to_string()))?;
    Ok((
        row.get(0).map_err(to_query_error)?,
        row.get(1).map_err(to_query_error)?,
        row.get(2).map_err(to_query_error)?,
        row.get(3).map_err(to_query_error)?,
    ))
}

async fn append_request(
    transaction: &libsql::Transaction,
    session_id: &str,
    request: &PreparedRequest<'_>,
    state: &(String, i64, i64, i64),
) -> Result<bool, StoreError> {
    let link = request.record;
    let can_append = state.0 == "open"
        && state.1 < i64::try_from(gateway_core::MAX_AGENT_SESSION_REQUESTS).unwrap_or(i64::MAX);
    if can_append {
        let inserted = transaction.execute("INSERT INTO agent_session_requests (agent_session_id, request_id, request_log_id, usage_event_id, ordinal, execution_id, parent_execution_id, normalized_session_id, correlation_confidence, limitation_codes_json, occurred_at, completed_at, terminal_success) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13) ON CONFLICT (agent_session_id, request_id) DO NOTHING",
            libsql::params![session_id, link.request_id.as_str(), request.log_id.as_deref(), request.usage_id.as_deref(), state.2,
                link.execution_id.as_deref(), link.parent_execution_id.as_deref(), link.normalized_session_id.as_deref(),
                request.confidence.as_str(), request.limitations.as_str(), request.occurred_at, request.completed_at, link.terminal_success.map(i64::from)],
        ).await.map_err(to_query_error)?;
        if inserted > 0 {
            return Ok(true);
        }
    }
    let sql = format!(
        "SELECT {REQUEST_COLUMNS} FROM agent_session_requests WHERE agent_session_id = ?1 AND request_id = ?2"
    );
    let mut rows = transaction
        .query(&sql, libsql::params![session_id, link.request_id.as_str()])
        .await
        .map_err(to_query_error)?;
    let existing = rows
        .next()
        .await
        .map_err(to_query_error)?
        .as_ref()
        .map(decode_request_link)
        .transpose()?;
    drop(rows);
    if let Some(existing) = existing {
        if !agent_session_request_matches(&existing, link) {
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
    if state.0 == "open" {
        transaction.execute("UPDATE agent_sessions SET lifecycle = 'finalized', ended_at = input_watermark_at / 1000, finalized_reason = 'request_limit', input_generation = input_generation + 1, updated_at = MAX(updated_at, ?2) WHERE agent_session_id = ?1",
            libsql::params![session_id, request.activity_at / 1000]).await.map_err(to_query_error)?;
    }
    Err(StoreError::AgentSessionWindowClosed(session_id.to_string()))
}

async fn update_counters(
    transaction: &libsql::Transaction,
    input: &PreparedInput<'_>,
    request_inserted: bool,
    nested_delta: i64,
) -> Result<(), StoreError> {
    let request = input.request.as_ref().filter(|_| request_inserted);
    transaction.execute("UPDATE agent_sessions SET request_count = request_count + ?2, next_ordinal = next_ordinal + ?2, nested_fact_count = nested_fact_count + ?3, input_generation = input_generation + 1, started_at = MIN(started_at, COALESCE(?4, started_at)), input_watermark_at = MAX(input_watermark_at, COALESCE(?5, input_watermark_at)), updated_at = MAX(updated_at, COALESCE(?6, updated_at)) WHERE agent_session_id = ?1",
        libsql::params![input.session_id.as_str(), i64::from(request_inserted), nested_delta,
            request.map(|request| request.record.occurred_at.unix_timestamp()), request.map(|request| request.activity_at),
            request.map(|request| request.activity_at / 1000)],
    ).await.map_err(to_query_error)?;
    Ok(())
}

struct SetWrites {
    metadata: Vec<libsql::Value>,
    batches: Vec<(String, Vec<libsql::Value>)>,
}

impl SetWrites {
    fn new(set: &mut PreparedSet<'_>, session_id: &str) -> Self {
        use libsql::Value::{Integer, Text};
        let metadata = vec![
            Text(set.id.clone()),
            Text(session_id.to_owned()),
            Text(set.record.parser_version.clone()),
            Integer(set.watermark),
            Text(std::mem::take(&mut set.coverage)),
            Integer(set.record.created_at.unix_timestamp()),
        ];
        let mut rows = std::mem::take(&mut set.rows).into_iter();
        let mut batches = Vec::with_capacity(rows.len().div_ceil(100));
        while rows.len() > 0 {
            let count = rows.len().min(100);
            let placeholders = (0..count)
                .map(|index| {
                    let start = 3 + index * 7;
                    format!(
                        "(?{start}, ?2, ?1, ?{}, ?{}, ?{}, ?{}, ?{}, ?{})",
                        start + 1,
                        start + 2,
                        start + 3,
                        start + 4,
                        start + 5,
                        start + 6
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!(
                "INSERT INTO agent_inferred_observations (observation_id, observation_set_id, agent_session_id, kind, source_request_id, evidence, occurred_at, facts_json, limitation_codes_json) VALUES {placeholders}"
            );
            let mut values = Vec::with_capacity(2 + count * 7);
            values.extend([Text(session_id.to_owned()), Text(set.id.clone())]);
            for row in rows.by_ref().take(count) {
                values.extend([
                    Text(row.id),
                    Text(row.kind),
                    Text(row.request_id.to_owned()),
                    Text(row.evidence),
                    Integer(row.occurred_at),
                    Text(row.facts),
                    Text(row.limitations),
                ]);
            }
            batches.push((sql, values));
        }
        Self { metadata, batches }
    }
}

async fn insert_set(
    transaction: &libsql::Transaction,
    writes: SetWrites,
) -> Result<(), StoreError> {
    transaction.execute(
        "INSERT INTO agent_inferred_observation_sets (observation_set_id, agent_session_id, parser_version, source_watermark_at, coverage_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        libsql::params_from_iter(writes.metadata),
    ).await.map_err(to_query_error)?;
    for (sql, values) in writes.batches {
        transaction
            .execute(&sql, libsql::params_from_iter(values))
            .await
            .map_err(to_query_error)?;
    }
    Ok(())
}

async fn load_set(
    transaction: &libsql::Transaction,
    id: &str,
) -> Result<Option<AgentObservationSetRecord>, StoreError> {
    let mut rows = transaction.query("SELECT agent_session_id, parser_version, source_watermark_at, coverage_json, created_at FROM agent_inferred_observation_sets WHERE observation_set_id = ?1", [id])
        .await.map_err(to_query_error)?;
    let Some(row) = rows.next().await.map_err(to_query_error)? else {
        return Ok(None);
    };
    drop(rows);
    let mut observation_rows = transaction.query("SELECT o.observation_id, o.kind, o.source_request_id, o.evidence, o.occurred_at, o.facts_json, o.limitation_codes_json, s.parser_version FROM agent_inferred_observations o JOIN agent_inferred_observation_sets s ON s.observation_set_id = o.observation_set_id WHERE o.observation_set_id = ?1 ORDER BY o.occurred_at, o.observation_id", [id]).await.map_err(to_query_error)?;
    let mut observations = Vec::new();
    while let Some(observation) = observation_rows.next().await.map_err(to_query_error)? {
        observations.push(decode_observation(&observation)?);
    }
    Ok(Some(AgentObservationSetRecord {
        observation_set_id: parse_uuid(id)?,
        agent_session_id: parse_uuid(&row.get::<String>(0).map_err(to_query_error)?)?,
        parser_version: row.get(1).map_err(to_query_error)?,
        source_watermark_at: unix_millis_to_datetime(row.get(2).map_err(to_query_error)?)?,
        coverage: serde_json::from_str(&row.get::<String>(3).map_err(to_query_error)?)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        created_at: unix_to_datetime(row.get(4).map_err(to_query_error)?)?,
        observations,
    }))
}
