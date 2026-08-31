use std::collections::BTreeMap;

use super::super::{
    AgentHarness, classify_agent_harness, normalized_user_agent, request_user_agent,
};

#[test]
fn classifies_known_agent_harness_user_agents() {
    let cases = [
        ("opencode/1.2.3", "opencode", "Opencode"),
        ("opencode/1.2.3-beta.1", "opencode", "Opencode"),
        (
            "opencode/1.16.0 ai-sdk/provider-utils/4.0.23 runtime/bun/1.3.14",
            "opencode",
            "Opencode",
        ),
        ("pi/0.4.0 (darwin; bun/1.2.19; arm64)", "pi", "Pi"),
        ("pi/0.4.0 (linux; node/v22.14.0; x64)", "pi", "Pi"),
        ("pi/0.4.0", "pi", "Pi"),
        ("mastra", "mastra", "Mastra"),
        ("MASTRA", "mastra", "Mastra"),
        ("Mastra/0.19.0", "mastra", "Mastra"),
        ("MASTRA/", "mastra", "Mastra"),
        ("claude-code/2.1.89 (cli)", "claude_code", "Claude Code"),
        (
            "claude-cli/2.1.170 (external, claude-vscode, agent-sdk/0.3.165)",
            "claude_cli",
            "Claude CLI",
        ),
        (
            "claude-cli/2.1.158 (external, cli)",
            "claude_cli",
            "Claude CLI",
        ),
        (
            "Claude-User (claude-code/2.1.83; +https://support.anthropic.com/)",
            "claude_code",
            "Claude Code",
        ),
        ("DSPy/3.2.1", "dspy", "DSPy"),
        ("curl/8.7.1", "curl", "curl"),
        (
            "GeminiCLI/0.37.0/gemini-pro (linux; x64; terminal)",
            "gemini_cli",
            "Gemini CLI",
        ),
        (
            "GeminiCLI-a2a-server/0.34.0/gemini-pro (linux; x64; vscode)",
            "gemini_cli",
            "Gemini CLI",
        ),
        (
            "CloudCodeVSCode/0.37.0 (aidev_client; os_type=Linux; proxy_client=geminicli)",
            "gemini_cli",
            "Gemini CLI",
        ),
        ("GitHub CLI 2.88.1 Agent/opencode", "opencode", "Opencode"),
        (
            "GitHub CLI 2.88.1 Agent/claude-code",
            "claude_code",
            "Claude Code",
        ),
        (
            "GitHub CLI 2.88.1 Agent/gemini-cli",
            "gemini_cli",
            "Gemini CLI",
        ),
        (
            "GitHub CLI 2.88.1 Agent/copilot-cli",
            "copilot_cli",
            "Copilot CLI",
        ),
        ("GithubCopilot/1.155.0", "github_copilot", "GitHub Copilot"),
        ("GitHubCopilot/1.155.0", "github_copilot", "GitHub Copilot"),
    ];

    for (user_agent, key, label) in cases {
        assert_eq!(
            classify_agent_harness(Some(user_agent)),
            AgentHarness { key, label }
        );
    }
}

#[test]
fn classifies_missing_empty_and_unmatched_user_agents_as_unknown() {
    for user_agent in [
        None,
        Some(""),
        Some("   "),
        Some("undici"),
        Some("Mozilla/5.0"),
        Some("mastra-es"),
        Some("mastra-es/1.0.0"),
        Some("mastra-elasticsearch"),
        Some("mastra-elasticsearch/1.0.0"),
    ] {
        assert_eq!(classify_agent_harness(user_agent), AgentHarness::UNKNOWN);
    }
}

#[test]
fn normalizes_user_agent_with_length_cap() {
    let value = "a".repeat(600);
    let normalized = normalized_user_agent(Some(&value)).expect("normalized user agent");

    assert_eq!(normalized.len(), 512);
    assert!(normalized.chars().all(|value| value == 'a'));
}

#[test]
fn reads_user_agent_header_case_insensitively() {
    let headers = BTreeMap::from([("User-Agent".to_string(), "opencode/1.2.3".to_string())]);

    assert_eq!(
        request_user_agent(&headers).map(String::as_str),
        Some("opencode/1.2.3")
    );
}

#[test]
fn classifies_supported_session_analysis_harnesses() {
    for (user_agent, expected_key) in [
        ("codex_cli_rs/0.106.0", "codex"),
        ("Codex/0.106.0", "codex"),
        ("claude-code/2.1.0", "claude_code"),
        ("opencode/1.2.3", "opencode"),
        ("pi/0.45.0", "pi"),
        ("oh-my-pi/0.45.0", "oh_my_pi"),
        ("mastra/0.19.0", "mastra"),
        ("OMP/0.45.0", "oh_my_pi"),
    ] {
        assert_eq!(
            classify_agent_harness(Some(user_agent)).key,
            expected_key,
            "{user_agent}"
        );
    }
}
