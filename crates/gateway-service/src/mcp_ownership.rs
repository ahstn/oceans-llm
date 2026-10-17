//! Who may administer an MCP server, and who may be granted its tools.
//!
//! Every MCP admin check that depends on ownership goes through [`McpOwnership`],
//! so tool grants, tag-based grants and tool approvals agree on the boundary.
//! See `docs/adr/2026-10-17-delegated-mcp-server-ownership.md`.

use std::{net::IpAddr, sync::Arc};

use gateway_core::{
    AdminApiKeyRepository, AuthError, ExternalMcpServerRecord, GatewayError, IdentityRepository,
    McpAccessRepository, McpGrantSubject, McpRegistryRepository, McpServerOwnership,
    McpServerVisibility, McpToolGrantRecord, McpToolGrantSubjectKind, McpToolGrantTargetKind,
    McpUpstreamCredentialOwnerScopeKind, McpUpstreamCredentialRepository, MembershipRole,
    StoreError, TeamMembershipRecord,
};
use serde::Deserialize;
use serde_json::{Map, Value};
use url::Url;
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
    R: McpRegistryRepository
        + McpAccessRepository
        + IdentityRepository
        + AdminApiKeyRepository
        + McpUpstreamCredentialRepository,
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

    /// Checks a team admin pointing a server at a new URL. Credentials the team
    /// did not set itself (users' own, another team's, or a `secret_ref` to a
    /// gateway-wide variable) would be sent to the new destination, so they must
    /// be revoked first.
    pub async fn authorize_retarget(
        &self,
        scope: &McpAdminScope,
        server: &ExternalMcpServerRecord,
        server_url: &str,
    ) -> Result<(), GatewayError> {
        if scope.is_platform_admin() || server_url.trim() == server.server_url {
            return Ok(());
        }
        let foreign = self
            .repo
            .list_mcp_upstream_credential_bindings(Some(server.mcp_server_id), None, None, false)
            .await?
            .into_iter()
            // Only the current owner's own team and service-account credentials,
            // stored as encrypted secrets, may follow the server to a new URL.
            .filter(|binding| {
                binding.owner_scope_kind == McpUpstreamCredentialOwnerScopeKind::User
                    || binding.secret_ref.is_some()
                    || binding.owner_team_id != server.ownership.owner_team_id
            })
            .count();
        if foreign > 0 {
            return Err(conflict(format!(
                "{foreign} user or secret-reference credential binding(s) would be sent to the new URL; revoke them before changing server_url"
            )));
        }
        Ok(())
    }

    /// Re-runs the narrowing check after a write that limited a server to
    /// `team_id`. Each side of a race between a grant and a transfer checks
    /// after its own write, so whichever lands second sees the other.
    pub async fn confirm_no_stranded_access(
        &self,
        mcp_server_id: Uuid,
        team_id: Uuid,
    ) -> Result<(), GatewayError> {
        self.check_no_stranded_access(mcp_server_id, team_id).await
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
    ///
    /// Returns the server a tool grant was authorized against and the
    /// subject's team at that moment. The caller passes both to the store (a
    /// [`gateway_core::McpServerOwnershipGuard`] and the expected subject team),
    /// so the grant is written only while neither has changed.
    pub async fn authorize_grant(
        &self,
        scope: &McpAdminScope,
        subject: McpGrantSubject,
        target_kind: McpToolGrantTargetKind,
        target_id: Uuid,
    ) -> Result<Option<(ExternalMcpServerRecord, Option<Uuid>)>, GatewayError> {
        match target_kind {
            McpToolGrantTargetKind::Toolset => {
                if scope.is_platform_admin() {
                    Ok(None)
                } else {
                    Err(forbidden())
                }
            }
            McpToolGrantTargetKind::Tool => {
                let server = self.tool_server(scope, target_id).await?;
                let subject_team = self.authorize_server_grant(scope, &server, subject).await?;
                Ok(Some((server, subject_team)))
            }
        }
    }

    /// Checks that `subject` may be granted tools of `server` by the caller, and
    /// returns the subject's team the check relied on.
    pub async fn authorize_server_grant(
        &self,
        scope: &McpAdminScope,
        server: &ExternalMcpServerRecord,
        subject: McpGrantSubject,
    ) -> Result<Option<Uuid>, GatewayError> {
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
        Ok(subject_team)
    }

    /// Checks a grant revoke. A team admin may revoke any grant on their own
    /// servers' tools, because revoking only removes access.
    ///
    /// For a team admin, returns the server the revoke was authorized against,
    /// so the caller can make the store revoke conditional on it.
    pub async fn authorize_grant_revoke(
        &self,
        scope: &McpAdminScope,
        target_kind: McpToolGrantTargetKind,
        target_id: Uuid,
    ) -> Result<Option<ExternalMcpServerRecord>, GatewayError> {
        match (target_kind, scope.is_platform_admin()) {
            (_, true) => Ok(None),
            (McpToolGrantTargetKind::Toolset, false) => Err(forbidden()),
            (McpToolGrantTargetKind::Tool, false) => {
                let server = self.tool_server(scope, target_id).await?;
                if scope.can_manage(&server) {
                    Ok(Some(server))
                } else {
                    Err(forbidden())
                }
            }
        }
    }

    /// The grants the caller may see, optionally for one subject. A team admin
    /// sees only grants on their team's servers' tools, read in one query.
    pub async fn list_grants(
        &self,
        scope: &McpAdminScope,
        subject_kind: Option<McpToolGrantSubjectKind>,
        subject_id: Option<Uuid>,
    ) -> Result<Vec<McpToolGrantRecord>, GatewayError> {
        let Some(team_id) = scope.managed_team_id() else {
            return Ok(self
                .repo
                .list_mcp_tool_grants(subject_kind, subject_id)
                .await?);
        };
        Ok(self
            .repo
            .list_mcp_tool_grants_for_owner_team(team_id, subject_kind, subject_id)
            .await?)
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
        uses_secret_ref: bool,
    ) -> Result<(), GatewayError> {
        if !scope.can_manage(server) {
            return Err(forbidden());
        }
        let Some(team_id) = scope.managed_team_id() else {
            return Ok(());
        };
        // A secret reference names a process-wide environment variable, which
        // could hold another team's or the platform's credential. Team admins
        // store their own secret, encrypted, instead.
        if uses_secret_ref {
            return Err(GatewayError::InvalidRequest(
                "team admins store a credential's secret directly; secret_ref is for platform admins"
                    .to_string(),
            ));
        }
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
    /// service account's team, or the team behind an API key's owner. A subject
    /// that does not exist is an invalid request.
    pub async fn subject_team(
        &self,
        subject: McpGrantSubject,
    ) -> Result<Option<Uuid>, GatewayError> {
        let missing = subject_not_found(subject.clone());
        self.resolve_subject_team(subject).await?.ok_or(missing)
    }

    /// Like [`Self::subject_team`], but `Ok(None)` when the subject, or the user
    /// or service account behind an API key, no longer exists. Store failures are
    /// returned as errors, never treated as a missing subject.
    async fn resolve_subject_team(
        &self,
        subject: McpGrantSubject,
    ) -> Result<Option<Option<Uuid>>, GatewayError> {
        let subject_id = subject.subject_id;
        match subject.subject_kind {
            McpToolGrantSubjectKind::Team => {
                Ok(IdentityRepository::get_team_by_id(&*self.repo, subject_id)
                    .await?
                    .map(|team| Some(team.team_id)))
            }
            McpToolGrantSubjectKind::User => self.resolve_user_team(subject_id).await,
            McpToolGrantSubjectKind::ServiceAccount => {
                self.resolve_service_account_team(subject_id).await
            }
            McpToolGrantSubjectKind::ApiKey => {
                let Some(key) =
                    AdminApiKeyRepository::get_api_key_by_id(&*self.repo, subject_id).await?
                else {
                    return Ok(None);
                };
                if let Some(team_id) = key.owner_team_id {
                    Ok(Some(Some(team_id)))
                } else if let Some(service_account_id) = key.owner_service_account_id {
                    self.resolve_service_account_team(service_account_id).await
                } else if let Some(user_id) = key.owner_user_id {
                    self.resolve_user_team(user_id).await
                } else {
                    Ok(Some(None))
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
        // Both reads are scoped to this server's tools, not the whole organisation.
        if let Some(toolset_key) = self
            .repo
            .first_active_mcp_toolset_key_for_server(mcp_server_id)
            .await?
        {
            return Err(conflict(format!(
                "tool set `{toolset_key}` contains this server's tools; remove them before limiting the server to one team"
            )));
        }
        // One query resolves every grant subject's team; a subject that no
        // longer exists holds no access and is not counted.
        let outside = self
            .repo
            .count_mcp_tool_grants_outside_team(mcp_server_id, team_id)
            .await?;
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
        // A server the caller cannot see hides its tools too. Any other
        // failure, such as a store outage, is reported as it is.
        self.visible_server(scope, tool.mcp_server_id)
            .await
            .map_err(|error| match error {
                GatewayError::Store(StoreError::NotFound(_)) => tool_not_found(mcp_tool_id),
                other => other,
            })
    }

    async fn existing_team(&self, team_id: Uuid) -> Result<Uuid, GatewayError> {
        IdentityRepository::get_team_by_id(&*self.repo, team_id)
            .await?
            .map(|team| team.team_id)
            .ok_or_else(|| GatewayError::InvalidRequest(format!("team `{team_id}` not found")))
    }

    async fn resolve_user_team(&self, user_id: Uuid) -> Result<Option<Option<Uuid>>, GatewayError> {
        if IdentityRepository::get_user_by_id(&*self.repo, user_id)
            .await?
            .is_none()
        {
            return Ok(None);
        }
        Ok(Some(
            self.repo
                .get_team_membership_for_user(user_id)
                .await?
                .map(|membership| membership.team_id),
        ))
    }

    async fn resolve_service_account_team(
        &self,
        service_account_id: Uuid,
    ) -> Result<Option<Option<Uuid>>, GatewayError> {
        Ok(
            IdentityRepository::get_service_account_by_id(&*self.repo, service_account_id)
                .await?
                .map(|service_account| Some(service_account.team_id)),
        )
    }
}

/// Rejects a server URL a team admin may not point the gateway at: one whose
/// host is, or resolves to, a loopback, private, link-local, shared or other
/// non-public address, or that does not resolve at all. Platform admins
/// register internal servers, including for a team.
///
/// This runs when a team admin saves a URL or refreshes discovery. It does not
/// pin the address for the later connection, so on its own it is not a defence
/// against DNS rebinding. A team-owned server's OAuth requests, discovery
/// refresh and proxy traffic go through the outbound fetcher's guarded
/// resolver, which is (see [`crate::McpUpstreamClients`]).
pub async fn authorize_destination(
    scope: &McpAdminScope,
    server_url: &str,
) -> Result<(), GatewayError> {
    if scope.is_platform_admin() {
        return Ok(());
    }
    let url = Url::parse(server_url.trim())
        .map_err(|error| GatewayError::InvalidRequest(format!("server_url is invalid: {error}")))?;
    require_public_destination(&url).await
}

/// Rejects a URL whose host is, or resolves to, a non-public address, or does
/// not resolve at all: an unresolved name could resolve to an internal address
/// by the time the gateway connects.
pub async fn require_public_destination(url: &Url) -> Result<(), GatewayError> {
    let blocked = || {
        GatewayError::InvalidRequest(format!(
            "`{}` must reach a public address; ask a platform admin to register servers on internal networks",
            url.host_str().unwrap_or_default()
        ))
    };
    let port = url.port_or_known_default().unwrap_or(443);
    let addresses: Vec<IpAddr> = match url.host() {
        Some(url::Host::Ipv4(address)) => vec![IpAddr::V4(address)],
        Some(url::Host::Ipv6(address)) => vec![IpAddr::V6(address)],
        Some(url::Host::Domain(domain)) => {
            let domain = domain.trim_end_matches('.').to_ascii_lowercase();
            if domain == "localhost" || domain.ends_with(".localhost") {
                return Err(blocked());
            }
            let resolved: Vec<IpAddr> = tokio::net::lookup_host((domain.as_str(), port))
                .await
                .map(|resolved| resolved.map(|address| address.ip()).collect())
                .unwrap_or_default();
            if resolved.is_empty() {
                return Err(GatewayError::InvalidRequest(format!(
                    "`{domain}` does not resolve; a server must reach a public address"
                )));
            }
            resolved
        }
        None => return Err(blocked()),
    };
    if addresses.into_iter().all(is_public_address) {
        Ok(())
    } else {
        Err(blocked())
    }
}

/// Checks a team admin's server auth config against the config it replaces
/// (`None` for a new server). A team admin may not add or retarget a gateway
/// secret reference (`env/...`), because the gateway would resolve that
/// process-wide secret and send it to a server the team controls; nor add a
/// non-public URL, such as a token endpoint the gateway calls. Values a
/// platform admin set are exempt only while they, and the server URL, stay the
/// same. An inherited secret reference also needs every URL in the config to
/// stay the same, since the gateway sends that secret to a token endpoint the
/// config names.
pub async fn authorize_team_auth_config(
    scope: &McpAdminScope,
    auth_config: &Map<String, Value>,
    previous: Option<(&Map<String, Value>, &str)>,
    server_url: &str,
    clients: &crate::McpUpstreamClients,
) -> Result<(), GatewayError> {
    if scope.is_platform_admin() {
        return Ok(());
    }
    let mut values = Vec::new();
    collect_strings("", &Value::Object(auth_config.clone()), &mut values);
    let mut previous_values = Vec::new();
    if let Some((previous, _)) = previous {
        collect_strings("", &Value::Object(previous.clone()), &mut previous_values);
    }
    let url_unchanged = previous.is_some_and(|(_, url)| url == server_url.trim());
    let config_urls = |values: &[(String, String)]| {
        values
            .iter()
            .filter(|(_, value)| is_http_url(value))
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
    };
    let config_urls_unchanged = config_urls(&values) == config_urls(&previous_values);
    for (path, value) in values {
        let inherited = previous_values.contains(&(path.clone(), value.clone()));
        if value.starts_with("env/") {
            if !(inherited && url_unchanged && config_urls_unchanged) {
                return Err(GatewayError::Auth(AuthError::InsufficientPrivileges));
            }
            continue;
        }
        // An inherited URL, such as an internal token endpoint a platform admin
        // set, is exempt only while the server keeps its URL; once the team
        // points the server elsewhere, every URL is checked again.
        if !(inherited && url_unchanged)
            && is_http_url(&value)
            && let Ok(url) = Url::parse(&value)
        {
            refuse_private_network_host(scope, clients, &value)?;
            require_public_destination(&url).await?;
        }
    }
    Ok(())
}

/// Refuses a team admin's URL on a host listed under
/// `mcp.oauth.private_network_hosts`. That list lets a name reach private
/// networks whatever it resolves to, so only a platform admin may point a
/// server, or an endpoint it calls, at one; a URL a platform admin set stays
/// usable while it is unchanged.
pub fn refuse_private_network_host(
    scope: &McpAdminScope,
    clients: &crate::McpUpstreamClients,
    url: &str,
) -> Result<(), GatewayError> {
    if scope.is_platform_admin() || !clients.is_private_network_host(url.trim()) {
        return Ok(());
    }
    Err(GatewayError::InvalidRequest(
        "this host is listed under mcp.oauth.private_network_hosts; a platform admin registers servers on it"
            .to_string(),
    ))
}

fn is_http_url(value: &str) -> bool {
    Url::parse(value).is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
}

fn collect_strings(path: &str, value: &Value, out: &mut Vec<(String, String)>) {
    match value {
        Value::String(text) => out.push((path.to_string(), text.trim().to_string())),
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                collect_strings(&format!("{path}/{index}"), item, out);
            }
        }
        Value::Object(fields) => {
            for (key, item) in fields {
                collect_strings(&format!("{path}/{key}"), item, out);
            }
        }
        _ => {}
    }
}

/// Whether an address is reachable on the public internet. This is the
/// outbound fetcher's own rule, so a URL a team admin saves is judged the same
/// way the fetcher judges it at connect time.
#[must_use]
pub fn is_public_address(address: IpAddr) -> bool {
    crate::outbound_fetch::is_public_address(address)
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
