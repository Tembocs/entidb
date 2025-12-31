//! Server configuration.

use std::net::SocketAddr;
use std::time::Duration;

/// Configuration for the sync server.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Address to bind to.
    pub bind_addr: SocketAddr,
    /// Maximum concurrent connections.
    pub max_connections: usize,
    /// Request timeout.
    pub request_timeout: Duration,
    /// Maximum batch size for pull responses.
    pub max_pull_batch: u32,
    /// Maximum batch size for push requests.
    pub max_push_batch: u32,
    /// Whether to require authentication.
    pub require_auth: bool,
    /// Secret key for token validation (if auth enabled).
    pub auth_secret: Option<Vec<u8>>,
}

impl ServerConfig {
    /// Creates a new server configuration.
    pub fn new(bind_addr: SocketAddr) -> Self {
        Self {
            bind_addr,
            max_connections: 1000,
            request_timeout: Duration::from_secs(30),
            max_pull_batch: 100,
            max_push_batch: 100,
            require_auth: false,
            auth_secret: None,
        }
    }

    /// Sets the maximum concurrent connections.
    pub fn with_max_connections(mut self, max: usize) -> Self {
        self.max_connections = max;
        self
    }

    /// Sets the request timeout.
    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Sets the maximum pull batch size.
    pub fn with_max_pull_batch(mut self, size: u32) -> Self {
        self.max_pull_batch = size;
        self
    }

    /// Sets the maximum push batch size.
    pub fn with_max_push_batch(mut self, size: u32) -> Self {
        self.max_push_batch = size;
        self
    }

    /// Enables authentication with the given secret.
    pub fn with_auth(mut self, secret: Vec<u8>) -> Self {
        self.require_auth = true;
        self.auth_secret = Some(secret);
        self
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self::new(SocketAddr::from(([127, 0, 0, 1], 8080)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = ServerConfig::default();
        assert_eq!(config.max_connections, 1000);
        assert!(!config.require_auth);
    }

    #[test]
    fn config_builder() {
        let config = ServerConfig::new("0.0.0.0:9000".parse().unwrap())
            .with_max_connections(500)
            .with_max_pull_batch(50)
            .with_auth(vec![1, 2, 3, 4]);

        assert_eq!(config.max_connections, 500);
        assert_eq!(config.max_pull_batch, 50);
        assert!(config.require_auth);
        assert_eq!(config.auth_secret, Some(vec![1, 2, 3, 4]));
    }
}
