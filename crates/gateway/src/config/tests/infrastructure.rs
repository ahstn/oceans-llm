use super::*;

#[test]
fn rejects_invalid_server_bind_address() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");
    write_config(&config_path, "server:\n  bind: not-a-socket-address\n");

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");

    assert!(format!("{error:#}").contains("server.bind"));
}

#[test]
fn rejects_invalid_otel_endpoints() {
    for field in ["otel_endpoint", "otel_metrics_endpoint"] {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("gateway.yaml");
        write_config(
            &config_path,
            &format!("server:\n  {field}: 'not a valid URI'\n"),
        );

        let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");

        assert!(format!("{error:#}").contains(&format!("server.{field}")));
    }
}

#[test]
fn otel_trace_sample_ratio_defaults_to_one() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");
    write_config(&config_path, "server: {}\n");

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");

    assert_eq!(config.server.otel_trace_sample_ratio, 1.0);
}

#[test]
fn accepts_inclusive_otel_trace_sample_ratio_boundaries() {
    for ratio in [0.0, 1.0] {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("gateway.yaml");
        write_config(
            &config_path,
            &format!("server:\n  otel_trace_sample_ratio: {ratio}\n"),
        );

        let config = GatewayConfig::from_path(&config_path).expect("config should parse");

        assert_eq!(config.server.otel_trace_sample_ratio, ratio);
    }
}

#[test]
fn rejects_invalid_otel_trace_sample_ratio() {
    for ratio in ["-0.1", "1.1", ".nan"] {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("gateway.yaml");
        write_config(
            &config_path,
            &format!("server:\n  otel_trace_sample_ratio: {ratio}\n"),
        );

        let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");

        assert!(format!("{error:#}").contains("server.otel_trace_sample_ratio"));
    }
}

#[test]
fn request_log_payload_policy_defaults_match_current_capture_behavior() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(&config_path, "");

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");
    let policy = config.request_log_payload_policy().expect("policy");

    assert_eq!(
        policy.capture_mode,
        RequestLogPayloadCaptureMode::RedactedPayloads
    );
    assert_eq!(policy.request_max_bytes, 128 * 1024);
    assert_eq!(policy.response_max_bytes, 64 * 1024);
    assert_eq!(policy.stream_max_events, 128);
}

#[test]
fn request_log_purge_config_defaults_are_disabled_with_daily_retention() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(&config_path, "");

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");

    assert!(!config.request_logging.purge.enabled);
    assert_eq!(
        config.request_logging.purge.retention,
        RequestLogRetentionWindow::SevenDays
    );
    assert_eq!(config.request_logging.purge.schedule, "0 0 * * *");
}

#[test]
fn parses_request_log_purge_config() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
request_logging:
  purge:
    enabled: true
    retention: 3d
    schedule: "30 1 * * *"
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");

    assert!(config.request_logging.purge.enabled);
    assert_eq!(
        config.request_logging.purge.retention,
        RequestLogRetentionWindow::ThreeDays
    );
    assert_eq!(config.request_logging.purge.schedule, "30 1 * * *");
}

#[test]
fn rejects_request_log_purge_schedule_more_frequent_than_daily() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
request_logging:
  purge:
    enabled: true
    schedule: "0 */12 * * *"
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains(
            "request_logging.purge.schedule must not run more frequently than once per day"
        ),
        "unexpected error: {error_text}"
    );
}

#[test]
fn parses_request_log_payload_policy_config() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
request_logging:
  payloads:
    capture_mode: summary_only
    request_max_bytes: 1024
    response_max_bytes: 2048
    stream_max_events: 3
    redaction_paths:
      - body.messages.*.metadata.internal
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");
    let policy = config.request_log_payload_policy().expect("policy");

    assert_eq!(
        policy.capture_mode,
        RequestLogPayloadCaptureMode::SummaryOnly
    );
    assert_eq!(policy.request_max_bytes, 1024);
    assert_eq!(policy.response_max_bytes, 2048);
    assert_eq!(policy.stream_max_events, 3);
}

#[test]
fn rejects_invalid_request_log_payload_policy_config() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
request_logging:
  payloads:
    capture_mode: redacted_payloads
    request_max_bytes: 0
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("request_logging.payloads.request_max_bytes must be > 0"),
        "unexpected error: {error_text}"
    );

    write_config(
        &config_path,
        r#"
request_logging:
  payloads:
    request_max_bytes: 262145
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("request_logging.payloads.request_max_bytes must be <= 262144"),
        "unexpected error: {error_text}"
    );

    write_config(
        &config_path,
        r#"
request_logging:
  payloads:
    stream_max_events: 0
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("request_logging.payloads.stream_max_events must be > 0"),
        "unexpected error: {error_text}"
    );

    write_config(
        &config_path,
        r#"
request_logging:
  payloads:
    redaction_paths:
      - body..messages
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("request_logging.payloads.redaction_paths"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn resolves_default_and_postgres_database_options() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let missing_path = tmp.path().join("missing.yaml");
    let default_config = GatewayConfig::from_path(&missing_path)?;
    assert!(matches!(
        default_config.database_options()?,
        StoreConnectionOptions::Libsql { .. }
    ));
    default_config.resolved_admin_permissions()?;

    let config_path = tmp.path().join("gateway.yaml");
    write_config(
        &config_path,
        r#"
database:
  url: literal.postgres://gateway:secret@localhost/gateway
  max_connections: 17
"#,
    );
    let config = GatewayConfig::from_path(&config_path)?;
    match config.database_options()? {
        StoreConnectionOptions::Postgres {
            url,
            max_connections,
        } => {
            assert_eq!(url, "postgres://gateway:secret@localhost/gateway");
            assert_eq!(max_connections, 17);
        }
        StoreConnectionOptions::Libsql { .. } => panic!("expected postgres options"),
    }

    write_config(&config_path, "database:\n  kind: unsupported\n");
    let error = GatewayConfig::from_path(&config_path).expect_err("invalid database kind");
    assert!(format!("{error:#}").contains("unsupported database.kind"));

    write_config(&config_path, "database:\n  kind: postgres\n");
    let error = GatewayConfig::from_path(&config_path).expect_err("missing postgres URL");
    assert!(format!("{error:#}").contains("database.url is required"));

    Ok(())
}

#[test]
fn rejects_invalid_budget_alert_email_config() -> anyhow::Result<()> {
    let cases = [
        (
            "budget_alerts:\n  email:\n    from_email: ''\n",
            "from_email cannot be empty",
        ),
        (
            "budget_alerts:\n  email:\n    poll_interval_secs: 0\n",
            "poll_interval_secs must be > 0",
        ),
        (
            "budget_alerts:\n  email:\n    batch_size: 0\n",
            "batch_size must be > 0",
        ),
        (
            "budget_alerts:\n  email:\n    transport:\n      kind: smtp\n      host: ''\n",
            "smtp.host cannot be empty",
        ),
    ];

    for (yaml, expected) in cases {
        let tmp = tempdir()?;
        let config_path = tmp.path().join("gateway.yaml");
        write_config(&config_path, yaml);
        let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
        assert!(
            format!("{error:#}").contains(expected),
            "expected error containing `{expected}`, got `{error:#}`"
        );
    }

    Ok(())
}
