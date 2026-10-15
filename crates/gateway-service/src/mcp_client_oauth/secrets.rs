use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

const SECRET_BYTES: usize = 32;

/// A 256-bit random value, base64url-encoded behind `prefix`.
#[must_use]
pub(crate) fn generate_secret(prefix: &str) -> String {
    let mut bytes = [0_u8; SECRET_BYTES];
    rand::fill(&mut bytes);
    format!("{prefix}{}", URL_SAFE_NO_PAD.encode(bytes))
}

/// The stored form of a token, code, or other high-entropy secret.
///
/// SHA-256 suffices because the inputs are 256-bit random values; Argon2 is
/// reserved for secrets a user could guess.
#[must_use]
pub fn oauth_secret_hash(secret: &str) -> String {
    let digest = Sha256::digest(secret.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// RFC 7636 S256: `BASE64URL(SHA256(code_verifier)) == code_challenge`.
#[must_use]
pub(crate) fn pkce_s256_matches(code_verifier: &str, code_challenge: &str) -> bool {
    if !is_valid_code_verifier(code_verifier) {
        return false;
    }
    let computed = URL_SAFE_NO_PAD.encode(Sha256::digest(code_verifier.as_bytes()));
    constant_time_eq(computed.as_bytes(), code_challenge.as_bytes())
}

/// RFC 7636 section 4.1: 43 to 128 unreserved characters.
fn is_valid_code_verifier(value: &str) -> bool {
    (43..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~'))
}

/// A code challenge is the base64url form of a SHA-256 digest: 43 characters.
#[must_use]
pub(crate) fn is_valid_s256_challenge(value: &str) -> bool {
    value.len() == 43
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0_u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_secrets_are_prefixed_and_unique() {
        let first = generate_secret("oat_");
        let second = generate_secret("oat_");
        assert!(first.starts_with("oat_"));
        assert_eq!(first.len(), 4 + 43);
        assert_ne!(first, second);
        assert_eq!(oauth_secret_hash(&first).len(), 64);
    }

    #[test]
    fn pkce_matches_the_rfc_7636_example() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
        assert!(is_valid_s256_challenge(challenge));
        assert!(pkce_s256_matches(verifier, challenge));
        assert!(!pkce_s256_matches(
            "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXX",
            challenge
        ));
        assert!(!pkce_s256_matches("short", challenge));
    }
}
