// Prevents an additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // CLI mode (spec 03 §20): when argv[1] is a known subcommand this is a
    // terminal invocation talking to the ALREADY-RUNNING app over its control
    // socket — answer and exit before any Tauri code runs. Subcommands are
    // positional words, so GUI flags such as `--minimized` fall through
    // untouched and the normal launch path is unchanged.
    let argv: Vec<String> = std::env::args().collect();
    if let Some(code) = tunnel_pilot_lib::cli::client::dispatch(&argv) {
        std::process::exit(code);
    }

    tunnel_pilot_lib::run();
}
