//! Web UI sessions and login throttling (spec 02 §10).
//!
//! - A session id is 32 bytes from the OS CSPRNG, hex-encoded, and lives only in
//!   memory (a restart signs everyone out). It expires after [`IDLE_TIMEOUT`]
//!   without use and [`ABSOLUTE_TIMEOUT`] after login, whichever comes first.
//! - Each session remembers the admin hash it was created under; once the
//!   password changes (`web set-password`) the hash on disk differs and every
//!   older session stops validating — no restart needed.
//! - Failed logins are throttled per client (an IPv4 address, or an IPv6 /64 so
//!   address rotation inside one allocation does not reset the budget). An
//!   attempt is counted BEFORE the password is checked, so a burst of parallel
//!   guesses cannot slip past the limit. [`MAX_FAILURES`] attempts without a
//!   success lock that client out for [`BASE_LOCKOUT`], doubling on each
//!   repeat up to [`MAX_LOCKOUT`].
//! - At most [`VERIFY_SLOTS`] argon2 verifications run at once (each needs
//!   ~19 MiB); further logins are answered "busy" instead of queueing, so a flood
//!   of logins cannot exhaust memory or the blocking thread pool.
//!
//! Nothing here logs a password or a session id.

use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::error::AppError;
use crate::web::admin;

/// A session unused for this long is gone.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(12 * 60 * 60);
/// No session outlives this, however active.
pub const ABSOLUTE_TIMEOUT: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// Live sessions kept at most; the least recently used is evicted beyond it.
pub const MAX_SESSIONS: usize = 64;
/// Unsuccessful attempts per client before a lockout starts.
pub const MAX_FAILURES: u32 = 5;
/// First lockout; each further lockout doubles it.
pub const BASE_LOCKOUT: Duration = Duration::from_secs(30);
/// Longest lockout.
pub const MAX_LOCKOUT: Duration = Duration::from_secs(15 * 60);
/// A client with no attempt for this long starts from a clean slate.
pub const FAILURE_MEMORY: Duration = Duration::from_secs(60 * 60);
/// Concurrent argon2 verifications.
pub const VERIFY_SLOTS: usize = 2;
/// Distinct clients tracked for throttling (stale ones are dropped beyond it).
const MAX_TRACKED_CLIENTS: usize = 4096;

struct Session {
    /// The admin hash this session was created under.
    hash: String,
    created: Instant,
    last_seen: Instant,
}

struct Failures {
    /// Attempts since the last success or lockout.
    count: u32,
    /// How many lockouts this client has earned (drives the backoff).
    lockouts: u32,
    locked_until: Option<Instant>,
    last_attempt: Instant,
}

/// Why a login did not produce a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginError {
    /// Wrong password.
    Rejected,
    /// Too many failures from this client; retry after this many seconds.
    RateLimited(u64),
    /// Every verification slot is busy; retry shortly.
    Busy,
    /// No admin password configured (the daemon refuses `--web` without one,
    /// so this only happens if the file was deleted while running).
    NotConfigured,
    /// The hash file could not be read, or hashing failed.
    Internal(String),
}

/// Session store + login throttle for one web server.
pub struct Auth {
    config_dir: PathBuf,
    sessions: Mutex<HashMap<String, Session>>,
    failures: Mutex<HashMap<IpAddr, Failures>>,
    verify_slots: tokio::sync::Semaphore,
}

impl Auth {
    pub fn new(config_dir: PathBuf) -> Self {
        Self {
            config_dir,
            sessions: Mutex::new(HashMap::new()),
            failures: Mutex::new(HashMap::new()),
            verify_slots: tokio::sync::Semaphore::new(VERIFY_SLOTS),
        }
    }

    fn current_hash(&self) -> Option<String> {
        admin::load_hash(&self.config_dir).ok().flatten()
    }

    /// Count an attempt from `key` BEFORE verifying it. `Err(secs)` while the
    /// client is locked out. Reaching [`MAX_FAILURES`] starts the next lockout
    /// (this attempt still runs; the following ones are refused).
    fn begin_attempt(&self, key: IpAddr, now: Instant) -> Result<(), u64> {
        let Ok(mut failures) = self.failures.lock() else {
            return Err(BASE_LOCKOUT.as_secs());
        };
        if failures.len() >= MAX_TRACKED_CLIENTS && !failures.contains_key(&key) {
            failures.retain(|_, f| {
                f.locked_until.is_some_and(|t| t > now)
                    || now.duration_since(f.last_attempt) < FAILURE_MEMORY
            });
            if failures.len() >= MAX_TRACKED_CLIENTS {
                if let Some(oldest) = failures
                    .iter()
                    .min_by_key(|(_, f)| f.last_attempt)
                    .map(|(k, _)| *k)
                {
                    failures.remove(&oldest);
                }
            }
        }
        let entry = failures.entry(key).or_insert(Failures {
            count: 0,
            lockouts: 0,
            locked_until: None,
            last_attempt: now,
        });
        if let Some(until) = entry.locked_until {
            if until > now {
                return Err(until.duration_since(now).as_secs().max(1));
            }
            entry.locked_until = None;
        }
        if now.duration_since(entry.last_attempt) >= FAILURE_MEMORY {
            entry.count = 0;
            entry.lockouts = 0;
        }
        entry.last_attempt = now;
        entry.count += 1;
        if entry.count >= MAX_FAILURES {
            let factor = 1u32 << entry.lockouts.min(8);
            entry.locked_until = Some(now + (BASE_LOCKOUT * factor).min(MAX_LOCKOUT));
            entry.lockouts += 1;
            entry.count = 0;
        }
        Ok(())
    }

    /// A correct password clears the client's record.
    fn succeed(&self, key: IpAddr) {
        if let Ok(mut failures) = self.failures.lock() {
            failures.remove(&key);
        }
    }

    /// Check `password` for a login from client `key` (see [`throttle_key`]);
    /// on success return a new session id.
    pub async fn login(&self, key: IpAddr, password: String) -> Result<String, LoginError> {
        let key = throttle_key(key);
        // Bound the memory/CPU a login flood can take: no free slot → "busy",
        // and a busy answer does not count against the client.
        let Ok(_slot) = self.verify_slots.try_acquire() else {
            return Err(LoginError::Busy);
        };
        let now = Instant::now();
        self.begin_attempt(key, now)
            .map_err(LoginError::RateLimited)?;
        let hash = match admin::load_hash(&self.config_dir) {
            Ok(Some(hash)) => hash,
            Ok(None) => return Err(LoginError::NotConfigured),
            Err(e) => return Err(LoginError::Internal(e.to_string())),
        };
        // argon2 is deliberately slow: keep it off the async workers.
        let check = hash.clone();
        let ok = tokio::task::spawn_blocking(move || admin::verify_password(&check, &password))
            .await
            .map_err(|e| LoginError::Internal(format!("verify task failed: {e}")))?;
        if !ok {
            tracing::warn!(client = %key, "web login rejected");
            return Err(LoginError::Rejected);
        }
        self.succeed(key);
        let id = new_session_id().map_err(|e| LoginError::Internal(e.to_string()))?;
        self.insert_session(id.clone(), hash, now);
        tracing::info!(client = %key, "web login");
        Ok(id)
    }

    fn insert_session(&self, id: String, hash: String, now: Instant) {
        let Ok(mut sessions) = self.sessions.lock() else {
            return;
        };
        sessions.retain(|_, s| is_live(s, now));
        if sessions.len() >= MAX_SESSIONS {
            if let Some(lru) = sessions
                .iter()
                .min_by_key(|(_, s)| s.last_seen)
                .map(|(k, _)| k.clone())
            {
                sessions.remove(&lru);
            }
        }
        sessions.insert(
            id,
            Session {
                hash,
                created: now,
                last_seen: now,
            },
        );
    }

    /// Whether `id` is a live session under the CURRENT admin password; touches
    /// its idle timer when it is.
    pub fn validate(&self, id: &str) -> bool {
        let now = Instant::now();
        let current = self.current_hash();
        let Ok(mut sessions) = self.sessions.lock() else {
            return false;
        };
        let valid = match (sessions.get(id), &current) {
            (Some(s), Some(hash)) => is_live(s, now) && &s.hash == hash,
            _ => false,
        };
        if valid {
            if let Some(s) = sessions.get_mut(id) {
                s.last_seen = now;
            }
        } else {
            sessions.remove(id);
        }
        valid
    }

    /// Forget a session (logout).
    pub fn logout(&self, id: &str) {
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.remove(id);
        }
    }
}

/// The throttling identity of a client: IPv4 as is (IPv4-mapped IPv6 folded to
/// IPv4), IPv6 truncated to its /64 — one allocation is one client.
pub fn throttle_key(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(_) => ip,
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => IpAddr::V6(std::net::Ipv6Addr::from(u128::from(v6) & (u128::MAX << 64))),
        },
    }
}

fn is_live(s: &Session, now: Instant) -> bool {
    now.duration_since(s.last_seen) < IDLE_TIMEOUT
        && now.duration_since(s.created) < ABSOLUTE_TIMEOUT
}

/// 32 CSPRNG bytes, hex-encoded (64 chars).
fn new_session_id() -> Result<String, AppError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|e| AppError::Internal(format!("no OS randomness for a session: {e}")))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    const IP: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 7));
    const PASSWORD: &str = "correct horse battery";

    fn auth() -> (tempfile::TempDir, Auth) {
        let dir = tempfile::tempdir().expect("tmp");
        admin::set_password(dir.path(), PASSWORD).expect("set");
        let auth = Auth::new(dir.path().to_path_buf());
        (dir, auth)
    }

    #[tokio::test]
    async fn login_issues_a_random_session_that_validates() {
        let (_dir, auth) = auth();
        let a = auth.login(IP, PASSWORD.into()).await.expect("login");
        let b = auth.login(IP, PASSWORD.into()).await.expect("login");
        assert_eq!(a.len(), 64);
        assert_ne!(a, b, "session ids are random");
        assert!(auth.validate(&a));
        assert!(!auth.validate("not-a-session"));
        auth.logout(&a);
        assert!(!auth.validate(&a), "logout ends the session");
        assert!(auth.validate(&b));
    }

    #[tokio::test]
    async fn wrong_password_is_rejected_then_rate_limited() {
        let (_dir, auth) = auth();
        for _ in 0..MAX_FAILURES {
            assert_eq!(
                auth.login(IP, "wrong-password".into()).await,
                Err(LoginError::Rejected)
            );
        }
        // Locked out — even the right password is refused for the window.
        assert!(matches!(
            auth.login(IP, PASSWORD.into()).await,
            Err(LoginError::RateLimited(_))
        ));
        // Another IP is unaffected.
        let other = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 8));
        assert!(auth.login(other, PASSWORD.into()).await.is_ok());
    }

    #[tokio::test]
    async fn a_parallel_burst_cannot_exceed_the_attempt_budget() {
        let (_dir, auth) = auth();
        let auth = std::sync::Arc::new(auth);
        let mut tasks = Vec::new();
        for _ in 0..40 {
            let auth = auth.clone();
            tasks.push(tokio::spawn(async move {
                auth.login(IP, "wrong-password".into()).await
            }));
        }
        let mut verified = 0;
        for t in tasks {
            if let Ok(Err(LoginError::Rejected)) = t.await {
                verified += 1;
            }
        }
        assert!(
            verified <= MAX_FAILURES as usize,
            "{verified} guesses were verified; the budget is {MAX_FAILURES}"
        );
    }

    #[test]
    fn ipv6_is_throttled_per_64_and_mapped_v4_folds() {
        let a: IpAddr = "2001:db8:1:2:aaaa::1".parse().expect("ip");
        let b: IpAddr = "2001:db8:1:2:ffff::9".parse().expect("ip");
        let c: IpAddr = "2001:db8:1:3::1".parse().expect("ip");
        assert_eq!(throttle_key(a), throttle_key(b));
        assert_ne!(throttle_key(a), throttle_key(c));
        let mapped: IpAddr = "::ffff:10.0.0.7".parse().expect("ip");
        assert_eq!(throttle_key(mapped), IP);
    }

    #[test]
    fn lockouts_back_off_and_cap() {
        let dir = tempfile::tempdir().expect("tmp");
        let auth = Auth::new(dir.path().to_path_buf());
        let t0 = Instant::now();
        for _ in 0..MAX_FAILURES {
            assert!(auth.begin_attempt(IP, t0).is_ok());
        }
        let first = auth.begin_attempt(IP, t0).expect_err("locked");
        assert!(first <= BASE_LOCKOUT.as_secs());
        // After the first lockout, the next MAX_FAILURES earn a doubled one.
        let t1 = t0 + BASE_LOCKOUT;
        for _ in 0..MAX_FAILURES {
            assert!(auth.begin_attempt(IP, t1).is_ok());
        }
        let second = auth.begin_attempt(IP, t1).expect_err("locked again");
        assert!(second > BASE_LOCKOUT.as_secs() && second <= 2 * BASE_LOCKOUT.as_secs());
        auth.succeed(IP);
        assert!(
            auth.begin_attempt(IP, t1).is_ok(),
            "success clears the record"
        );
    }

    #[tokio::test]
    async fn changing_the_password_signs_everyone_out() {
        let (dir, auth) = auth();
        let id = auth.login(IP, PASSWORD.into()).await.expect("login");
        assert!(auth.validate(&id));
        admin::set_password(dir.path(), "a brand new password").expect("change");
        assert!(
            !auth.validate(&id),
            "old sessions die with the old password"
        );
        assert_eq!(
            auth.login(IP, PASSWORD.into()).await,
            Err(LoginError::Rejected)
        );
        assert!(auth.login(IP, "a brand new password".into()).await.is_ok());
    }

    #[tokio::test]
    async fn missing_password_file_means_not_configured() {
        let dir = tempfile::tempdir().expect("tmp");
        let auth = Auth::new(dir.path().to_path_buf());
        assert_eq!(
            auth.login(IP, PASSWORD.into()).await,
            Err(LoginError::NotConfigured)
        );
    }

    #[test]
    fn idle_and_absolute_expiry() {
        let now = Instant::now();
        let fresh = Session {
            hash: String::new(),
            created: now,
            last_seen: now,
        };
        assert!(is_live(&fresh, now));
        assert!(!is_live(&fresh, now + IDLE_TIMEOUT));
        let busy_but_old = Session {
            hash: String::new(),
            created: now,
            last_seen: now + ABSOLUTE_TIMEOUT,
        };
        assert!(!is_live(&busy_but_old, now + ABSOLUTE_TIMEOUT));
    }
}
