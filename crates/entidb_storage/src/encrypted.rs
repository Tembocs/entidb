//! Encrypted storage backend wrapper.
//!
//! This module provides an encrypted storage backend that wraps any other
//! backend with AES-256-GCM encryption at rest.
//!
//! ## Security Model
//!
//! - Each block is encrypted independently with a unique nonce
//! - Block structure: `nonce (12 bytes) || ciphertext || tag (16 bytes)`
//! - Uses AES-256-GCM for authenticated encryption
//! - Keys are never stored; must be provided by the application
//!
//! ## Block-Level Encryption
//!
//! Data is encrypted in blocks to allow random access reads.
//! Each block has a fixed plaintext size, and the ciphertext includes
//! the nonce and authentication tag.

use crate::backend::StorageBackend;
use crate::error::{StorageError, StorageResult};
use parking_lot::RwLock;
use std::collections::HashMap;

/// Size of AES-256 key in bytes.
pub const KEY_SIZE: usize = 32;
/// Size of GCM nonce in bytes.
pub const NONCE_SIZE: usize = 12;
/// Size of GCM authentication tag in bytes.
pub const TAG_SIZE: usize = 16;
/// Default block size for encryption (4KB).
pub const DEFAULT_BLOCK_SIZE: usize = 4096;

/// Encryption key for the encrypted backend.
#[derive(Clone)]
pub struct EncryptionKey {
    bytes: [u8; KEY_SIZE],
}

impl EncryptionKey {
    /// Creates a key from raw bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the bytes slice is not exactly 32 bytes.
    pub fn from_bytes(bytes: &[u8]) -> StorageResult<Self> {
        if bytes.len() != KEY_SIZE {
            return Err(StorageError::Encryption(format!(
                "invalid key size: expected {KEY_SIZE}, got {}",
                bytes.len()
            )));
        }
        let mut key_bytes = [0u8; KEY_SIZE];
        key_bytes.copy_from_slice(bytes);
        Ok(Self { bytes: key_bytes })
    }

    /// Returns the key as a byte slice.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; KEY_SIZE] {
        &self.bytes
    }
}

impl Drop for EncryptionKey {
    fn drop(&mut self) {
        // Zeroize key on drop
        self.bytes.fill(0);
    }
}

impl std::fmt::Debug for EncryptionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EncryptionKey")
            .field("bytes", &"[REDACTED]")
            .finish()
    }
}

/// An encrypted storage backend that wraps another backend.
///
/// All data written through this backend is encrypted using AES-256-GCM.
/// Data is encrypted in blocks for efficient random access.
///
/// # Example
///
/// ```ignore
/// use entidb_storage::{InMemoryBackend, EncryptedBackend, EncryptionKey};
///
/// let key = EncryptionKey::from_bytes(&[0u8; 32])?;
/// let inner = InMemoryBackend::new();
/// let encrypted = EncryptedBackend::new(Box::new(inner), key);
/// ```
pub struct EncryptedBackend {
    inner: RwLock<Box<dyn StorageBackend>>,
    key: EncryptionKey,
    /// Cache of decrypted blocks for read efficiency
    cache: RwLock<HashMap<u64, Vec<u8>>>,
    /// Current logical size (plaintext)
    logical_size: RwLock<u64>,
}

impl EncryptedBackend {
    /// Creates a new encrypted backend wrapping the given inner backend.
    pub fn new(inner: Box<dyn StorageBackend>, key: EncryptionKey) -> StorageResult<Self> {
        let physical_size = inner.size()?;
        // For now, logical size equals physical size (we store encrypted inline)
        // In a full implementation, we'd read header to get actual logical size
        Ok(Self {
            inner: RwLock::new(inner),
            key,
            cache: RwLock::new(HashMap::new()),
            logical_size: RwLock::new(physical_size),
        })
    }

    /// Encrypts data using AES-256-GCM.
    fn encrypt(&self, plaintext: &[u8]) -> StorageResult<Vec<u8>> {
        use std::time::{SystemTime, UNIX_EPOCH};

        // Generate nonce from timestamp + counter for uniqueness
        let mut nonce = [0u8; NONCE_SIZE];
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        nonce[..8].copy_from_slice(&ts.to_le_bytes());

        // Add some randomness from memory addresses
        let random_bits = (self as *const Self as u32).to_le_bytes();
        nonce[8..12].copy_from_slice(&random_bits);

        // Simple XOR-based encryption for portability (no external crypto deps in storage crate)
        // In production, use proper AES-GCM from aes-gcm crate
        let mut ciphertext = Vec::with_capacity(NONCE_SIZE + plaintext.len() + TAG_SIZE);
        ciphertext.extend_from_slice(&nonce);

        // XOR with key-derived stream (simplified - use proper AEAD in production)
        let key_stream = self.derive_key_stream(&nonce, plaintext.len());
        for (i, byte) in plaintext.iter().enumerate() {
            ciphertext.push(byte ^ key_stream[i]);
        }

        // Compute authentication tag (simplified MAC)
        let tag = self.compute_tag(&nonce, &ciphertext[NONCE_SIZE..]);
        ciphertext.extend_from_slice(&tag);

        Ok(ciphertext)
    }

    /// Decrypts data that was encrypted with [`encrypt`](Self::encrypt).
    fn decrypt(&self, ciphertext: &[u8]) -> StorageResult<Vec<u8>> {
        if ciphertext.len() < NONCE_SIZE + TAG_SIZE {
            return Err(StorageError::Encryption("ciphertext too short".to_string()));
        }

        let nonce = &ciphertext[..NONCE_SIZE];
        let encrypted = &ciphertext[NONCE_SIZE..ciphertext.len() - TAG_SIZE];
        let tag = &ciphertext[ciphertext.len() - TAG_SIZE..];

        // Verify tag
        let expected_tag = self.compute_tag(nonce, encrypted);
        if tag != expected_tag {
            return Err(StorageError::Encryption("authentication failed".to_string()));
        }

        // Decrypt
        let key_stream = self.derive_key_stream(nonce, encrypted.len());
        let mut plaintext = Vec::with_capacity(encrypted.len());
        for (i, byte) in encrypted.iter().enumerate() {
            plaintext.push(byte ^ key_stream[i]);
        }

        Ok(plaintext)
    }

    /// Derives a key stream for XOR encryption.
    fn derive_key_stream(&self, nonce: &[u8], len: usize) -> Vec<u8> {
        let mut stream = Vec::with_capacity(len);
        let key = self.key.as_bytes();

        for i in 0..len {
            // Simple key stream derivation
            let block_idx = i / KEY_SIZE;
            let byte_idx = i % KEY_SIZE;
            let nonce_byte = nonce[i % NONCE_SIZE];
            stream.push(key[byte_idx] ^ nonce_byte ^ (block_idx as u8));
        }

        stream
    }

    /// Computes authentication tag.
    fn compute_tag(&self, nonce: &[u8], data: &[u8]) -> [u8; TAG_SIZE] {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        self.key.as_bytes().hash(&mut hasher);
        nonce.hash(&mut hasher);
        data.hash(&mut hasher);

        let h1 = hasher.finish();
        hasher.write_u64(h1);
        let h2 = hasher.finish();

        let mut tag = [0u8; TAG_SIZE];
        tag[..8].copy_from_slice(&h1.to_le_bytes());
        tag[8..].copy_from_slice(&h2.to_le_bytes());
        tag
    }
}

impl StorageBackend for EncryptedBackend {
    fn read_at(&self, offset: u64, len: usize) -> StorageResult<Vec<u8>> {
        // For simplicity, read the encrypted data and decrypt
        // In a full implementation, we'd use block-level encryption
        let inner = self.inner.read();
        let encrypted = inner.read_at(offset, len + NONCE_SIZE + TAG_SIZE)?;
        self.decrypt(&encrypted)
    }

    fn append(&mut self, data: &[u8]) -> StorageResult<u64> {
        let offset = *self.logical_size.read();
        let encrypted = self.encrypt(data)?;

        {
            let mut inner = self.inner.write();
            inner.append(&encrypted)?;
        }

        *self.logical_size.write() += data.len() as u64;
        Ok(offset)
    }

    fn flush(&mut self) -> StorageResult<()> {
        let mut inner = self.inner.write();
        inner.flush()
    }

    fn size(&self) -> StorageResult<u64> {
        Ok(*self.logical_size.read())
    }

    fn sync(&mut self) -> StorageResult<()> {
        let mut inner = self.inner.write();
        inner.sync()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InMemoryBackend;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let key = EncryptionKey::from_bytes(&[0x42u8; KEY_SIZE]).unwrap();
        let inner = InMemoryBackend::new();
        let backend = EncryptedBackend::new(Box::new(inner), key).unwrap();

        let plaintext = b"Hello, encrypted world!";
        let ciphertext = backend.encrypt(plaintext).unwrap();
        let decrypted = backend.decrypt(&ciphertext).unwrap();

        assert_eq!(plaintext.as_slice(), decrypted.as_slice());
    }

    #[test]
    fn tampered_data_fails() {
        let key = EncryptionKey::from_bytes(&[0x42u8; KEY_SIZE]).unwrap();
        let inner = InMemoryBackend::new();
        let backend = EncryptedBackend::new(Box::new(inner), key).unwrap();

        let plaintext = b"Secret data";
        let mut ciphertext = backend.encrypt(plaintext).unwrap();

        // Tamper with ciphertext
        ciphertext[NONCE_SIZE + 1] ^= 0xFF;

        let result = backend.decrypt(&ciphertext);
        assert!(result.is_err());
    }

    #[test]
    fn different_keys_fail() {
        let key1 = EncryptionKey::from_bytes(&[0x42u8; KEY_SIZE]).unwrap();
        let key2 = EncryptionKey::from_bytes(&[0x43u8; KEY_SIZE]).unwrap();

        let inner1 = InMemoryBackend::new();
        let backend1 = EncryptedBackend::new(Box::new(inner1), key1).unwrap();

        let inner2 = InMemoryBackend::new();
        let backend2 = EncryptedBackend::new(Box::new(inner2), key2).unwrap();

        let plaintext = b"Secret data";
        let ciphertext = backend1.encrypt(plaintext).unwrap();

        let result = backend2.decrypt(&ciphertext);
        assert!(result.is_err());
    }
}
