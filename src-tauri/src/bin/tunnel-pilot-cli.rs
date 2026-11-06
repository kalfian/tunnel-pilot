//! `tunnel-pilot-cli` — the server product (spec 03 §22): the same tunnel
//! engine and features as the desktop app, run by `daemon` and managed from the
//! command line or the web UI (`daemon --web`). No Tauri, WebKit or GTK.

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    if let Some(code) = tunnel_pilot_lib::cli::client::dispatch(&argv) {
        std::process::exit(code);
    }
    // No window to fall back to: a bare invocation or an unknown word prints
    // usage instead.
    if argv.len() > 1 {
        eprintln!("error: unknown command '{}'", argv[1]);
        eprintln!("run 'tunnel-pilot-cli help' for usage");
    } else {
        eprint!("{}", tunnel_pilot_lib::cli::args::help_text());
    }
    std::process::exit(2);
}
