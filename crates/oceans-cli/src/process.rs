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
}

/// Runs `command` with stdin and stderr closed and captures at most `max_stdout` bytes of
/// stdout. The child, and on Unix and Windows everything it started, is killed once `timeout`
/// passes.
pub fn run_captured(
    command: &mut Command,
    timeout: Duration,
    max_stdout: usize,
) -> Result<Captured, RunError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
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
    let stdout = child.stdout.take().expect("stdout is piped");
    let (sender, receiver) = mpsc::channel();
    // A reader thread keeps a full pipe from stalling the child while this thread waits.
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take(max_stdout as u64 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = sender.send(result);
    });
    let deadline = Instant::now() + timeout;
    let status = wait_until(&mut child, deadline)?;
    // A grandchild can hold the pipe open after the child exits, so the read is bounded too.
    let remaining = deadline
        .saturating_duration_since(Instant::now())
        .max(Duration::from_millis(100));
    let stdout = receiver
        .recv_timeout(remaining)
        .map_err(|_| RunError::TimedOut)?
        .map_err(RunError::Io)?;
    if stdout.len() > max_stdout {
        return Err(RunError::OutputTooLarge);
    }
    Ok(Captured { status, stdout })
}

fn wait_until(child: &mut Child, deadline: Instant) -> Result<ExitStatus, RunError> {
    let mut pause = Duration::from_millis(1);
    loop {
        if let Some(status) = child.try_wait().map_err(RunError::Io)? {
            return Ok(status);
        }
        let now = Instant::now();
        if now >= deadline {
            kill_tree(child);
            let _ = child.wait();
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
