CREATE TABLE guardrail_decisions (
    decision_id TEXT PRIMARY KEY NOT NULL,
    request_id TEXT,
    mcp_tool_invocation_id TEXT,
    phase TEXT NOT NULL,
    effective_scope TEXT NOT NULL,
    evaluator TEXT NOT NULL,
    managed_service TEXT,
    pack_id TEXT,
    rule_id TEXT,
    action TEXT NOT NULL,
    reason_code TEXT NOT NULL,
    latency_micros INTEGER NOT NULL,
    failure_disposition TEXT,
    transformed INTEGER NOT NULL DEFAULT 0 CHECK (transformed IN (0, 1)),
    content_hash TEXT NOT NULL,
    occurred_at INTEGER NOT NULL
);

CREATE INDEX idx_guardrail_decisions_occurred_at
    ON guardrail_decisions (occurred_at DESC, decision_id DESC);
CREATE INDEX idx_guardrail_decisions_request_id
    ON guardrail_decisions (request_id, occurred_at DESC);
CREATE INDEX idx_guardrail_decisions_phase_action
    ON guardrail_decisions (phase, action, occurred_at DESC);
