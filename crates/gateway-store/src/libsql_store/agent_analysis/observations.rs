use super::*;
use std::collections::HashMap;

impl LibsqlStore {
    pub(super) async fn load_session_observation_sets(
        &self,
        session_id: Uuid,
    ) -> Result<Vec<AgentObservationSetRecord>, StoreError> {
        self.load_session_observation_sets_matching(session_id, None)
            .await
    }

    pub(super) async fn load_session_observation_sets_for_parser(
        &self,
        session_id: Uuid,
        parser_version: &str,
    ) -> Result<Vec<AgentObservationSetRecord>, StoreError> {
        self.load_session_observation_sets_matching(session_id, Some(parser_version))
            .await
    }

    async fn load_session_observation_sets_matching(
        &self,
        session_id: Uuid,
        parser_version: Option<&str>,
    ) -> Result<Vec<AgentObservationSetRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                LOAD_SETS_SQL,
                libsql::params![session_id.to_string(), parser_version],
            )
            .await
            .map_err(to_query_error)?;
        let mut sets = Vec::new();
        let mut ids = Vec::new();
        let mut positions = HashMap::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            let id: String = row.get(0).map_err(to_query_error)?;
            let observation_set_id = parse_uuid(&id)?;
            positions.insert(observation_set_id, sets.len());
            let coverage: String = row.get(3).map_err(to_query_error)?;
            sets.push(AgentObservationSetRecord {
                observation_set_id,
                agent_session_id: session_id,
                parser_version: row.get(1).map_err(to_query_error)?,
                source_watermark_at: unix_millis_to_datetime(row.get(2).map_err(to_query_error)?)?,
                coverage: serde_json::from_str(&coverage)
                    .map_err(|error| StoreError::Serialization(error.to_string()))?,
                created_at: unix_to_datetime(row.get(4).map_err(to_query_error)?)?,
                observations: Vec::new(),
            });
            ids.push(id);
        }
        drop(rows);
        let mut ids = ids.into_iter();
        while ids.len() > 0 {
            let count = ids.len().min(500);
            let sql = selected_ids_query(
                "SELECT o.observation_id, o.kind, o.source_request_id, o.evidence, o.occurred_at, o.facts_json, o.limitation_codes_json, s.parser_version, o.observation_set_id FROM agent_inferred_observations o JOIN agent_inferred_observation_sets s ON s.observation_set_id = o.observation_set_id WHERE o.observation_set_id IN (",
                count,
                ") ORDER BY o.occurred_at, o.observation_id",
            );
            let mut rows = self
                .connection
                .query(&sql, libsql::params_from_iter(ids.by_ref().take(count)))
                .await
                .map_err(to_query_error)?;
            while let Some(row) = rows.next().await.map_err(to_query_error)? {
                let set_id = parse_uuid(&row.get::<String>(8).map_err(to_query_error)?)?;
                let &position = positions.get(&set_id).ok_or_else(|| {
                    StoreError::Unexpected("observation set was not selected".to_string())
                })?;
                sets[position].observations.push(decode_observation(&row)?);
            }
        }
        Ok(sets)
    }
}

const LOAD_SETS_SQL: &str = r"
SELECT observation_set_id, parser_version, source_watermark_at, coverage_json, created_at
FROM agent_inferred_observation_sets
WHERE agent_session_id = ?1 AND (?2 IS NULL OR parser_version = ?2)
ORDER BY source_watermark_at, created_at, observation_set_id LIMIT 1001
";
