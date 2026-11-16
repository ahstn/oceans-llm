//! `oceans hook prs`: recently updated pull requests, open, closed or merged, with their
//! commits, from the local `gh`, so the gateway never needs a GitHub token (ADR "Pull request
//! merges"). The repository comes from the same remote git-ai reports, so pull request commits
//! join the notes. Reviews, descriptions and commit messages are reduced here to counts, times
//! and revert links; reviewer logins and description text never leave this machine.

use std::{
    path::Path,
    process::Command,
    time::{Duration, SystemTime},
};

use serde::Deserialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{
    payload::{
        Payload, PullRequest, PullRequestCommit, PullRequestLifecycle, PullRequestsPayload,
        RevertLink, is_commit_sha,
    },
    queue::EnqueueReport,
    repo::git_context,
    reverts::{self, Candidate},
    state::State,
};
use crate::process::{RunError, run_captured_with_stderr};

const GH_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_GH_OUTPUT: usize = 64 * 1024 * 1024;
const MAX_GH_STDERR: usize = 64 * 1024;
// A listed PR selects 100 reviews and a merged one 100 commits with one author each, so a page
// stays far below GitHub's 500,000-node GraphQL limit.
const GH_PAGE_SIZE: u32 = 49;
// Recently updated first, so a PR that was reviewed, merged or closed since the last sync is
// fetched again and its stored state moves on. Only the lifecycle and reviews: the description
// and commits are read for merged pull requests alone, by `MERGED_QUERY`.
const PR_QUERY: &str = r#"
query($owner: String!, $name: String!, $pageSize: Int!, $cursor: String) {
  repository(owner: $owner, name: $name) {
    pullRequests(first: $pageSize, after: $cursor, states: [OPEN, CLOSED, MERGED],
                 orderBy: {field: UPDATED_AT, direction: DESC}) {
      nodes {
        id number title url state isDraft createdAt updatedAt closedAt mergedAt baseRefName
        headRefName additions deletions changedFiles
        author { __typename login }
        comments { totalCount }
        reviews(first: 100) {
          totalCount
          nodes { state submittedAt author { __typename login } }
        }
      }
      pageInfo { hasNextPage endCursor }
    }
  }
}
"#;
const PR_PAGE_FILTER: &str = r#".data.repository.pullRequests | {
  pullRequests: [.nodes[] | .reviewTotal = .reviews.totalCount | .reviews = .reviews.nodes
    | .commentCount = .comments.totalCount | del(.comments)],
  pageInfo
}"#;
/// What only a merged pull request needs: its commits and merge commit join notes and
/// fingerprint the change, and its description and commit messages hold revert links. The
/// gateway reads open and closed pull requests for their lifecycle alone.
const MERGED_QUERY: &str = r#"
query($ids: [ID!]!) {
  nodes(ids: $ids) {
    ... on PullRequest {
      id body mergeCommit { oid }
      commits(first: 100) {
        nodes { commit {
          oid messageHeadline messageBody authoredDate authors(first: 1) { nodes { email } }
        } }
      }
    }
  }
}
"#;
// Keep the same commit shape as `gh pr list --json commits`.
const MERGED_FILTER: &str = r#"[.data.nodes[] | select(. != null)
  | .commits = [.commits.nodes[].commit | .authors = .authors.nodes]]"#;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhMergedDetails {
    id: String,
    #[serde(default)]
    body: Option<String>,
    merge_commit: Option<GhCommitRef>,
    #[serde(default)]
    commits: Vec<GhCommit>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhPage {
    pull_requests: Vec<GhPullRequest>,
    page_info: GhPageInfo,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhPageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}
/// The gateway's text limits.
const MAX_TEXT_BYTES: usize = 1024;
const MAX_REF_BYTES: usize = 256;
const MAX_URL_BYTES: usize = 2048;
/// Longer pull requests keep their first commits; the gateway accepts up to 1,000.
const MAX_COMMITS_PER_PULL_REQUEST: usize = 250;
/// Entries stay well under the gateway's 2 MiB batch limit and 100 pull requests per entry.
const MAX_PAYLOAD_BYTES: usize = 1024 * 1024;
const MAX_PULL_REQUESTS_PER_PAYLOAD: usize = 100;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhPullRequest {
    /// GitHub's node id, which `MERGED_QUERY` reads a merged pull request's details by.
    #[serde(default)]
    id: String,
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
    /// `OPEN`, `CLOSED` or `MERGED`.
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    is_draft: Option<bool>,
    #[serde(default)]
    created_at: Option<String>,
    /// When GitHub last saw any change: a push, review, comment, merge or close.
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    closed_at: Option<String>,
    /// Read for revert links only; never sent.
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    additions: Option<u64>,
    #[serde(default)]
    deletions: Option<u64>,
    #[serde(default)]
    changed_files: Option<u64>,
    #[serde(default)]
    author: Option<GhActor>,
    #[serde(default)]
    comment_count: Option<u32>,
    #[serde(default)]
    reviews: Vec<GhReview>,
    /// Every review, of which `reviews` holds the first 100.
    #[serde(default)]
    review_total: Option<u64>,
}

#[derive(Clone, Deserialize)]
struct GhActor {
    #[serde(rename = "__typename", default)]
    kind: String,
    #[serde(default)]
    login: String,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhReview {
    #[serde(default)]
    state: String,
    submitted_at: Option<String>,
    author: Option<GhActor>,
}

#[derive(Clone, Deserialize)]
struct GhCommitRef {
    oid: String,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhCommit {
    oid: String,
    #[serde(default)]
    message_headline: String,
    /// Read for `This reverts commit` lines only; never sent.
    #[serde(default)]
    message_body: String,
    authored_date: Option<String>,
    #[serde(default)]
    authors: Vec<GhAuthor>,
}

#[derive(Clone, Deserialize)]
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
    /// Queued this many pull requests; `sync`'s `queued` callback saw the queue's report.
    Queued { pull_requests: usize },
}

/// Fetches up to `limit` merged pull requests for the repository in `directory` and queues
/// them. With `if_stale`, does nothing when the last sync for this repository is newer.
/// `queued` runs as soon as they are queued, before the slower fingerprinting of new merges,
/// so delivery can start without waiting for it.
pub fn sync(
    state: &State,
    directory: &Path,
    limit: u32,
    if_stale: Option<Duration>,
    queued: impl FnOnce(&EnqueueReport),
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
    let fetched = match run_gh(directory, &repository, limit) {
        Ok(output) => output,
        Err(reason) => return Ok(SyncOutcome::Unavailable(reason)),
    };
    let repository_path = repository.split_once('/').map_or("", |(_, path)| path);
    let mut cache = state.change_cache(&repository);
    // Detection reads only fingerprints already cached, so the pull requests are queued before
    // any git runs. A revert whose original is fingerprinted below links on a later sync, which
    // fetches the same recent pull requests again.
    let links = revert_links(&fetched, repository_path, &cache);
    let pull_requests: Vec<PullRequest> = fetched
        .iter()
        .cloned()
        .zip(links)
        .filter_map(|(fetched, link)| pull_request(fetched, link))
        .collect();
    let count = pull_requests.len();
    let payloads = chunk(&repository, pull_requests);
    let report = state.enqueue(payloads)?;
    queued(&report);
    remember_merges(&fetched, directory, &mut cache);
    reverts::trim(&mut cache);
    // The cache only speeds up later matches, so a failed write must not lose the sync.
    let _ = state.save_change_cache(&repository, &cache);
    Ok(SyncOutcome::Queued {
        pull_requests: count,
    })
}

/// Pages the repository explicitly, ignoring `GH_REPO` and gh's default repository, then reads
/// the merged pull requests' details.
fn run_gh(directory: &Path, repository: &str, limit: u32) -> Result<Vec<GhPullRequest>, String> {
    let (host, path) = repository.split_once('/').ok_or("invalid repository")?;
    let (owner, name) = path.split_once('/').ok_or("invalid repository")?;
    let mut fetched = fetch_pages(limit, |page_size, cursor| {
        let mut fields = vec![
            format!("owner={owner}"),
            format!("name={name}"),
            format!("pageSize={page_size}"),
        ];
        if let Some(cursor) = cursor {
            fields.push(format!("cursor={cursor}"));
        }
        gh_graphql(directory, host, PR_QUERY, &fields, PR_PAGE_FILTER)
    })?;
    fill_merged(&mut fetched, |ids| {
        let fields: Vec<String> = ids.iter().map(|id| format!("ids[]={id}")).collect();
        gh_graphql(directory, host, MERGED_QUERY, &fields, MERGED_FILTER)
    })?;
    Ok(fetched)
}

/// Runs one `gh api graphql` query. Each field is a string variable (`pageSize` an integer),
/// and `key[]=value` fields build a list.
fn gh_graphql<T: serde::de::DeserializeOwned>(
    directory: &Path,
    host: &str,
    query: &str,
    fields: &[String],
    filter: &str,
) -> Result<T, String> {
    let mut command = Command::new("gh");
    command
        .current_dir(directory)
        .args(["api", "graphql", "--hostname", host, "-f"])
        .arg(format!("query={query}"));
    for field in fields {
        // `-F` types a number; every other value stays a string.
        let flag = if field.starts_with("pageSize=") {
            "-F"
        } else {
            "-f"
        };
        command.arg(flag).arg(field);
    }
    command
        .args(["--jq", filter])
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .env("NO_COLOR", "1")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE");
    match run_captured_with_stderr(&mut command, GH_TIMEOUT, MAX_GH_OUTPUT, MAX_GH_STDERR) {
        Ok(output) if output.status.success() => serde_json::from_slice(&output.stdout)
            .map_err(|_| "gh printed pull requests in an unexpected shape".into()),
        Ok(output) => {
            let detail = String::from_utf8_lossy(&output.stderr);
            let detail = detail.trim();
            Err(if detail.is_empty() {
                format!("gh api graphql failed ({})", output.status)
            } else {
                format!("gh api graphql failed ({}): {detail}", output.status)
            })
        }
        Err(RunError::NotFound) => Err("gh is not installed".into()),
        Err(error) => Err(format!("gh api graphql: {error}")),
    }
}

/// Fills each merged pull request's description, merge commit and commits, a page of ids at a
/// time. Open and closed ones keep none: nothing reads them.
fn fill_merged(
    fetched: &mut [GhPullRequest],
    mut fetch: impl FnMut(&[&str]) -> Result<Vec<GhMergedDetails>, String>,
) -> Result<(), String> {
    let ids: Vec<String> = fetched
        .iter()
        .filter(|pr| pr.merged_at.is_some() && !pr.id.is_empty())
        .map(|pr| pr.id.clone())
        .collect();
    let mut details = std::collections::HashMap::new();
    for chunk in ids.chunks(GH_PAGE_SIZE as usize) {
        let chunk: Vec<&str> = chunk.iter().map(String::as_str).collect();
        for detail in fetch(&chunk)? {
            details.insert(detail.id.clone(), detail);
        }
    }
    for pr in fetched.iter_mut() {
        if let Some(detail) = details.remove(&pr.id) {
            pr.body = detail.body;
            pr.merge_commit = detail.merge_commit;
            pr.commits = detail.commits;
        }
    }
    Ok(())
}

fn fetch_pages(
    limit: u32,
    mut fetch: impl FnMut(u32, Option<&str>) -> Result<GhPage, String>,
) -> Result<Vec<GhPullRequest>, String> {
    let mut fetched = Vec::new();
    let mut cursor: Option<String> = None;
    while fetched.len() < limit as usize {
        let page_size = GH_PAGE_SIZE.min(limit - fetched.len() as u32);
        let page = fetch(page_size, cursor.as_deref())?;
        fetched.extend(page.pull_requests.into_iter().take(page_size as usize));
        if !page.page_info.has_next_page {
            break;
        }
        let next = page
            .page_info
            .end_cursor
            .ok_or("gh omitted the next page cursor")?;
        if cursor.as_ref() == Some(&next) {
            return Err("gh repeated the pull request page cursor".into());
        }
        cursor = Some(next);
    }
    Ok(fetched)
}

/// Each fetched pull request's revert link, from its text and the fingerprints already cached.
/// It runs no git, so it never delays queuing the sync.
fn revert_links(
    fetched: &[GhPullRequest],
    repository_path: &str,
    cache: &reverts::ChangeCache,
) -> Vec<Option<RevertLink>> {
    fetched
        .iter()
        .map(|fetched| reverts::detect(&candidate(fetched), repository_path, cache))
        .collect()
}

/// Fingerprints the fetched merged pull requests not cached yet, within the per-sync count and
/// time budget, so later syncs can match a revert to its original in either order.
fn remember_merges(fetched: &[GhPullRequest], directory: &Path, cache: &mut reverts::ChangeCache) {
    let mut budget = reverts::FingerprintBudget::new(reverts::MAX_NEW_FINGERPRINTS);
    for fetched in fetched {
        reverts::remember(&candidate(fetched), directory, cache, &mut budget);
    }
}

fn candidate(fetched: &GhPullRequest) -> Candidate<'_> {
    let merged_at = fetched.merged_at.as_deref().filter(|time| is_rfc3339(time));
    Candidate {
        number: fetched.number,
        title: &fetched.title,
        body: fetched.body.as_deref().unwrap_or_default(),
        head_ref: &fetched.head_ref_name,
        merge_commit_sha: merged_at.and(
            fetched
                .merge_commit
                .as_ref()
                .map(|commit| commit.oid.as_str())
                .filter(|sha| is_commit_sha(sha)),
        ),
        merged_at,
        commit_bodies: fetched
            .commits
            .iter()
            .map(|commit| commit.message_body.as_str())
            .collect(),
        commit_subjects: fetched
            .commits
            .iter()
            .map(|commit| commit.message_headline.as_str())
            .collect(),
        commit_shas: fetched
            .commits
            .iter()
            .map(|commit| commit.oid.as_str())
            .collect(),
    }
}

/// Counts and times from the pull request's reviews and comments. Reviews by bots and by the
/// author are left out; logins are compared here and never sent.
fn lifecycle(fetched: &GhPullRequest) -> PullRequestLifecycle {
    let author = fetched
        .author
        .as_ref()
        .map(|author| author.login.as_str())
        .unwrap_or_default();
    let human_reviews: Vec<&GhReview> = fetched
        .reviews
        .iter()
        .filter(|review| review.state != "PENDING")
        .filter(|review| {
            review.author.as_ref().is_some_and(|reviewer| {
                reviewer.kind == "User" && !reviewer.login.is_empty() && reviewer.login != author
            })
        })
        .collect();
    let count = |state: &str| {
        let count = human_reviews
            .iter()
            .filter(|review| review.state == state)
            .count();
        u32::try_from(count).unwrap_or(u32::MAX)
    };
    let mut reviewers: Vec<&str> = human_reviews
        .iter()
        .filter_map(|review| {
            review
                .author
                .as_ref()
                .map(|reviewer| reviewer.login.as_str())
        })
        .collect();
    reviewers.sort_unstable();
    reviewers.dedup();
    // Past the first page the counts would read low, and a page of bot reviews would read as
    // no review at all, so they are sent as unknown. GitHub does not document the order of
    // the page either, so the earliest review on it is not known to be the first.
    let complete = fetched
        .review_total
        .is_none_or(|total| total <= fetched.reviews.len() as u64);
    let known = |count: u32| complete.then_some(count);
    let state = fetched.state.as_deref().map(|state| match state {
        "MERGED" => "merged",
        "CLOSED" => "closed",
        _ => "open",
    });
    PullRequestLifecycle {
        state: state.map(str::to_owned),
        is_draft: fetched.is_draft,
        opened_at: fetched.created_at.clone().filter(|time| is_rfc3339(time)),
        updated_at: fetched.updated_at.clone().filter(|time| is_rfc3339(time)),
        closed_at: fetched.closed_at.clone().filter(|time| is_rfc3339(time)),
        first_review_at: human_reviews
            .iter()
            .filter_map(|review| review.submitted_at.as_deref())
            .filter(|time| is_rfc3339(time))
            .min_by_key(|time| OffsetDateTime::parse(time, &Rfc3339).ok())
            .filter(|_| complete)
            .map(str::to_owned),
        review_count: known(u32::try_from(human_reviews.len()).unwrap_or(u32::MAX)),
        approval_count: known(count("APPROVED")),
        changes_requested_count: known(count("CHANGES_REQUESTED")),
        reviewer_count: known(u32::try_from(reviewers.len()).unwrap_or(u32::MAX)),
        comment_count: fetched.comment_count,
        additions: fetched.additions,
        deletions: fetched.deletions,
        changed_files: fetched.changed_files,
    }
}

fn pull_request(fetched: GhPullRequest, reverts: Option<RevertLink>) -> Option<PullRequest> {
    let lifecycle = lifecycle(&fetched);
    let merged_at = fetched.merged_at.filter(|time| is_rfc3339(time));
    // A merged pull request without a merge time is malformed; open and closed ones have none.
    if merged_at.is_none()
        && lifecycle
            .state
            .as_deref()
            .is_none_or(|state| state == "merged")
    {
        return None;
    }
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
        lifecycle,
        reverts,
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

    #[test]
    fn pagination_preserves_the_requested_limit_and_each_pull_request() {
        for available in [0, 17, 123, 1000] {
            for limit in [0, 1, 30, 49, 50, 200, 1000] {
                let mut offset = 0;
                let result = fetch_pages(limit, |size, cursor| {
                    assert!((1..=49).contains(&size));
                    assert_eq!(cursor, (offset > 0).then(|| offset.to_string()).as_deref());
                    let end = (offset + size).min(available);
                    let pull_requests = (offset..end)
                        .map(|number| {
                            let mut pr = fetched().remove(0);
                            pr.number = u64::from(number + 1);
                            pr
                        })
                        .collect();
                    offset = end;
                    Ok(GhPage {
                        pull_requests,
                        page_info: GhPageInfo {
                            has_next_page: end < available,
                            end_cursor: Some(end.to_string()),
                        },
                    })
                })
                .unwrap();
                assert_eq!(
                    result.into_iter().map(|pr| pr.number).collect::<Vec<_>>(),
                    (1..=u64::from(limit.min(available))).collect::<Vec<_>>()
                );
            }
        }
    }

    #[test]
    fn a_later_page_failure_returns_the_error_not_partial_results() {
        let mut calls = 0;
        let result = fetch_pages(200, |_, _| {
            calls += 1;
            if calls == 2 {
                return Err("GraphQL: API rate limit exceeded".into());
            }
            Ok(GhPage {
                pull_requests: fetched(),
                page_info: GhPageInfo {
                    has_next_page: true,
                    end_cursor: Some("next".into()),
                },
            })
        });
        assert_eq!(
            result.err().as_deref(),
            Some("GraphQL: API rate limit exceeded")
        );
        assert_eq!(calls, 2);
    }

    #[test]
    fn pagination_rejects_missing_or_repeated_cursors() {
        for cursor in [None, Some("same".to_owned())] {
            let result = fetch_pages(200, |_, _| {
                Ok(GhPage {
                    pull_requests: fetched(),
                    page_info: GhPageInfo {
                        has_next_page: true,
                        end_cursor: cursor.clone(),
                    },
                })
            });
            assert!(result.is_err());
        }
    }

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
    fn only_merged_pull_requests_read_their_details() {
        let mut listed: Vec<GhPullRequest> = serde_json::from_value(serde_json::json!([
            {"id": "PR_merged", "number": 1, "mergedAt": "2026-11-10T12:00:00Z"},
            {"id": "PR_open", "number": 2, "state": "OPEN", "mergedAt": null},
            {"id": "PR_merged_2", "number": 3, "mergedAt": "2026-11-11T12:00:00Z"},
        ]))
        .unwrap();
        let mut asked = Vec::new();
        fill_merged(&mut listed, |ids| {
            asked.extend(ids.iter().map(|id| (*id).to_string()));
            Ok(serde_json::from_value(serde_json::json!([
                {"id": "PR_merged_2", "body": "Reverts acme/widgets#1", "mergeCommit": {"oid": SHA_1},
                 "commits": [{"oid": SHA_2, "authors": []}]},
                {"id": "PR_merged", "body": null, "mergeCommit": null, "commits": []},
            ]))
            .unwrap())
        })
        .unwrap();
        assert_eq!(asked, ["PR_merged", "PR_merged_2"]);
        assert_eq!(listed[2].body.as_deref(), Some("Reverts acme/widgets#1"));
        assert_eq!(
            listed[2].merge_commit.as_ref().map(|c| c.oid.as_str()),
            Some(SHA_1)
        );
        assert_eq!(listed[2].commits.len(), 1);
        assert!(listed[1].commits.is_empty() && listed[1].body.is_none());
    }

    #[test]
    fn merged_pull_requests_map_to_the_wire_shape() {
        let pull_requests: Vec<_> = fetched()
            .into_iter()
            .filter_map(|pr| pull_request(pr, None))
            .collect();
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
        let template = fetched()
            .into_iter()
            .find_map(|pr| pull_request(pr, None))
            .unwrap();
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

    fn lifecycle_fixture() -> Vec<GhPullRequest> {
        serde_json::from_value(serde_json::json!([
            {
                "number": 20, "title": "Open work", "url": "https://github.com/acme/widgets/pull/20",
                "state": "OPEN", "isDraft": true, "createdAt": "2026-11-12T09:00:00Z",
                "closedAt": null, "mergedAt": null, "mergeCommit": null,
                "body": "Private description", "additions": 40, "deletions": 3, "changedFiles": 2,
                "author": {"__typename": "User", "login": "dev"}, "commentCount": 4,
                "reviews": [
                    {"state": "COMMENTED", "submittedAt": "2026-11-12T10:00:00Z", "author": {"__typename": "Bot", "login": "coderabbitai"}},
                    {"state": "COMMENTED", "submittedAt": "2026-11-12T10:30:00Z", "author": {"__typename": "User", "login": "dev"}},
                    {"state": "CHANGES_REQUESTED", "submittedAt": "2026-11-12T12:00:00Z", "author": {"__typename": "User", "login": "rev1"}},
                    {"state": "APPROVED", "submittedAt": "2026-11-12T11:00:00Z", "author": {"__typename": "User", "login": "rev2"}},
                    {"state": "APPROVED", "submittedAt": "2026-11-13T11:00:00Z", "author": {"__typename": "User", "login": "rev1"}},
                    {"state": "PENDING", "submittedAt": null, "author": {"__typename": "User", "login": "rev3"}}
                ],
                "commits": []
            },
            {
                "number": 21, "title": "Abandoned", "url": "https://github.com/acme/widgets/pull/21",
                "state": "CLOSED", "createdAt": "2026-11-01T09:00:00Z", "closedAt": "2026-11-02T09:00:00Z",
                "mergedAt": null, "mergeCommit": null, "commits": [], "reviews": []
            }
        ]))
        .unwrap()
    }

    #[test]
    fn open_and_closed_pull_requests_carry_their_lifecycle() {
        let prs: Vec<_> = lifecycle_fixture()
            .into_iter()
            .filter_map(|pr| pull_request(pr, None))
            .collect();
        assert_eq!(prs.len(), 2);
        let open = &prs[0];
        assert_eq!(open.merged_at, None);
        assert_eq!(open.lifecycle.state.as_deref(), Some("open"));
        assert_eq!(open.lifecycle.is_draft, Some(true));
        // Bot and self reviews are left out, and so is the pending one.
        assert_eq!(open.lifecycle.review_count, Some(3));
        assert_eq!(
            open.lifecycle.first_review_at.as_deref(),
            Some("2026-11-12T11:00:00Z")
        );
        assert_eq!(open.lifecycle.approval_count, Some(2));
        assert_eq!(open.lifecycle.changes_requested_count, Some(1));
        assert_eq!(open.lifecycle.reviewer_count, Some(2));
        assert_eq!(open.lifecycle.comment_count, Some(4));
        assert_eq!(
            (open.lifecycle.additions, open.lifecycle.deletions),
            (Some(40), Some(3))
        );
        let closed = &prs[1];
        assert_eq!(closed.lifecycle.state.as_deref(), Some("closed"));
        assert_eq!(
            closed.lifecycle.closed_at.as_deref(),
            Some("2026-11-02T09:00:00Z")
        );
        // Nothing that identifies a reviewer, nor the description, is in the payload.
        let wire = serde_json::to_string(&prs).unwrap();
        for private in ["coderabbitai", "rev1", "rev2", "dev", "Private description"] {
            assert!(
                !wire.contains(&format!("\"{private}\"")) && !wire.contains(private),
                "{private} leaked"
            );
        }
    }

    #[test]
    fn review_counts_past_the_first_page_are_unknown() {
        let mut fetched = lifecycle_fixture();
        fetched[0].review_total = Some(150);
        let pr = pull_request(fetched.remove(0), None).unwrap();
        assert_eq!(pr.lifecycle.review_count, None);
        assert_eq!(pr.lifecycle.approval_count, None);
        assert_eq!(pr.lifecycle.reviewer_count, None);
        // An earlier review may be on a later page, so the first review time is unknown too.
        assert_eq!(pr.lifecycle.first_review_at, None);

        // A complete page still reports its earliest review.
        let pr = pull_request(lifecycle_fixture().remove(0), None).unwrap();
        assert_eq!(
            pr.lifecycle.first_review_at.as_deref(),
            Some("2026-11-12T11:00:00Z")
        );
    }

    #[test]
    fn a_revert_line_in_a_commit_body_links_the_reverted_commit() {
        let mut fetched = fetched();
        fetched[0].title = "Undo widgets".into();
        // The reverted commit is not one of this pull request's own.
        const SHIPPED: &str = "3333333333333333333333333333333333333333";
        fetched[0].commits[0].message_body = format!("This reverts commit {SHIPPED}.");
        let cache = reverts::ChangeCache::new();
        let links = revert_links(&fetched, "acme/widgets", &cache);
        let link = links[0].as_ref().expect("revert link");
        assert_eq!(link.evidence, "revert_commit");
        assert_eq!(link.commit_shas, [SHIPPED]);
        assert!(links[1].is_none() && links[2].is_none());

        // Reverting the pull request's own commit (SHA_2) is branch cleanup, not a revert.
        fetched[0].commits[0].message_body = format!("This reverts commit {SHA_2}.");
        let links = revert_links(&fetched, "acme/widgets", &cache);
        assert!(links[0].is_none());
    }

    #[test]
    fn display_text_is_cut_on_character_boundaries() {
        assert_eq!(display_text("a\nb", 10), "a b");
        assert_eq!(display_text("ééé", 5), "éé");
    }
}
