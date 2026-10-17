//! Who may administer an MCP server, and who may be granted its tools.
//!
//! Every MCP admin check that depends on ownership goes through [`McpOwnership`],
//! so tool grants, tag-based grants and tool approvals agree on the boundary.
//! See `docs/adr/2026-10-17-delegated-mcp-server-ownership.md`.

use std::{collections::HashSet, sync::Arc};

use gateway_core::{
    AdminApiKeyRepository, AuthError, ExternalMcpServerRecord, GatewayError, IdentityRepository,
    McpAccessRepository, McpGrantSubject, McpRegistryRepository, McpServerOwnership,
    McpServerVisibility, McpToolGrantRecord, McpToolGrantSubjectKind, McpToolGrantTargetKind,
    McpUpstreamCredentialOwnerScopeKind, MembershipRole, StoreError, TeamMembershipRecord,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::admin_audit::AuditEvent;

/// Owner contacts longer than this are rejected.
pub const MAX_OWNER_CONTACT_CHARS: usize = 256;

/// Who may publish a server to the organisation catalog
/// (`mcp.registry.organisation_publishing`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpOrganisationPublishing {
    #[default]
    PlatformAdmins,
    TeamAdmins,
}

/// The caller of an MCP admin route: a platform admin, or the owner or admin of
/// one team. `membership` is the caller's team membership, if any, for audit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct McpAdminScope {
    platform_admin: bool,
    membership: Option<(Uuid, MembershipRole)>,
}

impl McpAdminScope {
    #[must_use]
    pub fn platform_admin(membership: Option<&TeamMembershipRecord>) -> Self {
        Self {
            platform_admin: true,
            membership: membership.map(|membership| (membership.team_id, membership.role)),
        }
    }

    /// A team owner or admin acting for their team. Members get `None`.
    #[must_use]
    pub fn team_admin(membership: &TeamMembershipRecord) -> Option<Self> {
        matches!(
            membership.role,
            MembershipRole::Owner | MembershipRole::Admin
        )
        .then_some(Self {
            platform_admin: false,
            membership: Some((membership.team_id, membership.role)),
        })
    }

    #[must_use]
    pub fn is_platform_admin(&self) -> bool {
        self.platform_admin
    }

    /// The team a team admin is limited to; `None` for a platform admin.
    #[must_use]
    pub fn managed_team_id(&self) -> Option<Uuid> {
        if self.platform_admin {
            None
        } else {
            self.membership.map(|(team_id, _)| team_id)
        }
    }

    /// Adds the actor's access and team role to an audit entry.
    #[must_use]
    pub fn annotate(&self, event: AuditEvent) -> AuditEvent {
        let event = event.meta(
            "actor_access",
            if self.platform_admin {
                "platform_admin"
            } else {
                "team_admin"
            },
        );
        match self.membership {
            Some((team_id, role)) => event
                .meta("actor_team_id", team_id)
                .meta("actor_team_role", role.as_str()),
            None => event,
        }
    }

    /// Whether the caller sees `server` at all.
    #[must_use]
    pub fn can_view(&self, server: &ExternalMcpServerRecord) -> bool {
        match self.managed_team_id() {
            None => true,
            Some(team_id) => {
                server.ownership.owned_by(team_id)
                    || server.ownership.visibility == McpServerVisibility::Organisation
            }
        }
    }

    /// Whether the caller may change `server`, its grants and its credentials.
    #[must_use]
    pub fn can_manage(&self, server: &ExternalMcpServerRecord) -> bool {
        self.managed_team_id()
            .is_none_or(|team_id| server.ownership.owned_by(team_id))
    }
}

/// Ownership requested when a server is registered.
#[derive(Debug, Clone, Default)]
pub struct RequestedMcpServerOwnership {
    pub owner_team_id: Option<Uuid>,
    pub owner_contact: Option<String>,
    pub visibility: Option<McpServerVisibility>,
}

#[derive(Clone)]
pub struct McpOwnership<R> {
    repo: Arc<R>,
    publishing: McpOrganisationPublishing,
}

impl<R> McpOwnership<R>
where
    R: McpRegistryRepository + McpAccessRepository + IdentityRepository + AdminApiKeyRepository,
{
    #[must_use]
    pub fn new(repo: Arc<R>, publishing: McpOrganisationPublishing) -> Self {
        Self { repo, publishing }
    }

    /// Whether the caller may publish servers they manage to the organisation.
    #[must_use]
    pub fn can_publish(&self, scope: &McpAdminScope) -> bool {
        scope.is_platform_admin() || self.publishing == McpOrganisationPublishing::TeamAdmins
    }

    /// Loads a server the caller can see. A hidden server is reported as missing,
    /// so its existence does not leak to other teams.
    pub async fn visible_server(
        &self,
        scope: &McpAdminScope,
        mcp_server_id: Uuid,
    ) -> Result<ExternalMcpServerRecord, GatewayError> {
        self.repo
            .get_external_mcp_server(mcp_server_id)
            .await?
            .filter(|server| scope.can_view(server))
            .ok_or_else(|| server_not_found(mcp_server_id))
    }

    /// Loads a server the caller may change.
    pub async fn managed_server(
        &self,
        scope: &McpAdminScope,
        mcp_server_id: Uuid,
    ) -> Result<ExternalMcpServerRecord, GatewayError> {
        let server = self.visible_server(scope, mcp_server_id).await?;
        if scope.can_manage(&server) {
            Ok(server)
        } else {
            Err(forbidden())
        }
    }

    /// Resolves the ownership of a new server. A team admin always registers for
    /// their own team; the contact defaults to `actor_email`.
    pub async fn new_server_ownership(
        &self,
        scope: &McpAdminScope,
        requested: RequestedMcpServerOwnership,
        actor_email: &str,
    ) -> Result<McpServerOwnership, GatewayError> {
        let owner_team_id = match scope.managed_team_id() {
            Some(team_id) => {
                if requested
                    .owner_team_id
                    .is_some_and(|requested| requested != team_id)
                {
                    return Err(forbidden());
                }
                Some(team_id)
            }
            None => match requested.owner_team_id {
                Some(team_id) => Some(self.existing_team(team_id).await?),
                None => None,
            },
        };
        let visibility = requested.visibility.unwrap_or(match owner_team_id {
            Some(_) => McpServerVisibility::Team,
            None => McpServerVisibility::Organisation,
        });
        self.check_visibility_allowed(scope, owner_team_id, visibility)?;
        let owner_contact = normalize_owner_contact(requested.owner_contact)?
            .or_else(|| Some(actor_email.to_string()));
        Ok(McpServerOwnership {
            owner_team_id,
            owner_contact,
            visibility,
        })
    }

    /// Checks a visibility change on a server the caller manages.
    pub async fn authorize_visibility_change(
        &self,
        scope: &McpAdminScope,
        server: &ExternalMcpServerRecord,
        visibility: McpServerVisibility,
    ) -> Result<(), GatewayError> {
        if visibility == server.ownership.visibility {
            return Ok(());
        }
        self.check_visibility_allowed(scope, server.ownership.owner_team_id, visibility)?;
        if let (McpServerVisibility::Team, Some(team_id)) =
            (visibility, server.ownership.owner_team_id)
        {
            self.check_no_stranded_access(server.mcp_server_id, team_id)
                .await?;
        }
        Ok(())
    }

    /// Resolves a platform admin's ownership transfer. Moving a server to the
    /// platform publishes it to the organisation; moving it to a team must not
    /// strand access held outside that team.
    pub async fn transfer_ownership(
        &self,
        scope: &McpAdminScope,
        server: &ExternalMcpServerRecord,
        requested: RequestedMcpServerOwnership,
    ) -> Result<McpServerOwnership, GatewayError> {
        if !scope.is_platform_admin() {
            return Err(forbidden());
        }
        let owner_team_id = match requested.owner_team_id {
            Some(team_id) => Some(self.existing_team(team_id).await?),
            None => None,
        };
        let visibility = match (owner_team_id, requested.visibility) {
            (None, _) => McpServerVisibility::Organisation,
            (Some(_), Some(visibility)) => visibility,
            (Some(_), None) => server.ownership.visibility,
        };
        if requested.visibility == Some(McpServerVisibility::Team) && owner_team_id.is_none() {
            return Err(platform_server_team_visibility());
        }
        let owner_contact = match requested.owner_contact {
            Some(contact) => normalize_owner_contact(Some(contact))?,
            None => server.ownership.owner_contact.clone(),
        };
        if let (McpServerVisibility::Team, Some(team_id)) = (visibility, owner_team_id) {
            self.check_no_stranded_access(server.mcp_server_id, team_id)
                .await?;
        }
        Ok(McpServerOwnership {
            owner_team_id,
            owner_contact,
            visibility,
        })
    }

    /// Checks a tool grant before it is written. This is the one place grant
    /// writes are bounded by ownership; tag-based grants call
    /// [`Self::authorize_server_grant`] for each server a tag reaches.
    pub async fn authorize_grant(
        &self,
        scope: &McpAdminScope,
        subject: McpGrantSubject,
        target_kind: McpToolGrantTargetKind,
        target_id: Uuid,
    ) -> Result<(), GatewayError> {
        match target_kind {
            McpToolGrantTargetKind::Toolset => {
                if scope.is_platform_admin() {
                    Ok(())
                } else {
                    Err(forbidden())
                }
            }
            McpToolGrantTargetKind::Tool => {
                let server = self.tool_server(scope, target_id).await?;
                self.authorize_server_grant(scope, &server, subject).await
            }
        }
    }

    /// Checks that `subject` may be granted tools of `server` by the caller.
    pub async fn authorize_server_grant(
        &self,
        scope: &McpAdminScope,
        server: &ExternalMcpServerRecord,
        subject: McpGrantSubject,
    ) -> Result<(), GatewayError> {
        if !scope.can_manage(server) {
            return Err(forbidden());
        }
        let subject_team = self.subject_team(subject).await?;
        if let Some(team_id) = scope.managed_team_id()
            && subject_team != Some(team_id)
        {
            return Err(forbidden());
        }
        if server.ownership.visibility == McpServerVisibility::Team
            && subject_team != server.ownership.owner_team_id
        {
            return Err(GatewayError::InvalidRequest(format!(
                "MCP server `{}` is visible to its team only, so its tools can be granted only to that team's subjects",
                server.server_key
            )));
        }
        Ok(())
    }

    /// Checks a grant revoke. A team admin may revoke any grant on their own
    /// servers' tools, because revoking only removes access.
    pub async fn authorize_grant_revoke(
        &self,
        scope: &McpAdminScope,
        target_kind: McpToolGrantTargetKind,
        target_id: Uuid,
    ) -> Result<(), GatewayError> {
        match (target_kind, scope.is_platform_admin()) {
            (_, true) => Ok(()),
            (McpToolGrantTargetKind::Toolset, false) => Err(forbidden()),
            (McpToolGrantTargetKind::Tool, false) => {
                let server = self.tool_server(scope, target_id).await?;
                if scope.can_manage(&server) {
                    Ok(())
                } else {
                    Err(forbidden())
                }
            }
        }
    }

    /// Keeps the grants a team admin may see: those on their servers' tools.
    pub async fn visible_grants(
        &self,
        scope: &McpAdminScope,
        grants: Vec<McpToolGrantRecord>,
    ) -> Result<Vec<McpToolGrantRecord>, GatewayError> {
        let Some(team_id) = scope.managed_team_id() else {
            return Ok(grants);
        };
        let tool_ids = self.team_tool_ids(team_id).await?;
        Ok(grants
            .into_iter()
            .filter(|grant| {
                grant.target_kind == McpToolGrantTargetKind::Tool
                    && tool_ids.contains(&grant.target_id)
            })
            .collect())
    }

    /// Tool sets are granted organisation-wide, so they hold only tools of
    /// servers published to the organisation.
    pub async fn authorize_toolset_tools(&self, tool_ids: &[Uuid]) -> Result<(), GatewayError> {
        for tool_id in tool_ids {
            let Some(tool) = self.repo.get_external_mcp_tool(*tool_id).await? else {
                continue;
            };
            if let Some(server) = self
                .repo
                .get_external_mcp_server(tool.mcp_server_id)
                .await?
                && server.ownership.visibility == McpServerVisibility::Team
            {
                return Err(GatewayError::InvalidRequest(format!(
                    "tool `{}` belongs to MCP server `{}`, which is visible to its team only; publish the server to the organisation before adding its tools to a tool set",
                    tool.upstream_name, server.server_key
                )));
            }
        }
        Ok(())
    }

    /// Checks a credential binding write on a server the caller manages. Team
    /// admins set only their team's shared credential or one of its service
    /// accounts' credentials; personal credentials stay with each user.
    pub async fn authorize_credential_binding(
        &self,
        scope: &McpAdminScope,
        server: &ExternalMcpServerRecord,
        owner_scope_kind: McpUpstreamCredentialOwnerScopeKind,
        owner_team_id: Option<Uuid>,
        owner_service_account_id: Option<Uuid>,
    ) -> Result<(), GatewayError> {
        if !scope.can_manage(server) {
            return Err(forbidden());
        }
        let Some(team_id) = scope.managed_team_id() else {
            return Ok(());
        };
        let allowed = match owner_scope_kind {
            McpUpstreamCredentialOwnerScopeKind::Team => owner_team_id == Some(team_id),
            McpUpstreamCredentialOwnerScopeKind::ServiceAccount => match owner_service_account_id {
                Some(service_account_id) if owner_team_id == Some(team_id) => {
                    self.subject_team(McpGrantSubject {
                        subject_kind: McpToolGrantSubjectKind::ServiceAccount,
                        subject_id: service_account_id,
                    })
                    .await?
                        == Some(team_id)
                }
                _ => false,
            },
            McpUpstreamCredentialOwnerScopeKind::User => false,
        };
        if allowed { Ok(()) } else { Err(forbidden()) }
    }

    /// The team a grant subject belongs to: the team itself, a user's team, a
    /// service account's team, or the team behind an API key's owner.
    pub async fn subject_team(
        &self,
        subject: McpGrantSubject,
    ) -> Result<Option<Uuid>, GatewayError> {
        let subject_id = subject.subject_id;
        match subject.subject_kind {
            McpToolGrantSubjectKind::Team => self
                .existing_team(subject_id)
                .await
                .map(Some)
                .map_err(|_| subject_not_found(subject)),
            McpToolGrantSubjectKind::User => self.user_team(subject_id).await,
            McpToolGrantSubjectKind::ServiceAccount => {
                Ok(Some(self.service_account_team(subject_id).await?))
            }
            McpToolGrantSubjectKind::ApiKey => {
                let key = AdminApiKeyRepository::get_api_key_by_id(&*self.repo, subject_id)
                    .await?
                    .ok_or_else(|| subject_not_found(subject))?;
                if let Some(team_id) = key.owner_team_id {
                    Ok(Some(team_id))
                } else if let Some(service_account_id) = key.owner_service_account_id {
                    Ok(Some(self.service_account_team(service_account_id).await?))
                } else if let Some(user_id) = key.owner_user_id {
                    self.user_team(user_id).await
                } else {
                    Ok(None)
                }
            }
        }
    }

    /// Rejects narrowing a server to `team_id` while access is held outside it:
    /// a tool in a tool set, or an active grant to a subject of another team.
    async fn check_no_stranded_access(
        &self,
        mcp_server_id: Uuid,
        team_id: Uuid,
    ) -> Result<(), GatewayError> {
        let tool_ids: HashSet<Uuid> = self
            .repo
            .list_external_mcp_tools(mcp_server_id, true)
            .await?
            .into_iter()
            .map(|tool| tool.mcp_tool_id)
            .collect();
        if tool_ids.is_empty() {
            return Ok(());
        }
        for toolset in self.repo.list_mcp_toolsets(true).await? {
            let members = self.repo.list_mcp_toolset_tools(toolset.toolset_id).await?;
            if members
                .iter()
                .any(|member| tool_ids.contains(&member.mcp_tool_id))
            {
                return Err(conflict(format!(
                    "tool set `{}` contains this server's tools; remove them before limiting the server to one team",
                    toolset.toolset_key
                )));
            }
        }
        let mut outside = 0_usize;
        for grant in self.repo.list_mcp_tool_grants(None, None).await? {
            if !grant.is_active
                || grant.target_kind != McpToolGrantTargetKind::Tool
                || !tool_ids.contains(&grant.target_id)
            {
                continue;
            }
            let subject = McpGrantSubject {
                subject_kind: grant.subject_kind,
                subject_id: grant.subject_id,
            };
            // A subject that no longer exists holds no access.
            match self.subject_team(subject).await {
                Ok(subject_team) if subject_team != Some(team_id) => outside += 1,
                _ => {}
            }
        }
        if outside > 0 {
            return Err(conflict(format!(
                "{outside} active grant(s) on this server's tools are held outside the owning team; revoke them before limiting the server to one team"
            )));
        }
        Ok(())
    }

    fn check_visibility_allowed(
        &self,
        scope: &McpAdminScope,
        owner_team_id: Option<Uuid>,
        visibility: McpServerVisibility,
    ) -> Result<(), GatewayError> {
        match visibility {
            McpServerVisibility::Team if owner_team_id.is_none() => {
                Err(platform_server_team_visibility())
            }
            McpServerVisibility::Organisation if !self.can_publish(scope) => {
                Err(GatewayError::Auth(AuthError::InsufficientPrivileges))
            }
            _ => Ok(()),
        }
    }

    async fn tool_server(
        &self,
        scope: &McpAdminScope,
        mcp_tool_id: Uuid,
    ) -> Result<ExternalMcpServerRecord, GatewayError> {
        let tool = self
            .repo
            .get_external_mcp_tool(mcp_tool_id)
            .await?
            .ok_or_else(|| tool_not_found(mcp_tool_id))?;
        self.visible_server(scope, tool.mcp_server_id)
            .await
            .map_err(|_| tool_not_found(mcp_tool_id))
    }

    async fn team_tool_ids(&self, team_id: Uuid) -> Result<HashSet<Uuid>, GatewayError> {
        let mut tool_ids = HashSet::new();
        for server in self.repo.list_external_mcp_servers(true).await? {
            if server.ownership.owned_by(team_id) {
                tool_ids.extend(
                    self.repo
                        .list_external_mcp_tools(server.mcp_server_id, true)
                        .await?
                        .into_iter()
                        .map(|tool| tool.mcp_tool_id),
                );
            }
        }
        Ok(tool_ids)
    }

    async fn existing_team(&self, team_id: Uuid) -> Result<Uuid, GatewayError> {
        IdentityRepository::get_team_by_id(&*self.repo, team_id)
            .await?
            .map(|team| team.team_id)
            .ok_or_else(|| GatewayError::InvalidRequest(format!("team `{team_id}` not found")))
    }

    async fn user_team(&self, user_id: Uuid) -> Result<Option<Uuid>, GatewayError> {
        if IdentityRepository::get_user_by_id(&*self.repo, user_id)
            .await?
            .is_none()
        {
            return Err(subject_not_found(McpGrantSubject {
                subject_kind: McpToolGrantSubjectKind::User,
                subject_id: user_id,
            }));
        }
        Ok(self
            .repo
            .get_team_membership_for_user(user_id)
            .await?
            .map(|membership| membership.team_id))
    }

    async fn service_account_team(&self, service_account_id: Uuid) -> Result<Uuid, GatewayError> {
        IdentityRepository::get_service_account_by_id(&*self.repo, service_account_id)
            .await?
            .map(|service_account| service_account.team_id)
            .ok_or_else(|| {
                subject_not_found(McpGrantSubject {
                    subject_kind: McpToolGrantSubjectKind::ServiceAccount,
                    subject_id: service_account_id,
                })
            })
    }
}

/// Trims an owner contact; blank means none.
pub fn normalize_owner_contact(contact: Option<String>) -> Result<Option<String>, GatewayError> {
    let Some(contact) = contact else {
        return Ok(None);
    };
    let contact = contact.trim();
    if contact.is_empty() {
        return Ok(None);
    }
    if contact.chars().count() > MAX_OWNER_CONTACT_CHARS {
        return Err(GatewayError::InvalidRequest(format!(
            "owner_contact must be at most {MAX_OWNER_CONTACT_CHARS} characters"
        )));
    }
    Ok(Some(contact.to_string()))
}

pub fn parse_mcp_server_visibility(value: &str) -> Result<McpServerVisibility, GatewayError> {
    McpServerVisibility::from_db(value).ok_or_else(|| {
        GatewayError::InvalidRequest("visibility must be `team` or `organisation`".to_string())
    })
}

fn forbidden() -> GatewayError {
    GatewayError::Auth(AuthError::InsufficientPrivileges)
}

fn conflict(message: String) -> GatewayError {
    GatewayError::Store(StoreError::Conflict(message))
}

fn platform_server_team_visibility() -> GatewayError {
    GatewayError::InvalidRequest(
        "a platform-owned MCP server is always visible to the organisation; assign an owning team first".to_string(),
    )
}

fn server_not_found(mcp_server_id: Uuid) -> GatewayError {
    GatewayError::Store(StoreError::NotFound(format!(
        "external MCP server `{mcp_server_id}` not found"
    )))
}

fn tool_not_found(mcp_tool_id: Uuid) -> GatewayError {
    GatewayError::Store(StoreError::NotFound(format!(
        "MCP tool `{mcp_tool_id}` not found"
    )))
}

fn subject_not_found(subject: McpGrantSubject) -> GatewayError {
    GatewayError::InvalidRequest(format!(
        "grant subject {} `{}` not found",
        subject.subject_kind.as_str(),
        subject.subject_id
    ))
}

#[cfg(test)]
#[path = "mcp_ownership_tests.rs"]
mod tests;
