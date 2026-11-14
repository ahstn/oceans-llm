//! Git AI attribution: the `oceans hook` commands. See
//! `docs/adr/2026-11-01-git-ai-attribution-via-oceans-hooks.md`.
//!
//! Hooks (`notes`, `claude`, `omp`, `prs` and background `send`) never fail their caller and
//! never write to stdout: Claude Code adds a `SessionStart` hook's stdout to the conversation.
//! Problems go to stderr and the hook exits 0.

mod hooks;
mod install;
pub mod ownership;
mod payload;
mod prs;
mod queue;
mod repo;
mod sender;
pub mod state;

use std::{
    ffi::OsStr,
    io::{self, Write},
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
};

use anyhow::{Context, anyhow, bail};

use self::{
    hooks::QueuedSession,
    install::Change,
    payload::SessionEvent,
    prs::SyncOutcome,
    queue::{EntrySummary, QueueListing},
    sender::{SendOptions, SendReport},
    state::State,
};
use crate::{
    cli::{HookCommand, InstallArgs, PrsArgs, QueueArgs},
    client::Client,
    config::{self, Env, Overrides},
    process::{self, shell_word},
};

pub fn run(command: HookCommand, overrides: Overrides<'_>, json: bool) -> anyhow::Result<()> {
    let env = Env::Process;
    match command {
        HookCommand::Notes => hook(|| notes(&env)),
        HookCommand::Claude => hook(|| {
            let state = State::from_env(&env)?;
            after_session(hooks::claude(&state, io::stdin().lock())?);
            Ok(())
        }),
        HookCommand::Omp => hook(|| {
            let state = State::from_env(&env)?;
            after_session(hooks::omp(&state, io::stdin().lock())?);
            Ok(())
        }),
        HookCommand::Prs(args) => hook(|| sync_pull_requests(&env, &args)),
        HookCommand::Send(args) if args.foreground => send_foreground(overrides, &env, json),
        HookCommand::Send(_) => hook(|| match send(overrides, &env)?.stopped {
            Some(reason) => bail!("delivery stopped: {reason}"),
            None => Ok(()),
        }),
        HookCommand::Queue(args) => queue(&args, &env, json),
        HookCommand::Install(args) => crate::block_on(install(&args, overrides, &env)),
    }
}

/// Runs a hook body. Errors and panics are reported on stderr; the hook still succeeds.
fn hook(body: impl FnOnce() -> anyhow::Result<()>) -> anyhow::Result<()> {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(Ok(())) => {}
        Ok(Err(error)) => eprintln!("oceans: {error:#}"),
        Err(_) => eprintln!("oceans: the hook stopped unexpectedly"),
    }
    Ok(())
}

/// Starts `oceans <args>` in the background. A failed start only delays delivery.
fn spawn(args: &[&OsStr]) {
    if let Err(error) = process::spawn_self(args) {
        eprintln!("oceans: could not start oceans in the background: {error}");
    }
}

fn spawn_sender() {
    spawn(&[OsStr::new("hook"), OsStr::new("send")]);
}

fn notes(env: &Env) -> anyhow::Result<()> {
    let state = State::from_env(env)?;
    let report = hooks::notes(&state, io::stdin().lock())?;
    if report.dropped > 0 {
        eprintln!(
            "oceans: the attribution queue is full; dropped {} entries",
            report.dropped
        );
    }
    if report.queued + report.unsequenced > 0 {
        spawn_sender();
    }
    Ok(())
}

/// Starts a throttled pull request sync when a session starts in a repository, then delivery.
fn after_session(queued: Option<QueuedSession>) {
    let Some(session) = queued else {
        return;
    };
    if session.event == SessionEvent::Start && session.has_repository {
        spawn(&[
            OsStr::new("hook"),
            OsStr::new("prs"),
            OsStr::new("--repo-dir"),
            session.directory.as_os_str(),
            OsStr::new("--if-stale"),
            OsStr::new("1h"),
        ]);
    }
    spawn_sender();
}

fn sync_pull_requests(env: &Env, args: &PrsArgs) -> anyhow::Result<()> {
    let state = State::from_env(env)?;
    let directory = match &args.repo_dir {
        Some(directory) => directory.clone(),
        None => std::env::current_dir()?,
    };
    match prs::sync(&state, &directory, args.limit, args.if_stale)? {
        SyncOutcome::Fresh => {}
        SyncOutcome::NoRepository => eprintln!(
            "oceans: {} has no git remote that names a repository; nothing to sync",
            directory.display()
        ),
        SyncOutcome::Unavailable(reason) => {
            eprintln!("oceans: skipped pull request sync: {reason}")
        }
        SyncOutcome::Queued {
            pull_requests,
            report,
        } => {
            eprintln!("oceans: queued {pull_requests} merged pull requests");
            if report.queued + report.unsequenced > 0 {
                spawn_sender();
            }
        }
    }
    Ok(())
}

fn send(overrides: Overrides<'_>, env: &Env) -> anyhow::Result<SendReport> {
    let state = State::from_env(env)?;
    let connect = || config::resolve_connection(&overrides, env, config::KEY_COMMAND_TIMEOUT);
    crate::block_on(sender::send(&state, connect, &SendOptions::default()))
}

fn send_foreground(overrides: Overrides<'_>, env: &Env, json: bool) -> anyhow::Result<()> {
    let report = send(overrides, env)?;
    if json {
        let value = serde_json::json!({
            "delivered": report.delivered,
            "quarantined": report.quarantined,
            "remaining": report.remaining,
            "already_running": report.already_running,
            "stopped": report.stopped,
        });
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else if report.already_running {
        println!("Another oceans hook send is delivering the queue.");
    } else {
        println!(
            "Delivered {}, quarantined {}, {} still queued.",
            report.delivered, report.quarantined, report.remaining
        );
    }
    match report.stopped {
        Some(reason) => bail!("delivery stopped: {reason}"),
        None => Ok(()),
    }
}

fn queue(args: &QueueArgs, env: &Env, json: bool) -> anyhow::Result<()> {
    let state = State::from_env(env)?;
    if args.discard {
        let count = state.discard_quarantine()?;
        println!("Deleted {count} quarantined entries.");
        return Ok(());
    }
    if args.resend {
        let count = state.resend_quarantine()?;
        if count > 0 {
            spawn_sender();
        }
        println!("Moved {count} quarantined entries back to the queue.");
        return Ok(());
    }
    let listing = state.listing()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&listing)?);
    } else {
        print_listing(&listing, &mut io::stdout().lock())?;
    }
    Ok(())
}

fn print_listing(listing: &QueueListing, out: &mut impl Write) -> io::Result<()> {
    let row = |summary: &EntrySummary| {
        let commit = summary
            .commit
            .as_deref()
            .map_or("-", |sha| &sha[..sha.len().min(12)]);
        let mut row = format!(
            "  {:<14} {:<40} {:<12}",
            summary.kind,
            summary.repository.as_deref().unwrap_or("-"),
            commit
        );
        if let Some(reason) = &summary.reason {
            row.push(' ');
            row.push_str(reason);
        }
        row.trim_end().to_owned()
    };
    writeln!(out, "Queued: {}", listing.queued.len())?;
    for summary in &listing.queued {
        writeln!(out, "{}", row(summary))?;
    }
    writeln!(out, "Quarantined: {}", listing.quarantined.len())?;
    for summary in &listing.quarantined {
        writeln!(out, "{}", row(summary))?;
    }
    writeln!(out, "Dropped: {}", listing.dropped)?;
    if !listing.quarantined.is_empty() {
        writeln!(
            out,
            "Run oceans hook queue --resend to retry quarantined entries, or --discard to delete them."
        )?;
    }
    Ok(())
}

/// Seeds the ownership cache with a fresh lookup, then installs each requested hook. Every
/// target is attempted; the command fails if any of them did.
async fn install(args: &InstallArgs, overrides: Overrides<'_>, env: &Env) -> anyhow::Result<()> {
    let connection = config::resolve_connection(&overrides, env, config::KEY_COMMAND_TIMEOUT)?;
    let client = Client::new(connection.url.clone(), &connection.api_key)?;
    let owner = ownership::lookup(&client)
        .await
        .map_err(|error| anyhow!("could not check the API key: {error}"))?;
    State::from_env(env)?.record_ownership(
        connection.url.as_str(),
        &ownership::key_fingerprint(&connection.api_key),
        &owner,
    )?;
    println!(
        "Attribution goes to {} {} on {}.",
        owner.owner_kind.replace('_', " "),
        owner.owner_id,
        connection.url
    );
    let oceans = process::oceans_binary()?;
    let mut failed = 0;
    let mut report = |target: &str, result: anyhow::Result<(Change, String)>| match result {
        Ok((change, location)) => println!("{target}: {} {location}", describe(change)),
        Err(error) => {
            failed += 1;
            eprintln!("{target}: {error:#}");
        }
    };
    if args.git_ai {
        report("git-ai", install_git_ai(&oceans));
    }
    if args.claude {
        report("Claude Code", install_claude(&oceans, env));
    }
    if args.omp {
        report("OMP", install_omp(&oceans, env));
        println!("{OMP_SESSION_HEADER_HINT}");
    }
    if failed > 0 {
        bail!("{failed} hook target(s) failed to install");
    }
    Ok(())
}

fn describe(change: Change) -> &'static str {
    match change {
        Change::Added => "added the hook to",
        Change::Updated => "updated the hook in",
        Change::Unchanged => "hook already in",
    }
}

/// The extension keeps `OCEANS_SESSION_ID` current; OMP only sends it once the user's Oceans
/// provider names it as a header, which `models.yml` holds and the CLI doesn't edit.
const OMP_SESSION_HEADER_HINT: &str = "OMP: to join requests to commits, add this to your Oceans \
provider in OMP's models.yml, only on machines with this extension:\n    headers:\n      \
session_id: OCEANS_SESSION_ID";

fn install_git_ai(oceans: &Path) -> anyhow::Result<(Change, String)> {
    let change = install::git_ai(&shell_word(oceans)?)?;
    Ok((
        change,
        "git-ai config git_ai_hooks.post_notes_updated".into(),
    ))
}

fn install_claude(oceans: &Path, env: &Env) -> anyhow::Result<(Change, String)> {
    let path = install::claude_settings_path(env)
        .context("could not find Claude Code's settings; set HOME or CLAUDE_CONFIG_DIR")?;
    let command = format!("{} hook claude", shell_word(oceans)?);
    Ok((
        install::claude(&path, &command)?,
        path.display().to_string(),
    ))
}

fn install_omp(oceans: &Path, env: &Env) -> anyhow::Result<(Change, String)> {
    let path = install::omp_extension_path(env)
        .context("could not find OMP's agent directory; set HOME or PI_CODING_AGENT_DIR")?;
    let oceans = oceans
        .to_str()
        .context("the oceans binary path is not valid UTF-8")?;
    let git_ai = process::find_on_path("git-ai");
    let git_ai = git_ai.as_deref().and_then(Path::to_str).unwrap_or("git-ai");
    let contents = install::render_omp_extension(oceans, git_ai);
    Ok((install::omp(&path, &contents)?, path.display().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_listings_print_short_commits_and_reasons() {
        let listing = QueueListing {
            queued: vec![EntrySummary {
                kind: "session",
                repository: None,
                commit: None,
                reason: None,
            }],
            quarantined: vec![EntrySummary {
                kind: "note",
                repository: Some("github.com/acme/widgets".into()),
                commit: Some("f6fb7a1c0d6f5e4b3a2918273645546372819a0b".into()),
                reason: Some("oversized_note".into()),
            }],
            dropped: 2,
        };
        let mut out = Vec::new();
        print_listing(&listing, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("Queued: 1\n  session"), "{text}");
        assert!(text.contains("github.com/acme/widgets"), "{text}");
        assert!(text.contains("f6fb7a1c0d6f oversized_note"), "{text}");
        assert!(!text.contains("f6fb7a1c0d6f5"), "{text}");
        assert!(text.contains("Dropped: 2"), "{text}");
    }
}
