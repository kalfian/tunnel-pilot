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

/// Which tray icon asset to display for the current tunnel picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayIcon {
    /// No tunnels connected and nothing transitional — the grey idle icon.
    Idle,
    /// One or more connected (and nothing transitional) — the numbered badge
    /// (1..=9, clamped).
    Badge(u8),
    /// At least one tunnel is in a transitional state (connecting /
    /// disconnecting). Rendered as the base glyph with **big bright ticking dots
    /// in the bottom-right corner** (`●`/`●●`/`●●●`, non-template so they stay
    /// visible); this variant maps to the first frame when a single still is
    /// needed.
    Connecting,
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

/// Pure tray-icon precedence including the transitional state (spec 03 §10 +
/// connecting indicator): a transitional tunnel (connecting/disconnecting) wins
/// over the connected count so the user always sees "something is working";
/// otherwise fall back to the connected-count badge / idle. Unit-tested.
pub fn tray_icon_for_state(transitional: bool, connected: usize) -> TrayIcon {
    if transitional {
        TrayIcon::Connecting
    } else {
        tray_icon_for_count(connected)
    }
}

/// Number of frames in the connecting corner-dots animation (`·` → `··` → `···`).
pub const CONNECTING_FRAMES: usize = 3;

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

/// Connecting ticking-dots frames (index = frame): the base glyph (baked mid-gray
/// so it reads on light AND dark bars) with 1 → 2 → 3 big blue dots + white halo
/// in the bottom-right corner, growing left→right. **Non-template** colored PNGs
/// (see [`set_connecting_frame`]) so the dots stay bright instead of merging into
/// the glyph — the only tray icons that are not template images.
const CONNECTING_PNG: [&[u8]; CONNECTING_FRAMES] = [
    include_bytes!("../../../assets/icons/tray_icon_connecting_0.png"),
    include_bytes!("../../../assets/icons/tray_icon_connecting_1.png"),
    include_bytes!("../../../assets/icons/tray_icon_connecting_2.png"),
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
        // A single still of the connecting state = the first (one-dot) frame.
        TrayIcon::Connecting => CONNECTING_PNG[0],
    }
}

/// The embedded PNG bytes for connecting `frame` (wraps defensively).
fn connecting_bytes(frame: usize) -> &'static [u8] {
    CONNECTING_PNG[frame % CONNECTING_FRAMES]
}

/// Decode the embedded PNG for `icon` into a Tauri [`Image`] (needs the
/// `image-png` cargo feature, enabled in `Cargo.toml`).
pub fn load_image(icon: TrayIcon) -> tauri::Result<Image<'static>> {
    Image::from_bytes(png_bytes(icon))
}

/// Decode the connecting `frame` PNG into a Tauri [`Image`].
pub fn load_connecting_frame(frame: usize) -> tauri::Result<Image<'static>> {
    Image::from_bytes(connecting_bytes(frame))
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

/// Paint one connecting corner-dots `frame` on the tray. Drawn **non-template**
/// (`set_icon_as_template(false)` on macOS) so the bright blue dots + white halo
/// keep their color instead of being flattened to the menu-bar tint — a black
/// template dot would merge into the black glyph and vanish. The glyph itself is
/// baked as a mid-gray so it stays legible on both light and dark bars.
/// [`update_tray_icon`] flips template back to `true` when settling to the
/// count/idle icon. Must run on the main thread (AppKit) — callers dispatch via
/// `AppHandle::run_on_main_thread`. Failures are logged, never fatal.
pub fn set_connecting_frame(app: &tauri::AppHandle, tray_id: &str, frame: usize) {
    let Some(tray) = app.tray_by_id(tray_id) else {
        tracing::warn!(tray_id, "tray icon not found; cannot set connecting frame");
        return;
    };
    match load_connecting_frame(frame) {
        Ok(img) => {
            if let Err(e) = tray.set_icon(Some(img)) {
                tracing::error!(error = %e, "failed to set connecting tray icon");
            }
            // Non-template: render the frame's own colors (dots must not merge
            // with the glyph). Settling re-enables template via update_tray_icon.
            #[cfg(target_os = "macos")]
            if let Err(e) = tray.set_icon_as_template(false) {
                tracing::error!(error = %e, "failed to clear template for connecting icon");
            }
        }
        Err(e) => tracing::error!(error = %e, frame, "failed to decode connecting frame PNG"),
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
    fn transitional_state_wins_over_count() {
        // Connecting/disconnecting always shows the connecting indicator,
        // regardless of how many are connected — the user must see activity.
        assert_eq!(tray_icon_for_state(true, 0), TrayIcon::Connecting);
        assert_eq!(tray_icon_for_state(true, 1), TrayIcon::Connecting);
        assert_eq!(tray_icon_for_state(true, 9), TrayIcon::Connecting);
        assert_eq!(tray_icon_for_state(true, 100), TrayIcon::Connecting);
    }

    #[test]
    fn non_transitional_falls_back_to_count() {
        // With nothing transitional it is exactly the count badge / idle.
        assert_eq!(tray_icon_for_state(false, 0), TrayIcon::Idle);
        assert_eq!(tray_icon_for_state(false, 1), TrayIcon::Badge(1));
        assert_eq!(tray_icon_for_state(false, 5), TrayIcon::Badge(5));
        assert_eq!(tray_icon_for_state(false, 42), TrayIcon::Badge(9));
    }

    #[test]
    fn every_connecting_frame_has_embedded_bytes() {
        for f in 0..CONNECTING_FRAMES {
            assert!(!connecting_bytes(f).is_empty(), "frame {f} has no bytes");
        }
        // The Connecting still resolves to the first frame's bytes.
        assert_eq!(png_bytes(TrayIcon::Connecting), connecting_bytes(0));
    }

    #[test]
    fn connecting_frame_index_wraps() {
        // The animator advances with modulo; an over-range frame must not panic.
        assert_eq!(connecting_bytes(CONNECTING_FRAMES), connecting_bytes(0));
        assert_eq!(connecting_bytes(CONNECTING_FRAMES + 1), connecting_bytes(1));
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
