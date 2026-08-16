use super::*;

fn parse(raw: &str) -> PermissionsConfig {
    serde_yaml::from_str(raw).expect("permissions config")
}

#[test]
fn defaults_resolve_in_stable_order() {
    let resolved = PermissionsConfig::default().resolve().expect("permissions");

    assert_eq!(resolved.users.pages, SHARED_PAGES);
    assert_eq!(resolved.team_admins.pages, SHARED_PAGES);
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
    assert_eq!(resolved.team_admins.pages, SHARED_PAGES);
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
        serde_yaml::from_str::<PermissionsConfig>("users:\n  pages: [models]\n  unexpected: true")
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
