use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use argon2::{
    password_hash::{PasswordHasher, SaltString},
    Argon2,
};
use rand::RngCore;

const NONCE_SIZE: usize = 12;

/// Derive a key from a passphrase using Argon2
fn derive_key(passphrase: &str, salt: &[u8]) -> Result<[u8; 32], String> {
    let argon2 = Argon2::default();
    let salt_str = SaltString::encode_b64(salt)
        .map_err(|e| format!("Failed to encode salt: {}", e))?;

    let password_hash = argon2
        .hash_password(passphrase.as_bytes(), &salt_str)
        .map_err(|e| format!("Failed to hash password: {}", e))?;

    let hash = password_hash.hash.ok_or("No hash generated")?;
    let hash_bytes = hash.as_bytes();

    let mut key = [0u8; 32];
    let len = std::cmp::min(hash_bytes.len(), 32);
    key[..len].copy_from_slice(&hash_bytes[..len]);

    Ok(key)
}

/// Encrypt data using AES-256-GCM with passphrase
/// Format: [salt (16 bytes)][nonce (12 bytes)][ciphertext]
pub fn encrypt_data(data: &[u8], passphrase: &str) -> Result<Vec<u8>, String> {
    // Generate random salt
    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);

    // Derive key from passphrase
    let key = derive_key(passphrase, &salt)?;

    // Create cipher
    let cipher = Aes256Gcm::new(&key.into());

    // Generate random nonce
    let mut nonce_bytes = [0u8; NONCE_SIZE];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    // Encrypt
    let ciphertext = cipher
        .encrypt(nonce, data)
        .map_err(|e| format!("Encryption failed: {}", e))?;

    // Combine salt + nonce + ciphertext
    let mut result = Vec::with_capacity(16 + NONCE_SIZE + ciphertext.len());
    result.extend_from_slice(&salt);
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);

    Ok(result)
}

/// Decrypt data using AES-256-GCM with passphrase
/// Format: [salt (16 bytes)][nonce (12 bytes)][ciphertext]
pub fn decrypt_data(encrypted: &[u8], passphrase: &str) -> Result<Vec<u8>, String> {
    if encrypted.len() < 16 + NONCE_SIZE {
        return Err("Invalid encrypted data: too short".to_string());
    }

    // Extract salt, nonce, and ciphertext
    let salt = &encrypted[..16];
    let nonce_bytes = &encrypted[16..16 + NONCE_SIZE];
    let ciphertext = &encrypted[16 + NONCE_SIZE..];

    // Derive key from passphrase
    let key = derive_key(passphrase, salt)?;

    // Create cipher
    let cipher = Aes256Gcm::new(&key.into());
    let nonce = Nonce::from_slice(nonce_bytes);

    // Decrypt
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| format!("Decryption failed: {}", e))?;

    Ok(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt() {
        let data = b"Hello, World!";
        let passphrase = "my-secret-password";

        let encrypted = encrypt_data(data, passphrase).unwrap();
        assert_ne!(data.to_vec(), encrypted);

        let decrypted = decrypt_data(&encrypted, passphrase).unwrap();
        assert_eq!(data.to_vec(), decrypted);
    }

    #[test]
    fn test_wrong_passphrase() {
        let data = b"Hello, World!";
        let encrypted = encrypt_data(data, "correct-password").unwrap();

        let result = decrypt_data(&encrypted, "wrong-password");
        assert!(result.is_err());
    }
}
