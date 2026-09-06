//! Known-device database for the JA11 / KTMicro KT02H20 family.
//!
//! Only the FiiO JA11's VID/PID is hardware-confirmed (via `ktflash`,
//! `2972:0102`). The clone entries share the same KT02H20 silicon and *likely*
//! speak the same runtime protocol, but their USB IDs here are **placeholders /
//! unconfirmed** and flagged as such — populate them as real captures land.

use serde::Serialize;

/// A device this tool knows (or suspects it knows) how to talk to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct KnownDevice {
    /// USB vendor id.
    pub vid: u16,
    /// USB product id.
    pub pid: u16,
    /// Human-readable model name.
    pub name: &'static str,
    /// Whether this VID/PID has been confirmed against real hardware.
    pub confirmed: bool,
}

/// The device table. The first entry is the confirmed JA11.
pub const KNOWN_DEVICES: &[KnownDevice] = &[
    KnownDevice {
        vid: 0x2972,
        pid: 0x0102,
        name: "FiiO JA11",
        confirmed: true,
    },
    // ── Clone family (KT02H20 silicon) — VID/PIDs UNCONFIRMED ─────────────────
    KnownDevice {
        vid: 0x2972,
        pid: 0x0000,
        name: "JCALLY JM12 (unconfirmed)",
        confirmed: false,
    },
    KnownDevice {
        vid: 0x2972,
        pid: 0x0001,
        name: "Moondrop dongle (KT02H20, unconfirmed)",
        confirmed: false,
    },
];

/// Look up a device by VID/PID.
pub fn identify(vid: u16, pid: u16) -> Option<&'static KnownDevice> {
    KNOWN_DEVICES.iter().find(|d| d.vid == vid && d.pid == pid)
}

/// A display label for a VID/PID, falling back to the raw ids if unknown.
pub fn label(vid: u16, pid: u16) -> String {
    match identify(vid, pid) {
        Some(d) if d.confirmed => d.name.to_string(),
        Some(d) => format!("{} [unconfirmed]", d.name),
        None => format!("unknown device {vid:#06x}:{pid:#06x}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ja11_is_confirmed() {
        let d = identify(0x2972, 0x0102).unwrap();
        assert_eq!(d.name, "FiiO JA11");
        assert!(d.confirmed);
    }

    #[test]
    fn unknown_falls_back_to_ids() {
        assert!(label(0xDEAD, 0xBEEF).contains("0xdead"));
    }

    #[test]
    fn clone_is_flagged_unconfirmed() {
        let l = label(0x2972, 0x0000);
        assert!(l.contains("unconfirmed"));
    }
}
