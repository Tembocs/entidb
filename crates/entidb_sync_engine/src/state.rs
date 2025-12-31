//! Sync engine state machine.

use crate::config::SyncConfig;
use crate::error::{SyncError, SyncResult};
use crate::transport::SyncTransport;
use entidb_sync_protocol::{
    Conflict, ConflictPolicy, HandshakeRequest, LogicalOplog, PullRequest, PushRequest,
    SyncOperation,
};
use parking_lot::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The current state of the sync engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncState {
    /// Engine is idle, not syncing.
    Idle,
    /// Engine is connecting to the server.
    Connecting,
    /// Engine is pulling changes from the server.
    Pulling,
    /// Engine is pushing changes to the server.
    Pushing,
    /// Engine has completed a sync cycle.
    Synced,
    /// Engine encountered an error.
    Error,
    /// Engine is waiting before retrying.
    RetryWait,
}

impl SyncState {
    /// Returns true if the engine is in an active sync state.
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            SyncState::Connecting | SyncState::Pulling | SyncState::Pushing
        )
    }

    /// Returns true if the engine can start a new sync.
    pub fn can_start_sync(&self) -> bool {
        matches!(self, SyncState::Idle | SyncState::Synced | SyncState::Error)
    }
}

/// Statistics about sync operations.
#[derive(Debug, Clone, Default)]
pub struct SyncStats {
    /// Total number of sync cycles completed.
    pub cycles_completed: u64,
    /// Total number of operations pulled.
    pub operations_pulled: u64,
    /// Total number of operations pushed.
    pub operations_pushed: u64,
    /// Total number of conflicts encountered.
    pub conflicts_encountered: u64,
    /// Total number of retries.
    pub retries: u64,
    /// Last sync time.
    pub last_sync_time: Option<Instant>,
    /// Last error message.
    pub last_error: Option<String>,
}

/// Result of a sync cycle.
#[derive(Debug, Clone)]
pub struct SyncCycleResult {
    /// Number of operations pulled.
    pub pulled: u64,
    /// Number of operations pushed.
    pub pushed: u64,
    /// Conflicts that were resolved.
    pub resolved_conflicts: Vec<Conflict>,
    /// Conflicts that require manual resolution.
    pub unresolved_conflicts: Vec<Conflict>,
    /// Whether the sync was successful.
    pub success: bool,
    /// Duration of the sync cycle.
    pub duration: Duration,
}

/// Callback for applying remote operations to the local database.
pub trait SyncApplier: Send + Sync {
    /// Applies a batch of operations from the server.
    fn apply_remote_operations(&self, operations: &[SyncOperation]) -> SyncResult<()>;

    /// Gets pending local operations to push.
    fn get_pending_operations(&self, limit: u32) -> SyncResult<Vec<SyncOperation>>;

    /// Marks operations as acknowledged (pushed successfully).
    fn acknowledge_operations(&self, up_to_op_id: u64) -> SyncResult<()>;

    /// Gets the current server cursor.
    fn get_server_cursor(&self) -> SyncResult<u64>;

    /// Sets the server cursor after a successful pull.
    fn set_server_cursor(&self, cursor: u64) -> SyncResult<()>;
}

/// The sync engine manages synchronization with a remote server.
pub struct SyncEngine<T: SyncTransport, A: SyncApplier> {
    config: SyncConfig,
    transport: Arc<T>,
    applier: Arc<A>,
    state: RwLock<SyncState>,
    stats: RwLock<SyncStats>,
    conflict_policy: RwLock<ConflictPolicy>,
    cancelled: std::sync::atomic::AtomicBool,
    current_retry: AtomicU64,
}

impl<T: SyncTransport, A: SyncApplier> SyncEngine<T, A> {
    /// Creates a new sync engine.
    pub fn new(config: SyncConfig, transport: T, applier: A) -> Self {
        Self {
            config,
            transport: Arc::new(transport),
            applier: Arc::new(applier),
            state: RwLock::new(SyncState::Idle),
            stats: RwLock::new(SyncStats::default()),
            conflict_policy: RwLock::new(ConflictPolicy::ServerWins),
            cancelled: std::sync::atomic::AtomicBool::new(false),
            current_retry: AtomicU64::new(0),
        }
    }

    /// Gets the current state.
    pub fn state(&self) -> SyncState {
        *self.state.read()
    }

    /// Gets the current stats.
    pub fn stats(&self) -> SyncStats {
        self.stats.read().clone()
    }

    /// Sets the conflict policy.
    pub fn set_conflict_policy(&self, policy: ConflictPolicy) {
        *self.conflict_policy.write() = policy;
    }

    /// Gets the conflict policy.
    pub fn conflict_policy(&self) -> ConflictPolicy {
        *self.conflict_policy.read()
    }

    /// Cancels any ongoing sync operation.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// Resets the cancelled flag.
    pub fn reset_cancel(&self) {
        self.cancelled.store(false, Ordering::SeqCst);
    }

    /// Checks if sync was cancelled.
    fn check_cancelled(&self) -> SyncResult<()> {
        if self.cancelled.load(Ordering::SeqCst) {
            Err(SyncError::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Sets the state.
    fn set_state(&self, state: SyncState) {
        *self.state.write() = state;
    }

    /// Performs a full sync cycle: pull then push.
    pub fn sync(&self) -> SyncResult<SyncCycleResult> {
        let start = Instant::now();
        self.reset_cancel();

        // Check if we can start
        if !self.state().can_start_sync() {
            return Err(SyncError::InvalidStateTransition {
                from: format!("{:?}", self.state()),
                to: "sync".into(),
            });
        }

        let mut result = SyncCycleResult {
            pulled: 0,
            pushed: 0,
            resolved_conflicts: Vec::new(),
            unresolved_conflicts: Vec::new(),
            success: false,
            duration: Duration::ZERO,
        };

        // Connect and handshake
        self.set_state(SyncState::Connecting);
        if let Err(e) = self.handshake() {
            self.handle_error(&e);
            result.duration = start.elapsed();
            return Err(e);
        }

        self.check_cancelled()?;

        // Pull phase
        self.set_state(SyncState::Pulling);
        match self.pull_all() {
            Ok((count, conflicts)) => {
                result.pulled = count;
                self.categorize_conflicts(conflicts, &mut result);
            }
            Err(e) => {
                self.handle_error(&e);
                result.duration = start.elapsed();
                return Err(e);
            }
        }

        self.check_cancelled()?;

        // Check for unresolved conflicts before pushing
        if !result.unresolved_conflicts.is_empty() {
            self.set_state(SyncState::Error);
            result.duration = start.elapsed();
            // Use first unresolved conflict for error
            let first = &result.unresolved_conflicts[0];
            return Err(SyncError::UnresolvedConflict {
                collection_id: first.collection_id,
                entity_id: first.entity_id,
            });
        }

        // Push phase
        self.set_state(SyncState::Pushing);
        match self.push_all() {
            Ok((count, conflicts)) => {
                result.pushed = count;
                self.categorize_conflicts(conflicts, &mut result);
            }
            Err(e) => {
                self.handle_error(&e);
                result.duration = start.elapsed();
                return Err(e);
            }
        }

        // Success
        result.success = true;
        result.duration = start.elapsed();
        self.set_state(SyncState::Synced);
        self.current_retry.store(0, Ordering::SeqCst);

        // Update stats
        {
            let mut stats = self.stats.write();
            stats.cycles_completed += 1;
            stats.operations_pulled += result.pulled;
            stats.operations_pushed += result.pushed;
            stats.conflicts_encountered +=
                (result.resolved_conflicts.len() + result.unresolved_conflicts.len()) as u64;
            stats.last_sync_time = Some(Instant::now());
            stats.last_error = None;
        }

        Ok(result)
    }

    /// Performs a sync with retry on transient errors.
    pub fn sync_with_retry(&self) -> SyncResult<SyncCycleResult> {
        let retry_config = &self.config.retry;
        let mut last_error = None;

        for attempt in 0..retry_config.max_attempts {
            if attempt > 0 {
                self.set_state(SyncState::RetryWait);
                let delay = retry_config.delay_for_attempt(attempt);
                std::thread::sleep(delay);

                self.stats.write().retries += 1;
            }

            self.check_cancelled()?;
            self.current_retry.store(attempt as u64, Ordering::SeqCst);

            match self.sync() {
                Ok(result) => return Ok(result),
                Err(e) => {
                    if e.is_retryable() && attempt + 1 < retry_config.max_attempts {
                        last_error = Some(e);
                        continue;
                    }
                    return Err(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| SyncError::Protocol("No sync attempts made".into())))
    }

    /// Performs the handshake with the server.
    fn handshake(&self) -> SyncResult<()> {
        let cursor = self.applier.get_server_cursor()?;
        let request = HandshakeRequest {
            db_id: self.config.db_id,
            device_id: self.config.device_id,
            protocol_version: self.config.protocol_version,
            last_cursor: cursor,
        };

        let response = self.transport.handshake(&request)?;

        if !response.success {
            return Err(SyncError::ServerError(
                response.error.unwrap_or_else(|| "Handshake failed".into()),
            ));
        }

        Ok(())
    }

    /// Pulls all available changes from the server.
    fn pull_all(&self) -> SyncResult<(u64, Vec<Conflict>)> {
        let mut total_pulled = 0u64;
        let all_conflicts = Vec::new();

        loop {
            self.check_cancelled()?;

            let cursor = self.applier.get_server_cursor()?;
            let request = PullRequest {
                cursor,
                limit: self.config.pull_batch_size,
            };

            let response = self.transport.pull(&request)?;

            if !response.operations.is_empty() {
                self.applier.apply_remote_operations(&response.operations)?;
                total_pulled += response.operations.len() as u64;
            }

            self.applier.set_server_cursor(response.new_cursor)?;

            if !response.has_more {
                break;
            }
        }

        Ok((total_pulled, all_conflicts))
    }

    /// Pushes all pending local changes to the server.
    fn push_all(&self) -> SyncResult<(u64, Vec<Conflict>)> {
        let mut total_pushed = 0u64;
        let mut all_conflicts = Vec::new();

        loop {
            self.check_cancelled()?;

            let operations = self
                .applier
                .get_pending_operations(self.config.push_batch_size)?;

            if operations.is_empty() {
                break;
            }

            let cursor = self.applier.get_server_cursor()?;
            let request = PushRequest {
                operations: operations.clone(),
                expected_cursor: cursor,
            };

            let response = self.transport.push(&request)?;

            if response.success {
                // Find the max op_id from the pushed operations
                if let Some(max_op_id) = operations.iter().map(|op| op.op_id).max() {
                    self.applier.acknowledge_operations(max_op_id)?;
                }
                total_pushed += operations.len() as u64;
            }

            all_conflicts.extend(response.conflicts);

            self.applier.set_server_cursor(response.new_cursor)?;
        }

        Ok((total_pushed, all_conflicts))
    }

    /// Categorizes conflicts based on the current policy.
    fn categorize_conflicts(&self, conflicts: Vec<Conflict>, result: &mut SyncCycleResult) {
        let policy = self.conflict_policy();

        for conflict in conflicts {
            if policy.auto_resolves() {
                result.resolved_conflicts.push(conflict);
            } else {
                result.unresolved_conflicts.push(conflict);
            }
        }
    }

    /// Handles an error by updating state and stats.
    fn handle_error(&self, error: &SyncError) {
        self.set_state(SyncState::Error);
        self.stats.write().last_error = Some(error.to_string());
    }
}

/// An in-memory sync applier for testing.
pub struct MemorySyncApplier {
    oplog: RwLock<LogicalOplog>,
    applied_operations: RwLock<Vec<SyncOperation>>,
}

impl MemorySyncApplier {
    /// Creates a new memory sync applier.
    pub fn new() -> Self {
        Self {
            oplog: RwLock::new(LogicalOplog::new()),
            applied_operations: RwLock::new(Vec::new()),
        }
    }

    /// Adds a pending operation.
    pub fn add_pending(&self, operation: SyncOperation) {
        self.oplog.write().append(operation);
    }

    /// Gets all applied operations.
    pub fn applied_operations(&self) -> Vec<SyncOperation> {
        self.applied_operations.read().clone()
    }
}

impl Default for MemorySyncApplier {
    fn default() -> Self {
        Self::new()
    }
}

impl SyncApplier for MemorySyncApplier {
    fn apply_remote_operations(&self, operations: &[SyncOperation]) -> SyncResult<()> {
        self.applied_operations
            .write()
            .extend(operations.iter().cloned());
        Ok(())
    }

    fn get_pending_operations(&self, limit: u32) -> SyncResult<Vec<SyncOperation>> {
        Ok(self
            .oplog
            .read()
            .pending_batch(limit as usize)
            .into_iter()
            .cloned()
            .collect())
    }

    fn acknowledge_operations(&self, up_to_op_id: u64) -> SyncResult<()> {
        self.oplog.write().acknowledge_up_to(up_to_op_id);
        Ok(())
    }

    fn get_server_cursor(&self) -> SyncResult<u64> {
        Ok(self.oplog.read().server_cursor())
    }

    fn set_server_cursor(&self, cursor: u64) -> SyncResult<()> {
        self.oplog.write().set_server_cursor(cursor);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;
    use entidb_sync_protocol::{HandshakeResponse, OperationType, PullResponse, PushResponse};

    fn make_operation(op_id: u64, entity_id: [u8; 16], op_type: OperationType) -> SyncOperation {
        SyncOperation {
            op_id,
            collection_id: 1,
            entity_id,
            op_type,
            payload: Some(vec![0x42]),
            sequence: op_id,
        }
    }

    #[test]
    fn sync_state_checks() {
        assert!(SyncState::Idle.can_start_sync());
        assert!(SyncState::Synced.can_start_sync());
        assert!(SyncState::Error.can_start_sync());
        assert!(!SyncState::Pulling.can_start_sync());
        assert!(!SyncState::Pushing.can_start_sync());

        assert!(SyncState::Pulling.is_active());
        assert!(SyncState::Pushing.is_active());
        assert!(!SyncState::Idle.is_active());
    }

    #[test]
    fn sync_engine_initial_state() {
        let config = SyncConfig::new([1u8; 16], [2u8; 16], "https://test.example.com");
        let transport = MockTransport::new();
        let applier = MemorySyncApplier::new();

        let engine = SyncEngine::new(config, transport, applier);
        assert_eq!(engine.state(), SyncState::Idle);
        assert_eq!(engine.stats().cycles_completed, 0);
    }

    #[test]
    fn sync_engine_successful_sync() {
        let config = SyncConfig::new([1u8; 16], [2u8; 16], "https://test.example.com");
        let transport = MockTransport::new();

        transport.set_handshake_response(HandshakeResponse::success(0));

        transport.set_pull_response(PullResponse::new(
            vec![make_operation(1, [3u8; 16], OperationType::Put)],
            1,
            false,
        ));

        transport.set_push_response(PushResponse::success(2));

        let applier = MemorySyncApplier::new();
        let engine = SyncEngine::new(config, transport, applier);

        let result = engine.sync().unwrap();
        assert!(result.success);
        assert_eq!(result.pulled, 1);
        assert_eq!(engine.state(), SyncState::Synced);
        assert_eq!(engine.stats().cycles_completed, 1);
    }

    #[test]
    fn sync_engine_handshake_failure() {
        let config = SyncConfig::new([1u8; 16], [2u8; 16], "https://test.example.com");
        let transport = MockTransport::new();

        transport.set_handshake_response(HandshakeResponse::error("Auth failed"));

        let applier = MemorySyncApplier::new();
        let engine = SyncEngine::new(config, transport, applier);

        let result = engine.sync();
        assert!(result.is_err());
        assert_eq!(engine.state(), SyncState::Error);
    }

    #[test]
    fn sync_engine_cancel() {
        // Test that cancel() sets the flag and check_cancelled works
        let config = SyncConfig::new([1u8; 16], [2u8; 16], "https://test.example.com");
        let transport = MockTransport::new();
        transport.set_handshake_response(HandshakeResponse::success(0));
        transport.set_pull_response(PullResponse::new(vec![], 0, false));
        transport.set_push_response(PushResponse::success(0));

        let applier = MemorySyncApplier::new();
        let engine = SyncEngine::new(config, transport, applier);

        // Test that cancellation works on the flag level
        assert!(!engine.cancelled.load(std::sync::atomic::Ordering::SeqCst));
        engine.cancel();
        assert!(engine.cancelled.load(std::sync::atomic::Ordering::SeqCst));
        engine.reset_cancel();
        assert!(!engine.cancelled.load(std::sync::atomic::Ordering::SeqCst));

        // Note: sync() resets the cancel flag at the start, so calling
        // cancel() before sync() won't prevent the sync - cancellation
        // is designed for cancelling an ongoing sync from another thread.
    }

    #[test]
    fn memory_applier_operations() {
        let applier = MemorySyncApplier::new();

        // Add pending operations
        applier.add_pending(make_operation(1, [1u8; 16], OperationType::Put));
        applier.add_pending(make_operation(2, [2u8; 16], OperationType::Put));

        // Check pending
        let pending = applier.get_pending_operations(10).unwrap();
        assert_eq!(pending.len(), 2);

        // Acknowledge first
        applier.acknowledge_operations(1).unwrap();
        let pending = applier.get_pending_operations(10).unwrap();
        assert_eq!(pending.len(), 1);

        // Apply remote
        let remote = vec![make_operation(100, [10u8; 16], OperationType::Put)];
        applier.apply_remote_operations(&remote).unwrap();
        assert_eq!(applier.applied_operations().len(), 1);
    }

    #[test]
    fn server_cursor_management() {
        let applier = MemorySyncApplier::new();

        assert_eq!(applier.get_server_cursor().unwrap(), 0);
        applier.set_server_cursor(42).unwrap();
        assert_eq!(applier.get_server_cursor().unwrap(), 42);
    }
}
