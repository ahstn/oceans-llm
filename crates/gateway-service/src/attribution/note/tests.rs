use super::{NoteError, NoteHuman, NoteSession, ParsedNote, author_email, parse_note};

/// The note git-ai 1.7.6 passed to `post_notes_updated` on this project's test machine.
const SESSION_NOTE: &str = "b.txt\n  s_b1d318a1ca4b3a::t_f577269eea4edb 1-3\n---\n{\n  \"schema_version\": \"authorship/3.0.0\",\n  \"git_ai_version\": \"1.7.6\",\n  \"base_commit_sha\": \"7734793b756b3921c88db5375a8c156e9532447b\",\n  \"prompts\": {},\n  \"sessions\": {\"s_b1d318a1ca4b3a\": {\"agent_id\": {\"tool\": \"mock_ai\", \"id\": \"ai-thread-1794\", \"model\": \"unknown\"}, \"human_author\": \"Adam Houston <a@b.c>\"}}\n}";

/// Section 1.2.7 of the git-ai standard: legacy prompts, sessions and humans in one note.
const MIXED_NOTE: &str = r#"src/main.rs
  s_c9883b05a2487d::t_9f8e7d6c5b4a32 1-10
  h_31dce776f88375 15-20
src/lib.rs
  abcd1234abcd1234 1-50
---
{
  "schema_version": "authorship/3.0.0",
  "git_ai_version": "1.4.5",
  "base_commit_sha": "7734793b756b3921c88db5375a8c156e9532447b",
  "prompts": {
    "abcd1234abcd1234": {
      "agent_id": {"tool": "cursor", "id": "old_session_id", "model": "gpt-4"},
      "human_author": "Developer <dev@example.com>",
      "total_additions": 50,
      "total_deletions": 0,
      "accepted_lines": 50,
      "overriden_lines": 0
    }
  },
  "humans": {"h_31dce776f88375": {"author": "Developer <dev@example.com>"}},
  "sessions": {
    "s_c9883b05a2487d": {
      "agent_id": {"tool": "claude", "id": "conv_abc123", "model": "claude-sonnet-4-5-20250514"},
      "human_author": "dev@example.com",
      "custom_attributes": {"team": "backend"}
    }
  }
}"#;

fn note_with(attestations: &str, sessions: &str) -> String {
    format!(
        "{attestations}---\n{{\"schema_version\": \"authorship/3.0.0\", \"base_commit_sha\": \"x\", \"prompts\": {{}}, \"sessions\": {sessions}}}"
    )
}

fn ai_lines_for(note: &ParsedNote, key: &str) -> i64 {
    note.sessions
        .iter()
        .find(|session| session.key == key)
        .map(|session| session.lines)
        .expect("session present")
}

#[test]
fn parses_the_session_note_git_ai_emits() {
    let note = parse_note(SESSION_NOTE).unwrap();
    assert_eq!(note.schema_version, "authorship/3.0.0");
    assert_eq!(note.git_ai_version.as_deref(), Some("1.7.6"));
    assert_eq!(
        note.sessions,
        vec![NoteSession {
            key: "s_b1d318a1ca4b3a".to_string(),
            tool: Some("mock_ai".to_string()),
            agent_session_id: Some("ai-thread-1794".to_string()),
            model: None,
            human_author: Some("Adam Houston <a@b.c>".to_string()),
            lines: 3,
        }]
    );
    assert!(note.humans.is_empty());
    assert_eq!((note.ai_lines(), note.human_lines()), (3, 0));
}

#[test]
fn parses_human_only_notes() {
    let note = parse_note(
        "a.txt\n  h_2f452a9c375069 1\n---\n{\"schema_version\": \"authorship/3.0.0\", \"base_commit_sha\": \"x\", \"prompts\": {}, \"humans\": {\"h_2f452a9c375069\": {\"author\": \"Name <name@example.com>\"}}}",
    )
    .unwrap();
    assert!(note.sessions.is_empty());
    assert_eq!(
        note.humans,
        vec![NoteHuman {
            key: "h_2f452a9c375069".to_string(),
            author: Some("Name <name@example.com>".to_string()),
            lines: 1,
        }]
    );
    assert_eq!((note.ai_lines(), note.human_lines()), (0, 1));
}

#[test]
fn parses_all_three_key_formats() {
    let note = parse_note(MIXED_NOTE).unwrap();
    let keys = note
        .sessions
        .iter()
        .map(|session| {
            (
                session.key.as_str(),
                session.tool.as_deref(),
                session.model.as_deref(),
                session.lines,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        keys,
        vec![
            ("abcd1234abcd1234", Some("cursor"), Some("gpt-4"), 50),
            (
                "s_c9883b05a2487d",
                Some("claude"),
                Some("claude-sonnet-4-5-20250514"),
                10
            ),
        ]
    );
    assert_eq!(note.humans.len(), 1);
    assert_eq!((note.ai_lines(), note.human_lines()), (60, 6));
}

#[test]
fn overlapping_ranges_count_once_per_key_and_file() {
    let note = parse_note(&note_with(
        "a.rs\n  s_aa::t_01 1-4,3-7\n  s_aa::t_02 6-8\n  s_bb::t_01 1-2\nb.rs\n  s_aa::t_01 1-2\na.rs\n  s_aa::t_03 8,9\n",
        "{}",
    ))
    .unwrap();
    // a.rs: 1-9 for s_aa across three traces and two blocks; b.rs adds 2.
    assert_eq!(ai_lines_for(&note, "s_aa"), 11);
    assert_eq!(ai_lines_for(&note, "s_bb"), 2);
}

#[test]
fn quoted_paths_and_crlf_lines_parse() {
    let note = parse_note(
        "\"src/my file.rs\"\r\n  abcd123 1-2\r\n---\r\n{\"schema_version\": \"authorship/3.0.0\", \"base_commit_sha\": \"x\", \"prompts\": {}}",
    )
    .unwrap();
    assert_eq!(ai_lines_for(&note, "abcd123"), 2);
}

#[test]
fn keeps_unattested_sessions_and_undescribed_keys() {
    let note = parse_note(&note_with(
        "a.rs\n  s_aa 1-3\n  h_cc 4\n",
        r#"{"s_dd": {"agent_id": {"tool": "codex", "id": "thread-1", "model": ""}}}"#,
    ))
    .unwrap();
    assert_eq!(ai_lines_for(&note, "s_aa"), 3, "a trace-less session key");
    let undescribed = &note.sessions[0];
    assert_eq!((undescribed.tool.as_ref(), undescribed.lines), (None, 3));
    let unattested = &note.sessions[1];
    assert_eq!(unattested.key, "s_dd");
    assert_eq!((unattested.lines, unattested.model.as_ref()), (0, None));
    assert_eq!(note.humans[0].author, None);
    assert_eq!(note.human_lines(), 1);
}

#[test]
fn rejects_malformed_notes() {
    let cases = [
        ("a.rs\n  abcd123 1\n", "missing divider"),
        ("a.rs\n  abcd123 1\n---\nnot json", "metadata is not JSON"),
        (
            "a.rs\n  abcd123 1\n---\n{\"prompts\": {}}",
            "no schema_version",
        ),
        (
            "  abcd123 1\n---\n{\"schema_version\": \"authorship/3.0.0\"}",
            "entry before a path",
        ),
    ];
    for (note, case) in cases {
        assert!(
            matches!(parse_note(note), Err(NoteError::Invalid(_))),
            "{case}"
        );
    }
    for attestation in [
        "a.rs\n  x_123 1\n",
        "a.rs\n  s_zz::t_00 1\n",
        "a.rs\n  s_aa::q_00 1\n",
        "a.rs\n  h_ 1\n",
        "a.rs\n  abcd12345 1\n",
        "a.rs\n  abcd123\n",
        "a.rs\n   abcd123 1\n",
    ] {
        assert!(
            matches!(
                parse_note(&note_with(attestation, "{}")),
                Err(NoteError::Invalid(_))
            ),
            "{attestation:?}"
        );
    }
    for ranges in ["1-", "a", "3-1", "0", "1,,2", "1 2", "-1"] {
        let note = note_with(&format!("a.rs\n  abcd123 {ranges}\n"), "{}");
        assert!(
            matches!(parse_note(&note), Err(NoteError::Invalid(_))),
            "{ranges:?}"
        );
    }
}

#[test]
fn checks_the_schema_version_before_the_shape() {
    for version in ["authorship/4.0.0", "authorship/2.1.0", "other"] {
        let note = format!("---\n{{\"schema_version\": \"{version}\", \"prompts\": 7}}");
        assert_eq!(
            parse_note(&note),
            Err(NoteError::UnsupportedSchema),
            "{version}"
        );
    }
    let wrong_shape = "---\n{\"schema_version\": \"authorship/3.0.0\", \"prompts\": 7}";
    assert!(matches!(
        parse_note(wrong_shape),
        Err(NoteError::Invalid(_))
    ));
}

#[test]
fn enforces_size_and_count_bounds() {
    let at_limit = note_with("a.rs\n  abcd123 10000000\n", "{}");
    assert_eq!(ai_lines_for(&parse_note(&at_limit).unwrap(), "abcd123"), 1);
    let past_line_limit = note_with("a.rs\n  abcd123 10000001\n", "{}");
    assert_eq!(parse_note(&past_line_limit), Err(NoteError::Oversized));
    let huge_number = note_with("a.rs\n  abcd123 99999999999999999999\n", "{}");
    assert_eq!(parse_note(&huge_number), Err(NoteError::Oversized));

    let ranges = (1..=10_001)
        .map(|line| line.to_string())
        .collect::<Vec<_>>();
    let too_many_ranges = note_with(&format!("a.rs\n  abcd123 {}\n", ranges.join(",")), "{}");
    assert_eq!(parse_note(&too_many_ranges), Err(NoteError::Oversized));

    let too_many_paths = note_with(&"f\n".repeat(10_001), "{}");
    assert_eq!(parse_note(&too_many_paths), Err(NoteError::Oversized));

    let oversized = format!("{}---\n{{}}", "a".repeat(super::MAX_NOTE_BYTES));
    assert_eq!(parse_note(&oversized), Err(NoteError::Oversized));

    let padding = " ".repeat(super::MAX_METADATA_BYTES);
    let big_metadata =
        format!("---\n{{\"schema_version\": \"authorship/3.0.0\", \"prompts\": {{}}{padding}}}");
    assert_eq!(parse_note(&big_metadata), Err(NoteError::Oversized));

    let long_tool = note_with(
        "",
        &format!(
            r#"{{"s_aa": {{"agent_id": {{"tool": "{}", "id": "x"}}}}}}"#,
            "t".repeat(65)
        ),
    );
    assert!(matches!(parse_note(&long_tool), Err(NoteError::Invalid(_))));
}

#[test]
fn debug_output_redacts_raw_identifiers() {
    let rendered = format!("{:?}", parse_note(MIXED_NOTE).unwrap());
    for raw in [
        "conv_abc123",
        "old_session_id",
        "dev@example.com",
        "Developer",
    ] {
        assert!(!rendered.contains(raw), "{raw} leaked into {rendered}");
    }
    assert!(rendered.contains("s_c9883b05a2487d"));
}

#[test]
fn extracts_normalized_author_emails() {
    assert_eq!(
        author_email("Adam Houston <A@B.C>").as_deref(),
        Some("a@b.c")
    );
    assert_eq!(
        author_email("  dev@Example.com ").as_deref(),
        Some("dev@example.com")
    );
    assert_eq!(
        author_email("Dev <dev@example.com> ").as_deref(),
        Some("dev@example.com")
    );
    for invalid in ["Adam Houston", "<>", "Name <not an email>", "", "a b@c.d"] {
        assert_eq!(author_email(invalid), None, "{invalid:?}");
    }
}
