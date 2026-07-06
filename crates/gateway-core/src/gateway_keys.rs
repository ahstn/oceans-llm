use aes_gcm::{
    Aes256Gcm, KeyInit,
    aead::{Aead, OsRng, rand_core::RngCore},
};
use argon2::{
    Argon2,
    password_hash::{PasswordHasher, SaltString},
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use uuid::Uuid;

use crate::GatewayError;

pub const GATEWAY_API_KEY_SECRET_KEY_ENV: &str = "OCEANS_API_KEY_SECRET_ENCRYPTION_KEY";
pub const GATEWAY_API_KEY_SECRET_KEY_ID: &str = "env/OCEANS_API_KEY_SECRET_ENCRYPTION_KEY";

pub struct EncryptedSecret {
    pub ciphertext: String,
    pub nonce: String,
    pub key_id: &'static str,
}

pub fn generate_gateway_api_key_value() -> String {
    let public_id = Uuid::new_v4().simple().to_string();
    let secret = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    format!("gwk_{public_id}.{secret}")
}

pub fn hash_gateway_key_secret(secret: &str) -> Result<String, GatewayError> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(secret.as_bytes(), &salt)
        .map_err(|error| {
            GatewayError::Internal(format!("failed to hash gateway key secret: {error}"))
        })?
        .to_string();

    Ok(hash)
}

pub fn encrypt_gateway_api_key_secret(secret: &str) -> Result<EncryptedSecret, GatewayError> {
    encrypt_secret_with_key(
        secret,
        GATEWAY_API_KEY_SECRET_KEY_ENV,
        GATEWAY_API_KEY_SECRET_KEY_ID,
        "API key secret",
    )
}

pub fn decrypt_gateway_api_key_secret(
    ciphertext: &str,
    nonce: &str,
    key_id: &str,
) -> Result<String, GatewayError> {
    decrypt_secret_with_key(
        ciphertext,
        nonce,
        key_id,
        GATEWAY_API_KEY_SECRET_KEY_ENV,
        GATEWAY_API_KEY_SECRET_KEY_ID,
        "API key secret",
    )
}

pub fn encrypt_secret_with_key(
    secret: &str,
    key_env: &'static str,
    key_id: &'static str,
    label: &'static str,
) -> Result<EncryptedSecret, GatewayError> {
    let key = cipher_key(key_env)?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|error| GatewayError::Internal(format!("invalid {label} cipher key: {error}")))?;
    let mut nonce_bytes = [0_u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let ciphertext = cipher
        .encrypt((&nonce_bytes).into(), secret.as_bytes())
        .map_err(|error| GatewayError::Internal(format!("failed encrypting {label}: {error}")))?;

    Ok(EncryptedSecret {
        ciphertext: BASE64.encode(ciphertext),
        nonce: BASE64.encode(nonce_bytes),
        key_id,
    })
}

pub fn decrypt_secret_with_key(
    ciphertext: &str,
    nonce: &str,
    key_id: &str,
    key_env: &'static str,
    expected_key_id: &'static str,
    label: &'static str,
) -> Result<String, GatewayError> {
    if key_id != expected_key_id {
        return Err(GatewayError::InvalidRequest(format!(
            "{label} was encrypted with an unknown key id"
        )));
    }

    let key = cipher_key(key_env)?;
    let nonce = BASE64.decode(nonce).map_err(|error| {
        GatewayError::InvalidRequest(format!("{label} nonce is invalid: {error}"))
    })?;
    if nonce.len() != 12 {
        return Err(GatewayError::InvalidRequest(format!(
            "{label} nonce must decode to exactly 12 bytes"
        )));
    }
    let ciphertext = BASE64.decode(ciphertext).map_err(|error| {
        GatewayError::InvalidRequest(format!("{label} ciphertext is invalid: {error}"))
    })?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|error| GatewayError::Internal(format!("invalid {label} cipher key: {error}")))?;
    let plaintext = cipher
        .decrypt(nonce.as_slice().into(), ciphertext.as_ref())
        .map_err(|_| GatewayError::InvalidRequest(format!("{label} could not be decrypted")))?;
    String::from_utf8(plaintext)
        .map_err(|error| GatewayError::InvalidRequest(format!("{label} is not UTF-8: {error}")))
}

pub fn validate_secret_key_env(key_env: &'static str) -> Result<(), GatewayError> {
    cipher_key(key_env).map(|_| ())
}

fn cipher_key(key_env: &'static str) -> Result<Vec<u8>, GatewayError> {
    let raw = std::env::var(key_env).map_err(|_| {
        GatewayError::InvalidRequest(format!(
            "{key_env} must be configured before encrypted secrets can be used"
        ))
    })?;
    let key = BASE64.decode(raw.trim()).map_err(|error| {
        GatewayError::InvalidRequest(format!("{key_env} must be base64: {error}"))
    })?;
    if key.len() != 32 {
        return Err(GatewayError::InvalidRequest(format!(
            "{key_env} must decode to exactly 32 bytes"
        )));
    }
    Ok(key)
}

#[cfg(test)]
mod tests {
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

    use super::{decrypt_secret_with_key, encrypt_secret_with_key, validate_secret_key_env};

    const TEST_KEY_ENV: &str = "OCEANS_TEST_SECRET_STORAGE_KEY";
    const TEST_KEY_ID: &str = "env/OCEANS_TEST_SECRET_STORAGE_KEY";
    const TEST_KEY: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

    #[test]
    fn encrypts_and_decrypts_secret_material_with_key_id() {
        unsafe {
            std::env::set_var(TEST_KEY_ENV, TEST_KEY);
        }

        validate_secret_key_env(TEST_KEY_ENV).expect("valid test key");
        let encrypted = encrypt_secret_with_key(
            "gwk_public.secret",
            TEST_KEY_ENV,
            TEST_KEY_ID,
            "test secret",
        )
        .expect("encrypt");
        assert_ne!(encrypted.ciphertext, "gwk_public.secret");
        assert_eq!(encrypted.key_id, TEST_KEY_ID);

        let decrypted = decrypt_secret_with_key(
            &encrypted.ciphertext,
            &encrypted.nonce,
            encrypted.key_id,
            TEST_KEY_ENV,
            TEST_KEY_ID,
            "test secret",
        )
        .expect("decrypt");
        assert_eq!(decrypted, "gwk_public.secret");

        let wrong_key = decrypt_secret_with_key(
            &encrypted.ciphertext,
            &encrypted.nonce,
            "env/other",
            TEST_KEY_ENV,
            TEST_KEY_ID,
            "test secret",
        )
        .expect_err("wrong key id should fail");
        assert!(wrong_key.to_string().contains("unknown key id"));
    }

    #[test]
    fn rejects_invalid_nonce_lengths_without_panicking() {
        unsafe {
            std::env::set_var(TEST_KEY_ENV, TEST_KEY);
        }

        let encrypted = encrypt_secret_with_key(
            "gwk_public.secret",
            TEST_KEY_ENV,
            TEST_KEY_ID,
            "test secret",
        )
        .expect("encrypt");
        let short_nonce = BASE64.encode([0_u8; 11]);

        let error = decrypt_secret_with_key(
            &encrypted.ciphertext,
            &short_nonce,
            encrypted.key_id,
            TEST_KEY_ENV,
            TEST_KEY_ID,
            "test secret",
        )
        .expect_err("short nonce should fail");

        assert!(error.to_string().contains("exactly 12 bytes"));
    }
}
