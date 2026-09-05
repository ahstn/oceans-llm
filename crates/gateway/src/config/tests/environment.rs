use std::{env, ffi::OsString};

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
