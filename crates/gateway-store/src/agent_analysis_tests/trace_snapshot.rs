use super::contracts::{CounterStore, observation_fixture, open_fixture, request_fixture};
use super::generation::{analysis_fixture, job_fixture};
use gateway_core::{AgentSessionAnalysisRepository, AgentSessionInput, AgentSessionRecord};
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;
use uuid::Uuid;

struct Pause {
    captured: Notify,
    resume: Notify,
}

static NEXT_READ: Mutex<Option<(Uuid, Arc<Pause>)>> = Mutex::new(None);

pub(crate) async fn after_session_read(session_id: Uuid) {
    let pause = {
        let mut next = NEXT_READ.lock().unwrap();
        if next.as_ref().is_some_and(|(id, _)| *id == session_id) {
            next.take().map(|(_, pause)| pause)
        } else {
            None
        }
    };
    if let Some(pause) = pause {
        pause.captured.notify_one();
        pause.resume.notified().await;
    }
}

pub(super) async fn generation_snapshot(
    store: &(impl CounterStore + AgentSessionAnalysisRepository),
    template: &AgentSessionRecord,
) {
    let session = open_fixture(template);
    store
        .insert_agent_session_if_absent(&session)
        .await
        .unwrap();
    let first = request_fixture(&session);
    let observations = observation_fixture(&first, 0);
    store
        .append_agent_session_input(AgentSessionInput {
            request: Some(&first),
            observations: AgentSessionInput::observations(&observations).observations,
            analysis: None,
        })
        .await
        .unwrap();
    let pause = Arc::new(Pause {
        captured: Notify::new(),
        resume: Notify::new(),
    });
    assert!(
        NEXT_READ
            .lock()
            .unwrap()
            .replace((session.agent_session_id, pause.clone()))
            .is_none()
    );
    let read = store.load_agent_session_trace(session.agent_session_id);
    let advance = async {
        pause.captured.notified().await;
        let second = request_fixture(&session);
        store
            .append_agent_session_input(AgentSessionInput::request(&second))
            .await
            .unwrap();
        let trace = store
            .load_agent_session_trace(session.agent_session_id)
            .await
            .unwrap()
            .unwrap();
        let report = analysis_fixture(
            &trace,
            &job_fixture(&session).desired_versions,
            session.input_watermark_at,
        );
        assert!(
            store
                .append_agent_session_analysis(&report, None)
                .await
                .unwrap()
        );
        pause.resume.notify_one();
        trace.input_generation
    };
    let (trace, new_generation) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(read, advance)
    })
    .await
    .expect("a trace read may race with input and report publication");
    let trace = trace.unwrap().unwrap();
    assert_eq!(trace.input_generation + 1, new_generation);
    assert!(
        trace.latest_analysis.is_none(),
        "do not attach a newer report to an older captured generation"
    );
    let fresh = store
        .load_agent_session_trace(session.agent_session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fresh.input_generation, new_generation);
    assert_eq!(
        fresh.latest_analysis.unwrap().input_generation,
        new_generation
    );
}
