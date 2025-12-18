//! Configuration for the sync engine.

use std::time::Duration;

/// Configuration for sync operations.
#[derive(Debug, Clone)]
pub struct SyncConfig {
    /// Database ID (unique per database).
    pub db_id: [u8; 16],
    /// Device ID (unique per device).
    pub device_id: [u8; 16],
    /// Server URL.
    pub server_url: String,
    /// Protocol version.
    pub protocol_version: u16,
    /// Maximum batch size for pull operations.
    pub pull_batch_size: u32,
    /// Maximum batch size for push operations.
    pub push_batch_size: u32,
    /// Retry configuration.
    pub retry: RetryConfig,
    /// Sync interval for automatic sync.
    pub sync_interval: Option<Duration>,
    /// Request timeout.
    pub timeout: Duration,
}

impl SyncConfig {
    /// Creates a new sync configuration.
    pub fn new(db_id: [u8; 16], device_id: [u8; 16], server_url: impl Into<String>) -> Self {
        Self {
            db_id,
            device_id,
            server_url: server_url.into(),
            protocol_version: 1,
            pull_batch_size: 100,
            push_batch_size: 100,
            retry: RetryConfig::default(),
            sync_interval: None,
            timeout: Duration::from_secs(30),
        }
    }

    /// Sets the pull batch size.
    pub fn with_pull_batch_size(mut self, size: u32) -> Self {
        self.pull_batch_size = size;
        self
    }

    /// Sets the push batch size.
    pub fn with_push_batch_size(mut self, size: u32) -> Self {
        self.push_batch_size = size;
        self
    }

    /// Sets the retry configuration.
    pub fn with_retry(mut self, retry: RetryConfig) -> Self {
        self.retry = retry;
        self
    }

    /// Sets the sync interval for automatic sync.
    pub fn with_sync_interval(mut self, interval: Duration) -> Self {
        self.sync_interval = Some(interval);
        self
    }

    /// Sets the request timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self::new([0u8; 16], [0u8; 16], "")
    }
}

/// Configuration for retry behavior.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts.
    pub max_attempts: u32,
    /// Initial delay between retries.
    pub initial_delay: Duration,
    /// Maximum delay between retries.
    pub max_delay: Duration,
    /// Multiplier for exponential backoff.
    pub backoff_multiplier: f64,
    /// Whether to add jitter to delays.
    pub add_jitter: bool,
}

impl RetryConfig {
    /// Creates a new retry configuration.
    pub fn new(max_attempts: u32) -> Self {
        Self {
            max_attempts,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(30),
            backoff_multiplier: 2.0,
            add_jitter: true,
        }
    }

    /// Creates a configuration with no retries.
    pub fn no_retry() -> Self {
        Self {
            max_attempts: 1,
            initial_delay: Duration::ZERO,
            max_delay: Duration::ZERO,
            backoff_multiplier: 1.0,
            add_jitter: false,
        }
    }

    /// Sets the initial delay.
    pub fn with_initial_delay(mut self, delay: Duration) -> Self {
        self.initial_delay = delay;
        self
    }

    /// Sets the maximum delay.
    pub fn with_max_delay(mut self, delay: Duration) -> Self {
        self.max_delay = delay;
        self
    }

    /// Sets the backoff multiplier.
    pub fn with_backoff_multiplier(mut self, multiplier: f64) -> Self {
        self.backoff_multiplier = multiplier;
        self
    }

    /// Calculates the delay for a given attempt (0-indexed).
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        if attempt == 0 {
            return Duration::ZERO;
        }

        let base_delay = self.initial_delay.as_secs_f64()
            * self.backoff_multiplier.powi(attempt.saturating_sub(1) as i32);

        let delay_secs = base_delay.min(self.max_delay.as_secs_f64());

        if self.add_jitter {
            // Add up to 25% jitter
            let jitter = delay_secs * 0.25 * rand_jitter();
            Duration::from_secs_f64(delay_secs + jitter)
        } else {
            Duration::from_secs_f64(delay_secs)
        }
    }
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self::new(3)
    }
}

/// Simple deterministic "jitter" for tests (no external RNG dependency).
fn rand_jitter() -> f64 {
    // Use a simple hash of current time for pseudo-random jitter
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    (nanos % 1000) as f64 / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_config_builder() {
        let config = SyncConfig::new([1u8; 16], [2u8; 16], "https://sync.example.com")
            .with_pull_batch_size(50)
            .with_push_batch_size(25)
            .with_timeout(Duration::from_secs(60));

        assert_eq!(config.db_id, [1u8; 16]);
        assert_eq!(config.device_id, [2u8; 16]);
        assert_eq!(config.server_url, "https://sync.example.com");
        assert_eq!(config.pull_batch_size, 50);
        assert_eq!(config.push_batch_size, 25);
        assert_eq!(config.timeout, Duration::from_secs(60));
    }

    #[test]
    fn retry_config_no_retry() {
        let config = RetryConfig::no_retry();
        assert_eq!(config.max_attempts, 1);
    }

    #[test]
    fn retry_delay_calculation() {
        let config = RetryConfig::new(5)
            .with_initial_delay(Duration::from_millis(100))
            .with_backoff_multiplier(2.0);

        // First attempt has no delay
        assert_eq!(config.delay_for_attempt(0), Duration::ZERO);

        // Subsequent attempts have exponential backoff
        // Note: jitter makes exact values unpredictable, but we can check bounds
        let delay1 = config.delay_for_attempt(1);
        assert!(delay1 >= Duration::from_millis(100));
        assert!(delay1 <= Duration::from_millis(150)); // with jitter

        let delay2 = config.delay_for_attempt(2);
        assert!(delay2 >= Duration::from_millis(200));
    }

    #[test]
    fn retry_delay_respects_max() {
        let config = RetryConfig::new(10)
            .with_initial_delay(Duration::from_secs(1))
            .with_max_delay(Duration::from_secs(5))
            .with_backoff_multiplier(10.0);

        // Even with high multiplier, should not exceed max
        let delay = config.delay_for_attempt(5);
        assert!(delay <= Duration::from_millis(6250)); // 5s + 25% jitter
    }
}
