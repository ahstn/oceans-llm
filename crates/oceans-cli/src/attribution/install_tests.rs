use std::fs;

use super::*;

const COMMAND: &str = "/usr/local/bin/oceans hook claude";

#[test]
fn git_ai_hook_listings_parse_in_both_shapes() {
    assert!(registered_hooks(b"{}").unwrap().is_empty());
    assert_eq!(
        registered_hooks(br#"{"post_notes_updated":["a hook notes","b"]}"#).unwrap(),
        ["a hook notes", "b"]
    );
    assert_eq!(
        registered_hooks(br#"{"post_notes_updated":"a hook notes"}"#).unwrap(),
        ["a hook notes"]
    );
    assert!(registered_hooks(b"not json").is_err());
}

#[test]
fn claude_hooks_are_added_without_disturbing_other_settings() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    let original = r#"{
  "model": "opus",
  "hooks": {
    "PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "lint"}]}],
    "SessionStart": [{"hooks": [{"type": "command", "command": "echo hi"}]}]
  },
  "permissions": {"allow": ["Bash(ls:*)"]},
  "zeta": 1.5
}
"#;
    fs::write(&path, original).unwrap();
    assert_eq!(claude(&path, COMMAND).unwrap(), Change::Added);
    let written = fs::read_to_string(&path).unwrap();
    let keys: Vec<_> = ["\"model\"", "\"hooks\"", "\"permissions\"", "\"zeta\""]
        .iter()
        .map(|key| written.find(key).unwrap())
        .collect();
    assert!(keys.is_sorted(), "{written}");
    let value: serde_json::Value = serde_json::from_str(&written).unwrap();
    assert_eq!(
        value["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
        "lint"
    );
    assert_eq!(
        value["hooks"]["SessionStart"][0]["hooks"][0]["command"],
        "echo hi"
    );
    assert_eq!(
        value["hooks"]["SessionStart"][1]["hooks"][0]["command"],
        COMMAND
    );
    assert_eq!(
        value["hooks"]["SessionEnd"][0]["hooks"][0]["command"],
        COMMAND
    );
    assert_eq!(value["zeta"], 1.5);

    assert_eq!(claude(&path, COMMAND).unwrap(), Change::Unchanged);
    assert_eq!(fs::read_to_string(&path).unwrap(), written);
}

#[test]
fn claude_hooks_from_a_moved_binary_are_updated_in_place() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    let old = serde_json::json!({"hooks": {
        "SessionStart": [{"hooks": [{"type": "command", "command": "'/old path/oceans' hook claude"}]}],
        "SessionEnd": [{"hooks": [{"type": "command", "command": "/old/oceans hook claude"}]}],
    }});
    fs::write(&path, old.to_string()).unwrap();
    assert_eq!(claude(&path, COMMAND).unwrap(), Change::Updated);
    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    for event in CLAUDE_EVENTS {
        let groups = value["hooks"][event].as_array().unwrap();
        assert_eq!(groups.len(), 1, "{event}");
        assert_eq!(groups[0]["hooks"][0]["command"], COMMAND);
    }
}

#[test]
fn invalid_claude_settings_are_left_untouched() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    for contents in ["{\"model\": ", "[]", "{\"hooks\": []}"] {
        fs::write(&path, contents).unwrap();
        assert!(claude(&path, COMMAND).is_err(), "{contents}");
        assert_eq!(fs::read_to_string(&path).unwrap(), contents);
    }
    let missing = directory.path().join("new/settings.json");
    assert_eq!(claude(&missing, COMMAND).unwrap(), Change::Added);
}

#[cfg(unix)]
#[test]
fn claude_settings_symlinks_and_permissions_are_kept() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let directory = tempfile::tempdir().unwrap();
    let real = directory.path().join("dotfiles-settings.json");
    fs::write(&real, "{}").unwrap();
    fs::set_permissions(&real, fs::Permissions::from_mode(0o640)).unwrap();
    let link = directory.path().join("settings.json");
    symlink(&real, &link).unwrap();
    assert_eq!(claude(&link, COMMAND).unwrap(), Change::Added);
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(fs::read_to_string(&real).unwrap().contains(COMMAND));
    assert_eq!(
        fs::metadata(&real).unwrap().permissions().mode() & 0o777,
        0o640
    );
}

#[test]
fn the_omp_extension_gets_both_binary_paths() {
    let rendered = render_omp_extension("/opt/my \"tools\"/oceans", "C:\\bin\\git-ai.exe");
    assert!(rendered.contains(r#"const OCEANS_BIN = "/opt/my \"tools\"/oceans";"#));
    assert!(rendered.contains(r#"const GIT_AI_BIN = "C:\\bin\\git-ai.exe";"#));
    assert!(!rendered.contains("__OCEANS_BIN__") && !rendered.contains("__GIT_AI_BIN__"));

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("agent/extensions/oceans-git-ai.ts");
    assert_eq!(omp(&path, &rendered).unwrap(), Change::Added);
    assert_eq!(omp(&path, &rendered).unwrap(), Change::Unchanged);
    let moved = render_omp_extension("/usr/bin/oceans", "git-ai");
    assert_eq!(omp(&path, &moved).unwrap(), Change::Updated);
    assert_eq!(fs::read_to_string(&path).unwrap(), moved);
}

#[test]
fn install_locations_follow_the_harness_environment() {
    let home = Path::new("/home/alice");
    assert_eq!(
        claude_settings_path(&Env::fixed(&[], Some(home))),
        Some("/home/alice/.claude/settings.json".into())
    );
    assert_eq!(
        claude_settings_path(&Env::fixed(
            &[("CLAUDE_CONFIG_DIR", "/cfg/claude")],
            Some(home)
        )),
        Some("/cfg/claude/settings.json".into())
    );
    assert_eq!(
        omp_extension_path(&Env::fixed(&[], Some(home))),
        Some("/home/alice/.omp/agent/extensions/oceans-git-ai.ts".into())
    );
    assert_eq!(
        omp_extension_path(&Env::fixed(
            &[("PI_CODING_AGENT_DIR", "/agents/pi")],
            Some(home)
        )),
        Some("/agents/pi/extensions/oceans-git-ai.ts".into())
    );
}
