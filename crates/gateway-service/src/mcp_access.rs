use std::{collections::HashSet, sync::Arc};

use gateway_core::{
    AuthenticatedApiKey, ExternalMcpToolRecord, GatewayError, IdentityRepository,
    McpAccessRepository, McpAccessResolution, McpGrantSubject, McpToolGrantSubjectKind,
    McpToolProjection,
};
use uuid::Uuid;

#[derive(Clone)]
pub struct McpAccess<R> {
    repo: Arc<R>,
}

impl<R> McpAccess<R>
where
    R: McpAccessRepository + IdentityRepository,
{
    #[must_use]
    pub fn new(repo: Arc<R>) -> Self {
        Self { repo }
    }

    /// The tools the key may call. A team-only server's tools count only for a
    /// caller whose current team owns it, checked here on every request, so a
    /// grant stops working when its holder leaves the team or the server moves.
    pub async fn effective_tools_for_api_key(
        &self,
        auth: &AuthenticatedApiKey,
        mcp_server_id: Option<Uuid>,
        projection: McpToolProjection,
    ) -> Result<McpAccessResolution, GatewayError> {
        let (subjects, team_id) = self.subjects_and_team(auth).await?;
        let mut resolution = self
            .repo
            .resolve_mcp_access_for_subjects(&subjects, mcp_server_id, projection)
            .await?;
        // A request naming a server checks only that server.
        let hidden: HashSet<Uuid> = match mcp_server_id {
            Some(mcp_server_id) => {
                if self
                    .repo
                    .is_mcp_server_hidden_from(mcp_server_id, team_id)
                    .await?
                {
                    HashSet::from([mcp_server_id])
                } else {
                    HashSet::new()
                }
            }
            None => self
                .repo
                .list_team_only_mcp_server_ids_outside(team_id)
                .await?
                .into_iter()
                .collect(),
        };
        retain_reachable_tools(&mut resolution, &hidden);
        Ok(resolution)
    }

    /// Whether the key may reach the server at all: it holds a grant on at
    /// least one of the server's active tools, and the server is not limited
    /// to a team other than the caller's.
    pub async fn has_server_grant(
        &self,
        auth: &AuthenticatedApiKey,
        mcp_server_id: Uuid,
    ) -> Result<bool, GatewayError> {
        let (subjects, team_id) = self.subjects_and_team(auth).await?;
        if self
            .repo
            .is_mcp_server_hidden_from(mcp_server_id, team_id)
            .await?
        {
            return Ok(false);
        }
        Ok(self
            .repo
            .has_mcp_server_grant(&subjects, mcp_server_id)
            .await?)
    }

    pub async fn allowed_tool_for_call(
        &self,
        auth: &AuthenticatedApiKey,
        mcp_server_id: Uuid,
        upstream_name: &str,
    ) -> Result<Option<ExternalMcpToolRecord>, GatewayError> {
        let resolution = self
            .effective_tools_for_api_key(auth, Some(mcp_server_id), McpToolProjection::Access)
            .await?;
        Ok(resolution
            .allowed_tools
            .into_iter()
            .find(|tool| tool.upstream_name == upstream_name))
    }
}

impl<R> McpAccess<R>
where
    R: IdentityRepository,
{
    pub async fn grant_subjects(
        &self,
        auth: &AuthenticatedApiKey,
    ) -> Result<Vec<McpGrantSubject>, GatewayError> {
        Ok(self.subjects_and_team(auth).await?.0)
    }

    /// The key's grant subjects, and the team it acts for now: the key's own
    /// team (a service account's), or its owner's current team.
    pub async fn subjects_and_team(
        &self,
        auth: &AuthenticatedApiKey,
    ) -> Result<(Vec<McpGrantSubject>, Option<Uuid>), GatewayError> {
        let mut subjects = grant_subjects(auth);
        let mut team_id = auth.owner_team_id;
        if team_id.is_none()
            && let Some(user_id) = auth.owner_user_id
            && let Some(membership) = self.repo.get_team_membership_for_user(user_id).await?
        {
            let mut seen = subjects_seen(&subjects);
            push_subject(
                &mut subjects,
                &mut seen,
                McpToolGrantSubjectKind::Team,
                membership.team_id,
            );
            team_id = Some(membership.team_id);
        }
        Ok((subjects, team_id))
    }
}

#[must_use]
pub fn grant_subjects(auth: &AuthenticatedApiKey) -> Vec<McpGrantSubject> {
    let mut subjects = Vec::new();
    let mut seen = HashSet::new();
    // Grants made to one key belong to that key; an OAuth token has none.
    if let Some(api_key_id) = auth.api_key_id() {
        push_subject(
            &mut subjects,
            &mut seen,
            McpToolGrantSubjectKind::ApiKey,
            api_key_id,
        );
    }
    if let Some(user_id) = auth.owner_user_id {
        push_subject(
            &mut subjects,
            &mut seen,
            McpToolGrantSubjectKind::User,
            user_id,
        );
    }
    if let Some(service_account_id) = auth.owner_service_account_id {
        push_subject(
            &mut subjects,
            &mut seen,
            McpToolGrantSubjectKind::ServiceAccount,
            service_account_id,
        );
    }
    if let Some(team_id) = auth.owner_team_id {
        push_subject(
            &mut subjects,
            &mut seen,
            McpToolGrantSubjectKind::Team,
            team_id,
        );
    }
    subjects
}

/// Drops tools of servers in `hidden` and recounts what the resolution reports.
pub fn retain_reachable_tools(resolution: &mut McpAccessResolution, hidden: &HashSet<Uuid>) {
    if hidden.is_empty() {
        return;
    }
    retain_tools_and_recount(
        &mut resolution.allowed_tools,
        ResolutionCounts {
            referenced_server_count: &mut resolution.referenced_server_count,
            filtered_tool_count: &mut resolution.filtered_tool_count,
            exposed_tool_count: resolution.exposed_tool_count,
        },
        |tool| tool.mcp_server_id,
        |tool| !hidden.contains(&tool.mcp_server_id),
    );
}

/// The counts a grant resolution reports beside its tools.
pub(crate) struct ResolutionCounts<'a> {
    pub(crate) referenced_server_count: &'a mut i64,
    pub(crate) filtered_tool_count: &'a mut i64,
    pub(crate) exposed_tool_count: i64,
}

/// Keeps the tools `keep` accepts and recounts the servers they reference and
/// the tools filtered out, so effective access and the catalog report the
/// same numbers after the same filter.
pub(crate) fn retain_tools_and_recount<T>(
    tools: &mut Vec<T>,
    counts: ResolutionCounts<'_>,
    server_of: impl Fn(&T) -> Uuid,
    keep: impl FnMut(&T) -> bool,
) {
    let before = tools.len();
    tools.retain(keep);
    if tools.len() == before {
        return;
    }
    *counts.referenced_server_count =
        tools.iter().map(server_of).collect::<HashSet<_>>().len() as i64;
    *counts.filtered_tool_count = counts.exposed_tool_count.saturating_sub(tools.len() as i64);
}

fn subjects_seen(subjects: &[McpGrantSubject]) -> HashSet<(McpToolGrantSubjectKind, Uuid)> {
    subjects
        .iter()
        .map(|subject| (subject.subject_kind, subject.subject_id))
        .collect()
}

fn push_subject(
    subjects: &mut Vec<McpGrantSubject>,
    seen: &mut HashSet<(McpToolGrantSubjectKind, Uuid)>,
    subject_kind: McpToolGrantSubjectKind,
    subject_id: Uuid,
) {
    if seen.insert((subject_kind, subject_id)) {
        subjects.push(McpGrantSubject {
            subject_kind,
            subject_id,
        });
    }
}
