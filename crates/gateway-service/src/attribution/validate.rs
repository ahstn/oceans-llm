//! Field checks for ingestion entries. A failed check names a reason code, never the value.

use super::AttributionRejectReason;

pub(super) const MAX_BRANCH_BYTES: usize = 256;
pub(super) const MAX_HARNESS_VERSION_BYTES: usize = 128;
pub(super) const MAX_TITLE_BYTES: usize = 1024;
const MAX_ENTRY_ID_BYTES: usize = 64;
const MAX_HARNESS_KEY_BYTES: usize = 64;
const MAX_REPOSITORY_BYTES: usize = 256;
const MAX_URL_BYTES: usize = 2048;

/// A client entry id: 1-64 characters of `[A-Za-z0-9._-]`.
pub(super) fn is_entry_id(value: &str) -> bool {
    (1..=MAX_ENTRY_ID_BYTES).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// A hook's harness key, such as `claude_code`: 1-64 characters of `[a-z0-9_-]`.
pub(super) fn is_harness_key(value: &str) -> bool {
    (1..=MAX_HARNESS_KEY_BYTES).contains(&value.len())
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        })
}

/// A repository as the CLI normalises remotes: `host[:port]/segment/segment…` with a
/// lowercase host and no scheme, userinfo or `.git` suffix.
pub(super) fn repository(value: &str) -> Result<String, AttributionRejectReason> {
    let mut parts = value.split('/');
    let authority_ok = parts.next().is_some_and(is_authority);
    let mut segments = 0;
    let segments_ok = parts.all(|segment| {
        segments += 1;
        is_path_segment(segment)
    });
    let has_git_suffix = value
        .len()
        .checked_sub(4)
        .and_then(|start| value.get(start..))
        .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".git"));
    if value.len() <= MAX_REPOSITORY_BYTES
        && authority_ok
        && segments_ok
        && segments >= 2
        && !has_git_suffix
    {
        Ok(value.to_string())
    } else {
        Err(AttributionRejectReason::InvalidRepository)
    }
}

fn is_authority(authority: &str) -> bool {
    let (host, port) = match authority.split_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    };
    let host_ok = (1..=253).contains(&host.len())
        && host.split('.').all(|label| {
            (1..=63).contains(&label.len())
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        });
    let port_ok = port.is_none_or(|port| {
        port.len() <= 5
            && port.bytes().all(|byte| byte.is_ascii_digit())
            && port.parse::<u16>().is_ok_and(|port| port > 0)
    });
    host_ok && port_ok
}

fn is_path_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && segment.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'~' | b'+' | b'%')
        })
}

/// A full 40- (SHA-1) or 64-character (SHA-256) lowercase hex object id.
pub(super) fn commit_sha(value: &str) -> Result<String, AttributionRejectReason> {
    let hex = value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if matches!(value.len(), 40 | 64) && hex {
        Ok(value.to_string())
    } else {
        Err(AttributionRejectReason::InvalidEntry)
    }
}

/// Optional display text: empty reads as absent; over-long or control characters are refused.
pub(super) fn optional_text(
    value: Option<String>,
    max_bytes: usize,
) -> Result<Option<String>, AttributionRejectReason> {
    match value {
        Some(value) if value.is_empty() => Ok(None),
        Some(value) if value.len() > max_bytes || value.chars().any(char::is_control) => {
            Err(AttributionRejectReason::InvalidEntry)
        }
        value => Ok(value),
    }
}

/// A pull request link: `https` without userinfo.
pub(super) fn pull_request_url(value: &str) -> Result<String, AttributionRejectReason> {
    let valid = value.len() <= MAX_URL_BYTES
        && url::Url::parse(value).is_ok_and(|url| {
            url.scheme() == "https"
                && url.username().is_empty()
                && url.password().is_none()
                && url.host_str().is_some()
        });
    if valid {
        Ok(value.to_string())
    } else {
        Err(AttributionRejectReason::InvalidEntry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_normalised_repositories_only() {
        for valid in [
            "github.com/Acme/W",
            "github.com/acme/widgets",
            "gitlab.example.com:8443/group/sub/project",
            "dev.azure.com/org/My%20Project/_git/repo",
        ] {
            assert_eq!(repository(valid).as_deref(), Ok(valid), "{valid}");
        }
        for invalid in [
            "",
            "https://github.com/acme/widgets",
            "git@github.com:acme/widgets",
            "user:tok@github.com/acme/widgets",
            "github.com/acme/widgets.git",
            "github.com/acme/widgets.GIT",
            "GitHub.com/acme/widgets",
            "github.com/acme",
            "github.com//widgets",
            "github.com/acme/../widgets",
            "github.com:0/acme/widgets",
            "github.com:99999/acme/widgets",
            "github.com/acme/wid gets",
            "github.com/acme/widgets?x=1",
        ] {
            assert_eq!(
                repository(invalid),
                Err(AttributionRejectReason::InvalidRepository),
                "{invalid}"
            );
        }
        let long = format!("github.com/acme/{}", "a".repeat(250));
        assert!(repository(&long).is_err());
    }

    #[test]
    fn checks_ids_shas_text_and_urls() {
        assert!(is_entry_id("q-0001.note_a"));
        assert!(!is_entry_id(""));
        assert!(!is_entry_id(&"a".repeat(65)));
        assert!(!is_entry_id("a/b"));

        assert!(is_harness_key("claude_code"));
        assert!(is_harness_key("gemini-cli2"));
        assert!(!is_harness_key(""));
        assert!(!is_harness_key("Claude_Code"));
        assert!(!is_harness_key("oh my pi"));
        assert!(!is_harness_key(&"a".repeat(65)));

        assert!(commit_sha(&"a".repeat(40)).is_ok());
        assert!(commit_sha(&"0".repeat(64)).is_ok());
        assert!(commit_sha(&"A".repeat(40)).is_err());
        assert!(commit_sha(&"a".repeat(41)).is_err());
        assert!(commit_sha("f6fb").is_err());

        assert_eq!(optional_text(Some(String::new()), 4), Ok(None));
        assert_eq!(
            optional_text(Some("main".into()), 4),
            Ok(Some("main".into()))
        );
        assert!(optional_text(Some("mains".into()), 4).is_err());
        assert!(optional_text(Some("a\nb".into()), 4).is_err());

        assert!(pull_request_url("https://github.com/acme/widgets/pull/12").is_ok());
        assert!(pull_request_url("http://github.com/acme/widgets/pull/12").is_err());
        assert!(pull_request_url("https://user:tok@github.com/acme/widgets/pull/12").is_err());
        assert!(pull_request_url("not a url").is_err());
    }
}
