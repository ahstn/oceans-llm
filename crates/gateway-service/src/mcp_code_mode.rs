//! Code Mode MCP execution: backend-neutral executor abstraction plus the
//! host dispatcher that mediates every `oceans.*` call through the existing
//! grant, credential, and invocation-logging machinery.

use std::{
    fmt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::{Duration as StdDuration, Instant},
};

use async_trait::async_trait;
use gateway_core::{
    AuthenticatedApiKey, GatewayError, IdentityRepository, McpAccessRepository,
    McpCatalogToolRecord, McpRegistryRepository, McpToolInvocationRepository,
    McpToolInvocationStatus, McpToolPolicyResult, McpUpstreamCredentialRepository,
};
use gateway_mcp::{StreamableHttpClient, server::call_tool_error_result};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    mcp_catalog::{CallMcpToolInput, DescribeMcpToolInput, McpCatalog, SearchMcpToolsInput},
    mcp_gateway::{McpGatewayService, invocation_status_for_error, map_mcp_client_error},
    mcp_invocation_logging::{
        McpInvocationLogInput, McpInvocationLogging, McpInvocationPayloadPolicy,
    },
};

pub const CODE_MODE_SERVER_DISPLAY_KEY: &str = "code-mode";
pub const CODE_MODE_SERVER_DISPLAY_NAME: &str = "Code Mode";
pub const OCEANS_HOST_NAMESPACE: &str = "oceans";
pub const HOST_FN_SEARCH_TOOLS: &str = "searchTools";
pub const HOST_FN_DESCRIBE_TOOL: &str = "describeTool";
pub const HOST_FN_CALL_TOOL: &str = "callTool";
/// Upper bound on the serialized size of a single host-call argument payload.
pub const MAX_HOST_CALL_ARGUMENT_BYTES: usize = 256 * 1024;
/// Substring present in every capability-denial message. Used to attribute
/// an *uncaught* execution failure to a capability denial; must stay in sync
/// with the [`HostCallError::CapabilityDenied`] `Display` impl.
const CAPABILITY_DENIED_MARKER: &str = "is not available in this capability profile";

/// Capability profile granted to a Code Mode execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityProfile {
    /// `oceans.searchTools` + `oceans.describeTool` only.
    Explore,
    /// Explore capabilities plus `oceans.callTool`.
    Execute,
}

impl CapabilityProfile {
    #[must_use]
    pub fn from_tool_name(name: &str) -> Option<Self> {
        match name {
            "explore" => Some(Self::Explore),
            "execute" => Some(Self::Execute),
            _ => None,
        }
    }

    #[must_use]
    pub const fn tool_display_key(self) -> &'static str {
        match self {
            Self::Explore => "explore",
            Self::Execute => "execute",
        }
    }

    #[must_use]
    pub const fn tool_display_name(self) -> &'static str {
        match self {
            Self::Explore => "Explore",
            Self::Execute => "Execute",
        }
    }

    #[must_use]
    pub fn allows_host_function(self, name: &str) -> bool {
        match name {
            HOST_FN_SEARCH_TOOLS | HOST_FN_DESCRIBE_TOOL => true,
            HOST_FN_CALL_TOOL => self == Self::Execute,
            _ => false,
        }
    }
}

/// Resource limits applied to a single Code Mode execution.
#[derive(Debug, Clone)]
pub struct CodeModeLimits {
    pub execution_timeout_ms: u64,
    pub memory_limit_bytes: u64,
    pub max_output_bytes: usize,
    pub max_log_lines: usize,
    pub max_log_bytes: usize,
    pub max_host_calls: u32,
    /// Gateway-wide cap on concurrently running sandbox executions; further
    /// executions queue (and keep burning their wall-clock timeout).
    pub max_concurrent_executions: usize,
}

impl Default for CodeModeLimits {
    fn default() -> Self {
        Self {
            execution_timeout_ms: 30_000,
            memory_limit_bytes: 64 * 1024 * 1024,
            max_output_bytes: 32_768,
            max_log_lines: 100,
            max_log_bytes: 16_384,
            max_host_calls: 50,
            max_concurrent_executions: 4,
        }
    }
}

/// Result of one Code Mode execution. Guest failures land in `error`;
/// executors never return `Err` for guest-level problems.
#[derive(Debug, Clone, Default)]
pub struct ExecutionOutcome {
    pub result: Option<Value>,
    pub error: Option<String>,
    pub logs: Vec<String>,
    pub truncated: bool,
}

/// Infrastructure-level executor failure (never used for guest errors).
#[derive(Debug)]
pub enum ExecutorError {
    Timeout,
    Infrastructure(String),
}

impl fmt::Display for ExecutorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout => write!(f, "code execution timed out"),
            Self::Infrastructure(message) => write!(f, "code executor failed: {message}"),
        }
    }
}

impl std::error::Error for ExecutorError {}

/// Error surfaced to the guest as a catchable exception via the JSON envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostCallError {
    CapabilityDenied {
        name: String,
    },
    UnknownFunction {
        namespace: String,
        name: String,
    },
    CallLimitExceeded {
        max: u32,
    },
    ArgumentsTooLarge {
        limit_bytes: usize,
    },
    InvalidArguments {
        message: String,
    },
    Tool {
        error_code: &'static str,
        message: String,
    },
}

impl fmt::Display for HostCallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Keep in sync with `CAPABILITY_DENIED_MARKER`.
            Self::CapabilityDenied { name } => write!(
                f,
                "oceans.{name} is not available in this capability profile"
            ),
            Self::UnknownFunction { namespace, name } => {
                write!(f, "unknown host function `{namespace}.{name}`")
            }
            Self::CallLimitExceeded { max } => {
                write!(
                    f,
                    "host call limit exceeded: at most {max} calls per execution"
                )
            }
            Self::ArgumentsTooLarge { limit_bytes } => {
                write!(f, "host call arguments exceed {limit_bytes} bytes")
            }
            Self::InvalidArguments { message } => write!(f, "invalid arguments: {message}"),
            Self::Tool { message, .. } => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for HostCallError {}

/// Cloudflare-compatible guest envelope: `{"result": ...}` / `{"error": ...}`.
#[must_use]
pub fn host_call_envelope(outcome: Result<Value, HostCallError>) -> Value {
    match outcome {
        Ok(result) => json!({ "result": result }),
        Err(error) => json!({ "error": error.to_string() }),
    }
}

#[async_trait]
pub trait CodeExecutor: Send + Sync {
    /// `Err` = infrastructure failure only. Guest failures land in
    /// `ExecutionOutcome.error` (executors never throw for guest errors).
    async fn execute(
        &self,
        code: &str,
        dispatcher: Arc<dyn HostDispatcher>,
        limits: &CodeModeLimits,
    ) -> Result<ExecutionOutcome, ExecutorError>;
}

#[async_trait]
pub trait HostDispatcher: Send + Sync {
    /// namespace `oceans`; name `searchTools` | `describeTool` | `callTool`.
    /// Enforces the capability profile, nested-call count, and arg-size limits.
    async fn call(&self, namespace: &str, name: &str, args: Value) -> Result<Value, HostCallError>;
}

/// Capability + limit policy shared by host dispatchers; independently testable.
#[derive(Debug)]
pub struct HostCallPolicy {
    profile: CapabilityProfile,
    max_host_calls: u32,
    calls: AtomicU32,
    capability_denied: AtomicBool,
}

impl HostCallPolicy {
    #[must_use]
    pub fn new(profile: CapabilityProfile, max_host_calls: u32) -> Self {
        Self {
            profile,
            max_host_calls,
            calls: AtomicU32::new(0),
            capability_denied: AtomicBool::new(false),
        }
    }

    pub fn authorize(
        &self,
        namespace: &str,
        name: &str,
        args: &Value,
    ) -> Result<(), HostCallError> {
        let used = self.calls.fetch_add(1, Ordering::SeqCst);
        if used >= self.max_host_calls {
            return Err(HostCallError::CallLimitExceeded {
                max: self.max_host_calls,
            });
        }
        if namespace != OCEANS_HOST_NAMESPACE
            || !matches!(
                name,
                HOST_FN_SEARCH_TOOLS | HOST_FN_DESCRIBE_TOOL | HOST_FN_CALL_TOOL
            )
        {
            return Err(HostCallError::UnknownFunction {
                namespace: namespace.to_string(),
                name: name.to_string(),
            });
        }
        if !self.profile.allows_host_function(name) {
            self.capability_denied.store(true, Ordering::SeqCst);
            return Err(HostCallError::CapabilityDenied {
                name: name.to_string(),
            });
        }
        let encoded_len = serde_json::to_vec(args)
            .map(|bytes| bytes.len())
            .unwrap_or(usize::MAX);
        if encoded_len > MAX_HOST_CALL_ARGUMENT_BYTES {
            return Err(HostCallError::ArgumentsTooLarge {
                limit_bytes: MAX_HOST_CALL_ARGUMENT_BYTES,
            });
        }
        Ok(())
    }

    #[must_use]
    pub fn capability_denied(&self) -> bool {
        self.capability_denied.load(Ordering::SeqCst)
    }
}

/// Host dispatcher backed by the real catalog/access/credential services.
/// Nested invocation logs are buffered and flushed by [`CodeModeService`]
/// after the parent invocation row exists (FK ordering).
pub struct OceansHostDispatcher<R> {
    repo: Arc<R>,
    auth: AuthenticatedApiKey,
    request_id: String,
    policy: HostCallPolicy,
    pending_logs: Mutex<Vec<McpInvocationLogInput>>,
}

impl<R> OceansHostDispatcher<R>
where
    R: McpAccessRepository
        + IdentityRepository
        + McpRegistryRepository
        + McpUpstreamCredentialRepository
        + McpToolInvocationRepository
        + Send
        + Sync,
{
    #[must_use]
    pub fn new(
        repo: Arc<R>,
        auth: AuthenticatedApiKey,
        request_id: String,
        profile: CapabilityProfile,
        limits: &CodeModeLimits,
    ) -> Self {
        Self {
            repo,
            auth,
            request_id,
            policy: HostCallPolicy::new(profile, limits.max_host_calls),
            pending_logs: Mutex::new(Vec::new()),
        }
    }

    #[must_use]
    pub fn capability_denied(&self) -> bool {
        self.policy.capability_denied()
    }

    #[must_use]
    pub fn take_pending_logs(&self) -> Vec<McpInvocationLogInput> {
        std::mem::take(
            &mut self
                .pending_logs
                .lock()
                .expect("pending log mutex poisoned"),
        )
    }

    async fn search_tools(&self, args: Value) -> Result<Value, HostCallError> {
        let input: SearchMcpToolsInput = parse_host_args(args)?;
        let output = McpCatalog::new(self.repo.clone())
            .search_tools(&self.auth, input)
            .await
            .map_err(tool_error)?;
        serde_json::to_value(output).map_err(serialization_error)
    }

    async fn describe_tool(&self, args: Value) -> Result<Value, HostCallError> {
        let input: DescribeMcpToolInput = parse_host_args(args)?;
        let output = McpCatalog::new(self.repo.clone())
            .describe_tool(&self.auth, input)
            .await
            .map_err(tool_error)?;
        serde_json::to_value(output).map_err(serialization_error)
    }

    async fn call_tool(&self, args: Value) -> Result<Value, HostCallError> {
        let input: CallMcpToolInput = parse_host_args(args)?;
        let started_at = Instant::now();
        let record = McpCatalog::new(self.repo.clone())
            .authorized_tool_by_address(&self.auth, &input.address)
            .await
            .map_err(tool_error)?;
        if let Some(schema_hash) = input.schema_hash.as_deref()
            && schema_hash != record.tool.schema_hash
        {
            return tool_error_value(
                "Tool schema changed",
                "tool_schema_changed",
                json!({
                    "address": input.address,
                    "expected_schema_hash": schema_hash,
                    "actual_schema_hash": record.tool.schema_hash,
                    "schema_version": record.tool.schema_version
                }),
            );
        }

        let gateway = McpGatewayService::new(self.repo.clone());
        let upstream = match gateway
            .prepare_upstream_for_auth(&self.auth, record.server.clone())
            .await
        {
            Ok(upstream) => upstream,
            Err(error @ GatewayError::McpCredentialRequired { .. })
            | Err(error @ GatewayError::McpCredentialExpired { .. }) => {
                self.buffer_nested_log(
                    &record,
                    McpToolInvocationStatus::Unauthorized,
                    Some(error.error_code().to_string()),
                    Some(input.arguments.clone()),
                    None,
                    started_at,
                );
                return tool_error_value(
                    error.to_string(),
                    error.error_code(),
                    json!({
                        "address": input.address,
                        "server_key": record.server.server_key
                    }),
                );
            }
            Err(error) => return Err(tool_error(error)),
        };

        let client = StreamableHttpClient::new(
            &upstream.server.server_url,
            StdDuration::from_millis(upstream.server.timeout_ms.max(1) as u64),
        )
        .map_err(|error| tool_error(map_mcp_client_error(error)))?;
        let arguments = if input.arguments.is_null() {
            json!({})
        } else {
            input.arguments.clone()
        };
        match client
            .call_tool(
                upstream.headers.as_ref(),
                &record.tool.upstream_name,
                arguments.clone(),
            )
            .await
        {
            Ok(result) => {
                let result_json = serde_json::to_value(&result).ok();
                self.buffer_nested_log(
                    &record,
                    if result.is_error.unwrap_or(false) {
                        McpToolInvocationStatus::UpstreamError
                    } else {
                        McpToolInvocationStatus::Success
                    },
                    None,
                    Some(arguments),
                    result_json.clone(),
                    started_at,
                );
                result_json.ok_or_else(|| HostCallError::Tool {
                    error_code: "internal_error",
                    message: "upstream tool result could not be serialized".to_string(),
                })
            }
            Err(error) => {
                let gateway_error = map_mcp_client_error(error);
                self.buffer_nested_log(
                    &record,
                    invocation_status_for_error(&gateway_error),
                    Some(gateway_error.error_code().to_string()),
                    Some(arguments),
                    None,
                    started_at,
                );
                tool_error_value(
                    gateway_error.to_string(),
                    gateway_error.error_code(),
                    json!({
                        "address": input.address,
                        "server_key": record.server.server_key
                    }),
                )
            }
        }
    }

    fn buffer_nested_log(
        &self,
        record: &McpCatalogToolRecord,
        status: McpToolInvocationStatus,
        error_code: Option<String>,
        arguments_json: Option<Value>,
        result_json: Option<Value>,
        started_at: Instant,
    ) {
        let input = McpInvocationLogInput {
            request_log_id: None,
            parent_invocation_id: None,
            request_id: self.request_id.clone(),
            server_id: Some(record.server.mcp_server_id),
            server_display_key: record.server.server_key.clone(),
            server_display_name: record.server.display_name.clone(),
            tool_id: Some(record.tool.mcp_tool_id),
            tool_display_key: record.tool.upstream_name.clone(),
            tool_display_name: record.tool.display_name.clone(),
            status,
            policy_result: McpToolPolicyResult::Allowed,
            latency_ms: Some(started_at.elapsed().as_millis() as i64),
            error_code,
            arguments_json,
            result_json,
            metadata: Map::from_iter([("mcp_route".to_string(), json!("code-mode"))]),
            occurred_at: OffsetDateTime::now_utc(),
        };
        self.pending_logs
            .lock()
            .expect("pending log mutex poisoned")
            .push(input);
    }
}

#[async_trait]
impl<R> HostDispatcher for OceansHostDispatcher<R>
where
    R: McpAccessRepository
        + IdentityRepository
        + McpRegistryRepository
        + McpUpstreamCredentialRepository
        + McpToolInvocationRepository
        + Send
        + Sync,
{
    async fn call(&self, namespace: &str, name: &str, args: Value) -> Result<Value, HostCallError> {
        self.policy.authorize(namespace, name, &args)?;
        match name {
            HOST_FN_SEARCH_TOOLS => self.search_tools(args).await,
            HOST_FN_DESCRIBE_TOOL => self.describe_tool(args).await,
            HOST_FN_CALL_TOOL => self.call_tool(args).await,
            _ => unreachable!("policy rejects unknown host functions"),
        }
    }
}

fn parse_host_args<T: serde::de::DeserializeOwned>(args: Value) -> Result<T, HostCallError> {
    let args = if args.is_null() { json!({}) } else { args };
    serde_json::from_value(args).map_err(|error| HostCallError::InvalidArguments {
        message: error.to_string(),
    })
}

fn tool_error(error: GatewayError) -> HostCallError {
    HostCallError::Tool {
        error_code: error.error_code(),
        message: error.to_string(),
    }
}

fn serialization_error(error: serde_json::Error) -> HostCallError {
    HostCallError::Tool {
        error_code: "internal_error",
        message: error.to_string(),
    }
}

/// Builds an aggregate-`call_tool`-compatible error result so Code Mode
/// callers see the same structured contracts (`tool_schema_changed`,
/// `credential_required`, ...) as `/mcp` clients.
fn tool_error_value(
    message: impl Into<String>,
    error_code: impl Into<String>,
    structured: Value,
) -> Result<Value, HostCallError> {
    serde_json::to_value(call_tool_error_result(message, error_code, structured))
        .map_err(serialization_error)
}

/// Deterministic in-process executor for route and service tests. The `code`
/// string is interpreted as a JSON script:
///
/// ```json
/// {
///   "calls": [{"name": "searchTools", "args": {"query": "github"}}],
///   "logs": ["line"],
///   "result": {"optional": "static result"},
///   "fail": "optional guest error message",
///   "fail_on_error": false
/// }
/// ```
///
/// `fail_on_error: true` simulates an uncaught guest exception: the first
/// host-call error envelope becomes the guest error.
///
/// Each scripted call is dispatched through the real [`HostDispatcher`] and
/// collected as a `{"result"}/{"error"}` envelope. When `result` is omitted
/// the outcome result is the array of call envelopes. Invalid scripts become
/// guest errors (never `Err`), matching the executor contract.
#[derive(Debug, Clone, Copy, Default)]
pub struct DeterministicTestExecutor;

#[derive(Debug, Deserialize)]
struct TestScript {
    #[serde(default)]
    calls: Vec<TestScriptCall>,
    #[serde(default)]
    logs: Vec<String>,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    fail: Option<String>,
    #[serde(default)]
    fail_on_error: bool,
}

#[derive(Debug, Deserialize)]
struct TestScriptCall {
    #[serde(default = "default_namespace")]
    namespace: String,
    name: String,
    #[serde(default)]
    args: Value,
}

fn default_namespace() -> String {
    OCEANS_HOST_NAMESPACE.to_string()
}

#[async_trait]
impl CodeExecutor for DeterministicTestExecutor {
    async fn execute(
        &self,
        code: &str,
        dispatcher: Arc<dyn HostDispatcher>,
        _limits: &CodeModeLimits,
    ) -> Result<ExecutionOutcome, ExecutorError> {
        let script: TestScript = match serde_json::from_str(code) {
            Ok(script) => script,
            Err(error) => {
                return Ok(ExecutionOutcome {
                    error: Some(format!("invalid test script: {error}")),
                    ..ExecutionOutcome::default()
                });
            }
        };
        let mut envelopes = Vec::with_capacity(script.calls.len());
        let mut uncaught_error = None;
        for call in script.calls {
            let outcome = dispatcher
                .call(&call.namespace, &call.name, call.args)
                .await;
            if script.fail_on_error
                && uncaught_error.is_none()
                && let Err(error) = &outcome
            {
                uncaught_error = Some(error.to_string());
            }
            envelopes.push(host_call_envelope(outcome));
        }
        let error = script.fail.or(uncaught_error);
        Ok(ExecutionOutcome {
            result: if error.is_some() {
                None
            } else {
                Some(script.result.unwrap_or(Value::Array(envelopes)))
            },
            error,
            logs: script.logs,
            truncated: false,
        })
    }
}

/// Outcome of one explore/execute tool call, post limit enforcement.
#[derive(Debug, Clone)]
pub struct CodeModeRunOutcome {
    pub status: McpToolInvocationStatus,
    pub error_code: Option<String>,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub logs: Vec<String>,
    pub truncated: bool,
    pub parent_invocation_id: Option<Uuid>,
}

/// Orchestrates one Code Mode execution: dispatcher scoping, executor run,
/// limit enforcement, and parent/nested invocation logging.
pub struct CodeModeService<R> {
    repo: Arc<R>,
    executor: Arc<dyn CodeExecutor>,
    limits: CodeModeLimits,
    payload_policy: McpInvocationPayloadPolicy,
}

impl<R> CodeModeService<R>
where
    R: McpAccessRepository
        + IdentityRepository
        + McpRegistryRepository
        + McpUpstreamCredentialRepository
        + McpToolInvocationRepository
        + Send
        + Sync
        + 'static,
{
    #[must_use]
    pub fn new(repo: Arc<R>, executor: Arc<dyn CodeExecutor>, limits: CodeModeLimits) -> Self {
        Self::new_with_payload_policy(repo, executor, limits, McpInvocationPayloadPolicy::default())
    }

    /// Parent and nested invocation payloads obey the same
    /// [`McpInvocationPayloadPolicy`] (capture on/off, byte caps, key-based
    /// redaction) as every other invocation payload. Note that key-based
    /// redaction cannot scrub inside the opaque submitted `code` string or
    /// captured console log lines; operators can disable payload capture
    /// entirely via the policy.
    #[must_use]
    pub fn new_with_payload_policy(
        repo: Arc<R>,
        executor: Arc<dyn CodeExecutor>,
        limits: CodeModeLimits,
        payload_policy: McpInvocationPayloadPolicy,
    ) -> Self {
        Self {
            repo,
            executor,
            limits,
            payload_policy,
        }
    }

    fn logger(&self) -> McpInvocationLogging<R> {
        McpInvocationLogging::new_with_payload_policy(
            self.repo.clone(),
            self.payload_policy.clone(),
        )
    }

    pub async fn run(
        &self,
        auth: &AuthenticatedApiKey,
        profile: CapabilityProfile,
        code: &str,
        request_id: String,
    ) -> CodeModeRunOutcome {
        let started_at = Instant::now();
        let dispatcher = Arc::new(OceansHostDispatcher::new(
            self.repo.clone(),
            auth.clone(),
            request_id.clone(),
            profile,
            &self.limits,
        ));
        let execution = self
            .executor
            .execute(code, dispatcher.clone(), &self.limits)
            .await;

        let (outcome, status, error_code) = match execution {
            Ok(outcome) => {
                let (status, error_code) = match &outcome.error {
                    None => (McpToolInvocationStatus::Success, None),
                    // Attribute the failure to a capability denial only when
                    // the *uncaught* error is one: code that catches a denial
                    // and later fails for an unrelated reason must not be
                    // audited as policy-denied.
                    Some(error)
                        if dispatcher.capability_denied()
                            && error.contains(CAPABILITY_DENIED_MARKER) =>
                    {
                        (
                            McpToolInvocationStatus::PolicyDenied,
                            Some("capability_denied".to_string()),
                        )
                    }
                    Some(_) => (
                        McpToolInvocationStatus::GatewayError,
                        Some("code_execution_error".to_string()),
                    ),
                };
                (outcome, status, error_code)
            }
            Err(ExecutorError::Timeout) => (
                ExecutionOutcome {
                    error: Some(ExecutorError::Timeout.to_string()),
                    ..ExecutionOutcome::default()
                },
                McpToolInvocationStatus::Timeout,
                Some("timeout".to_string()),
            ),
            Err(error) => (
                ExecutionOutcome {
                    error: Some(error.to_string()),
                    ..ExecutionOutcome::default()
                },
                McpToolInvocationStatus::GatewayError,
                Some("code_mode_executor_error".to_string()),
            ),
        };
        let outcome = apply_outcome_limits(outcome, &self.limits);

        let parent_invocation_id = self
            .log_parent_invocation(
                auth,
                profile,
                &request_id,
                status,
                error_code.clone(),
                code,
                &outcome,
                started_at,
            )
            .await;
        // Nested rows describe upstream calls that already executed, so they
        // are flushed even when the parent insert failed (with a NULL parent
        // as the fallback linkage) rather than silently dropped.
        let pending_logs = dispatcher.take_pending_logs();
        if parent_invocation_id.is_none() && !pending_logs.is_empty() {
            tracing::warn!(
                nested_rows = pending_logs.len(),
                "code mode parent invocation insert failed; \
                 flushing nested invocation rows without parent linkage"
            );
        }
        let logger = self.logger();
        for mut input in pending_logs {
            input.parent_invocation_id = parent_invocation_id;
            if let Err(error) = logger.log_invocation(auth, input).await {
                tracing::warn!(%error, "failed writing code mode nested invocation log row");
            }
        }

        CodeModeRunOutcome {
            status,
            error_code,
            result: outcome.result,
            error: outcome.error,
            logs: outcome.logs,
            truncated: outcome.truncated,
            parent_invocation_id,
        }
    }

    /// Logs the parent invocation row for a request that never reached the
    /// executor (for example a missing or invalid `code` argument).
    pub async fn log_invalid_request(
        &self,
        auth: &AuthenticatedApiKey,
        profile: CapabilityProfile,
        request_id: String,
        message: &str,
    ) {
        let logger = self.logger();
        let _ = logger
            .log_invocation(
                auth,
                McpInvocationLogInput {
                    request_log_id: None,
                    parent_invocation_id: None,
                    request_id,
                    server_id: None,
                    server_display_key: CODE_MODE_SERVER_DISPLAY_KEY.to_string(),
                    server_display_name: CODE_MODE_SERVER_DISPLAY_NAME.to_string(),
                    tool_id: None,
                    tool_display_key: profile.tool_display_key().to_string(),
                    tool_display_name: profile.tool_display_name().to_string(),
                    status: McpToolInvocationStatus::InvalidRequest,
                    policy_result: McpToolPolicyResult::NotEvaluated,
                    latency_ms: Some(0),
                    error_code: Some("invalid_request".to_string()),
                    arguments_json: None,
                    result_json: Some(json!({ "error": message })),
                    metadata: code_mode_metadata(),
                    occurred_at: OffsetDateTime::now_utc(),
                },
            )
            .await;
    }

    #[allow(clippy::too_many_arguments)]
    async fn log_parent_invocation(
        &self,
        auth: &AuthenticatedApiKey,
        profile: CapabilityProfile,
        request_id: &str,
        status: McpToolInvocationStatus,
        error_code: Option<String>,
        code: &str,
        outcome: &ExecutionOutcome,
        started_at: Instant,
    ) -> Option<Uuid> {
        self.logger()
            .log_invocation(
                auth,
                McpInvocationLogInput {
                    request_log_id: None,
                    parent_invocation_id: None,
                    request_id: request_id.to_string(),
                    server_id: None,
                    server_display_key: CODE_MODE_SERVER_DISPLAY_KEY.to_string(),
                    server_display_name: CODE_MODE_SERVER_DISPLAY_NAME.to_string(),
                    tool_id: None,
                    tool_display_key: profile.tool_display_key().to_string(),
                    tool_display_name: profile.tool_display_name().to_string(),
                    status,
                    policy_result: if status == McpToolInvocationStatus::PolicyDenied {
                        McpToolPolicyResult::Denied
                    } else {
                        McpToolPolicyResult::Allowed
                    },
                    latency_ms: Some(started_at.elapsed().as_millis() as i64),
                    error_code,
                    arguments_json: Some(json!({ "code": code })),
                    result_json: Some(json!({
                        "result": outcome.result,
                        "error": outcome.error,
                        "logs": outcome.logs,
                        "truncated": outcome.truncated,
                    })),
                    metadata: code_mode_metadata(),
                    occurred_at: OffsetDateTime::now_utc(),
                },
            )
            .await
            .map_err(
                |error| tracing::warn!(%error, "failed writing code mode parent invocation row"),
            )
            .ok()
            .map(|logged| logged.mcp_tool_invocation_id)
    }
}

fn code_mode_metadata() -> Map<String, Value> {
    Map::from_iter([("mcp_route".to_string(), json!("code-mode"))])
}

/// Applies output and log caps to an execution outcome. Backends capture raw
/// output; the gateway enforces bounds centrally so every executor behaves
/// identically.
#[must_use]
pub fn apply_outcome_limits(
    outcome: ExecutionOutcome,
    limits: &CodeModeLimits,
) -> ExecutionOutcome {
    let (result, result_truncated) = cap_result(outcome.result, limits.max_output_bytes);
    let (logs, logs_truncated) = cap_logs(outcome.logs, limits);
    ExecutionOutcome {
        result,
        error: outcome.error,
        logs,
        truncated: outcome.truncated || result_truncated || logs_truncated,
    }
}

fn cap_result(result: Option<Value>, max_output_bytes: usize) -> (Option<Value>, bool) {
    let Some(value) = result else {
        return (None, false);
    };
    match serde_json::to_string(&value) {
        Ok(encoded) if encoded.len() <= max_output_bytes => (Some(value), false),
        Ok(encoded) => (
            Some(Value::String(truncate_utf8(&encoded, max_output_bytes))),
            true,
        ),
        Err(_) => (
            Some(Value::String("output serialization failed".to_string())),
            true,
        ),
    }
}

/// Returns the capped log lines plus whether any line or byte was dropped.
fn cap_logs(logs: Vec<String>, limits: &CodeModeLimits) -> (Vec<String>, bool) {
    let total_lines = logs.len();
    let mut capped = Vec::new();
    let mut used_bytes = 0usize;
    let mut truncated = false;
    for line in logs.into_iter().take(limits.max_log_lines) {
        if used_bytes + line.len() > limits.max_log_bytes {
            let remaining = limits.max_log_bytes.saturating_sub(used_bytes);
            if remaining > 0 {
                capped.push(truncate_utf8(&line, remaining));
            }
            truncated = true;
            break;
        }
        used_bytes += line.len();
        capped.push(line);
    }
    let truncated = truncated || capped.len() < total_lines;
    (capped, truncated)
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    let mut end = max_bytes.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    struct RecordingDispatcher {
        calls: Mutex<Vec<(String, String, Value)>>,
        responses: Mutex<VecDeque<Result<Value, HostCallError>>>,
    }

    impl RecordingDispatcher {
        fn new(responses: Vec<Result<Value, HostCallError>>) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                responses: Mutex::new(responses.into()),
            }
        }
    }

    #[async_trait]
    impl HostDispatcher for RecordingDispatcher {
        async fn call(
            &self,
            namespace: &str,
            name: &str,
            args: Value,
        ) -> Result<Value, HostCallError> {
            self.calls
                .lock()
                .expect("calls")
                .push((namespace.to_string(), name.to_string(), args));
            self.responses
                .lock()
                .expect("responses")
                .pop_front()
                .unwrap_or(Ok(Value::Null))
        }
    }

    #[test]
    fn capability_profiles_gate_call_tool() {
        assert!(CapabilityProfile::Explore.allows_host_function(HOST_FN_SEARCH_TOOLS));
        assert!(CapabilityProfile::Explore.allows_host_function(HOST_FN_DESCRIBE_TOOL));
        assert!(!CapabilityProfile::Explore.allows_host_function(HOST_FN_CALL_TOOL));
        assert!(CapabilityProfile::Execute.allows_host_function(HOST_FN_CALL_TOOL));
        assert!(!CapabilityProfile::Execute.allows_host_function("evalCode"));
        assert_eq!(
            CapabilityProfile::from_tool_name("explore"),
            Some(CapabilityProfile::Explore)
        );
        assert_eq!(
            CapabilityProfile::from_tool_name("execute"),
            Some(CapabilityProfile::Execute)
        );
        assert_eq!(CapabilityProfile::from_tool_name("call_tool"), None);
    }

    #[test]
    fn host_call_policy_enforces_capability_count_and_arg_size() {
        let policy = HostCallPolicy::new(CapabilityProfile::Explore, 2);
        assert!(
            policy
                .authorize(OCEANS_HOST_NAMESPACE, HOST_FN_SEARCH_TOOLS, &json!({}))
                .is_ok()
        );
        assert!(!policy.capability_denied());
        assert_eq!(
            policy.authorize(OCEANS_HOST_NAMESPACE, HOST_FN_CALL_TOOL, &json!({})),
            Err(HostCallError::CapabilityDenied {
                name: HOST_FN_CALL_TOOL.to_string()
            })
        );
        assert!(policy.capability_denied());
        assert_eq!(
            policy.authorize(OCEANS_HOST_NAMESPACE, HOST_FN_SEARCH_TOOLS, &json!({})),
            Err(HostCallError::CallLimitExceeded { max: 2 })
        );

        let policy = HostCallPolicy::new(CapabilityProfile::Execute, 10);
        assert_eq!(
            policy.authorize("globalThis", HOST_FN_SEARCH_TOOLS, &json!({})),
            Err(HostCallError::UnknownFunction {
                namespace: "globalThis".to_string(),
                name: HOST_FN_SEARCH_TOOLS.to_string()
            })
        );
        let oversized = json!({
            "blob": "x".repeat(MAX_HOST_CALL_ARGUMENT_BYTES + 1)
        });
        assert_eq!(
            policy.authorize(OCEANS_HOST_NAMESPACE, HOST_FN_CALL_TOOL, &oversized),
            Err(HostCallError::ArgumentsTooLarge {
                limit_bytes: MAX_HOST_CALL_ARGUMENT_BYTES
            })
        );
    }

    #[test]
    fn host_call_envelope_uses_result_and_error_shape() {
        assert_eq!(
            host_call_envelope(Ok(json!({"items": []}))),
            json!({"result": {"items": []}})
        );
        let envelope = host_call_envelope(Err(HostCallError::CapabilityDenied {
            name: HOST_FN_CALL_TOOL.to_string(),
        }));
        assert_eq!(
            envelope["error"],
            json!("oceans.callTool is not available in this capability profile")
        );
        assert!(envelope.get("result").is_none());
    }

    #[tokio::test]
    async fn deterministic_executor_reports_invalid_script_as_guest_error() {
        let dispatcher = Arc::new(RecordingDispatcher::new(Vec::new()));
        let outcome = DeterministicTestExecutor
            .execute("not json", dispatcher.clone(), &CodeModeLimits::default())
            .await
            .expect("never Err for guest failures");
        assert!(
            outcome
                .error
                .expect("guest error")
                .contains("invalid test script")
        );
        assert!(outcome.result.is_none());
        assert!(dispatcher.calls.lock().expect("calls").is_empty());
    }

    #[tokio::test]
    async fn deterministic_executor_drives_scripted_host_calls() {
        let dispatcher = Arc::new(RecordingDispatcher::new(vec![
            Ok(json!({"total": 1})),
            Err(HostCallError::CapabilityDenied {
                name: HOST_FN_CALL_TOOL.to_string(),
            }),
        ]));
        let code = json!({
            "calls": [
                {"name": "searchTools", "args": {"query": "github"}},
                {"name": "callTool", "args": {"address": "mcp://github/tools/x"}}
            ],
            "logs": ["started"]
        })
        .to_string();
        let outcome = DeterministicTestExecutor
            .execute(&code, dispatcher.clone(), &CodeModeLimits::default())
            .await
            .expect("execute");
        assert!(outcome.error.is_none());
        assert_eq!(outcome.logs, vec!["started".to_string()]);
        let envelopes = outcome.result.expect("result");
        assert_eq!(envelopes[0], json!({"result": {"total": 1}}));
        assert!(
            envelopes[1]["error"]
                .as_str()
                .expect("error")
                .contains("callTool")
        );
        let calls = dispatcher.calls.lock().expect("calls");
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, OCEANS_HOST_NAMESPACE);
        assert_eq!(calls[0].1, HOST_FN_SEARCH_TOOLS);
    }

    #[tokio::test]
    async fn deterministic_executor_guest_failure_clears_result() {
        let dispatcher = Arc::new(RecordingDispatcher::new(Vec::new()));
        let code = json!({"fail": "boom", "logs": ["before failure"]}).to_string();
        let outcome = DeterministicTestExecutor
            .execute(&code, dispatcher, &CodeModeLimits::default())
            .await
            .expect("execute");
        assert_eq!(outcome.error.as_deref(), Some("boom"));
        assert!(outcome.result.is_none());
    }

    #[test]
    fn outcome_limits_truncate_output_with_flag() {
        let limits = CodeModeLimits {
            max_output_bytes: 16,
            ..CodeModeLimits::default()
        };
        let outcome = apply_outcome_limits(
            ExecutionOutcome {
                result: Some(json!({"big": "x".repeat(64)})),
                ..ExecutionOutcome::default()
            },
            &limits,
        );
        assert!(outcome.truncated);
        let preview = outcome
            .result
            .expect("preview")
            .as_str()
            .expect("string")
            .to_string();
        assert!(preview.len() <= 16);

        let untouched = apply_outcome_limits(
            ExecutionOutcome {
                result: Some(json!({"ok": true})),
                ..ExecutionOutcome::default()
            },
            &limits,
        );
        assert!(!untouched.truncated);
        assert_eq!(untouched.result, Some(json!({"ok": true})));
    }

    #[test]
    fn outcome_limits_cap_log_lines_and_bytes() {
        let limits = CodeModeLimits {
            max_log_lines: 2,
            max_log_bytes: 12,
            ..CodeModeLimits::default()
        };
        let outcome = apply_outcome_limits(
            ExecutionOutcome {
                logs: vec![
                    "0123456789".to_string(),
                    "abcdefghij".to_string(),
                    "never seen".to_string(),
                ],
                ..ExecutionOutcome::default()
            },
            &limits,
        );
        assert_eq!(
            outcome.logs,
            vec!["0123456789".to_string(), "ab".to_string()]
        );
        // Dropped lines/bytes are reported through the truncation flag.
        assert!(outcome.truncated);

        let untouched = apply_outcome_limits(
            ExecutionOutcome {
                logs: vec!["short".to_string()],
                ..ExecutionOutcome::default()
            },
            &CodeModeLimits::default(),
        );
        assert!(!untouched.truncated);
    }
}
