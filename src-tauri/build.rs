use std::path::{Path, PathBuf};

fn main() {
    // The desktop build generates the Tauri context (config, capabilities,
    // icons). The server build has no Tauri at all.
    #[cfg(feature = "gui")]
    tauri_build::build();

    // The server build embeds the web UI (`pnpm build:web` → ../dist-web) into
    // the binary (spec 02 §10). Without a build there it still compiles, and
    // the server answers with a page explaining how to build the UI.
    if std::env::var_os("CARGO_FEATURE_SERVER").is_some() {
        embed_web_assets();
    }
}

fn embed_web_assets() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let root = manifest.join("..").join("dist-web");
    println!("cargo:rerun-if-changed={}", root.display());

    let mut files = Vec::new();
    if root.is_dir() {
        collect(&root, &root, &mut files);
    } else {
        println!(
            "cargo:warning=web UI not embedded: {} is missing (run `pnpm build:web` first)",
            root.display()
        );
    }
    files.sort();

    let mut out = String::from("pub static ASSETS: &[Asset] = &[\n");
    for (rel, abs) in &files {
        out.push_str(&format!(
            "    Asset {{ path: {rel:?}, body: include_bytes!({abs:?}), mime: {mime:?} }},\n",
            mime = mime_for(rel)
        ));
    }
    out.push_str("];\n");

    let dest = PathBuf::from(std::env::var("OUT_DIR").unwrap_or_default()).join("web_assets.rs");
    if let Err(e) = std::fs::write(&dest, out) {
        panic!("cannot write {}: {e}", dest.display());
    }
}

/// Every regular file under `dir`, as (path relative to `root` with `/`, absolute path).
fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, out);
        } else if path.is_file() {
            let Ok(rel) = path.strip_prefix(root) else {
                continue;
            };
            let rel = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            let abs = path.canonicalize().unwrap_or(path.clone());
            out.push((rel, abs.to_string_lossy().into_owned()));
        }
    }
}

fn mime_for(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "webmanifest" => "application/manifest+json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "ico" => "image/x-icon",
        "webp" => "image/webp",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}
