use std::{collections::HashSet, sync::Arc};

use gateway_core::{
    AdminApiKeyRepository, AuthenticatedApiKey, ExternalMcpToolRecord, GatewayError,
    IdentityRepository, McpAccessRepository, McpAccessResolution, McpGrantSubject,
    McpToolGrantSubjectKind, McpToolProjection,
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
    R: IdentityRepository + AdminApiKeyRepository,
{
    /// Expands previewed subjects the way runtime resolution would for a
    /// caller: a key brings its owner and team, a user its current team, and a
    /// service account its team. Rules and grants on those subjects then show in
    /// the preview, as they apply to real requests.
    pub async fn preview_subjects(
        &self,
        selected: &[McpGrantSubject],
    ) -> Result<Vec<McpGrantSubject>, GatewayError> {
        let mut subjects = Vec::new();
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
                        let auth = AuthenticatedApiKey {
                            id: key.id,
                            public_id: key.public_id,
                            name: key.name,
                            model_grant_mode: key.model_grant_mode,
                            owner_kind: key.owner_kind,
                            owner_user_id: key.owner_user_id,
                            owner_team_id: key.owner_team_id,
                            owner_service_account_id: key.owner_service_account_id,
                            access: key.access,
                            oauth: None,
                        };
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
                    if let Some(account) = self
                        .repo
                        .get_service_account_by_id(subject.subject_id)
                        .await?
                    {
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
        Ok(subjects)
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
