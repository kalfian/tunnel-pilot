//! Credential storage via the `keyring` crate (per-target features, F9) with a
//! `keychain_available()` probe and a plaintext fallback file + warning flag.
//! Stable `KC_SERVICE = "tunnel-pilot"` (spec 03 §9, 04 §10).
//!
//! TODO(M2): keyring set/get/delete, availability probe, fallback store.
