use super::contracts::{
    CounterStore, FixtureParam, observation_fixture, open_fixture, request_fixture,
};
use gateway_core::{AgentSessionInput, AgentSessionRecord, MAX_AGENT_SESSION_REQUESTS};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

const CORRUPT_OBSERVATION_SQL: &str = "
    UPDATE agent_inferred_observations SET facts_json = 'not-json'
    WHERE observation_set_id = $1";
const CORRUPT_SET_SQL: &str = "
    UPDATE agent_inferred_observation_sets SET coverage_json = 'not-json'
    WHERE observation_set_id = $1";
const HISTORICAL_SETS_SQL: &str = "
    WITH RECURSIVE historical(n) AS (
        SELECT 1 UNION ALL SELECT n + 1 FROM historical WHERE n < 1001
    )
    INSERT INTO agent_inferred_observation_sets (
        observation_set_id, agent_session_id, parser_version,
        source_watermark_at, coverage_json, created_at
    )
    SELECT substr($1, 1, 24) || CAST(100000000000 + n AS TEXT),
        $1, $2, 0, '{}', 0 FROM historical";

pub(super) async fn parser_filtered_loading(
    store: &impl CounterStore,
    template: &AgentSessionRecord,
) {
    let session = open_fixture(template);
    assert!(
        store
            .insert_agent_session_if_absent(&session)
            .await
            .unwrap()
    );
    let mut request = request_fixture(&session);
    // Observation timestamps are persisted at second precision.
    request.occurred_at =
        OffsetDateTime::from_unix_timestamp(request.occurred_at.unix_timestamp()).unwrap();
    request.completed_at = Some(request.occurred_at);
    let mut old = observation_fixture(&request, 1);
    old.parser_version = "historical-parser".to_string();
    old.observations[0].parser_version = old.parser_version.clone();
    assert!(
        store
            .append_agent_session_input(AgentSessionInput::observations(&old))
            .await
            .unwrap()
            .observations_inserted
    );

    let first = observation_fixture(&request, 1);
    let mut second = observation_fixture(&request, 1);
    second.source_watermark_at += Duration::milliseconds(1);
    second.created_at += Duration::seconds(1);
    for set in [&first, &second] {
        assert!(
            store
                .append_agent_session_input(AgentSessionInput::observations(set))
                .await
                .unwrap()
                .observations_inserted
        );
    }

    // The existing unfiltered API must still expose historical parser versions.
    let all = store
        .load_agent_observation_sets(session.agent_session_id)
        .await
        .unwrap();
    assert_eq!(all.len(), 3);
    assert!(all.contains(&old));
    assert!(all.contains(&first));
    assert!(all.contains(&second));

    // A bad obsolete child must not be fetched or decoded by the filtered loader.
    store
        .fixture_sql(
            CORRUPT_OBSERVATION_SQL,
            &[FixtureParam::Uuid(old.observation_set_id)],
        )
        .await;
    assert_eq!(
        store
            .load_agent_observation_sets_for_parser(session.agent_session_id, "contract")
            .await
            .unwrap(),
        vec![first.clone(), second.clone()]
    );
    assert!(
        store
            .load_agent_observation_sets(session.agent_session_id)
            .await
            .is_err()
    );

    // Also exclude obsolete parent decoding and apply the filter before LIMIT 1001:
    // more than a full historical window precedes the two matching current sets.
    store
        .fixture_sql(
            CORRUPT_SET_SQL,
            &[FixtureParam::Uuid(old.observation_set_id)],
        )
        .await;
    store
        .fixture_sql(
            HISTORICAL_SETS_SQL,
            &[
                FixtureParam::Uuid(session.agent_session_id),
                FixtureParam::Text("historical-parser"),
            ],
        )
        .await;
    assert_eq!(
        store
            .load_agent_observation_sets_for_parser(session.agent_session_id, "contract")
            .await
            .unwrap(),
        vec![first, second]
    );
    assert!(
        store
            .load_agent_observation_sets_for_parser(session.agent_session_id, "missing-parser")
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .load_agent_observation_sets_for_parser(Uuid::new_v4(), "contract")
            .await
            .unwrap()
            .is_empty()
    );

    // Matching sets retain the overflow sentinel instead of silently truncating at 1,000.
    let overflow = open_fixture(template);
    assert!(
        store
            .insert_agent_session_if_absent(&overflow)
            .await
            .unwrap()
    );
    store
        .fixture_sql(
            HISTORICAL_SETS_SQL,
            &[
                FixtureParam::Uuid(overflow.agent_session_id),
                FixtureParam::Text("contract"),
            ],
        )
        .await;
    assert_eq!(
        store
            .load_agent_observation_sets_for_parser(overflow.agent_session_id, "contract")
            .await
            .unwrap()
            .len(),
        MAX_AGENT_SESSION_REQUESTS as usize + 1
    );
}
