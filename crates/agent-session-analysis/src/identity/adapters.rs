use super::*;
pub(super) fn extract_session(
    body: SessionFields<'_>,
    headers: &BTreeMap<String, String>,
    adapter: Option<HarnessAdapter>,
    codex_metadata: &[EmbeddedMetadata],
) -> SessionResolution {
    match adapter {
        Some(HarnessAdapter::ClaudeCode) => {
            let mut observations =
                header_observations(headers, "x-claude-code-session-id", body.policy);
            observations.extend(claude_metadata_session(body));
            resolve_session(observations)
        }
        Some(HarnessAdapter::Codex) => {
            let mut observations = header_observations(headers, "session-id", body.policy);
            observations.extend(body_observation(
                body,
                &["client_metadata", "session_id"],
                "body:client_metadata.session_id",
            ));
            observations.extend(
                codex_metadata.iter().filter_map(|metadata| {
                    metadata_observation(metadata, "session_id", body.policy)
                }),
            );
            resolve_session(observations)
        }
        Some(HarnessAdapter::OpenCode) => extract_opencode_session(body, headers),
        Some(HarnessAdapter::Pi) => extract_pi_session(body, headers),
        Some(HarnessAdapter::OhMyPi) => extract_oh_my_pi_session(body, headers),
        None => SessionResolution::default(),
    }
}

fn extract_opencode_session(
    body: SessionFields<'_>,
    headers: &BTreeMap<String, String>,
) -> SessionResolution {
    if headers
        .keys()
        .any(|name| name.eq_ignore_ascii_case("x-opencode-session-id"))
    {
        // OpenCode sends the conversation ID separately from cache affinity. Child sessions
        // can share their parent's affinity, so these are not equivalent identity aliases.
        return resolve_session(header_observations(
            headers,
            "x-opencode-session-id",
            body.policy,
        ));
    }
    // Older clients expose only affinity headers. Keep their contract, including overlapping
    // provider groups, but require every supplied legacy alias to agree.
    let mut observations = header_observations(headers, "x-session-id", body.policy);
    observations.extend(header_observations(
        headers,
        "x-session-affinity",
        body.policy,
    ));
    observations.extend(header_observations(
        headers,
        "x-opencode-session",
        body.policy,
    ));
    resolve_session(observations)
}

fn extract_pi_session(
    body: SessionFields<'_>,
    headers: &BTreeMap<String, String>,
) -> SessionResolution {
    let mut observations = header_observations(headers, "session-id", body.policy);
    observations.extend(header_observations(headers, "x-session-id", body.policy));
    observations.extend(header_observations(headers, "session_id", body.policy));
    if observations.is_empty() {
        // With cacheRetention: "none", Pi can send no session header. A request ID or
        // prompt_cache_key alone is not evidence of a conversation.
        return SessionResolution::default();
    }
    observations.extend(header_observations(
        headers,
        "x-client-request-id",
        body.policy,
    ));
    resolve_session(observations)
}

fn extract_oh_my_pi_session(
    body: SessionFields<'_>,
    headers: &BTreeMap<String, String>,
) -> SessionResolution {
    let mut observations = header_observations(headers, "x-claude-code-session-id", body.policy);
    observations.extend(header_observations(headers, "session_id", body.policy));
    observations.extend(header_observations(
        headers,
        "x-litellm-session-id",
        body.policy,
    ));
    observations.extend(body_observation(body, &["session_id"], "body:session_id"));
    observations.extend(claude_metadata_session(body));
    resolve_session(observations)
}

fn claude_metadata_session(body: SessionFields<'_>) -> Option<CandidateObservation> {
    let user_id = body.get("metadata")?.get("user_id")?;
    let Some(user_id) = user_id.as_str() else {
        return Some(CandidateObservation::Invalid);
    };
    if user_id.len() > MAX_TURN_METADATA_BYTES {
        return Some(CandidateObservation::Invalid);
    }
    if user_id == REDACTED_VALUE {
        return (body.policy == ParsePolicy::Routing).then_some(CandidateObservation::Invalid);
    }
    if user_id.trim_start().starts_with('{') {
        let Some(value) = parse_bounded_json_object(user_id) else {
            return Some(CandidateObservation::Invalid);
        };
        let session = value.get("session_id")?;
        let session = match session.as_str() {
            Some(REDACTED_VALUE) if body.policy == ParsePolicy::Passive => return None,
            Some(session) => normalized_identifier(session, false),
            None => None,
        };
        return Some(session.map_or(CandidateObservation::Invalid, |value| {
            CandidateObservation::Valid {
                value,
                source: "body:metadata.user_id.session_id".to_string(),
            }
        }));
    }
    // Claude's legacy wire format encodes device, account and session separately. Only
    // recognize that shape; arbitrary user IDs (even ones containing `_session_`) are not sessions.
    let (device, remainder) = user_id.strip_prefix("user_")?.split_once("_account_")?;
    if device.len() != 64 || !device.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let (account, session) = remainder.split_once("_session_")?;
    if !account.is_empty() && !is_legacy_claude_uuid(account) {
        return None;
    }
    Some(if is_legacy_claude_uuid(session) {
        CandidateObservation::Valid {
            value: session.to_string(),
            source: "body:metadata.user_id.session_id".to_string(),
        }
    } else {
        CandidateObservation::Invalid
    })
}

fn is_legacy_claude_uuid(value: &str) -> bool {
    value.len() == 36 && uuid::Uuid::try_parse(value).is_ok()
}

fn parse_bounded_json_object(value: &str) -> Option<Value> {
    if value.len() > MAX_TURN_METADATA_BYTES {
        return None;
    }
    let parsed: Value = serde_json::from_str(value).ok()?;
    parsed.is_object().then_some(parsed)
}

pub(super) fn codex_turn_metadata(
    body: SessionFields<'_>,
    headers: &BTreeMap<String, String>,
) -> (Vec<EmbeddedMetadata>, bool) {
    let mut result = Vec::new();
    let mut malformed = false;
    let body_value = body
        .get("client_metadata")
        .and_then(|metadata| metadata.get("x-codex-turn-metadata"));
    if let Some(raw) = body_value {
        if let Some(value) = raw.as_str().and_then(parse_bounded_json_object) {
            result.push(EmbeddedMetadata {
                value,
                source: "body:client_metadata.x-codex-turn-metadata".to_string(),
            });
        } else {
            malformed = true;
        }
    }
    for (_, raw) in headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("x-codex-turn-metadata"))
    {
        if let Some(value) = parse_bounded_json_object(raw.trim_matches([' ', '\t'])) {
            result.push(EmbeddedMetadata {
                value,
                source: "header:x-codex-turn-metadata".to_string(),
            });
        } else {
            malformed = true;
        }
    }
    (result, malformed)
}

fn resolved_lineage_value(observations: Vec<CandidateObservation>) -> Option<String> {
    let resolution = resolve_session(observations);
    if resolution.limitation.is_none() {
        resolution.value
    } else {
        None
    }
}

pub(super) fn extract_lineage(
    body: SessionFields<'_>,
    headers: &BTreeMap<String, String>,
    adapter: Option<HarnessAdapter>,
    codex_metadata: &[EmbeddedMetadata],
) -> (Option<String>, Option<String>) {
    match adapter {
        Some(HarnessAdapter::ClaudeCode) => (
            resolved_lineage_value(header_observations(
                headers,
                "x-claude-code-agent-id",
                body.policy,
            )),
            resolved_lineage_value(header_observations(
                headers,
                "x-claude-code-parent-agent-id",
                body.policy,
            )),
        ),
        Some(HarnessAdapter::OpenCode) => {
            let mut parent =
                header_observations(headers, "x-opencode-parent-session-id", body.policy);
            parent.extend(header_observations(
                headers,
                "x-parent-session-id",
                body.policy,
            ));
            (None, resolved_lineage_value(parent))
        }
        Some(HarnessAdapter::Codex) => extract_codex_lineage(body, headers, codex_metadata),
        Some(HarnessAdapter::Pi | HarnessAdapter::OhMyPi) | None => (None, None),
    }
}

fn extract_codex_lineage(
    body: SessionFields<'_>,
    headers: &BTreeMap<String, String>,
    metadata: &[EmbeddedMetadata],
) -> (Option<String>, Option<String>) {
    let mut thread = header_observations(headers, "thread-id", body.policy);
    thread.extend(header_observations(
        headers,
        "x-client-request-id",
        body.policy,
    ));
    thread.extend(body_observation(
        body,
        &["client_metadata", "thread_id"],
        "body:client_metadata.thread_id",
    ));
    thread.extend(
        metadata
            .iter()
            .filter_map(|value| metadata_observation(value, "thread_id", body.policy)),
    );
    let thread = resolve_session(thread);
    let execution_id = if thread.limitation.is_some() {
        None
    } else if thread.value.is_some() {
        thread.value
    } else {
        let mut turn = body_observation(
            body,
            &["client_metadata", "turn_id"],
            "body:client_metadata.turn_id",
        )
        .into_iter()
        .collect::<Vec<_>>();
        turn.extend(
            metadata
                .iter()
                .filter_map(|value| metadata_observation(value, "turn_id", body.policy)),
        );
        resolved_lineage_value(turn)
    };

    let mut parent = header_observations(headers, "x-codex-parent-thread-id", body.policy);
    parent.extend(body_observation(
        body,
        &["client_metadata", "x-codex-parent-thread-id"],
        "body:client_metadata.x-codex-parent-thread-id",
    ));
    parent.extend(
        metadata
            .iter()
            .filter_map(|value| metadata_observation(value, "parent_thread_id", body.policy)),
    );
    if parent.is_empty() {
        parent.extend(
            metadata.iter().filter_map(|value| {
                metadata_observation(value, "forked_from_thread_id", body.policy)
            }),
        );
    }
    (execution_id, resolved_lineage_value(parent))
}
