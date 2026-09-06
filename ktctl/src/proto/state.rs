//! Device-state channel models (RE write-up §4b, roadmap Phase 3 items 6-7).
//!
//! These opcodes share the exact same wire frame as the EQ channel but expose
//! the JA11 Status screen: volume, sample rate/format, firmware version, in-line
//! mic detect, and the UAC 1.0/2.0 mode toggle. All still provisional pending
//! hardware validation.

use serde::{Deserialize, Serialize};

/// USB Audio Class mode. The JA11's Status screen lets you switch between these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UacMode {
    /// UAC 1.0 (broader host compatibility, capped sample rates).
    Uac1,
    /// UAC 2.0 (high sample rates; the JA11 default seen in screenshots).
    Uac2,
    /// An unrecognised mode byte.
    Raw(u8),
}

impl UacMode {
    /// Encode to the single `0x20` payload byte (`1` = UAC1, `2` = UAC2).
    pub fn to_byte(self) -> u8 {
        match self {
            UacMode::Uac1 => 1,
            UacMode::Uac2 => 2,
            UacMode::Raw(b) => b,
        }
    }

    /// Decode from the single `0x20` payload byte.
    pub fn from_byte(b: u8) -> Self {
        match b {
            1 => UacMode::Uac1,
            2 => UacMode::Uac2,
            other => UacMode::Raw(other),
        }
    }

    /// Parse from a CLI string (`1`, `2`, `uac1`, `uac2`).
    pub fn parse(s: &str) -> Result<Self, String> {
        match s
            .to_ascii_lowercase()
            .replace("uac", "")
            .replace('.', "")
            .trim()
        {
            "1" | "10" => Ok(UacMode::Uac1),
            "2" | "20" => Ok(UacMode::Uac2),
            other => Err(format!("invalid UAC mode '{other}' (use 1 or 2)")),
        }
    }
}

impl std::fmt::Display for UacMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UacMode::Uac1 => write!(f, "UAC 1.0"),
            UacMode::Uac2 => write!(f, "UAC 2.0"),
            UacMode::Raw(b) => write!(f, "UAC raw({b:#04x})"),
        }
    }
}

/// The 15-entry PCM/DSD sample-rate/format table the `0x09` opcode indexes into.
///
/// The exact ordering is **inferred** — a plausible ascending PCM run followed
/// by DSD rates, matching the "384k" value seen on FIIO's Status screenshot at
/// some index. Hardware must confirm both the ordering and the count.
pub const SAMPLE_RATE_TABLE: [&str; 15] = [
    "44.1k", "48k", "88.2k", "96k", "176.4k", "192k", "352.8k", "384k", "705.6k", "768k", "DSD64",
    "DSD128", "DSD256", "DSD512", "unknown",
];

/// Resolve a `0x09` table index to a human-readable label.
pub fn sample_rate_label(index: u8) -> &'static str {
    SAMPLE_RATE_TABLE
        .get(index as usize)
        .copied()
        .unwrap_or("out-of-range")
}

/// Decode a firmware-version payload (`"{major}.{minor}"`) from two bytes.
///
/// Reported by `0x0B`. The Status screen shows e.g. `1.4`; note this may differ
/// from the flashable firmware build string `ktflash` sees (`V2.2`) — an open
/// discrepancy tracked in `docs/PROTOCOL.md`.
pub fn firmware_version(payload: &[u8]) -> String {
    match payload {
        [major, minor, ..] => format!("{major}.{minor}"),
        [major] => format!("{major}.0"),
        [] => "unknown".to_string(),
    }
}

/// A snapshot of the JA11 Status screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceState {
    /// Device volume (raw device units; screenshot showed `60`).
    pub volume: u8,
    /// Sample-rate/format table index and its resolved label.
    pub sample_rate_index: u8,
    /// Human-readable sample rate (resolved from [`SAMPLE_RATE_TABLE`]).
    pub sample_rate: String,
    /// Firmware version string, e.g. `"1.4"`.
    pub firmware: String,
    /// In-line microphone detected.
    pub mic_present: bool,
    /// Current UAC mode.
    pub uac: UacMode,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uac_round_trip() {
        assert_eq!(UacMode::from_byte(1), UacMode::Uac1);
        assert_eq!(UacMode::from_byte(2), UacMode::Uac2);
        assert_eq!(UacMode::from_byte(9), UacMode::Raw(9));
        assert_eq!(UacMode::Uac2.to_byte(), 2);
    }

    #[test]
    fn uac_parse() {
        assert_eq!(UacMode::parse("1"), Ok(UacMode::Uac1));
        assert_eq!(UacMode::parse("UAC2"), Ok(UacMode::Uac2));
        assert_eq!(UacMode::parse("2.0"), Ok(UacMode::Uac2));
        assert!(UacMode::parse("3").is_err());
    }

    #[test]
    fn sample_rate_labels() {
        assert_eq!(sample_rate_label(7), "384k");
        assert_eq!(sample_rate_label(99), "out-of-range");
    }

    #[test]
    fn firmware_formatting() {
        assert_eq!(firmware_version(&[1, 4]), "1.4");
        assert_eq!(firmware_version(&[2]), "2.0");
        assert_eq!(firmware_version(&[]), "unknown");
    }
}
