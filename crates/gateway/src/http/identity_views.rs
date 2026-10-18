use std::collections::HashMap;

use gateway_core::{AuthMode, IdentityRepository, IdentityUserRecord, MembershipRole, UserStatus};
use gateway_store::{AnyStore, GatewayStore};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::http::{
    admin_contract::{
        AdminEntityTagView, AdminIdentityUserView, AdminOnboardingActionView, AdminTeamAdminView,
        AdminTeamAssignableUserView, AdminTeamManagementView, AdminTeamMemberView,
        IdentityDirectoryTeamView, IdentityDirectoryUserView, format_timestamp,
    },
    error::AppError,
    identity::{invitation_url, oauth_sign_in_url, oidc_sign_in_url},
};

pub(crate) async fn build_admin_identity_user_view(
    store: &AnyStore,
    secret: &str,
    origin: &str,
    now: OffsetDateTime,
    user: IdentityUserRecord,
    include_onboarding: bool,
) -> Result<AdminIdentityUserView, AppError> {
    let onboarding = match (include_onboarding, user.user.auth_mode) {
        (false, _) => None,
        (true, AuthMode::Password) if user.user.status == UserStatus::Invited => {
            let active_invitation = store
                .find_active_password_invitation_for_user(user.user.user_id, now)
                .await?;
            Some(AdminOnboardingActionView::PasswordInvite {
                invite_url: active_invitation
                    .as_ref()
                    .map(|invitation| invitation_url(origin, invitation, secret)),
                expires_at: active_invitation
                    .as_ref()
                    .map(|invitation| format_timestamp(invitation.expires_at)),
                can_resend: true,
            })
        }
        (true, AuthMode::Oidc) => user.oidc_provider_key.as_deref().map(|provider_key| {
            AdminOnboardingActionView::OidcSignIn {
                sign_in_url: oidc_sign_in_url(origin, provider_key, &user.user.email),
                provider_key: provider_key.to_string(),
                provider_label: provider_key.to_string(),
            }
        }),
        (true, AuthMode::Oauth) => user.oauth_provider_key.as_deref().map(|provider_key| {
            AdminOnboardingActionView::OauthSignIn {
                sign_in_url: oauth_sign_in_url(origin, provider_key, &user.user.email),
                provider_key: provider_key.to_string(),
                provider_label: provider_key.to_string(),
            }
        }),
        _ => None,
    };

    Ok(AdminIdentityUserView {
        id: user.user.user_id.to_string(),
        name: user.user.name,
        email: user.user.email,
        auth_mode: user.user.auth_mode.as_str().to_string(),
        global_role: user.user.global_role.as_str().to_string(),
        request_logging_enabled: user.user.request_logging_enabled,
        team_id: user.team_id.map(|value| value.to_string()),
        team_name: user.team_name,
        team_role: user.membership_role.map(|value| value.as_str().to_string()),
        status: format_user_status(user.user.status),
        tags: entity_tag_views(&user.user.tags),
        onboarding,
    })
}

pub(crate) fn build_identity_directory_user_views(
    users: &[IdentityUserRecord],
) -> Vec<IdentityDirectoryUserView> {
    users
        .iter()
        .map(|user| IdentityDirectoryUserView {
            id: user.user.user_id.to_string(),
            name: user.user.name.clone(),
            email: user.user.email.clone(),
            global_role: user.user.global_role.as_str().to_string(),
            team_id: user.team_id.map(|value| value.to_string()),
            team_name: user.team_name.clone(),
            team_role: user.membership_role.map(|value| value.as_str().to_string()),
            status: format_user_status(user.user.status),
        })
        .collect()
}

pub(crate) fn build_identity_directory_team_views(
    teams: &[gateway_core::TeamRecord],
    users: &[IdentityUserRecord],
) -> Vec<IdentityDirectoryTeamView> {
    let mut members_by_team: HashMap<Uuid, Vec<AdminTeamMemberView>> = HashMap::new();
    for user in users {
        let Some(team_id) = user.team_id else {
            continue;
        };
        members_by_team
            .entry(team_id)
            .or_default()
            .push(AdminTeamMemberView {
                id: user.user.user_id.to_string(),
                name: user.user.name.clone(),
                email: user.user.email.clone(),
                status: format_user_status(user.user.status),
                role: user
                    .membership_role
                    .map(|value| value.as_str().to_string())
                    .unwrap_or_else(|| "member".to_string()),
            });
    }

    teams
        .iter()
        .map(|team| {
            let mut members = members_by_team.remove(&team.team_id).unwrap_or_default();
            members.sort_by(|left, right| {
                left.name
                    .cmp(&right.name)
                    .then_with(|| left.email.cmp(&right.email))
            });
            IdentityDirectoryTeamView {
                id: team.team_id.to_string(),
                name: team.team_name.clone(),
                status: team.status.clone(),
                member_count: members.len(),
                members,
            }
        })
        .collect()
}

pub(crate) fn build_assignable_user_views(
    users: &[IdentityUserRecord],
) -> Vec<AdminTeamAssignableUserView> {
    let mut views: Vec<_> = users
        .iter()
        .map(|user| AdminTeamAssignableUserView {
            id: user.user.user_id.to_string(),
            name: user.user.name.clone(),
            email: user.user.email.clone(),
            status: format_user_status(user.user.status),
            team_id: user.team_id.map(|value| value.to_string()),
            team_name: user.team_name.clone(),
            team_role: user.membership_role.map(|value| value.as_str().to_string()),
        })
        .collect();
    views.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.email.cmp(&right.email))
    });
    views
}

pub(crate) fn build_admin_team_views(
    teams: &[gateway_core::TeamRecord],
    users: &[IdentityUserRecord],
) -> Vec<AdminTeamManagementView> {
    let mut users_by_team: HashMap<Uuid, Vec<&IdentityUserRecord>> = HashMap::new();
    for user in users {
        let Some(team_id) = user.team_id else {
            continue;
        };
        users_by_team.entry(team_id).or_default().push(user);
    }

    teams
        .iter()
        .map(|team| {
            let team_users = users_by_team
                .get(&team.team_id)
                .map(Vec::as_slice)
                .unwrap_or_default();
            build_admin_team_view(team, team_users)
        })
        .collect()
}

/// `team_users` must already be limited to `team`'s members.
fn build_admin_team_view(
    team: &gateway_core::TeamRecord,
    team_users: &[&IdentityUserRecord],
) -> AdminTeamManagementView {
    let mut admins: Vec<_> = team_users
        .iter()
        .filter(|user| user.membership_role == Some(MembershipRole::Admin))
        .map(|user| AdminTeamAdminView {
            id: user.user.user_id.to_string(),
            name: user.user.name.clone(),
            email: user.user.email.clone(),
            status: format_user_status(user.user.status),
        })
        .collect();
    let mut members: Vec<_> = team_users
        .iter()
        .map(|user| AdminTeamMemberView {
            id: user.user.user_id.to_string(),
            name: user.user.name.clone(),
            email: user.user.email.clone(),
            status: format_user_status(user.user.status),
            role: user
                .membership_role
                .map(|value| value.as_str().to_string())
                .unwrap_or_else(|| "member".to_string()),
        })
        .collect();
    admins.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.email.cmp(&right.email))
    });
    members.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.email.cmp(&right.email))
    });

    AdminTeamManagementView {
        id: team.team_id.to_string(),
        name: team.team_name.clone(),
        key: team.team_key.clone(),
        status: team.status.clone(),
        tags: entity_tag_views(&team.tags),
        member_count: members.len(),
        admins,
        members,
    }
}

pub(crate) async fn reload_team_view(
    store: &AnyStore,
    team_id: Uuid,
) -> Result<AdminTeamManagementView, AppError> {
    let team = store.get_team_by_id(team_id).await?.ok_or_else(|| {
        AppError(gateway_core::GatewayError::Store(
            gateway_core::StoreError::NotFound("team missing".to_string()),
        ))
    })?;
    let users = store.list_identity_users().await?;
    let team_users: Vec<_> = users
        .iter()
        .filter(|user| user.team_id == Some(team_id))
        .collect();
    Ok(build_admin_team_view(&team, &team_users))
}

pub(crate) async fn reload_identity_user(
    store: &AnyStore,
    user_id: Uuid,
) -> Result<IdentityUserRecord, AppError> {
    store.get_identity_user(user_id).await?.ok_or_else(|| {
        AppError(gateway_core::GatewayError::Store(
            gateway_core::StoreError::NotFound("user missing".to_string()),
        ))
    })
}

pub(crate) fn format_user_status(status: UserStatus) -> String {
    status.as_str().to_string()
}

pub(crate) fn entity_tag_views(tags: &[gateway_core::RequestTag]) -> Vec<AdminEntityTagView> {
    tags.iter()
        .map(|tag| AdminEntityTagView {
            key: tag.key.clone(),
            value: tag.value.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use gateway_core::{
        AuthMode, GatewayError, GlobalRole, IdentityUserRecord, MembershipRole, ModelAccessMode,
        RequestTag, StoreError, TeamRecord, UserRecord, UserStatus,
    };
    use gateway_store::GatewayStore;
    use time::OffsetDateTime;
    use uuid::Uuid;

    use super::{build_admin_team_views, reload_team_view};
    use crate::http::{admin_contract::AdminTeamManagementView, test_support::app_state};

    fn team(name: &str) -> TeamRecord {
        TeamRecord {
            team_id: Uuid::new_v4(),
            team_key: name.to_lowercase(),
            team_name: name.to_string(),
            status: "active".to_string(),
            model_access_mode: ModelAccessMode::All,
            tags: Vec::new(),
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    fn teamless_user(name: &str, email: &str) -> IdentityUserRecord {
        IdentityUserRecord {
            user: UserRecord {
                user_id: Uuid::new_v4(),
                name: name.to_string(),
                email: email.to_string(),
                email_normalized: email.to_string(),
                global_role: GlobalRole::User,
                auth_mode: AuthMode::Password,
                status: UserStatus::Active,
                must_change_password: false,
                request_logging_enabled: true,
                model_access_mode: ModelAccessMode::All,
                tags: Vec::new(),
                created_at: OffsetDateTime::UNIX_EPOCH,
                updated_at: OffsetDateTime::UNIX_EPOCH,
            },
            team_id: None,
            team_name: None,
            membership_role: None,
            oidc_provider_id: None,
            oidc_provider_key: None,
            oauth_provider_id: None,
            oauth_provider_key: None,
        }
    }

    fn team_user(
        team: &TeamRecord,
        role: MembershipRole,
        name: &str,
        email: &str,
    ) -> IdentityUserRecord {
        IdentityUserRecord {
            team_id: Some(team.team_id),
            team_name: Some(team.team_name.clone()),
            membership_role: Some(role),
            ..teamless_user(name, email)
        }
    }

    fn admin_emails(view: &AdminTeamManagementView) -> Vec<&str> {
        view.admins
            .iter()
            .map(|admin| admin.email.as_str())
            .collect()
    }

    fn member_emails(view: &AdminTeamManagementView) -> Vec<&str> {
        view.members
            .iter()
            .map(|member| member.email.as_str())
            .collect()
    }

    #[test]
    fn team_views_sort_admins_and_members_by_name_then_email() {
        let platform = team("Platform");
        // Sorted by email alone, Cy and Zoe would come first.
        let users = [
            (MembershipRole::Member, "Zoe", "a.zoe@example.test"),
            (MembershipRole::Admin, "Cy", "a.cy@example.test"),
            (MembershipRole::Admin, "Ada", "ada.b@example.test"),
            (MembershipRole::Member, "Bo", "bo@example.test"),
            (MembershipRole::Admin, "Ada", "ada.a@example.test"),
        ]
        .map(|(role, name, email)| team_user(&platform, role, name, email));

        let views = build_admin_team_views(&[platform], &users);

        assert_eq!(
            admin_emails(&views[0]),
            [
                "ada.a@example.test",
                "ada.b@example.test",
                "a.cy@example.test"
            ]
        );
        assert_eq!(
            member_emails(&views[0]),
            [
                "ada.a@example.test",
                "ada.b@example.test",
                "bo@example.test",
                "a.cy@example.test",
                "a.zoe@example.test",
            ]
        );
    }

    #[test]
    fn team_views_list_only_the_admin_role_as_admins() {
        let platform = team("Platform");
        let mut without_role =
            team_user(&platform, MembershipRole::Member, "Di", "di@example.test");
        without_role.membership_role = None;
        let users = [
            team_user(&platform, MembershipRole::Owner, "Ada", "ada@example.test"),
            team_user(&platform, MembershipRole::Admin, "Bo", "bo@example.test"),
            team_user(&platform, MembershipRole::Member, "Cy", "cy@example.test"),
            without_role,
        ];

        let views = build_admin_team_views(&[platform], &users);

        let view = &views[0];
        assert_eq!(admin_emails(view), ["bo@example.test"]);
        assert_eq!(view.admins[0].id, users[1].user.user_id.to_string());
        assert_eq!(view.admins[0].name, "Bo");
        assert_eq!(view.admins[0].status, "active");
        // Owners stay members only, and a membership without a role reads as "member".
        let roles: Vec<_> = view
            .members
            .iter()
            .map(|member| (member.email.as_str(), member.role.as_str()))
            .collect();
        assert_eq!(
            roles,
            [
                ("ada@example.test", "owner"),
                ("bo@example.test", "admin"),
                ("cy@example.test", "member"),
                ("di@example.test", "member"),
            ]
        );
        assert_eq!(view.member_count, 4);
    }

    #[test]
    fn team_views_leave_out_teamless_users_and_other_teams_members() {
        let platform = team("Platform");
        let research = team("Research");
        let users = [
            teamless_user("Ada", "ada@example.test"),
            team_user(&research, MembershipRole::Admin, "Bo", "bo@example.test"),
            team_user(&platform, MembershipRole::Member, "Cy", "cy@example.test"),
            teamless_user("Di", "di@example.test"),
        ];

        let views = build_admin_team_views(&[platform, research], &users);

        assert!(views[0].admins.is_empty());
        assert_eq!(member_emails(&views[0]), ["cy@example.test"]);
        assert_eq!(views[0].member_count, 1);
        assert_eq!(admin_emails(&views[1]), ["bo@example.test"]);
        assert_eq!(member_emails(&views[1]), ["bo@example.test"]);
        assert_eq!(views[1].member_count, 1);
    }

    #[test]
    fn team_views_keep_empty_teams_in_the_given_order() {
        let platform = team("Platform");
        let mut empty = team("Empty");
        empty.tags = vec![RequestTag {
            key: "cost_center".to_string(),
            value: "ops".to_string(),
        }];
        let team_ids = [platform.team_id.to_string(), empty.team_id.to_string()];
        let users = [team_user(
            &platform,
            MembershipRole::Admin,
            "Ada",
            "ada@example.test",
        )];

        let views = build_admin_team_views(&[platform, empty], &users);

        let ids: Vec<_> = views.iter().map(|view| view.id.clone()).collect();
        assert_eq!(ids, team_ids);
        let empty_view = &views[1];
        assert_eq!(empty_view.name, "Empty");
        assert_eq!(empty_view.key, "empty");
        assert_eq!(empty_view.status, "active");
        assert_eq!(empty_view.tags[0].key, "cost_center");
        assert_eq!(empty_view.tags[0].value, "ops");
        assert_eq!(empty_view.member_count, 0);
        assert!(empty_view.admins.is_empty());
        assert!(empty_view.members.is_empty());
    }

    #[tokio::test]
    async fn reload_team_view_matches_the_listed_view_of_each_team() {
        let (_directory, state) = app_state().await;
        let store = &state.store;
        let platform = store.create_team("platform", "Platform").await.unwrap();
        let research = store.create_team("research", "Research").await.unwrap();
        store.create_team("empty", "Empty").await.unwrap();
        let tags = [RequestTag {
            key: "cost_center".to_string(),
            value: "ops".to_string(),
        }];
        store
            .update_team_tags(platform.team_id, &tags, OffsetDateTime::now_utc())
            .await
            .unwrap();
        for (name, membership) in [
            ("Bo", Some((&platform, MembershipRole::Member))),
            ("Ada", Some((&platform, MembershipRole::Admin))),
            ("Cy", Some((&research, MembershipRole::Admin))),
            ("Di", None),
        ] {
            let email = format!("{}@example.test", name.to_lowercase());
            let user = store
                .create_identity_user(
                    name,
                    &email,
                    &email,
                    GlobalRole::User,
                    AuthMode::Password,
                    UserStatus::Active,
                )
                .await
                .unwrap();
            if let Some((team, role)) = membership {
                store
                    .assign_team_membership(user.user_id, team.team_id, role)
                    .await
                    .unwrap();
            }
        }

        let listed = build_admin_team_views(
            &store.list_teams().await.unwrap(),
            &store.list_identity_users().await.unwrap(),
        );

        let platform_view = listed
            .iter()
            .find(|view| view.id == platform.team_id.to_string())
            .expect("platform is listed");
        assert_eq!(admin_emails(platform_view), ["ada@example.test"]);
        assert_eq!(
            member_emails(platform_view),
            ["ada@example.test", "bo@example.test"]
        );
        assert!(listed.iter().any(|view| view.members.is_empty()));
        for listed_view in &listed {
            let team_id = Uuid::parse_str(&listed_view.id).unwrap();
            let reloaded = reload_team_view(store, team_id)
                .await
                .unwrap_or_else(|error| panic!("reload {}: {:?}", listed_view.name, error.0));
            assert_eq!(
                serde_json::to_value(&reloaded).unwrap(),
                serde_json::to_value(listed_view).unwrap(),
                "{}",
                listed_view.name
            );
        }
    }

    #[tokio::test]
    async fn reload_team_view_reports_a_missing_team() {
        let (_directory, state) = app_state().await;

        let error = reload_team_view(&state.store, Uuid::new_v4())
            .await
            .expect_err("an unknown team should fail");

        assert!(matches!(
            error.0,
            GatewayError::Store(StoreError::NotFound(message)) if message == "team missing"
        ));
    }
}
