//! Child processes with bounded time and output, and detached background processes for hooks.

use std::{
    ffi::OsStr,
    fmt, fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

#[cfg(windows)]
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Context;

#[derive(Debug)]
pub enum RunError {
    NotFound,
    Spawn(io::Error),
    TimedOut,
    OutputTooLarge,
    Io(io::Error),
}

impl fmt::Display for RunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => formatter.write_str("the program was not found"),
            Self::Spawn(error) => write!(formatter, "the program could not start: {error}"),
            Self::TimedOut => formatter.write_str("the program timed out"),
            Self::OutputTooLarge => formatter.write_str("the program printed too much output"),
            Self::Io(error) => write!(formatter, "could not read the program's output: {error}"),
        }
    }
}

impl std::error::Error for RunError {}

pub struct Captured {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Runs `command` with stdin and stderr closed and captures at most `max_stdout` bytes of
/// stdout. The child is killed once `timeout` passes; on Unix its process group is
/// killed too. Windows descendant cleanup uses best-effort `taskkill /T`, which cannot
/// guarantee termination after the direct parent exits.
pub fn run_captured(
    command: &mut Command,
    timeout: Duration,
    max_stdout: usize,
) -> Result<Captured, RunError> {
    run_captured_inner(command, timeout, max_stdout, None)
}

/// Like `run_captured`, but also captures bounded stderr for user-facing errors.
pub fn run_captured_with_stderr(
    command: &mut Command,
    timeout: Duration,
    max_stdout: usize,
    max_stderr: usize,
) -> Result<Captured, RunError> {
    run_captured_inner(command, timeout, max_stdout, Some(max_stderr))
}

fn run_captured_inner(
    command: &mut Command,
    timeout: Duration,
    max_stdout: usize,
    max_stderr: Option<usize>,
) -> Result<Captured, RunError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(if max_stderr.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Its own process group, so a timeout can kill whatever the child started too.
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => RunError::NotFound,
        _ => RunError::Spawn(error),
    })?;
    let deadline = Instant::now() + timeout;
    #[cfg(windows)]
    let cancelled = AtomicBool::new(false);
    thread::scope(|scope| {
        let receiver = capture_pipe(
            scope,
            child.stdout.take().expect("stdout is piped"),
            max_stdout,
            #[cfg(windows)]
            &cancelled,
        );
        let stderr_receiver = max_stderr.map(|limit| {
            capture_pipe(
                scope,
                child.stderr.take().expect("stderr is piped"),
                limit,
                #[cfg(windows)]
                &cancelled,
            )
        });
        let result = (|| {
            // Don't reap the child until both pipes close: a descendant can keep either
            // pipe open after its parent exits, and the unreaped pid pins the group id.
            let stdout = receive_pipe(receiver, deadline, max_stdout)?;
            let stderr = match stderr_receiver {
                Some(receiver) => receive_pipe(receiver, deadline, max_stderr.unwrap())?,
                None => Vec::new(),
            };
            let status = wait_until(&mut child, deadline)?;
            Ok(Captured {
                status,
                stdout,
                stderr,
            })
        })();
        if result.is_err() {
            #[cfg(windows)]
            cancelled.store(true, Ordering::Relaxed);
            // On Unix the unreaped child pins the group id until the kill, closing
            // inherited write ends before the scope joins both pipe readers. Windows
            // readers also observe cancellation if taskkill cannot find the descendants.
            kill_tree(&mut child);
            let _ = child.wait();
        }
        result
    })
}

#[cfg(not(windows))]
fn capture_pipe<'scope>(
    scope: &'scope thread::Scope<'scope, '_>,
    pipe: impl Read + Send + 'static,
    limit: usize,
) -> mpsc::Receiver<io::Result<Vec<u8>>> {
    let (sender, receiver) = mpsc::channel();
    // Drain both pipes concurrently so neither can stall the child.
    scope.spawn(move || {
        let mut bytes = Vec::new();
        let result = pipe
            .take((limit as u64).saturating_add(1))
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = sender.send(result);
    });
    receiver
}

#[cfg(windows)]
fn capture_pipe<'scope>(
    scope: &'scope thread::Scope<'scope, '_>,
    mut pipe: impl Read + std::os::windows::io::AsRawHandle + Send + 'static,
    limit: usize,
    cancelled: &'scope AtomicBool,
) -> mpsc::Receiver<io::Result<Vec<u8>>> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn PeekNamedPipe(
            pipe: *mut std::ffi::c_void,
            buffer: *mut std::ffi::c_void,
            buffer_size: u32,
            bytes_read: *mut u32,
            bytes_available: *mut u32,
            bytes_left: *mut u32,
        ) -> i32;
    }

    let (sender, receiver) = mpsc::channel();
    scope.spawn(move || {
        let result = (|| {
            let mut bytes = Vec::new();
            let max_bytes = (limit as u64).saturating_add(1);
            loop {
                if cancelled.load(Ordering::Relaxed) {
                    return Err(io::ErrorKind::Interrupted.into());
                }
                let mut available = 0;
                // SAFETY: `pipe` owns this handle for the entire call, and only the
                // valid `available` out pointer is non-null. Peeking never consumes data.
                let success = unsafe {
                    PeekNamedPipe(
                        pipe.as_raw_handle(),
                        std::ptr::null_mut(),
                        0,
                        std::ptr::null_mut(),
                        &mut available,
                        std::ptr::null_mut(),
                    )
                };
                if success == 0 {
                    let error = io::Error::last_os_error();
                    // ERROR_BROKEN_PIPE: every inherited write handle has closed.
                    return if error.raw_os_error() == Some(109) {
                        Ok(bytes)
                    } else {
                        Err(error)
                    };
                }
                if available == 0 {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
                // There is no other reader, so consuming at most the peeked byte count
                // cannot block. In particular, never wait for EOF on an inherited pipe:
                // taskkill cannot find descendants whose direct parent already exited.
                let remaining = max_bytes - bytes.len() as u64;
                let read = pipe
                    .by_ref()
                    .take(u64::from(available).min(remaining))
                    .read_to_end(&mut bytes)?;
                if read == 0 || bytes.len() > limit {
                    return Ok(bytes);
                }
            }
        })();
        let _ = sender.send(result);
    });
    receiver
}

fn receive_pipe(
    receiver: mpsc::Receiver<io::Result<Vec<u8>>>,
    deadline: Instant,
    limit: usize,
) -> Result<Vec<u8>, RunError> {
    // A grandchild can hold a pipe open after the child exits, so bound the read too.
    let remaining = deadline.saturating_duration_since(Instant::now());
    let bytes = receiver
        .recv_timeout(remaining)
        .map_err(|_| RunError::TimedOut)?
        .map_err(RunError::Io)?;
    if bytes.len() > limit {
        return Err(RunError::OutputTooLarge);
    }
    Ok(bytes)
}

fn wait_until(child: &mut Child, deadline: Instant) -> Result<ExitStatus, RunError> {
    let mut pause = Duration::from_millis(1);
    loop {
        if let Some(status) = child.try_wait().map_err(RunError::Io)? {
            return Ok(status);
        }
        let now = Instant::now();
        if now >= deadline {
            return Err(RunError::TimedOut);
        }
        thread::sleep(pause.min(deadline - now));
        pause = (pause * 2).min(Duration::from_millis(25));
    }
}

/// Kills `child` and the processes it started: its process group, which `run_captured`
/// created for it.
#[cfg(unix)]
fn kill_tree(child: &mut Child) {
    let Ok(group) = i32::try_from(child.id()) else {
        let _ = child.kill();
        return;
    };
    // SAFETY: kill only sends a signal. The group id is the child's pid, which can't be
    // reused until `child.wait()` reaps it, so it can't name an unrelated group.
    if unsafe { libc::kill(-group, libc::SIGKILL) } != 0 {
        let _ = child.kill();
    }
}

/// Kills `child` and its descendants with `taskkill /T`, then the child itself in case
/// taskkill couldn't run.
#[cfg(windows)]
fn kill_tree(child: &mut Child) {
    let _ = Command::new("taskkill")
        .args(["/T", "/F", "/PID"])
        .arg(child.id().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
}

#[cfg(not(any(unix, windows)))]
fn kill_tree(child: &mut Child) {
    let _ = child.kill();
}

/// Starts `program` detached from the caller, with all standard streams closed, and doesn't
/// wait for it. Hooks use this so their caller never waits on delivery.
pub fn spawn_detached(program: &Path, args: &[&OsStr]) -> io::Result<()> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    detach(&mut command);
    command.spawn().map(drop)
}

#[cfg(unix)]
fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    // A new process group keeps signals aimed at the hook's caller away from the child.
    command.process_group(0);
}

#[cfg(windows)]
fn detach(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS);
}

#[cfg(not(any(unix, windows)))]
fn detach(_command: &mut Command) {}

/// Spawns `oceans <args>` from the running binary in the background.
pub fn spawn_self(args: &[&OsStr]) -> io::Result<()> {
    spawn_detached(&std::env::current_exe()?, args)
}

/// The first executable called `name` on `PATH`.
pub fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .filter(|directory| directory.is_absolute())
        .find_map(|directory| executable_in(&directory, name))
}

#[cfg(unix)]
fn executable_in(directory: &Path, name: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let candidate = directory.join(name);
    let metadata = fs::metadata(&candidate).ok()?;
    (metadata.is_file() && metadata.permissions().mode() & 0o111 != 0).then_some(candidate)
}

#[cfg(not(unix))]
fn executable_in(directory: &Path, name: &str) -> Option<PathBuf> {
    ["exe", "cmd", "bat"].iter().find_map(|extension| {
        let candidate = directory.join(format!("{name}.{extension}"));
        candidate.is_file().then_some(candidate)
    })
}

/// The path hooks should run this binary from. A package manager's `PATH` entry, often a
/// symlink, stays valid across upgrades, so it wins when it resolves to this binary.
pub fn oceans_binary() -> anyhow::Result<PathBuf> {
    let exe = std::env::current_exe().context("could not find the oceans binary")?;
    let resolved = fs::canonicalize(&exe).unwrap_or_else(|_| exe.clone());
    let on_path = find_on_path("oceans")
        .filter(|candidate| fs::canonicalize(candidate).is_ok_and(|path| path == resolved));
    Ok(on_path.unwrap_or(exe))
}

/// Quotes `path` as one word for the shell that git-ai and Claude Code run hooks with.
pub fn shell_word(path: &Path) -> anyhow::Result<String> {
    let text = path
        .to_str()
        .context("the oceans binary path is not valid UTF-8")?;
    Ok(quote_word(text))
}

#[cfg(not(windows))]
fn quote_word(text: &str) -> String {
    let plain = !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"/._-+:@%=,".contains(&byte));
    if plain {
        text.to_owned()
    } else {
        format!("'{}'", text.replace('\'', r"'\''"))
    }
}

#[cfg(windows)]
fn quote_word(text: &str) -> String {
    if text.contains([' ', '&', '(', ')', '^', '%', '!']) {
        format!("\"{text}\"")
    } else {
        text.to_owned()
    }
}

/// A command run through the platform shell: `sh -c` on Unix, `cmd /C` on Windows.
pub fn shell_command(script: &str) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut command = Command::new("cmd");
        command.arg("/C").raw_arg(script);
        command
    }
    #[cfg(not(windows))]
    {
        let mut command = Command::new("sh");
        command.arg("-c").arg(script);
        command
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn captured_stderr_preserves_failure_details_and_is_bounded() {
        let output = run_captured_with_stderr(
            &mut shell_command(
                "printf 'partial'; printf 'GraphQL: node limit exceeded\\n' >&2; exit 1",
            ),
            Duration::from_secs(5),
            64,
            64,
        )
        .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(output.stdout, b"partial");
        assert_eq!(output.stderr, b"GraphQL: node limit exceeded\n");
        assert!(matches!(
            run_captured_with_stderr(
                &mut shell_command("head -c 100 /dev/zero >&2"),
                Duration::from_secs(5),
                64,
                10,
            ),
            Err(RunError::OutputTooLarge)
        ));
    }

    #[test]
    fn captured_output_is_bounded_in_size_and_time() {
        let output = run_captured(
            &mut shell_command("printf 'hello\\n'"),
            Duration::from_secs(5),
            64,
        )
        .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"hello\n");
        assert!(matches!(
            run_captured(
                &mut shell_command("head -c 100 /dev/zero"),
                Duration::from_secs(5),
                10
            ),
            Err(RunError::OutputTooLarge)
        ));
        let started = Instant::now();
        assert!(matches!(
            run_captured(
                &mut shell_command("exec sleep 5"),
                Duration::from_millis(200),
                10
            ),
            Err(RunError::TimedOut)
        ));
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(matches!(
            run_captured(
                &mut Command::new("/nonexistent/oceans-test-binary"),
                Duration::from_secs(1),
                10
            ),
            Err(RunError::NotFound)
        ));
    }

    #[test]
    fn captured_output_accepts_exact_limits() {
        let output = run_captured_with_stderr(
            &mut shell_command("printf '1234'; printf '5678' >&2"),
            Duration::from_secs(5),
            4,
            4,
        )
        .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"1234");
        assert_eq!(output.stderr, b"5678");
    }

    #[test]
    fn stdout_timeout_cleans_up_after_child_exit() {
        assert_exited_child_cleanup(false, false);
    }

    #[test]
    fn stderr_timeout_cleans_up_after_child_exit() {
        assert_exited_child_cleanup(true, false);
    }

    #[test]
    fn oversized_output_cleans_up_after_child_exit() {
        assert_exited_child_cleanup(false, true);
        assert_exited_child_cleanup(true, true);
    }

    fn assert_exited_child_cleanup(stderr: bool, oversized: bool) {
        let directory = tempfile::tempdir().unwrap();
        let pid_file = directory.path().join("pid");
        let redirect = if stderr { ">/dev/null" } else { "2>/dev/null" };
        let output = match (stderr, oversized) {
            (true, true) => "printf '12345' >&2",
            (false, true) => "printf '12345'",
            (_, false) => ":",
        };
        // The shell exits without waiting, leaving only one pipe's write end in sleep.
        let script = format!(
            "sleep 30 {redirect} & echo $! > {}; {output}",
            quote_word(pid_file.to_str().unwrap())
        );
        let started = Instant::now();
        let result = run_captured_with_stderr(
            &mut shell_command(&script),
            Duration::from_millis(200),
            4,
            4,
        );
        let elapsed = started.elapsed();
        let pid: i32 = fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let gone = (0..200).any(|_| {
            // SAFETY: signal 0 only checks whether the process exists.
            let gone = unsafe { libc::kill(pid, 0) } != 0
                && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
            if !gone {
                thread::sleep(Duration::from_millis(10));
            }
            gone
        });
        assert!(gone, "process {pid} survived capture cleanup");
        assert!(elapsed < Duration::from_secs(3));
        if oversized {
            assert!(matches!(result, Err(RunError::OutputTooLarge)));
        } else {
            assert!(matches!(result, Err(RunError::TimedOut)));
        }
    }

    #[test]
    fn a_timeout_kills_what_the_command_started() {
        let directory = tempfile::tempdir().unwrap();
        let pid_file = directory.path().join("pid");
        let script = format!(
            "sleep 30 & echo $! > {}; wait",
            quote_word(pid_file.to_str().unwrap())
        );
        assert!(matches!(
            run_captured(&mut shell_command(&script), Duration::from_millis(200), 10),
            Err(RunError::TimedOut)
        ));
        let pid: i32 = fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        // The orphaned grandchild is reaped by init shortly after the kill.
        let gone = (0..200).any(|_| {
            // SAFETY: signal 0 only checks whether the process exists.
            let gone = unsafe { libc::kill(pid, 0) } != 0
                && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
            if !gone {
                thread::sleep(Duration::from_millis(10));
            }
            gone
        });
        assert!(gone, "process {pid} survived the timeout");
    }

    #[test]
    fn shell_words_quote_only_when_needed() {
        assert_eq!(quote_word("/usr/local/bin/oceans"), "/usr/local/bin/oceans");
        assert_eq!(
            quote_word("/Users/a b/bin/oceans"),
            "'/Users/a b/bin/oceans'"
        );
        assert_eq!(quote_word("/tmp/it's/oceans"), r"'/tmp/it'\''s/oceans'");
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn cancelled_readers_do_not_wait_for_pipe_eof() {
        // A cmd builtin waits for stdin while retaining both output handles.
        let mut child = Command::new("cmd")
            .args(["/C", "set /p input="])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let cancelled = AtomicBool::new(false);
        let started = Instant::now();
        let (stdout, stderr) = thread::scope(|scope| {
            let stdout = capture_pipe(scope, child.stdout.take().unwrap(), 4, &cancelled);
            let stderr = capture_pipe(scope, child.stderr.take().unwrap(), 4, &cancelled);
            thread::sleep(Duration::from_millis(50));
            cancelled.store(true, Ordering::Relaxed);
            let deadline = Instant::now() + Duration::from_secs(1);
            (
                receive_pipe(stdout, deadline, 4),
                receive_pipe(stderr, deadline, 4),
            )
        });
        let elapsed = started.elapsed();
        let _ = child.kill();
        let _ = child.wait();
        for result in [stdout, stderr] {
            assert!(matches!(
                result,
                Err(RunError::Io(error)) if error.kind() == io::ErrorKind::Interrupted
            ));
        }
        assert!(elapsed < Duration::from_secs(2));
    }
}
