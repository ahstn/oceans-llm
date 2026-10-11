use std::{env, ffi::OsString};

use super::super::references::{
    resolve_copilot_private_key, resolve_path_reference, resolve_secret_reference,
    validate_env_reference_if_needed,
};

/// Restores modified variables even when a serial environment test panics.
pub(super) struct TestEnvironment(Vec<(&'static str, Option<OsString>)>);

impl TestEnvironment {
    pub(super) fn capture(keys: &[&'static str]) -> Self {
        Self(keys.iter().map(|&key| (key, env::var_os(key))).collect())
    }
}

impl Drop for TestEnvironment {
    fn drop(&mut self) {
        for (key, previous) in &self.0 {
            // Callers hold the serial-test lock while changing and restoring these values.
            unsafe {
                match previous {
                    Some(value) => env::set_var(key, value),
                    None => env::remove_var(key),
                }
            }
        }
    }
}

#[test]
#[serial_test::serial]
fn environment_references_preserve_valid_values() {
    const KEY: &str = "OCEANS_TEST_ENV_REFERENCE_VALID";
    let _environment = TestEnvironment::capture(&[KEY]);
    let reference = format!("env.{KEY}");

    for value in ["", "test-token", "  token-\u{2713}\n"] {
        unsafe { env::set_var(KEY, value) };
        assert_eq!(resolve_secret_reference(&reference).unwrap(), value);
        assert_eq!(resolve_path_reference(&reference).unwrap(), value);
        validate_env_reference_if_needed(&reference).unwrap();
    }
}

#[test]
#[serial_test::serial]
fn missing_environment_references_preserve_error_context() {
    const KEY: &str = "OCEANS_TEST_ENV_REFERENCE_MISSING";
    let _environment = TestEnvironment::capture(&[KEY]);
    unsafe { env::remove_var(KEY) };

    let error = resolve_secret_reference(&format!("env.{KEY}")).unwrap_err();
    assert_eq!(
        error.to_string(),
        format!("required environment variable `{KEY}` is not set")
    );
    assert!(matches!(
        error.downcast_ref::<env::VarError>(),
        Some(env::VarError::NotPresent)
    ));
}

#[cfg(any(unix, windows))]
#[test]
#[serial_test::serial]
fn non_unicode_environment_values_never_reach_error_output() {
    const KEY: &str = "OCEANS_TEST_ENV_REFERENCE_NON_UNICODE";
    const SECRET: &str = "synthetic-secret-canary";
    let _environment = TestEnvironment::capture(&[KEY]);
    #[cfg(unix)]
    let value = {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec([SECRET.as_bytes(), &[0xff]].concat())
    };
    #[cfg(windows)]
    let value = {
        use std::os::windows::ffi::OsStringExt;
        OsString::from_wide(&SECRET.encode_utf16().chain([0xd800]).collect::<Vec<_>>())
    };
    unsafe { env::set_var(KEY, value) };
    let reference = format!("env.{KEY}");

    for result in [
        resolve_secret_reference(&reference).map(|_| ()),
        resolve_path_reference(&reference).map(|_| ()),
        validate_env_reference_if_needed(&reference),
        resolve_copilot_private_key(&reference).map(|_| ()),
    ] {
        let error = result
            .expect_err("non-Unicode environment values must be rejected")
            .context("provider authentication configuration");
        for output in [
            format!("{error}"),
            format!("{error:#}"),
            format!("{error:?}"),
            format!("{error:#?}"),
        ] {
            assert!(
                !output.contains(SECRET),
                "secret value leaked in error output"
            );
        }
        for cause in error.chain() {
            assert!(!cause.to_string().contains(SECRET));
            assert!(!format!("{cause:?}").contains(SECRET));
        }
        assert_eq!(
            error.root_cause().to_string(),
            format!("environment variable `{KEY}` is not valid UTF-8")
        );
        assert!(error.downcast_ref::<env::VarError>().is_none());
    }
}
