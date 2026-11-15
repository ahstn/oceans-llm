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

use std::{collections::BTreeMap, path::Path, time::Duration};

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

/// What revert detection needs from one fetched pull request.
pub struct Candidate<'a> {
    pub number: u64,
    pub title: &'a str,
    pub body: &'a str,
    pub head_ref: &'a str,
    pub merge_commit_sha: Option<&'a str>,
    pub merged_at: Option<&'a str>,
    pub commit_bodies: Vec<&'a str>,
}

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

/// Adds a merged candidate's fingerprints to `cache`, so a revert fetched in the same batch
/// can match it whichever order the two arrive in.
pub fn remember(candidate: &Candidate<'_>, directory: &Path, cache: &mut ChangeCache) {
    if let Some((sha, merged_at)) = candidate.merge_commit_sha.zip(candidate.merged_at) {
        prints_for(directory, cache, sha, candidate.number, merged_at);
    }
}

/// The strongest revert signal for `candidate`, using `cache` for change matching. The
/// candidate's own fingerprints are added to the cache when git can compute them.
pub fn detect(
    candidate: &Candidate<'_>,
    repository_path: &str,
    directory: &Path,
    cache: &mut ChangeCache,
) -> Option<RevertLink> {
    let number = github_revert_number(candidate.body, candidate.head_ref, repository_path)
        .filter(|reverted| *reverted != candidate.number);
    let commit_shas = reverted_commits(&candidate.commit_bodies);
    let titled = is_revert_title(candidate.title);
    let prints = candidate
        .merge_commit_sha
        .zip(candidate.merged_at)
        .and_then(|(sha, merged_at)| {
            prints_for(directory, cache, sha, candidate.number, merged_at)
        });
    if !commit_shas.is_empty() {
        return Some(RevertLink {
            pull_request_number: number,
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
    if let Some((sha, prints)) = prints.and_then(|prints| matching_change(cache, &prints)) {
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

/// The SHAs named by `This reverts commit <sha>` lines, in order and without repeats.
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
            }
        }
    }
    shas.truncate(MAX_REVERTED_COMMITS);
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

/// `Revert "…"`, `Revert …` or a Conventional Commits `revert:` / `revert(scope):` title.
pub fn is_revert_title(title: &str) -> bool {
    let title = title.trim_start();
    title.starts_with("Revert ")
        || title.starts_with("revert:")
        || title.starts_with("revert(")
        || title.starts_with("revert!:")
}

/// The cached fingerprints for `merge_sha`, computing and caching them when the commit and
/// its first parent are in the local clone.
fn prints_for(
    directory: &Path,
    cache: &mut ChangeCache,
    merge_sha: &str,
    number: u64,
    merged_at: &str,
) -> Option<ChangePrints> {
    if let Some(prints) = cache.get(merge_sha) {
        return Some(prints.clone());
    }
    let parent = format!("{merge_sha}^1");
    let commit = format!("{merge_sha}^{{commit}}");
    git_bytes(
        directory,
        &["cat-file", "-e", &commit],
        SHORT_GIT_TIMEOUT,
        64,
    )?;
    let forward = fingerprint(&diff(directory, &parent, merge_sha)?)?;
    let inverse = fingerprint(&diff(directory, merge_sha, &parent)?)?;
    let prints = ChangePrints {
        number,
        merged_at: merged_at.to_owned(),
        forward,
        inverse,
    };
    cache.insert(merge_sha.to_owned(), prints.clone());
    Some(prints)
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
            "-U0",
            from,
            to,
            "--",
        ],
        DIFF_TIMEOUT,
        MAX_DIFF_BYTES,
    )
}

/// A fingerprint of a `-U0` diff: per file, the path and each added or removed line with its
/// whitespace stripped, combined in path order. Context, line numbers and blob ids are left
/// out, so an exact inverse matches even after unrelated edits nearby. `None` for an empty
/// diff, which would match every other empty change.
pub fn fingerprint(diff: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(diff);
    let mut files: BTreeMap<String, Sha256> = BTreeMap::new();
    let mut current: Option<String> = None;
    let mut lines = 0usize;
    for line in text.lines() {
        if let Some(header) = line.strip_prefix("diff --git ") {
            current = Some(header.to_owned());
            files.entry(header.to_owned()).or_default();
            continue;
        }
        let Some(path) = current.as_ref() else {
            continue;
        };
        if line.starts_with("+++ ") || line.starts_with("--- ") || line.starts_with("@@") {
            continue;
        }
        let (sign, content) = match line.as_bytes().first() {
            Some(b'+') => ('+', &line[1..]),
            Some(b'-') => ('-', &line[1..]),
            // Binary files, mode changes and new or deleted file markers.
            _ if line.starts_with("Binary files ")
                || line.starts_with("new file mode")
                || line.starts_with("deleted file mode") =>
            {
                ('=', line)
            }
            _ => continue,
        };
        let squeezed: String = content.chars().filter(|c| !c.is_whitespace()).collect();
        let hasher = files.entry(path.clone()).or_default();
        hasher.update([sign as u8]);
        hasher.update(squeezed.as_bytes());
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
            "Revert the cache",
            "revert: drop flag",
            "revert(api): x",
        ] {
            assert!(is_revert_title(title), "{title}");
        }
        for title in [
            "Reverting is hard",
            "fix: revert-proof cache",
            "Add revert button",
        ] {
            assert!(!is_revert_title(title), "{title}");
        }
    }

    const FORWARD: &str = "diff --git a/src/a.rs b/src/a.rs\nindex 1..2 100644\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -3,0 +4,2 @@\n+let a = 1;\n+let b = 2;\n@@ -9 +11 @@\n-old()\n+new()\n";
    // The same change undone, with different line numbers and spacing.
    const INVERSE_OF_REVERT: &str = "diff --git a/src/a.rs b/src/a.rs\nindex 2..1 100644\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -40,0 +41,2 @@\n+let a =  1;\n+let b = 2;\n@@ -90 +92 @@\n-old( )\n+new()\n";

    #[test]
    fn fingerprints_ignore_line_numbers_and_whitespace_but_not_content() {
        assert_eq!(
            fingerprint(FORWARD.as_bytes()),
            fingerprint(INVERSE_OF_REVERT.as_bytes())
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
    fn the_strongest_signal_wins() {
        let mut cache = ChangeCache::new();
        let base = Candidate {
            number: 50,
            title: "Revert \"Add widgets\"",
            body: "Reverts acme/widgets#42",
            head_ref: "revert-42-feat",
            merge_commit_sha: None,
            merged_at: None,
            commit_bodies: vec!["This reverts commit 1111111111111111111111111111111111111111."],
        };
        let dir = Path::new("/nonexistent");
        let link = detect(&base, "acme/widgets", dir, &mut cache).unwrap();
        assert_eq!(
            (link.evidence.as_str(), link.pull_request_number),
            ("revert_commit", Some(42))
        );
        let button = Candidate {
            commit_bodies: vec![],
            ..base
        };
        let link = detect(&button, "acme/widgets", dir, &mut cache).unwrap();
        assert_eq!(link.evidence, "github_revert");
        // A description claim without a revert title or line is not trusted.
        let claim = Candidate {
            title: "Tidy up",
            ..button
        };
        assert!(detect(&claim, "acme/widgets", dir, &mut cache).is_none());
        let titled = Candidate {
            body: "",
            head_ref: "main",
            title: "Revert the cache",
            ..claim
        };
        let link = detect(&titled, "acme/widgets", dir, &mut cache).unwrap();
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
        };
        let original_pr = Candidate {
            number: 1,
            title: "Add widgets",
            body: "",
            head_ref: "feat",
            merge_commit_sha: Some(&original),
            merged_at: Some("2026-11-01T00:00:00Z"),
            commit_bodies: vec![],
        };
        // Fetched newest first: the revert arrives before its original.
        remember(&revert_pr, dir, &mut cache);
        remember(&original_pr, dir, &mut cache);
        let link = detect(&revert_pr, "acme/widgets", dir, &mut cache).expect("change match");
        assert_eq!(link.evidence, "change_match");
        assert_eq!(link.pull_request_number, Some(1));
        assert_eq!(link.commit_shas, std::slice::from_ref(&original));
        assert!(detect(&original_pr, "acme/widgets", dir, &mut cache).is_none());
    }
}
