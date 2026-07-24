//! Dynamic tray-icon selection: idle grey / badge 1–9 (clamp), template images
//! on macOS (spec 03 §10).
//!
//! The count→asset mapping is a pure function ([`tray_icon_for_count`]) so it is
//! unit-testable without a display. The PNG bytes are embedded at compile time
//! (`include_bytes!`) so the icons are available identically in dev, tests, and
//! every bundle regardless of the runtime working directory.
//!
//! NOTE (M3): the embedded PNGs are the v1 pre-colored tray assets reused as
//! placeholders. On Windows/Linux they render as-is (blue badge). On macOS they
//! are marked as **template images** per spec — macOS then tints them by the
//! menu-bar appearance using the alpha channel. Dedicated monochrome template
//! art (where the count digit reads crisply in both light and dark menu bars) is
//! a follow-up for the design agent; the count→asset selection here is final.

use tauri::image::Image;

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
}
