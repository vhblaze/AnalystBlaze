//! Correlates three things Windows reports as unrelated problems into the
//! one fault they usually are together: a USB-attached 4G/5G modem whose
//! USB link keeps dropping. Built from a real case (2026-09-17): a user
//! with a real 4G/5G antenna saw two generic "device has a problem" cards,
//! and the disable/enable cycle (correctly) didn't help either, because
//! neither device was a driver-state problem:
//!
//! - `USB\VID_0000&PID_0002` "Unknown USB Device (Device Descriptor
//!   Request Failed)" / `PID_0001` "(Port Reset Failed)", Code 43 - the
//!   placeholder ID Windows assigns when a USB device is present on the
//!   port but never answers the very first enumeration request. That is a
//!   physical-layer failure (port, cable, power), not something software
//!   can clear.
//! - `ROOT\NET\...` "Generic Mobile Broadband Adapter", Code 10 - not
//!   hardware at all: the virtual adapter Windows' mobile-broadband stack
//!   creates to represent a modem. It can't start because the real modem
//!   behind it never finished enumerating.
//! - N hidden "Generic Mobile Broadband Adapter" network interfaces in
//!   "Not Present" state (`Celular`, `Celular 2`, ... - 18 on that
//!   machine). Every time the modem drops and re-enumerates, Windows makes
//!   a new one and orphans the old, so the count is a direct record of how
//!   many times the link has flapped.
//!
//! Detection is deterministic and cheap (three PnP/WMI reads), and only the
//! last two lookups run when the first signal is present at all, so a
//! healthy machine pays nothing extra.

use serde::{Deserialize, Serialize};

use super::advanced::FailingDevice;

/// Fewer hidden WWAN interfaces than this could just be a modem that was
/// used once and removed; at this many, the link has clearly been flapping.
pub const GHOST_WWAN_THRESHOLD: u32 = 3;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsbLinkDetails {
    /// Port index on the parent hub, parsed from the instance ID's last
    /// `&`-separated segment (`6&C1A2E2F&0&3` -> 3).
    pub port: Option<u32>,
    /// Parent is a USB root hub, i.e. plugged straight into a controller
    /// port with no intermediate hub.
    pub on_root_hub: bool,
    /// "3.0" / "2.0" from the root hub's instance ID (`USB\ROOT_HUB30` vs
    /// `USB\ROOT_HUB20`) - the ID, not the localized friendly name.
    pub hub_version: Option<String>,
    /// The host controller's friendly name when the device sits on a root
    /// hub (e.g. "AMD USB 3.10 eXtensible Host Controller ...").
    pub controller: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModemUsbLinkIssue {
    pub usb_device_id: String,
    pub usb_device_name: Option<String>,
    pub link: UsbLinkDetails,
    /// The `ROOT\NET` virtual mobile-broadband adapter stuck on Code 10,
    /// when one exists.
    pub root_net_device_id: Option<String>,
    pub ghost_wwan_adapter_count: u32,
}

/// Windows' placeholder IDs for a USB device that failed enumeration -
/// PID_0001 "Port Reset Failed", PID_0002 "Device Descriptor Request Failed".
pub fn is_unenumerated_usb_device(device_id: &str) -> bool {
    let upper = device_id.to_ascii_uppercase();
    upper.starts_with(r"USB\VID_0000&PID_0001") || upper.starts_with(r"USB\VID_0000&PID_0002")
}

/// The virtual adapter the mobile-broadband stack root-enumerates - not a
/// piece of hardware, so a Code 10 here means "nothing real behind it".
pub fn is_root_net_adapter(device_id: &str) -> bool {
    device_id.to_ascii_uppercase().starts_with(r"ROOT\NET\")
}

/// Last `&`-separated segment of an instance ID's final path component.
pub fn usb_port_from_instance_id(device_id: &str) -> Option<u32> {
    device_id
        .rsplit('\\')
        .next()?
        .rsplit('&')
        .next()?
        .parse()
        .ok()
}

/// "3.0"/"2.0" from a root hub instance ID like `USB\ROOT_HUB30\5&...`.
pub fn hub_version_from_instance_id(parent_id: &str) -> Option<String> {
    let upper = parent_id.to_ascii_uppercase();
    if upper.contains("ROOT_HUB30") {
        Some("3.0".to_string())
    } else if upper.contains("ROOT_HUB20") {
        Some("2.0".to_string())
    } else {
        None
    }
}

pub fn is_root_hub_instance_id(parent_id: &str) -> bool {
    parent_id.to_ascii_uppercase().starts_with(r"USB\ROOT_HUB")
}

/// The correlation rule. `link_for` is only ever called for the one
/// unenumerated USB device (if any), and `ghost_count_fn` only when that
/// trigger exists - so the extra lookups cost nothing on a machine that
/// doesn't have this problem.
pub fn detect(
    failing: &[FailingDevice],
    link_for: impl Fn(&str) -> UsbLinkDetails,
    ghost_count_fn: impl Fn() -> u32,
) -> Option<ModemUsbLinkIssue> {
    let unknown_usb = failing
        .iter()
        .find(|device| device.problem_code == 43 && is_unenumerated_usb_device(&device.device_id))?;

    let root_net = failing
        .iter()
        .find(|device| device.problem_code == 10 && is_root_net_adapter(&device.device_id));
    let ghost_wwan_adapter_count = ghost_count_fn();

    if root_net.is_none() && ghost_wwan_adapter_count < GHOST_WWAN_THRESHOLD {
        return None;
    }

    Some(ModemUsbLinkIssue {
        usb_device_id: unknown_usb.device_id.clone(),
        usb_device_name: unknown_usb.name.clone(),
        link: link_for(&unknown_usb.device_id),
        root_net_device_id: root_net.map(|device| device.device_id.clone()),
        ghost_wwan_adapter_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(device_id: &str, problem_code: u32) -> FailingDevice {
        FailingDevice {
            name: None,
            device_id: device_id.to_string(),
            device_class: None,
            problem_code,
        }
    }

    fn no_link(_: &str) -> UsbLinkDetails {
        UsbLinkDetails::default()
    }

    #[test]
    fn the_real_case_correlates_into_one_issue() {
        let failing = vec![
            device(r"ROOT\NET\0000", 10),
            device(r"USB\VID_0000&PID_0002\6&C1A2E2F&0&3", 43),
        ];
        let issue = detect(&failing, no_link, || 18).expect("should correlate");
        assert_eq!(issue.usb_device_id, r"USB\VID_0000&PID_0002\6&C1A2E2F&0&3");
        assert_eq!(issue.root_net_device_id.as_deref(), Some(r"ROOT\NET\0000"));
        assert_eq!(issue.ghost_wwan_adapter_count, 18);
    }

    #[test]
    fn ghost_adapters_alone_are_enough_without_the_root_net_device() {
        let failing = vec![device(r"USB\VID_0000&PID_0001\6&C1A2E2F&0&3", 43)];
        assert!(detect(&failing, no_link, || GHOST_WWAN_THRESHOLD).is_some());
        assert!(
            detect(&failing, no_link, || GHOST_WWAN_THRESHOLD - 1).is_none(),
            "a couple of leftover interfaces is not evidence of flapping"
        );
    }

    #[test]
    fn no_unenumerated_usb_device_means_no_modem_link_diagnosis() {
        // A ROOT\NET Code 10 with no USB placeholder device could be a modem
        // that's simply unplugged - a different situation, not this one.
        let failing = vec![device(r"ROOT\NET\0000", 10)];
        assert!(detect(&failing, no_link, || 18).is_none());
        // And the ghost lookup must not even run in that case.
        let called = std::cell::Cell::new(false);
        let _ = detect(&failing, no_link, || {
            called.set(true);
            0
        });
        assert!(!called.get());
    }

    #[test]
    fn a_real_vendor_usb_device_with_code_43_is_not_this_pattern() {
        let failing = vec![
            device(r"ROOT\NET\0000", 10),
            device(r"USB\VID_8087&PID_0029\6&3365FBAF&0&9", 43),
        ];
        assert!(detect(&failing, no_link, || 18).is_none());
    }

    #[test]
    fn parses_port_and_hub_version_from_instance_ids() {
        assert_eq!(usb_port_from_instance_id(r"USB\VID_0000&PID_0002\6&C1A2E2F&0&3"), Some(3));
        assert_eq!(usb_port_from_instance_id(r"USB\VID_0000&PID_0002\6&C1A2E2F&0&12"), Some(12));
        assert_eq!(usb_port_from_instance_id(r"USB\VID_046D&PID_0892\EE5FDD1F"), None);
        assert_eq!(hub_version_from_instance_id(r"USB\ROOT_HUB30\5&2c35141&0&0").as_deref(), Some("3.0"));
        assert_eq!(hub_version_from_instance_id(r"USB\ROOT_HUB20\5&abc&0&0").as_deref(), Some("2.0"));
        assert_eq!(hub_version_from_instance_id(r"USB\VID_05E3&PID_0610\6&3365FBAF&0&11"), None);
        assert!(is_root_hub_instance_id(r"USB\ROOT_HUB30\5&2c35141&0&0"));
        assert!(!is_root_hub_instance_id(r"USB\VID_05E3&PID_0610\6&3365FBAF&0&11"));
    }
}

#[cfg(test)]
mod modem_link_live_check {
    #[test]
    #[ignore] // machine-dependent - run manually with --ignored
    fn live_modem_usb_link_issue_on_this_machine() {
        let telemetry = crate::telemetry::advanced::collect_advanced_telemetry(None);
        println!("failing_devices = {:#?}", telemetry.failing_devices);
        println!("modem_usb_link_issue = {:#?}", telemetry.modem_usb_link_issue);
    }
}
