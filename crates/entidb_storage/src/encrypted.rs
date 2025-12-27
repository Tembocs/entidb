//! Encrypted storage backend wrapper.
//!
//! This module provides an encrypted storage backend that wraps any other
//! backend with AES-256-GCM encryption at rest.
//!
//! ## Security Model
//!
//! - All data is encrypted in fixed-size blocks (default 4KB plaintext)
//! - Each block is encrypted with AES-256-GCM
//! - Block structure: `nonce (12 bytes) || ciphertext (block_size) || tag (16 bytes)`
//! - Nonces are derived deterministically from block number for AC-01 compliance
//! - Keys are never stored; must be provided by the application
//! - Keys are zeroized on drop
//!
//! ## Block-Level Encryption
//!
//! Data is encrypted in fixed-size blocks to enable random access reads.
//! Each encrypted block has overhead of NONCE_SIZE + TAG_SIZE = 28 bytes.
//!
//! ```text
//! Physical layout:
//! [Header (32 bytes)][Block 0][Block 1][Block 2]...
//!
//! Each Block:
//! [Nonce (12 bytes)][Ciphertext (block_size bytes)][Tag (16 bytes)]
//! ```
//!
//! The header contains:
//! - Magic bytes (8 bytes): "ENTIDBEC"
//! - Version (4 bytes): format version
//! - Block size (4 bytes): plaintext block size
//! - Logical size (8 bytes): total plaintext bytes written
//! - Reserved (8 bytes): for future use
//!
//! ## Deterministic Nonces
//!
//! For AC-01 (determinism) compliance, nonces are derived from:
//! - A key-derived nonce key (via HMAC-like construction)
//! - The block number
//!
//! This ensures the same block number always produces the same nonce,
//! while different keys produce different nonce sequences.

use crate::backend::StorageBackend;
use crate::error::{StorageError, StorageResult};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use parking_lot::RwLock;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Size of AES-256 key in bytes.
pub const KEY_SIZE: usize = 32;
/// Size of GCM nonce in bytes.
pub const NONCE_SIZE: usize = 12;
/// Size of GCM authentication tag in bytes.
pub const TAG_SIZE: usize = 16;
/// Default block size for plaintext (4KB).
pub const DEFAULT_BLOCK_SIZE: usize = 4096;
/// Header size in bytes.
const HEADER_SIZE: usize = 32;
/// Magic bytes identifying encrypted EntiDB storage.
const MAGIC: &[u8; 8] = b"ENTIDBEC";
/// Current format version.
const FORMAT_VERSION: u32 = 1;
/// Size of the length prefix in each block (stores actual plaintext length).
const BLOCK_LEN_SIZE: usize = 4;

/// Overhead per encrypted block (nonce + tag).
const fn block_overhead() -> usize {
    NONCE_SIZE + TAG_SIZE
}

/// Calculate the physical size of an encrypted block.
/// Each block stores: [length (4 bytes)][plaintext (padded to block_size)][nonce][tag]
const fn encrypted_block_size(plaintext_block_size: usize) -> usize {
    BLOCK_LEN_SIZE + plaintext_block_size + block_overhead()
}

/// Encryption key for the encrypted backend.
///
/// The key is automatically zeroized when dropped for security.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
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

impl std::fmt::Debug for EncryptionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EncryptionKey")
            .field("bytes", &"[REDACTED]")
            .finish()
    }
}

/// Header for encrypted storage files.
#[derive(Debug, Clone, Copy)]
struct Header {
    /// Plaintext block size.
    block_size: u32,
    /// Total logical (plaintext) bytes written.
    logical_size: u64,
}

impl Header {
    fn new(block_size: u32) -> Self {
        Self {
            block_size,
            logical_size: 0,
        }
    }

    fn encode(&self) -> [u8; HEADER_SIZE] {
        let mut buf = [0u8; HEADER_SIZE];
        buf[0..8].copy_from_slice(MAGIC);
        buf[8..12].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
        buf[12..16].copy_from_slice(&self.block_size.to_le_bytes());
        buf[16..24].copy_from_slice(&self.logical_size.to_le_bytes());
        // bytes 24..32 reserved
        buf
    }

    fn decode(bytes: &[u8]) -> StorageResult<Self> {
        if bytes.len() < HEADER_SIZE {
            return Err(StorageError::Encryption("header too short".to_string()));
        }

        // Verify magic
        if &bytes[0..8] != MAGIC {
            return Err(StorageError::Encryption(
                "invalid magic bytes - not an encrypted EntiDB file".to_string(),
            ));
        }

        // Verify version
        let version = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        if version != FORMAT_VERSION {
            return Err(StorageError::Encryption(format!(
                "unsupported format version: {version}, expected {FORMAT_VERSION}"
            )));
        }

        let block_size = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
        let logical_size = u64::from_le_bytes(bytes[16..24].try_into().unwrap());

        // Validate block size is reasonable (1KB to 1MB)
        if block_size < 1024 || block_size > 1024 * 1024 {
            return Err(StorageError::Encryption(format!(
                "invalid block size: {block_size}"
            )));
        }

        Ok(Self {
            block_size,
            logical_size,
        })
    }
}

/// Derives a nonce key from the main encryption key.
///
/// This uses a simple HMAC-like construction to derive a separate key
/// for nonce generation, ensuring nonces are unique per encryption key.
fn derive_nonce_key(key: &[u8; KEY_SIZE]) -> [u8; KEY_SIZE] {
    // Simple key derivation: XOR with domain separator and hash-like mix
    // In production, consider using HKDF, but this is sufficient for
    // deterministic nonce derivation from block numbers.
    let mut nonce_key = [0u8; KEY_SIZE];
    let domain = b"EntiDB-Nonce-Key-Derivation-V1\x00\x00"; // 32 bytes

    for i in 0..KEY_SIZE {
        // Mix key with domain separator
        nonce_key[i] = key[i] ^ domain[i];
    }

    // Apply simple mixing rounds (not cryptographic, just for derivation)
    for round in 0..4 {
        let mut temp = [0u8; KEY_SIZE];
        for i in 0..KEY_SIZE {
            let prev = nonce_key[(i + KEY_SIZE - 1) % KEY_SIZE];
            let next = nonce_key[(i + 1) % KEY_SIZE];
            temp[i] = nonce_key[i]
                .wrapping_add(prev.rotate_left(3))
                .wrapping_add(next.rotate_right(5))
                .wrapping_add(round);
        }
        nonce_key = temp;
    }

    nonce_key
}

/// Derives a deterministic nonce for a given block number.
///
/// # Security
///
/// - The nonce is unique per (key, block_number) pair
/// - Same key + block = same nonce (determinism for AC-01)
/// - Different keys produce completely different nonce sequences
/// - Block numbers must never be reused with the same key for different data
fn derive_nonce(nonce_key: &[u8; KEY_SIZE], block_number: u64) -> [u8; NONCE_SIZE] {
    let mut nonce = [0u8; NONCE_SIZE];
    let block_bytes = block_number.to_le_bytes();

    // Mix block number with nonce key
    for i in 0..NONCE_SIZE {
        let key_byte = nonce_key[i % KEY_SIZE];
        let block_byte = block_bytes[i % 8];
        // Additional mixing with position
        nonce[i] = key_byte
            .wrapping_add(block_byte)
            .wrapping_add((i as u8).wrapping_mul(17))
            .rotate_left((block_number % 7) as u32 + 1);
    }

    nonce
}

/// An encrypted storage backend that wraps another backend.
///
/// All data written through this backend is encrypted using AES-256-GCM.
/// Data is encrypted in fixed-size blocks for efficient random access.
///
/// # Security Guarantees
///
/// - **Confidentiality**: Data is encrypted with AES-256-GCM
/// - **Integrity**: Each block has a 128-bit authentication tag
/// - **Determinism**: Same data + key produces identical ciphertext (AC-01)
/// - **Key security**: Keys are zeroized on drop
///
/// # Example
///
/// ```ignore
/// use entidb_storage::{InMemoryBackend, EncryptedBackend, EncryptionKey};
///
/// let key = EncryptionKey::from_bytes(&[0x42u8; 32])?;
/// let inner = InMemoryBackend::new();
/// let mut encrypted = EncryptedBackend::new(Box::new(inner), key)?;
///
/// let offset = encrypted.append(b"secret data")?;
/// let data = encrypted.read_at(offset, 11)?;
/// assert_eq!(&data, b"secret data");
/// ```
pub struct EncryptedBackend {
    /// The underlying storage backend.
    inner: RwLock<Box<dyn StorageBackend>>,
    /// AES-256-GCM cipher instance.
    cipher: Aes256Gcm,
    /// Derived key for nonce generation.
    nonce_key: [u8; KEY_SIZE],
    /// Plaintext block size.
    block_size: usize,
    /// Cached header (updated on writes).
    header: RwLock<Header>,
    /// Buffer for the current (partial) block being written.
    /// Data is only encrypted when a full block is ready or on flush.
    write_buffer: RwLock<Vec<u8>>,
}

impl EncryptedBackend {
    /// Creates a new encrypted backend wrapping the given inner backend.
    ///
    /// If the inner backend is empty, initializes a new encrypted storage.
    /// If it contains data, reads and validates the header.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The inner backend contains invalid encrypted data
    /// - The encryption key is wrong (authentication will fail on first read)
    /// - The format version is unsupported
    pub fn new(inner: Box<dyn StorageBackend>, key: EncryptionKey) -> StorageResult<Self> {
        Self::with_block_size(inner, key, DEFAULT_BLOCK_SIZE)
    }

    /// Creates a new encrypted backend with a custom block size.
    ///
    /// # Arguments
    ///
    /// * `inner` - The underlying storage backend
    /// * `key` - The encryption key
    /// * `block_size` - Plaintext block size (must be 1KB to 1MB)
    ///
    /// # Errors
    ///
    /// Returns an error if the block size is invalid or the storage is corrupted.
    pub fn with_block_size(
        inner: Box<dyn StorageBackend>,
        key: EncryptionKey,
        block_size: usize,
    ) -> StorageResult<Self> {
        if block_size < 1024 || block_size > 1024 * 1024 {
            return Err(StorageError::Encryption(format!(
                "block size must be between 1KB and 1MB, got {block_size}"
            )));
        }

        let cipher = Aes256Gcm::new_from_slice(key.as_bytes())
            .map_err(|e| StorageError::Encryption(format!("failed to create cipher: {e}")))?;

        let nonce_key = derive_nonce_key(key.as_bytes());

        let physical_size = inner.size()?;

        let (inner, header, cipher) = if physical_size == 0 {
            // New storage - initialize header
            let header = Header::new(block_size as u32);
            let mut inner = inner;
            inner.append(&header.encode())?;
            inner.flush()?;
            (inner, header, cipher)
        } else if physical_size < HEADER_SIZE as u64 {
            return Err(StorageError::Encryption(
                "storage too small to contain header".to_string(),
            ));
        } else {
            // Existing storage - read and validate header
            let header_bytes = inner.read_at(0, HEADER_SIZE)?;
            let mut header = Header::decode(&header_bytes)?;

            // Validate block size matches
            if header.block_size as usize != block_size {
                return Err(StorageError::Encryption(format!(
                    "block size mismatch: storage has {}, requested {block_size}",
                    header.block_size
                )));
            }

            // Compute actual logical size from physical size and block contents
            // Physical layout: [Header][Block 0][Block 1]...
            let data_size = physical_size - HEADER_SIZE as u64;
            let enc_block_size = encrypted_block_size(block_size) as u64;
            
            if data_size > 0 {
                // Number of complete encrypted blocks
                let num_blocks = data_size / enc_block_size;
                let remainder = data_size % enc_block_size;
                
                if remainder != 0 {
                    return Err(StorageError::Encryption(
                        "storage contains partial encrypted block - possible corruption".to_string(),
                    ));
                }
                
                if num_blocks > 0 {
                    // Sum up the actual lengths from all blocks
                    // Full blocks contribute block_size bytes each
                    // The last block may be partial - read its embedded length
                    let mut total_logical_size: u64 = 0;
                    
                    for block_num in 0..num_blocks {
                        let physical_offset = HEADER_SIZE as u64 + block_num * enc_block_size;
                        let encrypted = inner.read_at(physical_offset, enc_block_size as usize)?;
                        
                        // Decrypt to get actual length
                        let nonce_bytes = &encrypted[..NONCE_SIZE];
                        let ciphertext = &encrypted[NONCE_SIZE..];
                        
                        let expected_nonce = derive_nonce(&nonce_key, block_num);
                        if nonce_bytes != expected_nonce {
                            return Err(StorageError::Encryption(format!(
                                "nonce mismatch for block {block_num} during recovery"
                            )));
                        }
                        
                        let nonce = Nonce::from_slice(nonce_bytes);
                        let block_data = cipher
                            .decrypt(nonce, ciphertext)
                            .map_err(|_| StorageError::Encryption(
                                "decryption failed during recovery - wrong key?".to_string()
                            ))?;
                        
                        if block_data.len() < BLOCK_LEN_SIZE {
                            return Err(StorageError::Encryption(
                                "block too short during recovery".to_string()
                            ));
                        }
                        
                        let block_len = u32::from_le_bytes(
                            block_data[..BLOCK_LEN_SIZE].try_into().unwrap()
                        ) as u64;
                        
                        total_logical_size += block_len;
                    }
                    
                    header.logical_size = total_logical_size;
                }
            }

            (inner, header, cipher)
        };

        Ok(Self {
            inner: RwLock::new(inner),
            cipher,
            nonce_key,
            block_size,
            header: RwLock::new(header),
            write_buffer: RwLock::new(Vec::new()),
        })
    }

    /// Returns the current logical (plaintext) size.
    fn logical_size(&self) -> u64 {
        let header = self.header.read();
        let buffer = self.write_buffer.read();
        header.logical_size + buffer.len() as u64
    }

    /// Calculates the physical offset where a block starts.
    fn block_physical_offset(&self, block_number: u64) -> u64 {
        HEADER_SIZE as u64 + block_number * encrypted_block_size(self.block_size) as u64
    }

    /// Calculates which block contains a given logical offset.
    fn logical_to_block(&self, logical_offset: u64) -> (u64, usize) {
        let block_number = logical_offset / self.block_size as u64;
        let offset_in_block = (logical_offset % self.block_size as u64) as usize;
        (block_number, offset_in_block)
    }

    /// Encrypts a single block.
    /// 
    /// Block format: [length (4 bytes, little-endian)][padded plaintext]
    /// The length stores the actual number of valid bytes in this block.
    fn encrypt_block(&self, block_number: u64, plaintext: &[u8]) -> StorageResult<Vec<u8>> {
        if plaintext.len() > self.block_size {
            return Err(StorageError::Encryption(format!(
                "block too large: {} > {}",
                plaintext.len(),
                self.block_size
            )));
        }

        // Build block with length prefix and padded plaintext
        let actual_len = plaintext.len() as u32;
        let mut block_data = Vec::with_capacity(BLOCK_LEN_SIZE + self.block_size);
        block_data.extend_from_slice(&actual_len.to_le_bytes());
        block_data.extend_from_slice(plaintext);
        // Pad to full block size
        block_data.resize(BLOCK_LEN_SIZE + self.block_size, 0);

        let nonce_bytes = derive_nonce(&self.nonce_key, block_number);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = self
            .cipher
            .encrypt(nonce, block_data.as_slice())
            .map_err(|e| StorageError::Encryption(format!("encryption failed: {e}")))?;

        // Build encrypted block: nonce || ciphertext (includes tag)
        let mut encrypted_block = Vec::with_capacity(NONCE_SIZE + ciphertext.len());
        encrypted_block.extend_from_slice(&nonce_bytes);
        encrypted_block.extend_from_slice(&ciphertext);

        Ok(encrypted_block)
    }

    /// Decrypts a single block and returns (plaintext, actual_length).
    fn decrypt_block(&self, block_number: u64, encrypted: &[u8]) -> StorageResult<(Vec<u8>, usize)> {
        if encrypted.len() < NONCE_SIZE + TAG_SIZE + BLOCK_LEN_SIZE {
            return Err(StorageError::Encryption(
                "encrypted block too short".to_string(),
            ));
        }

        let nonce_bytes = &encrypted[..NONCE_SIZE];
        let ciphertext = &encrypted[NONCE_SIZE..];

        // Verify nonce matches expected (detect block reordering/tampering)
        let expected_nonce = derive_nonce(&self.nonce_key, block_number);
        if nonce_bytes != expected_nonce {
            return Err(StorageError::Encryption(format!(
                "nonce mismatch for block {block_number} - possible data corruption or wrong key"
            )));
        }

        let nonce = Nonce::from_slice(nonce_bytes);

        let block_data = self
            .cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| {
                StorageError::Encryption(
                    "decryption failed - wrong key or data corrupted".to_string(),
                )
            })?;

        if block_data.len() < BLOCK_LEN_SIZE {
            return Err(StorageError::Encryption(
                "decrypted block too short".to_string(),
            ));
        }

        // Extract length prefix
        let actual_len = u32::from_le_bytes(block_data[..BLOCK_LEN_SIZE].try_into().unwrap()) as usize;
        
        if actual_len > self.block_size {
            return Err(StorageError::Encryption(format!(
                "invalid block length: {actual_len} > {}",
                self.block_size
            )));
        }

        // Extract actual plaintext (without length prefix)
        let plaintext = block_data[BLOCK_LEN_SIZE..BLOCK_LEN_SIZE + actual_len].to_vec();

        Ok((plaintext, actual_len))
    }

    /// Reads a single block by block number, returns (plaintext, actual_length).
    fn read_block(&self, block_number: u64) -> StorageResult<(Vec<u8>, usize)> {
        let physical_offset = self.block_physical_offset(block_number);
        let encrypted_size = encrypted_block_size(self.block_size);

        let inner = self.inner.read();
        let encrypted = inner.read_at(physical_offset, encrypted_size)?;
        drop(inner);

        self.decrypt_block(block_number, &encrypted)
    }

    /// Flushes the write buffer, encrypting any pending data.
    fn flush_write_buffer(&self) -> StorageResult<()> {
        let mut buffer = self.write_buffer.write();
        if buffer.is_empty() {
            return Ok(());
        }

        let mut header = self.header.write();
        let block_number = header.logical_size / self.block_size as u64;

        // Encrypt the buffer (may be partial block)
        let encrypted = self.encrypt_block(block_number, &buffer)?;

        // Write encrypted block
        let physical_offset = self.block_physical_offset(block_number);
        {
            let mut inner = self.inner.write();
            // For append-only semantics, verify we're writing at the expected position
            let current_size = inner.size()?;
            if physical_offset != current_size {
                // This means we need to handle partial block updates
                // For now, this should not happen with append-only semantics
                return Err(StorageError::Encryption(
                    "unexpected write position - storage may be corrupted".to_string(),
                ));
            }
            inner.append(&encrypted)?;
        }

        // Update header
        header.logical_size += buffer.len() as u64;
        buffer.clear();

        Ok(())
    }
}

impl StorageBackend for EncryptedBackend {
    fn read_at(&self, offset: u64, len: usize) -> StorageResult<Vec<u8>> {
        if len == 0 {
            return Ok(Vec::new());
        }

        let logical_size = self.logical_size();
        if offset >= logical_size {
            return Err(StorageError::ReadPastEnd {
                offset,
                len,
                size: logical_size,
            });
        }

        // Clamp read to available data
        let available = (logical_size - offset) as usize;
        let actual_len = len.min(available);

        let (start_block, start_offset) = self.logical_to_block(offset);
        let (end_block, _) = self.logical_to_block(offset + actual_len as u64 - 1);

        let mut result = Vec::with_capacity(actual_len);
        let header = self.header.read();
        let committed_logical_size = header.logical_size;
        // Number of blocks with committed data (ceiling division)
        let committed_blocks = if committed_logical_size == 0 {
            0
        } else {
            (committed_logical_size + self.block_size as u64 - 1) / self.block_size as u64
        };
        drop(header);

        for block_num in start_block..=end_block {
            let block_logical_start = block_num * self.block_size as u64;
            
            // Get the plaintext and its actual length for this block
            let (plaintext, block_actual_len) = if block_num < committed_blocks {
                // Read from encrypted storage (actual length embedded in block)
                self.read_block(block_num)?
            } else {
                // This block is partially or fully in the write buffer
                let buffer = self.write_buffer.read();
                
                // Calculate where in the buffer this block's data starts
                let buffer_start = if block_logical_start >= committed_logical_size {
                    (block_logical_start - committed_logical_size) as usize
                } else {
                    0
                };

                if buffer_start < buffer.len() {
                    let copy_len = (buffer.len() - buffer_start).min(self.block_size);
                    let plaintext = buffer[buffer_start..buffer_start + copy_len].to_vec();
                    (plaintext, copy_len)
                } else {
                    (Vec::new(), 0)
                }
            };

            // Calculate what portion of this block we need
            let read_start = if block_num == start_block {
                start_offset
            } else {
                0
            };

            let read_end = if block_num == end_block {
                let end_in_block = ((offset + actual_len as u64) - block_logical_start) as usize;
                end_in_block.min(block_actual_len)
            } else {
                block_actual_len
            };

            if read_start < read_end && read_end <= plaintext.len() {
                result.extend_from_slice(&plaintext[read_start..read_end]);
            }
        }

        Ok(result)
    }

    fn append(&mut self, data: &[u8]) -> StorageResult<u64> {
        if data.is_empty() {
            return Ok(self.logical_size());
        }

        let offset = self.logical_size();

        let mut buffer = self.write_buffer.write();
        let mut data_offset = 0;

        while data_offset < data.len() {
            let space_in_buffer = self.block_size - buffer.len();
            let to_copy = (data.len() - data_offset).min(space_in_buffer);

            buffer.extend_from_slice(&data[data_offset..data_offset + to_copy]);
            data_offset += to_copy;

            // If buffer is full, flush it
            if buffer.len() >= self.block_size {
                drop(buffer);
                self.flush_write_buffer()?;
                buffer = self.write_buffer.write();
            }
        }

        Ok(offset)
    }

    fn flush(&mut self) -> StorageResult<()> {
        // Flush any pending data in write buffer
        self.flush_write_buffer()?;

        // Flush underlying storage
        let mut inner = self.inner.write();
        inner.flush()
    }

    fn size(&self) -> StorageResult<u64> {
        Ok(self.logical_size())
    }

    fn sync(&mut self) -> StorageResult<()> {
        self.flush()?;
        let mut inner = self.inner.write();
        inner.sync()
    }

    fn truncate(&mut self, new_size: u64) -> StorageResult<()> {
        if new_size == 0 {
            // Clear everything and reinitialize
            let mut inner = self.inner.write();
            inner.truncate(0)?;

            // Reinitialize with header
            let header = Header::new(self.block_size as u32);
            inner.append(&header.encode())?;
            inner.flush()?;

            *self.header.write() = header;
            self.write_buffer.write().clear();

            Ok(())
        } else {
            // Partial truncation of encrypted data is complex because:
            // 1. We need to preserve complete blocks
            // 2. May need to rewrite a partial last block
            //
            // For now, only support truncate to block boundaries
            let (block_num, offset_in_block) = self.logical_to_block(new_size);

            if offset_in_block != 0 {
                return Err(StorageError::Encryption(
                    "encrypted backend only supports truncation to block boundaries".to_string(),
                ));
            }

            let physical_size =
                HEADER_SIZE as u64 + block_num * encrypted_block_size(self.block_size) as u64;

            let mut inner = self.inner.write();
            inner.truncate(physical_size)?;

            let mut header = self.header.write();
            header.logical_size = new_size;
            self.write_buffer.write().clear();

            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InMemoryBackend;

    fn test_key() -> EncryptionKey {
        EncryptionKey::from_bytes(&[0x42u8; KEY_SIZE]).unwrap()
    }

    fn test_key_different() -> EncryptionKey {
        EncryptionKey::from_bytes(&[0x43u8; KEY_SIZE]).unwrap()
    }

    #[test]
    fn new_encrypted_backend_initializes_header() {
        let inner = InMemoryBackend::new();
        let backend = EncryptedBackend::new(Box::new(inner), test_key()).unwrap();
        assert_eq!(backend.size().unwrap(), 0);
    }

    #[test]
    fn append_and_read_small_data() {
        let inner = InMemoryBackend::new();
        let mut backend = EncryptedBackend::new(Box::new(inner), test_key()).unwrap();

        let data = b"Hello, encrypted world!";
        let offset = backend.append(data).unwrap();
        assert_eq!(offset, 0);

        // Flush to ensure data is encrypted
        backend.flush().unwrap();

        let read_back = backend.read_at(0, data.len()).unwrap();
        assert_eq!(&read_back, data);
    }

    #[test]
    fn append_and_read_multiple_blocks() {
        let inner = InMemoryBackend::new();
        let mut backend =
            EncryptedBackend::with_block_size(Box::new(inner), test_key(), 1024).unwrap();

        // Write 3.5 blocks worth of data
        let data = vec![0xABu8; 3584];
        let offset = backend.append(&data).unwrap();
        assert_eq!(offset, 0);

        backend.flush().unwrap();

        let read_back = backend.read_at(0, data.len()).unwrap();
        assert_eq!(read_back, data);
    }

    #[test]
    fn read_partial_block() {
        let inner = InMemoryBackend::new();
        let mut backend =
            EncryptedBackend::with_block_size(Box::new(inner), test_key(), 1024).unwrap();

        let data = b"ABCDEFGHIJ";
        backend.append(data).unwrap();
        backend.flush().unwrap();

        // Read middle portion
        let partial = backend.read_at(3, 4).unwrap();
        assert_eq!(&partial, b"DEFG");
    }

    #[test]
    fn read_across_block_boundary() {
        let inner = InMemoryBackend::new();
        let mut backend =
            EncryptedBackend::with_block_size(Box::new(inner), test_key(), 1024).unwrap();

        // Write 2 blocks
        let data = vec![0x11u8; 2048];
        backend.append(&data).unwrap();
        backend.flush().unwrap();

        // Read across block boundary
        let read_back = backend.read_at(512, 1024).unwrap();
        assert_eq!(read_back, vec![0x11u8; 1024]);
    }

    #[test]
    fn determinism_same_data_same_key() {
        // This test verifies AC-01: deterministic encryption
        let data = b"Test data for determinism check";

        let inner1 = InMemoryBackend::new();
        let mut backend1 = EncryptedBackend::new(Box::new(inner1), test_key()).unwrap();
        backend1.append(data).unwrap();
        backend1.flush().unwrap();

        let inner2 = InMemoryBackend::new();
        let mut backend2 = EncryptedBackend::new(Box::new(inner2), test_key()).unwrap();
        backend2.append(data).unwrap();
        backend2.flush().unwrap();

        // Read raw encrypted bytes from both backends
        let encrypted1 = backend1.inner.read().read_at(0, 200).unwrap();
        let encrypted2 = backend2.inner.read().read_at(0, 200).unwrap();

        assert_eq!(encrypted1, encrypted2, "Encryption must be deterministic");
    }

    #[test]
    fn different_keys_produce_different_ciphertext() {
        let data = b"Test data";

        let inner1 = InMemoryBackend::new();
        let mut backend1 = EncryptedBackend::new(Box::new(inner1), test_key()).unwrap();
        backend1.append(data).unwrap();
        backend1.flush().unwrap();

        let inner2 = InMemoryBackend::new();
        let mut backend2 = EncryptedBackend::new(Box::new(inner2), test_key_different()).unwrap();
        backend2.append(data).unwrap();
        backend2.flush().unwrap();

        let encrypted1 = backend1
            .inner
            .read()
            .read_at(HEADER_SIZE as u64, 100)
            .unwrap();
        let encrypted2 = backend2
            .inner
            .read()
            .read_at(HEADER_SIZE as u64, 100)
            .unwrap();

        assert_ne!(
            encrypted1, encrypted2,
            "Different keys must produce different ciphertext"
        );
    }

    #[test]
    fn wrong_key_fails_on_open() {
        let data = b"Secret data";

        // Encrypt with key1
        let inner = InMemoryBackend::new();
        let mut backend = EncryptedBackend::new(Box::new(inner), test_key()).unwrap();
        backend.append(data).unwrap();
        backend.flush().unwrap();

        // Get the raw encrypted storage
        let inner_data = {
            let inner = backend.inner.read();
            inner.read_at(0, inner.size().unwrap() as usize).unwrap()
        };

        // Try to open with different key - should fail during recovery
        let mut new_inner = InMemoryBackend::new();
        new_inner.append(&inner_data).unwrap();

        let result = EncryptedBackend::new(Box::new(new_inner), test_key_different());
        
        // Opening with wrong key should fail (nonce mismatch since key-derived nonces differ)
        assert!(result.is_err(), "Opening with wrong key must fail");
    }

    #[test]
    fn tampered_data_fails_on_open() {
        let data = b"Important data";

        let inner = InMemoryBackend::new();
        let mut backend = EncryptedBackend::new(Box::new(inner), test_key()).unwrap();
        backend.append(data).unwrap();
        backend.flush().unwrap();

        // Get raw storage and tamper with it
        let mut raw_data = {
            let inner = backend.inner.read();
            inner.read_at(0, inner.size().unwrap() as usize).unwrap()
        };

        // Tamper with the ciphertext (after header and nonce)
        let tamper_offset = HEADER_SIZE + NONCE_SIZE + 5;
        raw_data[tamper_offset] ^= 0xFF;

        // Create new backend with tampered data
        let mut tampered_inner = InMemoryBackend::new();
        tampered_inner.append(&raw_data).unwrap();

        // Opening with tampered data should fail during recovery
        let result = EncryptedBackend::new(Box::new(tampered_inner), test_key());
        assert!(result.is_err(), "Tampered data must fail on open");
    }

    #[test]
    fn truncate_to_zero_works() {
        let inner = InMemoryBackend::new();
        let mut backend = EncryptedBackend::new(Box::new(inner), test_key()).unwrap();

        backend.append(b"some data").unwrap();
        backend.flush().unwrap();

        backend.truncate(0).unwrap();
        assert_eq!(backend.size().unwrap(), 0);

        // Can write again after truncate
        backend.append(b"new data").unwrap();
        backend.flush().unwrap();

        let read_back = backend.read_at(0, 8).unwrap();
        assert_eq!(&read_back, b"new data");
    }

    #[test]
    fn reopen_encrypted_storage() {
        // This test verifies that encrypted storage can be reopened
        let key = test_key();
        let data = b"Persistent encrypted data";

        // Create and write
        let raw_data = {
            let inner = InMemoryBackend::new();
            let mut backend = EncryptedBackend::new(Box::new(inner), key.clone()).unwrap();
            backend.append(data).unwrap();
            backend.flush().unwrap();

            let inner = backend.inner.read();
            inner.read_at(0, inner.size().unwrap() as usize).unwrap()
        };

        // Reopen
        let mut reopened_inner = InMemoryBackend::new();
        reopened_inner.append(&raw_data).unwrap();

        let reopened = EncryptedBackend::new(Box::new(reopened_inner), key).unwrap();
        let read_back = reopened.read_at(0, data.len()).unwrap();
        assert_eq!(&read_back, data);
    }

    #[test]
    fn key_zeroization() {
        let key = EncryptionKey::from_bytes(&[0xFFu8; KEY_SIZE]).unwrap();

        // Drop the key - zeroize derive handles this
        drop(key);

        // The Zeroize derive macro handles zeroization automatically.
        // We can't verify without unsafe code, but this test ensures compilation.
    }

    #[test]
    fn empty_append() {
        let inner = InMemoryBackend::new();
        let mut backend = EncryptedBackend::new(Box::new(inner), test_key()).unwrap();

        let offset = backend.append(&[]).unwrap();
        assert_eq!(offset, 0);
        assert_eq!(backend.size().unwrap(), 0);
    }

    #[test]
    fn read_past_end_fails() {
        let inner = InMemoryBackend::new();
        let mut backend = EncryptedBackend::new(Box::new(inner), test_key()).unwrap();

        backend.append(b"hello").unwrap();
        backend.flush().unwrap();

        let result = backend.read_at(10, 5);
        assert!(result.is_err());
    }

    #[test]
    fn block_size_validation() {
        let inner = InMemoryBackend::new();

        // Too small
        let result = EncryptedBackend::with_block_size(Box::new(inner), test_key(), 512);
        assert!(result.is_err());

        let inner = InMemoryBackend::new();
        // Too large
        let result =
            EncryptedBackend::with_block_size(Box::new(inner), test_key(), 2 * 1024 * 1024);
        assert!(result.is_err());
    }

    #[test]
    fn nonce_derivation_is_deterministic() {
        let nonce_key = derive_nonce_key(&[0x42u8; KEY_SIZE]);

        let nonce1 = derive_nonce(&nonce_key, 0);
        let nonce2 = derive_nonce(&nonce_key, 0);
        assert_eq!(nonce1, nonce2);

        let nonce3 = derive_nonce(&nonce_key, 1);
        assert_ne!(nonce1, nonce3, "Different blocks must have different nonces");
    }

    #[test]
    fn nonce_key_derivation_is_deterministic() {
        let key = [0x42u8; KEY_SIZE];
        let nonce_key1 = derive_nonce_key(&key);
        let nonce_key2 = derive_nonce_key(&key);
        assert_eq!(nonce_key1, nonce_key2);

        let different_key = [0x43u8; KEY_SIZE];
        let nonce_key3 = derive_nonce_key(&different_key);
        assert_ne!(nonce_key1, nonce_key3);
    }

    #[test]
    fn large_data_roundtrip() {
        let inner = InMemoryBackend::new();
        let mut backend =
            EncryptedBackend::with_block_size(Box::new(inner), test_key(), 1024).unwrap();

        // Write 100KB of data
        let data: Vec<u8> = (0..102400).map(|i| (i % 256) as u8).collect();
        backend.append(&data).unwrap();
        backend.flush().unwrap();

        let read_back = backend.read_at(0, data.len()).unwrap();
        assert_eq!(read_back, data);

        // Read various portions
        let portion = backend.read_at(50000, 1000).unwrap();
        assert_eq!(portion, &data[50000..51000]);
    }
}
