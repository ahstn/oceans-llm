//! The read-only MCP registry members see: the servers and tools their own
//! grants, keys and team reach.

use super::*;

/// How many of a member's API keys `member_access` resolves on their own at
/// once.
const MEMBER_KEY_RESOLUTIONS_AT_ONCE: usize = 4;

/// The active tools a user can call, by ID, and the servers they are on, read
/// by ID. Grants to the user or their team count as they reach any of the
/// user's MCP clients. A grant to one of their API keys counts only while that
/// key may call MCP at all and its server list, if it has one, includes the
/// tool's server, as runtime resolution enforces. As at runtime, a team-only
/// server counts only for its own team's members.
///
/// `server_id` limits the result to one server. Each MCP-capable key is
/// resolved with its owner and team, as a request made with it would be, so
/// tag rules on any of them apply. Only keys a tag rule could change are
/// resolved on their own, at most `MEMBER_KEY_RESOLUTIONS_AT_ONCE` at a time;
/// every other key's grants are read together in one query.
pub(super) struct MemberAccess {
    pub(super) tool_ids: Vec<Uuid>,
    pub(super) servers: Vec<gateway_core::ExternalMcpServerRecord>,
}

pub(super) async fn member_access(
    state: &AppState,
    user_id: Uuid,
    team_id: Option<Uuid>,
    server_id: Option<Uuid>,
) -> Result<MemberAccess, AppError> {
    use gateway_core::{AdminApiKeyRepository, SCOPE_MCP_AGGREGATE, SCOPE_MCP_PROXY};
    let keys: std::collections::HashMap<Uuid, gateway_core::ApiKeyAccess> = state
        .store
        .list_api_keys_for_user_scope(user_id, None)
        .await?
        .into_iter()
        .filter(|key| key.owner_user_id == Some(user_id) && key.revoked_at.is_none())
        .filter(|key| {
            key.access.has_scope(SCOPE_MCP_AGGREGATE) || key.access.has_scope(SCOPE_MCP_PROXY)
        })
        .map(|key| (key.id, key.access))
        .collect();
    let mut subjects = vec![McpGrantSubject {
        subject_kind: McpToolGrantSubjectKind::User,
        subject_id: user_id,
    }];
    if let Some(team_id) = team_id {
        subjects.push(McpGrantSubject {
            subject_kind: McpToolGrantSubjectKind::Team,
            subject_id: team_id,
        });
    }
    // Tool ID to server ID, for every tool the user can call.
    let mut tool_servers: std::collections::HashMap<Uuid, Uuid> = state
        .store
        // Only the granted tools are read here, so no catalog-wide count runs.
        .resolve_mcp_access_for_subjects(&subjects, server_id, McpToolProjection::AccessUncounted)
        .await?
        .allowed_tools
        .into_iter()
        .map(|tool| (tool.mcp_tool_id, tool.mcp_server_id))
        .collect();
    // A key reaches what its owner and team do, plus its own grants. Its tools
    // are what runtime resolves for the key, its owner and their team
    // together, so tag rules on any of them, include and exclude alike, apply.
    // Most keys need no resolution of their own: one with no tag rules adds
    // exactly its grants, read for every key in one query, unless its owner or
    // team has rules that could exclude some of them. Only the other keys are
    // resolved one by one, a few at a time.
    let key_ids: Vec<Uuid> = keys.keys().copied().collect();
    let key_grants = state
        .store
        .list_mcp_tool_access_for_api_keys(&key_ids, server_id)
        .await?;
    let keys_with_rules: std::collections::HashSet<Uuid> = state
        .store
        .list_mcp_tag_rule_subject_ids(McpToolGrantSubjectKind::ApiKey, &key_ids)
        .await?
        .into_iter()
        .collect();
    let owner_has_rules = !state
        .store
        .list_mcp_tag_rule_subject_ids(McpToolGrantSubjectKind::User, &[user_id])
        .await?
        .is_empty()
        || match team_id {
            Some(team_id) => !state
                .store
                .list_mcp_tag_rule_subject_ids(McpToolGrantSubjectKind::Team, &[team_id])
                .await?
                .is_empty(),
            None => false,
        };
    let keys_with_grants: std::collections::HashSet<Uuid> =
        key_grants.iter().map(|grant| grant.api_key_id).collect();
    let resolve_each: std::collections::HashSet<Uuid> = key_ids
        .iter()
        .copied()
        .filter(|key_id| {
            keys_with_rules.contains(key_id)
                || (owner_has_rules && keys_with_grants.contains(key_id))
        })
        .collect();
    let mut key_tools: Vec<(Uuid, Uuid, Uuid)> = key_grants
        .into_iter()
        .filter(|grant| !resolve_each.contains(&grant.api_key_id))
        .map(|grant| (grant.api_key_id, grant.mcp_tool_id, grant.mcp_server_id))
        .collect();
    use futures_util::{StreamExt, TryStreamExt};
    let resolved: Vec<Vec<(Uuid, Uuid, Uuid)>> = futures_util::stream::iter(resolve_each)
        .map(|key_id| {
            let mut key_subjects = subjects.clone();
            key_subjects.push(McpGrantSubject {
                subject_kind: McpToolGrantSubjectKind::ApiKey,
                subject_id: key_id,
            });
            async move {
                let access = state
                    .store
                    .resolve_mcp_access_for_subjects(
                        &key_subjects,
                        server_id,
                        McpToolProjection::AccessUncounted,
                    )
                    .await?;
                Ok::<_, GatewayError>(
                    access
                        .allowed_tools
                        .into_iter()
                        .map(|tool| (key_id, tool.mcp_tool_id, tool.mcp_server_id))
                        .collect::<Vec<_>>(),
                )
            }
        })
        .buffer_unordered(MEMBER_KEY_RESOLUTIONS_AT_ONCE)
        .try_collect()
        .await?;
    key_tools.extend(resolved.into_iter().flatten());
    let referenced: Vec<Uuid> = tool_servers
        .values()
        .copied()
        .chain(key_tools.iter().map(|(_, _, server)| *server))
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let mut servers = state
        .store
        .list_external_mcp_servers_by_ids(&referenced)
        .await?;
    // A key's tools count only while it may call MCP on their server.
    let server_keys: std::collections::HashMap<Uuid, &str> = servers
        .iter()
        .map(|server| (server.mcp_server_id, server.server_key.as_str()))
        .collect();
    for (key_id, tool_id, tool_server) in key_tools {
        if server_keys
            .get(&tool_server)
            .zip(keys.get(&key_id))
            .is_some_and(|(server_key, access)| access.allows_mcp_server(server_key))
        {
            tool_servers.insert(tool_id, tool_server);
        }
    }
    // Only servers left with a tool the user can call, on servers they reach.
    let with_tools: std::collections::HashSet<Uuid> = tool_servers.values().copied().collect();
    servers.retain(|server| {
        with_tools.contains(&server.mcp_server_id) && server.ownership.is_reachable_from(team_id)
    });
    let reachable: std::collections::HashSet<Uuid> =
        servers.iter().map(|server| server.mcp_server_id).collect();
    let tool_ids = tool_servers
        .into_iter()
        .filter(|(_, server)| reachable.contains(server))
        .map(|(tool, _)| tool)
        .collect();
    Ok(MemberAccess { tool_ids, servers })
}

/// A member's read-only server list: the servers they can connect to.
pub(super) async fn list_member_servers(
    state: &AppState,
    user_id: Uuid,
    team_id: Option<Uuid>,
    query: &McpServersQuery,
) -> Result<Json<Envelope<McpServersPayload>>, AppError> {
    // Only the servers the member can reach are read, not the whole registry.
    // The reads follow the member's team, so after them each listed server's
    // ownership is read again, and then the member's team, last: a server
    // moved away, or a member who left or changed team, in between gets the
    // list for the team they are in now, never the former team's servers.
    let owner = query.owner.as_deref().map(parse_owner_filter).transpose()?;
    let mut team_id = team_id;
    for _ in 0..3 {
        let access = member_access(state, user_id, team_id, None).await?;
        let servers: Vec<_> = access
            .servers
            .into_iter()
            .filter(|server| {
                query.include_disabled
                    || server.status != gateway_core::ExternalMcpServerStatus::Disabled
            })
            .filter(|server| owner.is_none_or(|owner| owner.matches(server)))
            .collect();
        let names: std::collections::HashMap<Uuid, String> = if servers
            .iter()
            .any(|server| server.ownership.owner_team_id.is_some())
        {
            gateway_core::AdminIdentityRepository::list_teams_by_ids(
                &*state.store,
                &owner_team_ids(&servers),
            )
            .await?
            .into_iter()
            .map(|team| (team.team_id, team.team_name))
            .collect()
        } else {
            std::collections::HashMap::new()
        };
        let ids: Vec<Uuid> = servers.iter().map(|server| server.mcp_server_id).collect();
        let ownerships = state.store.list_mcp_server_ownerships(&ids).await?;
        let current =
            gateway_core::IdentityRepository::get_team_membership_for_user(&*state.store, user_id)
                .await?
                .map(|membership| membership.team_id);
        if current != team_id {
            team_id = current;
            continue;
        }
        let still_reached: std::collections::HashSet<Uuid> = ownerships
            .into_iter()
            .filter(|(_, ownership)| ownership.is_reachable_from(team_id))
            .map(|(mcp_server_id, _)| mcp_server_id)
            .collect();
        let items = servers
            .into_iter()
            .filter(|server| still_reached.contains(&server.mcp_server_id))
            .map(|server| {
                let name = server
                    .ownership
                    .owner_team_id
                    .and_then(|team_id| names.get(&team_id).cloned());
                map_server(server).for_viewer(name, false)
            })
            .collect();
        return Ok(Json(envelope(McpServersPayload {
            items,
            can_publish_to_organisation: false,
        })));
    }
    Err(AppError(GatewayError::Store(
        gateway_core::StoreError::Conflict(
            "your team membership kept changing while MCP servers were listed; try again"
                .to_string(),
        ),
    )))
}
