//! Vault — AES-256-GCM encryption at rest for sensitive config values.
//!
//! Secrets are encrypted with a randomly generated 256-bit key stored in
//! `~/.zoidclaw/vault.key`. The key file is created on first use.
//!
//! Encrypted values are prefixed with `vault:` followed by the base64-encoded
//! nonce + ciphertext. Plain values (without the prefix) are returned as-is,
//! allowing graceful migration.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand::RngCore;
use std::fs;
use std::path::PathBuf;

/// Prefix for encrypted values stored in config.
const VAULT_PREFIX: &str = "vault:";

/// Length of AES-256-GCM nonce (96 bits).
const NONCE_LEN: usize = 12;

/// Length of AES-256 key (256 bits).
const KEY_LEN: usize = 32;

// ── Key Management ─────────────────────────────────────────────────

/// Get the path to the vault key file.
fn vault_key_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".zoidclaw")
        .join("vault.key")
}

/// Load or generate the vault encryption key.
///
/// On first call, generates a cryptographically random 256-bit key
/// and saves it to `~/.zoidclaw/vault.key`.
fn load_or_create_key() -> anyhow::Result<[u8; KEY_LEN]> {
    let path = vault_key_path();

    if path.exists() {
        let data = fs::read(&path)?;
        if data.len() != KEY_LEN {
            anyhow::bail!(
                "vault.key has invalid length: {} (expected {})",
                data.len(),
                KEY_LEN
            );
        }
        let mut key = [0u8; KEY_LEN];
        key.copy_from_slice(&data);
        return Ok(key);
    }

    // Generate a new random key
    let mut key = [0u8; KEY_LEN];
    rand::thread_rng().fill_bytes(&mut key);

    // Ensure the directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(&path, key)?;
    tracing::info!("Generated new vault key at {}", path.display());

    Ok(key)
}

// ── Public API ─────────────────────────────────────────────────────

/// Encrypt a plaintext secret and return a `vault:...` string for storage.
pub fn encrypt(plaintext: &str) -> anyhow::Result<String> {
    let key = load_or_create_key()?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| anyhow::anyhow!("cipher init: {}", e))?;

    // Random 96-bit nonce
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    #[allow(deprecated)]
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| anyhow::anyhow!("encrypt: {}", e))?;

    // Encode nonce + ciphertext as base64
    let mut combined = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);

    Ok(format!("{}{}", VAULT_PREFIX, B64.encode(&combined)))
}

/// Decrypt a `vault:...` string. If the value is not encrypted (no prefix),
/// returns it as-is — this allows gradual migration from plaintext.
pub fn decrypt(value: &str) -> anyhow::Result<String> {
    if !value.starts_with(VAULT_PREFIX) {
        // Not encrypted — return plaintext as-is (migration path)
        return Ok(value.to_string());
    }

    let encoded = &value[VAULT_PREFIX.len()..];
    let combined = B64.decode(encoded)
        .map_err(|e| anyhow::anyhow!("base64 decode: {}", e))?;

    if combined.len() < NONCE_LEN {
        anyhow::bail!("encrypted value too short");
    }

    let key = load_or_create_key()?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| anyhow::anyhow!("cipher init: {}", e))?;

    let (nonce_bytes, ciphertext) = combined.split_at(NONCE_LEN);
    #[allow(deprecated)]
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow::anyhow!("decrypt: {} (wrong vault.key?)", e))?;

    String::from_utf8(plaintext)
        .map_err(|e| anyhow::anyhow!("utf8 decode: {}", e))
}

/// Returns `true` if the value looks like a vault-encrypted string.
pub fn is_encrypted(value: &str) -> bool {
    value.starts_with(VAULT_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let secret = "sk-ant-api03-super-secret-key-1234567890";
        let encrypted = encrypt(secret).unwrap();
        assert!(encrypted.starts_with(VAULT_PREFIX));
        assert_ne!(encrypted, secret);

        let decrypted = decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, secret);
    }

    #[test]
    fn test_plaintext_passthrough() {
        let plain = "gsk_not_encrypted_value";
        let result = decrypt(plain).unwrap();
        assert_eq!(result, plain);
    }

    #[test]
    fn test_different_nonces() {
        let secret = "same-secret";
        let a = encrypt(secret).unwrap();
        let b = encrypt(secret).unwrap();
        // Each encryption should produce different ciphertext (random nonce)
        assert_ne!(a, b);
        assert_eq!(decrypt(&a).unwrap(), secret);
        assert_eq!(decrypt(&b).unwrap(), secret);
    }
}
