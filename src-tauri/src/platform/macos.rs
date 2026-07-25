//! macOS-only cocoa/objc shims for behavior Tauri does not expose directly.

/// Bring the app to the foreground WITHOUT changing the activation policy
/// (BUG 1).
///
/// When `showInDock` is off the app runs as an `Accessory` agent (no dock icon).
/// `window.set_focus()` alone cannot steal focus from the currently-frontmost
/// app in that state, and flipping the activation policy to `Regular` to front
/// it would order an already-open window out (the "vanish" bug — the vanish is
/// the POLICY TRANSITION, not activation). `NSApplication`
/// `activateIgnoringOtherApps:` fronts the app+window as a plain *activation*:
/// it does NOT add a dock icon and does NOT trigger the policy-transition
/// vanish.
///
/// This is a minimal `objc2` `msg_send!` shim — objc2 is already in the
/// dependency tree via Tauri. AppKit UI calls must run on the main thread; the
/// caller dispatches this via `AppHandle::run_on_main_thread`.
pub fn activate_ignoring_other_apps() {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};

    // SAFETY: `+[NSApplication sharedApplication]` returns the shared app
    // instance (non-null once the app has launched, which it has by the time any
    // window is shown). `-activateIgnoringOtherApps:` takes a single `BOOL` and
    // returns `void`; we call both with correct signatures on the main thread.
    unsafe {
        let ns_app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        if !ns_app.is_null() {
            let _: () = msg_send![ns_app, activateIgnoringOtherApps: true];
        }
    }
}

// --- tray-popover NSPanel shim ---------------------------------------------

/// `NSWindowStyleMaskNonactivatingPanel` — a panel that can become key WITHOUT
/// activating the owning application, so clicking the popover never fronts the
/// app or steals the previous app's activation (AppKit `NSWindow.StyleMask`).
const NS_WINDOW_STYLE_MASK_NONACTIVATING_PANEL: usize = 1 << 7;
/// `NSWindowStyleMaskUtilityWindow` — utility ("panel") chrome/behaviour.
const NS_WINDOW_STYLE_MASK_UTILITY_WINDOW: usize = 1 << 4;

/// `NSWindowCollectionBehaviorCanJoinAllSpaces` — visible on every Space so the
/// popover follows the user rather than being tied to one desktop.
const NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES: usize = 1 << 0;
/// `NSWindowCollectionBehaviorTransient` — excluded from Mission Control / the
/// window-cycle so the popover never shows up as a "real" window.
const NS_WINDOW_COLLECTION_BEHAVIOR_TRANSIENT: usize = 1 << 3;
/// `NSWindowCollectionBehaviorFullScreenAuxiliary` — may float over another
/// app's full-screen Space (so the popover still appears above a full-screen app).
const NS_WINDOW_COLLECTION_BEHAVIOR_FULLSCREEN_AUXILIARY: usize = 1 << 8;

/// `NSStatusWindowLevel` (Carbon `kCGStatusWindowLevel` = 25): sit above normal
/// and floating windows, level with the menu-bar extras the popover drops from.
const NS_STATUS_WINDOW_LEVEL: isize = 25;

/// Turn a plain Tauri `NSWindow` into a non-activating tray popover (best-effort,
/// macOS only).
///
/// Tauri backs every window with an `NSWindow`, not an `NSPanel`, and exposes no
/// API for the non-activating-panel behaviour a tray popover needs. This is a
/// MINIMAL `objc2` `msg_send!` shim (objc2 is already in the tree via Tauri) that:
///
/// - adds the **non-activating panel** + **utility** style-mask bits so the
///   window can become key without activating the app (clicks don't front the
///   app / don't add it to the app-switcher when it runs `Regular`);
/// - sets the **status window level** so it floats above ordinary windows;
/// - sets a **transient / all-Spaces** collection behaviour so it stays out of
///   Mission Control and the window-cycle and follows the active Space.
///
/// `ns_window` is the raw `*mut NSWindow` from `WebviewWindow::ns_window()`.
/// Guarded against a null pointer; every `msg_send!` targets a documented AppKit
/// selector with the correct signature. AppKit UI mutation must run on the main
/// thread — the caller invokes this from the Tauri `setup` hook (main thread).
///
/// Best-effort: if the runtime ignores the style-mask change on a non-`NSPanel`
/// window, the level + collection behaviour still apply; the definitive check
/// needs a real display (see the module owner's report).
pub fn make_nonactivating_popover(ns_window: *mut std::ffi::c_void) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    if ns_window.is_null() {
        tracing::warn!("popover NSWindow pointer is null; nonactivating shim skipped");
        return;
    }

    // SAFETY: `ns_window` is a valid `*mut NSWindow` handed back by Tauri's
    // `WebviewWindow::ns_window()`. Each selector below exists on `NSWindow` with
    // the signature used: `-styleMask`/`-collectionBehavior` return `NSUInteger`
    // (usize); `-setStyleMask:`/`-setCollectionBehavior:` take `NSUInteger`;
    // `-setLevel:` takes `NSInteger` (isize); `-setHidesOnDeactivate:` takes a
    // `BOOL`. All run on the main thread (called from the `setup` hook).
    unsafe {
        let win: *mut AnyObject = ns_window.cast();

        let style: usize = msg_send![win, styleMask];
        let style =
            style | NS_WINDOW_STYLE_MASK_NONACTIVATING_PANEL | NS_WINDOW_STYLE_MASK_UTILITY_WINDOW;
        let _: () = msg_send![win, setStyleMask: style];

        let _: () = msg_send![win, setLevel: NS_STATUS_WINDOW_LEVEL];

        let behavior: usize = NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES
            | NS_WINDOW_COLLECTION_BEHAVIOR_TRANSIENT
            | NS_WINDOW_COLLECTION_BEHAVIOR_FULLSCREEN_AUXILIARY;
        let _: () = msg_send![win, setCollectionBehavior: behavior];

        // We manage dismissal ourselves (blur-to-dismiss); don't let AppKit hide
        // the popover on app deactivation as well (would double-fire with blur).
        let _: () = msg_send![win, setHidesOnDeactivate: false];
    }
}
