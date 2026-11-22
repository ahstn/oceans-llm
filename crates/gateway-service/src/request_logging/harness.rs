use std::collections::BTreeMap;

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
