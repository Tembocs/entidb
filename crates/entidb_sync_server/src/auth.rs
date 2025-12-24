//! Authentication support for the sync server.
//!
//! This module provides token-based authentication using HMAC-SHA256.
//! Tokens include a timestamp for expiration checking.
//!
//! ## Token Format
//!
//! Tokens are composed of:
//! - 16 bytes: device_id
//! - 16 bytes: db_id
//! - 8 bytes: timestamp (Unix millis, big-endian)
//! - 32 bytes: HMAC-SHA256 signature
//!
//! Total: 72 bytes, base64-encoded for transport.

use crate::error::{ServerError, ServerResult};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

type HmacSha256 = Hmac<Sha256>;

/// Authentication configuration.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// Secret key for HMAC.
    pub secret: Vec<u8>,
    /// Token expiration duration.
    pub token_expiry: Duration,
}

impl AuthConfig {
    /// Creates a new auth configuration.
    pub fn new(secret: Vec<u8>) -> Self {
        Self {
            secret,
            token_expiry: Duration::from_secs(24 * 60 * 60), // 24 hours
        }
    }

    /// Sets the token expiration duration.
    pub fn with_expiry(mut self, expiry: Duration) -> Self {
        self.token_expiry = expiry;
        self
    }
}

/// Token validator for incoming requests.
#[derive(Clone)]
pub struct TokenValidator {
    config: AuthConfig,
}

impl TokenValidator {
    /// Creates a new token validator.
    pub fn new(config: AuthConfig) -> Self {
        Self { config }
    }

    /// Creates a new auth token for a device.
    ///
    /// # Arguments
    ///
    /// * `device_id` - The device identifier
    /// * `db_id` - The database identifier
    ///
    /// # Returns
    ///
    /// The token as bytes (can be base64 encoded for transport).
    pub fn create_token(&self, device_id: [u8; 16], db_id: [u8; 16]) -> Vec<u8> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let mut data = Vec::with_capacity(40);
        data.extend_from_slice(&device_id);
        data.extend_from_slice(&db_id);
        data.extend_from_slice(&timestamp.to_be_bytes());

        let signature = self.sign(&data);

        let mut token = data;
        token.extend_from_slice(&signature);
        token
    }

    /// Validates a token.
    ///
    /// # Arguments
    ///
    /// * `token` - The token bytes
    /// * `expected_device_id` - Expected device ID (must match)
    /// * `expected_db_id` - Expected database ID (must match)
    ///
    /// # Returns
    ///
    /// Ok(()) if valid, error otherwise.
    pub fn validate_token(
        &self,
        token: &[u8],
        expected_device_id: &[u8; 16],
        expected_db_id: &[u8; 16],
    ) -> ServerResult<()> {
        if token.len() != 72 {
            return Err(ServerError::NotAuthorized("Invalid token length".into()));
        }

        // Extract components
        let device_id: [u8; 16] = token[0..16].try_into().unwrap();
        let db_id: [u8; 16] = token[16..32].try_into().unwrap();
        let timestamp_bytes: [u8; 8] = token[32..40].try_into().unwrap();
        let signature: [u8; 32] = token[40..72].try_into().unwrap();

        // Verify device and db match
        if device_id != *expected_device_id {
            return Err(ServerError::NotAuthorized("Device ID mismatch".into()));
        }
        if db_id != *expected_db_id {
            return Err(ServerError::NotAuthorized("Database ID mismatch".into()));
        }

        // Verify signature
        let expected_signature = self.sign(&token[0..40]);
        if signature != expected_signature.as_slice() {
            return Err(ServerError::NotAuthorized("Invalid signature".into()));
        }

        // Check expiration
        let timestamp = u64::from_be_bytes(timestamp_bytes);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let expiry_millis = self.config.token_expiry.as_millis() as u64;
        if now > timestamp + expiry_millis {
            return Err(ServerError::NotAuthorized("Token expired".into()));
        }

        Ok(())
    }

    /// Signs data with HMAC-SHA256.
    fn sign(&self, data: &[u8]) -> [u8; 32] {
        let mut mac =
            HmacSha256::new_from_slice(&self.config.secret).expect("HMAC can take key of any size");
        mac.update(data);
        let result = mac.finalize();
        result.into_bytes().into()
    }
}

/// Simple token validator that doesn't check expiration.
/// Useful for testing.
#[derive(Clone)]
pub struct SimpleTokenValidator {
    secret: Vec<u8>,
}

impl SimpleTokenValidator {
    /// Creates a validator with a simple shared secret.
    pub fn new(secret: Vec<u8>) -> Self {
        Self { secret }
    }

    /// Validates that the token matches the expected format.
    /// For simple tokens, this just checks that the secret matches.
    pub fn validate(&self, token: &[u8]) -> ServerResult<()> {
        if token == self.secret.as_slice() {
            Ok(())
        } else {
            Err(ServerError::NotAuthorized("Invalid token".into()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_validate_token() {
        let config = AuthConfig::new(b"test-secret-key-32-bytes-long!!".to_vec());
        let validator = TokenValidator::new(config);

        let device_id = [1u8; 16];
        let db_id = [2u8; 16];

        let token = validator.create_token(device_id, db_id);
        assert_eq!(token.len(), 72);

        let result = validator.validate_token(&token, &device_id, &db_id);
        assert!(result.is_ok());
    }

    #[test]
    fn reject_wrong_device() {
        let config = AuthConfig::new(b"test-secret-key-32-bytes-long!!".to_vec());
        let validator = TokenValidator::new(config);

        let device_id = [1u8; 16];
        let wrong_device = [3u8; 16];
        let db_id = [2u8; 16];

        let token = validator.create_token(device_id, db_id);
        let result = validator.validate_token(&token, &wrong_device, &db_id);
        assert!(result.is_err());
    }

    #[test]
    fn reject_wrong_db() {
        let config = AuthConfig::new(b"test-secret-key-32-bytes-long!!".to_vec());
        let validator = TokenValidator::new(config);

        let device_id = [1u8; 16];
        let db_id = [2u8; 16];
        let wrong_db = [3u8; 16];

        let token = validator.create_token(device_id, db_id);
        let result = validator.validate_token(&token, &device_id, &wrong_db);
        assert!(result.is_err());
    }

    #[test]
    fn reject_tampered_token() {
        let config = AuthConfig::new(b"test-secret-key-32-bytes-long!!".to_vec());
        let validator = TokenValidator::new(config);

        let device_id = [1u8; 16];
        let db_id = [2u8; 16];

        let mut token = validator.create_token(device_id, db_id);
        token[50] ^= 0xFF; // Flip a bit in the signature

        let result = validator.validate_token(&token, &device_id, &db_id);
        assert!(result.is_err());
    }

    #[test]
    fn reject_expired_token() {
        // Create a config with 0 expiry
        let config = AuthConfig::new(b"test-secret-key-32-bytes-long!!".to_vec())
            .with_expiry(Duration::from_secs(0));
        let validator = TokenValidator::new(config);

        let device_id = [1u8; 16];
        let db_id = [2u8; 16];

        let token = validator.create_token(device_id, db_id);

        // Wait a tiny bit to ensure expiration
        std::thread::sleep(Duration::from_millis(10));

        let result = validator.validate_token(&token, &device_id, &db_id);
        assert!(result.is_err());
    }

    #[test]
    fn simple_validator() {
        let validator = SimpleTokenValidator::new(b"shared-secret".to_vec());

        assert!(validator.validate(b"shared-secret").is_ok());
        assert!(validator.validate(b"wrong-secret").is_err());
    }
}
