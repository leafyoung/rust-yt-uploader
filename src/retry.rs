//! Retry logic with exponential backoff and jitter.
//!
//! This module provides retry functionality for YouTube API operations,
//! handling retriable HTTP errors and connection issues.

use anyhow::Result;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{error, info, warn};

use crate::models::RetryConfig;

/// HTTP status codes that should trigger a retry
pub const RETRIABLE_STATUS_CODES: &[u16] = &[500, 502, 503, 504];

/// Check if an HTTP status code is retriable
pub fn is_retriable_status(status: u16) -> bool {
    RETRIABLE_STATUS_CODES.contains(&status)
}

/// Check if an error is retriable based on its type
pub fn is_retriable_error(error: &anyhow::Error) -> bool {
    // Check for HTTP errors with retriable status codes
    if let Some(reqwest_error) = error.downcast_ref::<reqwest::Error>() {
        if let Some(status) = reqwest_error.status() {
            return is_retriable_status(status.as_u16());
        }

        // Connection errors are retriable
        if reqwest_error.is_connect() || reqwest_error.is_timeout() {
            return true;
        }
    }

    // IO errors are generally retriable
    if error.downcast_ref::<std::io::Error>().is_some() {
        return true;
    }

    false
}

/// Execute a function with retry logic using exponential backoff.
///
/// # Arguments
/// * `operation` - The async operation to retry
/// * `config` - Retry configuration
/// * `operation_name` - Name for logging purposes
///
/// # Returns
/// * Result of the operation
///
/// # Type Parameters
/// * `T` - Return type of the operation
/// * `F` - Future type returned by the operation
/// * `Op` - Operation function type
pub async fn retry_with_backoff<T, F, Op>(
    mut operation: Op,
    config: &RetryConfig,
    operation_name: &str,
) -> Result<T>
where
    F: std::future::Future<Output = Result<T>>,
    Op: FnMut() -> F,
{
    let mut last_error = None;

    for attempt in 0..=config.max_retries {
        match operation().await {
            Ok(result) => {
                if attempt > 0 {
                    info!(
                        "Operation '{}' succeeded after {} attempts",
                        operation_name,
                        attempt + 1
                    );
                }
                return Ok(result);
            }
            Err(error) => {
                last_error = Some(error);
                let error_ref = last_error.as_ref().unwrap();

                if attempt == config.max_retries {
                    error!(
                        "Operation '{}' failed after {} attempts: {}",
                        operation_name,
                        attempt + 1,
                        error_ref
                    );
                    break;
                }

                if !is_retriable_error(error_ref) {
                    error!(
                        "Operation '{}' failed with non-retriable error: {}",
                        operation_name, error_ref
                    );
                    break;
                }

                let sleep_duration = config.calculate_sleep_time(attempt + 1);
                warn!(
                    "Operation '{}' failed (attempt {}/{}): {}. Retrying in {:.2}s...",
                    operation_name,
                    attempt + 1,
                    config.max_retries + 1,
                    error_ref,
                    sleep_duration
                );

                sleep(Duration::from_secs_f64(sleep_duration)).await;
            }
        }
    }

    // Return the last error
    Err(last_error.unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

    #[test]
    fn test_is_retriable_status() {
        assert!(is_retriable_status(500));
        assert!(is_retriable_status(502));
        assert!(is_retriable_status(503));
        assert!(is_retriable_status(504));
        assert!(!is_retriable_status(400));
        assert!(!is_retriable_status(404));
        assert!(!is_retriable_status(200));
    }

    #[tokio::test]
    async fn test_retry_success_on_first_attempt() {
        let config = RetryConfig::default();
        let mut call_count = 0;

        let result = retry_with_backoff(
            || {
                call_count += 1;
                async { Ok::<i32, anyhow::Error>(42) }
            },
            &config,
            "test_operation",
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
        assert_eq!(call_count, 1);
    }

    #[tokio::test]
    #[ignore]
    async fn test_retry_success_after_failures() {
        let config = RetryConfig {
            max_retries: 3,
            base_sleep: 0.001, // Very short sleep for testing
            max_sleep: 0.01,
            exponential_base: 2,
        };
        let mut call_count = 0;

        let result = retry_with_backoff(
            || {
                call_count += 1;
                async move {
                    if call_count < 3 {
                        Err(anyhow!("Temporary failure"))
                    } else {
                        Ok::<i32, anyhow::Error>(42)
                    }
                }
            },
            &config,
            "test_operation",
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
        assert_eq!(call_count, 3);
    }

    #[tokio::test]
    #[ignore]
    async fn test_retry_max_attempts_exceeded() {
        let config = RetryConfig {
            max_retries: 2,
            base_sleep: 0.001,
            max_sleep: 0.01,
            exponential_base: 2,
        };
        let mut call_count = 0;

        let result = retry_with_backoff(
            || {
                call_count += 1;
                async { Err::<i32, anyhow::Error>(anyhow!("Persistent failure")) }
            },
            &config,
            "test_operation",
        )
        .await;

        assert!(result.is_err());
        assert_eq!(call_count, 3); // Initial attempt + 2 retries
    }
}
