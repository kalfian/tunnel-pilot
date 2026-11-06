// Prevents an additional console window on Windows in release, DO NOT REMOVE!!
// (Desktop build only — the headless CLI build is a console program.)
#![cfg_attr(
    all(feature = "gui", not(debug_assertions)),
    windows_subsystem = "windows"
)]

fn main() {
    // CLI mode (spec 03 §20): when argv[1] is a known subcommand this is a
    // terminal invocation — usually talking to the ALREADY-RUNNING app or
    // daemon over its control socket — so answer and exit before any Tauri
    // code runs. Subcommands are positional words, so GUI flags such as
    // `--minimized` fall through untouched and the normal launch path is
    // unchanged.
    let argv: Vec<String> = std::env::args().collect();
    if let Some(code) = tunnel_pilot_lib::cli::client::dispatch(&argv) {
        std::process::exit(code);
    }

    #[cfg(feature = "gui")]
    tunnel_pilot_lib::run();

    // The CLI-only build has no window to fall back to: a bare invocation (or
    // an unknown word) prints usage instead (spec 03 §21).
    #[cfg(not(feature = "gui"))]
    {
        if argv.len() > 1 {
            eprintln!("error: unknown command '{}'", argv[1]);
            eprintln!("run 'tunnel-pilot help' for usage");
        } else {
            eprint!("{}", tunnel_pilot_lib::cli::args::HELP_TEXT);
        }
        std::process::exit(2);
    }
}
