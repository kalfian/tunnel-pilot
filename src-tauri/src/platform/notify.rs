//! Desktop notifications via `tauri-plugin-notification`: connect/disconnect/
//! error (unexpected only) + update-once-per-version; macOS permission timing.
//! F5: may silently fail on the unsigned macOS build — verify in M6 (spec 03 §15).
//!
//! TODO(M6): notification wrapper + suppression on user-initiated disconnect.
