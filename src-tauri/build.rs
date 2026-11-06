fn main() {
    // The desktop build generates the Tauri context (config, capabilities,
    // icons). The headless CLI build (`--no-default-features`) has no Tauri
    // at all, so there is nothing to generate.
    #[cfg(feature = "gui")]
    tauri_build::build();
}
