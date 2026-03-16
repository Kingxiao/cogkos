//! Data encryption module
//!
//! Provides:
//! - AES-256-GCM encryption
//! - Key management
//! - Sensitive field encryption utilities

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use parking_lot::RwLock;
use rand::rngs::OsRng;
use rand::TryRngCore as _;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Encryption key manager
pub struct KeyManager {
    key: Arc<RwLock<Option<[u8; 32]>>>,
}

impl KeyManager {
    /// Create a new key manager
    pub fn new() -> Self {
        Self {
            key: Arc::new(RwLock::new(None)),
        }
    }

    /// Generate a new encryption key
    pub fn generate_key(&self) -> Result<[u8; 32], String> {
        let mut key = [0u8; 32];
        OsRng
            .try_fill_bytes(&mut key)
            .map_err(|e| format!("OS RNG failed: {}", e))?;
        *self.key.write() = Some(key);
        Ok(key)
    }

    /// Set encryption key from raw bytes
    pub fn set_key(&self, key: [u8; 32]) {
        *self.key.write() = Some(key);
    }

    /// Set encryption key from base64 encoded string
    pub fn set_key_from_base64(&self, key_str: &str) -> Result<(), String> {
        let key_bytes = BASE64
            .decode(key_str)
            .map_err(|e| format!("Invalid base64 key: {}", e))?;

        if key_bytes.len() != 32 {
            return Err("Key must be 32 bytes".to_string());
        }

        let mut key = [0u8; 32];
        key.copy_from_slice(&key_bytes);
        self.set_key(key);
        Ok(())
    }

    /// Get current key (returns None if not set)
    pub fn get_key(&self) -> Option<[u8; 32]> {
        *self.key.read()
    }

    /// Check if key is set
    pub fn has_key(&self) -> bool {
        self.key.read().is_some()
    }
}

impl Default for KeyManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Encryptor using AES-256-GCM
pub struct Encryptor {
    key_manager: Arc<KeyManager>,
}

impl Encryptor {
    /// Create a new encryptor with key manager
    pub fn new(key_manager: Arc<KeyManager>) -> Self {
        Self { key_manager }
    }

    /// Encrypt data
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        let key = self.key_manager.get_key().ok_or("Encryption key not set")?;

        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| format!("Failed to create cipher: {}", e))?;

        // Generate random nonce
        let mut nonce_bytes = [0u8; 12];
        OsRng
            .try_fill_bytes(&mut nonce_bytes)
            .map_err(|e| format!("OS RNG failed: {}", e))?;
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Encrypt
        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| format!("Encryption failed: {}", e))?;

        // Prepend nonce to ciphertext
        let mut result = nonce_bytes.to_vec();
        result.extend(ciphertext);
        Ok(result)
    }

    /// Decrypt data
    pub fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        if data.len() < 12 {
            return Err("Data too short".to_string());
        }

        let key = self.key_manager.get_key().ok_or("Encryption key not set")?;

        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| format!("Failed to create cipher: {}", e))?;

        // Extract nonce and ciphertext
        let nonce = Nonce::from_slice(&data[..12]);
        let ciphertext = &data[12..];

        // Decrypt
        cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| format!("Decryption failed: {}", e))
    }

    /// Encrypt string and return base64 encoded
    pub fn encrypt_string(&self, plaintext: &str) -> Result<String, String> {
        let encrypted = self.encrypt(plaintext.as_bytes())?;
        Ok(BASE64.encode(&encrypted))
    }

    /// Decrypt base64 encoded string
    pub fn decrypt_string(&self, encrypted_b64: &str) -> Result<String, String> {
        let encrypted = BASE64
            .decode(encrypted_b64)
            .map_err(|e| format!("Invalid base64: {}", e))?;
        let decrypted = self.decrypt(&encrypted)?;
        String::from_utf8(decrypted).map_err(|e| format!("Invalid UTF-8: {}", e))
    }
}

/// Encrypted field wrapper
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EncryptedField {
    /// Base64 encoded encrypted data
    pub ciphertext: String,
}

impl EncryptedField {
    /// Create from plaintext
    pub fn new(plaintext: &str, encryptor: &Encryptor) -> Result<Self, String> {
        let ciphertext = encryptor.encrypt_string(plaintext)?;
        Ok(Self { ciphertext })
    }

    /// Decrypt to plaintext
    pub fn decrypt(&self, encryptor: &Encryptor) -> Result<String, String> {
        encryptor.decrypt_string(&self.ciphertext)
    }
}

/// Sensitive data types that should be encrypted
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SensitiveData {
    /// API key
    ApiKey {
        #[serde(skip_serializing)]
        value: String,
    },
    /// Password
    Password {
        #[serde(skip_serializing)]
        value: String,
    },
    /// Secret token
    Token {
        #[serde(skip_serializing)]
        value: String,
    },
    /// Generic encrypted value
    Encrypted(EncryptedField),
}

impl SensitiveData {
    /// Create encrypted API key
    pub fn new_api_key(value: String, encryptor: &Encryptor) -> Result<Self, String> {
        let encrypted = EncryptedField::new(&value, encryptor)?;
        Ok(SensitiveData::Encrypted(encrypted))
    }

    /// Create encrypted password
    pub fn new_password(value: String, encryptor: &Encryptor) -> Result<Self, String> {
        let encrypted = EncryptedField::new(&value, encryptor)?;
        Ok(SensitiveData::Encrypted(encrypted))
    }

    /// Create encrypted token
    pub fn new_token(value: String, encryptor: &Encryptor) -> Result<Self, String> {
        let encrypted = EncryptedField::new(&value, encryptor)?;
        Ok(SensitiveData::Encrypted(encrypted))
    }

    /// Decrypt and get value
    pub fn get_value(&self, encryptor: &Encryptor) -> Result<String, String> {
        match self {
            SensitiveData::ApiKey { value } => Ok(value.clone()),
            SensitiveData::Password { value } => Ok(value.clone()),
            SensitiveData::Token { value } => Ok(value.clone()),
            SensitiveData::Encrypted(field) => field.decrypt(encryptor),
        }
    }
}

lazy_static::lazy_static! {
    pub static ref ENCRYPTOR: Encryptor = {
        let key_manager = Arc::new(KeyManager::new());
        // In production, load key from secure storage
        Encryptor::new(key_manager)
    };
}

/// Encrypt a string using global encryptor
pub fn encrypt(plaintext: &str) -> Result<String, String> {
    ENCRYPTOR.encrypt_string(plaintext)
}

/// Decrypt a string using global encryptor
pub fn decrypt(encrypted: &str) -> Result<String, String> {
    ENCRYPTOR.decrypt_string(encrypted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_manager() {
        let manager = KeyManager::new();
        assert!(!manager.has_key());

        let key = manager.generate_key().unwrap();
        assert!(manager.has_key());
        assert_eq!(key.len(), 32);
    }

    #[test]
    fn test_encrypt_decrypt() {
        let key_manager = Arc::new(KeyManager::new());
        key_manager.generate_key().unwrap();
        let encryptor = Encryptor::new(key_manager);

        let plaintext = "Hello, World!";
        let encrypted = encryptor.encrypt_string(plaintext).unwrap();
        let decrypted = encryptor.decrypt_string(&encrypted).unwrap();

        assert_eq!(plaintext, decrypted);
    }

    #[test]
    fn test_encrypted_field() {
        let key_manager = Arc::new(KeyManager::new());
        key_manager.generate_key().unwrap();
        let encryptor = Encryptor::new(key_manager);

        let field = EncryptedField::new("secret", &encryptor).unwrap();
        let decrypted = field.decrypt(&encryptor).unwrap();

        assert_eq!(decrypted, "secret");
    }

    #[test]
    fn test_sensitive_data() {
        let key_manager = Arc::new(KeyManager::new());
        key_manager.generate_key().unwrap();
        let encryptor = Encryptor::new(key_manager);

        let api_key = SensitiveData::new_api_key("sk-12345".to_string(), &encryptor).unwrap();
        let value = api_key.get_value(&encryptor).unwrap();

        assert_eq!(value, "sk-12345");
    }
}
