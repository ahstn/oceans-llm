use std::{fs, path::Path, time::Duration};

use url::Url;

use super::*;

const TIMEOUT: Duration = Duration::from_secs(5);

fn write_file(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
}

fn env_with_config(directory: &Path, vars: &[(&str, &str)], contents: &str) -> Env {
    write_file(&directory.join("oceans/config.yaml"), contents);
    let mut all = vec![("XDG_CONFIG_HOME", directory.to_str().unwrap())];
    all.extend_from_slice(vars);
    Env::fixed(&all, None)
}

#[test]
fn config_path_prefers_absolute_xdg_config_home() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("alice");
    let xdg = root.path().join("xdg");
    assert_eq!(
        config_path(&Env::fixed(
            &[("XDG_CONFIG_HOME", xdg.to_str().unwrap())],
            Some(&home)
        )),
        Some(xdg.join("oceans").join("config.yaml"))
    );
    for vars in [
        vec![],
        vec![("XDG_CONFIG_HOME", "relative/dir")],
        vec![("XDG_CONFIG_HOME", "")],
    ] {
        assert_eq!(
            config_path(&Env::fixed(&vars, Some(&home))),
            Some(home.join(".config").join("oceans").join("config.yaml"))
        );
    }
    assert_eq!(config_path(&Env::fixed(&[], None)), None);
}

#[test]
fn flags_win_over_environment_which_wins_over_the_file() {
    let directory = tempfile::tempdir().unwrap();
    let contents = "url: https://file.example.com\napi_key: file-key\n";
    let flag_url = Url::parse("https://flag.example.com").unwrap();
    let flags = Overrides {
        url: Some(&flag_url),
        api_key: Some("flag-key"),
    };
    let environment = [
        ("OCEANS_URL", "https://env.example.com"),
        ("OCEANS_API_KEY", "env-key"),
    ];

    let env = env_with_config(directory.path(), &environment, contents);
    let connection = resolve_connection(&flags, &env, TIMEOUT).unwrap();
    assert_eq!(connection.url.as_str(), "https://flag.example.com/");
    assert_eq!(connection.api_key, "flag-key");

    let connection = resolve_connection(&Overrides::default(), &env, TIMEOUT).unwrap();
    assert_eq!(connection.url.as_str(), "https://env.example.com/");
    assert_eq!(connection.api_key, "env-key");

    let env = env_with_config(directory.path(), &[], contents);
    let connection = resolve_connection(&Overrides::default(), &env, TIMEOUT).unwrap();
    assert_eq!(connection.url.as_str(), "https://file.example.com/");
    assert_eq!(connection.api_key, "file-key");

    let env = env_with_config(directory.path(), &[], "api_key: file-key\n");
    let connection = resolve_connection(&Overrides::default(), &env, TIMEOUT).unwrap();
    assert_eq!(connection.url.as_str(), "http://127.0.0.1:8080/");
}

#[test]
fn missing_key_and_invalid_urls_explain_the_fix() {
    let directory = tempfile::tempdir().unwrap();
    let env = Env::fixed(
        &[("XDG_CONFIG_HOME", directory.path().to_str().unwrap())],
        None,
    );
    let error = resolve_connection(&Overrides::default(), &env, TIMEOUT)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("oceans login"), "{error}");

    let env = env_with_config(directory.path(), &[], "url: not a url\napi_key: key\n");
    let error = resolve_connection(&Overrides::default(), &env, TIMEOUT)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("url in"), "{error}");
}

#[test]
fn api_key_forms_resolve_in_adr_order() {
    let env = Env::fixed(
        &[("OCEANS_INGEST_KEY", " gwk_from_env \n"), ("EMPTY", "")],
        None,
    );
    assert_eq!(
        KeySource::parse("gwk_literal")
            .resolve(&env, TIMEOUT)
            .unwrap(),
        "gwk_literal"
    );
    assert_eq!(
        KeySource::parse("env:OCEANS_INGEST_KEY")
            .resolve(&env, TIMEOUT)
            .unwrap(),
        "gwk_from_env"
    );
    for name in ["MISSING_KEY", "EMPTY"] {
        let error = KeySource::parse(&format!("env:{name}"))
            .resolve(&env, TIMEOUT)
            .unwrap_err()
            .to_string();
        assert!(error.contains(name), "{error}");
    }
    // A bare variable name is a literal key, never a variable lookup.
    assert_eq!(
        KeySource::parse("OCEANS_INGEST_KEY")
            .resolve(&env, TIMEOUT)
            .unwrap(),
        "OCEANS_INGEST_KEY"
    );
    assert!(
        KeySource::parse("env:not-a-name")
            .resolve(&env, TIMEOUT)
            .is_err()
    );
    assert_eq!(
        KeySource::parse("!fnox get KEY"),
        KeySource::Command("fnox get KEY".into())
    );
}

#[cfg(unix)]
#[test]
fn key_commands_return_trimmed_stdout_and_hide_output_on_failure() {
    let env = Env::fixed(&[], None);
    assert_eq!(
        KeySource::parse("!printf '  gwk_from_command\\n'")
            .resolve(&env, TIMEOUT)
            .unwrap(),
        "gwk_from_command"
    );
    let error = KeySource::parse("!printf gwk_partial_secret; exit 3")
        .resolve(&env, TIMEOUT)
        .unwrap_err()
        .to_string();
    assert!(error.contains("exit status: 3"), "{error}");
    assert!(!error.contains("gwk_partial_secret"), "{error}");
    let error = KeySource::parse("!exec sleep 5")
        .resolve(&env, Duration::from_millis(200))
        .unwrap_err()
        .to_string();
    assert!(error.contains("didn't finish"), "{error}");
    assert!(KeySource::parse("!true").resolve(&env, TIMEOUT).is_err());
}

#[cfg(unix)]
#[test]
fn config_files_other_users_can_write_are_refused() {
    use std::os::unix::fs::PermissionsExt;
    for api_key in [
        "\"!printf gwk_secret\"",
        "gwk_literal",
        "env:OCEANS_INGEST_KEY",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let env = env_with_config(
            directory.path(),
            &[("OCEANS_INGEST_KEY", "gwk_env")],
            &format!("api_key: {api_key}\n"),
        );
        let path = directory.path().join("oceans/config.yaml");
        for mode in [0o620, 0o602, 0o666] {
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
            let error = resolve_connection(&Overrides::default(), &env, TIMEOUT)
                .err()
                .unwrap()
                .to_string();
            assert!(error.contains("chmod 600"), "{error}");
        }
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(resolve_connection(&Overrides::default(), &env, TIMEOUT).is_ok());
    }
}

#[cfg(unix)]
#[test]
fn config_files_other_users_own_are_refused() {
    let path = Path::new("config.yaml");
    assert!(ensure_owned(path, 1000, 1000).is_ok());
    let error = ensure_owned(path, 1001, 1000).unwrap_err().to_string();
    assert!(error.contains("belongs to another user"), "{error}");
}

#[test]
fn unquoted_commands_are_rejected_with_the_quoted_form() {
    let error = parse_config("api_key: !fnox get OCEANS_INGEST_KEY\n")
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(r#"api_key: "!fnox get OCEANS_INGEST_KEY""#),
        "{error}"
    );
    let error = parse_config("api_key: !op\n").unwrap_err().to_string();
    assert!(error.contains(r#"api_key: "!op""#), "{error}");
    assert_eq!(
        parse_config("api_key: \"!fnox get OCEANS_INGEST_KEY\"\n").unwrap(),
        ConfigFile {
            url: None,
            api_key: Some("!fnox get OCEANS_INGEST_KEY".into())
        }
    );
}

#[test]
fn config_files_reject_unknown_settings_and_non_strings() {
    assert!(parse_config("apikey: x\n").is_err());
    assert!(parse_config("url: [1]\n").is_err());
    assert!(parse_config("- url\n").is_err());
    assert_eq!(parse_config("").unwrap(), ConfigFile::default());
    assert_eq!(parse_config("url: ~\n").unwrap(), ConfigFile::default());
}

#[test]
fn written_config_round_trips_with_private_permissions() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("oceans/config.yaml");
    let url = Url::parse("https://oceans.example.com").unwrap();
    for api_key in [
        "!fnox get OCEANS_INGEST_KEY",
        "env:OCEANS_INGEST_KEY",
        "gwk_a\"b\\c",
    ] {
        write_config(&path, &url, api_key).unwrap();
        let config = read_config(&path).unwrap().unwrap();
        assert_eq!(config.url.as_deref(), Some("https://oceans.example.com/"));
        assert_eq!(config.api_key.as_deref(), Some(api_key));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
    }
}
