use gateway_core::{
    ExternalMcpAuthMode, ExternalMcpServerStatus, ExternalMcpTransport, McpServerOwnership,
};
use serde_json::Map;
use time::OffsetDateTime;

use super::*;

fn membership(team_id: Uuid, role: MembershipRole) -> TeamMembershipRecord {
    let now = OffsetDateTime::now_utc();
    TeamMembershipRecord {
        team_id,
        user_id: Uuid::new_v4(),
        role,
        created_at: now,
        updated_at: now,
    }
}

fn server(ownership: McpServerOwnership) -> ExternalMcpServerRecord {
    let now = OffsetDateTime::now_utc();
    ExternalMcpServerRecord {
        mcp_server_id: Uuid::new_v4(),
        server_key: "docs".to_string(),
        display_name: "Docs".to_string(),
        description: None,
        transport: ExternalMcpTransport::StreamableHttp,
        server_url: "https://mcp.example.test/mcp".to_string(),
        auth_mode: ExternalMcpAuthMode::None,
        auth_config: Map::new(),
        timeout_ms: 30_000,
        status: ExternalMcpServerStatus::Active,
        last_discovery_status: None,
        last_discovery_at: None,
        last_successful_discovery_at: None,
        last_error_summary: None,
        last_tool_count: None,
        created_at: now,
        updated_at: now,
        disabled_at: None,
        ownership,
    }
}

#[test]
fn members_get_no_admin_scope() {
    let team = Uuid::new_v4();
    assert!(McpAdminScope::team_admin(&membership(team, MembershipRole::Member)).is_none());
    for role in [MembershipRole::Owner, MembershipRole::Admin] {
        let scope = McpAdminScope::team_admin(&membership(team, role)).expect("team admin");
        assert_eq!(scope.managed_team_id(), Some(team));
        assert!(!scope.is_platform_admin());
    }
}

#[test]
fn team_admins_see_their_servers_and_published_ones_only() {
    let team = Uuid::new_v4();
    let other = Uuid::new_v4();
    let scope = McpAdminScope::team_admin(&membership(team, MembershipRole::Admin)).unwrap();

    let own = server(McpServerOwnership::team(team, None));
    let other_private = server(McpServerOwnership::team(other, None));
    let mut other_published = server(McpServerOwnership::team(other, None));
    other_published.ownership.visibility = McpServerVisibility::Organisation;
    let platform = server(McpServerOwnership::default());

    assert!(scope.can_view(&own) && scope.can_manage(&own));
    assert!(!scope.can_view(&other_private) && !scope.can_manage(&other_private));
    assert!(scope.can_view(&other_published) && !scope.can_manage(&other_published));
    assert!(scope.can_view(&platform) && !scope.can_manage(&platform));
}

#[test]
fn platform_admins_manage_every_server() {
    let scope = McpAdminScope::platform_admin(None);
    let private = server(McpServerOwnership::team(Uuid::new_v4(), None));
    assert!(scope.can_view(&private) && scope.can_manage(&private));
    assert_eq!(scope.managed_team_id(), None);
}

#[test]
fn audit_entries_record_the_actor_team_role() {
    let team = Uuid::new_v4();
    let scope = McpAdminScope::team_admin(&membership(team, MembershipRole::Owner)).unwrap();
    let event = scope.annotate(AuditEvent::new(
        crate::admin_audit::actions::MCP_SERVER_CREATED,
    ));
    let details = event.metadata();
    assert_eq!(details["actor_access"], "team_admin");
    assert_eq!(details["actor_team_role"], "owner");
    assert_eq!(details["actor_team_id"], team.to_string());

    let platform = McpAdminScope::platform_admin(Some(&membership(team, MembershipRole::Member)));
    let event = platform.annotate(AuditEvent::new(
        crate::admin_audit::actions::MCP_SERVER_CREATED,
    ));
    let details = event.metadata();
    assert_eq!(details["actor_access"], "platform_admin");
    assert_eq!(details["actor_team_role"], "member");
}

#[test]
fn owner_contacts_are_trimmed_and_bounded() {
    assert_eq!(normalize_owner_contact(Some("  ".into())).unwrap(), None);
    assert_eq!(
        normalize_owner_contact(Some(" #team-docs ".into())).unwrap(),
        Some("#team-docs".to_string())
    );
    assert!(normalize_owner_contact(Some("x".repeat(MAX_OWNER_CONTACT_CHARS + 1))).is_err());
}

#[test]
fn only_public_addresses_pass_the_destination_check() {
    use std::net::IpAddr;
    for public in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
        assert!(
            is_public_address(public.parse::<IpAddr>().unwrap()),
            "{public}"
        );
    }
    for internal in [
        "127.0.0.1",
        "10.1.2.3",
        "172.16.0.1",
        "192.168.0.1",
        "169.254.169.254",
        "100.100.100.200",
        "0.0.0.0",
        "255.255.255.255",
        "::1",
        "::",
        "fd00::1",
        "fe80::1",
        "::ffff:10.0.0.1",
        "2001:db8::1",
    ] {
        assert!(
            !is_public_address(internal.parse::<IpAddr>().unwrap()),
            "{internal}"
        );
    }
}
