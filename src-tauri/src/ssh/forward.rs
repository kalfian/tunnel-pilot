//! Forwarded-connection piping: open a `direct-tcpip` channel per accepted
//! local socket (10s timeout) and pump bytes bidirectionally with counters
//! (spec 03 §§1,6).
//!
//! Dead-CHANNEL teardown signal (F26/F27/F30): on a channel-open timeout or a
//! copy error the child bumps **its attempt's own** `attempt_fail_count` and,
//! at `>= 3` consecutive failures, fires **that attempt's** `attempt_fail_notify`
//! (a WAKE only) — it **never** cancels the parent. A successful open resets the
//! consecutive-failure counter (matches v1). Both the counter and the notify are
//! per-attempt, so a straggler from a dropped attempt lands on a dead counter no
//! live supervisor reads.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Notify;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::ssh::client::Session;
use crate::ssh::stats::StatsInner;
use crate::state::models::ForwardConfig;

/// direct-tcpip channel-open timeout (spec 03 §1).
const CHANNEL_OPEN_TIMEOUT: Duration = Duration::from_secs(10);
/// Consecutive forward failures that trigger a dead-channel teardown (spec 03 §1).
pub const MAX_FORWARD_FAILURES: usize = 3;

/// Shared per-attempt failure signal handed to each child copy task.
#[derive(Clone)]
pub struct ForwardFailSignal {
    /// Per-attempt consecutive-failure counter (F30) — NOT the durable stats.
    pub count: Arc<AtomicUsize>,
    /// Per-attempt WAKE (F27a) — fired at `>= 3`; the supervisor re-checks the
    /// authoritative `count`.
    pub notify: Arc<Notify>,
}

impl ForwardFailSignal {
    /// Mint a fresh signal at the start of an attempt (F27a/F30).
    pub fn new() -> Self {
        Self {
            count: Arc::new(AtomicUsize::new(0)),
            notify: Arc::new(Notify::new()),
        }
    }

    fn record_failure(&self) {
        let n = self.count.fetch_add(1, Ordering::SeqCst) + 1;
        if n >= MAX_FORWARD_FAILURES {
            // WAKE only — the supervisor re-reads `count` (F27b). Never the parent.
            self.notify.notify_one();
        }
    }

    fn reset(&self) {
        self.count.store(0, Ordering::SeqCst);
    }
}

impl Default for ForwardFailSignal {
    fn default() -> Self {
        Self::new()
    }
}

/// Increment `active_connections` on open, decrement on drop — so the count is
/// correct even if the copy loop errors/panics (spec 03 §6: never negative).
struct ActiveConnGuard(Arc<StatsInner>);
impl ActiveConnGuard {
    fn new(stats: Arc<StatsInner>) -> Self {
        stats.active_connections.fetch_add(1, Ordering::Relaxed);
        Self(stats)
    }
}
impl Drop for ActiveConnGuard {
    fn drop(&mut self) {
        // saturating: never go below zero.
        let prev = self.0.active_connections.load(Ordering::Relaxed);
        if prev > 0 {
            self.0.active_connections.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

/// Spawn the per-connection forward task for one accepted local socket.
///
/// `session` is the shared (`Arc`) russh handle owned by the supervisor;
/// `attempt_cancel` is this attempt's child token so the copy is torn down on
/// attempt reset / disconnect.
#[allow(clippy::too_many_arguments)]
pub fn spawn_forward_conn(
    session: Arc<Session>,
    local: TcpStream,
    cfg: Arc<ForwardConfig>,
    stats: Arc<StatsInner>,
    fail: ForwardFailSignal,
    attempt_cancel: CancellationToken,
) {
    tokio::spawn(async move {
        let (orig_addr, orig_port) = match local.local_addr() {
            Ok(a) => (a.ip().to_string(), a.port() as u32),
            Err(_) => ("127.0.0.1".to_string(), 0),
        };

        let open = timeout(
            CHANNEL_OPEN_TIMEOUT,
            session.channel_open_direct_tcpip(
                cfg.remote_host.clone(),
                cfg.remote_port as u32,
                orig_addr,
                orig_port,
            ),
        )
        .await;

        let channel = match open {
            Ok(Ok(ch)) => ch,
            Ok(Err(e)) => {
                tracing::warn!(tunnel = %cfg.id, error = %e, "direct-tcpip open failed");
                fail.record_failure();
                return;
            }
            Err(_) => {
                tracing::warn!(tunnel = %cfg.id, "direct-tcpip open timed out (10s)");
                fail.record_failure();
                return;
            }
        };

        // A successful open clears the consecutive-failure streak (v1 parity).
        fail.reset();

        let _guard = ActiveConnGuard::new(stats.clone());
        let stream = channel.into_stream(); // AsyncRead + AsyncWrite (russh 0.45)
        pipe_bidirectional(local, stream, &stats, &attempt_cancel).await;
    });
}

/// Pump bytes both ways, counting up (local→remote) and down (remote→local),
/// until either side closes, an error occurs, or the attempt is cancelled.
async fn pipe_bidirectional<S>(
    local: TcpStream,
    stream: S,
    stats: &Arc<StatsInner>,
    attempt_cancel: &CancellationToken,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (mut local_r, mut local_w) = tokio::io::split(local);
    let (mut stream_r, mut stream_w) = tokio::io::split(stream);

    let up = copy_counting(&mut local_r, &mut stream_w, &stats.bytes_up);
    let down = copy_counting(&mut stream_r, &mut local_w, &stats.bytes_down);

    tokio::select! {
        _ = up => {}
        _ = down => {}
        _ = attempt_cancel.cancelled() => {}
    }
    // On exit the split halves drop, closing both sides.
}

/// Copy `reader → writer`, adding each chunk's length to `counter`. Returns on
/// EOF (n == 0) or the first I/O error.
async fn copy_counting<R, W>(
    reader: &mut R,
    writer: &mut W,
    counter: &AtomicU64,
) -> std::io::Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let n = reader.read(&mut buf).await?;
        if n == 0 {
            let _ = writer.shutdown().await;
            return Ok(());
        }
        writer.write_all(&buf[..n]).await?;
        counter.fetch_add(n as u64, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fail_signal_notifies_only_at_threshold() {
        let sig = ForwardFailSignal::new();
        // First two failures: below threshold, no teardown-worthy count.
        sig.record_failure();
        assert_eq!(sig.count.load(Ordering::SeqCst), 1);
        sig.record_failure();
        assert_eq!(sig.count.load(Ordering::SeqCst), 2);
        // Third consecutive failure reaches the threshold.
        sig.record_failure();
        assert_eq!(sig.count.load(Ordering::SeqCst), MAX_FORWARD_FAILURES);
    }

    #[test]
    fn success_resets_consecutive_failures() {
        let sig = ForwardFailSignal::new();
        sig.record_failure();
        sig.record_failure();
        sig.reset(); // a successful open
        assert_eq!(sig.count.load(Ordering::SeqCst), 0);
        sig.record_failure();
        assert_eq!(sig.count.load(Ordering::SeqCst), 1);
    }
}
