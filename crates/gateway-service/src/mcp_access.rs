use std::{collections::HashSet, sync::Arc};

use gateway_core::{
    AdminApiKeyRepository, ApiKeyAccess, ApiKeyRepository, AuthenticatedApiKey, BudgetRepository,
    ExternalMcpToolRecord, GatewayError, IdentityRepository, McpAccessRepository,
    McpAccessResolution, McpGrantSubject, McpToolGrantSubjectKind, McpToolProjection,
    SCOPE_MCP_AGGREGATE, SCOPE_MCP_PROXY, ServiceAccountStatus,
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

    pub async fn effective_tools_for_api_key(
        &self,
        auth: &AuthenticatedApiKey,
        mcp_server_id: Option<Uuid>,
        projection: McpToolProjection,
    ) -> Result<McpAccessResolution, GatewayError> {
        let subjects = self.grant_subjects(auth).await?;
        Ok(self
            .repo
            .resolve_mcp_access_for_subjects(&subjects, mcp_server_id, projection)
            .await?)
    }

    /// Whether the key may reach the server at all: it holds a grant on at
    /// least one of the server's active tools.
    pub async fn has_server_grant(
        &self,
        auth: &AuthenticatedApiKey,
        mcp_server_id: Uuid,
    ) -> Result<bool, GatewayError> {
        let subjects = self.grant_subjects(auth).await?;
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
        let subjects = self.grant_subjects(auth).await?;
        Ok(self
            .repo
            .resolve_mcp_tool_for_subjects(&subjects, mcp_server_id, upstream_name)
            .await?)
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
        let mut subjects = grant_subjects(auth);
        if auth.owner_team_id.is_none()
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
        }
        Ok(subjects)
    }
}

impl<R> McpAccess<R>
where
    R: IdentityRepository + AdminApiKeyRepository + ApiKeyRepository + BudgetRepository,
{
    /// Expands previewed subjects the way runtime resolution would for a
    /// caller: a key brings its owner and team, a user its current team, and a
    /// service account its team. Rules and grants on those subjects then show in
    /// the preview, as they apply to real requests.
    pub async fn preview_subjects(
        &self,
        selected: &[McpGrantSubject],
    ) -> Result<McpAccessPreview, GatewayError> {
        let mut subjects = Vec::new();
        let mut key_access = Vec::new();
        let mut seen = HashSet::new();
        for subject in selected {
            push_subject(
                &mut subjects,
                &mut seen,
                subject.subject_kind,
                subject.subject_id,
            );
            match subject.subject_kind {
                McpToolGrantSubjectKind::ApiKey => {
                    if let Some(key) = self.repo.get_api_key_by_id(subject.subject_id).await? {
                        // A key that cannot authenticate has no access to
                        // preview: the same checks as authentication apply.
                        crate::authenticator::ensure_api_key_usable(self.repo.as_ref(), &key)
                            .await
                            .map_err(|error| {
                                GatewayError::InvalidRequest(format!(
                                    "the API key cannot authenticate, so it has no MCP access: {error}"
                                ))
                            })?;
                        let auth = AuthenticatedApiKey::from(key);
                        key_access.push(auth.access.clone());
                        for owned in self.grant_subjects(&auth).await? {
                            push_subject(
                                &mut subjects,
                                &mut seen,
                                owned.subject_kind,
                                owned.subject_id,
                            );
                        }
                    }
                }
                McpToolGrantSubjectKind::User => {
                    if let Some(membership) = self
                        .repo
                        .get_team_membership_for_user(subject.subject_id)
                        .await?
                    {
                        push_subject(
                            &mut subjects,
                            &mut seen,
                            McpToolGrantSubjectKind::Team,
                            membership.team_id,
                        );
                    }
                }
                McpToolGrantSubjectKind::ServiceAccount => {
                    if let Some(account) = IdentityRepository::get_service_account_by_id(
                        self.repo.as_ref(),
                        subject.subject_id,
                    )
                    .await?
                    {
                        // A disabled account's keys cannot authenticate.
                        if account.status != ServiceAccountStatus::Active {
                            return Err(GatewayError::InvalidRequest(
                                "the service account is disabled, so it has no MCP access"
                                    .to_string(),
                            ));
                        }
                        push_subject(
                            &mut subjects,
                            &mut seen,
                            McpToolGrantSubjectKind::Team,
                            account.team_id,
                        );
                    }
                }
                McpToolGrantSubjectKind::Team => {}
            }
        }
        Ok(McpAccessPreview {
            subjects,
            key_access,
        })
    }
}

/// The subjects a preview resolves, and the scope limits of any previewed API
/// key, which runtime applies on top of grants and rules.
#[derive(Debug, Clone, Default)]
pub struct McpAccessPreview {
    pub subjects: Vec<McpGrantSubject>,
    pub key_access: Vec<ApiKeyAccess>,
}

impl McpAccessPreview {
    /// Whether every previewed key may reach the server through MCP: it has
    /// an MCP scope (or no scope list) and the server is in its server list.
    #[must_use]
    pub fn allows_server(&self, server_key: &str) -> bool {
        self.key_access.iter().all(|access| {
            (access.has_scope(SCOPE_MCP_AGGREGATE) || access.has_scope(SCOPE_MCP_PROXY))
                && access.allows_mcp_server(server_key)
        })
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

#[cfg(test)]
mod preview_tests {
    use super::*;

    fn access(scopes: Option<&[&str]>, servers: Option<&[&str]>) -> ApiKeyAccess {
        ApiKeyAccess::new(
            scopes.map(|scopes| scopes.iter().map(|scope| (*scope).to_string()).collect()),
            servers.map(|servers| servers.iter().map(|server| (*server).to_string()).collect()),
        )
        .expect("access")
    }

    #[test]
    fn key_scopes_limit_previewed_servers() {
        let open = McpAccessPreview::default();
        assert!(open.allows_server("github"), "no previewed key, no limit");

        let limited = McpAccessPreview {
            subjects: Vec::new(),
            key_access: vec![access(Some(&[SCOPE_MCP_PROXY]), Some(&["notes"]))],
        };
        assert!(limited.allows_server("notes"));
        assert!(!limited.allows_server("github"));

        let inference_only = McpAccessPreview {
            subjects: Vec::new(),
            key_access: vec![access(Some(&[gateway_core::SCOPE_INFERENCE]), None)],
        };
        assert!(
            !inference_only.allows_server("notes"),
            "no MCP scope, no tools"
        );
    }
}
