//! The web UI's static files, embedded at build time by `build.rs` from
//! `../dist-web` (`pnpm build:web`). Served with an SPA fallback: any GET that is
//! not an API route and not a file resolves to `index.html`.

/// One embedded file.
pub struct Asset {
    /// Path relative to the web root, `/`-separated, no leading slash.
    pub path: &'static str,
    pub body: &'static [u8],
    pub mime: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/web_assets.rs"));

/// The file at `path` (leading `/` ignored), if embedded.
pub fn get(path: &str) -> Option<&'static Asset> {
    let path = path.trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    ASSETS.iter().find(|a| a.path == path)
}

/// The SPA entry document.
pub fn index() -> Option<&'static Asset> {
    get("index.html")
}

/// Whether `path` names a content-hashed build artifact that may be cached
/// forever (Vite emits them under `assets/`).
pub fn is_immutable(path: &str) -> bool {
    path.trim_start_matches('/').starts_with("assets/")
}

/// Shown when the binary was built without `pnpm build:web` output.
pub const MISSING_UI_PAGE: &str = "<!doctype html><meta charset=utf-8>\
<title>Tunnel Pilot CLI</title>\
<body style=\"font-family:system-ui;max-width:40rem;margin:4rem auto;padding:0 1rem\">\
<h1>Web UI not built</h1>\
<p>This tunnel-pilot-cli binary was compiled without the web UI. Build it with \
<code>pnpm build:web</code> before <code>cargo build --features server</code>, or \
install a release build. The API and the command line work normally.</p>";
