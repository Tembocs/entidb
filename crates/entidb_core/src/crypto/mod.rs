//! Cryptographic operations for EntiDB.
//!
//! This module provides encryption at rest using AES-256-GCM.
//! Encryption is optional and must be enabled via the `encryption` feature.
//!
//! ## Security Model
//!
//! - Uses AES-256-GCM for authenticated encryption
//! - Unique nonce per encryption operation
//! - Keys are zeroized on drop
//! - Key derivation uses HKDF when deriving from passwords
//!
//! ## Usage
//!
//! ```ignore
//! use entidb_core::crypto::{EncryptionKey, CryptoManager};
//!
//! let key = EncryptionKey::generate();
//! let manager = CryptoManager::new(key);
//!
//! let ciphertext = manager.encrypt(b"secret data")?;
//! let plaintext = manager.decrypt(&ciphertext)?;
//! ```

#[cfg(feature = "encryption")]
mod encrypted;

#[cfg(feature = "encryption")]
pub use encrypted::*;

/// Module contents when encryption feature is disabled.
#[cfg(not(feature = "encryption"))]
mod stub {
    use crate::error::{CoreError, CoreResult};

    /// Encryption key (stub when encryption disabled).
    #[derive(Debug, Clone)]
    pub struct EncryptionKey {
        _private: (),
    }

    impl EncryptionKey {
        /// Always returns an error when encryption is disabled.
        pub fn generate() -> CoreResult<Self> {
            Err(CoreError::encryption_not_enabled())
        }

        /// Always returns an error when encryption is disabled.
        pub fn from_bytes(_bytes: &[u8]) -> CoreResult<Self> {
            Err(CoreError::encryption_not_enabled())
        }

        /// Always returns an error when encryption is disabled.
        pub fn derive_from_password(_password: &[u8], _salt: &[u8]) -> CoreResult<Self> {
            Err(CoreError::encryption_not_enabled())
        }
    }

    /// Crypto manager (stub when encryption disabled).
    #[derive(Debug)]
    pub struct CryptoManager {
        _private: (),
    }

    impl CryptoManager {
        /// Always returns an error when encryption is disabled.
        pub fn new(_key: EncryptionKey) -> CoreResult<Self> {
            Err(CoreError::encryption_not_enabled())
        }

        /// Always returns an error when encryption is disabled.
        pub fn encrypt(&self, _data: &[u8]) -> CoreResult<Vec<u8>> {
            Err(CoreError::encryption_not_enabled())
        }

        /// Always returns an error when encryption is disabled.
        pub fn decrypt(&self, _data: &[u8]) -> CoreResult<Vec<u8>> {
            Err(CoreError::encryption_not_enabled())
        }
    }
}

#[cfg(not(feature = "encryption"))]
pub use stub::*;
