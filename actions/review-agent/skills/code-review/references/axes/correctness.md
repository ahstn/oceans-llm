# Correctness & Robustness Axis

Focus on behavior.

Look for:

- mismatch with the stated task or expected behavior
- broken invariants, state transitions, or lifecycle assumptions
- missing edge-case handling
- missing error-path handling
- races, ordering bugs, stale state, and off-by-one mistakes
- tests that miss the changed behavior or only test the happy path

Do not spend time on style unless it obscures a correctness issue.

## Evidence-led investigation

Establish the reachable trigger, expected contract, changed behavior, and observable impact. Trace callers, guards, schemas, serializers, migrations, cleanup, and downstream consumers. Compare old and new behavior. Try boundary values, absent values, retries, concurrent calls, cancellation, and partial failure. Reject cases excluded by a verified invariant.

Tests must detect the failure they claim to cover. Flag changed tests that accept broken behavior, replace meaningful assertions with truthiness, or duplicate implementation logic so the same bug passes both. Identify the missed behavior; do not ask for coverage based only on a percentage or test count.

For JavaScript and TypeScript, check falsey defaults, unjoined promises, stale closures, shared mutation, runtime schema drift, module exports, and cleanup. A cast, optional chain, or hook warning alone is not a defect. For Python, check mutable defaults, coroutine handling, iterator reuse, transaction/task boundaries, retry side effects, and timezone or precision changes. Framework validation or locally owned mutable state may make these patterns safe.

For GitHub workflows, trace the effective event, checked-out SHA, job conditions, matrices, outputs, artifacts, and scripts loaded by local actions. Look for skipped required work, false success, wrong deployment targets, and producer/consumer mismatches. Intentional optional diagnostics or a failure checked downstream can be safe. Workflow privilege issues belong to the security axis.

For persistence and orchestration, establish what happens after each partial failure and retry. Prefer atomic state transitions where the contract requires them; distinguish database atomicity from external side effects that need idempotency or compensation.
