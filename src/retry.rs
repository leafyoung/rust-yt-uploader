//! Retry logic with exponential backoff and jitter.
//!
//! This module provides retry functionality for YouTube API operations,
//! handling retriable HTTP errors and connection issues.

use anyhow::Result;
use rand::RngExt;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{error, info, warn};

/// Default maximum retries after the initial attempt (legacy config-struct default).
pub const DEFAULT_MAX_RETRIES: u32 = 10;
/// Default base delay in milliseconds for the exponential backoff (legacy config-struct default).
pub const DEFAULT_BASE_DELAY_MS: u64 = 1000;
/// Upper bound (in seconds) on any single backoff sleep (legacy config-struct default).
const MAX_SLEEP_SECS: f64 = 60.0;
/// Exponential growth factor between retry attempts (legacy config-struct default).
const EXPONENTIAL_BASE: u32 = 2;

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

/// Compute the backoff sleep for a retry attempt: jittered exponential growth
/// (`rand() * base_delay * EXPONENTIAL_BASE^attempt`), capped at [`MAX_SLEEP_SECS`].
///
/// With the default parameters this reproduces the legacy config-struct math
/// exactly (base 1.0s, exponential base 2, capped at 60s, sleep attempt is 1-based).
fn backoff_sleep_secs(retry_attempt: u32, base_delay_ms: u64) -> f64 {
    // f64 powi is exact for powers of two up to 2^53, matching the legacy
    // integer pow for every realistic attempt count without overflow panics.
    let exponential_sleep = (EXPONENTIAL_BASE as f64).powi(retry_attempt as i32);
    let jittered =
        rand::rng().random::<f64>() * (base_delay_ms as f64 / 1000.0) * exponential_sleep;
    jittered.min(MAX_SLEEP_SECS)
}

/// Execute a function with retry logic using exponential backoff.
///
/// The operation runs up to `max_retries + 1` times (initial attempt plus
/// retries). Only retriable errors (see [`is_retriable_error`]) are retried;
/// non-retriable failures abort immediately.
///
/// # Arguments
/// * `operation` - The async operation to retry
/// * `max_retries` - Maximum retries after the initial attempt (11 total attempts with the default of 10)
/// * `base_delay_ms` - Base delay in milliseconds; sleep grows exponentially from here with jitter, capped at 60s
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
    max_retries: u32,
    base_delay_ms: u64,
    operation_name: &str,
) -> Result<T>
where
    F: std::future::Future<Output = Result<T>>,
    Op: FnMut() -> F,
{
    let mut last_error = None;

    for attempt in 0..=max_retries {
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

                if attempt == max_retries {
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

                let sleep_duration = backoff_sleep_secs(attempt + 1, base_delay_ms);
                warn!(
                    "Operation '{}' failed (attempt {}/{}): {}. Retrying in {:.2}s...",
                    operation_name,
                    attempt + 1,
                    max_retries + 1,
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

    #[test]
    fn test_default_retry_params_match_legacy_config() {
        assert_eq!(DEFAULT_MAX_RETRIES, 10);
        assert_eq!(DEFAULT_BASE_DELAY_MS, 1000);
    }

    #[test]
    fn test_backoff_sleep_bounds() {
        // Sleep must stay within [0, MAX_SLEEP_SECS] for every legacy attempt.
        for attempt in 1..=DEFAULT_MAX_RETRIES {
            let sleep_time = backoff_sleep_secs(attempt, DEFAULT_BASE_DELAY_MS);
            assert!(sleep_time >= 0.0);
            assert!(sleep_time <= MAX_SLEEP_SECS);
        }
    }

    #[test]
    fn test_backoff_sleep_caps_large_attempts() {
        // Even tiny base delays with huge attempt numbers must be capped at 60s.
        let sleep_time = backoff_sleep_secs(40, DEFAULT_BASE_DELAY_MS);
        assert_eq!(sleep_time, MAX_SLEEP_SECS);
    }

    #[tokio::test]
    async fn test_retry_success_on_first_attempt() {
        let mut call_count = 0;

        let result = retry_with_backoff(
            || {
                call_count += 1;
                async { Ok::<i32, anyhow::Error>(42) }
            },
            DEFAULT_MAX_RETRIES,
            DEFAULT_BASE_DELAY_MS,
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
            3,
            1, // 1ms base delay: very short sleeps for testing
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
        let mut call_count = 0;

        let result = retry_with_backoff(
            || {
                call_count += 1;
                async { Err::<i32, anyhow::Error>(anyhow!("Persistent failure")) }
            },
            2,
            1,
            "test_operation",
        )
        .await;

        assert!(result.is_err());
        assert_eq!(call_count, 3); // Initial attempt + 2 retries
    }
}
