//! Revert detection for `oceans hook prs`. Signals are checked strongest first, and only the
//! link (the reverted pull request or commits, and the evidence) leaves this machine:
//!
//! 1. `revert_commit`: a pull request commit carries the `This reverts commit <sha>` line
//!    `git revert` writes, whatever its subject says.
//! 2. `github_revert`: GitHub's revert button (`Reverts owner/repo#N` in the description or a
//!    `revert-N-…` branch), counted only when the title or a commit also says revert, because a
//!    description is editable text.
//! 3. `change_match`: the pull request's change is the exact inverse of an earlier merged pull
//!    request's, compared by a fingerprint of the added and removed lines. This catches reverts
//!    whose messages were rewritten. It needs both merge commits in the local clone, and the
//!    fingerprints stay in a private cache under the state directory.
//! 4. `title`: a `Revert "…"` or `revert:` title and nothing else. A possible revert only.

use std::{
    collections::BTreeMap,
    path::Path,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{payload::RevertLink, repo::git_bytes};

/// Larger diffs are skipped; one this size is rarely an exact revert worth the time.
const MAX_DIFF_BYTES: usize = 32 * 1024 * 1024;
const DIFF_TIMEOUT: Duration = Duration::from_secs(20);
const SHORT_GIT_TIMEOUT: Duration = Duration::from_secs(2);
/// The fingerprint cache keeps this many merge commits per repository, newest first.
pub const MAX_CACHED_CHANGES: usize = 5_000;
const MAX_REVERTED_COMMITS: usize = 40;
/// Context lines around each edit in a fingerprinted diff.
const CONTEXT_LINES: &str = "-U3";

/// What revert detection needs from one fetched pull request.
pub struct Candidate<'a> {
    pub number: u64,
    pub title: &'a str,
    pub body: &'a str,
    pub head_ref: &'a str,
    pub merge_commit_sha: Option<&'a str>,
    pub merged_at: Option<&'a str>,
    pub commit_bodies: Vec<&'a str>,
    /// The pull request's commit subjects, oldest first. A rebase merge copies them, which is
    /// how it is told apart from a squash.
    pub commit_subjects: Vec<&'a str>,
    /// The pull request's own commit SHAs. A `git revert` of one of them undoes work the branch
    /// itself added, not a shipped change, so it is not revert evidence.
    pub commit_shas: Vec<&'a str>,
}

/// `gh` lists at most this many commits per pull request, so a longer rebase merge's range
/// can't be established.
const MAX_LISTED_COMMITS: usize = 100;

/// Fingerprints of a merged pull request's change, forwards and inverted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangePrints {
    pub number: u64,
    pub merged_at: String,
    pub forward: String,
    pub inverse: String,
}

/// Merge commit SHA → fingerprints, for one repository.
pub type ChangeCache = BTreeMap<String, ChangePrints>;

/// At most this many uncached merge commits are fingerprinted per sync, in fetch order (most
/// recently updated first). Each costs a few git processes; the cache keeps them, and every
/// sync fetches the same recent pull requests, so a first sync's backlog fills over the next
/// few.
pub const MAX_NEW_FINGERPRINTS: usize = 200;
/// And for at most this long per sync.
const FINGERPRINT_TIME_BUDGET: Duration = Duration::from_secs(30);

/// How much new fingerprinting one sync may still do.
pub struct FingerprintBudget {
    remaining: usize,
    deadline: Instant,
}

impl FingerprintBudget {
    pub fn new(count: usize) -> Self {
        Self {
            remaining: count,
            deadline: Instant::now() + FINGERPRINT_TIME_BUDGET,
        }
    }

    fn take(&mut self) -> bool {
        if self.remaining == 0 || Instant::now() >= self.deadline {
            return false;
        }
        self.remaining -= 1;
        true
    }

    /// Returns a slot taken for a merge that couldn't be fingerprinted, such as one missing from
    /// the local clone, so a run of those doesn't starve later merges every sync. The time limit
    /// still bounds the attempts.
    fn refund(&mut self) {
        self.remaining += 1;
    }
}

/// Adds a merged candidate's fingerprints to `cache` when they are not there yet and the budget
/// allows. Syncs run this after queuing, so a revert links to an original fingerprinted here on
/// the next sync, which fetches the same recent pull requests again.
pub fn remember(
    candidate: &Candidate<'_>,
    directory: &Path,
    cache: &mut ChangeCache,
    budget: &mut FingerprintBudget,
) {
    let Some((sha, merged_at)) = candidate.merge_commit_sha.zip(candidate.merged_at) else {
        return;
    };
    if !cache.contains_key(sha)
        && budget.take()
        && prints_for(directory, cache, candidate, sha, merged_at).is_none()
    {
        budget.refund();
    }
}

/// The strongest revert signal for `candidate`. The commit, GitHub and title signals come from
/// the fetched text; change matching reads only fingerprints `remember` already cached, so it
/// runs no git.
pub fn detect(
    candidate: &Candidate<'_>,
    repository_path: &str,
    cache: &ChangeCache,
) -> Option<RevertLink> {
    let number = github_revert_number(candidate.body, candidate.head_ref, repository_path)
        .filter(|reverted| *reverted != candidate.number);
    // A revert of a commit the same pull request added is branch cleanup, not a revert.
    let commit_shas: Vec<String> = reverted_commits(&candidate.commit_bodies)
        .into_iter()
        .filter(|reverted| {
            !candidate
                .commit_shas
                .iter()
                .any(|own| own.to_ascii_lowercase().starts_with(reverted.as_str()))
        })
        .collect();
    let titled = is_revert_title(candidate.title);
    if !commit_shas.is_empty() {
        // The description or branch may name another pull request than the one the reverted
        // commits came from, so the gateway resolves the target from the SHAs alone.
        return Some(RevertLink {
            pull_request_number: None,
            commit_shas,
            evidence: "revert_commit".into(),
        });
    }
    if number.is_some() && titled {
        return Some(RevertLink {
            pull_request_number: number,
            commit_shas: Vec::new(),
            evidence: "github_revert".into(),
        });
    }
    let prints = candidate.merge_commit_sha.and_then(|sha| cache.get(sha));
    if let Some((sha, prints)) = prints.and_then(|prints| matching_change(cache, prints)) {
        return Some(RevertLink {
            pull_request_number: Some(prints.number),
            commit_shas: vec![sha],
            evidence: "change_match".into(),
        });
    }
    titled.then(|| RevertLink {
        pull_request_number: number,
        commit_shas: Vec::new(),
        evidence: "title".into(),
    })
}

/// The first `MAX_REVERTED_COMMITS` SHAs named by `This reverts commit <sha>` lines, in order
/// and without repeats.
pub fn reverted_commits(bodies: &[&str]) -> Vec<String> {
    const MARKER: &str = "This reverts commit ";
    let mut shas: Vec<String> = Vec::new();
    for body in bodies {
        for line in body.lines() {
            let Some(start) = line.find(MARKER) else {
                continue;
            };
            let sha: String = line[start + MARKER.len()..]
                .chars()
                .take_while(char::is_ascii_hexdigit)
                .collect::<String>()
                .to_ascii_lowercase();
            if (7..=40).contains(&sha.len()) && !shas.contains(&sha) {
                shas.push(sha);
                if shas.len() == MAX_REVERTED_COMMITS {
                    return shas;
                }
            }
        }
    }
    shas
}

/// The pull request GitHub's revert button names: `Reverts owner/repo#N` (this repository
/// only) or `Reverts #N` in the description, else a `revert-N-…` head branch.
pub fn github_revert_number(body: &str, head_ref: &str, repository_path: &str) -> Option<u64> {
    for line in body.lines() {
        let Some(rest) = line.trim().strip_prefix("Reverts ") else {
            continue;
        };
        let rest = rest.trim();
        let Some((target, number)) = rest.rsplit_once('#') else {
            continue;
        };
        let same_repository = target.is_empty() || target.eq_ignore_ascii_case(repository_path);
        let digits: String = number.chars().take_while(char::is_ascii_digit).collect();
        if same_repository && let Ok(number) = digits.parse::<u64>() {
            return Some(number).filter(|number| *number > 0);
        }
    }
    let digits: String = head_ref
        .strip_prefix("revert-")?
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    let rest = &head_ref["revert-".len() + digits.len()..];
    (rest.starts_with('-'))
        .then(|| digits.parse::<u64>().ok())
        .flatten()
        .filter(|number| *number > 0)
}

/// `Revert "…"` as GitHub's revert button writes it, `Revert …`, or a Conventional Commits
/// `revert:`, `revert!:` or `revert(scope):` type, in any case. The gateway's fallback for
/// pull requests without a hook verdict (`report::insights::is_revert_title`) keeps the same
/// rule and test cases; change both together.
pub fn is_revert_title(title: &str) -> bool {
    let lowered = title.trim_start().to_ascii_lowercase();
    ["revert ", "revert\"", "revert:", "revert!:", "revert("]
        .iter()
        .any(|prefix| lowered.starts_with(prefix))
}

/// Computes and caches the fingerprints for `merge_sha` when the pull request's whole change is
/// in the local clone. `None` when nothing was cached.
fn prints_for(
    directory: &Path,
    cache: &mut ChangeCache,
    candidate: &Candidate<'_>,
    merge_sha: &str,
    merged_at: &str,
) -> Option<()> {
    let base = change_base(directory, candidate, merge_sha)?;
    let forward = fingerprint(&diff(directory, &base, merge_sha)?)?;
    let inverse = fingerprint(&diff(directory, merge_sha, &base)?)?;
    let prints = ChangePrints {
        number: candidate.number,
        merged_at: merged_at.to_owned(),
        forward,
        inverse,
    };
    cache.insert(merge_sha.to_owned(), prints);
    Some(())
}

/// The commit the pull request's whole change starts from: the first parent of a merge
/// commit or of a squashed commit. A rebase merge's SHA is only the last of the rebased
/// commits, so when the `N` commits ending at it carry the pull request's `N` subjects in
/// order, the change starts at `merge~N`. `None` when the commit is missing locally or a
/// longer rebase than `gh` lists can't be ruled out.
fn change_base(directory: &Path, candidate: &Candidate<'_>, merge_sha: &str) -> Option<String> {
    let commit = format!("{merge_sha}^{{commit}}");
    git_bytes(
        directory,
        &["cat-file", "-e", &commit],
        SHORT_GIT_TIMEOUT,
        64,
    )?;
    let first_parent = Some(format!("{merge_sha}^1"));
    let subjects = &candidate.commit_subjects;
    if subjects.len() < 2 {
        return first_parent;
    }
    let count = subjects.len().to_string();
    let log = git_bytes(
        directory,
        &[
            "log",
            "--first-parent",
            "--format=%P%x00%s",
            "-n",
            &count,
            merge_sha,
        ],
        SHORT_GIT_TIMEOUT,
        1024 * 1024,
    )?;
    let log = String::from_utf8_lossy(&log);
    let commits: Vec<(usize, &str)> = log
        .lines()
        .filter_map(|line| line.split_once('\0'))
        .map(|(parents, subject)| (parents.split_whitespace().count(), subject.trim()))
        .collect();
    let rebased = commits.len() == subjects.len()
        && commits.iter().all(|(parents, _)| *parents == 1)
        && commits
            .iter()
            .rev()
            .zip(subjects)
            .all(|((_, subject), expected)| *subject == expected.trim());
    if !rebased {
        // A squash or merge commit stands alone. When `gh` listed only the first commits, a
        // single-parent commit may be a rebase it can't see the end of, so it is skipped.
        let partial = subjects.len() >= MAX_LISTED_COMMITS
            && commits.first().is_some_and(|(parents, _)| *parents == 1);
        return if partial { None } else { first_parent };
    }
    let base = format!("{merge_sha}~{count}");
    git_bytes(
        directory,
        &["rev-parse", "--verify", "--quiet", &base],
        SHORT_GIT_TIMEOUT,
        128,
    )?;
    Some(base)
}

/// An earlier merged pull request whose change `prints` exactly undoes.
fn matching_change(cache: &ChangeCache, prints: &ChangePrints) -> Option<(String, ChangePrints)> {
    cache
        .iter()
        .filter(|(_, other)| other.number != prints.number && other.merged_at < prints.merged_at)
        .filter(|(_, other)| other.forward == prints.inverse)
        .max_by(|(_, a), (_, b)| a.merged_at.cmp(&b.merged_at))
        .map(|(sha, other)| (sha.clone(), other.clone()))
}

fn diff(directory: &Path, from: &str, to: &str) -> Option<Vec<u8>> {
    git_bytes(
        directory,
        &[
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-renames",
            "--no-textconv",
            "--full-index",
            // Three lines of context place each edit. Git can give two methods of one `impl` or
            // class the same hunk label, so the label alone can't tell them apart; three lines
            // around the edit usually can, while edits further away leave the match alone.
            CONTEXT_LINES,
            from,
            to,
            "--",
        ],
        DIFF_TIMEOUT,
        MAX_DIFF_BYTES,
    )
}

/// A fingerprint of a `-U3` diff: per file, the path, each hunk's function label and each
/// added, removed or context line exactly as written, combined in path order. The three context
/// lines on each side of an edit and the label git gives the hunk (the nearest function line
/// above it in the old side) place it, so the same lines changed in another function don't
/// match, even when their nearest lines and the hunk label are the same. Both sides of a match are labelled
/// from the same file content, the original's new side and the revert's old side. Line
/// numbers and text blob ids are left out, so an exact inverse matches even after unrelated edits elsewhere; an edit
/// right next to the change breaks the match, and the revert then falls back to its other
/// signals. Whitespace is kept: a revert
/// reproduces the original lines exactly, and whitespace inside a string is content. A binary
/// file has no lines to compare, so it contributes its `index <from>..<to>` blob ids, which keep
/// the change's direction: two unrelated edits to one image never match. File mode changes,
/// such as an executable bit, count with their old and new modes. `None` for an empty
/// diff, which would match every other empty change.
pub fn fingerprint(diff: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(diff);
    let mut files: BTreeMap<String, Sha256> = BTreeMap::new();
    let mut current: Option<String> = None;
    let mut index: Option<String> = None;
    let mut lines = 0usize;
    // Inside a hunk every line is content, even one starting `--- ` or `+++ `, such as a
    // removed `-- x;`; outside one those are the file headers.
    let mut in_hunk = false;
    for line in text.lines() {
        if let Some(header) = line.strip_prefix("diff --git ") {
            current = Some(header.to_owned());
            index = None;
            in_hunk = false;
            files.entry(header.to_owned()).or_default();
            continue;
        }
        let Some(path) = current.as_ref() else {
            continue;
        };
        if let Some(header) = line.strip_prefix("@@") {
            in_hunk = true;
            // `@@ -a,b +c,d @@ label`: the line numbers are dropped and the label kept.
            let label = header
                .split_once("@@")
                .map_or("", |(_, label)| label.trim());
            let hasher = files.entry(path.clone()).or_default();
            hasher.update(b"@");
            hasher.update(label.as_bytes());
            hasher.update(b"\n");
            continue;
        }
        if !in_hunk && (line.starts_with("+++ ") || line.starts_with("--- ")) {
            continue;
        }
        if let Some(blobs) = line.strip_prefix("index ").filter(|_| !in_hunk) {
            // `index <from>..<to> [mode]`; only the blob ids identify a binary change.
            index = blobs.split_whitespace().next().map(str::to_owned);
            continue;
        }
        let binary;
        let (sign, content) = match line.as_bytes().first() {
            Some(b'+') => ('+', &line[1..]),
            Some(b'-') => ('-', &line[1..]),
            Some(b' ') => (' ', &line[1..]),
            // `\ No newline at end of file`, after the line it applies to: restoring a file's
            // text without its final newline isn't an exact revert.
            Some(b'\\') if in_hunk => ('\\', ""),
            _ if line.starts_with("Binary files ") => {
                binary = index.clone().unwrap_or_else(|| line.to_owned());
                ('b', binary.as_str())
            }
            // New or deleted file and mode change markers, which keep their direction: a
            // revert that restores a file's text but not its executable bit doesn't match.
            _ if line.starts_with("new file mode")
                || line.starts_with("deleted file mode")
                || line.starts_with("old mode")
                || line.starts_with("new mode") =>
            {
                ('=', line)
            }
            _ => continue,
        };
        let hasher = files.entry(path.clone()).or_default();
        hasher.update([sign as u8]);
        hasher.update(content.trim_end_matches('\r').as_bytes());
        hasher.update(b"\n");
        lines += 1;
    }
    if lines == 0 {
        return None;
    }
    let mut combined = Sha256::new();
    for (path, hasher) in files {
        combined.update(path.as_bytes());
        combined.update(hasher.finalize());
    }
    Some(format!("{:x}", combined.finalize()))
}

/// Keeps the newest `MAX_CACHED_CHANGES` merge commits.
pub fn trim(cache: &mut ChangeCache) {
    if cache.len() <= MAX_CACHED_CHANGES {
        return;
    }
    let mut by_age: Vec<(String, String)> = cache
        .iter()
        .map(|(sha, prints)| (prints.merged_at.clone(), sha.clone()))
        .collect();
    by_age.sort();
    for (_, sha) in by_age.into_iter().take(cache.len() - MAX_CACHED_CHANGES) {
        cache.remove(&sha);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revert_lines_name_their_commits() {
        let bodies = [
            "This reverts commit 1111111111111111111111111111111111111111.\n",
            "Revert two\n\nThis reverts commit abcdef1, reversing\nchanges made to 2222222.",
            "This reverts commit 1111111111111111111111111111111111111111.",
            "no marker here; This reverts commit xyz",
        ];
        assert_eq!(
            reverted_commits(&bodies),
            ["1111111111111111111111111111111111111111", "abcdef1"]
        );
    }

    #[test]
    fn revert_lines_stop_at_the_commit_limit() {
        let shas: Vec<String> = (0..MAX_REVERTED_COMMITS + 10)
            .map(|index| format!("{index:07x}"))
            .collect();
        // Every SHA named twice, the repeats after the limit is reached.
        let body: String = shas
            .iter()
            .chain(&shas)
            .map(|sha| format!("This reverts commit {sha}.\n"))
            .collect();
        assert_eq!(
            reverted_commits(&[body.as_str()]),
            shas[..MAX_REVERTED_COMMITS]
        );
    }

    #[test]
    fn a_failed_fingerprint_gives_its_slot_back() {
        let mut budget = FingerprintBudget::new(1);
        assert!(budget.take());
        assert!(!budget.take());
        budget.refund();
        assert!(budget.take());
        // A refund doesn't extend the time limit.
        let mut late = FingerprintBudget {
            remaining: 0,
            deadline: Instant::now(),
        };
        late.refund();
        assert!(!late.take());
    }

    #[test]
    fn github_revert_numbers_come_from_the_body_or_the_branch() {
        let repo = "acme/widgets";
        assert_eq!(
            github_revert_number("Reverts acme/widgets#42", "", repo),
            Some(42)
        );
        assert_eq!(
            github_revert_number("Reverts ACME/Widgets#42\n", "", repo),
            Some(42)
        );
        assert_eq!(github_revert_number("Reverts #7", "", repo), Some(7));
        assert_eq!(
            github_revert_number("Reverts other/repo#42", "", repo),
            None
        );
        assert_eq!(github_revert_number("", "revert-42-feat/x", repo), Some(42));
        assert_eq!(github_revert_number("", "revert-42", repo), None);
        assert_eq!(github_revert_number("", "feature/revert-42-x", repo), None);
        assert_eq!(github_revert_number("Mentions #42", "main", repo), None);
    }

    #[test]
    fn revert_titles_cover_git_and_conventional_commits() {
        for title in [
            "Revert \"Add widgets\"",
            "Revert\"Add widgets\"",
            "Revert the cache",
            "REVERT the cache",
            "revert: drop flag",
            "Revert: drop flag",
            "revert!: drop flag",
            "revert(api): x",
            "revert(api)!: x",
        ] {
            assert!(is_revert_title(title), "{title}");
        }
        for title in [
            "Reverting is hard",
            "fix: revert-proof cache",
            "fix: revert detection",
            "Add revert button",
        ] {
            assert!(!is_revert_title(title), "{title}");
        }
    }

    const FORWARD: &str = "diff --git a/src/a.rs b/src/a.rs\nindex 1..2 100644\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -3,0 +4,2 @@\n+let a = 1;\n+let b = 2;\n@@ -9 +11 @@\n-old()\n+new()\n";
    // The same change undone, with different line numbers.
    const INVERSE_OF_REVERT: &str = "diff --git a/src/a.rs b/src/a.rs\nindex 2..1 100644\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -40,0 +41,2 @@\n+let a = 1;\n+let b = 2;\n@@ -90 +92 @@\n-old()\n+new()\n";

    #[test]
    fn fingerprints_ignore_line_numbers_but_not_content_or_whitespace() {
        assert_eq!(
            fingerprint(FORWARD.as_bytes()),
            fingerprint(INVERSE_OF_REVERT.as_bytes())
        );
        // `"a b"` → `"ab"` is not undone by `"ab"` → `"a  b"`.
        let spaced = FORWARD.replace("let a = 1;", "let a =  1;");
        assert_ne!(
            fingerprint(FORWARD.as_bytes()),
            fingerprint(spaced.as_bytes())
        );
        let different = FORWARD.replace("let b = 2;", "let b = 3;");
        assert_ne!(
            fingerprint(FORWARD.as_bytes()),
            fingerprint(different.as_bytes())
        );
        assert_eq!(fingerprint(b""), None);
        assert_eq!(fingerprint(b"diff --git a/x b/x\nindex 1..2\n"), None);
    }

    #[test]
    fn fingerprints_keep_the_hunk_function_label() {
        let hunk = |header: &str| {
            format!(
                "diff --git a/a.rs b/a.rs\nindex 1..2 100644\n--- a/a.rs\n+++ b/a.rs\n{header}\n     let x = 0;\n-    return 1\n+    return 2\n }}\n"
            )
        };
        let first = hunk("@@ -2,3 +2,3 @@ fn first() {");
        // The same lines and context in another function.
        assert_ne!(
            fingerprint(first.as_bytes()),
            fingerprint(hunk("@@ -7,3 +7,3 @@ fn second() {").as_bytes())
        );
        // The same function further down the file.
        assert_eq!(
            fingerprint(first.as_bytes()),
            fingerprint(hunk("@@ -20,3 +20,3 @@ fn first() {").as_bytes())
        );
    }

    #[test]
    fn binary_changes_match_only_their_exact_inverse() {
        let edit = |from: &str, to: &str| {
            format!(
                "diff --git a/logo.png b/logo.png\nindex {from}..{to} 100644\nBinary files a/logo.png and b/logo.png differ\n"
            )
        };
        // A later, unrelated edit of the same image does not undo the first.
        assert_ne!(
            fingerprint(edit("a1", "b2").as_bytes()),
            fingerprint(edit("c3", "d4").as_bytes())
        );
        // The revert's inverse diff runs from the original's old blob to its new one again.
        assert_eq!(
            fingerprint(edit("a1", "b2").as_bytes()),
            fingerprint(edit("a1", "b2").as_bytes())
        );
        assert_ne!(
            fingerprint(edit("a1", "b2").as_bytes()),
            fingerprint(edit("b2", "a1").as_bytes())
        );
    }

    #[test]
    fn hunk_lines_that_look_like_file_headers_still_count() {
        let edit = |from: &str, to: &str| {
            format!(
                "diff --git a/a.js b/a.js\nindex a1..b2 100644\n--- a/a.js\n+++ b/a.js\n@@ -1,2 +1,2 @@\n let x = 0;\n-{from}\n+{to}\n"
            )
        };
        // `-- first;` → `++ first;` is not undone by `-- second;` → `++ first;`.
        assert_ne!(
            fingerprint(edit("-- first;", "++ first;").as_bytes()),
            fingerprint(edit("-- second;", "++ first;").as_bytes())
        );
        assert_eq!(
            fingerprint(edit("-- first;", "++ first;").as_bytes()),
            fingerprint(edit("-- first;", "++ first;").as_bytes())
        );
    }

    #[test]
    fn a_missing_final_newline_must_be_undone_too() {
        let hunk = |body: &str| {
            format!(
                "diff --git a/a.txt b/a.txt\nindex a1..b2 100644\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n{body}"
            )
        };
        // `old\n` → `new\n`, then `new\n` → `old` without a newline: the inverse of the second
        // keeps the marker on `old`, so it doesn't match the first.
        let original = hunk("-old\n+new\n");
        let partial_inverse = hunk("-old\n\\ No newline at end of file\n+new\n");
        assert_ne!(
            fingerprint(original.as_bytes()),
            fingerprint(partial_inverse.as_bytes())
        );
        assert_eq!(
            fingerprint(partial_inverse.as_bytes()),
            fingerprint(partial_inverse.as_bytes())
        );
    }

    #[test]
    fn mode_changes_must_be_undone_too() {
        let edit = |modes: &str| {
            format!(
                "diff --git a/run.sh b/run.sh\n{modes}index a1..b2\n--- a/run.sh\n+++ b/run.sh\n@@ -1 +1 @@\n-echo a\n+echo b\n"
            )
        };
        let forward = edit("old mode 100644\nnew mode 100755\n");
        // The revert's inverse diff carries the same mode change; a text-only revert doesn't.
        assert_eq!(
            fingerprint(forward.as_bytes()),
            fingerprint(edit("old mode 100644\nnew mode 100755\n").as_bytes())
        );
        assert_ne!(
            fingerprint(forward.as_bytes()),
            fingerprint(edit("").as_bytes())
        );
        assert_ne!(
            fingerprint(forward.as_bytes()),
            fingerprint(edit("old mode 100755\nnew mode 100644\n").as_bytes())
        );
    }

    #[test]
    fn the_same_lines_changed_in_another_function_do_not_match() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let write = |first: &str, second: &str| {
            std::fs::write(
                dir.join("a.rs"),
                // Identical lines around each edit, so only the function tells them apart.
                format!(
                    "fn first() {{\n    let x = 0;\n    {first}\n}}\n\nfn second() {{\n    let x = 0;\n    {second}\n}}\n"
                ),
            )
            .unwrap();
        };
        run(dir, &["init", "-q", "-b", "main"]);
        write("return 1", "return 2");
        run(dir, &["add", "."]);
        run(dir, &["commit", "-qm", "base"]);
        write("return 2", "return 2");
        run(dir, &["commit", "-qam", "first returns 2"]);
        let original = run(dir, &["rev-parse", "HEAD"]);
        // `second` goes from 2 to 1: the same `-`/`+` lines inverted, in another function.
        write("return 2", "return 1");
        run(dir, &["commit", "-qam", "second returns 1"]);
        let unrelated = run(dir, &["rev-parse", "HEAD"]);
        // The real revert puts `first` back.
        write("return 1", "return 1");
        run(dir, &["commit", "-qam", "first returns 1 again"]);
        let revert = run(dir, &["rev-parse", "HEAD"]);

        let candidate = |number, sha: &'static str, merged_at| Candidate {
            number,
            title: "Change",
            body: "",
            head_ref: "x",
            merge_commit_sha: Some(sha),
            merged_at: Some(merged_at),
            commit_bodies: vec![],
            commit_subjects: vec![],
            commit_shas: vec![],
        };
        let (original, unrelated, revert): (&'static str, &'static str, &'static str) =
            (original.leak(), unrelated.leak(), revert.leak());
        let mut cache = ChangeCache::new();
        let mut budget = FingerprintBudget::new(MAX_NEW_FINGERPRINTS);
        let prs = [
            candidate(1, original, "2026-11-01T00:00:00Z"),
            candidate(2, unrelated, "2026-11-02T00:00:00Z"),
            candidate(3, revert, "2026-11-03T00:00:00Z"),
        ];
        for pr in &prs {
            remember(pr, dir, &mut cache, &mut budget);
        }
        assert!(detect(&prs[1], "acme/widgets", &cache).is_none());
        let link = detect(&prs[2], "acme/widgets", &cache).expect("change match");
        assert_eq!(link.pull_request_number, Some(1));
    }

    #[test]
    fn methods_sharing_a_hunk_label_do_not_match() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let write = |first: &str, second: &str| {
            std::fs::write(
                dir.join("a.rs"),
                // Git labels hunks in both methods `impl Example {`, and the lines right around
                // each edit are identical; only the wider context tells the methods apart.
                format!(
                    "impl Example {{\n    fn first(&self) {{\n        alpha();\n        beta();\n        let x = 0;\n        {first}\n    }}\n\n    fn second(&self) {{\n        gamma();\n        delta();\n        let x = 0;\n        {second}\n    }}\n}}\n"
                ),
            )
            .unwrap();
        };
        run(dir, &["init", "-q", "-b", "main"]);
        write("old();", "new();");
        run(dir, &["add", "."]);
        run(dir, &["commit", "-qm", "base"]);
        write("new();", "new();");
        run(dir, &["commit", "-qam", "first calls new"]);
        let original = run(dir, &["rev-parse", "HEAD"]);
        // `second` goes from new to old: the same lines inverted, in the other method.
        write("new();", "old();");
        run(dir, &["commit", "-qam", "second calls old"]);
        let unrelated = run(dir, &["rev-parse", "HEAD"]);

        let candidate = |number, sha: &'static str, merged_at| Candidate {
            number,
            title: "Change",
            body: "",
            head_ref: "x",
            merge_commit_sha: Some(sha),
            merged_at: Some(merged_at),
            commit_bodies: vec![],
            commit_subjects: vec![],
            commit_shas: vec![],
        };
        let (original, unrelated): (&'static str, &'static str) =
            (original.leak(), unrelated.leak());
        let mut cache = ChangeCache::new();
        let mut budget = FingerprintBudget::new(MAX_NEW_FINGERPRINTS);
        let prs = [
            candidate(1, original, "2026-11-01T00:00:00Z"),
            candidate(2, unrelated, "2026-11-02T00:00:00Z"),
        ];
        for pr in &prs {
            remember(pr, dir, &mut cache, &mut budget);
        }
        assert_eq!(cache.len(), 2, "both merges are fingerprinted");
        assert!(detect(&prs[1], "acme/widgets", &cache).is_none());
    }

    #[test]
    fn a_change_matches_only_an_earlier_inverse() {
        let mut cache = ChangeCache::new();
        cache.insert(
            "a".repeat(40),
            ChangePrints {
                number: 10,
                merged_at: "2026-11-01T00:00:00Z".into(),
                forward: "F".into(),
                inverse: "I".into(),
            },
        );
        let revert = ChangePrints {
            number: 11,
            merged_at: "2026-11-05T00:00:00Z".into(),
            forward: "I".into(),
            inverse: "F".into(),
        };
        let (sha, original) = matching_change(&cache, &revert).expect("match");
        assert_eq!(
            (sha.as_str(), original.number),
            ("a".repeat(40).as_str(), 10)
        );
        // A change merged before the original cannot revert it.
        let earlier = ChangePrints {
            merged_at: "2026-10-01T00:00:00Z".into(),
            ..revert
        };
        assert!(matching_change(&cache, &earlier).is_none());
    }

    #[test]
    fn a_revert_of_the_pull_requests_own_commit_is_not_a_revert() {
        let own = "2222222222222222222222222222222222222222";
        let cleanup = Candidate {
            number: 9,
            title: "Add search",
            body: "",
            head_ref: "feat/search",
            merge_commit_sha: None,
            merged_at: None,
            commit_bodies: vec!["", "This reverts commit 2222222."],
            commit_subjects: vec!["Add a draft", "Revert \"Add a draft\""],
            commit_shas: vec![own, "3333333333333333333333333333333333333333"],
        };
        assert_eq!(detect(&cleanup, "acme/widgets", &ChangeCache::new()), None);
        // A revert of a commit from elsewhere in the same pull request still counts.
        let shipped = Candidate {
            commit_bodies: vec!["This reverts commit 4444444444444444444444444444444444444444."],
            ..cleanup
        };
        let link = detect(&shipped, "acme/widgets", &ChangeCache::new()).expect("revert");
        assert_eq!(link.evidence, "revert_commit");
        assert_eq!(
            link.commit_shas,
            vec!["4444444444444444444444444444444444444444"]
        );
    }

    #[test]
    fn the_strongest_signal_wins() {
        let cache = ChangeCache::new();
        let base = Candidate {
            number: 50,
            title: "Revert \"Add widgets\"",
            body: "Reverts acme/widgets#42",
            head_ref: "revert-42-feat",
            merge_commit_sha: None,
            merged_at: None,
            commit_bodies: vec!["This reverts commit 1111111111111111111111111111111111111111."],
            commit_subjects: vec![],
            commit_shas: vec![],
        };
        let link = detect(&base, "acme/widgets", &cache).unwrap();
        // The description names #42, but the reverted commit may be from another pull request,
        // so the gateway resolves the target from the SHA.
        assert_eq!(
            (link.evidence.as_str(), link.pull_request_number),
            ("revert_commit", None)
        );
        assert_eq!(
            link.commit_shas,
            ["1111111111111111111111111111111111111111"]
        );
        let button = Candidate {
            commit_bodies: vec![],
            commit_subjects: vec![],
            commit_shas: vec![],
            ..base
        };
        let link = detect(&button, "acme/widgets", &cache).unwrap();
        assert_eq!(link.evidence, "github_revert");
        // A description claim without a revert title or line is not trusted.
        let claim = Candidate {
            title: "Tidy up",
            ..button
        };
        assert!(detect(&claim, "acme/widgets", &cache).is_none());
        let titled = Candidate {
            body: "",
            head_ref: "main",
            title: "Revert the cache",
            ..claim
        };
        let link = detect(&titled, "acme/widgets", &cache).unwrap();
        assert_eq!(
            (link.evidence.as_str(), link.pull_request_number),
            ("title", None)
        );
    }

    fn run(dir: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .output()
            .expect("git");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    #[test]
    fn a_reworded_revert_matches_its_original_through_real_git() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        run(dir, &["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("a.rs"), "fn a() {}\n\nfn keep() {}\n").unwrap();
        run(dir, &["add", "."]);
        run(dir, &["commit", "-qm", "base"]);
        std::fs::write(
            dir.join("a.rs"),
            "fn a() {}\nfn added() {}\n\nfn keep() {}\n",
        )
        .unwrap();
        std::fs::write(dir.join("b.rs"), "fn b() {}\n").unwrap();
        run(dir, &["add", "."]);
        run(dir, &["commit", "-qm", "feature"]);
        let original = run(dir, &["rev-parse", "HEAD"]);
        // An unrelated change in between, so the revert's context differs from the original's.
        std::fs::write(dir.join("c.rs"), "fn c() {}\n").unwrap();
        run(dir, &["add", "."]);
        run(dir, &["commit", "-qm", "other"]);
        run(dir, &["revert", "--no-edit", "HEAD~1"]);
        run(
            dir,
            &["commit", "--amend", "-qm", "Tidy the widgets module"],
        );
        let revert = run(dir, &["rev-parse", "HEAD"]);

        let mut cache = ChangeCache::new();
        let revert_pr = Candidate {
            number: 2,
            title: "Tidy the widgets module",
            body: "",
            head_ref: "tidy",
            merge_commit_sha: Some(&revert),
            merged_at: Some("2026-11-03T00:00:00Z"),
            commit_bodies: vec![],
            commit_subjects: vec![],
            commit_shas: vec![],
        };
        let original_pr = Candidate {
            number: 1,
            title: "Add widgets",
            body: "",
            head_ref: "feat",
            merge_commit_sha: Some(&original),
            merged_at: Some("2026-11-01T00:00:00Z"),
            commit_bodies: vec![],
            commit_subjects: vec![],
            commit_shas: vec![],
        };
        // Fetched newest first: the revert arrives before its original.
        remember(
            &revert_pr,
            dir,
            &mut cache,
            &mut FingerprintBudget::new(MAX_NEW_FINGERPRINTS),
        );
        remember(
            &original_pr,
            dir,
            &mut cache,
            &mut FingerprintBudget::new(MAX_NEW_FINGERPRINTS),
        );
        let link = detect(&revert_pr, "acme/widgets", &cache).expect("change match");
        assert_eq!(link.evidence, "change_match");
        assert_eq!(link.pull_request_number, Some(1));
        assert_eq!(link.commit_shas, std::slice::from_ref(&original));
        assert!(detect(&original_pr, "acme/widgets", &cache).is_none());
    }

    #[test]
    fn a_rebase_merge_is_fingerprinted_across_all_its_commits() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        run(dir, &["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("a.rs"), "fn a() {}\n").unwrap();
        run(dir, &["add", "."]);
        run(dir, &["commit", "-qm", "base"]);
        let base = run(dir, &["rev-parse", "HEAD"]);
        // Rebase-merged: two commits land on main and the merge SHA is the second.
        std::fs::write(dir.join("b.rs"), "fn b() {}\n").unwrap();
        run(dir, &["add", "."]);
        run(dir, &["commit", "-qm", "Add b"]);
        std::fs::write(dir.join("c.rs"), "fn c() {}\n").unwrap();
        run(dir, &["add", "."]);
        run(dir, &["commit", "-qm", "Add c"]);
        let rebased = run(dir, &["rev-parse", "HEAD"]);
        // Squash-merged: two commits on the branch, one commit on main with its own subject.
        std::fs::write(dir.join("d.rs"), "fn d() {}\n").unwrap();
        run(dir, &["add", "."]);
        run(dir, &["commit", "-qm", "Add d (#2)"]);
        let squashed = run(dir, &["rev-parse", "HEAD"]);

        let original = Candidate {
            number: 1,
            title: "Add b and c",
            body: "",
            head_ref: "feat",
            merge_commit_sha: Some(&rebased),
            merged_at: Some("2026-11-01T00:00:00Z"),
            commit_bodies: vec![],
            commit_subjects: vec!["Add b", "Add c"],
            commit_shas: vec![],
        };
        assert_eq!(
            change_base(dir, &original, &rebased),
            Some(format!("{rebased}~2"))
        );
        let squash = Candidate {
            number: 2,
            title: "Add d",
            merge_commit_sha: Some(&squashed),
            commit_bodies: vec![],
            commit_subjects: vec!["Start d", "Finish d"],
            commit_shas: vec![],
            ..original
        };
        assert_eq!(
            change_base(dir, &squash, &squashed),
            Some(format!("{squashed}^1"))
        );
        let partial = Candidate {
            commit_subjects: vec!["Start d"; MAX_LISTED_COMMITS],
            ..squash
        };
        assert_eq!(change_base(dir, &partial, &squashed), None);

        // One squashed revert of the whole rebased change.
        run(dir, &["rm", "-q", "b.rs", "c.rs"]);
        run(dir, &["commit", "-qm", "Drop b and c"]);
        let revert = run(dir, &["rev-parse", "HEAD"]);
        let revert_pr = Candidate {
            number: 3,
            title: "Drop b and c",
            body: "",
            head_ref: "drop",
            merge_commit_sha: Some(&revert),
            merged_at: Some("2026-11-03T00:00:00Z"),
            commit_bodies: vec![],
            commit_subjects: vec!["Drop b and c"],
            commit_shas: vec![],
        };
        let mut cache = ChangeCache::new();
        let mut budget = FingerprintBudget::new(1);
        // A merge missing from the clone caches nothing and gives its slot back.
        let missing = Candidate {
            number: 4,
            merge_commit_sha: Some("5555555555555555555555555555555555555555"),
            commit_bodies: vec![],
            commit_subjects: vec![],
            commit_shas: vec![],
            ..original
        };
        remember(&missing, dir, &mut cache, &mut budget);
        assert!(cache.is_empty());
        remember(&original, dir, &mut cache, &mut budget);
        assert!(cache.contains_key(rebased.as_str()));
        // Out of budget: the revert isn't fingerprinted, so nothing matches yet.
        remember(&revert_pr, dir, &mut cache, &mut budget);
        assert!(detect(&revert_pr, "acme/widgets", &cache).is_none());
        // The next sync fingerprints it; the original is cached and costs nothing.
        let mut budget = FingerprintBudget::new(1);
        remember(&original, dir, &mut cache, &mut budget);
        remember(&revert_pr, dir, &mut cache, &mut budget);
        let link = detect(&revert_pr, "acme/widgets", &cache).expect("change match");
        assert_eq!(
            (link.evidence.as_str(), link.pull_request_number),
            ("change_match", Some(1))
        );
        assert_ne!(base, rebased);
    }
}
