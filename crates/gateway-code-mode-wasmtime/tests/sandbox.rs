//! Milestone 2 sandbox test matrix against the real Wasmtime + QuickJS guest
//! with a stub host dispatcher.

use std::sync::{Arc, Mutex, OnceLock};

use async_trait::async_trait;
use gateway_code_mode_wasmtime::WasmtimeQuickjsExecutor;
use gateway_service::{
    CodeExecutor, CodeModeLimits, ExecutionOutcome, ExecutorError, HostCallError, HostDispatcher,
};
use serde_json::{Value, json};

/// Engine + compiled guest are shared across tests (compilation is the slow
/// part); every execution still gets its own fresh store.
fn executor() -> &'static WasmtimeQuickjsExecutor {
    static EXECUTOR: OnceLock<WasmtimeQuickjsExecutor> = OnceLock::new();
    EXECUTOR.get_or_init(|| {
        WasmtimeQuickjsExecutor::new(&CodeModeLimits::default())
            .expect("guest module must validate")
    })
}

type StubResponder = dyn Fn(&str, &str, &Value) -> Result<Value, HostCallError> + Send + Sync;

struct StubDispatcher {
    calls: Mutex<Vec<(String, String, Value)>>,
    respond: Box<StubResponder>,
}

impl StubDispatcher {
    fn new(
        respond: impl Fn(&str, &str, &Value) -> Result<Value, HostCallError> + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::new(Vec::new()),
            respond: Box::new(respond),
        })
    }

    fn calls(&self) -> Vec<(String, String, Value)> {
        self.calls.lock().expect("calls").clone()
    }
}

#[async_trait]
impl HostDispatcher for StubDispatcher {
    async fn call(&self, namespace: &str, name: &str, args: Value) -> Result<Value, HostCallError> {
        self.calls.lock().expect("calls").push((
            namespace.to_string(),
            name.to_string(),
            args.clone(),
        ));
        (self.respond)(namespace, name, &args)
    }
}

/// Dispatcher whose calls never resolve, simulating a hung upstream.
struct HangingDispatcher;

#[async_trait]
impl HostDispatcher for HangingDispatcher {
    async fn call(
        &self,
        _namespace: &str,
        _name: &str,
        _args: Value,
    ) -> Result<Value, HostCallError> {
        futures_util::future::pending().await
    }
}

fn limits_with_timeout(execution_timeout_ms: u64) -> CodeModeLimits {
    CodeModeLimits {
        execution_timeout_ms,
        ..CodeModeLimits::default()
    }
}

async fn run(
    code: &str,
    dispatcher: Arc<dyn HostDispatcher>,
    limits: &CodeModeLimits,
) -> ExecutionOutcome {
    executor()
        .execute(code, dispatcher, limits)
        .await
        .expect("guest failures must be outcomes, not Err")
}

#[tokio::test]
async fn happy_path_filters_search_results_in_sandbox() {
    let dispatcher = StubDispatcher::new(|_, name, args| match name {
        "searchTools" => Ok(json!({
            "items": [
                {"address": "mcp://github/tools/create_issue", "score": 0.9},
                {"address": "mcp://jira/tools/create_ticket", "score": 0.4}
            ],
            "total": 2
        })),
        "describeTool" => Ok(json!({
            "address": args["address"],
            "tool": {"input_schema": {"required": ["title"]}, "schema_hash": "sha256:x"}
        })),
        _ => Err(HostCallError::CapabilityDenied {
            name: name.to_string(),
        }),
    });
    let code = r#"
        const { items } = await oceans.searchTools({ query: "issues" });
        const top = items.filter((item) => item.score > 0.5);
        const details = [];
        for (const item of top) {
            details.push(await oceans.describeTool({ address: item.address }));
        }
        return details.map((d) => ({ address: d.address, required: d.tool.input_schema.required }));
    "#;
    let outcome = run(code, dispatcher.clone(), &CodeModeLimits::default()).await;
    assert_eq!(outcome.error, None);
    assert_eq!(
        outcome.result,
        Some(json!([
            {"address": "mcp://github/tools/create_issue", "required": ["title"]}
        ]))
    );
    let calls = dispatcher.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, "oceans");
    assert_eq!(calls[0].1, "searchTools");
    assert_eq!(calls[1].1, "describeTool");
}

#[tokio::test]
async fn infinite_loop_is_preempted_by_epoch_deadline() {
    let dispatcher = StubDispatcher::new(|_, _, _| Ok(Value::Null));
    let error = executor()
        .execute("while (true) {}", dispatcher, &limits_with_timeout(300))
        .await
        .expect_err("infinite loop must time out");
    assert!(matches!(error, ExecutorError::Timeout));
}

#[tokio::test]
async fn allocation_bomb_hits_resource_limit_without_harming_host() {
    let dispatcher = StubDispatcher::new(|_, _, _| Ok(Value::Null));
    let limits = CodeModeLimits {
        memory_limit_bytes: 16 * 1024 * 1024,
        execution_timeout_ms: 10_000,
        ..CodeModeLimits::default()
    };
    let code = r#"
        const chunks = [];
        while (true) { chunks.push("x".repeat(1024 * 1024)); }
    "#;
    let outcome = run(code, dispatcher, &limits).await;
    let error = outcome.error.expect("allocation bomb must fail");
    assert!(error.contains("memory limit"), "got: {error}");
    assert!(outcome.result.is_none());

    // Host unaffected: the same executor still runs follow-up code.
    let follow_up = run(
        "return 6 * 7;",
        StubDispatcher::new(|_, _, _| Ok(Value::Null)),
        &CodeModeLimits::default(),
    )
    .await;
    assert_eq!(follow_up.result, Some(json!(42)));
}

#[tokio::test]
async fn sandbox_has_no_ambient_capabilities() {
    let dispatcher = StubDispatcher::new(|_, _, _| Ok(Value::Null));
    let code = r#"
        return {
            fetch: typeof fetch,
            process: typeof process,
            require: typeof require,
            xhr: typeof XMLHttpRequest,
            deno: typeof Deno,
            importFails: await (async () => {
                try { await import("fs"); return "resolved"; } catch (e) { return "throws"; }
            })(),
        };
    "#;
    let outcome = run(code, dispatcher.clone(), &CodeModeLimits::default()).await;
    assert_eq!(outcome.error, None);
    assert_eq!(
        outcome.result,
        Some(json!({
            "fetch": "undefined",
            "process": "undefined",
            "require": "undefined",
            "xhr": "undefined",
            "deno": "undefined",
            "importFails": "throws",
        }))
    );
    assert!(dispatcher.calls().is_empty());
}

#[tokio::test]
async fn hanging_host_call_is_bounded_by_wall_clock_timeout() {
    let error = executor()
        .execute(
            "await oceans.callTool({ address: 'mcp://x/tools/y' }); return 1;",
            Arc::new(HangingDispatcher),
            &limits_with_timeout(300),
        )
        .await
        .expect_err("hung host call must time out");
    assert!(matches!(error, ExecutorError::Timeout));
}

#[tokio::test]
async fn console_is_captured_and_log_caps_apply() {
    let dispatcher = StubDispatcher::new(|_, _, _| Ok(Value::Null));
    let limits = CodeModeLimits {
        max_log_lines: 3,
        max_log_bytes: 64,
        ..CodeModeLimits::default()
    };
    let code = r#"
        console.log("plain", {a: 1});
        console.warn("warned");
        console.error("failed");
        console.log("dropped by line cap");
        return null;
    "#;
    let outcome = run(code, dispatcher, &limits).await;
    assert_eq!(outcome.error, None);
    assert_eq!(
        outcome.logs,
        vec![
            "plain {\"a\":1}".to_string(),
            "[warn] warned".to_string(),
            "[error] failed".to_string(),
        ]
    );
}

#[tokio::test]
async fn oversized_output_is_truncated_with_flag() {
    let dispatcher = StubDispatcher::new(|_, _, _| Ok(Value::Null));
    let limits = CodeModeLimits {
        max_output_bytes: 64,
        ..CodeModeLimits::default()
    };
    let outcome = run("return 'y'.repeat(4096);", dispatcher, &limits).await;
    assert_eq!(outcome.error, None);
    assert!(outcome.truncated);
    let preview = outcome.result.expect("preview");
    assert!(preview.as_str().expect("string").len() <= 64);
}

#[tokio::test]
async fn error_envelopes_are_catchable_guest_exceptions() {
    let dispatcher = StubDispatcher::new(|_, name, _| match name {
        "callTool" => Err(HostCallError::CapabilityDenied {
            name: name.to_string(),
        }),
        _ => Ok(json!({"ok": true})),
    });
    let code = r#"
        try {
            await oceans.callTool({ address: "mcp://github/tools/x" });
            return "not reached";
        } catch (error) {
            return { caught: error.message };
        }
    "#;
    let outcome = run(code, dispatcher, &CodeModeLimits::default()).await;
    assert_eq!(outcome.error, None);
    assert_eq!(
        outcome.result,
        Some(json!({
            "caught": "oceans.callTool is not available in this capability profile"
        }))
    );
}

#[tokio::test]
async fn uncaught_host_call_error_becomes_guest_error_outcome() {
    let dispatcher = StubDispatcher::new(|_, _, _| {
        Err(HostCallError::Tool {
            error_code: "credential_required",
            message: "credential required for server `github`".to_string(),
        })
    });
    let outcome = run(
        "return await oceans.callTool({ address: 'mcp://github/tools/x' });",
        dispatcher,
        &CodeModeLimits::default(),
    )
    .await;
    let error = outcome.error.expect("uncaught exception is guest error");
    assert!(error.contains("credential required"), "got: {error}");
    assert!(outcome.result.is_none());
}

#[tokio::test]
async fn syntax_and_runtime_errors_are_outcomes_not_err() {
    let dispatcher = StubDispatcher::new(|_, _, _| Ok(Value::Null));
    let outcome = run(
        "return notDefined.anywhere;",
        dispatcher.clone(),
        &CodeModeLimits::default(),
    )
    .await;
    let error = outcome.error.expect("reference error");
    assert!(error.contains("notDefined"), "got: {error}");

    let outcome = run(
        "this is not javascript",
        dispatcher,
        &CodeModeLimits::default(),
    )
    .await;
    assert!(outcome.error.is_some());
}

#[tokio::test]
async fn awaiting_a_foreign_promise_fails_with_no_event_loop_error() {
    let dispatcher = StubDispatcher::new(|_, _, _| Ok(Value::Null));
    let outcome = run(
        "return await new Promise(() => {});",
        dispatcher,
        &CodeModeLimits::default(),
    )
    .await;
    let error = outcome.error.expect("pending promise must fail");
    assert!(error.contains("no event loop"), "got: {error}");
}

/// W2 regression: with yield-on-epoch, CPU-bound guest code must not pin a
/// runtime worker. On a single-threaded runtime, a `while (true) {}`
/// execution and an unrelated quick execution run concurrently — the quick
/// one completes long before the spinner's timeout because the spinning
/// fiber yields back to the executor on every epoch tick.
#[tokio::test(flavor = "current_thread")]
async fn busy_loop_yields_to_other_executions_on_a_single_thread() {
    let limits = limits_with_timeout(2_000);
    let spinner = executor().execute(
        "while (true) {}",
        StubDispatcher::new(|_, _, _| Ok(Value::Null)),
        &limits,
    );
    let quick = async {
        // Let the spinner start first.
        tokio::task::yield_now().await;
        let started = std::time::Instant::now();
        let outcome = run(
            "return 6 * 7;",
            StubDispatcher::new(|_, _, _| Ok(Value::Null)),
            &limits,
        )
        .await;
        (outcome, started.elapsed())
    };
    let (spinner_result, (quick_outcome, quick_elapsed)) = tokio::join!(spinner, quick);
    assert!(matches!(spinner_result, Err(ExecutorError::Timeout)));
    assert_eq!(quick_outcome.result, Some(json!(42)));
    assert!(
        quick_elapsed < std::time::Duration::from_millis(1_500),
        "quick execution was starved for {quick_elapsed:?}"
    );
}

/// Executions beyond `max_concurrent_executions` queue on the semaphore and
/// keep burning their wall-clock budget while queued.
#[tokio::test]
async fn concurrency_cap_queues_excess_executions_within_wall_clock() {
    let limits = CodeModeLimits {
        max_concurrent_executions: 1,
        execution_timeout_ms: 1_000,
        ..CodeModeLimits::default()
    };
    let executor = WasmtimeQuickjsExecutor::new(&limits).expect("guest module must validate");
    let spinner = executor.execute(
        "while (true) {}",
        StubDispatcher::new(|_, _, _| Ok(Value::Null)),
        &limits,
    );
    let queued = async {
        tokio::task::yield_now().await;
        executor
            .execute(
                "return 1;",
                StubDispatcher::new(|_, _, _| Ok(Value::Null)),
                &limits,
            )
            .await
    };
    let (spinner_result, queued_result) = tokio::join!(spinner, queued);
    assert!(matches!(spinner_result, Err(ExecutorError::Timeout)));
    // The queued execution spends its whole budget waiting for the permit
    // held by the spinner, so the wall clock bounds it to a timeout. (If the
    // spinner finishes first the queued run may still succeed; both outcomes
    // respect the budget.)
    match queued_result {
        Err(ExecutorError::Timeout) => {}
        Ok(outcome) => assert_eq!(outcome.result, Some(json!(1))),
        Err(other) => panic!("unexpected executor failure: {other}"),
    }
}

#[tokio::test]
async fn parallel_executions_are_isolated() {
    let make_code = |tag: &str| {
        format!(
            r#"
            globalThis.marker = (globalThis.marker ?? "") + "{tag}";
            console.log("running {tag}");
            await oceans.searchTools({{ query: "{tag}" }});
            return globalThis.marker;
            "#
        )
    };
    let dispatcher_a = StubDispatcher::new(|_, _, _| Ok(json!({"items": [], "total": 0})));
    let dispatcher_b = StubDispatcher::new(|_, _, _| Ok(json!({"items": [], "total": 0})));
    let limits = CodeModeLimits::default();
    let code_a = make_code("alpha");
    let code_b = make_code("beta");
    let (a, b) = tokio::join!(
        run(&code_a, dispatcher_a.clone(), &limits),
        run(&code_b, dispatcher_b.clone(), &limits),
    );
    assert_eq!(a.result, Some(json!("alpha")));
    assert_eq!(b.result, Some(json!("beta")));
    assert_eq!(a.logs, vec!["running alpha".to_string()]);
    assert_eq!(b.logs, vec!["running beta".to_string()]);
    assert_eq!(dispatcher_a.calls().len(), 1);
    assert_eq!(dispatcher_a.calls()[0].2, json!({"query": "alpha"}));
    assert_eq!(dispatcher_b.calls()[0].2, json!({"query": "beta"}));
}
