//! Keep macOS from App Napping Stow while it sits in the menu bar: napped apps get
//! their timers throttled, so reminders would fire late and background jobs crawl.
//! `NSAppSleepDisabled` in Info.plist is the static half; this is the runtime half.

/// Why the activity is held, as shown in Activity Monitor.
pub const REASON: &str = "Stow reminders and background jobs";

/// `NSActivityBackground`: not latency-critical, no sleep prevention, just no App Nap.
#[cfg(target_os = "macos")]
pub fn activity_options() -> objc2_foundation::NSActivityOptions {
    objc2_foundation::NSActivityOptions::Background
}

/// Begin a process-lifetime activity. The token is intentionally leaked: ending it
/// would re-enable App Nap, and the process exit ends it anyway.
#[cfg(target_os = "macos")]
pub fn disable_app_nap() {
    use objc2_foundation::{NSProcessInfo, NSString};
    let reason = NSString::from_str(REASON);
    let token =
        NSProcessInfo::processInfo().beginActivityWithOptions_reason(activity_options(), &reason);
    std::mem::forget(token);
}

#[cfg(not(target_os = "macos"))]
pub fn disable_app_nap() {}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    #[test]
    fn activity_is_background_without_latency_critical_or_sleep_prevention() {
        use objc2_foundation::NSActivityOptions;
        let o = super::activity_options();
        assert_eq!(o, NSActivityOptions::Background);
        assert!(!o.contains(NSActivityOptions::LatencyCritical));
        assert!(!o.contains(NSActivityOptions::IdleSystemSleepDisabled));
        assert!(!o.contains(NSActivityOptions::IdleDisplaySleepDisabled));
        assert!(!super::REASON.is_empty());
    }
}
