//! Encryption FFI functions.
//!
//! This module provides FFI functions for encryption and decryption operations.
//! The encryption feature must be enabled for these functions to be available.

use crate::buffer::EntiDbBuffer;
use crate::error::{clear_last_error, set_last_error, EntiDbResult};
use std::ptr;

/// Opaque handle to a CryptoManager.
pub struct EntiDbCryptoHandle {
    #[cfg(feature = "encryption")]
    manager: entidb_core::crypto::CryptoManager,
}

/// Creates a new CryptoManager with a generated random key.
///
/// The key is stored internally and can be exported with `entidb_crypto_export_key`.
///
/// # Arguments
///
/// * `out_handle` - Output pointer for the crypto handle
/// * `out_key` - Output pointer for the 32-byte key (caller should provide a 32-byte buffer)
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `out_handle` must be a valid pointer
/// - `out_key` must point to a buffer of at least 32 bytes
#[no_mangle]
#[cfg(feature = "encryption")]
pub unsafe extern "C" fn entidb_crypto_create(
    out_handle: *mut *mut EntiDbCryptoHandle,
    out_key: *mut u8,
) -> EntiDbResult {
    clear_last_error();

    if out_handle.is_null() || out_key.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    use entidb_core::crypto::EncryptionKey;

    let key = EncryptionKey::generate();
    let key_bytes = key.as_bytes();

    // Copy key to output buffer
    ptr::copy_nonoverlapping(key_bytes.as_ptr(), out_key, 32);

    let manager = entidb_core::crypto::CryptoManager::new(key);
    let handle = Box::new(EntiDbCryptoHandle { manager });

    *out_handle = Box::into_raw(handle);

    EntiDbResult::Ok
}

/// Creates a CryptoManager from an existing key.
///
/// # Arguments
///
/// * `key_ptr` - Pointer to the 32-byte encryption key
/// * `out_handle` - Output pointer for the crypto handle
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `key_ptr` must point to a buffer of exactly 32 bytes
/// - `out_handle` must be a valid pointer
#[no_mangle]
#[cfg(feature = "encryption")]
pub unsafe extern "C" fn entidb_crypto_create_with_key(
    key_ptr: *const u8,
    out_handle: *mut *mut EntiDbCryptoHandle,
) -> EntiDbResult {
    clear_last_error();

    if key_ptr.is_null() || out_handle.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    use entidb_core::crypto::EncryptionKey;

    // Read key bytes
    let key_slice = std::slice::from_raw_parts(key_ptr, 32);
    let key = match EncryptionKey::from_bytes(key_slice) {
        Ok(k) => k,
        Err(e) => {
            set_last_error(format!("failed to create key: {e}"));
            return EntiDbResult::InvalidArgument;
        }
    };

    let manager = entidb_core::crypto::CryptoManager::new(key);
    let handle = Box::new(EntiDbCryptoHandle { manager });

    *out_handle = Box::into_raw(handle);

    EntiDbResult::Ok
}

/// Derives an encryption key from a password and salt.
///
/// # Arguments
///
/// * `password_ptr` - Pointer to the password bytes
/// * `password_len` - Length of the password
/// * `salt_ptr` - Pointer to the salt bytes (should be random and stored)
/// * `salt_len` - Length of the salt (recommended: 16+ bytes)
/// * `out_handle` - Output pointer for the crypto handle
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// All pointer arguments must be valid.
#[no_mangle]
#[cfg(feature = "encryption")]
pub unsafe extern "C" fn entidb_crypto_create_from_password(
    password_ptr: *const u8,
    password_len: usize,
    salt_ptr: *const u8,
    salt_len: usize,
    out_handle: *mut *mut EntiDbCryptoHandle,
) -> EntiDbResult {
    clear_last_error();

    if password_ptr.is_null() || salt_ptr.is_null() || out_handle.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    use entidb_core::crypto::EncryptionKey;

    let password = std::slice::from_raw_parts(password_ptr, password_len);
    let salt = std::slice::from_raw_parts(salt_ptr, salt_len);

    let key = match EncryptionKey::derive_from_password(password, salt) {
        Ok(k) => k,
        Err(e) => {
            set_last_error(format!("failed to derive key: {e}"));
            return EntiDbResult::Error;
        }
    };

    let manager = entidb_core::crypto::CryptoManager::new(key);
    let handle = Box::new(EntiDbCryptoHandle { manager });

    *out_handle = Box::into_raw(handle);

    EntiDbResult::Ok
}

/// Encrypts data using AES-256-GCM.
///
/// The output is: nonce (12 bytes) || ciphertext || tag (16 bytes)
///
/// # Arguments
///
/// * `handle` - The crypto handle
/// * `data_ptr` - Pointer to the data to encrypt
/// * `data_len` - Length of the data
/// * `out_buffer` - Output buffer for the encrypted data
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// All pointer arguments must be valid.
#[no_mangle]
#[cfg(feature = "encryption")]
pub unsafe extern "C" fn entidb_crypto_encrypt(
    handle: *const EntiDbCryptoHandle,
    data_ptr: *const u8,
    data_len: usize,
    out_buffer: *mut EntiDbBuffer,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || data_ptr.is_null() || out_buffer.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let handle = &*handle;
    let data = std::slice::from_raw_parts(data_ptr, data_len);

    match handle.manager.encrypt(data) {
        Ok(encrypted) => {
            *out_buffer = EntiDbBuffer::from_vec(encrypted);
            EntiDbResult::Ok
        }
        Err(e) => {
            set_last_error(format!("encryption failed: {e}"));
            EntiDbResult::Error
        }
    }
}

/// Decrypts data that was encrypted with entidb_crypto_encrypt.
///
/// # Arguments
///
/// * `handle` - The crypto handle (must have the same key used for encryption)
/// * `data_ptr` - Pointer to the encrypted data
/// * `data_len` - Length of the encrypted data
/// * `out_buffer` - Output buffer for the decrypted data
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// All pointer arguments must be valid.
#[no_mangle]
#[cfg(feature = "encryption")]
pub unsafe extern "C" fn entidb_crypto_decrypt(
    handle: *const EntiDbCryptoHandle,
    data_ptr: *const u8,
    data_len: usize,
    out_buffer: *mut EntiDbBuffer,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || data_ptr.is_null() || out_buffer.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let handle = &*handle;
    let data = std::slice::from_raw_parts(data_ptr, data_len);

    match handle.manager.decrypt(data) {
        Ok(decrypted) => {
            *out_buffer = EntiDbBuffer::from_vec(decrypted);
            EntiDbResult::Ok
        }
        Err(e) => {
            set_last_error(format!("decryption failed: {e}"));
            EntiDbResult::Error
        }
    }
}

/// Encrypts data with associated authenticated data (AAD).
///
/// The AAD is authenticated but not encrypted, useful for binding
/// ciphertext to metadata like entity IDs or collection names.
///
/// # Arguments
///
/// * `handle` - The crypto handle
/// * `data_ptr` - Pointer to the data to encrypt
/// * `data_len` - Length of the data
/// * `aad_ptr` - Pointer to the associated data
/// * `aad_len` - Length of the associated data
/// * `out_buffer` - Output buffer for the encrypted data
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// All pointer arguments must be valid.
#[no_mangle]
#[cfg(feature = "encryption")]
pub unsafe extern "C" fn entidb_crypto_encrypt_with_aad(
    handle: *const EntiDbCryptoHandle,
    data_ptr: *const u8,
    data_len: usize,
    aad_ptr: *const u8,
    aad_len: usize,
    out_buffer: *mut EntiDbBuffer,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || data_ptr.is_null() || aad_ptr.is_null() || out_buffer.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let handle = &*handle;
    let data = std::slice::from_raw_parts(data_ptr, data_len);
    let aad = std::slice::from_raw_parts(aad_ptr, aad_len);

    match handle.manager.encrypt_with_aad(data, aad) {
        Ok(encrypted) => {
            *out_buffer = EntiDbBuffer::from_vec(encrypted);
            EntiDbResult::Ok
        }
        Err(e) => {
            set_last_error(format!("encryption failed: {e}"));
            EntiDbResult::Error
        }
    }
}

/// Decrypts data with associated authenticated data (AAD).
///
/// The same AAD must be provided as was used during encryption.
///
/// # Arguments
///
/// * `handle` - The crypto handle
/// * `data_ptr` - Pointer to the encrypted data
/// * `data_len` - Length of the encrypted data
/// * `aad_ptr` - Pointer to the associated data (must match encryption)
/// * `aad_len` - Length of the associated data
/// * `out_buffer` - Output buffer for the decrypted data
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// All pointer arguments must be valid.
#[no_mangle]
#[cfg(feature = "encryption")]
pub unsafe extern "C" fn entidb_crypto_decrypt_with_aad(
    handle: *const EntiDbCryptoHandle,
    data_ptr: *const u8,
    data_len: usize,
    aad_ptr: *const u8,
    aad_len: usize,
    out_buffer: *mut EntiDbBuffer,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || data_ptr.is_null() || aad_ptr.is_null() || out_buffer.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let handle = &*handle;
    let data = std::slice::from_raw_parts(data_ptr, data_len);
    let aad = std::slice::from_raw_parts(aad_ptr, aad_len);

    match handle.manager.decrypt_with_aad(data, aad) {
        Ok(decrypted) => {
            *out_buffer = EntiDbBuffer::from_vec(decrypted);
            EntiDbResult::Ok
        }
        Err(e) => {
            set_last_error(format!("decryption failed: {e}"));
            EntiDbResult::Error
        }
    }
}

/// Frees a crypto handle.
///
/// # Arguments
///
/// * `handle` - The crypto handle to free
///
/// # Safety
///
/// The handle must be valid and must not be used after this call.
#[no_mangle]
#[cfg(feature = "encryption")]
pub unsafe extern "C" fn entidb_crypto_free(handle: *mut EntiDbCryptoHandle) {
    if !handle.is_null() {
        drop(Box::from_raw(handle));
    }
}

/// Returns whether encryption is available.
///
/// This is always true when the encryption feature is enabled.
#[no_mangle]
pub extern "C" fn entidb_crypto_available() -> bool {
    cfg!(feature = "encryption")
}

// Stubs when encryption feature is disabled
#[cfg(not(feature = "encryption"))]
mod stubs {
    use super::*;

    #[no_mangle]
    pub unsafe extern "C" fn entidb_crypto_create(
        _out_handle: *mut *mut EntiDbCryptoHandle,
        _out_key: *mut u8,
    ) -> EntiDbResult {
        set_last_error("encryption feature not enabled");
        EntiDbResult::NotSupported
    }

    #[no_mangle]
    pub unsafe extern "C" fn entidb_crypto_create_with_key(
        _key_ptr: *const u8,
        _out_handle: *mut *mut EntiDbCryptoHandle,
    ) -> EntiDbResult {
        set_last_error("encryption feature not enabled");
        EntiDbResult::NotSupported
    }

    #[no_mangle]
    pub unsafe extern "C" fn entidb_crypto_create_from_password(
        _password_ptr: *const u8,
        _password_len: usize,
        _salt_ptr: *const u8,
        _salt_len: usize,
        _out_handle: *mut *mut EntiDbCryptoHandle,
    ) -> EntiDbResult {
        set_last_error("encryption feature not enabled");
        EntiDbResult::NotSupported
    }

    #[no_mangle]
    pub unsafe extern "C" fn entidb_crypto_encrypt(
        _handle: *const EntiDbCryptoHandle,
        _data_ptr: *const u8,
        _data_len: usize,
        _out_buffer: *mut EntiDbBuffer,
    ) -> EntiDbResult {
        set_last_error("encryption feature not enabled");
        EntiDbResult::NotSupported
    }

    #[no_mangle]
    pub unsafe extern "C" fn entidb_crypto_decrypt(
        _handle: *const EntiDbCryptoHandle,
        _data_ptr: *const u8,
        _data_len: usize,
        _out_buffer: *mut EntiDbBuffer,
    ) -> EntiDbResult {
        set_last_error("encryption feature not enabled");
        EntiDbResult::NotSupported
    }

    #[no_mangle]
    pub unsafe extern "C" fn entidb_crypto_encrypt_with_aad(
        _handle: *const EntiDbCryptoHandle,
        _data_ptr: *const u8,
        _data_len: usize,
        _aad_ptr: *const u8,
        _aad_len: usize,
        _out_buffer: *mut EntiDbBuffer,
    ) -> EntiDbResult {
        set_last_error("encryption feature not enabled");
        EntiDbResult::NotSupported
    }

    #[no_mangle]
    pub unsafe extern "C" fn entidb_crypto_decrypt_with_aad(
        _handle: *const EntiDbCryptoHandle,
        _data_ptr: *const u8,
        _data_len: usize,
        _aad_ptr: *const u8,
        _aad_len: usize,
        _out_buffer: *mut EntiDbBuffer,
    ) -> EntiDbResult {
        set_last_error("encryption feature not enabled");
        EntiDbResult::NotSupported
    }

    #[no_mangle]
    pub unsafe extern "C" fn entidb_crypto_free(_handle: *mut EntiDbCryptoHandle) {
        // Nothing to free
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(feature = "encryption")]
    fn test_crypto_roundtrip() {
        unsafe {
            let mut handle: *mut EntiDbCryptoHandle = ptr::null_mut();
            let mut key = [0u8; 32];

            let result = entidb_crypto_create(&mut handle, key.as_mut_ptr());
            assert_eq!(result, EntiDbResult::Ok);
            assert!(!handle.is_null());

            let data = b"Hello, encrypted world!";
            let mut encrypted = EntiDbBuffer::empty();

            let result = entidb_crypto_encrypt(handle, data.as_ptr(), data.len(), &mut encrypted);
            assert_eq!(result, EntiDbResult::Ok);

            let enc_slice = std::slice::from_raw_parts(encrypted.data, encrypted.len);

            let mut decrypted = EntiDbBuffer::empty();
            let result =
                entidb_crypto_decrypt(handle, enc_slice.as_ptr(), enc_slice.len(), &mut decrypted);
            assert_eq!(result, EntiDbResult::Ok);

            let dec_slice = std::slice::from_raw_parts(decrypted.data, decrypted.len);
            assert_eq!(dec_slice, data);

            crate::buffer::entidb_free_buffer(encrypted);
            crate::buffer::entidb_free_buffer(decrypted);
            entidb_crypto_free(handle);
        }
    }

    #[test]
    #[cfg(feature = "encryption")]
    fn test_crypto_with_key() {
        unsafe {
            let key = [42u8; 32];
            let mut handle: *mut EntiDbCryptoHandle = ptr::null_mut();

            let result = entidb_crypto_create_with_key(key.as_ptr(), &mut handle);
            assert_eq!(result, EntiDbResult::Ok);
            assert!(!handle.is_null());

            let data = b"Secret data";
            let mut encrypted = EntiDbBuffer::empty();

            let result = entidb_crypto_encrypt(handle, data.as_ptr(), data.len(), &mut encrypted);
            assert_eq!(result, EntiDbResult::Ok);

            entidb_crypto_free(handle);

            // Re-create with same key and decrypt
            let mut handle2: *mut EntiDbCryptoHandle = ptr::null_mut();
            let result = entidb_crypto_create_with_key(key.as_ptr(), &mut handle2);
            assert_eq!(result, EntiDbResult::Ok);

            let enc_slice = std::slice::from_raw_parts(encrypted.data, encrypted.len);
            let mut decrypted = EntiDbBuffer::empty();

            let result =
                entidb_crypto_decrypt(handle2, enc_slice.as_ptr(), enc_slice.len(), &mut decrypted);
            assert_eq!(result, EntiDbResult::Ok);

            let dec_slice = std::slice::from_raw_parts(decrypted.data, decrypted.len);
            assert_eq!(dec_slice, data);

            crate::buffer::entidb_free_buffer(encrypted);
            crate::buffer::entidb_free_buffer(decrypted);
            entidb_crypto_free(handle2);
        }
    }

    #[test]
    #[cfg(feature = "encryption")]
    fn test_crypto_with_aad() {
        unsafe {
            let key = [0xABu8; 32];
            let mut handle: *mut EntiDbCryptoHandle = ptr::null_mut();

            let result = entidb_crypto_create_with_key(key.as_ptr(), &mut handle);
            assert_eq!(result, EntiDbResult::Ok);

            let data = b"Authenticated data";
            let aad = b"collection:users,entity:12345";
            let mut encrypted = EntiDbBuffer::empty();

            let result = entidb_crypto_encrypt_with_aad(
                handle,
                data.as_ptr(),
                data.len(),
                aad.as_ptr(),
                aad.len(),
                &mut encrypted,
            );
            assert_eq!(result, EntiDbResult::Ok);

            let enc_slice = std::slice::from_raw_parts(encrypted.data, encrypted.len);

            // Decrypt with correct AAD
            let mut decrypted = EntiDbBuffer::empty();
            let result = entidb_crypto_decrypt_with_aad(
                handle,
                enc_slice.as_ptr(),
                enc_slice.len(),
                aad.as_ptr(),
                aad.len(),
                &mut decrypted,
            );
            assert_eq!(result, EntiDbResult::Ok);

            let dec_slice = std::slice::from_raw_parts(decrypted.data, decrypted.len);
            assert_eq!(dec_slice, data);

            // Decrypt with wrong AAD should fail
            let wrong_aad = b"wrong:aad";
            let mut bad_decrypted = EntiDbBuffer::empty();
            let result = entidb_crypto_decrypt_with_aad(
                handle,
                enc_slice.as_ptr(),
                enc_slice.len(),
                wrong_aad.as_ptr(),
                wrong_aad.len(),
                &mut bad_decrypted,
            );
            assert_eq!(result, EntiDbResult::Error);

            crate::buffer::entidb_free_buffer(encrypted);
            crate::buffer::entidb_free_buffer(decrypted);
            entidb_crypto_free(handle);
        }
    }

    #[test]
    #[cfg(feature = "encryption")]
    fn test_crypto_from_password() {
        unsafe {
            let password = b"my-secret-password";
            let salt = b"random-salt-value";
            let mut handle: *mut EntiDbCryptoHandle = ptr::null_mut();

            let result = entidb_crypto_create_from_password(
                password.as_ptr(),
                password.len(),
                salt.as_ptr(),
                salt.len(),
                &mut handle,
            );
            assert_eq!(result, EntiDbResult::Ok);
            assert!(!handle.is_null());

            let data = b"Password-protected data";
            let mut encrypted = EntiDbBuffer::empty();

            let result = entidb_crypto_encrypt(handle, data.as_ptr(), data.len(), &mut encrypted);
            assert_eq!(result, EntiDbResult::Ok);

            entidb_crypto_free(handle);

            // Re-derive key from same password + salt
            let mut handle2: *mut EntiDbCryptoHandle = ptr::null_mut();
            let result = entidb_crypto_create_from_password(
                password.as_ptr(),
                password.len(),
                salt.as_ptr(),
                salt.len(),
                &mut handle2,
            );
            assert_eq!(result, EntiDbResult::Ok);

            let enc_slice = std::slice::from_raw_parts(encrypted.data, encrypted.len);
            let mut decrypted = EntiDbBuffer::empty();

            let result =
                entidb_crypto_decrypt(handle2, enc_slice.as_ptr(), enc_slice.len(), &mut decrypted);
            assert_eq!(result, EntiDbResult::Ok);

            let dec_slice = std::slice::from_raw_parts(decrypted.data, decrypted.len);
            assert_eq!(dec_slice, data);

            crate::buffer::entidb_free_buffer(encrypted);
            crate::buffer::entidb_free_buffer(decrypted);
            entidb_crypto_free(handle2);
        }
    }

    #[test]
    fn test_crypto_available() {
        let available = entidb_crypto_available();
        assert_eq!(available, cfg!(feature = "encryption"));
    }
}
