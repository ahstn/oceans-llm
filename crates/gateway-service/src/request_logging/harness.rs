use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentHarness {
    pub key: &'static str,
    pub label: &'static str,
}

impl AgentHarness {
    pub const UNKNOWN: Self = Self {
        key: "unknown",
        label: "Unknown",
    };
}

const MAX_USER_AGENT_RAW_CHARS: usize = 512;

pub(super) fn normalized_user_agent(value: Option<&String>) -> Option<String> {
    value
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(truncate_user_agent)
}

pub(super) fn request_user_agent(headers: &BTreeMap<String, String>) -> Option<&String> {
    headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("user-agent"))
        .map(|(_, value)| value)
}

fn truncate_user_agent(value: &str) -> String {
    if value.chars().count() <= MAX_USER_AGENT_RAW_CHARS {
        return value.to_string();
    }

    value
        .chars()
        .take(MAX_USER_AGENT_RAW_CHARS)
        .collect::<String>()
}

#[must_use]
pub fn classify_agent_harness(user_agent: Option<&str>) -> AgentHarness {
    let Some(user_agent) = user_agent.map(str::trim).filter(|value| !value.is_empty()) else {
        return AgentHarness::UNKNOWN;
    };
    let lower = user_agent.to_ascii_lowercase();

    if lower.starts_with("codex_cli_rs/") || lower.starts_with("codex/") {
        return AgentHarness {
            key: "codex",
            label: "Codex",
        };
    }
    if lower.starts_with("oh-my-pi/") || lower.starts_with("omp/") {
        return AgentHarness {
            key: "oh_my_pi",
            label: "Oh My Pi",
        };
    }
    if lower == "mastra" || lower.starts_with("mastra/") {
        return AgentHarness {
            key: "mastra",
            label: "Mastra",
        };
    }
    if lower.contains("agent/opencode") {
        return AgentHarness {
            key: "opencode",
            label: "Opencode",
        };
    }
    if lower.contains("agent/claude-code") {
        return AgentHarness {
            key: "claude_code",
            label: "Claude Code",
        };
    }
    if lower.contains("agent/gemini-cli") {
        return AgentHarness {
            key: "gemini_cli",
            label: "Gemini CLI",
        };
    }
    if lower.contains("agent/copilot-cli") {
        return AgentHarness {
            key: "copilot_cli",
            label: "Copilot CLI",
        };
    }
    if lower.starts_with("opencode/") {
        return AgentHarness {
            key: "opencode",
            label: "Opencode",
        };
    }
    if lower.starts_with("claude-cli/") {
        return AgentHarness {
            key: "claude_cli",
            label: "Claude CLI",
        };
    }
    if lower.starts_with("dspy/") {
        return AgentHarness {
            key: "dspy",
            label: "DSPy",
        };
    }
    if lower.starts_with("curl/") {
        return AgentHarness {
            key: "curl",
            label: "curl",
        };
    }
    if lower.starts_with("pi/") {
        return AgentHarness {
            key: "pi",
            label: "Pi",
        };
    }
    if lower.starts_with("claude-code/") || lower.starts_with("claude-user (claude-code/") {
        return AgentHarness {
            key: "claude_code",
            label: "Claude Code",
        };
    }
    if user_agent.starts_with("GeminiCLI")
        && (user_agent.starts_with("GeminiCLI/") || user_agent.starts_with("GeminiCLI-"))
    {
        return AgentHarness {
            key: "gemini_cli",
            label: "Gemini CLI",
        };
    }
    if user_agent.starts_with("CloudCodeVSCode/") {
        return AgentHarness {
            key: "gemini_cli",
            label: "Gemini CLI",
        };
    }
    if lower.starts_with("githubcopilot/") {
        return AgentHarness {
            key: "github_copilot",
            label: "GitHub Copilot",
        };
    }

    AgentHarness::UNKNOWN
}
