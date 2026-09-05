//! PEQ band model and its fixed-point wire encoding.
//!
//! The `0x15` payload recovered in Phase 0 is:
//!
//! ```text
//! [ index, Q×100 (i16 BE), gain×10 dB (i16 BE), freq Hz (u16 BE), filterType ]
//! ```
//!
//! That is **7 bytes**: 1 (index) + 2 (Q) + 2 (gain) + 2 (freq) + ... wait —
//! the recovered layout carries `filterType` as a trailing byte, giving 8 bytes
//! total. Scaling: gain `×10`, Q `×100`, freq plain Hz. All still provisional
//! (roadmap Phase 0 / `docs/PROTOCOL.md`).

use serde::{Deserialize, Serialize};

use super::frame::{Frame, FrameError};
use super::opcode::CMD_PEQ_BAND;

/// The JA11 exposes a fixed 5-band parametric EQ.
pub const BAND_COUNT: usize = 5;

/// Number of payload bytes in a `0x15` band record.
pub const BAND_PAYLOAD_LEN: usize = 8;

/// Biquad filter type. Enum meanings are **inferred** — the real mapping is an
/// open Phase 0 question, so [`FilterType::Unknown`] preserves any byte we
/// don't recognise for lossless round-tripping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FilterType {
    /// Peaking / bell filter (the default and most common PEQ band).
    Peaking,
    /// Low-shelf filter.
    LowShelf,
    /// High-shelf filter.
    HighShelf,
    /// A filter-type byte we have not yet mapped; carries the raw value.
    Unknown(u8),
}

impl FilterType {
    /// Encode to the trailing wire byte.
    pub fn to_byte(self) -> u8 {
        match self {
            FilterType::Peaking => 0x00,
            FilterType::LowShelf => 0x01,
            FilterType::HighShelf => 0x02,
            FilterType::Unknown(b) => b,
        }
    }

    /// Decode from the trailing wire byte.
    pub fn from_byte(b: u8) -> Self {
        match b {
            0x00 => FilterType::Peaking,
            0x01 => FilterType::LowShelf,
            0x02 => FilterType::HighShelf,
            other => FilterType::Unknown(other),
        }
    }

    /// Parse from a CLI string (`peaking`, `low-shelf`, `high-shelf`, or a raw
    /// integer for the unknown case).
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().replace('_', "-").as_str() {
            "peaking" | "peak" | "pk" => Ok(FilterType::Peaking),
            "low-shelf" | "lowshelf" | "ls" => Ok(FilterType::LowShelf),
            "high-shelf" | "highshelf" | "hs" => Ok(FilterType::HighShelf),
            other => other
                .parse::<u8>()
                .map(FilterType::from_byte)
                .map_err(|_| format!("unknown filter type '{s}'")),
        }
    }
}

impl std::fmt::Display for FilterType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FilterType::Peaking => write!(f, "peaking"),
            FilterType::LowShelf => write!(f, "low-shelf"),
            FilterType::HighShelf => write!(f, "high-shelf"),
            FilterType::Unknown(b) => write!(f, "unknown({b:#04x})"),
        }
    }
}

/// A single parametric-EQ band in engineering units.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PeqBand {
    /// Band index (0..[`BAND_COUNT`]).
    pub index: u8,
    /// Centre / corner frequency in Hz.
    pub freq_hz: u16,
    /// Gain in dB (stored on the wire as `×10`).
    pub gain_db: f32,
    /// Quality factor Q (stored on the wire as `×100`).
    pub q: f32,
    /// Filter type.
    pub filter: FilterType,
}

impl PeqBand {
    /// A flat, disabled-looking band at a sensible default frequency.
    pub fn flat(index: u8) -> Self {
        PeqBand {
            index,
            freq_hz: default_freq_for_band(index),
            gain_db: 0.0,
            q: 1.0,
            filter: FilterType::Peaking,
        }
    }

    /// Serialize this band to the 8-byte `0x15` payload.
    pub fn to_payload(&self) -> [u8; BAND_PAYLOAD_LEN] {
        let q_fixed = (self.q * 100.0).round() as i16;
        let gain_fixed = (self.gain_db * 10.0).round() as i16;
        let mut out = [0u8; BAND_PAYLOAD_LEN];
        out[0] = self.index;
        out[1..3].copy_from_slice(&q_fixed.to_be_bytes());
        out[3..5].copy_from_slice(&gain_fixed.to_be_bytes());
        out[5..7].copy_from_slice(&self.freq_hz.to_be_bytes());
        out[7] = self.filter.to_byte();
        out
    }

    /// Parse a band from the 8-byte `0x15` payload.
    pub fn from_payload(p: &[u8]) -> Result<Self, PeqError> {
        if p.len() != BAND_PAYLOAD_LEN {
            return Err(PeqError::BadPayloadLen {
                got: p.len(),
                want: BAND_PAYLOAD_LEN,
            });
        }
        let index = p[0];
        let q_fixed = i16::from_be_bytes([p[1], p[2]]);
        let gain_fixed = i16::from_be_bytes([p[3], p[4]]);
        let freq_hz = u16::from_be_bytes([p[5], p[6]]);
        let filter = FilterType::from_byte(p[7]);
        Ok(PeqBand {
            index,
            freq_hz,
            gain_db: gain_fixed as f32 / 10.0,
            q: q_fixed as f32 / 100.0,
            filter,
        })
    }

    /// Build the write frame that sets this band.
    pub fn to_write_frame(&self, seq: u16) -> Frame {
        Frame::write(seq, CMD_PEQ_BAND, self.to_payload().to_vec())
    }

    /// Build the read frame that queries this band by index.
    pub fn query_frame(index: u8, seq: u16) -> Frame {
        Frame::read(seq, CMD_PEQ_BAND, vec![index])
    }
}

/// Errors from PEQ (de)serialization.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PeqError {
    /// Payload length did not match [`BAND_PAYLOAD_LEN`].
    #[error("bad PEQ payload length: got {got}, want {want}")]
    BadPayloadLen {
        /// Length received.
        got: usize,
        /// Length expected.
        want: usize,
    },
    /// A frame carried the wrong opcode for a PEQ band.
    #[error("expected PEQ band opcode, got {0:#04x}")]
    WrongOpcode(u8),
    /// Underlying frame decode error.
    #[error(transparent)]
    Frame(#[from] FrameError),
}

/// PEQ preset / enable state carried by opcode `0x16`.
///
/// Values `0..=3` select a preset slot; `4` means PEQ off (inferred). Any other
/// byte is preserved as [`PresetState::Raw`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PresetState {
    /// Active preset slot 0..=3.
    Slot(u8),
    /// PEQ disabled.
    Off,
    /// An unrecognised state byte.
    Raw(u8),
}

impl PresetState {
    /// Encode to the single `0x16` payload byte.
    pub fn to_byte(self) -> u8 {
        match self {
            PresetState::Slot(n) => n,
            PresetState::Off => 4,
            PresetState::Raw(b) => b,
        }
    }

    /// Decode from the single `0x16` payload byte.
    pub fn from_byte(b: u8) -> Self {
        match b {
            0..=3 => PresetState::Slot(b),
            4 => PresetState::Off,
            other => PresetState::Raw(other),
        }
    }

    /// Parse from a CLI string (`0`..`3` or `off`).
    pub fn parse(s: &str) -> Result<Self, String> {
        if s.eq_ignore_ascii_case("off") {
            return Ok(PresetState::Off);
        }
        match s.parse::<u8>() {
            Ok(n @ 0..=3) => Ok(PresetState::Slot(n)),
            Ok(n) => Err(format!("preset slot {n} out of range (0-3, or 'off')")),
            Err(_) => Err(format!("invalid preset '{s}' (expected 0-3 or 'off')")),
        }
    }
}

impl std::fmt::Display for PresetState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PresetState::Slot(n) => write!(f, "slot {n}"),
            PresetState::Off => write!(f, "off"),
            PresetState::Raw(b) => write!(f, "raw({b:#04x})"),
        }
    }
}

/// A full snapshot of the device's PEQ-relevant runtime state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeqState {
    /// The five PEQ bands.
    pub bands: Vec<PeqBand>,
    /// Global / makeup gain in dB.
    pub gain_db: f32,
    /// Active preset / enable state.
    pub preset: PresetState,
}

impl PeqState {
    /// A flat default snapshot (all bands 0 dB, gain 0, preset slot 0).
    pub fn flat() -> Self {
        PeqState {
            bands: (0..BAND_COUNT as u8).map(PeqBand::flat).collect(),
            gain_db: 0.0,
            preset: PresetState::Slot(0),
        }
    }
}

/// Encode a global-gain value to the `0x17` payload (i16 BE, `×10` dB).
pub fn gain_to_payload(gain_db: f32) -> [u8; 2] {
    ((gain_db * 10.0).round() as i16).to_be_bytes()
}

/// Decode a global-gain value from the `0x17` payload.
pub fn gain_from_payload(p: &[u8]) -> Result<f32, PeqError> {
    if p.len() != 2 {
        return Err(PeqError::BadPayloadLen {
            got: p.len(),
            want: 2,
        });
    }
    Ok(i16::from_be_bytes([p[0], p[1]]) as f32 / 10.0)
}

/// Reasonable default centre frequencies spread across the band, used only for
/// display defaults / the fake device — not a claim about hardware defaults.
fn default_freq_for_band(index: u8) -> u16 {
    match index {
        0 => 31,
        1 => 125,
        2 => 500,
        3 => 2000,
        4 => 8000,
        _ => 1000,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_payload_round_trip() {
        let b = PeqBand {
            index: 2,
            freq_hz: 1000,
            gain_db: -3.5,
            q: 0.71,
            filter: FilterType::Peaking,
        };
        let p = b.to_payload();
        assert_eq!(p.len(), BAND_PAYLOAD_LEN);
        let back = PeqBand::from_payload(&p).unwrap();
        assert_eq!(back.index, 2);
        assert_eq!(back.freq_hz, 1000);
        assert!((back.gain_db - (-3.5)).abs() < 1e-6);
        assert!((back.q - 0.71).abs() < 1e-6);
        assert_eq!(back.filter, FilterType::Peaking);
    }

    #[test]
    fn negative_gain_and_q_encode_as_signed() {
        let b = PeqBand {
            index: 0,
            freq_hz: 60,
            gain_db: -12.0,
            q: 4.0,
            filter: FilterType::LowShelf,
        };
        let p = b.to_payload();
        // gain ×10 = -120 => 0xFF88
        assert_eq!(&p[3..5], &(-120i16).to_be_bytes());
        assert_eq!(&p[1..3], &400i16.to_be_bytes());
        assert_eq!(p[7], 0x01);
    }

    #[test]
    fn unknown_filter_byte_round_trips() {
        assert_eq!(FilterType::from_byte(0x42), FilterType::Unknown(0x42));
        assert_eq!(FilterType::Unknown(0x42).to_byte(), 0x42);
    }

    #[test]
    fn preset_state_mapping() {
        assert_eq!(PresetState::from_byte(0), PresetState::Slot(0));
        assert_eq!(PresetState::from_byte(3), PresetState::Slot(3));
        assert_eq!(PresetState::from_byte(4), PresetState::Off);
        assert_eq!(PresetState::from_byte(9), PresetState::Raw(9));
        assert_eq!(PresetState::Off.to_byte(), 4);
    }

    #[test]
    fn preset_parse() {
        assert_eq!(PresetState::parse("off"), Ok(PresetState::Off));
        assert_eq!(PresetState::parse("2"), Ok(PresetState::Slot(2)));
        assert!(PresetState::parse("7").is_err());
        assert!(PresetState::parse("nope").is_err());
    }

    #[test]
    fn gain_payload_round_trip() {
        let p = gain_to_payload(6.0);
        assert_eq!(gain_from_payload(&p).unwrap(), 6.0);
        let p = gain_to_payload(-2.5);
        assert_eq!(gain_from_payload(&p).unwrap(), -2.5);
    }

    #[test]
    fn filter_type_parse() {
        assert_eq!(FilterType::parse("peaking"), Ok(FilterType::Peaking));
        assert_eq!(FilterType::parse("low_shelf"), Ok(FilterType::LowShelf));
        assert_eq!(FilterType::parse("HS"), Ok(FilterType::HighShelf));
        assert_eq!(FilterType::parse("7"), Ok(FilterType::Unknown(7)));
        assert!(FilterType::parse("bogus").is_err());
    }
}
