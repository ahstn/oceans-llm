use std::{collections::BTreeSet, fmt};

use anyhow::{Result, bail};
use gateway_core::{GlobalRole, MembershipRole};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, ToSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum AdminPage {
    ApiKeys,
    Models,
    Mcp,
    ReviewAgent,
    UsageCosts,
    SpendControls,
    Leaderboard,
    AgentHarnesses,
    AgentSessions,
    RequestLogs,
    McpInvocations,
    Teams,
    Users,
    ServiceAccounts,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, ToSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum AdminAction {
    CreateApiKey,
    UpdateApiKey,
    RevokeApiKey,
    RevealApiKey,
}

impl fmt::Display for AdminAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::CreateApiKey => "create_api_key",
            Self::UpdateApiKey => "update_api_key",
            Self::RevokeApiKey => "revoke_api_key",
            Self::RevealApiKey => "reveal_api_key",
        })
    }
}

impl fmt::Display for AdminPage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ApiKeys => "api_keys",
            Self::Models => "models",
            Self::Mcp => "mcp",
            Self::ReviewAgent => "review_agent",
            Self::UsageCosts => "usage_costs",
            Self::SpendControls => "spend_controls",
            Self::Leaderboard => "leaderboard",
            Self::AgentHarnesses => "agent_harnesses",
            Self::AgentSessions => "agent_sessions",
            Self::RequestLogs => "request_logs",
            Self::McpInvocations => "mcp_invocations",
            Self::Teams => "teams",
            Self::Users => "users",
            Self::ServiceAccounts => "service_accounts",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AdminPermissionGroup {
    PlatformAdmins,
    TeamAdmins,
    Users,
}

impl AdminPermissionGroup {
    #[must_use]
    pub fn for_user(global_role: GlobalRole, membership_role: Option<MembershipRole>) -> Self {
        if global_role == GlobalRole::PlatformAdmin {
            return Self::PlatformAdmins;
        }

        match membership_role {
            Some(MembershipRole::Owner | MembershipRole::Admin) => Self::TeamAdmins,
            Some(MembershipRole::Member) | None => Self::Users,
        }
    }
}

impl fmt::Display for AdminPermissionGroup {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::PlatformAdmins => "platform_admins",
            Self::TeamAdmins => "team_admins",
            Self::Users => "users",
        })
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PermissionsConfig {
    pub platform_admins: Option<PermissionSetConfig>,
    pub team_admins: Option<PermissionSetConfig>,
    pub users: Option<PermissionSetConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionSetConfig {
    pub pages: Option<Vec<AdminPage>>,
    pub actions: Option<Vec<AdminAction>>,
    pub default_page: Option<AdminPage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPermissionSet {
    pub pages: Vec<AdminPage>,
    pub actions: Vec<AdminAction>,
    pub default_page: Option<AdminPage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAdminPermissions {
    users: ResolvedPermissionSet,
    team_admins: ResolvedPermissionSet,
    platform_admins: ResolvedPermissionSet,
}

impl ResolvedAdminPermissions {
    #[must_use]
    pub fn for_group(&self, group: AdminPermissionGroup) -> &ResolvedPermissionSet {
        match group {
            AdminPermissionGroup::PlatformAdmins => &self.platform_admins,
            AdminPermissionGroup::TeamAdmins => &self.team_admins,
            AdminPermissionGroup::Users => &self.users,
        }
    }
}

impl PermissionsConfig {
    pub fn resolve(&self) -> Result<ResolvedAdminPermissions> {
        let user_direct = direct_pages(
            self.users.as_ref(),
            DEFAULT_USER_PAGES,
            AdminPermissionGroup::Users,
        )?;
        let team_admin_direct = direct_pages(
            self.team_admins.as_ref(),
            DEFAULT_TEAM_ADMIN_PAGES,
            AdminPermissionGroup::TeamAdmins,
        )?;
        let platform_admin_direct = direct_pages(
            self.platform_admins.as_ref(),
            DEFAULT_PLATFORM_ADMIN_PAGES,
            AdminPermissionGroup::PlatformAdmins,
        )?;
        let user_action_direct = direct_actions(
            self.users.as_ref(),
            DEFAULT_USER_ACTIONS,
            AdminPermissionGroup::Users,
        )?;
        let team_admin_action_direct = direct_actions(
            self.team_admins.as_ref(),
            DEFAULT_TEAM_ADMIN_ACTIONS,
            AdminPermissionGroup::TeamAdmins,
        )?;
        let platform_admin_action_direct = direct_actions(
            self.platform_admins.as_ref(),
            DEFAULT_PLATFORM_ADMIN_ACTIONS,
            AdminPermissionGroup::PlatformAdmins,
        )?;

        let users = resolve_group(
            &user_direct,
            &user_action_direct,
            self.users.as_ref().and_then(|group| group.default_page),
            AdminPage::UsageCosts,
            AdminPermissionGroup::Users,
        )?;
        let team_admin_pages = union_pages(&[&user_direct, &team_admin_direct]);
        let team_admin_actions = union_actions(&[&user_action_direct, &team_admin_action_direct]);
        let team_admins = resolve_group(
            &team_admin_pages,
            &team_admin_actions,
            self.team_admins
                .as_ref()
                .and_then(|group| group.default_page),
            AdminPage::UsageCosts,
            AdminPermissionGroup::TeamAdmins,
        )?;
        let platform_admin_pages =
            union_pages(&[&user_direct, &team_admin_direct, &platform_admin_direct]);
        let platform_admin_actions = union_actions(&[
            &user_action_direct,
            &team_admin_action_direct,
            &platform_admin_action_direct,
        ]);
        let platform_admins = resolve_group(
            &platform_admin_pages,
            &platform_admin_actions,
            self.platform_admins
                .as_ref()
                .and_then(|group| group.default_page),
            AdminPage::ApiKeys,
            AdminPermissionGroup::PlatformAdmins,
        )?;

        Ok(ResolvedAdminPermissions {
            users,
            team_admins,
            platform_admins,
        })
    }
}

const ADMIN_PAGE_ORDER: &[AdminPage] = &[
    AdminPage::ApiKeys,
    AdminPage::Models,
    AdminPage::Mcp,
    AdminPage::ReviewAgent,
    AdminPage::UsageCosts,
    AdminPage::SpendControls,
    AdminPage::Leaderboard,
    AdminPage::AgentHarnesses,
    AdminPage::AgentSessions,
    AdminPage::RequestLogs,
    AdminPage::McpInvocations,
    AdminPage::Teams,
    AdminPage::Users,
    AdminPage::ServiceAccounts,
];

const SHARED_PAGES: &[AdminPage] = &[
    AdminPage::ApiKeys,
    AdminPage::Models,
    AdminPage::UsageCosts,
    AdminPage::Leaderboard,
    AdminPage::AgentHarnesses,
    AdminPage::RequestLogs,
    AdminPage::McpInvocations,
    AdminPage::Teams,
    AdminPage::Users,
    AdminPage::ServiceAccounts,
];

const TEAM_ADMIN_PAGES: &[AdminPage] = &[
    AdminPage::ApiKeys,
    AdminPage::Models,
    AdminPage::UsageCosts,
    AdminPage::Leaderboard,
    AdminPage::AgentHarnesses,
    AdminPage::AgentSessions,
    AdminPage::RequestLogs,
    AdminPage::McpInvocations,
    AdminPage::Teams,
    AdminPage::Users,
    AdminPage::ServiceAccounts,
];

const DEFAULT_USER_PAGES: &[AdminPage] = SHARED_PAGES;
const DEFAULT_TEAM_ADMIN_PAGES: &[AdminPage] = &[AdminPage::AgentSessions];
const DEFAULT_PLATFORM_ADMIN_PAGES: &[AdminPage] = &[
    AdminPage::Mcp,
    AdminPage::ReviewAgent,
    AdminPage::SpendControls,
];

const ADMIN_ACTION_ORDER: &[AdminAction] = &[
    AdminAction::CreateApiKey,
    AdminAction::UpdateApiKey,
    AdminAction::RevokeApiKey,
    AdminAction::RevealApiKey,
];

const USER_ACTIONS: &[AdminAction] = &[
    AdminAction::CreateApiKey,
    AdminAction::UpdateApiKey,
    AdminAction::RevokeApiKey,
];

const DEFAULT_USER_ACTIONS: &[AdminAction] = USER_ACTIONS;
const DEFAULT_TEAM_ADMIN_ACTIONS: &[AdminAction] = &[AdminAction::RevealApiKey];
const DEFAULT_PLATFORM_ADMIN_ACTIONS: &[AdminAction] = &[];

fn direct_pages(
    config: Option<&PermissionSetConfig>,
    default_pages: &[AdminPage],
    group: AdminPermissionGroup,
) -> Result<Vec<AdminPage>> {
    let pages = config
        .and_then(|group| group.pages.as_deref())
        .unwrap_or(default_pages);
    let capability_ceiling = match group {
        AdminPermissionGroup::PlatformAdmins => ADMIN_PAGE_ORDER,
        AdminPermissionGroup::TeamAdmins => TEAM_ADMIN_PAGES,
        AdminPermissionGroup::Users => SHARED_PAGES,
    };

    if let Some(page) = pages.iter().find(|page| !capability_ceiling.contains(page)) {
        bail!("page `{page}` is not supported for permission group `{group}`");
    }

    Ok(normalize_pages(pages.iter().copied()))
}

fn direct_actions(
    config: Option<&PermissionSetConfig>,
    default_actions: &[AdminAction],
    group: AdminPermissionGroup,
) -> Result<Vec<AdminAction>> {
    let actions = config
        .and_then(|group| group.actions.as_deref())
        .unwrap_or(default_actions);
    let capability_ceiling = match group {
        AdminPermissionGroup::PlatformAdmins | AdminPermissionGroup::TeamAdmins => {
            ADMIN_ACTION_ORDER
        }
        AdminPermissionGroup::Users => USER_ACTIONS,
    };

    if let Some(action) = actions
        .iter()
        .find(|action| !capability_ceiling.contains(action))
    {
        bail!("action `{action}` is not supported for permission group `{group}`");
    }

    Ok(normalize_actions(actions.iter().copied()))
}

fn union_pages(page_sets: &[&[AdminPage]]) -> Vec<AdminPage> {
    normalize_pages(page_sets.iter().flat_map(|pages| pages.iter().copied()))
}

fn union_actions(action_sets: &[&[AdminAction]]) -> Vec<AdminAction> {
    normalize_actions(
        action_sets
            .iter()
            .flat_map(|actions| actions.iter().copied()),
    )
}

fn normalize_pages(pages: impl IntoIterator<Item = AdminPage>) -> Vec<AdminPage> {
    let pages = pages.into_iter().collect::<BTreeSet<_>>();
    ADMIN_PAGE_ORDER
        .iter()
        .filter(|page| pages.contains(page))
        .copied()
        .collect()
}

fn normalize_actions(actions: impl IntoIterator<Item = AdminAction>) -> Vec<AdminAction> {
    let actions = actions.into_iter().collect::<BTreeSet<_>>();
    ADMIN_ACTION_ORDER
        .iter()
        .filter(|action| actions.contains(action))
        .copied()
        .collect()
}

fn resolve_group(
    pages: &[AdminPage],
    actions: &[AdminAction],
    configured_default: Option<AdminPage>,
    preferred_default: AdminPage,
    group: AdminPermissionGroup,
) -> Result<ResolvedPermissionSet> {
    if let Some(default_page) = configured_default
        && !pages.contains(&default_page)
    {
        bail!("default page `{default_page}` is not available to permission group `{group}`");
    }

    let default_page = configured_default
        .or_else(|| {
            pages
                .contains(&preferred_default)
                .then_some(preferred_default)
        })
        .or_else(|| pages.first().copied());

    Ok(ResolvedPermissionSet {
        pages: pages.to_vec(),
        actions: actions.to_vec(),
        default_page,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(raw: &str) -> PermissionsConfig {
        serde_yaml::from_str(raw).expect("permissions config")
    }

    #[test]
    fn defaults_resolve_in_stable_order() {
        let resolved = PermissionsConfig::default().resolve().expect("permissions");

        assert_eq!(resolved.users.pages, SHARED_PAGES);
        assert_eq!(resolved.team_admins.pages, TEAM_ADMIN_PAGES);
        assert_eq!(resolved.platform_admins.pages, ADMIN_PAGE_ORDER);
        assert_eq!(resolved.users.actions, USER_ACTIONS);
        assert_eq!(resolved.team_admins.actions, ADMIN_ACTION_ORDER);
        assert_eq!(resolved.platform_admins.actions, ADMIN_ACTION_ORDER);
        assert_eq!(resolved.users.default_page, Some(AdminPage::UsageCosts));
        assert_eq!(
            resolved.platform_admins.default_page,
            Some(AdminPage::ApiKeys)
        );
    }

    #[test]
    fn higher_groups_inherit_and_deduplicate_lower_grants() {
        let config = parse(
            r#"
users:
  pages: [models, api_keys, api_keys]
  actions: [update_api_key, create_api_key, create_api_key]
team_admins:
  pages: [leaderboard, models]
  actions: [reveal_api_key, update_api_key]
platform_admins:
  pages: [mcp, leaderboard]
  actions: [revoke_api_key, reveal_api_key]
"#,
        );
        let resolved = config.resolve().expect("permissions");

        assert_eq!(
            resolved.users.pages,
            vec![AdminPage::ApiKeys, AdminPage::Models]
        );
        assert_eq!(
            resolved.team_admins.pages,
            vec![
                AdminPage::ApiKeys,
                AdminPage::Models,
                AdminPage::Leaderboard
            ]
        );
        assert_eq!(
            resolved.platform_admins.pages,
            vec![
                AdminPage::ApiKeys,
                AdminPage::Models,
                AdminPage::Mcp,
                AdminPage::Leaderboard
            ]
        );
        assert_eq!(
            resolved.users.actions,
            vec![AdminAction::CreateApiKey, AdminAction::UpdateApiKey]
        );
        assert_eq!(
            resolved.team_admins.actions,
            vec![
                AdminAction::CreateApiKey,
                AdminAction::UpdateApiKey,
                AdminAction::RevealApiKey,
            ]
        );
        assert_eq!(resolved.platform_admins.actions, ADMIN_ACTION_ORDER);
    }

    #[test]
    fn explicit_empty_lists_can_resolve_to_no_access() {
        let config = parse(
            r#"
users:
  pages: []
  actions: []
team_admins:
  pages: []
  actions: []
platform_admins:
  pages: []
  actions: []
"#,
        );
        let resolved = config.resolve().expect("permissions");

        assert!(resolved.users.pages.is_empty());
        assert!(resolved.team_admins.pages.is_empty());
        assert!(resolved.platform_admins.pages.is_empty());
        assert!(resolved.users.actions.is_empty());
        assert!(resolved.team_admins.actions.is_empty());
        assert!(resolved.platform_admins.actions.is_empty());
        assert_eq!(resolved.users.default_page, None);
    }

    #[test]
    fn partial_groups_keep_direct_defaults() {
        let config = parse(
            r#"
users:
  default_page: leaderboard
team_admins: {}
"#,
        );
        let resolved = config.resolve().expect("permissions");

        assert_eq!(resolved.users.pages, SHARED_PAGES);
        assert_eq!(resolved.team_admins.pages, TEAM_ADMIN_PAGES);
        assert_eq!(resolved.platform_admins.pages, ADMIN_PAGE_ORDER);
        assert_eq!(resolved.users.default_page, Some(AdminPage::Leaderboard));
    }

    #[test]
    fn invalid_defaults_and_capability_grants_fail() {
        let invalid_default = parse(
            r#"
users:
  pages: [models]
  default_page: api_keys
"#,
        );
        assert!(invalid_default.resolve().is_err());

        let unsupported_page = parse(
            r#"
users:
  pages: [spend_controls]
"#,
        );
        assert!(unsupported_page.resolve().is_err());

        let unsupported_action = parse(
            r#"
users:
  actions: [reveal_api_key]
"#,
        );
        assert!(unsupported_action.resolve().is_err());
    }

    #[test]
    fn unknown_fields_and_pages_fail_to_parse() {
        assert!(serde_yaml::from_str::<PermissionsConfig>("guests: {}").is_err());
        assert!(
            serde_yaml::from_str::<PermissionsConfig>(
                "users:\n  pages: [models]\n  unexpected: true"
            )
            .is_err()
        );
        assert!(serde_yaml::from_str::<PermissionsConfig>("users:\n  pages: [new_page]").is_err());
        assert!(
            serde_yaml::from_str::<PermissionsConfig>("users:\n  actions: [delete_everything]")
                .is_err()
        );
    }

    #[test]
    fn group_resolution_preserves_platform_admin_precedence() {
        assert_eq!(
            AdminPermissionGroup::for_user(GlobalRole::PlatformAdmin, Some(MembershipRole::Member)),
            AdminPermissionGroup::PlatformAdmins
        );
        assert_eq!(
            AdminPermissionGroup::for_user(GlobalRole::User, Some(MembershipRole::Owner)),
            AdminPermissionGroup::TeamAdmins
        );
        assert_eq!(
            AdminPermissionGroup::for_user(GlobalRole::User, Some(MembershipRole::Admin)),
            AdminPermissionGroup::TeamAdmins
        );
        assert_eq!(
            AdminPermissionGroup::for_user(GlobalRole::User, Some(MembershipRole::Member)),
            AdminPermissionGroup::Users
        );
        assert_eq!(
            AdminPermissionGroup::for_user(GlobalRole::User, None),
            AdminPermissionGroup::Users
        );
    }
}
