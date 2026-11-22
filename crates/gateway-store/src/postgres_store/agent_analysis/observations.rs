use super::*;
use std::collections::HashMap;

impl PostgresStore {
    pub(super) async fn load_session_observation_sets(
        &self,
        session_id: Uuid,
    ) -> Result<Vec<AgentObservationSetRecord>, StoreError> {
        let rows = sqlx::query(LOAD_SETS_SQL)
            .bind(session_id.to_string())
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?;
        let mut sets = Vec::with_capacity(rows.len());
        let mut ids = Vec::with_capacity(rows.len());
        let mut positions = HashMap::with_capacity(rows.len());
        for row in rows {
            let id: String = row.try_get(0).map_err(to_query_error)?;
            let observation_set_id = parse_uuid(&id)?;
            positions.insert(observation_set_id, sets.len());
            let coverage: String = row.try_get(3).map_err(to_query_error)?;
            sets.push(AgentObservationSetRecord {
                observation_set_id,
                agent_session_id: session_id,
                parser_version: row.try_get(1).map_err(to_query_error)?,
                source_watermark_at: unix_millis_to_datetime(
                    row.try_get(2).map_err(to_query_error)?,
                )?,
                coverage: serde_json::from_str(&coverage)
                    .map_err(|error| StoreError::Serialization(error.to_string()))?,
                created_at: unix_to_datetime(row.try_get(4).map_err(to_query_error)?)?,
                observations: Vec::new(),
            });
            ids.push(id);
        }
        if ids.is_empty() {
            return Ok(sets);
        }
        let rows = sqlx::query(LOAD_OBSERVATIONS_SQL)
            .bind(&ids)
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?;
        for row in rows {
            let set_id = parse_uuid(&row.try_get::<String, _>(8).map_err(to_query_error)?)?;
            let &position = positions.get(&set_id).ok_or_else(|| {
                StoreError::Unexpected("observation set was not selected".to_string())
            })?;
            sets[position].observations.push(decode_observation(&row)?);
        }
        Ok(sets)
    }
}

const LOAD_SETS_SQL: &str = r"
SELECT observation_set_id, parser_version, source_watermark_at, coverage_json, created_at
FROM agent_inferred_observation_sets WHERE agent_session_id = $1
ORDER BY source_watermark_at, created_at, observation_set_id LIMIT 1001
";

const LOAD_OBSERVATIONS_SQL: &str = r"
SELECT o.observation_id, o.kind, o.source_request_id, o.evidence, o.occurred_at,
       o.facts_json, o.limitation_codes_json, s.parser_version, o.observation_set_id
FROM agent_inferred_observations o
JOIN agent_inferred_observation_sets s ON s.observation_set_id = o.observation_set_id
WHERE o.observation_set_id = ANY($1::text[]) ORDER BY o.occurred_at, o.observation_id
";
