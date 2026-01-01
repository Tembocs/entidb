//! Encryption implementation using AES-256-GCM.

use crate::error::{CoreError, CoreResult};
use aes_gcm::{
    aead::{Aead, KeyInit, generic_array::GenericArray},
    Aes256Gcm, Nonce,
};
use rand::RngCore;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Size of the AES-256 key in bytes.
pub const KEY_SIZE: usize = 32;
/// Size of the GCM nonce in bytes.
pub const NONCE_SIZE: usize = 12;
/// Size of the GCM authentication tag in bytes.
pub const TAG_SIZE: usize = 16;

/// Encryption key for AES-256-GCM.
///
/// The key is automatically zeroized when dropped for security.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct EncryptionKey {
    bytes: [u8; KEY_SIZE],
}

impl EncryptionKey {
    /// Generates a new random encryption key.
    #[must_use]
    pub fn generate() -> Self {
        let mut bytes = [0u8; KEY_SIZE];
        rand::thread_rng().fill_bytes(&mut bytes);
        Self { bytes }
    }

    /// Creates a key from raw bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the bytes slice is not exactly 32 bytes.
    pub fn from_bytes(bytes: &[u8]) -> CoreResult<Self> {
        if bytes.len() != KEY_SIZE {
            return Err(CoreError::invalid_key_size(bytes.len(), KEY_SIZE));
        }

        let mut key_bytes = [0u8; KEY_SIZE];
        key_bytes.copy_from_slice(bytes);
        Ok(Self { bytes: key_bytes })
    }

    /// Returns the key as a byte slice.
    ///
    /// # Security
    ///
    /// Be careful with this method - don't log or serialize the result.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; KEY_SIZE] {
        &self.bytes
    }

    /// Derives a key from a password using HKDF-SHA256.
    ///
    /// # Arguments
    ///
    /// * `password` - The password to derive from
    /// * `salt` - A unique salt for this database (should be random and stored)
    ///
    /// # Security Note
    ///
    /// HKDF is a key derivation function, not a password hashing function.
    /// For maximum security with user-chosen passwords, consider using Argon2id
    /// or PBKDF2 with a high iteration count. HKDF is appropriate when the input
    /// key material already has high entropy (e.g., a randomly generated passphrase).
    pub fn derive_from_password(password: &[u8], salt: &[u8]) -> CoreResult<Self> {
        use hkdf::Hkdf;
        use sha2::Sha256;

        // Use HKDF with SHA-256 for key derivation
        // The salt is used as the HKDF salt, password as IKM (input key material)
        let hk = Hkdf::<Sha256>::new(Some(salt), password);

        let mut bytes = [0u8; KEY_SIZE];
        // Use a fixed info string for the application context
        hk.expand(b"entidb-encryption-key-v1", &mut bytes)
            .map_err(|_| CoreError::key_derivation_failed("HKDF expand failed"))?;

        Ok(Self { bytes })
    }
}

impl std::fmt::Debug for EncryptionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EncryptionKey")
            .field("bytes", &"[REDACTED]")
            .finish()
    }
}

/// Manages encryption and decryption operations.
///
/// This is the main interface for encrypting and decrypting data.
/// It uses AES-256-GCM for authenticated encryption.
pub struct CryptoManager {
    cipher: Aes256Gcm,
}

impl CryptoManager {
    /// Creates a new crypto manager with the given key.
    #[must_use]
    pub fn new(key: EncryptionKey) -> Self {
        // Use GenericArray::from_slice which converts our fixed-size key directly.
        // This is infallible since EncryptionKey.bytes is always exactly KEY_SIZE (32) bytes,
        // which matches AES-256's key size requirement.
        let key_array = GenericArray::from_slice(key.as_bytes());
        let cipher = Aes256Gcm::new(key_array);
        Self { cipher }
    }

    /// Encrypts data using AES-256-GCM.
    ///
    /// The output format is: `nonce (12 bytes) || ciphertext || tag (16 bytes)`
    ///
    /// # Arguments
    ///
    /// * `plaintext` - The data to encrypt
    ///
    /// # Returns
    ///
    /// The encrypted data with nonce prepended.
    pub fn encrypt(&self, plaintext: &[u8]) -> CoreResult<Vec<u8>> {
        // Generate random nonce
        let mut nonce_bytes = [0u8; NONCE_SIZE];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Encrypt
        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext)
            .map_err(|_| CoreError::encryption_failed("encryption error"))?;

        // Prepend nonce
        let mut result = Vec::with_capacity(NONCE_SIZE + ciphertext.len());
        result.extend_from_slice(&nonce_bytes);
        result.extend(ciphertext);

        Ok(result)
    }

    /// Decrypts data that was encrypted with [`encrypt`](Self::encrypt).
    ///
    /// # Arguments
    ///
    /// * `ciphertext` - The encrypted data (with nonce prepended)
    ///
    /// # Returns
    ///
    /// The decrypted plaintext.
    ///
    /// # Errors
    ///
    /// Returns an error if decryption fails (wrong key, corrupted data, etc.).
    pub fn decrypt(&self, ciphertext: &[u8]) -> CoreResult<Vec<u8>> {
        if ciphertext.len() < NONCE_SIZE + TAG_SIZE {
            return Err(CoreError::decryption_failed("ciphertext too short"));
        }

        // Extract nonce
        let nonce = Nonce::from_slice(&ciphertext[..NONCE_SIZE]);
        let encrypted = &ciphertext[NONCE_SIZE..];

        // Decrypt
        self.cipher
            .decrypt(nonce, encrypted)
            .map_err(|_| CoreError::decryption_failed("decryption error"))
    }

    /// Encrypts data with associated data (AEAD).
    ///
    /// The associated data is authenticated but not encrypted.
    /// This is useful for binding ciphertext to metadata.
    pub fn encrypt_with_aad(&self, plaintext: &[u8], aad: &[u8]) -> CoreResult<Vec<u8>> {
        use aes_gcm::aead::Payload;

        let mut nonce_bytes = [0u8; NONCE_SIZE];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let payload = Payload {
            msg: plaintext,
            aad,
        };

        let ciphertext = self
            .cipher
            .encrypt(nonce, payload)
            .map_err(|_| CoreError::encryption_failed("encryption error"))?;

        let mut result = Vec::with_capacity(NONCE_SIZE + ciphertext.len());
        result.extend_from_slice(&nonce_bytes);
        result.extend(ciphertext);

        Ok(result)
    }

    /// Decrypts data that was encrypted with [`encrypt_with_aad`](Self::encrypt_with_aad).
    ///
    /// The same AAD must be provided as was used during encryption.
    pub fn decrypt_with_aad(&self, ciphertext: &[u8], aad: &[u8]) -> CoreResult<Vec<u8>> {
        use aes_gcm::aead::Payload;

        if ciphertext.len() < NONCE_SIZE + TAG_SIZE {
            return Err(CoreError::decryption_failed("ciphertext too short"));
        }

        let nonce = Nonce::from_slice(&ciphertext[..NONCE_SIZE]);
        let encrypted = &ciphertext[NONCE_SIZE..];

        let payload = Payload {
            msg: encrypted,
            aad,
        };

        self.cipher
            .decrypt(nonce, payload)
            .map_err(|_| CoreError::decryption_failed("decryption error"))
    }
}

impl std::fmt::Debug for CryptoManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CryptoManager")
            .field("cipher", &"Aes256Gcm")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_key() {
        let key1 = EncryptionKey::generate();
        let key2 = EncryptionKey::generate();

        // Keys should be different
        assert_ne!(key1.as_bytes(), key2.as_bytes());
    }

    #[test]
    fn key_from_bytes() {
        let bytes = [42u8; KEY_SIZE];
        let key = EncryptionKey::from_bytes(&bytes).unwrap();
        assert_eq!(key.as_bytes(), &bytes);
    }

    #[test]
    fn key_wrong_size() {
        let short = [0u8; 16];
        assert!(EncryptionKey::from_bytes(&short).is_err());

        let long = [0u8; 64];
        assert!(EncryptionKey::from_bytes(&long).is_err());
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let key = EncryptionKey::generate();
        let manager = CryptoManager::new(key);

        let plaintext = b"Hello, EntiDB!";
        let ciphertext = manager.encrypt(plaintext).unwrap();

        // Ciphertext should be different from plaintext
        assert_ne!(&ciphertext[NONCE_SIZE..], plaintext);

        // Decrypt should recover plaintext
        let decrypted = manager.decrypt(&ciphertext).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn encrypt_produces_different_ciphertext() {
        let key = EncryptionKey::generate();
        let manager = CryptoManager::new(key);

        let plaintext = b"same data";
        let ct1 = manager.encrypt(plaintext).unwrap();
        let ct2 = manager.encrypt(plaintext).unwrap();

        // Due to random nonce, ciphertexts should differ
        assert_ne!(ct1, ct2);
    }

    #[test]
    fn decrypt_wrong_key_fails() {
        let key1 = EncryptionKey::generate();
        let key2 = EncryptionKey::generate();
        let manager1 = CryptoManager::new(key1);
        let manager2 = CryptoManager::new(key2);

        let plaintext = b"secret";
        let ciphertext = manager1.encrypt(plaintext).unwrap();

        // Wrong key should fail
        assert!(manager2.decrypt(&ciphertext).is_err());
    }

    #[test]
    fn decrypt_corrupted_data_fails() {
        let key = EncryptionKey::generate();
        let manager = CryptoManager::new(key);

        let plaintext = b"data";
        let mut ciphertext = manager.encrypt(plaintext).unwrap();

        // Corrupt the ciphertext
        let len = ciphertext.len();
        ciphertext[len - 1] ^= 0xFF;

        assert!(manager.decrypt(&ciphertext).is_err());
    }

    #[test]
    fn decrypt_too_short_fails() {
        let key = EncryptionKey::generate();
        let manager = CryptoManager::new(key);

        let short = vec![0u8; 10]; // Too short
        assert!(manager.decrypt(&short).is_err());
    }

    #[test]
    fn encrypt_decrypt_with_aad() {
        let key = EncryptionKey::generate();
        let manager = CryptoManager::new(key);

        let plaintext = b"secret";
        let aad = b"entity_id:12345";

        let ciphertext = manager.encrypt_with_aad(plaintext, aad).unwrap();
        let decrypted = manager.decrypt_with_aad(&ciphertext, aad).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn wrong_aad_fails() {
        let key = EncryptionKey::generate();
        let manager = CryptoManager::new(key);

        let plaintext = b"secret";
        let aad = b"correct_aad";
        let wrong_aad = b"wrong_aad";

        let ciphertext = manager.encrypt_with_aad(plaintext, aad).unwrap();

        // Decrypting with wrong AAD should fail
        assert!(manager.decrypt_with_aad(&ciphertext, wrong_aad).is_err());
    }

    #[test]
    fn derive_key_from_password() {
        let password = b"my_password";
        let salt = b"random_salt";

        let key1 = EncryptionKey::derive_from_password(password, salt).unwrap();
        let key2 = EncryptionKey::derive_from_password(password, salt).unwrap();

        // Same password + salt should produce same key
        assert_eq!(key1.as_bytes(), key2.as_bytes());

        // Different salt should produce different key
        let key3 = EncryptionKey::derive_from_password(password, b"other_salt").unwrap();
        assert_ne!(key1.as_bytes(), key3.as_bytes());
    }

    #[test]
    fn empty_plaintext() {
        let key = EncryptionKey::generate();
        let manager = CryptoManager::new(key);

        let plaintext = b"";
        let ciphertext = manager.encrypt(plaintext).unwrap();
        let decrypted = manager.decrypt(&ciphertext).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn large_plaintext() {
        let key = EncryptionKey::generate();
        let manager = CryptoManager::new(key);

        let plaintext = vec![0xAB; 1024 * 1024]; // 1 MB
        let ciphertext = manager.encrypt(&plaintext).unwrap();
        let decrypted = manager.decrypt(&ciphertext).unwrap();

        assert_eq!(decrypted, plaintext);
    }
}
