//! System tray icon + menu (spec 03 §§12,13; 02 §8).
//!
//! The full dynamic tray (count icon 1–9, menu rebuild on state change,
//! per-tunnel rows, bulk actions, update notice) lands in M3. The minimal M0
//! tray (Open/Quit) is built inline in `lib.rs` setup.
//!
//! TODO(M3): fill the submodules below.

pub mod icon;
pub mod menu;
