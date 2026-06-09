use std::{collections::HashSet, sync::Arc};

use gateway_core::{
    AuthenticatedApiKey, ExternalMcpToolRecord, GatewayError, IdentityRepository,
    McpAccessRepository, McpAccessResolution, McpGrantSubject, McpToolGrantSubjectKind,
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
    ) -> Result<McpAccessResolution, GatewayError> {
        let subjects = self.grant_subjects(auth).await?;
        Ok(self
            .repo
            .resolve_mcp_access_for_subjects(&subjects, mcp_server_id)
            .await?)
    }

    pub async fn allowed_tool_for_call(
        &self,
        auth: &AuthenticatedApiKey,
        mcp_server_id: Uuid,
        upstream_name: &str,
    ) -> Result<Option<ExternalMcpToolRecord>, GatewayError> {
        let resolution = self
            .effective_tools_for_api_key(auth, Some(mcp_server_id))
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

#[must_use]
pub fn grant_subjects(auth: &AuthenticatedApiKey) -> Vec<McpGrantSubject> {
    let mut subjects = Vec::new();
    let mut seen = HashSet::new();
    push_subject(
        &mut subjects,
        &mut seen,
        McpToolGrantSubjectKind::ApiKey,
        auth.id,
    );
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
