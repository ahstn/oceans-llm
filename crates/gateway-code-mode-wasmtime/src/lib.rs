//! Wasmtime + QuickJS Code Mode backend.
//!
//! Embeds the checked-in `wasm32-wasip1` guest artifact (built from
//! `crates/code-mode-guest`, refreshed via `mise run code-mode-guest-build`)
//! and executes one caller code string per fresh [`Store`]. The guest's only
//! capability is the `oceans.oceans_call` import, which bridges into the
//! gateway's [`HostDispatcher`]; a handful of zero-capability WASI stubs
//! satisfy the Rust/quickjs-ng libc imports (see [`wasi_stubs`]).
//!
//! Failure mapping follows the Milestone 1 contract: epoch preemption and
//! wall-clock expiry become [`ExecutorError::Timeout`]; memory-cap denials
//! and guest panics become `ExecutionOutcome.error` values (never `Err`).

mod wasi_stubs;

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use gateway_service::{
    CodeExecutor, CodeModeLimits, ExecutionOutcome, ExecutorError, HostDispatcher,
    apply_outcome_limits, host_call_envelope,
};
use serde_json::Value;
use tokio::sync::Semaphore;
use wasmtime::{
    Caller, Config, Engine, Error, Extern, ExternType, InstancePre, Linker, Module,
    ResourceLimiter, Result, Store, Strategy, Trap, TypedFunc, bail, error::Context,
};

const GUEST_WASM: &[u8] = include_bytes!("../guest/code_mode_guest.wasm");
const OCEANS_IMPORT_MODULE: &str = "oceans";
const OCEANS_IMPORT_NAME: &str = "oceans_call";
/// Epoch tick granularity. Each tick forces running guest fibers to yield
/// back to the async executor (cooperative timeslicing); termination is the
/// wall-clock timeout's job.
const EPOCH_TICK: Duration = Duration::from_millis(10);
/// Synchronous wasm stack cap; deliberately below the async fiber stack so
/// guest recursion exhausts wasm limits before the host stack.
const MAX_WASM_STACK_BYTES: usize = 512 * 1024;
const ASYNC_STACK_BYTES: usize = 2 * 1024 * 1024;
/// The guest declares one fixed-size funcref table (623 elements today);
/// this bound only needs to deny hostile growth, not match it exactly.
const MAX_TABLE_ELEMENTS: usize = 4096;

const MEMORY_LIMIT_ERROR: &str = "execution exceeded the sandbox memory limit and was terminated";
const GUEST_FAILURE_ERROR: &str = "code execution failed in the sandbox";

/// State stored per execution: the resource limiter plus the dispatcher the
/// host import forwards to. A fresh value per [`Store`] guarantees isolation.
pub(crate) struct ExecutionState {
    limiter: CodeModeLimiter,
    dispatcher: Arc<dyn HostDispatcher>,
}

/// Memory/instance/table limiter that records memory-cap denials so traps
/// caused by allocation pressure map to a resource-limit outcome.
struct CodeModeLimiter {
    memory_limit_bytes: usize,
    memory_denied: bool,
}

impl ResourceLimiter for CodeModeLimiter {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> Result<bool> {
        if desired > self.memory_limit_bytes {
            self.memory_denied = true;
            return Ok(false);
        }
        Ok(true)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> Result<bool> {
        Ok(desired <= MAX_TABLE_ELEMENTS)
    }

    fn instances(&self) -> usize {
        1
    }

    fn tables(&self) -> usize {
        1
    }

    fn memories(&self) -> usize {
        1
    }
}

/// Code Mode executor backed by Wasmtime (Cranelift only) running the
/// embedded QuickJS guest. Construct once at startup; cheap to share.
pub struct WasmtimeQuickjsExecutor {
    engine: Engine,
    instance_pre: InstancePre<ExecutionState>,
    /// Bounds concurrently running sandbox executions gateway-wide; further
    /// executions queue on `acquire` while their wall-clock timeout keeps
    /// running, so queueing never extends the per-execution budget.
    execution_permits: Arc<Semaphore>,
}

impl WasmtimeQuickjsExecutor {
    /// Compiles and validates the embedded guest module against the
    /// configured limits. Fails loudly when the artifact is invalid, declares
    /// imports the linker does not stub, or cannot fit its baseline memory
    /// footprint inside `limits.memory_limit_bytes`.
    pub fn new(limits: &CodeModeLimits) -> Result<Self> {
        let mut config = Config::new();
        config.strategy(Strategy::Cranelift);
        config.epoch_interruption(true);
        config.max_wasm_stack(MAX_WASM_STACK_BYTES);
        config.async_stack_size(ASYNC_STACK_BYTES);
        // No `Config::async_support` knob: wasmtime 45 deprecated it as a
        // no-op (bytecodealliance/wasmtime#12371) — the async APIs used below
        // (`call_async`, `func_wrap_async`, `instantiate_async`) are always
        // available when the crate's `async` feature is enabled, which the
        // workspace pin guarantees.
        let engine = Engine::new(&config).context("failed building wasmtime engine")?;

        let module = Module::new(&engine, GUEST_WASM)
            .context("embedded code-mode guest module failed validation")?;
        validate_memory_limit(&module, limits.memory_limit_bytes)?;
        let mut linker: Linker<ExecutionState> = Linker::new(&engine);
        wasi_stubs::add_wasi_stubs(&mut linker).context("failed registering WASI stubs")?;
        add_oceans_import(&mut linker).context("failed registering oceans_call import")?;
        let instance_pre = linker
            .instantiate_pre(&module)
            .context("code-mode guest declares imports the gateway does not provide")?;

        spawn_epoch_ticker(&engine);
        Ok(Self {
            engine,
            instance_pre,
            execution_permits: Arc::new(Semaphore::new(limits.max_concurrent_executions.max(1))),
        })
    }

    async fn run_guest(
        &self,
        code: &str,
        dispatcher: Arc<dyn HostDispatcher>,
        limits: &CodeModeLimits,
    ) -> std::result::Result<ExecutionOutcome, ExecutorError> {
        let mut store = Store::new(
            &self.engine,
            ExecutionState {
                limiter: CodeModeLimiter {
                    memory_limit_bytes: usize::try_from(limits.memory_limit_bytes)
                        .unwrap_or(usize::MAX),
                    memory_denied: false,
                },
                dispatcher,
            },
        );
        store.limiter(|state| &mut state.limiter);
        // Epochs are used purely as a cooperative-yield heartbeat: every tick
        // the fiber yields back to the tokio executor, so CPU-bound guest
        // code (`while (true) {}`) cannot pin a runtime worker thread. The
        // wall-clock timeout wrapping `run_guest` is the sole terminator and
        // intentionally keeps burning during host calls (per the plan, the
        // execution budget includes host-call time); when it fires the
        // future is dropped and the suspended fiber is unwound.
        store.set_epoch_deadline(1);
        store.epoch_deadline_async_yield_and_update(1);

        let memory_denied = |store: &Store<ExecutionState>| store.data().limiter.memory_denied;
        match self.call_evaluate(&mut store, code).await {
            // QuickJS often survives a denied `memory.grow` by throwing a
            // catchable "out of memory" error; when the guest still failed,
            // attribute the failure to the memory cap explicitly.
            Ok(mut outcome) => {
                if outcome.error.is_some() && memory_denied(&store) {
                    outcome.error = Some(MEMORY_LIMIT_ERROR.to_string());
                }
                Ok(apply_outcome_limits(outcome, limits))
            }
            Err(error) => map_guest_failure(&error, memory_denied(&store)),
        }
    }

    async fn call_evaluate(
        &self,
        store: &mut Store<ExecutionState>,
        code: &str,
    ) -> Result<ExecutionOutcome> {
        let instance = self
            .instance_pre
            .instantiate_async(&mut *store)
            .await
            .context("guest instantiation failed")?;
        let memory = instance
            .get_memory(&mut *store, "memory")
            .context("guest does not export linear memory")?;
        let alloc: TypedFunc<u32, u32> = instance
            .get_typed_func(&mut *store, "alloc")
            .context("guest does not export alloc")?;
        let evaluate: TypedFunc<(u32, u32), u64> = instance
            .get_typed_func(&mut *store, "evaluate")
            .context("guest does not export evaluate")?;

        let code_len = u32::try_from(code.len())
            .ok()
            .context("code exceeds guest address space")?;
        let code_ptr = if code_len == 0 {
            0
        } else {
            let ptr = alloc
                .call_async(&mut *store, code_len)
                .await
                .context("guest code-buffer allocation failed")?;
            memory
                .write(&mut *store, ptr as usize, code.as_bytes())
                .context("guest code-buffer write failed")?;
            ptr
        };

        let packed = evaluate
            .call_async(&mut *store, (code_ptr, code_len))
            .await
            .context("guest evaluation trapped")?;
        parse_guest_outcome(read_packed_buffer(store, &memory, packed)?)
    }
}

#[async_trait]
impl CodeExecutor for WasmtimeQuickjsExecutor {
    async fn execute(
        &self,
        code: &str,
        dispatcher: Arc<dyn HostDispatcher>,
        limits: &CodeModeLimits,
    ) -> std::result::Result<ExecutionOutcome, ExecutorError> {
        // The wall-clock timeout bounds everything: queueing for an
        // execution permit, guest CPU time (epochs only force yields), and
        // host-call hangs that suspend the guest fiber.
        let deadline = Duration::from_millis(limits.execution_timeout_ms);
        let bounded = async {
            let _permit = self
                .execution_permits
                .acquire()
                .await
                .map_err(|_| ExecutorError::Infrastructure("executor shut down".to_string()))?;
            self.run_guest(code, dispatcher, limits).await
        };
        match tokio::time::timeout(deadline, bounded).await {
            Ok(result) => result,
            Err(_elapsed) => Err(ExecutorError::Timeout),
        }
    }
}

/// Background thread advancing the engine epoch; exits when the engine (and
/// thus the executor) is dropped.
fn spawn_epoch_ticker(engine: &Engine) {
    let weak = engine.weak();
    std::thread::Builder::new()
        .name("code-mode-epoch-ticker".to_string())
        .spawn(move || {
            loop {
                std::thread::sleep(EPOCH_TICK);
                let Some(engine) = weak.upgrade() else {
                    return;
                };
                engine.increment_epoch();
            }
        })
        .expect("failed spawning code-mode epoch ticker thread");
}

/// Fails startup when the configured memory limit cannot fit the guest's
/// baseline (minimum) linear memory: a too-small limit would otherwise deny
/// the *initial* allocation and surface as a per-execution guest error
/// instead of a loud misconfiguration error.
fn validate_memory_limit(module: &Module, memory_limit_bytes: u64) -> Result<()> {
    let minimum_bytes = module
        .exports()
        .find_map(|export| match export.ty() {
            ExternType::Memory(memory) => Some(memory.minimum().saturating_mul(memory.page_size())),
            _ => None,
        })
        .context("code-mode guest does not export a linear memory")?;
    if memory_limit_bytes < minimum_bytes {
        bail!(
            "mcp.code_mode.limits.memory_limit_bytes ({memory_limit_bytes}) is below the \
             embedded guest's minimum memory requirement ({minimum_bytes} bytes)"
        );
    }
    Ok(())
}

/// Guest failures become outcomes, never `Err` — except epoch interrupts,
/// which map to [`ExecutorError::Timeout`] so the route logs the same
/// timeout status as a wall-clock expiry (epochs are yield-only today, so
/// this mapping is defensive). Trap details stay host-side: they can
/// reference guest internals and host call stacks.
fn map_guest_failure(
    error: &Error,
    memory_denied: bool,
) -> std::result::Result<ExecutionOutcome, ExecutorError> {
    if matches!(
        error.root_cause().downcast_ref::<Trap>(),
        Some(Trap::Interrupt)
    ) {
        return Err(ExecutorError::Timeout);
    }
    let message = if memory_denied {
        MEMORY_LIMIT_ERROR
    } else {
        GUEST_FAILURE_ERROR
    };
    Ok(ExecutionOutcome {
        error: Some(message.to_string()),
        ..ExecutionOutcome::default()
    })
}

fn read_packed_buffer(
    store: &mut Store<ExecutionState>,
    memory: &wasmtime::Memory,
    packed: u64,
) -> Result<Vec<u8>> {
    let ptr = (packed >> 32) as usize;
    let len = (packed & 0xffff_ffff) as usize;
    if len == 0 {
        bail!("guest returned an empty outcome buffer");
    }
    let data = memory.data(&mut *store);
    let bytes = data
        .get(ptr..ptr.saturating_add(len))
        .context("guest outcome buffer out of bounds")?;
    Ok(bytes.to_vec())
}

fn parse_guest_outcome(bytes: Vec<u8>) -> Result<ExecutionOutcome> {
    #[derive(serde::Deserialize)]
    struct GuestOutcome {
        #[serde(default)]
        result: Option<Value>,
        #[serde(default)]
        error: Option<String>,
        #[serde(default)]
        logs: Vec<String>,
    }
    let outcome: GuestOutcome =
        serde_json::from_slice(&bytes).context("guest returned a malformed outcome payload")?;
    Ok(ExecutionOutcome {
        result: if outcome.error.is_some() {
            None
        } else {
            outcome.result
        },
        error: outcome.error,
        logs: outcome.logs,
        truncated: false,
    })
}

/// Registers the single host capability the guest receives:
/// `oceans.oceans_call(ns_ptr, ns_len, fn_ptr, fn_len, args_ptr, args_len) -> packed`.
/// The response envelope is written into a guest-allocated buffer and
/// returned as `(ptr << 32) | len`.
fn add_oceans_import(linker: &mut Linker<ExecutionState>) -> Result<()> {
    linker.func_wrap_async(
        OCEANS_IMPORT_MODULE,
        OCEANS_IMPORT_NAME,
        |mut caller: Caller<'_, ExecutionState>,
         (ns_ptr, ns_len, fn_ptr, fn_len, args_ptr, args_len): (u32, u32, u32, u32, u32, u32)| {
            Box::new(async move {
                dispatch_oceans_call(
                    &mut caller,
                    (ns_ptr, ns_len),
                    (fn_ptr, fn_len),
                    (args_ptr, args_len),
                )
                .await
            })
        },
    )?;
    Ok(())
}

async fn dispatch_oceans_call(
    caller: &mut Caller<'_, ExecutionState>,
    namespace: (u32, u32),
    function: (u32, u32),
    arguments: (u32, u32),
) -> Result<u64> {
    let memory = match caller.get_export("memory") {
        Some(Extern::Memory(memory)) => memory,
        _ => bail!("guest does not export linear memory"),
    };
    let namespace = read_guest_string(caller, &memory, namespace)?;
    let function = read_guest_string(caller, &memory, function)?;
    let args_json = read_guest_string(caller, &memory, arguments)?;

    let args: Value = serde_json::from_str(&args_json).unwrap_or(Value::Null);
    let dispatcher = caller.data().dispatcher.clone();
    let envelope = host_call_envelope(dispatcher.call(&namespace, &function, args).await);
    let bytes = serde_json::to_vec(&envelope).context("host envelope serialization failed")?;

    write_guest_buffer(caller, &memory, &bytes).await
}

fn read_guest_string(
    caller: &mut Caller<'_, ExecutionState>,
    memory: &wasmtime::Memory,
    (ptr, len): (u32, u32),
) -> Result<String> {
    let data = memory.data(&mut *caller);
    let bytes = data
        .get(ptr as usize..(ptr as usize).saturating_add(len as usize))
        .context("guest host-call argument out of bounds")?;
    Ok(String::from_utf8_lossy(bytes).into_owned())
}

/// Allocates a buffer via the guest `alloc` export (re-entering wasm),
/// writes `bytes`, and returns the packed `(ptr << 32) | len` value the
/// guest unpacks and frees.
async fn write_guest_buffer(
    caller: &mut Caller<'_, ExecutionState>,
    memory: &wasmtime::Memory,
    bytes: &[u8],
) -> Result<u64> {
    let alloc = match caller.get_export("alloc") {
        Some(Extern::Func(func)) => func
            .typed::<u32, u32>(&mut *caller)
            .context("guest alloc export has an unexpected signature")?,
        _ => bail!("guest does not export alloc"),
    };
    let len = u32::try_from(bytes.len())
        .ok()
        .context("host envelope exceeds guest address space")?;
    let ptr = alloc
        .call_async(&mut *caller, len)
        .await
        .context("guest envelope-buffer allocation failed")?;
    memory
        .write(&mut *caller, ptr as usize, bytes)
        .context("guest envelope-buffer write failed")?;
    Ok((u64::from(ptr) << 32) | u64::from(len))
}
