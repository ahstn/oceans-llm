//! `oceans hook prs`: merged pull requests and their commits from the local `gh`, so the
//! gateway never needs a GitHub token (ADR "Pull request merges"). The repository comes from
//! the same remote git-ai reports, so pull request commits join the notes.

use std::{
    path::Path,
    process::Command,
    time::{Duration, SystemTime},
};

use serde::Deserialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{
    payload::{Payload, PullRequest, PullRequestCommit, PullRequestsPayload, is_commit_sha},
    queue::EnqueueReport,
    repo::git_context,
    state::State,
};
use crate::process::{RunError, run_captured};

const GH_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_GH_OUTPUT: usize = 64 * 1024 * 1024;
/// The gateway's text limits.
const MAX_TEXT_BYTES: usize = 1024;
const MAX_REF_BYTES: usize = 256;
const MAX_URL_BYTES: usize = 2048;
/// Longer pull requests keep their first commits; the gateway accepts up to 1,000.
const MAX_COMMITS_PER_PULL_REQUEST: usize = 250;
/// Entries stay well under the gateway's 2 MiB batch limit and 100 pull requests per entry.
const MAX_PAYLOAD_BYTES: usize = 1024 * 1024;
const MAX_PULL_REQUESTS_PER_PAYLOAD: usize = 100;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhPullRequest {
    number: u64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    merged_at: Option<String>,
    #[serde(default)]
    base_ref_name: String,
    #[serde(default)]
    head_ref_name: String,
    merge_commit: Option<GhCommitRef>,
    #[serde(default)]
    commits: Vec<GhCommit>,
}

#[derive(Deserialize)]
struct GhCommitRef {
    oid: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhCommit {
    oid: String,
    #[serde(default)]
    message_headline: String,
    authored_date: Option<String>,
    #[serde(default)]
    authors: Vec<GhAuthor>,
}

#[derive(Deserialize)]
struct GhAuthor {
    #[serde(default)]
    email: Option<String>,
}

pub enum SyncOutcome {
    /// Synced within the throttle window.
    Fresh,
    /// `gh` is missing, unauthenticated or failed; nothing was queued.
    Unavailable(String),
    /// The directory isn't a repository with a usable remote.
    NoRepository,
    Queued {
        pull_requests: usize,
        report: EnqueueReport,
    },
}

/// Fetches up to `limit` merged pull requests for the repository in `directory` and queues
/// them. With `if_stale`, does nothing when the last sync for this repository is newer.
pub fn sync(
    state: &State,
    directory: &Path,
    limit: u32,
    if_stale: Option<Duration>,
) -> anyhow::Result<SyncOutcome> {
    let Some(repository) = git_context(directory).repository else {
        return Ok(SyncOutcome::NoRepository);
    };
    let now = SystemTime::now();
    if if_stale.is_some_and(|window| !state.pr_sync_due(&repository, window, now)) {
        return Ok(SyncOutcome::Fresh);
    }
    // Marked first, so sessions starting together run `gh` once.
    state.mark_pr_sync(&repository, now)?;
    let output = match run_gh(directory, &repository, limit) {
        Ok(output) => output,
        Err(reason) => return Ok(SyncOutcome::Unavailable(reason)),
    };
    let fetched: Vec<GhPullRequest> = serde_json::from_slice(&output)
        .map_err(|_| anyhow::anyhow!("gh printed pull requests in an unexpected shape"))?;
    let pull_requests: Vec<PullRequest> = fetched.into_iter().filter_map(pull_request).collect();
    let count = pull_requests.len();
    let payloads = chunk(&repository, pull_requests);
    let report = state.enqueue(payloads)?;
    Ok(SyncOutcome::Queued {
        pull_requests: count,
        report,
    })
}

/// Lists `repository`'s merged pull requests. The repository is passed explicitly, so neither
/// `GH_REPO` nor a fork's gh default repository can stamp another repository's pull requests
/// with this one's name.
fn run_gh(directory: &Path, repository: &str, limit: u32) -> Result<Vec<u8>, String> {
    let mut command = Command::new("gh");
    command
        .current_dir(directory)
        .args([
            "pr", "list", "--repo", repository, "--state", "merged", "--limit",
        ])
        .arg(limit.to_string())
        .args([
            "--json",
            "number,title,url,mergedAt,baseRefName,headRefName,mergeCommit,commits",
        ])
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .env("NO_COLOR", "1")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE");
    match run_captured(&mut command, GH_TIMEOUT, MAX_GH_OUTPUT) {
        Ok(output) if output.status.success() => Ok(output.stdout),
        Ok(output) => Err(format!(
            "gh pr list failed ({}); check gh auth status in this repository",
            output.status
        )),
        Err(RunError::NotFound) => Err("gh is not installed".into()),
        Err(error) => Err(format!("gh pr list: {error}")),
    }
}

fn pull_request(fetched: GhPullRequest) -> Option<PullRequest> {
    let merged_at = fetched.merged_at.filter(|time| is_rfc3339(time))?;
    let url = fetched
        .url
        .starts_with("https://")
        .then_some(fetched.url)
        .filter(|url| url.len() <= MAX_URL_BYTES && !url.contains('@'))?;
    let commits = fetched
        .commits
        .into_iter()
        .filter(|commit| is_commit_sha(&commit.oid))
        .take(MAX_COMMITS_PER_PULL_REQUEST)
        .map(|commit| PullRequestCommit {
            sha: commit.oid,
            subject: display_text(&commit.message_headline, MAX_TEXT_BYTES),
            authored_at: commit.authored_date.filter(|time| is_rfc3339(time)),
            author_email: commit
                .authors
                .into_iter()
                .next()
                .and_then(|author| author.email)
                .filter(|email| !email.is_empty() && email.len() <= MAX_TEXT_BYTES),
        })
        .collect();
    Some(PullRequest {
        number: fetched.number,
        title: display_text(&fetched.title, MAX_TEXT_BYTES),
        url,
        base_ref: display_text(&fetched.base_ref_name, MAX_REF_BYTES),
        head_ref: display_text(&fetched.head_ref_name, MAX_REF_BYTES),
        merge_commit_sha: fetched
            .merge_commit
            .map(|commit| commit.oid)
            .filter(|sha| is_commit_sha(sha)),
        merged_at,
        commits,
    })
    .filter(|pull_request| pull_request.number > 0)
}

/// Splits pull requests into entries of at most 1 MiB and 100 pull requests each.
fn chunk(repository: &str, pull_requests: Vec<PullRequest>) -> Vec<Payload> {
    let mut payloads = Vec::new();
    let mut current: Vec<PullRequest> = Vec::new();
    let mut size = 0;
    for pull_request in pull_requests {
        let bytes = serde_json::to_vec(&pull_request).map_or(0, |bytes| bytes.len());
        if !current.is_empty()
            && (size + bytes > MAX_PAYLOAD_BYTES || current.len() == MAX_PULL_REQUESTS_PER_PAYLOAD)
        {
            payloads.push(payload(repository, std::mem::take(&mut current)));
            size = 0;
        }
        size += bytes;
        current.push(pull_request);
    }
    if !current.is_empty() {
        payloads.push(payload(repository, current));
    }
    payloads
}

fn payload(repository: &str, pull_requests: Vec<PullRequest>) -> Payload {
    Payload::PullRequests(PullRequestsPayload {
        repository: repository.to_owned(),
        pull_requests,
    })
}

fn is_rfc3339(value: &str) -> bool {
    OffsetDateTime::parse(value, &Rfc3339).is_ok()
}

/// Display text the gateway accepts: control characters become spaces and the text is cut
/// to `max_bytes` on a character boundary.
fn display_text(value: &str, max_bytes: usize) -> String {
    let mut text: String = value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if text.len() > max_bytes {
        let mut cut = max_bytes;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA_1: &str = "1111111111111111111111111111111111111111";
    const SHA_2: &str = "2222222222222222222222222222222222222222";

    fn fetched() -> Vec<GhPullRequest> {
        serde_json::from_value(serde_json::json!([
            {
                "number": 12,
                "title": "Add widgets\u{7}",
                "url": "https://github.com/acme/widgets/pull/12",
                "mergedAt": "2026-11-10T12:00:00Z",
                "baseRefName": "main",
                "headRefName": "feat/widgets",
                "mergeCommit": {"oid": SHA_1},
                "commits": [{
                    "oid": SHA_2,
                    "messageHeadline": "Add widgets",
                    "messageBody": "",
                    "authoredDate": "2026-11-09T08:00:00Z",
                    "committedDate": "2026-11-09T08:00:00Z",
                    "authors": [{"email": "a@example.com", "id": "1", "login": "a", "name": "A"}],
                }, {"oid": "short", "authors": []}],
            },
            {"number": 13, "title": "Open", "url": "https://github.com/acme/widgets/pull/13", "mergedAt": null, "mergeCommit": null, "commits": []},
            {"number": 14, "title": "Squash", "url": "https://github.com/acme/widgets/pull/14", "mergedAt": "2026-11-11T12:00:00Z", "mergeCommit": null,
             "commits": [{"oid": SHA_1, "messageHeadline": "x", "authoredDate": "yesterday", "authors": [{"email": ""}]}]},
        ]))
        .unwrap()
    }

    #[test]
    fn merged_pull_requests_map_to_the_wire_shape() {
        let pull_requests: Vec<_> = fetched().into_iter().filter_map(pull_request).collect();
        assert_eq!(pull_requests.len(), 2);
        let first = &pull_requests[0];
        assert_eq!(first.title, "Add widgets ");
        assert_eq!(first.merge_commit_sha.as_deref(), Some(SHA_1));
        assert_eq!(first.commits.len(), 1);
        assert_eq!(
            first.commits[0].author_email.as_deref(),
            Some("a@example.com")
        );
        assert_eq!(
            first.commits[0].authored_at.as_deref(),
            Some("2026-11-09T08:00:00Z")
        );
        let second = &pull_requests[1];
        assert_eq!(second.merge_commit_sha, None);
        assert_eq!(second.commits[0].authored_at, None);
        assert_eq!(second.commits[0].author_email, None);
    }

    #[test]
    fn payloads_split_by_size_and_count() {
        let template = fetched().into_iter().find_map(pull_request).unwrap();
        let many: Vec<_> = (1..=250)
            .map(|number| PullRequest {
                number,
                ..template.clone()
            })
            .collect();
        let payloads = chunk("github.com/acme/widgets", many);
        let sizes: Vec<_> = payloads
            .iter()
            .map(|payload| match payload {
                Payload::PullRequests(prs) => prs.pull_requests.len(),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(sizes, [100, 100, 50]);

        let large = PullRequest {
            title: "t".repeat(1024),
            commits: vec![template.commits[0].clone(); 250],
            ..template
        };
        let payloads = chunk("github.com/acme/widgets", vec![large; 40]);
        assert!(payloads.len() > 1);
        for payload in &payloads {
            assert!(serde_json::to_vec(payload).unwrap().len() <= MAX_PAYLOAD_BYTES + 64 * 1024);
        }
    }

    #[test]
    fn display_text_is_cut_on_character_boundaries() {
        assert_eq!(display_text("a\nb", 10), "a b");
        assert_eq!(display_text("ééé", 5), "éé");
    }
}
