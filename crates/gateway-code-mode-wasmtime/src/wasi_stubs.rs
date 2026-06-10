//! Minimal `wasi_snapshot_preview1` stubs for the QuickJS guest.
//!
//! No WASI context is linked. The guest is built from Rust + quickjs-ng for
//! `wasm32-wasip1`, whose libc startup and allocator paths import a handful
//! of WASI functions (entropy for hash seeds, clocks for `Date`, stdio for
//! abort messages). Each import is satisfied here with the smallest possible
//! behavior and **zero capabilities**: no filesystem, no network, no
//! environment, no process control. Anything else traps.

use std::time::{SystemTime, UNIX_EPOCH};

use rand::RngCore;
use wasmtime::{Caller, Extern, Linker, Memory, Result, bail, error::Context, format_err};

use crate::ExecutionState;

const WASI_MODULE: &str = "wasi_snapshot_preview1";
const ERRNO_SUCCESS: i32 = 0;
const ERRNO_BADF: i32 = 8;
const ERRNO_INVAL: i32 = 28;

/// Upper bound on one `random_get` request. libc/quickjs-ng only ever ask
/// for a few bytes of seed material; the cap stops a compromised guest from
/// driving large host-side allocations with an attacker-controlled length.
const MAX_RANDOM_GET_BYTES: u32 = 4096;
/// Upper bound on `fd_write` iovec counts. libc uses 1-2 iovecs; the cap
/// bounds host-side loop work before any per-iovec memory reads happen.
const MAX_FD_WRITE_IOVS: u32 = 64;
/// `clock_time_get` precision floor: timestamps are quantized to 1 ms so
/// hostile guest code cannot use the clock as a high-resolution timer for
/// cache-timing/Spectre-class side-channel probes (the same mitigation
/// Cloudflare Workers applies).
const CLOCK_QUANTUM_NANOS: u64 = 1_000_000;

/// Registers every WASI import the guest artifact declares. The guest module
/// is validated against the linker at startup, so a guest rebuild that grows
/// new WASI imports fails loudly instead of silently gaining capabilities.
pub(crate) fn add_wasi_stubs(linker: &mut Linker<ExecutionState>) -> Result<()> {
    linker.func_wrap(WASI_MODULE, "random_get", random_get)?;
    linker.func_wrap(WASI_MODULE, "clock_time_get", clock_time_get)?;
    linker.func_wrap(
        WASI_MODULE,
        "environ_get",
        |_: Caller<'_, ExecutionState>, _: u32, _: u32| ERRNO_SUCCESS,
    )?;
    linker.func_wrap(WASI_MODULE, "environ_sizes_get", environ_sizes_get)?;
    linker.func_wrap(WASI_MODULE, "fd_write", fd_write)?;
    linker.func_wrap(WASI_MODULE, "fd_close", |_: u32| ERRNO_BADF)?;
    linker.func_wrap(WASI_MODULE, "fd_fdstat_get", |_: u32, _: u32| ERRNO_BADF)?;
    linker.func_wrap(WASI_MODULE, "fd_seek", |_: u32, _: i64, _: u32, _: u32| {
        ERRNO_BADF
    })?;
    linker.func_wrap(WASI_MODULE, "fd_prestat_get", |_: u32, _: u32| ERRNO_BADF)?;
    linker.func_wrap(
        WASI_MODULE,
        "fd_prestat_dir_name",
        |_: u32, _: u32, _: u32| ERRNO_BADF,
    )?;
    linker.func_wrap(WASI_MODULE, "proc_exit", |code: i32| -> Result<()> {
        bail!("guest requested process exit with code {code}")
    })?;
    Ok(())
}

fn guest_memory(caller: &mut Caller<'_, ExecutionState>) -> Result<Memory> {
    match caller.get_export("memory") {
        Some(Extern::Memory(memory)) => Ok(memory),
        _ => Err(format_err!("guest does not export linear memory")),
    }
}

/// Fills the requested buffer with host CSPRNG bytes (quickjs-ng hash seeds
/// and `Math.random` seeding). No other entropy state leaks into the guest.
/// The length is bounded before any host-side allocation happens.
fn random_get(mut caller: Caller<'_, ExecutionState>, buf: u32, len: u32) -> Result<i32> {
    if len > MAX_RANDOM_GET_BYTES {
        return Ok(ERRNO_INVAL);
    }
    let memory = guest_memory(&mut caller)?;
    let mut bytes = vec![0u8; len as usize];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    memory
        .write(&mut caller, buf as usize, &bytes)
        .context("random_get out of bounds")?;
    Ok(ERRNO_SUCCESS)
}

/// Wall-clock time for every clock id, quantized to 1 ms; `Date.now()` works
/// while no monotonic scheduling or timer capability exists in the guest and
/// no high-resolution timing primitive is exposed to hostile code (see
/// [`CLOCK_QUANTUM_NANOS`]).
fn clock_time_get(
    mut caller: Caller<'_, ExecutionState>,
    _id: u32,
    _precision: i64,
    out: u32,
) -> Result<i32> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos() as u64)
        .unwrap_or(0);
    let nanos = nanos - (nanos % CLOCK_QUANTUM_NANOS);
    let memory = guest_memory(&mut caller)?;
    memory
        .write(&mut caller, out as usize, &nanos.to_le_bytes())
        .context("clock_time_get out of bounds")?;
    Ok(ERRNO_SUCCESS)
}

/// Reports an empty environment (count = 0, buffer size = 0).
fn environ_sizes_get(
    mut caller: Caller<'_, ExecutionState>,
    count_out: u32,
    size_out: u32,
) -> Result<i32> {
    let memory = guest_memory(&mut caller)?;
    memory
        .write(&mut caller, count_out as usize, &0u32.to_le_bytes())
        .context("environ_sizes_get out of bounds")?;
    memory
        .write(&mut caller, size_out as usize, &0u32.to_le_bytes())
        .context("environ_sizes_get out of bounds")?;
    Ok(ERRNO_SUCCESS)
}

/// Swallows writes (libc abort messages on stderr) while reporting success so
/// the guest's panic machinery can run to completion. Guest-visible logging
/// goes through the dedicated `console` capture instead. The iovec count is
/// bounded before any host-side iteration happens.
fn fd_write(
    mut caller: Caller<'_, ExecutionState>,
    _fd: u32,
    iovs: u32,
    iovs_len: u32,
    nwritten: u32,
) -> Result<i32> {
    if iovs_len > MAX_FD_WRITE_IOVS {
        return Ok(ERRNO_INVAL);
    }
    let memory = guest_memory(&mut caller)?;
    let mut total: u32 = 0;
    for index in 0..iovs_len {
        let mut iovec = [0u8; 8];
        memory
            .read(&caller, (iovs + index * 8) as usize, &mut iovec)
            .context("fd_write iovec out of bounds")?;
        let len = u32::from_le_bytes(iovec[4..8].try_into().expect("4-byte slice"));
        total = total.saturating_add(len);
    }
    memory
        .write(&mut caller, nwritten as usize, &total.to_le_bytes())
        .context("fd_write out of bounds")?;
    Ok(ERRNO_SUCCESS)
}
