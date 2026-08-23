use async_trait::async_trait;
use gateway_core::{
    GuardrailDecisionEventRecord, GuardrailDecisionPage, GuardrailDecisionQuery,
    GuardrailDecisionRepository, MAX_GUARDRAIL_DECISION_PAGE_SIZE, StoreError,
};

use super::{LibsqlStore, to_query_error};
use crate::shared::{datetime_to_unix_millis, parse_uuid, unix_millis_to_datetime};

fn decode(row: &libsql::Row) -> Result<GuardrailDecisionEventRecord, StoreError> {
    let decision_id: String = row.get(0).map_err(to_query_error)?;
    let invocation_id: Option<String> = row.get(2).map_err(to_query_error)?;
    let transformed: i64 = row.get(13).map_err(to_query_error)?;
    let occurred_at: i64 = row.get(15).map_err(to_query_error)?;
    Ok(GuardrailDecisionEventRecord {
        decision_id: parse_uuid(&decision_id)?,
        request_id: row.get(1).map_err(to_query_error)?,
        mcp_tool_invocation_id: invocation_id.as_deref().map(parse_uuid).transpose()?,
        phase: row.get(3).map_err(to_query_error)?,
        effective_scope: row.get(4).map_err(to_query_error)?,
        evaluator: row.get(5).map_err(to_query_error)?,
        managed_service: row.get(6).map_err(to_query_error)?,
        pack_id: row.get(7).map_err(to_query_error)?,
        rule_id: row.get(8).map_err(to_query_error)?,
        action: row.get(9).map_err(to_query_error)?,
        reason_code: row.get(10).map_err(to_query_error)?,
        latency_micros: row.get(11).map_err(to_query_error)?,
        failure_disposition: row.get(12).map_err(to_query_error)?,
        transformed: transformed == 1,
        content_hash: row.get(14).map_err(to_query_error)?,
        occurred_at: unix_millis_to_datetime(occurred_at)?,
    })
}

#[async_trait]
impl GuardrailDecisionRepository for LibsqlStore {
    async fn insert_guardrail_decision(
        &self,
        decision: &GuardrailDecisionEventRecord,
    ) -> Result<(), StoreError> {
        self.connection
            .execute(
                r#"
                INSERT INTO guardrail_decisions (
                    decision_id, request_id, mcp_tool_invocation_id, phase, effective_scope,
                    evaluator, managed_service, pack_id, rule_id, action, reason_code,
                    latency_micros, failure_disposition, transformed, content_hash, occurred_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                "#,
                libsql::params![
                    decision.decision_id.to_string(),
                    decision.request_id.as_deref(),
                    decision
                        .mcp_tool_invocation_id
                        .map(|value| value.to_string()),
                    decision.phase.as_str(),
                    decision.effective_scope.as_str(),
                    decision.evaluator.as_str(),
                    decision.managed_service.as_deref(),
                    decision.pack_id.as_deref(),
                    decision.rule_id.as_deref(),
                    decision.action.as_str(),
                    decision.reason_code.as_str(),
                    decision.latency_micros,
                    decision.failure_disposition.as_deref(),
                    if decision.transformed { 1_i64 } else { 0_i64 },
                    decision.content_hash.as_str(),
                    datetime_to_unix_millis(decision.occurred_at)?,
                ],
            )
            .await
            .map_err(to_query_error)?;
        Ok(())
    }

    async fn list_guardrail_decisions(
        &self,
        query: &GuardrailDecisionQuery,
    ) -> Result<GuardrailDecisionPage, StoreError> {
        let page = query.page.max(1);
        let page_size = query.page_size.clamp(1, MAX_GUARDRAIL_DECISION_PAGE_SIZE);
        let offset = u64::from(page.saturating_sub(1)) * u64::from(page_size);
        let start = query
            .occurred_at_start
            .map(datetime_to_unix_millis)
            .transpose()?;
        let end = query
            .occurred_at_end
            .map(datetime_to_unix_millis)
            .transpose()?;
        let parameters = libsql::params![
            query.request_id.as_deref(),
            query.phase.as_deref(),
            query.action.as_deref(),
            query.evaluator.as_deref(),
            start,
            end,
        ];
        let mut count_rows = self
            .connection
            .query(
                r#"
                SELECT COUNT(*) FROM guardrail_decisions
                WHERE (?1 IS NULL OR request_id = ?1)
                  AND (?2 IS NULL OR phase = ?2)
                  AND (?3 IS NULL OR action = ?3)
                  AND (?4 IS NULL OR evaluator = ?4)
                  AND (?5 IS NULL OR occurred_at >= ?5)
                  AND (?6 IS NULL OR occurred_at <= ?6)
                "#,
                parameters,
            )
            .await
            .map_err(to_query_error)?;
        let total: i64 = count_rows
            .next()
            .await
            .map_err(to_query_error)?
            .ok_or_else(|| StoreError::Query("guardrail decision count row missing".into()))?
            .get(0)
            .map_err(to_query_error)?;
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT decision_id, request_id, mcp_tool_invocation_id, phase, effective_scope,
                       evaluator, managed_service, pack_id, rule_id, action, reason_code,
                       latency_micros, failure_disposition, transformed, content_hash, occurred_at
                FROM guardrail_decisions
                WHERE (?1 IS NULL OR request_id = ?1)
                  AND (?2 IS NULL OR phase = ?2)
                  AND (?3 IS NULL OR action = ?3)
                  AND (?4 IS NULL OR evaluator = ?4)
                  AND (?5 IS NULL OR occurred_at >= ?5)
                  AND (?6 IS NULL OR occurred_at <= ?6)
                ORDER BY occurred_at DESC, decision_id DESC
                LIMIT ?7 OFFSET ?8
                "#,
                libsql::params![
                    query.request_id.as_deref(),
                    query.phase.as_deref(),
                    query.action.as_deref(),
                    query.evaluator.as_deref(),
                    start,
                    end,
                    i64::from(page_size),
                    offset as i64,
                ],
            )
            .await
            .map_err(to_query_error)?;
        let mut items = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            items.push(decode(&row)?);
        }
        Ok(GuardrailDecisionPage {
            items,
            page,
            page_size,
            total: u64::try_from(total).unwrap_or_default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use tempfile::tempdir;
    use time::OffsetDateTime;
    use uuid::Uuid;

    use super::*;
    use crate::run_migrations;

    #[tokio::test]
    async fn decision_metadata_round_trips_and_filters_without_payloads() {
        let temporary = tempdir().expect("tempdir");
        let database_path = temporary.path().join("gateway.db");
        run_migrations(&database_path).await.expect("migrations");
        let store = LibsqlStore::new_local(database_path.to_str().expect("database path"))
            .await
            .expect("store");
        let decision = GuardrailDecisionEventRecord {
            decision_id: Uuid::new_v4(),
            request_id: Some("request-guardrail-1".to_string()),
            mcp_tool_invocation_id: Some(Uuid::new_v4()),
            phase: "prompt".to_string(),
            effective_scope: "model_route:test/openai/model".to_string(),
            evaluator: "deterministic".to_string(),
            managed_service: None,
            pack_id: Some("core.shell".to_string()),
            rule_id: Some("shell.recursive-delete".to_string()),
            action: "audit".to_string(),
            reason_code: "destructive_operation".to_string(),
            latency_micros: 42,
            failure_disposition: None,
            transformed: false,
            content_hash: "sha256:fixture".to_string(),
            occurred_at: OffsetDateTime::now_utc(),
        };

        store
            .insert_guardrail_decision(&decision)
            .await
            .expect("insert decision");
        let page = store
            .list_guardrail_decisions(&GuardrailDecisionQuery {
                page: 1,
                page_size: 20,
                request_id: Some("request-guardrail-1".to_string()),
                phase: Some("prompt".to_string()),
                action: Some("audit".to_string()),
                evaluator: Some("deterministic".to_string()),
                occurred_at_start: None,
                occurred_at_end: None,
            })
            .await
            .expect("list decisions");

        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].decision_id, decision.decision_id);
        assert_eq!(page.items[0].request_id, decision.request_id);
        assert_eq!(
            page.items[0].mcp_tool_invocation_id,
            decision.mcp_tool_invocation_id
        );
        assert_eq!(page.items[0].content_hash, decision.content_hash);
        assert_eq!(page.items[0].pack_id, decision.pack_id);
        assert_eq!(
            page.items[0].occurred_at.unix_timestamp_nanos() / 1_000_000,
            decision.occurred_at.unix_timestamp_nanos() / 1_000_000,
        );
        let after_decision = store
            .list_guardrail_decisions(&GuardrailDecisionQuery {
                occurred_at_start: Some(decision.occurred_at + time::Duration::milliseconds(1)),
                ..Default::default()
            })
            .await
            .expect("filter decisions after millisecond boundary");
        assert_eq!(after_decision.total, 0);
        gateway_core::RequestLogRepository::purge_request_logs_older_than(
            &store,
            decision.occurred_at + time::Duration::milliseconds(1),
            false,
        )
        .await
        .expect("purge guardrail decision through request retention");
        assert_eq!(
            store
                .list_guardrail_decisions(&GuardrailDecisionQuery::default())
                .await
                .expect("list after retention purge")
                .total,
            0
        );
    }

    #[tokio::test]
    async fn decision_event_persistence_load_gate_stays_within_release_budget() {
        const EVENT_COUNT: usize = 100;
        let temporary = tempdir().expect("tempdir");
        let database_path = temporary.path().join("gateway.db");
        run_migrations(&database_path).await.expect("migrations");
        let store = LibsqlStore::new_local(database_path.to_str().expect("database path"))
            .await
            .expect("store");
        let started = Instant::now();
        for index in 0..EVENT_COUNT {
            store
                .insert_guardrail_decision(&GuardrailDecisionEventRecord {
                    decision_id: Uuid::new_v4(),
                    request_id: Some(format!("load-{index}")),
                    mcp_tool_invocation_id: None,
                    phase: "prompt".into(),
                    effective_scope: "global".into(),
                    evaluator: "deterministic".into(),
                    managed_service: None,
                    pack_id: Some("core.shell".into()),
                    rule_id: Some("shell.recursive-delete".into()),
                    action: "audit".into(),
                    reason_code: "destructive_operation".into(),
                    latency_micros: 10,
                    failure_disposition: None,
                    transformed: false,
                    content_hash: format!("sha256:{index:064x}"),
                    occurred_at: OffsetDateTime::now_utc(),
                })
                .await
                .expect("insert load event");
        }
        let page = store
            .list_guardrail_decisions(&GuardrailDecisionQuery {
                page: 1,
                page_size: EVENT_COUNT as u32,
                ..Default::default()
            })
            .await
            .expect("list load events");
        assert_eq!(page.total, EVENT_COUNT as u64);
        assert_eq!(page.items.len(), EVENT_COUNT);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{EVENT_COUNT} persisted decisions took {:?}",
            started.elapsed()
        );
    }
}
