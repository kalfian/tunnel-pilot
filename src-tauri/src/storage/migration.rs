//! v1→v2 migration: hardcoded per-OS v1-path probe + lenient import (plaintext
//! passwords → keychain); Linux = no probe (F2/F17, spec 04 §12).
//!
//! TODO(M2): v1_config_path() probe, import steps, `.v1-backup`, idempotency.
