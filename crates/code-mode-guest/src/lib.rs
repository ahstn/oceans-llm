//! Code Mode QuickJS guest: a `wasm32-wasip1` cdylib embedding quickjs-ng
//! (via rquickjs) that evaluates one caller-supplied JavaScript async arrow
//! function body per instantiation.
//!
//! ABI (all strings are UTF-8 byte ranges in guest linear memory):
//! - exports `alloc(len) -> ptr` / `dealloc(ptr, len)`: buffer management for
//!   both directions; buffers use `Layout::array::<u8>(len)`.
//! - exports `evaluate(code_ptr, code_len) -> packed`: runs the code and
//!   returns a guest-allocated JSON outcome `{"result"|"error", "logs"}`.
//!   `packed` is `(ptr << 32) | len`; the host copies the buffer and then
//!   drops the whole store (one fresh instance per execution), so it never
//!   calls `dealloc` for the outcome buffer. `dealloc` is exported for ABI
//!   completeness; an executor that reused instances would need to call it
//!   to avoid leaking.
//! - imports `oceans.oceans_call(ns_ptr, ns_len, fn_ptr, fn_len, args_ptr,
//!   args_len) -> packed`: the single host function. It returns the
//!   Cloudflare-compatible `{"result"}/{"error"}` JSON envelope in a buffer
//!   the host allocated via the guest `alloc` export; the guest frees it.
//!
//! There is no event loop: every `await oceans.*()` completes synchronously
//! because the host import blocks the guest until the dispatcher answers.
//! The guest always serializes the final outcome explicitly; promise
//! rejections are caught and folded into the `error` field.

use std::{
    alloc::{Layout, alloc as raw_alloc, dealloc as raw_dealloc},
    cell::RefCell,
    slice,
};

use rquickjs::{
    CatchResultExt, CaughtError, Context, Ctx, Error as JsError, Promise, Runtime, Value,
    context::EvalOptions, prelude::Func,
};
use serde_json::{Value as JsonValue, json};

const PRELUDE_JS: &str = include_str!("prelude.js");
const OCEANS_NAMESPACE: &str = "oceans";
/// Below the default 1 MiB wasm shadow stack so QuickJS detects recursion
/// before the linear-memory stack overflows.
const JS_MAX_STACK_BYTES: usize = 256 * 1024;

#[link(wasm_import_module = "oceans")]
unsafe extern "C" {
    fn oceans_call(
        ns_ptr: *const u8,
        ns_len: u32,
        fn_ptr: *const u8,
        fn_len: u32,
        args_ptr: *const u8,
        args_len: u32,
    ) -> u64;
}

thread_local! {
    static LOGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Allocates a buffer the host can write into. Freed via [`dealloc`].
#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: u32) -> *mut u8 {
    if len == 0 {
        return std::ptr::null_mut();
    }
    let layout = Layout::array::<u8>(len as usize).expect("buffer layout");
    unsafe { raw_alloc(layout) }
}

/// Frees a buffer produced by [`alloc`] or returned from [`evaluate`].
///
/// # Safety
/// `ptr`/`len` must describe exactly one live buffer from this module.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dealloc(ptr: *mut u8, len: u32) {
    if ptr.is_null() || len == 0 {
        return;
    }
    let layout = Layout::array::<u8>(len as usize).expect("buffer layout");
    unsafe { raw_dealloc(ptr, layout) };
}

/// Evaluates one caller code string and returns the packed outcome buffer.
///
/// # Safety
/// `code_ptr`/`code_len` must describe a live, valid UTF-8 buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn evaluate(code_ptr: *const u8, code_len: u32) -> u64 {
    let code = if code_ptr.is_null() || code_len == 0 {
        String::new()
    } else {
        let bytes = unsafe { slice::from_raw_parts(code_ptr, code_len as usize) };
        String::from_utf8_lossy(bytes).into_owned()
    };
    let outcome = run_code(&code);
    let logs = LOGS.with(|logs| std::mem::take(&mut *logs.borrow_mut()));
    let payload = match outcome {
        Ok(result) => json!({ "result": result, "logs": logs }),
        Err(error) => json!({ "error": error, "logs": logs }),
    };
    let encoded = serde_json::to_vec(&payload)
        .unwrap_or_else(|_| br#"{"error":"guest outcome serialization failed","logs":[]}"#.into());
    leak_buffer(encoded)
}

fn run_code(code: &str) -> Result<JsonValue, String> {
    let runtime =
        Runtime::new().map_err(|error| format!("guest runtime initialization failed: {error}"))?;
    runtime.set_max_stack_size(JS_MAX_STACK_BYTES);
    let context = Context::full(&runtime)
        .map_err(|error| format!("guest context initialization failed: {error}"))?;
    context.with(|ctx| {
        install_host_bindings(&ctx)?;
        ctx.eval::<(), _>(PRELUDE_JS)
            .map_err(|error| format!("guest prelude failed: {error}"))?;
        evaluate_wrapped(&ctx, code)
    })
}

fn install_host_bindings(ctx: &Ctx<'_>) -> Result<(), String> {
    let globals = ctx.globals();
    globals
        .set(
            "__oceans_host_call",
            Func::from(|name: String, args_json: String| {
                dispatch_host_call(&name, &args_json)
            }),
        )
        .map_err(|error| format!("guest host-call binding failed: {error}"))?;
    globals
        .set(
            "__oceans_log",
            Func::from(|line: String| {
                LOGS.with(|logs| logs.borrow_mut().push(line));
            }),
        )
        .map_err(|error| format!("guest log binding failed: {error}"))
}

/// Evaluates the caller code as an async arrow function body and drives the
/// QuickJS job queue until the resulting promise settles. Host calls complete
/// synchronously, so a pending promise after the queue drains means the code
/// awaited something that can never resolve.
fn evaluate_wrapped(ctx: &Ctx<'_>, code: &str) -> Result<JsonValue, String> {
    let wrapped = format!("(async () => {{\n{code}\n}})()");
    let mut options = EvalOptions::default();
    options.strict = false;
    options.promise = false;
    let promise = ctx
        .eval_with_options::<Promise, _>(wrapped, options)
        .catch(ctx)
        .map_err(format_caught_error)?;
    match promise.finish::<Value>() {
        Ok(value) => stringify_result(ctx, value),
        Err(JsError::WouldBlock) => Err(
            "execution did not complete: awaited a promise that never resolves \
             (the sandbox has no event loop; only `await oceans.*()` calls complete)"
                .to_string(),
        ),
        Err(error) => Err(format_caught_error(CaughtError::from_error(ctx, error))),
    }
}

fn stringify_result<'js>(ctx: &Ctx<'js>, value: Value<'js>) -> Result<JsonValue, String> {
    let encoded = ctx
        .json_stringify(value)
        .catch(ctx)
        .map_err(format_caught_error)?;
    match encoded {
        None => Ok(JsonValue::Null),
        Some(text) => {
            let text = text
                .to_string()
                .map_err(|error| format!("guest result decoding failed: {error}"))?;
            serde_json::from_str(&text)
                .map_err(|error| format!("guest result was not valid JSON: {error}"))
        }
    }
}

fn format_caught_error(error: CaughtError<'_>) -> String {
    match error {
        CaughtError::Exception(exception) => {
            let message = exception
                .message()
                .unwrap_or_else(|| "unknown error".to_string());
            match exception.stack() {
                Some(stack) if !stack.is_empty() => format!("{message}\n{stack}"),
                _ => message,
            }
        }
        other => other.to_string(),
    }
}

/// Bridges one JS-side host call into the wasm import and returns the raw
/// envelope JSON. The JS prelude turns `{"error"}` envelopes into exceptions.
fn dispatch_host_call(name: &str, args_json: &str) -> String {
    let packed = unsafe {
        oceans_call(
            OCEANS_NAMESPACE.as_ptr(),
            OCEANS_NAMESPACE.len() as u32,
            name.as_ptr(),
            name.len() as u32,
            args_json.as_ptr(),
            args_json.len() as u32,
        )
    };
    take_packed_buffer(packed)
        .unwrap_or_else(|| r#"{"error":"host returned an empty envelope"}"#.to_string())
}

fn take_packed_buffer(packed: u64) -> Option<String> {
    let ptr = (packed >> 32) as u32 as *mut u8;
    let len = (packed & 0xffff_ffff) as usize;
    if ptr.is_null() || len == 0 {
        return None;
    }
    let bytes = unsafe { Vec::from_raw_parts(ptr, len, len) };
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn leak_buffer(bytes: Vec<u8>) -> u64 {
    let len = bytes.len() as u64;
    let mut boxed = bytes.into_boxed_slice();
    let ptr = boxed.as_mut_ptr();
    std::mem::forget(boxed);
    ((ptr as u32 as u64) << 32) | len
}
