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
