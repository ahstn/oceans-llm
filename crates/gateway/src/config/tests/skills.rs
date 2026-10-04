use super::*;

#[test]
fn skills_are_disabled_without_storage_credentials() {
    let tmp = tempdir().expect("tempdir");
    let path = tmp.path().join("gateway.yaml");
    write_config(&path, "skills: {}\n");

    let config = GatewayConfig::from_path(&path).expect("disabled skills config");

    assert!(!config.skills.enabled);
    assert_eq!(config.skills.limits.max_archive_bytes, 10 * 1024 * 1024);
    assert_eq!(config.skills.limits.max_expanded_bytes, 25 * 1024 * 1024);
    assert_eq!(config.skills.limits.max_files, 1000);
}

#[test]
fn disabled_skills_do_not_resolve_storage_secrets() {
    let tmp = tempdir().expect("tempdir");
    let path = tmp.path().join("gateway.yaml");
    write_config(
        &path,
        "skills:\n  storage:\n    access_key_id: env.UNSET_SKILL_STORAGE_TEST_KEY\n",
    );

    let config = GatewayConfig::from_path(&path).expect("disabled skills do not need storage");

    assert!(!config.skills.enabled);
}

#[test]
fn skills_accept_local_s3_and_resolve_secret_files() {
    let tmp = tempdir().expect("tempdir");
    let secret_path = tmp.path().join("secret");
    std::fs::write(&secret_path, "test-secret\n").expect("secret file");
    let path = tmp.path().join("gateway.yaml");
    write_config(
        &path,
        &format!(
            r#"
skills:
  enabled: true
  storage:
    bucket: oceans-skills
    endpoint: http://127.0.0.1:9000
    allow_http: true
    force_path_style: true
    access_key_id: literal.test-access
    secret_access_key: file.{}
  limits:
    max_files: 42
"#,
            secret_path.display()
        ),
    );

    let config = GatewayConfig::from_path(&path).expect("local S3 config");
    let storage = config.skills.storage_options().expect("storage options");

    assert_eq!(storage.endpoint.as_deref(), Some("http://127.0.0.1:9000"));
    assert_eq!(storage.region, "us-east-1");
    assert_eq!(storage.prefix, "skills/");
    assert!(storage.force_path_style);
    assert!(storage.allow_http);
    assert_eq!(storage.access_key_id.as_deref(), Some("test-access"));
    assert_eq!(storage.secret_access_key.as_deref(), Some("test-secret"));
    assert_eq!(storage.max_upload_bytes, 10 * 1024 * 1024);
    assert_eq!(config.skills.limits.max_files, 42);
}

#[test]
fn skills_allow_default_aws_credentials_without_explicit_secrets() {
    let tmp = tempdir().expect("tempdir");
    let path = tmp.path().join("gateway.yaml");
    write_config(
        &path,
        "skills:\n  enabled: true\n  storage:\n    bucket: oceans-skills\n",
    );

    let config = GatewayConfig::from_path(&path).expect("AWS config");
    let storage = config.skills.storage_options().expect("storage options");

    assert!(storage.endpoint.is_none());
    assert!(storage.access_key_id.is_none());
    assert!(storage.secret_access_key.is_none());
    assert!(storage.session_token.is_none());
}

#[test]
fn skills_reject_invalid_storage_configuration() {
    let cases = [
        ("bucket: ''", "skills.storage.bucket"),
        ("region: ''", "skills.storage.region"),
        ("prefix: '../skills'", "skills.storage.prefix"),
        ("endpoint: http://127.0.0.1:9000", "allow_http: true"),
        (
            "endpoint: https://user:pass@example.com",
            "without credentials",
        ),
        ("endpoint: https://example.com/path", "without credentials"),
        ("access_key_id: literal.test", "must be set together"),
        ("session_token: literal.test", "requires explicit"),
        (
            "access_key_id: raw-key\n    secret_access_key: literal.test",
            "unsupported secret reference",
        ),
        (
            "access_key_id: literal.test\n    secret_access_key: literal.",
            "cannot resolve to an empty value",
        ),
    ];
    for (storage_field, expected) in cases {
        let tmp = tempdir().expect("tempdir");
        let path = tmp.path().join("gateway.yaml");
        let bucket = if storage_field.starts_with("bucket:") {
            ""
        } else {
            "    bucket: oceans-skills\n"
        };
        write_config(
            &path,
            &format!("skills:\n  enabled: true\n  storage:\n{bucket}    {storage_field}\n"),
        );

        let error = GatewayConfig::from_path(&path).expect_err("invalid skills config");
        assert!(
            format!("{error:#}").contains(expected),
            "expected `{expected}`, got `{error:#}`"
        );
    }
}

#[test]
fn skills_reject_zero_limits_and_unknown_limit_fields() {
    for limits in [
        "max_archive_bytes: 0",
        "max_expanded_bytes: 0",
        "max_files: 0",
        "max_archive_size: 1024",
    ] {
        let tmp = tempdir().expect("tempdir");
        let path = tmp.path().join("gateway.yaml");
        write_config(
            &path,
            &format!(
                "skills:\n  enabled: true\n  storage:\n    bucket: oceans-skills\n  limits:\n    {limits}\n"
            ),
        );
        GatewayConfig::from_path(&path).expect_err("invalid skill limits");
    }
}

#[test]
fn skills_prefix_reserves_archive_key_space_in_utf8_bytes() {
    let cases = [
        (String::new(), true),
        ("a".repeat(983), true),
        (format!("{}/", "a".repeat(983)), true),
        (format!("{}///", "a".repeat(983)), true),
        (format!("{}a/", "é".repeat(491)), true),
        ("a".repeat(984), false),
        (format!("{}/", "a".repeat(984)), false),
        (format!("{}/", "é".repeat(492)), false),
    ];
    for (prefix, valid) in cases {
        let tmp = tempdir().expect("tempdir");
        let path = tmp.path().join("gateway.yaml");
        write_config(
            &path,
            &format!(
                "skills:\n  enabled: true\n  storage:\n    bucket: oceans-skills\n    prefix: '{prefix}'\n"
            ),
        );

        let config = GatewayConfig::from_path(&path);
        if valid {
            let storage = config
                .expect("valid prefix")
                .skills
                .storage_options()
                .expect("storage options");
            let prefix = storage.prefix.trim_end_matches('/');
            let archive_name = format!("{}.zip", uuid::Uuid::nil());
            let object_key = if prefix.is_empty() {
                archive_name
            } else {
                format!("{prefix}/{archive_name}")
            };
            assert!(object_key.len() <= 1024, "generated key exceeds S3 limit");
        } else {
            let error = config.expect_err("prefix cannot accommodate an archive name");
            assert!(format!("{error:#}").contains("at most 983 bytes"));
        }
    }
}
