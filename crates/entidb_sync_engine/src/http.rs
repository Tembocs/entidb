//! HTTP transport implementation.
//!
//! This module provides an HTTP-based transport for the sync engine.
//! The actual HTTP client is abstracted via a trait to allow different
//! implementations (reqwest, hyper, etc.).

use crate::error::{SyncError, SyncResult};
use crate::transport::SyncTransport;
use entidb_sync_protocol::{
    HandshakeRequest, HandshakeResponse, PullRequest, PullResponse, PushRequest, PushResponse,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

/// HTTP client abstraction.
///
/// Implement this trait to provide the actual HTTP transport.
/// This allows using different HTTP libraries (reqwest, hyper, ureq, etc.)
/// or even non-HTTP transports (WebSocket, gRPC).
pub trait HttpClient: Send + Sync {
    /// Sends a POST request and returns the response body.
    fn post(&self, url: &str, body: Vec<u8>) -> Result<Vec<u8>, String>;

    /// Checks if the client is connected/healthy.
    fn is_healthy(&self) -> bool;
}

/// HTTP-based sync transport.
///
/// Uses CBOR encoding for request/response bodies.
pub struct HttpTransport<C: HttpClient> {
    /// Base URL of the sync server (e.g., "https://sync.example.com").
    base_url: String,
    /// HTTP client implementation.
    client: C,
    /// Connection state.
    connected: AtomicBool,
    /// Last error message.
    last_error: RwLock<Option<String>>,
}

impl<C: HttpClient> HttpTransport<C> {
    /// Creates a new HTTP transport.
    pub fn new(base_url: impl Into<String>, client: C) -> Self {
        Self {
            base_url: base_url.into(),
            client,
            connected: AtomicBool::new(true),
            last_error: RwLock::new(None),
        }
    }

    /// Returns the base URL.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Returns the last error message.
    pub fn last_error(&self) -> Option<String> {
        self.last_error.read().unwrap().clone()
    }

    fn set_error(&self, err: &str) {
        *self.last_error.write().unwrap() = Some(err.to_string());
    }

    fn clear_error(&self) {
        *self.last_error.write().unwrap() = None;
    }

    fn post_cbor<Req, Res>(&self, endpoint: &str, request: &Req) -> SyncResult<Res>
    where
        Req: CborEncode,
        Res: CborDecode,
    {
        if !self.is_connected() {
            return Err(SyncError::NotConnected);
        }

        // Encode request to CBOR
        let body = request
            .encode_cbor()
            .map_err(|e| SyncError::Protocol(format!("Failed to encode request: {}", e)))?;

        // Send HTTP request
        let url = format!("{}{}", self.base_url, endpoint);
        let response_body = self.client.post(&url, body).map_err(|e| {
            self.set_error(&e);
            self.connected.store(false, Ordering::SeqCst);
            SyncError::transport_retryable(e)
        })?;

        self.clear_error();

        // Decode response from CBOR
        Res::decode_cbor(&response_body)
            .map_err(|e| SyncError::Protocol(format!("Failed to decode response: {}", e)))
    }
}

impl<C: HttpClient> SyncTransport for HttpTransport<C> {
    fn handshake(&self, request: &HandshakeRequest) -> SyncResult<HandshakeResponse> {
        self.post_cbor("/sync/handshake", request)
    }

    fn pull(&self, request: &PullRequest) -> SyncResult<PullResponse> {
        self.post_cbor("/sync/pull", request)
    }

    fn push(&self, request: &PushRequest) -> SyncResult<PushResponse> {
        self.post_cbor("/sync/push", request)
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst) && self.client.is_healthy()
    }

    fn close(&self) -> SyncResult<()> {
        self.connected.store(false, Ordering::SeqCst);
        Ok(())
    }
}

/// Trait for CBOR encoding.
pub trait CborEncode {
    /// Encodes self to CBOR bytes.
    fn encode_cbor(&self) -> Result<Vec<u8>, String>;
}

/// Trait for CBOR decoding.
pub trait CborDecode: Sized {
    /// Decodes self from CBOR bytes.
    fn decode_cbor(bytes: &[u8]) -> Result<Self, String>;
}

// Implement CBOR encoding for protocol messages
impl CborEncode for HandshakeRequest {
    fn encode_cbor(&self) -> Result<Vec<u8>, String> {
        self.encode().map_err(|e| e.to_string())
    }
}

impl CborEncode for PullRequest {
    fn encode_cbor(&self) -> Result<Vec<u8>, String> {
        self.encode().map_err(|e| e.to_string())
    }
}

impl CborEncode for PushRequest {
    fn encode_cbor(&self) -> Result<Vec<u8>, String> {
        self.encode().map_err(|e| e.to_string())
    }
}

impl CborDecode for HandshakeResponse {
    fn decode_cbor(bytes: &[u8]) -> Result<Self, String> {
        Self::decode(bytes).map_err(|e| e.to_string())
    }
}

impl CborDecode for PullResponse {
    fn decode_cbor(bytes: &[u8]) -> Result<Self, String> {
        Self::decode(bytes).map_err(|e| e.to_string())
    }
}

impl CborDecode for PushResponse {
    fn decode_cbor(bytes: &[u8]) -> Result<Self, String> {
        Self::decode(bytes).map_err(|e| e.to_string())
    }
}

/// A loopback HTTP client that routes requests directly to a sync server.
///
/// Useful for testing without actual network overhead.
pub struct LoopbackClient<S: LoopbackServer> {
    server: S,
}

impl<S: LoopbackServer + Send + Sync> LoopbackClient<S> {
    /// Creates a new loopback client connected to the given server.
    pub fn new(server: S) -> Self {
        Self { server }
    }
}

/// Trait for servers that can handle loopback requests.
pub trait LoopbackServer {
    /// Handles a POST request and returns the response.
    fn handle_post(&self, path: &str, body: &[u8]) -> Result<Vec<u8>, String>;
}

impl<S: LoopbackServer + Send + Sync> HttpClient for LoopbackClient<S> {
    fn post(&self, url: &str, body: Vec<u8>) -> Result<Vec<u8>, String> {
        // Extract path from URL
        let path = url
            .find("/sync/")
            .map(|i| &url[i..])
            .unwrap_or(url);

        self.server.handle_post(path, &body)
    }

    fn is_healthy(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestClient {
        response: RwLock<Option<Vec<u8>>>,
        healthy: AtomicBool,
    }

    impl TestClient {
        fn new() -> Self {
            Self {
                response: RwLock::new(None),
                healthy: AtomicBool::new(true),
            }
        }

        fn set_response(&self, resp: Vec<u8>) {
            *self.response.write().unwrap() = Some(resp);
        }

        fn set_healthy(&self, healthy: bool) {
            self.healthy.store(healthy, Ordering::SeqCst);
        }
    }

    impl HttpClient for TestClient {
        fn post(&self, _url: &str, _body: Vec<u8>) -> Result<Vec<u8>, String> {
            self.response
                .read()
                .unwrap()
                .clone()
                .ok_or_else(|| "No response set".into())
        }

        fn is_healthy(&self) -> bool {
            self.healthy.load(Ordering::SeqCst)
        }
    }

    #[test]
    fn transport_creation() {
        let client = TestClient::new();
        let transport = HttpTransport::new("https://sync.example.com", client);
        assert_eq!(transport.base_url(), "https://sync.example.com");
        assert!(transport.is_connected());
    }

    #[test]
    fn transport_disconnect() {
        let client = TestClient::new();
        let transport = HttpTransport::new("https://sync.example.com", client);
        assert!(transport.is_connected());
        transport.close().unwrap();
        assert!(!transport.is_connected());
    }

    #[test]
    fn transport_not_connected_error() {
        let client = TestClient::new();
        let transport = HttpTransport::new("https://sync.example.com", client);
        transport.close().unwrap();

        let request = HandshakeRequest::new([0u8; 16], [0u8; 16], 0);
        let result = transport.handshake(&request);
        assert!(matches!(result, Err(SyncError::NotConnected)));
    }

    #[test]
    fn transport_unhealthy_client() {
        let client = TestClient::new();
        client.set_healthy(false);
        let transport = HttpTransport::new("https://sync.example.com", client);
        assert!(!transport.is_connected());
    }

    #[test]
    fn transport_handshake() {
        let client = TestClient::new();
        let response = HandshakeResponse::success(42);
        client.set_response(response.encode().unwrap());

        let transport = HttpTransport::new("https://sync.example.com", client);
        let request = HandshakeRequest::new([1u8; 16], [2u8; 16], 0);
        let result = transport.handshake(&request).unwrap();

        assert!(result.success);
        assert_eq!(result.server_cursor, 42);
    }
}
