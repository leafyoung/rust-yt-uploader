//! Stream wrapper for tracking upload progress and bandwidth throttling.
//!
//! This module provides a streaming wrapper that:
//! - Tracks upload progress in real-time
//! - Implements bandwidth throttling using token bucket algorithm
//! - Reports progress to a ProgressReporter trait implementation

use futures::Stream;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tokio_util::bytes::Bytes;

use crate::youtube_client::ProgressReporter;

/// Stream wrapper that tracks upload progress and throttles bandwidth
pub struct ProgressStream<S> {
    inner: S,
    bytes_sent: Arc<AtomicU64>,
    total_bytes: u64,
    progress_reporter: Arc<dyn ProgressReporter>,
    filename: String,
    bandwidth_limit: Option<u64>, // bytes per second
    last_update: Instant,
    tokens: f64, // Token bucket for rate limiting
}

impl<S> ProgressStream<S>
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Unpin,
{
    /// Create a new ProgressStream wrapper
    ///
    /// # Arguments
    /// * `inner` - The underlying stream to wrap
    /// * `total_bytes` - Total size of the data to upload
    /// * `progress_reporter` - Reporter for progress updates
    /// * `filename` - Name of the file being uploaded
    /// * `bandwidth_limit` - Optional bandwidth limit in bytes per second
    pub fn new(
        inner: S,
        total_bytes: u64,
        progress_reporter: Arc<dyn ProgressReporter>,
        filename: String,
        bandwidth_limit: Option<u64>,
    ) -> Self {
        Self {
            inner,
            bytes_sent: Arc::new(AtomicU64::new(0)),
            total_bytes,
            progress_reporter,
            filename,
            bandwidth_limit,
            last_update: Instant::now(),
            tokens: bandwidth_limit.map(|limit| limit as f64).unwrap_or(0.0),
        }
    }
}

impl<S> Stream for ProgressStream<S>
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Unpin,
{
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Refill token bucket based on elapsed time (for bandwidth throttling)
        if let Some(limit) = self.bandwidth_limit {
            let now = Instant::now();
            let elapsed = now.duration_since(self.last_update).as_secs_f64();
            self.last_update = now;

            // Add tokens based on bandwidth limit and elapsed time
            self.tokens += (limit as f64) * elapsed;

            // Cap tokens at 2x the bandwidth limit (allows small bursts)
            let max_tokens = (limit as f64) * 2.0;
            if self.tokens > max_tokens {
                self.tokens = max_tokens;
            }
        }

        match Pin::new(&mut self.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(bytes))) => {
                let bytes_len = bytes.len() as u64;

                // Apply bandwidth throttling if enabled
                if let Some(limit) = self.bandwidth_limit {
                    let required_tokens = bytes_len as f64;

                    if self.tokens < required_tokens {
                        // Not enough tokens, calculate sleep time
                        let tokens_needed = required_tokens - self.tokens;
                        let sleep_duration =
                            Duration::from_secs_f64(tokens_needed / (limit as f64));

                        // Wake up after sleep duration
                        let waker = cx.waker().clone();
                        tokio::spawn(async move {
                            sleep(sleep_duration).await;
                            waker.wake();
                        });

                        return Poll::Pending;
                    }

                    // Consume tokens
                    self.tokens -= required_tokens;
                }

                // Update progress
                let new_total = self.bytes_sent.fetch_add(bytes_len, Ordering::Relaxed) + bytes_len;
                self.progress_reporter
                    .report_progress(new_total, self.total_bytes, &self.filename);

                Poll::Ready(Some(Ok(bytes)))
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;

    struct TestReporter {
        updates: Arc<std::sync::Mutex<Vec<(u64, u64)>>>,
    }

    impl ProgressReporter for TestReporter {
        fn report_progress(&self, uploaded: u64, total: u64, _filename: &str) {
            self.updates.lock().unwrap().push((uploaded, total));
        }

        fn finish(&self) {}
    }

    #[tokio::test]
    async fn test_progress_tracking() {
        use futures::StreamExt;

        let updates = Arc::new(std::sync::Mutex::new(Vec::new()));
        let reporter = Arc::new(TestReporter {
            updates: updates.clone(),
        });

        let data = vec![
            Ok(Bytes::from(vec![1u8; 100])),
            Ok(Bytes::from(vec![2u8; 200])),
            Ok(Bytes::from(vec![3u8; 300])),
        ];
        let stream = stream::iter(data);

        let progress_stream = ProgressStream::new(
            stream,
            600,
            reporter,
            "test.dat".to_string(),
            None, // No bandwidth limit for testing
        );

        let collected: Vec<_> = progress_stream.collect().await;
        assert_eq!(collected.len(), 3);

        let progress_updates = updates.lock().unwrap();
        assert_eq!(progress_updates.len(), 3);
        assert_eq!(progress_updates[0], (100, 600));
        assert_eq!(progress_updates[1], (300, 600));
        assert_eq!(progress_updates[2], (600, 600));
    }
}
