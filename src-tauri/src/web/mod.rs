//! The web UI server of `tunnel-pilot-cli` (spec 02 §10, 03 §22).
//!
//! Serves the same Svelte UI as the desktop app plus a JSON binding of the same
//! IPC contract: `POST /api/invoke/<command>` runs `ops::dispatch::invoke`, and
//! `GET /api/events` streams the §7 events over a WebSocket. It is a third
//! driver of `AppState`, next to the tray/webview and the CLI socket — never a
//! second owner.
//!
//! Security model (the server may listen on a network interface):
//! - Login is always required (admin password, argon2id); sessions live in an
//!   `HttpOnly; SameSite=Strict` cookie (`auth.rs`).
//! - Every request's `Host` must be an IP literal, `localhost`, or a configured
//!   `--public-host` (DNS-rebinding defence).
//! - Every state-changing request must carry `X-Tunnel-Pilot: 1` and, when the
//!   browser sends an `Origin`, it must match the `Host` — a cross-site page can
//!   neither set the header nor pass the origin check (CSRF). The WebSocket
//!   handshake requires a matching `Origin`.
//! - `X-Forwarded-*` headers are ignored unless `--trust-proxy` says a reverse
//!   proxy sets them.
//! - Connections are served with a header-read timeout and capped in number
//!   (slowloris); logins are throttled and argon2 runs in bounded slots.
//! - Login attempts are throttled per IP; bodies are capped; every response
//!   carries a strict CSP, `X-Frame-Options: DENY`, `nosniff`, `no-referrer`,
//!   and API responses are `no-store`.
//! - No TLS here: terminate HTTPS at a reverse proxy. The daemon warns loudly
//!   when it listens beyond loopback.
//! - Nothing logs a password, a session id, or command arguments.

pub mod admin;
pub mod assets;
pub mod auth;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast::error::RecvError;

use crate::error::AppError;
use crate::state::AppState;
use auth::{Auth, LoginError};

/// Default web UI port (`--bind` without a port uses it too).
pub const DEFAULT_PORT: u16 = 1855;
/// Session cookie name.
pub const SESSION_COOKIE: &str = "tp_session";
/// The anti-CSRF header every state-changing request must carry.
pub const CSRF_HEADER: &str = "x-tunnel-pilot";
/// Largest accepted request body (a backup import is the biggest legit body).
pub const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
/// How often an open event stream re-checks its session and pings.
const WS_HEARTBEAT: Duration = Duration::from_secs(30);
/// The stream is server → client; anything a client sends is tiny or ignored.
const WS_MAX_INBOUND: usize = 4 * 1024;
/// A request whose headers take longer than this to arrive is dropped.
const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(10);
/// Open connections at once (WebSockets included); further ones are closed at
/// accept. Far below the default 1024 file limit, so the tunnels' own sockets
/// never starve.
const MAX_CONNECTIONS: usize = 256;

/// Strict CSP for the SPA: own scripts only, no framing, no plugins.
/// `style-src 'unsafe-inline'` is needed for Svelte's style attributes.
const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
img-src 'self' data:; font-src 'self' data:; connect-src 'self'; object-src 'none'; \
base-uri 'none'; form-action 'self'; frame-ancestors 'none'";

/// Web options from the daemon flags.
#[derive(Debug, Clone, Default)]
pub struct WebOptions {
    /// Trust `X-Forwarded-For/-Proto/-Host` (only behind a reverse proxy).
    pub trust_proxy: bool,
    /// Lower-cased host names accepted besides IP literals and `localhost`.
    pub public_hosts: Vec<String>,
}

struct Ctx {
    state: Arc<AppState>,
    auth: Auth,
    options: WebOptions,
}

/// Parse `--bind`: `addr`, `addr:port`, `[v6]:port`, or `localhost[:port]`;
/// `None` → `127.0.0.1:1855`.
pub fn parse_bind(raw: Option<&str>) -> Result<SocketAddr, AppError> {
    let Some(raw) = raw.map(str::trim).filter(|r| !r.is_empty()) else {
        return Ok(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            DEFAULT_PORT,
        ));
    };
    let raw = match raw.strip_prefix("localhost") {
        Some(rest) if rest.is_empty() || rest.starts_with(':') => format!("127.0.0.1{rest}"),
        _ => raw.to_string(),
    };
    if let Ok(addr) = raw.parse::<SocketAddr>() {
        return Ok(addr);
    }
    let ip_text = raw.trim_start_matches('[').trim_end_matches(']');
    ip_text
        .parse::<IpAddr>()
        .map(|ip| SocketAddr::new(ip, DEFAULT_PORT))
        .map_err(|_| {
            AppError::InvalidInput(format!(
                "--bind expects an IP address with an optional port (e.g. 0.0.0.0 or \
                 0.0.0.0:{DEFAULT_PORT}), got '{raw}'"
            ))
        })
}

/// Bind the listener now (so a busy port fails the daemon at startup) and
/// serve in the background. Returns the bound address and the server task.
pub async fn start(
    state: Arc<AppState>,
    config_dir: PathBuf,
    addr: SocketAddr,
    options: WebOptions,
) -> Result<(SocketAddr, tokio::task::JoinHandle<()>), AppError> {
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| AppError::Io(format!("cannot listen on {addr}: {e}")))?;
    let local = listener.local_addr().unwrap_or(addr);
    let ctx = Arc::new(Ctx {
        state,
        auth: Auth::new(config_dir),
        options,
    });
    let app = router(ctx);
    let task = tokio::spawn(accept_loop(listener, app));
    Ok((local, task))
}

/// Accept connections and serve each with hyper directly: a header-read
/// timeout drops slow-header (slowloris) clients, and a semaphore caps open
/// connections — neither is available through `axum::serve`.
async fn accept_loop(listener: tokio::net::TcpListener, app: Router) {
    use hyper_util::rt::{TokioIo, TokioTimer};
    use tower::ServiceExt;

    let slots = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(conn) => conn,
            Err(e) => {
                tracing::warn!(error = %e, "web accept failed");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let Ok(permit) = slots.clone().try_acquire_owned() else {
            tracing::warn!(client = %peer, "web connection limit reached; dropping");
            drop(stream);
            continue;
        };
        let app = app.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let service = hyper::service::service_fn(
                move |mut req: hyper::Request<hyper::body::Incoming>| {
                    req.extensions_mut().insert(ConnectInfo(peer));
                    app.clone().oneshot(req.map(axum::body::Body::new))
                },
            );
            let mut builder = hyper::server::conn::http1::Builder::new();
            builder
                .timer(TokioTimer::new())
                .header_read_timeout(HEADER_READ_TIMEOUT);
            if let Err(e) = builder
                .serve_connection(TokioIo::new(stream), service)
                .with_upgrades()
                .await
            {
                tracing::debug!(client = %peer, error = %e, "web connection ended");
            }
        });
    }
}

fn router(ctx: Arc<Ctx>) -> Router {
    Router::new()
        .route("/api/session", get(session))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/invoke/{command}", post(invoke))
        .route("/api/events", get(events))
        .fallback(static_files)
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(middleware::from_fn_with_state(ctx.clone(), host_guard))
        .layer(middleware::from_fn_with_state(
            ctx.clone(),
            security_headers,
        ))
        .with_state(ctx)
}

// --- responses ---

fn error_response(status: StatusCode, error: AppError) -> Response {
    (status, Json(error)).into_response()
}

fn status_for(error: &AppError) -> StatusCode {
    match error {
        AppError::NotFound(_) => StatusCode::NOT_FOUND,
        AppError::InvalidInput(_) => StatusCode::BAD_REQUEST,
        AppError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn unauthorized(message: &str) -> Response {
    error_response(
        StatusCode::UNAUTHORIZED,
        AppError::Unauthorized(message.into()),
    )
}

fn forbidden(message: &str) -> Response {
    error_response(
        StatusCode::FORBIDDEN,
        AppError::Unauthorized(message.into()),
    )
}

async fn security_headers(State(ctx): State<Arc<Ctx>>, req: Request, next: Next) -> Response {
    let is_api = req.uri().path().starts_with("/api/");
    // Browsers only honour COOP on a trustworthy origin (HTTPS or localhost);
    // over plain HTTP they ignore it AND log an error, so only send it behind TLS.
    let behind_tls = is_tls(req.headers(), &ctx.options);
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(CSP),
    );
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    if behind_tls {
        h.insert(
            "cross-origin-opener-policy",
            HeaderValue::from_static("same-origin"),
        );
    }
    h.insert(
        "cross-origin-resource-policy",
        HeaderValue::from_static("same-origin"),
    );
    if is_api {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    res
}

/// Refuse any request whose `Host` (and, behind a trusted proxy, its
/// `X-Forwarded-Host`) is not one of ours — a DNS-rebinding page names its own
/// domain there.
async fn host_guard(State(ctx): State<Arc<Ctx>>, req: Request, next: Next) -> Response {
    let headers = req.headers();
    let host_ok = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|h| host_allowed(h, &ctx.options));
    let forwarded_ok = forwarded(headers, "x-forwarded-host", &ctx.options)
        .map_or(true, |h| host_allowed(h, &ctx.options));
    if host_ok && forwarded_ok {
        next.run(req).await
    } else {
        error_response(
            StatusCode::MISDIRECTED_REQUEST,
            AppError::Unauthorized(
                "unknown host name; start the daemon with --public-host <name> to allow it".into(),
            ),
        )
    }
}

// --- request checks ---

/// A `X-Forwarded-*` header value — only when a trusted proxy sets them.
fn forwarded<'a>(headers: &'a HeaderMap, name: &str, options: &WebOptions) -> Option<&'a str> {
    if !options.trust_proxy {
        return None;
    }
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
}

/// Whether the request reached us over HTTPS (as reported by a trusted proxy).
fn is_tls(headers: &HeaderMap, options: &WebOptions) -> bool {
    forwarded(headers, "x-forwarded-proto", options)
        .is_some_and(|p| p.eq_ignore_ascii_case("https"))
}

/// The host part of a `Host`-style value (`name[:port]`, `[v6]:port`), lower-cased.
fn host_name(value: &str) -> String {
    let value = value.trim();
    let name = if let Some(rest) = value.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else if value.matches(':').count() > 1 {
        value // a bare IPv6 literal
    } else {
        value.split(':').next().unwrap_or("")
    };
    name.to_ascii_lowercase()
}

/// IP literals and `localhost` cannot be rebound; anything else must be listed.
fn host_allowed(value: &str, options: &WebOptions) -> bool {
    let name = host_name(value);
    !name.is_empty()
        && (name.parse::<IpAddr>().is_ok()
            || name == "localhost"
            || options.public_hosts.iter().any(|h| h == &name))
}

/// The client's IP: the TCP peer, or — behind a trusted proxy — the address the
/// proxy appended last to `X-Forwarded-For` (earlier entries are client-supplied).
fn client_ip(peer: SocketAddr, headers: &HeaderMap, options: &WebOptions) -> IpAddr {
    if options.trust_proxy {
        let last = headers
            .get_all("x-forwarded-for")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(','))
            .map(str::trim)
            .rfind(|s| !s.is_empty());
        if let Some(ip) = last.and_then(|s| s.parse::<IpAddr>().ok()) {
            return ip;
        }
    }
    peer.ip()
}

/// Every `tp_session` value in the `Cookie` headers. A cookie planted by a
/// sibling service on the same host name cannot shadow the real one: each
/// candidate is tried.
fn session_cookies(headers: &HeaderMap) -> Vec<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .filter(|(name, value)| *name == SESSION_COOKIE && !value.is_empty())
        .map(|(_, value)| value.to_string())
        .take(8)
        .collect()
}

/// The first presented session id that is live, if any.
fn valid_session(ctx: &Ctx, headers: &HeaderMap) -> Option<String> {
    session_cookies(headers)
        .into_iter()
        .find(|id| ctx.auth.validate(id))
}

fn has_csrf_header(headers: &HeaderMap) -> bool {
    headers
        .get(CSRF_HEADER)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == "1")
}

/// `host[:port]` of an `Origin` header value (`scheme://host[:port]`).
fn origin_authority(origin: &str) -> Option<&str> {
    let (_, rest) = origin.split_once("://")?;
    Some(rest.trim_end_matches('/'))
}

/// Whether the `Origin` (when `required`, it must be present) names this
/// server: it must equal `Host`, or `X-Forwarded-Host` behind a trusted proxy.
fn origin_matches(headers: &HeaderMap, required: bool, options: &WebOptions) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return !required;
    };
    let Some(authority) = origin_authority(origin) else {
        return false; // includes the opaque "null" origin
    };
    let host_matches = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|h| h.eq_ignore_ascii_case(authority));
    host_matches
        || forwarded(headers, "x-forwarded-host", options)
            .is_some_and(|h| h.eq_ignore_ascii_case(authority))
}

/// The checks every state-changing request passes before anything else.
fn check_mutation(headers: &HeaderMap, options: &WebOptions) -> Result<(), Box<Response>> {
    if !has_csrf_header(headers) {
        return Err(Box::new(forbidden("missing X-Tunnel-Pilot header")));
    }
    if !origin_matches(headers, false, options) {
        return Err(Box::new(forbidden("cross-origin request refused")));
    }
    Ok(())
}

fn require_session(ctx: &Ctx, headers: &HeaderMap) -> Result<String, Box<Response>> {
    valid_session(ctx, headers).ok_or_else(|| Box::new(unauthorized("sign in to continue")))
}

fn session_set_cookie(id: &str, headers: &HeaderMap, options: &WebOptions) -> String {
    let secure = is_tls(headers, options);
    format!(
        "{SESSION_COOKIE}={id}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}{}",
        auth::ABSOLUTE_TIMEOUT.as_secs(),
        if secure { "; Secure" } else { "" }
    )
}

fn clear_cookie() -> String {
    format!("{SESSION_COOKIE}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0")
}

// --- handlers ---

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionInfo {
    authenticated: bool,
    version: &'static str,
    edition: &'static str,
}

async fn session(State(ctx): State<Arc<Ctx>>, headers: HeaderMap) -> Response {
    let authenticated = valid_session(&ctx, &headers).is_some();
    Json(SessionInfo {
        authenticated,
        version: env!("CARGO_PKG_VERSION"),
        edition: crate::cli::EDITION,
    })
    .into_response()
}

/// Carries the admin password: no `Debug`, never logged.
#[derive(Deserialize)]
struct LoginBody {
    password: String,
}

async fn login(
    State(ctx): State<Arc<Ctx>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(res) = check_mutation(&headers, &ctx.options) {
        return *res;
    }
    let Ok(LoginBody { password }) = serde_json::from_slice::<LoginBody>(&body) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            AppError::InvalidInput("expected {\"password\": \"...\"}".into()),
        );
    };
    let client = client_ip(peer, &headers, &ctx.options);
    match ctx.auth.login(client, password).await {
        Ok(id) => (
            StatusCode::NO_CONTENT,
            [(
                header::SET_COOKIE,
                session_set_cookie(&id, &headers, &ctx.options),
            )],
        )
            .into_response(),
        Err(LoginError::Busy) => {
            let mut res = error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                AppError::Unauthorized("the server is busy; try again in a moment".into()),
            );
            res.headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("2"));
            res
        }
        Err(LoginError::Rejected) => unauthorized("wrong password"),
        Err(LoginError::RateLimited(retry)) => {
            let mut res = error_response(
                StatusCode::TOO_MANY_REQUESTS,
                AppError::Unauthorized(format!("too many failed sign-ins; try again in {retry}s")),
            );
            if let Ok(v) = HeaderValue::from_str(&retry.to_string()) {
                res.headers_mut().insert(header::RETRY_AFTER, v);
            }
            res
        }
        Err(LoginError::NotConfigured) => {
            unauthorized("no admin password is set; run: tunnel-pilot-cli web set-password")
        }
        Err(LoginError::Internal(msg)) => {
            tracing::error!(error = %msg, "web login failed internally");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                AppError::Internal("sign-in failed".into()),
            )
        }
    }
}

async fn logout(State(ctx): State<Arc<Ctx>>, headers: HeaderMap) -> Response {
    if let Err(res) = check_mutation(&headers, &ctx.options) {
        return *res;
    }
    for id in session_cookies(&headers) {
        ctx.auth.logout(&id);
    }
    (
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, clear_cookie())],
    )
        .into_response()
}

async fn invoke(
    State(ctx): State<Arc<Ctx>>,
    Path(command): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(res) = check_mutation(&headers, &ctx.options) {
        return *res;
    }
    if let Err(res) = require_session(&ctx, &headers) {
        return *res;
    }
    let args = if body.iter().all(u8::is_ascii_whitespace) {
        serde_json::Value::Null
    } else {
        match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(e) => {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    AppError::InvalidInput(format!("request body is not JSON: {e}")),
                )
            }
        }
    };
    tracing::debug!(command = %command, "web invoke");
    match crate::ops::dispatch::invoke(&ctx.state, &command, args).await {
        Ok(value) => Json(value).into_response(),
        Err(e) => error_response(status_for(&e), e),
    }
}

async fn events(
    State(ctx): State<Arc<Ctx>>,
    headers: HeaderMap,
    ws: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Response {
    // Authenticate BEFORE looking at the upgrade itself, so an anonymous or
    // cross-site caller learns nothing beyond 401/403.
    let id = match require_session(&ctx, &headers) {
        Ok(id) => id,
        Err(res) => return *res,
    };
    if !origin_matches(&headers, true, &ctx.options) {
        return forbidden("cross-origin event stream refused");
    }
    match ws {
        Ok(ws) => ws
            .max_message_size(WS_MAX_INBOUND)
            .max_frame_size(WS_MAX_INBOUND)
            .on_upgrade(move |socket| pump_events(ctx, id, socket)),
        Err(rejection) => rejection.into_response(),
    }
}

/// Forward bus events to one client until it leaves, lags, or its session ends.
async fn pump_events(ctx: Arc<Ctx>, session_id: String, mut socket: WebSocket) {
    let mut rx = ctx.state.subscribe_events();
    let mut heartbeat = tokio::time::interval(WS_HEARTBEAT);
    heartbeat.tick().await; // the first tick fires immediately
    loop {
        tokio::select! {
            event = rx.recv() => match event {
                Ok(event) => {
                    let Ok(text) = serde_json::to_string(&event) else { continue };
                    if socket.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                Err(RecvError::Lagged(_)) => {
                    // Too slow to keep up: make the client reconnect and rehydrate.
                    close(&mut socket, 4000, "resync").await;
                    break;
                }
                Err(RecvError::Closed) => break,
            },
            _ = heartbeat.tick() => {
                if !ctx.auth.validate(&session_id) {
                    close(&mut socket, 4401, "session ended").await;
                    break;
                }
                if socket.send(Message::Ping(Bytes::new())).await.is_err() {
                    break;
                }
            }
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => {} // the stream is server → client only
            },
        }
    }
}

async fn close(socket: &mut WebSocket, code: u16, reason: &'static str) {
    let _ = socket
        .send(Message::Close(Some(CloseFrame {
            code,
            reason: reason.into(),
        })))
        .await;
}

async fn static_files(method: Method, uri: Uri) -> Response {
    let path = uri.path();
    if path.starts_with("/api/") || path == "/api" {
        return error_response(
            StatusCode::NOT_FOUND,
            AppError::NotFound(format!("no API route {path}")),
        );
    }
    if method != Method::GET && method != Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let last = path.rsplit('/').next().unwrap_or("");
    let asset = match assets::get(path) {
        Some(a) => Some(a),
        // Client-side routes (no file extension) get the SPA shell.
        None if !last.contains('.') => assets::index(),
        None => None,
    };
    match asset {
        Some(asset) => {
            let cache = if assets::is_immutable(asset.path) {
                "public, max-age=31536000, immutable"
            } else {
                "no-cache"
            };
            (
                [
                    (header::CONTENT_TYPE, asset.mime),
                    (header::CACHE_CONTROL, cache),
                ],
                asset.body,
            )
                .into_response()
        }
        None if assets::ASSETS.is_empty() => (
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            assets::MISSING_UI_PAGE,
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.append(
                header::HeaderName::from_bytes(k.as_bytes()).expect("name"),
                HeaderValue::from_str(v).expect("value"),
            );
        }
        h
    }

    #[test]
    fn bind_defaults_and_forms() {
        assert_eq!(
            parse_bind(None).expect("default").to_string(),
            "127.0.0.1:1855"
        );
        assert_eq!(
            parse_bind(Some("0.0.0.0")).expect("ip").to_string(),
            "0.0.0.0:1855"
        );
        assert_eq!(
            parse_bind(Some("0.0.0.0:8080"))
                .expect("ip:port")
                .to_string(),
            "0.0.0.0:8080"
        );
        assert_eq!(
            parse_bind(Some("[::]")).expect("v6").to_string(),
            "[::]:1855"
        );
        assert_eq!(
            parse_bind(Some("localhost:9000"))
                .expect("localhost")
                .to_string(),
            "127.0.0.1:9000"
        );
        assert!(parse_bind(Some("example.com")).is_err());
        assert!(parse_bind(Some("notlocalhost")).is_err());
    }

    fn proxy() -> WebOptions {
        WebOptions {
            trust_proxy: true,
            public_hosts: vec!["tunnels.example.com".into()],
        }
    }

    #[test]
    fn cookie_parsing_collects_every_candidate() {
        let h = headers(&[("cookie", "a=1; tp_session=abc123; b=2")]);
        assert_eq!(session_cookies(&h), vec!["abc123".to_string()]);
        let h = headers(&[("cookie", "xtp_session=evil")]);
        assert!(session_cookies(&h).is_empty());
        assert!(session_cookies(&headers(&[("cookie", "tp_session=")])).is_empty());
        // A shadowing cookie from a sibling service does not hide the real one.
        let h = headers(&[("cookie", "tp_session=planted; tp_session=real")]);
        assert_eq!(
            session_cookies(&h),
            vec!["planted".to_string(), "real".to_string()]
        );
    }

    #[test]
    fn csrf_header_and_origin_rules() {
        let none = WebOptions::default();
        assert!(
            check_mutation(&headers(&[]), &none).is_err(),
            "header is mandatory"
        );
        assert!(check_mutation(
            &headers(&[(CSRF_HEADER, "1"), ("host", "10.0.0.5:1855")]),
            &none
        )
        .is_ok());
        assert!(check_mutation(
            &headers(&[
                (CSRF_HEADER, "1"),
                ("host", "10.0.0.5:1855"),
                ("origin", "http://10.0.0.5:1855"),
            ]),
            &none
        )
        .is_ok());
        assert!(
            check_mutation(
                &headers(&[
                    (CSRF_HEADER, "1"),
                    ("host", "10.0.0.5:1855"),
                    ("origin", "https://evil.example"),
                ]),
                &none
            )
            .is_err(),
            "a foreign origin is refused even with the header"
        );
        assert!(!origin_matches(
            &headers(&[("host", "h"), ("origin", "null")]),
            false,
            &none
        ));
        assert!(
            !origin_matches(&headers(&[("host", "h:1855")]), true, &none),
            "the event stream requires an Origin"
        );
        let proxied = headers(&[
            ("host", "127.0.0.1:1855"),
            ("x-forwarded-host", "tunnels.example.com"),
            ("origin", "https://tunnels.example.com"),
        ]);
        assert!(origin_matches(&proxied, true, &proxy()));
        assert!(
            !origin_matches(&proxied, true, &none),
            "X-Forwarded-Host is ignored without --trust-proxy"
        );
    }

    #[test]
    fn host_allowlist_blocks_rebinding_names() {
        let none = WebOptions::default();
        for ok in [
            "127.0.0.1:1855",
            "10.0.0.5",
            "[::1]:1855",
            "::1",
            "localhost:1855",
            "LOCALHOST",
        ] {
            assert!(host_allowed(ok, &none), "{ok} must be accepted");
        }
        for bad in [
            "rebind.evil.example:1855",
            "tunnels.example.com",
            "",
            "localhost.evil.com",
        ] {
            assert!(!host_allowed(bad, &none), "{bad} must be refused");
        }
        assert!(host_allowed("Tunnels.Example.com:443", &proxy()));
    }

    #[test]
    fn forwarded_headers_need_trust_proxy() {
        let peer = SocketAddr::from(([127, 0, 0, 1], 5000));
        let h = headers(&[
            ("x-forwarded-for", "6.6.6.6, 203.0.113.9"),
            ("x-forwarded-proto", "https"),
        ]);
        assert_eq!(client_ip(peer, &h, &WebOptions::default()), peer.ip());
        assert_eq!(
            client_ip(peer, &h, &proxy()).to_string(),
            "203.0.113.9",
            "the proxy-appended (last) entry, not the client-supplied first one"
        );
        assert!(!is_tls(&h, &WebOptions::default()));
        assert!(is_tls(&h, &proxy()));
    }

    #[test]
    fn cookie_is_httponly_strict_and_secure_behind_a_trusted_tls_proxy() {
        let tls = headers(&[("x-forwarded-proto", "https")]);
        let plain = session_set_cookie("id", &tls, &WebOptions::default());
        assert!(plain.contains("HttpOnly") && plain.contains("SameSite=Strict"));
        assert!(
            !plain.contains("Secure"),
            "untrusted X-Forwarded-Proto is ignored"
        );
        assert!(session_set_cookie("id", &tls, &proxy()).ends_with("; Secure"));
    }

    #[test]
    fn error_statuses() {
        assert_eq!(
            status_for(&AppError::NotFound("x".into())),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            status_for(&AppError::InvalidInput("x".into())),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            status_for(&AppError::Ssh("x".into())),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
    // --- router-level tests: the security rules, end to end in-process ---

    use axum::body::Body;
    use axum::extract::connect_info::MockConnectInfo;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    const PASSWORD: &str = "correct horse battery";

    fn app() -> (tempfile::TempDir, Router) {
        let dir = tempfile::tempdir().expect("tmp");
        admin::set_password(dir.path(), PASSWORD).expect("set");
        let ctx = Arc::new(Ctx {
            state: Arc::new(AppState::new_headless()),
            auth: Auth::new(dir.path().to_path_buf()),
            options: WebOptions::default(),
        });
        let router = router(ctx).layer(MockConnectInfo(SocketAddr::from(([10, 0, 0, 9], 4000))));
        (dir, router)
    }

    fn post(path: &str, body: &str, extra: &[(&str, &str)]) -> axum::http::Request<Body> {
        let mut req = axum::http::Request::builder()
            .method("POST")
            .uri(path)
            .header("host", "10.0.0.5:1855")
            .header("content-type", "application/json");
        for (k, v) in extra {
            req = req.header(*k, *v);
        }
        req.body(Body::from(body.to_string())).expect("request")
    }

    async fn send(app: &Router, req: axum::http::Request<Body>) -> (StatusCode, HeaderMap, String) {
        let res = app.clone().oneshot(req).await.expect("response");
        let status = res.status();
        let headers = res.headers().clone();
        let body = res.into_body().collect().await.expect("body").to_bytes();
        (status, headers, String::from_utf8_lossy(&body).into_owned())
    }

    async fn sign_in(app: &Router) -> String {
        let body = format!("{{\"password\":\"{PASSWORD}\"}}");
        let (status, headers, _) =
            send(app, post("/api/login", &body, &[(CSRF_HEADER, "1")])).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let cookie = headers
            .get(header::SET_COOKIE)
            .and_then(|v| v.to_str().ok())
            .expect("set-cookie")
            .to_string();
        assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Strict"));
        cookie.split(';').next().expect("pair").to_string()
    }

    #[tokio::test]
    async fn invoke_requires_a_session_and_the_csrf_header() {
        let (_dir, app) = app();
        // No session.
        let (status, _, body) = send(
            &app,
            post("/api/invoke/list_forwards", "{}", &[(CSRF_HEADER, "1")]),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(body.contains("\"kind\":\"unauthorized\""));

        let cookie = sign_in(&app).await;
        // Session but no CSRF header.
        let (status, _, _) = send(
            &app,
            post("/api/invoke/list_forwards", "{}", &[("cookie", &cookie)]),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        // Session + header + foreign origin.
        let (status, _, _) = send(
            &app,
            post(
                "/api/invoke/list_forwards",
                "{}",
                &[
                    ("cookie", &cookie),
                    (CSRF_HEADER, "1"),
                    ("origin", "https://evil.example"),
                ],
            ),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        // All good.
        let (status, headers, body) = send(
            &app,
            post(
                "/api/invoke/list_forwards",
                "",
                &[
                    ("cookie", &cookie),
                    (CSRF_HEADER, "1"),
                    ("origin", "http://10.0.0.5:1855"),
                ],
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "[]");
        assert_eq!(
            headers.get(header::CACHE_CONTROL).map(|v| v.as_bytes()),
            Some(&b"no-store"[..])
        );
        assert!(headers.get(header::CONTENT_SECURITY_POLICY).is_some());
        assert_eq!(
            headers.get(header::X_FRAME_OPTIONS).map(|v| v.as_bytes()),
            Some(&b"DENY"[..])
        );

        // Desktop-only commands are not served.
        let (status, _, _) = send(
            &app,
            post(
                "/api/invoke/quit_app",
                "{}",
                &[("cookie", &cookie), (CSRF_HEADER, "1")],
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn login_failures_are_401_then_429_and_logout_ends_the_session() {
        let (_dir, app) = app();
        let (status, _, _) = send(&app, post("/api/login", r#"{"password":"x"}"#, &[])).await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "login also needs the CSRF header"
        );
        for _ in 0..auth::MAX_FAILURES {
            let (status, _, _) = send(
                &app,
                post(
                    "/api/login",
                    r#"{"password":"wrong password!"}"#,
                    &[(CSRF_HEADER, "1")],
                ),
            )
            .await;
            assert_eq!(status, StatusCode::UNAUTHORIZED);
        }
        let (status, headers, _) = send(
            &app,
            post(
                "/api/login",
                r#"{"password":"wrong password!"}"#,
                &[(CSRF_HEADER, "1")],
            ),
        )
        .await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert!(headers.get(header::RETRY_AFTER).is_some());
    }

    #[tokio::test]
    async fn logout_invalidates_the_cookie() {
        let (_dir, app) = app();
        let cookie = sign_in(&app).await;
        let (status, headers, _) = send(
            &app,
            post(
                "/api/logout",
                "",
                &[("cookie", &cookie), (CSRF_HEADER, "1")],
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert!(headers
            .get(header::SET_COOKIE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|c| c.contains("Max-Age=0")));
        let (status, _, _) = send(
            &app,
            post(
                "/api/invoke/list_forwards",
                "{}",
                &[("cookie", &cookie), (CSRF_HEADER, "1")],
            ),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn session_endpoint_and_spa_fallback() {
        let (_dir, app) = app();
        let req = axum::http::Request::builder()
            .uri("/api/session")
            .header("host", "10.0.0.5:1855")
            .body(Body::empty())
            .expect("req");
        let (status, _, body) = send(&app, req).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("\"authenticated\":false") && body.contains("\"edition\""));

        let req = axum::http::Request::builder()
            .uri("/api/nope")
            .header("host", "10.0.0.5:1855")
            .body(Body::empty())
            .expect("req");
        let (status, _, body) = send(&app, req).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(
            body.contains("notFound"),
            "unknown API routes answer JSON, not the SPA"
        );

        let req = axum::http::Request::builder()
            .uri("/connections")
            .header("host", "10.0.0.5:1855")
            .body(Body::empty())
            .expect("req");
        let (status, headers, _) = send(&app, req).await;
        assert_eq!(status, StatusCode::OK);
        assert!(headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|t| t.starts_with("text/html")));
    }

    #[tokio::test]
    async fn a_rebinding_host_name_is_refused_on_every_route() {
        let (_dir, app) = app();
        for path in ["/", "/api/session"] {
            let req = axum::http::Request::builder()
                .uri(path)
                .header("host", "rebind.evil.example:1855")
                .body(Body::empty())
                .expect("req");
            let (status, headers, _) = send(&app, req).await;
            assert_eq!(status, StatusCode::MISDIRECTED_REQUEST, "{path}");
            assert!(
                headers.get(header::CONTENT_SECURITY_POLICY).is_some(),
                "headers still applied"
            );
        }
        let body = format!("{{\"password\":\"{PASSWORD}\"}}");
        let mut req = post(
            "/api/login",
            &body,
            &[
                (CSRF_HEADER, "1"),
                ("origin", "http://rebind.evil.example:1855"),
            ],
        );
        req.headers_mut()
            .insert("host", HeaderValue::from_static("rebind.evil.example:1855"));
        let (status, headers, _) = send(&app, req).await;
        assert_eq!(status, StatusCode::MISDIRECTED_REQUEST);
        assert!(
            headers.get(header::SET_COOKIE).is_none(),
            "no session for a rebound name"
        );
    }

    #[tokio::test]
    async fn event_stream_requires_session_and_origin() {
        let (_dir, app) = app();
        let ws_req = |cookie: Option<&str>, origin: Option<&str>| {
            let mut b = axum::http::Request::builder()
                .uri("/api/events")
                .header("host", "10.0.0.5:1855")
                .header("connection", "upgrade")
                .header("upgrade", "websocket")
                .header("sec-websocket-version", "13")
                .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==");
            if let Some(c) = cookie {
                b = b.header("cookie", c);
            }
            if let Some(o) = origin {
                b = b.header("origin", o);
            }
            b.body(Body::empty()).expect("req")
        };
        let (status, _, _) = send(&app, ws_req(None, Some("http://10.0.0.5:1855"))).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let cookie = sign_in(&app).await;
        let (status, _, _) = send(&app, ws_req(Some(&cookie), None)).await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "Origin is mandatory for the stream"
        );
        let (status, _, _) = send(&app, ws_req(Some(&cookie), Some("https://evil.example"))).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
}
