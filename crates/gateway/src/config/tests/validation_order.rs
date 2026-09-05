use super::*;

#[test]
fn reports_first_invalid_domain_in_existing_validation_order() {
    let invalid_domains = [
        (
            "agent_analysis:\n  context_input_boundary_tokens: 0\n",
            "agent_analysis.context_input_boundary_tokens must be > 0",
        ),
        (
            "providers:\n  - id: broken\n    type: openai_compat\n    base_url: ''\n    pricing_provider_id: openai\n",
            "openai_compat provider `broken` base_url cannot be empty",
        ),
        (
            "models:\n  - id: broken\n",
            "model `broken` must define either alias_of or at least one route",
        ),
        (
            "budgets:\n  users:\n    model_defaults:\n      - model: missing\n        budget:\n          cadence: daily\n          amount_usd: '1.0000'\n",
            "budgets.users.model_defaults references unknown model `missing`",
        ),
        (
            "teams:\n  - id: broken\n    name: ''\n",
            "team `broken` name cannot be empty",
        ),
        (
            "service_accounts:\n  - id: broken\n    team: missing\n    budget:\n      cadence: daily\n      amount_usd: '1.0000'\n",
            "service account `broken` references unknown team `missing`",
        ),
        (
            "users:\n  - name: ''\n    email: person@example.com\n    auth_mode: password\n",
            "user name cannot be empty",
        ),
    ];
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    // Remove each earlier error in turn; the next domain must become the first failure.
    for first in 0..invalid_domains.len() {
        let yaml = invalid_domains[first..]
            .iter()
            .map(|(yaml, _)| *yaml)
            .collect::<String>();
        write_config(&config_path, &yaml);

        let error = GatewayConfig::from_path(&config_path).expect_err("invalid configuration");
        assert_eq!(error.root_cause().to_string(), invalid_domains[first].1);
        assert_eq!(
            error.to_string(),
            format!("invalid gateway configuration `{}`", config_path.display())
        );
    }
}

#[test]
fn rejects_misspelled_top_level_and_identity_policy_fields() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("gateway.yaml");
    for (yaml, field) in [
        ("guardrail: {}", "guardrail"),
        (
            "users: [{name: Person, email: person@example.com, auth_mode: password, oidc_provider: corp}]",
            "oidc_provider",
        ),
        (
            "users: [{name: Person, email: person@example.com, auth_mode: password, membership: {team: corp, rol: admin}}]",
            "rol",
        ),
    ] {
        write_config(&path, yaml);
        let error = GatewayConfig::from_path(&path).unwrap_err();
        assert!(
            format!("{error:#}").contains(&format!("unknown field `{field}`")),
            "{error:#}"
        );
    }
}
