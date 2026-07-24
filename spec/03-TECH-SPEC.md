# 03 — Technical Spec: Backend Subsystems

> Per-subsystem implementation spec for the Rust core. Each section: **Behavior** (what v1
> does — the contract to replicate), **Rust approach** (types, async structure, `russh`
> specifics), **Acceptance criteria** (testable).
> Cross-refs: [02-ARCHITECTURE.md](02-ARCHITECTURE.md), [04-DATA-MODEL.md](04-DATA-MODEL.md),
> [06-MIGRATION-REPO.md](06-MIGRATION-REPO.md), [07-ROADMAP.md](07-ROADMAP.md).

## Conventions

- Errors: `AppError` (thiserror) with a serde `Serialize` impl for IPC. Internal edges may
  use `anyhow`, but public command boundaries return `Result<T, AppError>`.
- Logging: `tracing` everywhere; a custom layer also feeds the in-app `log_buffer`
  (see §Logs). Every user-visible event produces a `LogEntry`.
- Time: monotonic (`Instant`) for durations/latency/uptime deltas; wall clock (`chrono`/`time`)
  only for display timestamps.
- IDs: `TunnelId = String` (uuid v4). Never reuse the object identity for cancellation —
  see §Concurrency.
- **Pinned SSH stack (F16):** `russh = "0.45"` + `russh-keys = "0.45"` (keep the two in
  lockstep on the same minor). This is the reference version for every API named below —
  **the channel stream surface and `client::Config` keepalive fields shifted across russh
  releases**, so an agent MUST verify against the pinned version before coding and, if a
  newer version is adopted, re-verify these surfaces and update this doc. **Verified against
  0.45.0 source during the M1 F16 spike** (results below are ground-truth for 0.45.0):
  1. `client::Config { keepalive_interval: Option<Duration>, keepalive_max: usize, .. }`
     (the liveness mechanism — see §2). ✓ confirmed (`keepalive_max` default 3).
  2. Channel byte-stream API: in 0.45 use `channel.into_stream()` → `ChannelStream<S>` which
     impls `AsyncRead + AsyncWrite` you can `tokio::io::copy` over. ✓ confirmed
     (`make_reader`/`make_writer` also exist in 0.45 but `into_stream()` is what we use).
  3. `session.channel_open_direct_tcpip<A: Into<String>, B: Into<String>>(host_to_connect,
     port_to_connect: u32, originator_address, originator_port: u32)`. ✓ confirmed — host args
     are `Into<String>` (pass an owned `String`/clone, not `&String`), ports are `u32`.
  4. **`ping()` does NOT exist** on the client handle (correct — that was dartssh2). Latency is
     the `channel_open_session()` RTT probe (§6). ✓ confirmed.
  5. **CORRECTION (F16 spike): `is_closed()` DOES exist** on `client::Handle` in 0.45
     (`Handle::is_closed(&self) -> bool`). The earlier claim "no `is_closed()`" was wrong. It
     returns `true` once the internal session task has exited (see §2 for why that IS the
     liveness signal).
  6. **CORRECTION (F16 spike): the session future is NOT awaitable.** `client::connect` spawns
     the session in a **private** `join: JoinHandle` field inside `Handle`; there is no public
     method to await it. So "the supervisor awaits the session future" (F7) is realized by
     **polling `Handle::is_closed()`** (a tiny poll-interval arm in the supervisor `select!`),
     NOT by awaiting a future. The design intent is unchanged — keepalive is still the teardown
     authority and there is no app-level ping counter — only the observation mechanism differs.
  7. **CORRECTION (F16 spike): publickey auth in 0.45 takes `Arc<russh::keys::key::KeyPair>`,
     NOT `PrivateKeyWithHashAlg`.** See §1 (the F22 note is corrected there). `russh` re-exports
     keys as `russh::keys`; `load_secret_key(path, Option<&str>) -> Result<KeyPair>` is a
     **blocking** call (reads the file synchronously) → wrap in `spawn_blocking`.

---

<a id="ssh"></a>
## 1. SSH engine & port forwarding (`ssh/engine.rs`, `client.rs`, `forward.rs`)

### Behavior (replicate v1)
Connect sequence (the whole sequence is **cancellation-aware**, F24 — see below):
1. Bind a local TCP listener at `localBindAddress:localPort`; retry up to **5×** with
   **500ms** backoff on `EADDRINUSE` (errno 48 macOS / 98 Linux / WSAEADDRINUSE Windows).
   **This 5×500ms bind-retry subsumes v1's `_waitForPortAvailable` 15×200ms port-release poll**
   (F25) — both exist to dodge TIME_WAIT/late-release of the local port; we keep the single
   bind-retry loop rather than a separate pre-bind poll. (P4 in [01](01-PRD.md) and M1 in
   [07](07-ROADMAP.md) reflect this.)
2. SSH-connect with a **15s** timeout.
3. Authenticate — **password OR identity file, mutually exclusive**. Identity from PEM
   (`russh-keys`). If both are somehow present, **identity file takes precedence** (matches
   v1; phrased identically in [04 §1](04-DATA-MODEL.md)).
4. Authenticate (30s timeout), then begin accepting local connections. **This ordering is
   structurally enforced by russh** — you cannot open a `direct-tcpip` channel before the
   session is authenticated — so it is simply the natural code flow (`authenticate_*().await?`
   then start the accept loop), not a workaround.

**Cancellation-aware connect+auth (F24) — REQUIRED.** A `CancellationToken` only preempts an
await it is `select!`ed against; the sequential `bind → timeout(15s, connect) → timeout(30s,
auth)` awaits do **not** observe cancellation on their own, so a naive implementation would
defer a `disconnect`/`delete` issued during `connecting` for up to ~15–30s **while still
holding the bound local port**. Therefore wrap the entire bind→connect→auth phase so it races
the tokens:
```rust
// races BOTH the durable parent and this attempt's child token
match attempt_cancel.run_until_cancelled(do_connect_auth(&cfg)).await {
    Some(Ok(session)) => { /* proceed to accept loop */ }
    Some(Err(e))      => { /* timeout/auth failure → error/reconnect path */ }
    None => { /* CANCELLED mid-connect or mid-auth */
        drop(listener);          // release the bound local port immediately
        // in-flight russh connect future is dropped by run_until_cancelled → aborted
        return;                  // supervisor cleans up / exits
    }
}
```
(`attempt_cancel` is a child of `parent_cancel`, so cancelling the parent cancels this too —
§5/F6.) This bounds teardown-during-`connecting` to the cancel latency, not the 15–30s timeouts.

Per accepted local socket: open a forwarded channel (`direct-tcpip` → `remoteHost:remotePort`,
**10s** timeout); pipe bidirectionally with byte counters (up = local→remote, down =
remote→local). On timeout/failure increment `consecutiveForwardFailures`; **after 3 consecutive
failures, tear the tunnel down and enter the reconnect-eligibility path (F26 — see §2), NOT a
permanent kill**.

Conflict handling: connecting an already-connected config disconnects it first; also
disconnect any **other** tunnel bound to the same local port before rebinding.

**Connection-lost detection (F7):** the supervisor **owns its russh session internally** and,
within its own task, watches for session death. When russh's built-in keepalive (§2) sees the
peer miss `keepalive_max` consecutive keepalives, russh's internal `session.run()` returns
`Err(KeepaliveTimeout)` and the session task exits — that exit IS the "connection lost" signal.
**F16-spike correction:** that session task is a **private** `join` handle inside
`client::Handle` and cannot be awaited directly, so the supervisor observes the exit by
**polling `Handle::is_closed()`** (which flips to `true` once the task ends) via a short
poll-interval arm in its `select!`. Semantically this is the "session future" arm; mechanically
it is an `is_closed()` poll. On detection the supervisor sets status `error`, then loops to the
next auto-reconnect attempt (§3) if eligible. **This is the single source of truth for
liveness** — there is no separate app-level ping-failure counter (a transient RTT-probe failure
does NOT flip `is_closed()`, so probe failures never cause teardown; §6).

**Supervisor & session lifecycle across reconnect (F21) — PREFERRED shape, follow this:**
The per-tunnel supervisor is a **single long-lived tokio task** that owns its russh session
**internally** and **loops across reconnect attempts, re-establishing the session in-task on
each attempt**. Its `JoinHandle` identity therefore **never changes for the life of the
tunnel**. Consequently:
- **Do NOT store the russh session handle in `TunnelHandle` / the registry**, and the 3s
  sampler must **not** hold a session clone that can outlive a reconnect (that clone would go
  stale after the session future ends → latency stops updating). Instead the supervisor
  **performs the latency RTT probe itself** (it owns the live session) and **publishes latency
  + the other derived stats into a shared cell** — a `tokio::sync::watch<TunnelStats>` (plus
  the `Arc<StatsInner>` atomics for byte/conn counters). The sampler (§6) **reads that cell**;
  it never touches the session. After a reconnect the supervisor simply writes fresh values
  into the same cell, so **latency keeps updating across reconnects**.
- **`disconnect`/`delete`**: cancel the durable **parent** token (F6) → the supervisor's
  reconnect loop exits and **deterministically tears down its listener + open channels + closes
  the session** → then `disconnect` **awaits the supervisor's (stable) `JoinHandle`** before
  removing the registry entry. This guarantees **the local port is released before the entry is
  gone**, closing the reconnect-race.

**Status-transition authority & state machine (F23) — SINGLE writer protocol.** `status`
(`watch::Sender<ForwardStatus>`) must have exactly one writer at any instant. Split ownership
so writers never overlap:
- The **supervisor task** is the sole writer of `connecting` (its first action on every
  attempt), `connected`, and `error` (terminal-retries-exhausted OR session/channel drop).
- The **command handler** owns **only** the user-disconnect path `connected → disconnecting →
  disconnected` — it can write this because it outlives the task (it cancels the parent token,
  awaits the JoinHandle, then sets `disconnected`).
- Every write goes through **one GUARDED helper** `set_status(id, new)` that takes the
  **registry lock** and **enforces the transition table — it no-ops any transition not in the
  table (F28)**, returning whether it applied. The lock makes writes non-concurrent but NOT
  order-valid; guarding is what prevents an illegal transition when two legal writers race in a
  narrow window. Concretely: **once a tunnel is `disconnecting`, a supervisor write of
  `error`/`connected` is dropped** (a session drop that lands in the window after the command
  handler set `disconnecting` must NOT flash `disconnecting → error`). `disconnecting` only
  advances to `disconnected` (command handler). Implement as an explicit match on
  `(current, new)`; any pair absent from the table below is ignored.

Allowed transitions (`set_status` applies ONLY these; everything else is a no-op):

| From | To | Written by | Trigger |
|------|----|-----------|---------|
| disconnected | connecting | supervisor | `connect_forward` starts the supervisor |
| connecting | connected | supervisor | bind+connect+auth+accept-loop up |
| connecting | error | supervisor | connect/auth failure (and not user-cancelled) |
| connecting | disconnecting | command handler | **user `disconnect_forward` while still connecting (F31)** — cancels the attempt (F24) then proceeds to `disconnected` |
| connected | error | supervisor | session future ended / 3 forward failures (F26) |
| connected | disconnecting | command handler | user `disconnect_forward` |
| disconnecting | disconnected | command handler | cleanup done + JoinHandle awaited |
| error | connecting | supervisor | `retry_forward` fires the retry `Notify` (see below) |
| error | connecting | supervisor | auto-reconnect next attempt (§3) |
| error | disconnecting | command handler | user `disconnect_forward` while parked in error |

Note the deliberate absences: `disconnecting → error`, `disconnecting → connected`, and
`disconnecting → connecting` are **not** in the table → `set_status` drops them. So a
supervisor status write that loses the race to a user disconnect is silently ignored, and the
tunnel proceeds cleanly to `disconnected`. (The `connecting → disconnecting` row above is
distinct and IS allowed — a user disconnecting a still-connecting tunnel must not be stranded
in `connecting`; F31.)

**v1 toggle semantics (from `forward_provider.dart`) — enforced by the command handler:**
- Clicking disconnect while `connecting` **DISCONNECTS** the tunnel (v1 force-disconnects a
  connecting/reconnecting tunnel — `forward_provider.dart:183-188`). The handler sets
  `connecting → disconnecting`, cancels the in-flight attempt via the parent token (F24 releases
  the bound port fast), awaits the supervisor JoinHandle, then sets `disconnecting → disconnected`
  (F31).
- Clicking while `disconnecting` is **IGNORED** (`disconnect_forward`/`connect_forward` no-op
  when current state is `disconnecting`).
- Clicking while `error` **RETRIES**, it does not disconnect → the UI toggle in `error` maps to
  `retry_forward`, not `disconnect_forward`.

**Terminal `error` PARKS the supervisor — it does NOT exit (F23, consistent with F21).** When
retries exhaust or `autoReconnect=false`, the supervisor sets `error` and parks. **The source
of truth for a pending retry is a lock-guarded flag `retry_requested`, NOT a `Notify` permit
(F29).** A bare-permit scheme has a lost-wakeup hole: `retry_forward` is allowed to fire the
instant `set_status(error)` releases the lock — i.e. *before* the supervisor could drain/park —
so a drain-then-park would swallow a legitimate retry and strand the tunnel in `error`. Use a
flag that is set/checked/cleared inside the SAME critical section as the status:
```rust
// Registry entry holds: retry_requested: bool (guarded by the registry lock),
// and retry_notify: Notify used PURELY as a wakeup (never the source of truth).

// --- supervisor, entering terminal error ---
{
    let mut g = registry.lock();          // one critical section
    set_status(&mut g, id, Error);        // guarded transition (F28)
    // NIT-2: this in-section check is DEFENSIVE only — it is effectively always-false because a
    // retry cannot set the flag before status==error, which happens on the line above inside
    // this same lock. The load-bearing mechanism is the guard (retry_forward requires
    // status==error) + the on-wake re-check below. Keep it for symmetry/robustness, not truth.
    if take_retry_requested(&mut g, id) { // check-and-CLEAR in the same section (defensive)
        continue 'supervisor;             // a retry already arrived → new attempt, don't park
    }
} // lock released
loop {
    tokio::select! {
        _ = parent_cancel.cancelled() => { /* user disconnect/delete → clean up + exit */ }
        _ = retry_notify.notified()   => {
            let mut g = registry.lock();
            if take_retry_requested(&mut g, id) { drop(g); continue 'supervisor; } // re-check+clear (load-bearing)
            // spurious wake (no real request) → keep parking
        }
    }
}

// --- retry_forward(id) command handler ---
let mut g = registry.lock();
if current_status(&g, id) == Error {      // only valid while parked (F27c)
    set_retry_requested(&mut g, id, true);
    mint_fresh_attempt_cancel(&mut g, id); // fresh child token for the next attempt
    drop(g);
    retry_notify.notify_one();             // wakeup only; flag is the truth
}
```
Because the flag is set/checked/cleared under the lock and checked **once more in the same
critical section as `set_status(error)`**, a retry that races the final failure is honored
(either the supervisor sees the flag before parking, or the wakeup + re-check catches it) —
**no drain, no lost wakeup, no stale permit**. On a valid retry the **same** supervisor loops
back to `connecting` with a fresh `attempt_cancel`. **No respawn, no registry swap, JoinHandle
unchanged** (preserves the F21 invariant). Auto-reconnect (§3) uses the same in-loop path.

Cleanup: cancel accept loop, destroy active local sockets, close listener, close SSH session.

### Rust approach
```rust
// ssh/engine.rs
pub struct TunnelHandle {
    pub id: TunnelId,
    // Two-level cancellation hierarchy (F6, see §5):
    pub parent_cancel: CancellationToken, // durable per-tunnel; cancelled only by disconnect/delete
    pub attempt_cancel: CancellationToken, // child_token() of parent; replaced each (re)connect attempt
    pub join: JoinHandle<()>,             // supervisor task; STABLE for the tunnel's whole life (F21/F23)
    // NOTE (F21): NO russh session handle stored here. The supervisor owns the session
    // internally and re-creates it in-task per reconnect attempt.
    pub status: watch::Sender<ForwardStatus>, // written ONLY via GUARDED set_status(id,new) under the registry lock (F23/F28)
    // Retry (F29): the SOURCE OF TRUTH is the lock-guarded flag, not a Notify permit.
    pub retry_requested: bool,            // guarded by the registry lock; set by retry_forward, check-and-cleared by supervisor
    pub retry_notify: Arc<Notify>,        // WAKEUP ONLY (never the truth): retry_forward pokes it after setting the flag (F29)
    // NOTE (F27a): the dead-channel wake `attempt_fail_notify` is NOT here — it is a per-ATTEMPT
    // `Arc<Notify>` the supervisor mints fresh at each attempt start and hands (cloned) to that
    // attempt's child copy tasks; it is dropped at attempt end so no permit can cross an
    // attempt boundary.
    // NOTE (F30): the dead-channel counter is NOT here either — see StatsInner; it is a
    // per-ATTEMPT `Arc<AtomicUsize>` minted with the attempt so straggler children of a dropped
    // attempt cannot contaminate the next attempt's count.
    pub stats_cell: watch::Sender<TunnelStats>, // supervisor publishes latency + derived stats; sampler reads (§6)
    pub stats: Arc<StatsInner>,           // atomics for byte/conn counters (updated by copy tasks)
}

pub async fn connect(state: &AppState, id: &TunnelId) -> Result<(), AppError>;
pub async fn disconnect(state: &AppState, id: &TunnelId, user_initiated: bool) -> Result<(), AppError>;
```

- **Listener bind with retry**:
```rust
async fn bind_local(addr: SocketAddr) -> Result<TcpListener, AppError> {
    for attempt in 0..5 {
        match TcpListener::bind(addr).await {
            Ok(l) => return Ok(l),
            Err(e) if is_addr_in_use(&e) => { sleep(Duration::from_millis(500)).await; }
            Err(e) => return Err(AppError::Bind(e.to_string())),
        }
    }
    Err(AppError::PortInUse(addr.port()))
}
```
  `is_addr_in_use` checks `ErrorKind::AddrInUse` (portable) — do not hardcode errno; the
  errno note above is documentation, `ErrorKind` covers all three OSes.

- **russh client**: implement `russh::client::Handler`. Connect via
  `russh::client::connect(Arc::new(config), (host, port), handler)` wrapped in
  `tokio::time::timeout(Duration::from_secs(15), ...)`. Set `config.keepalive_interval` and
  `config.keepalive_max` from `keepAliveIntervalSec`/`keepAliveMaxCount` (see §2) — this is
  what makes the session future end when the peer goes away (F7).

- **Auth (mutually exclusive)**:
```rust
if let Some(key_path) = &cfg.identity_file_path {
    // load_secret_key is BLOCKING (sync file read) → run on a blocking thread.
    let path = key_path.clone();
    let key = tokio::task::spawn_blocking(move || russh::keys::load_secret_key(&path, None))
        .await
        .map_err(|e| AppError::Ssh(e.to_string()))??; // KeyPair
    // russh 0.45: authenticate_publickey takes Arc<keys::key::KeyPair> directly (F22-corrected).
    let accepted = session.authenticate_publickey(&cfg.ssh_username, Arc::new(key)).await?;
    if !accepted { return Err(AppError::Ssh("publickey auth rejected".into())); }
} else {
    let pw = credentials::get_password(&cfg.id)?; // keychain or fallback
    let accepted = session.authenticate_password(&cfg.ssh_username, pw).await?;
    if !accepted { return Err(AppError::Ssh("password auth rejected".into())); }
}
```
  Wrap the whole auth in `timeout(Duration::from_secs(30), ...)`. The accept loop is simply
  the code that runs after `authenticate_*().await?` returns — russh makes the ordering
  automatic (you cannot open a channel pre-auth).
  - **russh 0.45 API notes (F22 — CORRECTED by the M1 F16 spike):** `authenticate_publickey`
    in 0.45 has signature `authenticate_publickey<U: Into<String>>(&mut self, user: U,
    key: Arc<keys::key::KeyPair>)` — it takes a **bare `Arc<KeyPair>`**, NOT
    `PrivateKeyWithHashAlg` (that type belongs to a later russh version and does not exist in
    0.45). Both `authenticate_*` return `Result<bool>` where `bool == accepted` — **you MUST
    check it** (`false` = credentials rejected). `channel_open_direct_tcpip` takes `Into<String>`
    for the host args — pass an owned `String`/clone (or `&str`), **not** `&String`.

- **Per-attempt setup (F27a/F30):** at the **start of each attempt** the supervisor mints a
  **fresh `attempt_fail_notify = Arc::new(Notify::new())`** AND a **fresh
  `attempt_fail_count = Arc::new(AtomicUsize::new(0))`** (per-attempt, NOT the durable
  `StatsInner` counter — F30). Child copy tasks spawned in this attempt get clones of *this*
  attempt's notify **and** counter. Both are **dropped at attempt end**, so a straggler child
  from a previous attempt that `fetch_add`s late lands on a **dead counter nobody reads** and
  cannot inflate the next attempt's count. (Cumulative lifetime byte/conn stats stay in the
  durable `StatsInner`; only the failure-teardown counter is per-attempt.)

- **Accept loop** (inside the long-lived supervisor task, owning `session` locally): `select!`
  over **five** arms — `attempt_cancel.cancelled()`, `listener.accept()`, the **session-lost
  poll** (F7-corrected: a short `interval` tick that checks `handle.is_closed()`, since the
  session future is a private `join` and cannot be awaited — see Conventions/§2),
  **`attempt_fail_notify.notified()`** (F26 dead-channel WAKE), and the **wake-probe `Notify`
  arm** (NIT-1: the sleep/resume nudge from §4 — `request_wake_probe` pokes this so the
  supervisor runs an immediate RTT probe and reconnects if dead; do NOT omit it when building
  strictly from §1). Handling:
  - **session-lost poll sees `is_closed() == true`** → `set_status(error)`, break to reconnect
    step (§3). (This fires on keepalive timeout / hard drop; it is the F7 signal.)
  - **`attempt_fail_notify` fires** → it is a **WAKE only (F27b)**: re-check the authoritative
    **per-attempt** counter `attempt_fail_count.load() >= 3`. If true → `set_status(error)`,
    break to reconnect; **if < 3 (spurious/stale wake) → ignore and keep serving.**
    (Counter = truth, notify = wake — a stray permit never causes a false teardown.)
  - **wake-probe fires** → run one immediate channel-open RTT probe on the live session; if it
    fails, break to an immediate reconnect (bypass backoff); if it succeeds, keep serving (§4).
  - **`attempt_cancel.cancelled()`** → disambiguate (F27d): `attempt_cancel` is a child of
    `parent_cancel`, so this arm fires for BOTH a user teardown AND the dead-channel
    "cancel the attempt token" option. Branch on **`parent_cancel.is_cancelled()`**: `true` →
    user disconnect/delete → clean up and **exit**; `false` → attempt reset / dead-channel →
    break to the reconnect-eligibility path (do NOT exit).

  Each accepted `(sock, _)` spawns a child task:
```rust
// forward.rs — russh 0.45 API (see Conventions F16)
let ch = timeout(Duration::from_secs(10), session.channel_open_direct_tcpip(
    &cfg.remote_host, cfg.remote_port as u32,
    &local_ip, local_port as u32)).await??;
let mut stream = ch.into_stream(); // AsyncRead + AsyncWrite in russh 0.45
```
  Then bidirectional copy between `sock` and `stream` with `tokio::io::copy`, **wrapping each
  direction in a byte-counting adapter**:
```rust
struct CountingCopy { counter: Arc<AtomicU64> }
// on each copied chunk n: counter.fetch_add(n, Relaxed);
```
  Track up (local→remote) and down (remote→local) separately.

- **Dead-CHANNEL teardown signal path (F26/F27/F30).** On a channel-open timeout or copy error,
  a child task increments **its attempt's own** counter
  `let n = attempt_fail_count.fetch_add(1, SeqCst) + 1;` and, **if `n >= 3`, fires this attempt's
  `attempt_fail_notify`** (the per-attempt WAKE). It **must NEVER cancel the parent token**. The
  supervisor re-reads `attempt_fail_count` on wake (F27b — the counter is authoritative; the
  notify only wakes it), and on `>= 3` drops the current session/attempt, sets `error`, and
  **enters the SAME reconnect-eligibility path as a session drop** (reconnect iff
  `autoReconnect && attempts < max`, §3) — **matching v1** (`ssh_tunnel_service.dart` → error →
  `_tryAutoReconnect`). Because both the counter AND the notify are **per-attempt** (fresh each
  attempt, F27a/F30), a straggler child of a dropped attempt that increments late lands on a
  dead counter/notify no live supervisor reads → a healthy reconnected session **cannot** be
  torn down by a leftover count or stale permit. Dead-*channel* detection is distinct from
  dead-*session* detection (F7, russh keepalive): the former catches a stuck forward while the
  SSH session is still up, but both funnel into the same reconnect path.

- **Conflict handling**: `engine::connect` first calls `disconnect(id, user_initiated=false)`
  if already connected; then scans the registry for any other tunnel whose bound
  `(addr, port)` collides and disconnects it before binding.

- **Cleanup on cancel**: when the **parent** token is cancelled the supervisor's `select!`/
  reconnect loop exits, drops the listener (closes it), aborts child copy tasks (they also
  observe the attempt token), and calls `session.disconnect(...).await` / drops its in-task
  session. `disconnect` then awaits the supervisor's stable `JoinHandle` before removing the
  registry entry (F21) — the port is released before the entry is gone.

### Acceptance criteria
- [ ] Connecting binds locally, authenticates (password *or* key), and forwards traffic to
      the remote target; a client connecting to `localBindAddress:localPort` reaches
      `remoteHost:remotePort`.
- [ ] `EADDRINUSE` on bind retries 5× @500ms then errors with `PortInUse` (this subsumes v1's
      port-release wait — F25).
- [ ] Connect timeout (15s), auth timeout (30s), channel-open timeout (10s) enforced.
- [ ] **disconnect/delete during `connecting` (both mid-connect and mid-auth) tears down and
      releases the local port within a small bound** (cancel latency, not the 15–30s timeouts)
      (F24), **and the status actually reaches `disconnected`** — the guard allows
      `connecting → disconnecting → disconnected` and a `tunnel://status=disconnected` event is
      emitted (F31); the UI is never stranded spinning in `connecting`.
- [ ] **The status channel never has two concurrent writers** — every transition goes through
      `set_status` under the registry lock; supervisor owns connecting/connected/error, command
      handler owns disconnecting/disconnected (F23).
- [ ] **`set_status` enforces the table (F28):** a session drop coincident with a user
      disconnect never produces `disconnecting → error` — the illegal transition is dropped and
      the tunnel proceeds `disconnecting → disconnected` (unit-test the guarded function
      directly over all `(current, new)` pairs).
- [ ] **No stale `Notify` permit crosses an attempt boundary (F27):** a fresh
      `attempt_fail_notify` + fresh per-attempt `attempt_fail_count` are minted at each attempt
      start; a healthy new session after reconnect never sees a false dead-channel teardown.
      Test: force 3 forward failures, reconnect, then run clean traffic — no spurious teardown.
- [ ] **Forward-fail notify is a wake, not truth (F27b):** a spurious `attempt_fail_notify`
      fire with the per-attempt `attempt_fail_count < 3` is a no-op (tunnel keeps serving).
- [ ] **A retry fired at the instant of the final failure is honored, never lost (F29):**
      `retry_requested` is a lock-guarded flag set by `retry_forward` and check-and-cleared by
      the supervisor in the SAME critical section as `set_status(error)` (and again on wake); a
      retry racing the last auto-reconnect failure leaves `error` without needing a second click.
- [ ] **`retry_forward` while NOT parked in `error` is a no-op (F27c):** it only acts when
      status==`error`; a retry fired mid-`connecting`/backoff neither sets the flag nor wakes a
      future park.
- [ ] **Straggler failures from a dropped attempt cannot trip a teardown on the next attempt
      (F30):** the failure counter is per-attempt (`Arc<AtomicUsize>` dropped at attempt end), so
      a late `fetch_add` from an old child lands on a dead counter no live supervisor reads.
- [ ] **Retry from `error` reuses the same supervisor and JoinHandle (no respawn, no registry
      swap)** — the parked supervisor wakes on `retry_notify` and loops to `connecting` (F23).
- [ ] Clicking while `disconnecting` is ignored; clicking while `error` retries (not disconnect)
      — v1 toggle semantics enforced by the command handler (F23).
- [ ] **3 consecutive forward-*channel* failures tear the tunnel down into `error` and enter
      the reconnect-eligibility path** (reconnect iff `autoReconnect && attempts<max`); it does
      NOT cancel the parent / permanently kill the tunnel (F26, matches v1).
- [ ] When the russh **session future ends** (peer missed `keepalive_max` keepalives, or hard
      drop), the supervisor sets status `error` and reconnects within its own loop (F7) —
      verified by killing the sshd / cutting the network.
- [ ] The supervisor `JoinHandle` is **stable across reconnects** (its identity never changes);
      no russh session handle is stored in the registry (F21).
- [ ] **Latency keeps updating across a reconnect** — after a drop+recover, the stats cell
      shows fresh RTT (no stale/frozen latency from a dead session clone) (F21/§6).
- [ ] `disconnect` cancels the parent token and **awaits the supervisor JoinHandle before
      removing the registry entry**, so the local port is released before the entry disappears
      (F21) — verified by an immediate reconnect on the same port not hitting `EADDRINUSE`.
- [ ] Connecting an already-connected config re-connects cleanly; a second config on the
      same local port disconnects the first.
- [ ] Byte counters increment correctly in both directions (assert against known payload sizes).
- [ ] When both password and identity are set, **identity file is used** (precedence), matching
      v1 — phrased identically in [04 §1](04-DATA-MODEL.md).

---

<a id="keepalive"></a>
## 2. Keep-alive / liveness (`ssh/client.rs`, `ssh/health.rs`)

> **F1 — CRITICAL correction.** russh has **no `client.ping()` and no `is_closed()`** (those
> were dartssh2 APIs). v1's "count N app-level ping failures, then teardown" model does **not
> port**. Liveness/teardown is owned by russh's own keepalive; the 3s app timer is now a
> **stats/latency sampler only**, not a liveness authority. Do not implement a second
> teardown path — that would handle liveness twice.

### Behavior (v2 design)
- **Primary liveness = russh protocol keepalive (the teardown authority).** Configure
  `client::Config.keepalive_interval` + `keepalive_max`. russh sends keepalives on the
  interval; when the peer misses `keepalive_max` in a row, the **session future ends** — the
  supervisor picks that up (F7, §1) as "connection lost → status `error` → maybe reconnect."
  This single mechanism replaces both v1's SSH-protocol keepalive *and* v1's app-level
  ping-failure teardown.
  - `keepAliveIntervalSec`: default 30; **0 → 10** (faster VPN-death detection).
  - `keepAliveMaxCount`: default 5; **0 → 3**. Maps directly to `keepalive_max`.
- **Secondary = 3s stats path (stats + latency only, NO teardown).** Split by ownership (F21):
  the **supervisor** (owning the live session) runs the **latency channel-open RTT probe** on a
  3s interval and publishes latency + a fresh `TunnelStats` snapshot into its `stats_cell`
  (`watch`); the **single shared sampler task in `health.rs`** does NOT touch any session — it
  reads every tunnel's `stats_cell` on its 3s tick and emits `tunnel://stats`. Neither decides
  teardown. The shared sampler auto-starts on first connect, auto-stops when no connections
  remain.
- **Dead-*channel* teardown (distinct trigger, SAME recovery, F26)** stays as §1: 3 consecutive
  forward-channel failures tear down the tunnel even while the SSH session is otherwise alive.
  The child increments the per-attempt `attempt_fail_count` and fires the per-attempt
  `attempt_fail_notify` (a WAKE; the supervisor re-checks the authoritative per-attempt
  `attempt_fail_count >= 3`, F27b/F30) — **never the parent** — and the supervisor funnels it
  into the **same reconnect-eligibility path** as a session drop (reconnect iff
  `autoReconnect && attempts<max`), matching v1. It does not permanently kill the tunnel.

### Rust approach
- **Keepalive config** (the liveness mechanism):
```rust
let mut config = russh::client::Config::default();
config.keepalive_interval = Some(Duration::from_secs(
    if cfg.keep_alive_interval_sec == 0 { 10 } else { cfg.keep_alive_interval_sec as u64 }));
config.keepalive_max = if cfg.keep_alive_max_count == 0 { 3 } else { cfg.keep_alive_max_count as usize };
```
  Verify field names against pinned russh 0.45 (Conventions F16).
- **Latency probe in the supervisor** (owns the live session, F21): add a 3s `interval` arm to
  the supervisor `select!`; on tick, run the RTT probe on its own session and publish into the
  cell:
```rust
// inside the supervisor task (owns `session`):
let t0 = Instant::now();
if timeout(Duration::from_secs(3), async {
    let ch = session.channel_open_session().await?; ch.close().await // RTT (no ping() exists)
}).await.is_ok() {
    stats.set_latency(t0.elapsed());
}
stats_cell.send_replace(build_snapshot(&stats)); // watch<TunnelStats>
```
- **Shared 3s emit sampler** in `health.rs` — reads cells, emits, holds **no session**, never
  tears down:
```rust
pub fn ensure_sampler(state: AppState) { /* idempotent: start once on first connect */ }
async fn sampler_loop(state: AppState) {
    let mut tick = interval(Duration::from_secs(3));
    loop {
        tick.tick().await;
        let tunnels = state.registry.connected_snapshot();
        if tunnels.is_empty() { break; }                 // auto-stop
        for t in tunnels {
            let snapshot = t.stats_cell.borrow().clone(); // read cell; NO session access
            emit_stats(&state, &t.id, snapshot);          // emit tunnel://stats
        }
    }
}
```
  Teardown is NOT here — it is driven by the session future ending (F7) and by the
  forward-failure path (§1).

### Acceptance criteria
- [ ] `keepalive_interval`/`keepalive_max` applied per-config; interval `0`→10s, max `0`→3.
- [ ] Cutting the peer (kill sshd / drop network) ends the russh session future within
      ~`interval × max`; the supervisor moves the tunnel to `error` (verified in §1/§7 tests) —
      **no app-level ping counter involved.**
- [ ] Exactly **one** shared sampler task exists regardless of tunnel count; starts on first
      connect, stops when the last tunnel disconnects.
- [ ] The shared sampler holds no russh session and never calls `disconnect`/teardown.
- [ ] Latency is produced by the supervisor's channel-open RTT probe (no `ping()`) and read by
      the sampler from the `stats_cell`; **it keeps updating across reconnects** (F21).

---

<a id="reconnect"></a>
## 3. Auto-reconnect / backoff (`ssh/reconnect.rs`)

### Behavior (replicate v1)
Exponential backoff `delaySec * 2^attempts`, **clamped 1–60s**. Stop after
`autoReconnectMaxRetries`. **Skip** if the user manually disconnected or the config was
removed mid-flight. Only when `autoReconnect` setting is on.

### Rust approach
- **Reconnect is the supervisor's own loop, not a separate task (F21).** The single
  long-lived supervisor task loops: (establish session → accept loop) → on session-future end
  with `user_initiated == false` → backoff+reconnect in the *same* task. So there is no
  spawn/await of a distinct reconnect task and the `JoinHandle` stays stable.
- **Trigger (F7/F26):** the reconnect step is entered on **either** the **russh session future
  ending** (F7) **or** the **dead-channel signal** (per-attempt `attempt_fail_notify` woke the
  supervisor AND the per-attempt `attempt_fail_count >= 3`, F26/F27b/F30) — both are non-user
  teardowns and funnel into the same path. When the supervisor observes either and
  `settings.auto_reconnect` is on, it enters the backoff+reconnect step of its loop.
- **Per-attempt reset (F27a/F30):** at the top of each attempt the supervisor mints a fresh
  `attempt_cancel = parent.child_token()`, a fresh `attempt_fail_notify`, and a **fresh
  `attempt_fail_count` (`Arc<AtomicUsize>`)** — the failure counter is per-attempt, not the
  durable `StatsInner`. The **parent token is NOT touched** — a user `disconnect`/`delete`
  cancels the *parent* and ends the whole supervisor loop; a reconnect swapping its own
  child token/notify/counter never disturbs the durable per-tunnel lifetime and never inherits
  a stale permit or straggler count. See §5.
```rust
fn backoff(delay_sec: u32, attempt: u32) -> Duration {
    let secs = (delay_sec as u64) * 2u64.pow(attempt);
    Duration::from_secs(secs.clamp(1, 60))
}
```
- Within the loop, `attempt` counts up to `max_retries`: `select!` on
  `parent_cancel.cancelled()` vs `sleep(backoff(delay, attempt))`, then re-establish the
  session. Break to steady state on success.
- **On terminal exhaustion (or `autoReconnect=false`): set `error` and PARK — do NOT exit
  (F23).** The pending-retry truth is the **lock-guarded `retry_requested` flag, not a permit
  (F29)**: in the SAME critical section as `set_status(error)`, check-and-clear
  `retry_requested` — if already set, loop straight into a new attempt instead of parking.
  Otherwise `select!` on `parent_cancel.cancelled()` (user teardown → clean up + exit) vs
  `retry_notify.notified()` (a wakeup; on wake re-check-and-clear `retry_requested` under the
  lock before acting). `retry_forward` only acts when status==`error` (F27c). This keeps the
  JoinHandle stable (F21) with **no drain, no lost wakeup**.
- A user-initiated `disconnect` or `delete_forward` cancels the **parent** token → the
  supervisor's `select!` (whether backing off or parked in error) fires immediately → no
  further attempts.

### Acceptance criteria
- [ ] Backoff sequence for `delay=5`: 5,10,20,40,60(clamped),60… and never <1 or >60.
      Unit-test `backoff()` directly.
- [ ] Reconnect is triggered by the session future ending (F7) **or** the dead-channel signal
      (F26), not by a ping counter.
- [ ] Reconnect stops after `autoReconnectMaxRetries`, sets `error`, and **parks (task alive,
      JoinHandle stable)** — it does not exit (F23).
- [ ] Manual disconnect **during a backoff wait OR while parked in `error`** cancels/ends
      immediately (parent-token cancellation wins the `select!`).
- [ ] `retry_forward` while parked in `error` reuses the same supervisor (no respawn) (F23).
- [ ] **A retry racing the final failure is honored, never lost (F29):** the `retry_requested`
      flag is check-and-cleared in the same critical section as `set_status(error)` (and again on
      wake), so a retry fired the instant the last attempt fails leaves `error` without a second
      click.
- [ ] **A `retry_forward` fired while NOT parked does not auto-trigger the next park (F27c):** it
      only acts when status==`error`; rapid-toggle test — fire retry during `connecting`/backoff,
      confirm the eventual `error` park waits for a genuine new retry.
- [ ] Deleting a config mid-reconnect aborts without panicking or leaking a task.
- [ ] `autoReconnect=false` → no reconnect attempted; supervisor parks in `error` awaiting
      retry/teardown.

---

<a id="wake"></a>
## 4. Wake-from-sleep detection (`ssh/wake.rs`)

### Behavior (replicate v1)
On app resume after **>30s** of inactivity, probe all `connected` tunnels and
**force-reconnect** the dead ones (bypassing backoff).

### Rust approach
- Watchdog task: `interval(5s)`; record `last_tick = Instant::now()`. If the observed gap
  between ticks exceeds **30s**, infer sleep/resume and **nudge each supervisor to probe now**
  — the wake task does NOT hold a session (F21), so it signals the supervisor (which owns the
  live session) to run an immediate RTT probe and, if it fails, break to an immediate reconnect:
```rust
let gap = now.duration_since(last_tick);
if gap > Duration::from_secs(30) {
    for t in state.registry.connected_snapshot() {
        engine::request_wake_probe(&state, &t.id); // supervisor probes its own session; reconnects now if dead
    }
}
last_tick = now;
```
  `request_wake_probe` sends a lightweight nudge (e.g. on a per-tunnel `Notify`/channel that
  the supervisor's `select!` listens on); the supervisor performs the §6 channel-open RTT
  probe on its own session and, on failure, breaks its accept loop straight into reconnect
  (skipping the backoff wait).
- The intent is OS-agnostic, but **do not assume it is guaranteed (F15)**: whether
  `Instant`/monotonic clocks advance across system sleep, and whether a tokio timer fires
  promptly on resume, is **platform- and configuration-dependent** (e.g. macOS App Nap /
  timer coalescing, suspended runtimes). There is no `ping()` — probing is the supervisor's
  channel-open RTT — and recovery ultimately leans on the session-future signal (F7) as the
  real backstop even if the wake heuristic misses.

### Acceptance criteria
- [ ] A simulated clock gap >30s triggers a liveness sweep of connected tunnels.
- [ ] Dead tunnels reconnect immediately (no backoff wait) on wake; live ones are untouched.
- [ ] Gaps ≤30s do nothing.
- [ ] **Verify across REAL system sleep on each OS** (macOS/Windows/Linux): suspend the
      machine >30s, resume, confirm dead tunnels recover. Document any OS where the monotonic
      gap heuristic does not fire — in that case recovery still occurs via the session-future
      signal (F7), just not instantly.

---

<a id="concurrency"></a>
## 5. Concurrency & cancellation model (replaces generation-token guard)

### Behavior (replicate v1 intent)
v1 bumped a per-config **generation counter** on every connect/disconnect; stale async
callbacks checked the token before firing. **In Rust use real synchronization**, not a counter.

### Rust approach — TWO-LEVEL token hierarchy (F6)
A single flat token cannot distinguish "the user is done with this tunnel" from "this
reconnect attempt is obsolete." Use two levels:

- **Parent token (`parent_cancel`)** — durable, one per `TunnelHandle`, created when the
  tunnel is first started and cancelled **only** by `disconnect` (user), `delete_forward`, or
  app quit. Cancelling it kills the supervisor and **every** child (accept loop, all copy
  tasks, any in-flight reconnect).
- **Attempt tokens (`attempt_cancel = parent_cancel.child_token()`)** — one per
  connect/reconnect *attempt*. All per-attempt work (this attempt's accept loop + copies)
  observes the *attempt* token. A **reconnect replaces only the child token** (new
  `child_token()`), leaving the parent — and thus the tunnel's identity/lifetime — intact.
  A `retry` (status=error) is the same: new child token, parent untouched.

This resolves the disconnect-during-backoff race: the reconnect loop `select!`s on
`parent_cancel.cancelled()`, so a user disconnect cancels the parent and the reconnect (a
child) dies immediately; meanwhile normal reconnect churn only swaps children.

- `disconnect`/`delete` **cancel the parent token and await the supervisory `JoinHandle`** so
  cleanup is deterministic (listener closed, sockets dropped, SSH session closed) before the
  handle is removed from the registry. Eliminates stale-callback races structurally.
- Registry mutations guarded by a `tokio::sync::Mutex`/`RwLock` held only briefly; never hold
  a lock across an `.await` doing network I/O.
- `connect`/`disconnect` while `disconnecting` is **ignored** (state check under lock, F23).

- **Cancellation-aware connect+auth (F24):** the bind→connect→auth phase is sequential awaits
  that do NOT observe a token by themselves, so wrap it in `attempt_cancel.run_until_cancelled(
  do_connect_auth())` (a child of the parent). On cancel, drop the bound listener and abort the
  in-flight russh connect immediately, then return — so a teardown during `connecting` is bounded
  by cancel latency, not the 15–30s timeouts. Detailed in §1.

- **Single GUARDED status writer (F23/F28):** `status` (`watch`) is written only via
  `set_status(id, new)` under the registry lock, which **enforces the transition table and
  no-ops any illegal `(current, new)` pair**. The lock makes writes non-concurrent; the guard
  makes them order-valid — e.g. a supervisor `error` write that loses the race after the command
  handler set `disconnecting` is dropped, so no `disconnecting → error` flash. Supervisor writes
  `connecting`/`connected`/`error`; command handler writes `disconnecting`/`disconnected`
  (it outlives the task). See the transition table in §1.

- **Retry reuses the supervisor, flag-based & lost-wakeup-free (F23/F29):** the pending-retry
  truth is the **lock-guarded `retry_requested` flag**, NOT a `Notify` permit. On terminal
  `error` the supervisor, **in the same critical section as `set_status(error)`**, checks-and-
  clears `retry_requested` (set → new attempt immediately; unset → park). `retry_forward` acts
  **only when status==`error`** (F27c): under the lock it sets the flag, mints a fresh
  `attempt_cancel`, and pokes `retry_notify` **as a wakeup only**; the parked supervisor
  re-checks-and-clears the flag under the lock on wake. Same task/JoinHandle loops back — no
  respawn, no registry swap (F21) — and a retry racing the final failure is never lost (no
  drain, no permit-ordering hazard).

- **Per-attempt failure hygiene (F27a/F30):** BOTH the dead-channel wake `attempt_fail_notify`
  AND the failure counter `attempt_fail_count` (`Arc<AtomicUsize>`) are minted fresh per attempt
  and dropped at attempt end — neither a stale permit nor a straggler increment crosses an
  attempt boundary. The wake is advisory; teardown is gated on the authoritative **per-attempt**
  `attempt_fail_count >= 3` (F27b).

### Acceptance criteria
- [ ] Rapid connect→disconnect→connect cycles never leak tasks (assert task count / no
      lingering listeners) and never double-bind a port.
- [ ] No stale callback mutates a tunnel after its (parent or attempt) token is cancelled.
- [ ] A reconnect attempt swaps only the child token; the parent survives across attempts.
- [ ] A user disconnect during backoff (or while parked in `error`) cancels the parent → the
      supervisor ends immediately.
- [ ] Teardown during `connecting` (mid-connect/mid-auth) is bounded by cancel latency, not the
      15–30s timeouts (F24).
- [ ] `status` never has two concurrent writers; all writes go through `set_status` under the
      lock (F23), which **enforces the table and no-ops illegal transitions** — a drop coincident
      with a user disconnect never yields `disconnecting → error` (F28).
- [ ] Clicking during `disconnecting` is a no-op; clicking during `error` retries (reusing the
      same supervisor, no respawn) (F23).
- [ ] No stale `Notify` permit or straggler count crosses an attempt boundary — both
      `attempt_fail_notify` and `attempt_fail_count` are per-attempt (F27a/F30); no spurious
      teardown flapping.
- [ ] Retry is flag-based (`retry_requested`), not permit-based — a retry racing the final
      failure is honored, never lost (F29).
- [ ] `disconnect` awaits full cleanup before returning `Ok`.

---

<a id="stats"></a>
## 6. Per-tunnel stats & byte counting (`ssh/stats.rs`)

### Behavior
Track: `activeConnections`, cumulative `totalBytesUp`/`totalBytesDown`, `lastPingLatency`,
`connectedSince` (→ derived `uptime`). **Single cadence = every 3s (F4)**, tied to the one
sampler tick (§2) — there is no second sampler. Shown as chips under connected rows
([05-UI-UX-SPEC.md](05-UI-UX-SPEC.md) aligns to the same 3s).

### Rust approach — ownership split (F21)
```rust
// DURABLE per-tunnel byte/conn counters in Arc<StatsInner> (atomics), written by copy/accept tasks:
pub struct StatsInner {
    pub active_connections: AtomicUsize,
    pub bytes_up: AtomicU64,
    pub bytes_down: AtomicU64,
    pub last_latency_ms: AtomicU64,        // 0 = none; written by the SUPERVISOR's RTT probe
    pub connected_since: Mutex<Option<Instant>>, // set on connect, cleared on disconnect
    // NOTE (F30): the dead-channel teardown counter is NOT here — it is a PER-ATTEMPT
    // `Arc<AtomicUsize>` (`attempt_fail_count`) minted with each attempt (§1), so a straggler
    // child of a dropped attempt cannot contaminate the next attempt's count.
    // NOTE (F1): no `ping_failures` field — liveness/teardown is owned by russh keepalive (§2).
}
// The SUPERVISOR publishes a rolled-up snapshot into a watch cell that the sampler reads:
//   stats_cell: watch::Sender<TunnelStats>   (in TunnelHandle, §1)
```
- `active_connections` inc/dec around each local-connection child task.
- Byte counters updated inside the counting copy adapters (§1).
- **Latency via channel-open probe, done by the SUPERVISOR (F1/F21):** there is no protocol
  `ping()` on the russh handle. The supervisor — which owns the live session — times
  `channel_open_session()` + close = RTT on its 3s interval, writes it to `last_latency_ms`,
  and `send_replace`s a fresh `TunnelStats` into `stats_cell`. A failed probe leaves latency
  unchanged and is **not** a teardown signal. **Do NOT keep a session clone in the sampler** —
  a clone would go stale after a reconnect and freeze latency.
- The shared sampler (§2) **reads `stats_cell`** and emits `tunnel://stats`; it never accesses
  a session. Uptime derived from `connected_since` at render time.

### Acceptance criteria
- [ ] `activeConnections` reflects live local sockets (opens increment, closes decrement, never negative).
- [ ] Byte counters match transferred bytes within test tolerance.
- [ ] `connectedSince` set on connect, cleared on disconnect; uptime monotonic while connected.
- [ ] Latency comes from the supervisor's channel-open-session RTT probe; a failed probe leaves
      latency unchanged and does NOT tear down the tunnel.
- [ ] **Latency continues to update after a drop+reconnect** (the supervisor re-probes on the
      same `stats_cell`; no stale session clone in the sampler) (F21).
- [ ] `tunnel://stats` emitted on the single 3s cadence per connected tunnel; the sampler reads
      the cell, not a session.

---

<a id="persistence"></a>
## 7. Persistence (`storage/config_file.rs`)

### Behavior (replicate v1)
Single JSON `tunnel_pilot_config.json` in app-support dir. Shape
`{ forwards: [...], settings: {...} }` (+ v2 `schemaVersion`, `groups`). `forwards` and
`settings` saved **independently** via read-merge-write of the full file. Corrupted JSON on
load → copy to `.corrupted`, start fresh.

### Rust approach
- **Canonical v2 dir (F2 — pick ONE and use it everywhere):** use Tauri
  **`PathResolver::app_config_dir()`** as the single source for the config file. `app_config_dir`
  and `app_data_dir` resolve differently on Linux/Windows, so mixing them would split state —
  do not use both. Filename `tunnel_pilot_config.json`. The keychain-fallback secrets file
  ([§9](#credentials), [04 §10](04-DATA-MODEL.md)) lives in the **same** `app_config_dir`.
  - Note: this v2 dir is `<v2-bundle-identifier>/` and will **NOT** coincide with the v1
    config location on macOS (v1 bundle id `com.kalfian.tunnelpilot`, no underscore) or Windows
    (v1 used `%APPDATA%\kalfian\Tunnel Pilot\`). Cross-version pickup is handled by the explicit
    per-OS v1 probe in migration ([04 §12](04-DATA-MODEL.md)), NOT by Tauri dir resolution.
- **Atomic write**: write to `tunnel_pilot_config.json.tmp`, `fsync`, then rename over the
  target (rename is atomic on all three OSes for same-dir).
- **Read-merge-write**: load full file → mutate one section (forwards | settings | groups)
  → write full file. Guard with an async `Mutex` so concurrent saves serialize.
- **Corruption**: on parse error, copy the bad file to
  `tunnel_pilot_config.json.corrupted-<timestamp>`, log an `error` LogEntry, and initialize
  defaults. Never crash on bad config.
- Schema: top-level `schemaVersion` (v2 = `2`). See migration (§8 / [04](04-DATA-MODEL.md)).

### Acceptance criteria
- [ ] Save is atomic — a killed process mid-write never leaves a truncated config (tmp+rename).
- [ ] Mutating settings does not drop forwards/groups and vice versa (merge preserves siblings).
- [ ] Corrupted JSON → `.corrupted-*` sidecar written, app starts with defaults, error logged.
- [ ] Concurrent saves serialize (no interleaved writes).

---

## 8. v1→v2 migration (`storage/migration.rs`)
**Full detail, the verified per-OS v1 paths, and the hardcoded-path probe are in
[04 §12](04-DATA-MODEL.md).** Summary:
- **Do NOT rely on Tauri dir resolution to find the v1 config** — the v1 macOS bundle id
  (`com.kalfian.tunnelpilot`, no underscore) and Windows two-level path
  (`%APPDATA%\kalfian\Tunnel Pilot\`) do not match the v2 `app_config_dir` folder. Probe the
  hardcoded v1 path per OS (F2).
- If the v2 location has no config and a v1 file is found → import `forwards`+`settings`,
  assign `schemaVersion: 2`, `groupId=null`/`tags=[]`, and write to the v2 path.
- **Passwords**: v1 stored plaintext `sshPassword` in the config. Move each into the keychain
  (`credentials::set_password`), set `hasStoredPassword`, and never carry the plaintext into
  the v2 file. If keychain unavailable → fallback secrets file + warning flag.
- **Linux**: v1 never shipped → no probe, fresh install only (F17).
- Write the migrated file atomically; keep a `.v1-backup` copy.

### Acceptance criteria
- [ ] A real v1 `tunnel_pilot_config.json` loads without data loss (names, hosts, ports,
      keepalive, identity paths, settings).
- [ ] v1 plaintext passwords end up in keychain (or fallback + warning), never left in the
      v2 config when keychain works.
- [ ] `.v1-backup` written; migration is idempotent (re-run is a no-op once `schemaVersion==2`).

---

<a id="credentials"></a>
## 9. Keychain + fallback credential store (`credentials/mod.rs`)

### Behavior (locked decision)
Try OS keychain (macOS Keychain / Windows Credential Manager / Linux Secret Service) via
the `keyring` crate. If unavailable (e.g. Linux headless), fall back to plaintext in the
config file **with a visible UI warning**. Config stores a `hasStoredPassword` flag /
reference, not the secret, when keychain is used.

### `keyring` crate — REQUIRED per-target features (F9)
`keyring` v3 ships **no backend by default** — with no features, `keychain_available()` is
always false and every password silently goes to the plaintext fallback. Pin per-target
features in `Cargo.toml`:
```toml
[target.'cfg(target_os = "macos")'.dependencies]
keyring = { version = "3", features = ["apple-native"] }

[target.'cfg(target_os = "windows")'.dependencies]
keyring = { version = "3", features = ["windows-native"] }

[target.'cfg(target_os = "linux")'.dependencies]
# Secret Service backend (needs D-Bus at runtime; headless → falls back, handled below)
keyring = { version = "3", features = ["sync-secret-service", "crypto-rust"] }
```
Linux Secret Service needs a running D-Bus/keyring daemon; on headless Linux
`keychain_available()` returns false and we use the fallback file (already handled).

### Rust approach
```rust
// Deliberate, stable service string. It need NOT match any bundle id — keep it constant
// forever so keychain entries survive bundle-id changes. Account = forward id (uuid).
const KC_SERVICE: &str = "tunnel-pilot"; // do not change once shipped
pub fn set_password(id: &str, pw: &str) -> Result<(), AppError>;
pub fn get_password(id: &str) -> Result<Option<String>, AppError>; // keychain first, else fallback
pub fn delete_password(id: &str) -> Result<(), AppError>;
pub fn keychain_available() -> bool; // feature-detect once at boot (cached)
```
- `keychain_available()`: attempt a probe write+read+delete on a sentinel account at boot;
  cache the result. Exposed via `app_hydrate` → `keychain_available` so UI shows the warning.
- When keychain works: config's forward has `hasStoredPassword: true`, no secret in JSON.
- When it doesn't: secret stored in a **separate fallback secrets file** (not the main
  config, to keep backup-strip logic clean) — see [04](04-DATA-MODEL.md#keychain) —
  `has_stored_password: true` still set. Warning flag surfaced in UI.
- **Never** log or emit passwords over IPC. `copy_ssh_command` does not embed the password.

### Acceptance criteria
- [ ] With keychain available: password saved via `set_forward_password` is retrievable for
      auth; main config JSON contains **no** plaintext secret, only `hasStoredPassword`.
- [ ] With keychain unavailable (simulate): password stored in fallback secrets file;
      `keychain_available=false` propagated to UI for the warning.
- [ ] Deleting a forward deletes its keychain/fallback secret.
- [ ] Passwords never appear in logs, events, backups, or `copy_ssh_command` output.

---

<a id="tray"></a>
## 10. Tray (`tray/icon.rs`, `tray/menu.rs`)

### Behavior (replicate v1)
Dynamic icon: grey idle; blue badge with connected count **1–9** (clamped at 9). macOS
icons are **template images** (auto light/dark tint). Menu rebuilds on **every** state
change; per-tunnel rows show status icon + "Retry" on error; conditional bulk Start/Stop
All; update-available notice injected at top.

### Rust approach
- Preload icon assets (idle grey + numbered 1–9) as `tauri::image::Image`. On macOS set
  the tray icon as template (`set_icon_as_template(true)`).
- `update_tray_icon(count)`: `let n = count.clamp(0,9);` choose idle if 0 else badge[n].
- `rebuild_menu(state)`: construct a `Menu` with:
  - Update-available item at top (if `UpdateStatus.available`) → triggers `install_update`.
  - Per-tunnel submenu/rows: status glyph + name; on `error` show a "Retry" item → `retry_forward`.
  - Conditional "Start All"/"Stop All" (and per-group in the palette/window; tray keeps
    global bulk) shown based on whether any are disconnected/connected.
  - "Open" → `show_window`; "Quit" → `quit_app`.
- Menu building must run on the main thread via `AppHandle`. Coalesce rapid rebuilds (debounce
  ~100ms) to avoid thrash during bulk operations.

### Acceptance criteria
- [ ] Icon shows grey at 0, correct badge 1–9, and clamps to 9 at ≥9 connected.
- [ ] macOS icon is a template image (tints correctly in light/dark menu bar).
- [ ] Menu reflects current statuses; `error` rows expose Retry.
- [ ] Bulk Start/Stop items appear only when applicable.
- [ ] Update notice appears at top when an update is available and launches install.

---

<a id="single-instance"></a>
## 11. Single instance (`window/mod.rs`)

### Behavior (replicate v1, unified)
v1 had two OS-specific mechanisms; v2 uses `tauri-plugin-single-instance` cross-platform.
Second launch re-shows the window.

### Rust approach
- Register the plugin **first** in the builder. Its callback runs in the already-running
  instance: call `show_window()` + focus + emit `window://focus`.

### Acceptance criteria
- [ ] Launching a second copy does not spawn a second process; the existing window shows/focuses.

---

<a id="autostart"></a>
## 12. Autostart (`platform/autostart.rs`)

### Behavior (replicate v1)
Launch at startup; setting re-synced with the OS on every launch.

### Rust approach
- `tauri-plugin-autostart` (configure launch args to start hidden). On boot, read
  `settings.launch_at_login` and reconcile with the plugin's `is_enabled()` — enable/disable
  to match. `update_settings` applies changes immediately.

### Acceptance criteria
- [ ] Toggling `launchAtLogin` enables/disables OS autostart immediately.
- [ ] On boot, OS autostart state is reconciled to the setting (drift corrected).
- [ ] Autostarted launches open hidden (tray only).

---

<a id="dock"></a>
## 13. Dock / taskbar visibility (`platform/dock.rs`)

### Behavior (replicate v1)
`showInDock` setting. Window opens → show in dock **iff** `showInDock`; window closes →
**always** hide from dock. macOS switches activation policy; Windows/Linux use `skipTaskbar`.

### Rust approach
- macOS: use the **Tauri v2 built-in API — `app_handle.set_activation_policy(...)`** with
  `tauri::ActivationPolicy::Regular` (show in dock) / `ActivationPolicy::Accessory` (hide).
  **Do NOT hand-roll objc/cocoa FFI** — Tauri v2 exposes this natively. `LSUIElement=true` in
  Info.plist is the baseline (agent app).
- Windows/Linux: `window.set_skip_taskbar(true/false)`.
- Hook into `show_window` (apply per `showInDock`) and the hide-on-close path (always hide).

### Acceptance criteria
- [ ] `showInDock=true`: opening the window shows a Dock/taskbar entry; closing hides it.
- [ ] `showInDock=false`: no Dock/taskbar entry even when window is open.
- [ ] Changing the setting while the window is open applies immediately.

---

<a id="window"></a>
## 14. Window hide-on-close (`window/mod.rs`)

### Behavior (replicate v1)
Closing the window hides it and keeps the app alive in the tray (intercept close). App does
not quit on window close. App starts hidden after first frame.

### Rust approach
- Listen for `WindowEvent::CloseRequested` → `api.prevent_close()`, then `hide_window()`
  (hide + dock-hide + `set_skip_taskbar(true)`).
- Startup: window `visible:false` in config; optionally show only if not autostarted / on
  first-run — but default is hidden → tray.
- Only `quit_app` (tray/palette) actually exits.

### Window chrome (per-OS, resolved)
- **macOS**: **custom transparent titlebar with traffic lights** — set
  `titleBarStyle: "Overlay"` (or transparent titlebar) so the window content extends under
  the title bar while the native traffic-light buttons remain visible/functional. The
  frontend must leave a drag region / top inset for the traffic lights (design agent —
  [05-UI-UX-SPEC.md](05-UI-UX-SPEC.md)).
- **Windows / Linux**: **native OS decorations** (`decorations: true`) — standard title bar
  and window controls. No custom chrome.
- Configure this per-target in `tauri.conf.json` (macOS window overrides vs default
  decorations). The hide-on-close intercept applies identically on all OSes.

### Acceptance criteria
- [ ] Clicking OS close button hides the window; process keeps running (tray persists).
- [ ] App launches hidden (no window flash) with tray present.
- [ ] Quit from tray/palette fully exits and cleans up all tunnels.
- [ ] macOS shows a transparent titlebar with functional traffic lights; content insets correctly.
- [ ] Windows/Linux show native window decorations.

---

<a id="notifications"></a>
## 15. Notifications (`platform/notify.rs`)

### Behavior (replicate v1)
Notify on connect/disconnect/error for **unexpected** states only — user-initiated
disconnects are **silent**. Update-available notified **once per version**. Only when
`showNotifications`.

### Rust approach
- `tauri-plugin-notification`, unified across OS.
- Notification decision lives at the status-transition source: pass `user_initiated` through
  `disconnect`; suppress the notification when `true`.
- Track `notified_update_version` to fire update notice once per version (persist alongside
  `lastSkippedVersion` semantics).
- **macOS permission timing**: request permission at a deliberate moment — on the first
  event that would notify (or when the user enables notifications in Settings), not blindly
  at startup (avoids a permission prompt race before the app is interactive).
- **F5 — unsigned macOS caveat**: macOS `UNUserNotificationCenter` generally requires a
  **signed** bundle to register/display notifications. Since v2.0 ships **un-notarized/unsigned**
  ([01 §3.3](01-PRD.md), [06 §4](06-MIGRATION-REPO.md)), notifications may **silently fail** on
  macOS. This MUST be verified early (M6 spike, [07](07-ROADMAP.md)); do not assume it works.
  Fallback if it fails: rely on tray icon state + in-window log/status for signal, and document
  it as a known limitation of the unsigned build (fixed once OS signing is funded).

### Acceptance criteria
- [ ] Unexpected disconnect/error/connect notifies; user-initiated disconnect is silent.
- [ ] Update-available notifies once per version, not repeatedly.
- [ ] `showNotifications=false` suppresses all notifications.
- [ ] macOS permission requested at a controlled time, not a startup race.
- [ ] **Notifications verified on an UNSIGNED macOS build (F5).** If they do not work, the
      known limitation is documented and tray/log fallback confirmed — M6 acceptance does NOT
      assume macOS notifications function on the unsigned build.

---

<a id="updater"></a>
## 16. Updater with signed bundles (`updater/mod.rs`)

> **Two distinct signing concepts — do not conflate:**
> 1. **Updater bundle signing (minisign) — REQUIRED from day one.** `tauri-plugin-updater`
>    signs each update *bundle* with a minisign private key and verifies it against the
>    minisign **pubkey embedded in the app** (`tauri.conf.json`). This is what makes
>    self-update safe and is **enforced in v2.0**.
> 2. **OS code-signing / notarization (Apple Developer ID, Windows Authenticode) — DEFERRED
>    to a post-v2.0 task.** This is what the OS Gatekeeper/SmartScreen checks at *install*
>    time. v2.0 ships **unsigned at the OS level** (see [01 §3.3 backlog](01-PRD.md),
>    [06 §4](06-MIGRATION-REPO.md)). It is completely separate from #1 — updater signing
>    works and is enforced even though OS code-signing is off.

### Behavior (replace v1)
v1 pulled GitHub Releases via raw HttpClient, **no signature verification**, generated
install scripts inline. v2 uses `tauri-plugin-updater` with **minisign-signed bundles**.
Same GitHub repo Releases. See continuity/migration in [06-MIGRATION-REPO.md](06-MIGRATION-REPO.md).

### Rust approach
- **Minisign keypair**: generate with `pnpm tauri signer generate` (or
  `cargo tauri signer generate`). The **private key + its password live only as CI secrets**
  (`TAURI_SIGNING_PRIVATE_KEY` / `..._PASSWORD`), never in the repo. The **public key** goes
  in `tauri.conf.json` under `plugins.updater.pubkey`.
- `tauri.conf.json` → updater `active:true`, `pubkey` (the minisign public key),
  `endpoints` pointing at the GitHub Releases `latest.json`.
- `check_update`: query endpoint; compare version; respect `lastSkippedVersion`. Emit
  `update://status`.
- `install_update`: download → **verify minisign signature (enforced by plugin against the
  embedded pubkey)** → install; emit `update://progress` chunks; relaunch. Reject
  unsigned/invalid bundles.
- `skip_update(version)` sets `lastSkippedVersion`.
- CI signs the update bundle with the minisign private key (see [06 §4](06-MIGRATION-REPO.md)).

### Acceptance criteria
- [ ] Update check honors `autoCheckUpdates` and `lastSkippedVersion`.
- [ ] Install verifies the **minisign bundle signature**; a tampered/unsigned bundle is rejected and surfaced as error.
- [ ] The minisign private key exists only in CI secrets; the pubkey is committed in `tauri.conf.json`.
- [ ] Progress events drive a UI progress indicator; app relaunches into the new version.
- [ ] Endpoint is the same GitHub repo Releases (v1 users' repo).
- [ ] Updater signing works regardless of OS code-signing being deferred (the two are independent).

---

<a id="copy-ssh"></a>
## 17. Copy SSH command (`commands/forwards.rs::copy_ssh_command`)

### Behavior (replicate v1 EXACTLY — verified against `lib/models/forward_config.dart::toSshCommand`)
Build the equivalent `ssh -N -L ...` CLI string. **v1 ALWAYS emits `-p <sshPort>`** (it is
NOT conditional on port 22 — earlier drafts of this spec were wrong). v1 token order is:
`ssh -N -L <fwd> -p <port> [-i <identity>] user@host`.

### Rust approach
```
ssh -N -L <bindPrefix><localPort>:<remoteHost>:<remotePort> -p <sshPort> [-i <identityFilePath>] <sshUsername>@<sshHost>
```
- `<bindPrefix>` = `""` when `localBindAddress == "127.0.0.1"`, else `"<localBindAddress>:"`
  (exactly v1's rule).
- **Always** append `-p <sshPort>` (matches v1 — even for port 22).
- `-i` only when `identityFilePath` is set & non-empty; wrap the path in double quotes
  **only if it contains a space** (v1: `path.contains(' ') ? '"$path"' : path`).
- Never include the password.

### Acceptance criteria
- [ ] Default case (127.0.0.1, port 22, no identity) → `ssh -N -L <lp>:<rh>:<rp> -p 22 user@host`
      (note: `-p 22` IS present — v1 always emits it).
- [ ] Non-default bind address adds the `bind:` prefix to `-L`.
- [ ] Identity file adds `-i`, quoted only if the path contains a space.
- [ ] Token order matches v1: `-N`, `-L`, `-p`, `-i`, `user@host`.
- [ ] Output never contains a password.

---

<a id="logs"></a>
## 18. Logs (`state/log_buffer.rs`, `commands/logs.rs`)

### Behavior (replicate v1)
In-memory only (NOT persisted), cap 500 newest-first. Click row / Copy All → clipboard.
Clear action. `LogEntry`: level(info/warning/error), tunnelName, message, timestamp
`HH:mm:ss`; formatted line `[time] [LEVEL] [tunnel] message`.

### Rust approach
- `LogBuffer`: `VecDeque<LogEntry>` capped at 500 (pop back on overflow); newest-first for display.
- A `tracing` layer + explicit `log(level, tunnel, msg)` helper append entries and emit
  `log://line`. `get_logs` returns the snapshot; `clear_logs` empties + emits `log://cleared`;
  `get_logs_text` returns the joined formatted lines for Copy All.

### Acceptance criteria
- [ ] Buffer never exceeds 500; oldest dropped first.
- [ ] Formatted line matches `[HH:mm:ss] [LEVEL] [tunnel] message`.
- [ ] Clear empties buffer and notifies FE; Copy All returns full formatted text.
- [ ] Logs are not persisted across restarts.

---

<a id="platform"></a>
## 19. Platform notes (cross-cutting)

- **macOS unsandboxed** (`app-sandbox=false`): required for arbitrary SSH, reading
  `~/.ssh/id_*`, and self-update writes. Do **not** enable App Sandbox. Entitlements: keep
  network client/server; user-selected read-write for file picker.
- **OS code-signing / notarization is DEFERRED for v2.0** — the app ships un-notarized on
  macOS and unsigned on Windows. Users get past Gatekeeper via right-click → Open (macOS)
  and SmartScreen via "More info → Run anyway" (Windows). CI hooks for signing/notarization
  are written but stubbed/disabled; enabling them is a post-v2.0 task. See [06 §4](06-MIGRATION-REPO.md)
  and [01 §3.3](01-PRD.md). This is **independent** of updater bundle signing (§16), which is
  enforced from day one.
- **Window chrome (per-OS)**: macOS = custom transparent titlebar with native traffic
  lights; Windows/Linux = native OS decorations. Details + `tauri.conf.json` config in §14.
- **LSUIElement=true** on macOS (agent app / no dock by default) + runtime activation-policy
  switching (§13).
- **Fonts**: technical/numeric text (ports, latency, bytes, timestamps) uses a real
  monospace fallback stack in CSS — owned by design agent, see [05-UI-UX-SPEC.md](05-UI-UX-SPEC.md).
- **File picker**: `tauri-plugin-dialog` for identity file + backup path selection; scope
  in capabilities.
