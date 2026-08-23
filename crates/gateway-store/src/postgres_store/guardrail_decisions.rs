use async_trait::async_trait;
use gateway_core::{
    GuardrailDecisionEventRecord, GuardrailDecisionPage, GuardrailDecisionQuery,
    GuardrailDecisionRepository, MAX_GUARDRAIL_DECISION_PAGE_SIZE, StoreError,
};
use sqlx::{Row, postgres::PgRow};

use super::{PostgresStore, to_query_error};
use crate::shared::{parse_uuid, unix_to_datetime};

fn decode(row: &PgRow) -> Result<GuardrailDecisionEventRecord, StoreError> {
    let decision_id: String = row.try_get(0).map_err(to_query_error)?;
    let invocation_id: Option<String> = row.try_get(2).map_err(to_query_error)?;
    let transformed: i64 = row.try_get(13).map_err(to_query_error)?;
    let occurred_at: i64 = row.try_get(15).map_err(to_query_error)?;
    Ok(GuardrailDecisionEventRecord {
        decision_id: parse_uuid(&decision_id)?,
        request_id: row.try_get(1).map_err(to_query_error)?,
        mcp_tool_invocation_id: invocation_id.as_deref().map(parse_uuid).transpose()?,
        phase: row.try_get(3).map_err(to_query_error)?,
        effective_scope: row.try_get(4).map_err(to_query_error)?,
        evaluator: row.try_get(5).map_err(to_query_error)?,
        managed_service: row.try_get(6).map_err(to_query_error)?,
        pack_id: row.try_get(7).map_err(to_query_error)?,
        rule_id: row.try_get(8).map_err(to_query_error)?,
        action: row.try_get(9).map_err(to_query_error)?,
        reason_code: row.try_get(10).map_err(to_query_error)?,
        latency_micros: row.try_get(11).map_err(to_query_error)?,
        failure_disposition: row.try_get(12).map_err(to_query_error)?,
        transformed: transformed == 1,
        content_hash: row.try_get(14).map_err(to_query_error)?,
        occurred_at: unix_to_datetime(occurred_at)?,
    })
}

#[async_trait]
impl GuardrailDecisionRepository for PostgresStore {
    async fn insert_guardrail_decision(
        &self,
        decision: &GuardrailDecisionEventRecord,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO guardrail_decisions (
                decision_id, request_id, mcp_tool_invocation_id, phase, effective_scope,
                evaluator, managed_service, pack_id, rule_id, action, reason_code,
                latency_micros, failure_disposition, transformed, content_hash, occurred_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
            "#,
        )
        .bind(decision.decision_id.to_string())
        .bind(decision.request_id.as_deref())
        .bind(
            decision
                .mcp_tool_invocation_id
                .map(|value| value.to_string()),
        )
        .bind(decision.phase.as_str())
        .bind(decision.effective_scope.as_str())
        .bind(decision.evaluator.as_str())
        .bind(decision.managed_service.as_deref())
        .bind(decision.pack_id.as_deref())
        .bind(decision.rule_id.as_deref())
        .bind(decision.action.as_str())
        .bind(decision.reason_code.as_str())
        .bind(decision.latency_micros)
        .bind(decision.failure_disposition.as_deref())
        .bind(if decision.transformed { 1_i64 } else { 0_i64 })
        .bind(decision.content_hash.as_str())
        .bind(decision.occurred_at.unix_timestamp())
        .execute(&self.pool)
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
        let start = query.occurred_at_start.map(|value| value.unix_timestamp());
        let end = query.occurred_at_end.map(|value| value.unix_timestamp());
        let total_row = sqlx::query(
            r#"
            SELECT COUNT(*) FROM guardrail_decisions
            WHERE ($1::text IS NULL OR request_id = $1)
              AND ($2::text IS NULL OR phase = $2)
              AND ($3::text IS NULL OR action = $3)
              AND ($4::text IS NULL OR evaluator = $4)
              AND ($5::bigint IS NULL OR occurred_at >= $5)
              AND ($6::bigint IS NULL OR occurred_at <= $6)
            "#,
        )
        .bind(query.request_id.as_deref())
        .bind(query.phase.as_deref())
        .bind(query.action.as_deref())
        .bind(query.evaluator.as_deref())
        .bind(start)
        .bind(end)
        .fetch_one(&self.pool)
        .await
        .map_err(to_query_error)?;
        let total: i64 = total_row.try_get(0).map_err(to_query_error)?;
        let items = sqlx::query(
            r#"
            SELECT decision_id, request_id, mcp_tool_invocation_id, phase, effective_scope,
                   evaluator, managed_service, pack_id, rule_id, action, reason_code,
                   latency_micros, failure_disposition, transformed, content_hash, occurred_at
            FROM guardrail_decisions
            WHERE ($1::text IS NULL OR request_id = $1)
              AND ($2::text IS NULL OR phase = $2)
              AND ($3::text IS NULL OR action = $3)
              AND ($4::text IS NULL OR evaluator = $4)
              AND ($5::bigint IS NULL OR occurred_at >= $5)
              AND ($6::bigint IS NULL OR occurred_at <= $6)
            ORDER BY occurred_at DESC, decision_id DESC
            LIMIT $7 OFFSET $8
            "#,
        )
        .bind(query.request_id.as_deref())
        .bind(query.phase.as_deref())
        .bind(query.action.as_deref())
        .bind(query.evaluator.as_deref())
        .bind(start)
        .bind(end)
        .bind(i64::from(page_size))
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?
        .iter()
        .map(decode)
        .collect::<Result<Vec<_>, _>>()?;
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
    use serial_test::serial;
    use time::OffsetDateTime;
    use uuid::Uuid;

    use super::*;
    use crate::{
        StoreConnectionOptions, run_migrations_with_options,
        tests::{create_postgres_test_database, drop_postgres_test_database},
    };

    #[tokio::test]
    #[serial]
    async fn postgres_guardrail_decisions_round_trip_filter_and_paginate() {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!("skipping PostgreSQL guardrail test because TEST_POSTGRES_URL is not set");
            return;
        };
        let options = StoreConnectionOptions::Postgres {
            url: test_db.database_url.clone(),
            max_connections: 2,
        };
        run_migrations_with_options(&options)
            .await
            .expect("postgres migrations");
        let store = PostgresStore::connect(&test_db.database_url, 2)
            .await
            .expect("postgres store");
        let occurred_at = OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
        let records = [
            GuardrailDecisionEventRecord {
                decision_id: Uuid::new_v4(),
                request_id: Some("request-a".into()),
                mcp_tool_invocation_id: None,
                phase: "prompt".into(),
                effective_scope: "global".into(),
                evaluator: "deterministic".into(),
                managed_service: None,
                pack_id: Some("core.shell".into()),
                rule_id: Some("shell.recursive-delete".into()),
                action: "audit".into(),
                reason_code: "destructive_operation".into(),
                latency_micros: 31,
                failure_disposition: None,
                transformed: false,
                content_hash: "sha256:first".into(),
                occurred_at,
            },
            GuardrailDecisionEventRecord {
                decision_id: Uuid::new_v4(),
                request_id: Some("request-b".into()),
                mcp_tool_invocation_id: Some(Uuid::new_v4()),
                phase: "mcp_result".into(),
                effective_scope: "mcp_server:notion".into(),
                evaluator: "model-armor".into(),
                managed_service: Some("google_model_armor".into()),
                pack_id: None,
                rule_id: None,
                action: "deny".into(),
                reason_code: "managed_intervention".into(),
                latency_micros: 71,
                failure_disposition: Some("fail_closed".into()),
                transformed: true,
                content_hash: "sha256:second".into(),
                occurred_at: occurred_at + time::Duration::seconds(1),
            },
        ];
        for record in &records {
            store
                .insert_guardrail_decision(record)
                .await
                .expect("insert guardrail decision");
        }

        let first_page = store
            .list_guardrail_decisions(&GuardrailDecisionQuery {
                page: 1,
                page_size: 1,
                ..Default::default()
            })
            .await
            .expect("first page");
        assert_eq!(first_page.total, 2);
        assert_eq!(
            serde_json::to_value(&first_page.items).unwrap(),
            serde_json::to_value([&records[1]]).unwrap()
        );

        let filtered = store
            .list_guardrail_decisions(&GuardrailDecisionQuery {
                page: 1,
                page_size: 25,
                request_id: Some("request-a".into()),
                phase: Some("prompt".into()),
                action: Some("audit".into()),
                evaluator: Some("deterministic".into()),
                occurred_at_start: Some(occurred_at),
                occurred_at_end: Some(occurred_at),
            })
            .await
            .expect("filtered page");
        assert_eq!(filtered.total, 1);
        assert_eq!(
            serde_json::to_value(&filtered.items).unwrap(),
            serde_json::to_value([&records[0]]).unwrap()
        );

        drop(store);
        drop_postgres_test_database(&test_db).await;
    }
}
