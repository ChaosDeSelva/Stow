//! macOS privacy permissions: Automation (Apple Events to Chrome) and the microphone.
//! Status checks never prompt; the `request_*` functions may, and block until answered,
//! so callers run them on the blocking pool.
//!
//! This is the only module allowed to use `unsafe`: it calls `CoreServices`'
//! `AEDeterminePermissionToAutomateTarget` and `AVFoundation` through FFI.
#![allow(unsafe_code)]

use serde::Serialize;

/// Automation consent for sending Apple Events to Chrome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Automation {
    Granted,
    Denied,
    NotDetermined,
    ChromeNotRunning,
}

/// Microphone access (`AVAuthorizationStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Microphone {
    Granted,
    Denied,
    NotDetermined,
    Restricted,
}

/// `permission_status()` result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PermissionStatus {
    pub automation: Automation,
    pub microphone: Microphone,
}

const NO_ERR: i32 = 0;
const PROC_NOT_FOUND: i32 = -600;
const ERR_AE_EVENT_NOT_PERMITTED: i32 = -1743;
const ERR_AE_EVENT_WOULD_REQUIRE_USER_CONSENT: i32 = -1744;

/// Map an `AEDeterminePermissionToAutomateTarget` `OSStatus`.
pub fn map_automation_status(code: i32) -> Automation {
    match code {
        NO_ERR => Automation::Granted,
        ERR_AE_EVENT_NOT_PERMITTED => Automation::Denied,
        ERR_AE_EVENT_WOULD_REQUIRE_USER_CONSENT => Automation::NotDetermined,
        PROC_NOT_FOUND => Automation::ChromeNotRunning,
        other => {
            tracing::warn!(code = other, "unexpected automation permission status");
            Automation::NotDetermined
        }
    }
}

/// Map an `AVAuthorizationStatus` raw value.
pub fn map_microphone_status(raw: isize) -> Microphone {
    match raw {
        1 => Microphone::Restricted,
        2 => Microphone::Denied,
        3 => Microphone::Granted,
        _ => Microphone::NotDetermined,
    }
}

/// System Settings URL for a privacy pane name from the contract.
pub fn privacy_settings_url(pane: &str) -> Option<&'static str> {
    match pane {
        "automation" => {
            Some("x-apple.systempreferences:com.apple.preference.security?Privacy_Automation")
        }
        "microphone" => {
            Some("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
        }
        // Notification permission lives in the Notifications pane, not under Privacy.
        "notifications" => Some("x-apple.systempreferences:com.apple.preference.notifications"),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::c_void;
    use std::time::Duration;

    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_av_foundation::{AVCaptureDevice, AVMediaTypeAudio};

    /// `AEDesc` from AEDataModel.h, which is declared under `#pragma pack(push, 2)`.
    #[repr(C, packed(2))]
    struct AEDesc {
        descriptor_type: u32,
        data_handle: *mut c_void,
    }

    const fn four_cc(s: [u8; 4]) -> u32 {
        u32::from_be_bytes(s)
    }
    const TYPE_APPLICATION_BUNDLE_ID: u32 = four_cc(*b"bund");
    const TYPE_WILD_CARD: u32 = four_cc(*b"****");

    #[link(name = "CoreServices", kind = "framework")]
    extern "C" {
        fn AECreateDesc(
            type_code: u32,
            data_ptr: *const c_void,
            data_size: isize,
            result: *mut AEDesc,
        ) -> i16;
        fn AEDisposeDesc(desc: *mut AEDesc) -> i16;
        fn AEDeterminePermissionToAutomateTarget(
            target: *const AEDesc,
            event_class: u32,
            event_id: u32,
            ask_user_if_needed: u8,
        ) -> i32;
    }

    const CHROME_BUNDLE_ID: &str = "com.google.Chrome";

    /// Raw `OSStatus` for sending any Apple Event to Chrome. With `ask`, macOS may show
    /// the consent prompt and this blocks until the user answers.
    pub fn automation_code(ask: bool) -> i32 {
        let mut desc = AEDesc {
            descriptor_type: 0,
            data_handle: std::ptr::null_mut(),
        };
        let bytes = CHROME_BUNDLE_ID.as_bytes();
        // SAFETY: `bytes` outlives the call (AECreateDesc copies it), the size matches,
        // and `desc` is a valid out-pointer.
        let err = unsafe {
            AECreateDesc(
                TYPE_APPLICATION_BUNDLE_ID,
                bytes.as_ptr().cast(),
                isize::try_from(bytes.len()).unwrap_or(isize::MAX),
                &raw mut desc,
            )
        };
        if err != 0 {
            return i32::from(err);
        }
        // SAFETY: `desc` was initialised by AECreateDesc above and is disposed exactly once.
        unsafe {
            let status = AEDeterminePermissionToAutomateTarget(
                &raw const desc,
                TYPE_WILD_CARD,
                TYPE_WILD_CARD,
                u8::from(ask),
            );
            AEDisposeDesc(&raw mut desc);
            status
        }
    }

    pub fn microphone_raw() -> isize {
        // SAFETY: AVMediaTypeAudio is a framework constant; audio is a valid media type
        // for this class method (anything else would throw).
        unsafe {
            match AVMediaTypeAudio {
                Some(media) => AVCaptureDevice::authorizationStatusForMediaType(media).0,
                None => 0,
            }
        }
    }

    /// Ask for microphone access (shows the prompt only when not yet determined) and
    /// wait up to two minutes for the answer.
    pub fn request_microphone() {
        let (tx, rx) = std::sync::mpsc::channel::<bool>();
        let block = RcBlock::new(move |granted: Bool| {
            let _ = tx.send(granted.as_bool());
        });
        // SAFETY: as in `microphone_raw`; the block is retained by AVFoundation for the
        // duration of the request.
        unsafe {
            let Some(media) = AVMediaTypeAudio else {
                return;
            };
            AVCaptureDevice::requestAccessForMediaType_completionHandler(media, &block);
        }
        let _ = rx.recv_timeout(Duration::from_secs(120));
    }
}

#[cfg(target_os = "macos")]
pub fn automation(ask: bool) -> Automation {
    map_automation_status(mac::automation_code(ask))
}

#[cfg(target_os = "macos")]
pub fn microphone() -> Microphone {
    map_microphone_status(mac::microphone_raw())
}

#[cfg(target_os = "macos")]
pub fn request_microphone() -> Microphone {
    mac::request_microphone();
    microphone()
}

#[cfg(not(target_os = "macos"))]
pub fn automation(_ask: bool) -> Automation {
    Automation::Granted
}

#[cfg(not(target_os = "macos"))]
pub fn microphone() -> Microphone {
    Microphone::Granted
}

#[cfg(not(target_os = "macos"))]
pub fn request_microphone() -> Microphone {
    Microphone::Granted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automation_status_mapping() {
        assert_eq!(map_automation_status(0), Automation::Granted);
        assert_eq!(map_automation_status(-1743), Automation::Denied);
        assert_eq!(map_automation_status(-1744), Automation::NotDetermined);
        assert_eq!(map_automation_status(-600), Automation::ChromeNotRunning);
        assert_eq!(map_automation_status(-50), Automation::NotDetermined);
    }

    #[test]
    fn microphone_status_mapping() {
        assert_eq!(map_microphone_status(0), Microphone::NotDetermined);
        assert_eq!(map_microphone_status(1), Microphone::Restricted);
        assert_eq!(map_microphone_status(2), Microphone::Denied);
        assert_eq!(map_microphone_status(3), Microphone::Granted);
    }

    #[test]
    fn permission_status_serializes_snake_case() {
        let v = serde_json::to_value(PermissionStatus {
            automation: Automation::ChromeNotRunning,
            microphone: Microphone::NotDetermined,
        })
        .unwrap();
        assert_eq!(
            v,
            serde_json::json!({"automation": "chrome_not_running", "microphone": "not_determined"})
        );
    }

    #[test]
    fn privacy_urls() {
        assert!(privacy_settings_url("automation")
            .unwrap()
            .ends_with("Privacy_Automation"));
        assert!(privacy_settings_url("microphone")
            .unwrap()
            .ends_with("Privacy_Microphone"));
        assert!(privacy_settings_url("notifications")
            .unwrap()
            .contains("notifications"));
        assert_eq!(privacy_settings_url("camera"), None);
        assert_eq!(privacy_settings_url("automation; rm -rf"), None);
    }

    /// Calls the real API without prompting; the answer depends on the machine.
    #[cfg(target_os = "macos")]
    #[test]
    fn automation_and_microphone_status_do_not_crash() {
        let _ = automation(false);
        let _ = microphone();
    }
}
