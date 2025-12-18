//! Transport layer abstraction for sync operations.

use crate::error::{SyncError, SyncResult};
use entidb_sync_protocol::{
    HandshakeRequest, HandshakeResponse, PullRequest, PullResponse, PushRequest, PushResponse,
};

/// A sync transport handles network communication with the sync server.
///
/// This trait abstracts the network layer, allowing for different implementations
/// (HTTP, WebSocket, mock for testing, etc.).
pub trait SyncTransport: Send + Sync {
    /// Performs a handshake with the server.
    fn handshake(&self, request: &HandshakeRequest) -> SyncResult<HandshakeResponse>;

    /// Pulls changes from the server.
    fn pull(&self, request: &PullRequest) -> SyncResult<PullResponse>;

    /// Pushes changes to the server.
    fn push(&self, request: &PushRequest) -> SyncResult<PushResponse>;

    /// Checks if the transport is connected.
    fn is_connected(&self) -> bool;

    /// Closes the transport connection.
    fn close(&self) -> SyncResult<()>;
}

/// Request types for sync operations.
#[derive(Debug, Clone)]
pub enum SyncRequest {
    /// Handshake request.
    Handshake(HandshakeRequest),
    /// Pull request.
    Pull(PullRequest),
    /// Push request.
    Push(PushRequest),
}

/// Response types for sync operations.
#[derive(Debug, Clone)]
pub enum SyncResponse {
    /// Handshake response.
    Handshake(HandshakeResponse),
    /// Pull response.
    Pull(PullResponse),
    /// Push response.
    Push(PushResponse),
}

/// A mock transport for testing.
#[derive(Debug, Default)]
pub struct MockTransport {
    connected: std::sync::atomic::AtomicBool,
    handshake_response: std::sync::Mutex<Option<HandshakeResponse>>,
    pull_response: std::sync::Mutex<Option<PullResponse>>,
    push_response: std::sync::Mutex<Option<PushResponse>>,
}

impl MockTransport {
    /// Creates a new mock transport.
    pub fn new() -> Self {
        Self {
            connected: std::sync::atomic::AtomicBool::new(true),
            handshake_response: std::sync::Mutex::new(None),
            pull_response: std::sync::Mutex::new(None),
            push_response: std::sync::Mutex::new(None),
        }
    }

    /// Sets the handshake response.
    pub fn set_handshake_response(&self, response: HandshakeResponse) {
        *self.handshake_response.lock().unwrap() = Some(response);
    }

    /// Sets the pull response.
    pub fn set_pull_response(&self, response: PullResponse) {
        *self.pull_response.lock().unwrap() = Some(response);
    }

    /// Sets the push response.
    pub fn set_push_response(&self, response: PushResponse) {
        *self.push_response.lock().unwrap() = Some(response);
    }

    /// Sets the connected state.
    pub fn set_connected(&self, connected: bool) {
        self.connected
            .store(connected, std::sync::atomic::Ordering::SeqCst);
    }
}

impl SyncTransport for MockTransport {
    fn handshake(&self, _request: &HandshakeRequest) -> SyncResult<HandshakeResponse> {
        if !self.is_connected() {
            return Err(SyncError::NotConnected);
        }
        self.handshake_response
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| SyncError::Protocol("No mock handshake response set".into()))
    }

    fn pull(&self, _request: &PullRequest) -> SyncResult<PullResponse> {
        if !self.is_connected() {
            return Err(SyncError::NotConnected);
        }
        self.pull_response
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| SyncError::Protocol("No mock pull response set".into()))
    }

    fn push(&self, _request: &PushRequest) -> SyncResult<PushResponse> {
        if !self.is_connected() {
            return Err(SyncError::NotConnected);
        }
        self.push_response
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| SyncError::Protocol("No mock push response set".into()))
    }

    fn is_connected(&self) -> bool {
        self.connected.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn close(&self) -> SyncResult<()> {
        self.connected
            .store(false, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_transport_connection() {
        let transport = MockTransport::new();
        assert!(transport.is_connected());

        transport.set_connected(false);
        assert!(!transport.is_connected());

        transport.close().unwrap();
        assert!(!transport.is_connected());
    }

    #[test]
    fn mock_transport_not_connected_error() {
        let transport = MockTransport::new();
        transport.set_connected(false);

        let request = HandshakeRequest::new([0u8; 16], [0u8; 16], 0);

        let result = transport.handshake(&request);
        assert!(matches!(result, Err(SyncError::NotConnected)));
    }

    #[test]
    fn mock_transport_handshake() {
        let transport = MockTransport::new();
        let response = HandshakeResponse::success(0);
        transport.set_handshake_response(response);

        let request = HandshakeRequest::new([0u8; 16], [0u8; 16], 0);

        let result = transport.handshake(&request).unwrap();
        assert!(result.success);
    }
}
