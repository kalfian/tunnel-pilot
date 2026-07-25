//! Dynamic tray-icon selection: idle grey / badge 1–9 (clamp), template images
//! on macOS (spec 03 §10). Also hosts the small colored **status-dot** images
//! used as leading icons on per-tunnel menu rows (Herd-style).
//!
//! The count→asset mapping is a pure function ([`tray_icon_for_count`]) so it is
//! unit-testable without a display. The PNG bytes are embedded at compile time
//! (`include_bytes!`) so the icons are available identically in dev, tests, and
//! every bundle regardless of the runtime working directory.
//!
//! Menu-bar icon: the embedded `tray_icon_*` PNGs are **proper macOS template
//! images** — pure-black RGB (0,0,0) + alpha at 44×44 (22pt @2x). macOS tints
//! them to the menu-bar appearance (light/dark) via the alpha channel, so they
//! render crisp and native next to system icons. [`update_tray_icon`] sets
//! `set_icon_as_template(true)` on macOS. On Windows/Linux they render as-is.
//!
//! Status dots ([`load_dot`]) are *colored* (not template) — they must keep
//! their green/yellow/red/grey hue inside the menu regardless of appearance.

use tauri::image::Image;

use crate::state::models::ForwardStatus;

/// The connected-count value at which the badge clamps (spec 03 §10).
pub const MAX_BADGE: usize = 9;

/// Which tray icon asset to display for a given connected-tunnel count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayIcon {
    /// No tunnels connected — the grey idle icon.
    Idle,
    /// One or more connected — the numbered badge (1..=9, clamped).
    Badge(u8),
}

/// Pure count→asset selection (spec 03 §10): `0 → Idle`, otherwise the badge
/// clamped to `MAX_BADGE`. Unit-tested including the clamp at 9.
pub fn tray_icon_for_count(count: usize) -> TrayIcon {
    if count == 0 {
        TrayIcon::Idle
    } else {
        TrayIcon::Badge(count.min(MAX_BADGE) as u8)
    }
}

const IDLE_PNG: &[u8] = include_bytes!("../../../assets/icons/tray_icon_idle.png");

/// Badge PNGs for counts 1..=9 (index `n-1`).
const BADGE_PNG: [&[u8]; MAX_BADGE] = [
    include_bytes!("../../../assets/icons/tray_icon_1.png"),
    include_bytes!("../../../assets/icons/tray_icon_2.png"),
    include_bytes!("../../../assets/icons/tray_icon_3.png"),
    include_bytes!("../../../assets/icons/tray_icon_4.png"),
    include_bytes!("../../../assets/icons/tray_icon_5.png"),
    include_bytes!("../../../assets/icons/tray_icon_6.png"),
    include_bytes!("../../../assets/icons/tray_icon_7.png"),
    include_bytes!("../../../assets/icons/tray_icon_8.png"),
    include_bytes!("../../../assets/icons/tray_icon_9.png"),
];

/// The embedded PNG bytes backing a [`TrayIcon`].
fn png_bytes(icon: TrayIcon) -> &'static [u8] {
    match icon {
        TrayIcon::Idle => IDLE_PNG,
        // `Badge` is only ever constructed with 1..=MAX_BADGE by
        // `tray_icon_for_count`; clamp defensively so an out-of-range value can
        // never index out of bounds.
        TrayIcon::Badge(n) => {
            let idx = (n as usize).clamp(1, MAX_BADGE) - 1;
            BADGE_PNG[idx]
        }
    }
}

/// Decode the embedded PNG for `icon` into a Tauri [`Image`] (needs the
/// `image-png` cargo feature, enabled in `Cargo.toml`).
pub fn load_image(icon: TrayIcon) -> tauri::Result<Image<'static>> {
    Image::from_bytes(png_bytes(icon))
}

// --- per-tunnel status dots ------------------------------------------------

/// A colored status dot shown as the leading icon of a per-tunnel menu row.
/// Maps the app status palette: connected=green, connecting/disconnecting=yellow,
/// error=red, disconnected=grey.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusDot {
    Green,
    Yellow,
    Red,
    Grey,
}

impl StatusDot {
    /// The status→dot-color mapping (app status palette). Pure so it is
    /// unit-testable without a display.
    pub fn for_status(status: ForwardStatus) -> StatusDot {
        match status {
            ForwardStatus::Connected => StatusDot::Green,
            ForwardStatus::Connecting | ForwardStatus::Disconnecting => StatusDot::Yellow,
            ForwardStatus::Error => StatusDot::Red,
            ForwardStatus::Disconnected => StatusDot::Grey,
        }
    }
}

const DOT_GREEN_PNG: &[u8] = include_bytes!("../../../assets/icons/dot_green.png");
const DOT_YELLOW_PNG: &[u8] = include_bytes!("../../../assets/icons/dot_yellow.png");
const DOT_RED_PNG: &[u8] = include_bytes!("../../../assets/icons/dot_red.png");
const DOT_GREY_PNG: &[u8] = include_bytes!("../../../assets/icons/dot_grey.png");

/// The embedded PNG bytes backing a [`StatusDot`].
fn dot_bytes(dot: StatusDot) -> &'static [u8] {
    match dot {
        StatusDot::Green => DOT_GREEN_PNG,
        StatusDot::Yellow => DOT_YELLOW_PNG,
        StatusDot::Red => DOT_RED_PNG,
        StatusDot::Grey => DOT_GREY_PNG,
    }
}

/// Decode the status-dot PNG for `status` into a Tauri [`Image`] for use as an
/// `IconMenuItem` leading icon.
pub fn load_dot(status: ForwardStatus) -> tauri::Result<Image<'static>> {
    Image::from_bytes(dot_bytes(StatusDot::for_status(status)))
}

/// Apply the correct icon for `count` connected tunnels to the tray, marking it
/// a template image on macOS. Must run on the main thread (AppKit) — callers use
/// `AppHandle::run_on_main_thread`. Failures are logged, never fatal.
pub fn update_tray_icon(app: &tauri::AppHandle, tray_id: &str, count: usize) {
    let Some(tray) = app.tray_by_id(tray_id) else {
        tracing::warn!(tray_id, "tray icon not found; cannot update icon");
        return;
    };
    let kind = tray_icon_for_count(count);
    match load_image(kind) {
        Ok(img) => {
            if let Err(e) = tray.set_icon(Some(img)) {
                tracing::error!(error = %e, "failed to set tray icon");
            }
            // macOS: template images auto-tint for light/dark menu bars.
            #[cfg(target_os = "macos")]
            if let Err(e) = tray.set_icon_as_template(true) {
                tracing::error!(error = %e, "failed to set tray icon as template");
            }
        }
        Err(e) => tracing::error!(error = %e, ?kind, "failed to decode tray icon PNG"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_is_idle() {
        assert_eq!(tray_icon_for_count(0), TrayIcon::Idle);
    }

    #[test]
    fn one_through_nine_map_to_badge() {
        for n in 1..=9usize {
            assert_eq!(tray_icon_for_count(n), TrayIcon::Badge(n as u8));
        }
    }

    #[test]
    fn clamps_at_nine() {
        assert_eq!(tray_icon_for_count(9), TrayIcon::Badge(9));
        assert_eq!(tray_icon_for_count(10), TrayIcon::Badge(9));
        assert_eq!(tray_icon_for_count(100), TrayIcon::Badge(9));
        assert_eq!(tray_icon_for_count(usize::MAX), TrayIcon::Badge(9));
    }

    #[test]
    fn every_selectable_icon_has_embedded_bytes() {
        // Idle + each badge 1..=9 must resolve to non-empty PNG bytes.
        assert!(!png_bytes(TrayIcon::Idle).is_empty());
        for n in 1..=MAX_BADGE {
            let bytes = png_bytes(tray_icon_for_count(n));
            assert!(!bytes.is_empty(), "badge {n} has no bytes");
        }
    }

    #[test]
    fn status_dot_matches_palette() {
        assert_eq!(
            StatusDot::for_status(ForwardStatus::Connected),
            StatusDot::Green
        );
        assert_eq!(
            StatusDot::for_status(ForwardStatus::Connecting),
            StatusDot::Yellow
        );
        assert_eq!(
            StatusDot::for_status(ForwardStatus::Disconnecting),
            StatusDot::Yellow
        );
        assert_eq!(StatusDot::for_status(ForwardStatus::Error), StatusDot::Red);
        assert_eq!(
            StatusDot::for_status(ForwardStatus::Disconnected),
            StatusDot::Grey
        );
    }

    #[test]
    fn every_status_dot_has_embedded_bytes() {
        for dot in [
            StatusDot::Green,
            StatusDot::Yellow,
            StatusDot::Red,
            StatusDot::Grey,
        ] {
            assert!(!dot_bytes(dot).is_empty(), "dot {dot:?} has no bytes");
        }
    }
}
