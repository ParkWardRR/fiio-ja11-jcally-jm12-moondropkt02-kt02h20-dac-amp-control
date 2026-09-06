//! PEQ band model and its fixed-point wire encoding.
//!
//! The `0x15` payload recovered in Phase 0 (byte order **corrected 2026-09-05** —
//! gain comes before freq/Q) is 8 bytes:
//!
//! ```text
//! [ index, gain×10 dB (i16 BE), freq Hz (u16 BE), Q×100 (i16 BE), filterType ]
//! ```
//!
//! Scaling: gain `×10`, Q `×100`, freq plain Hz. Filter type on the JA11 is one
//! of three of FIIO's seven shared types (`0`=Peak, `1`=LowShelf, `2`=HighShelf).
//!
//! The master-gain (`0x17`) encoding is genuinely ambiguous and is modelled by
//! [`GainEncoding`]; see its docs. All still provisional (roadmap Phase 0 /
//! `docs/PROTOCOL.md`).

use serde::{Deserialize, Serialize};

use super::frame::{Frame, FrameError};
use super::opcode::CMD_PEQ_BAND;

/// The JA11 exposes a fixed 5-band parametric EQ.
pub const BAND_COUNT: usize = 5;

/// Number of payload bytes in a `0x15` band record.
pub const BAND_PAYLOAD_LEN: usize = 8;

/// Biquad filter type. The JA11's band-edit screen offers only the first three
/// of FIIO's seven shared types; [`FilterType::Unknown`] preserves any other
/// byte for lossless round-tripping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FilterType {
    /// Peak / bell filter (`0`) — the default and most common PEQ band.
    Peak,
    /// Low-shelf filter (`1`).
    LowShelf,
    /// High-shelf filter (`2`).
    HighShelf,
    /// A filter-type byte the JA11 UI doesn't offer; carries the raw value.
    Unknown(u8),
}

impl FilterType {
    /// Encode to the trailing wire byte.
    pub fn to_byte(self) -> u8 {
        match self {
            FilterType::Peak => 0x00,
            FilterType::LowShelf => 0x01,
            FilterType::HighShelf => 0x02,
            FilterType::Unknown(b) => b,
        }
    }

    /// Decode from the trailing wire byte.
    pub fn from_byte(b: u8) -> Self {
        match b {
            0x00 => FilterType::Peak,
            0x01 => FilterType::LowShelf,
            0x02 => FilterType::HighShelf,
            other => FilterType::Unknown(other),
        }
    }

    /// Parse from a CLI string (`peak`, `low-shelf`, `high-shelf`, or a raw int).
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().replace('_', "-").as_str() {
            "peak" | "peaking" | "pk" => Ok(FilterType::Peak),
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
            FilterType::Peak => write!(f, "peak"),
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
            filter: FilterType::Peak,
        }
    }

    /// Serialize this band to the 8-byte `0x15` payload
    /// (`index, gain×10 BE, freq BE, Q×100 BE, filter`).
    pub fn to_payload(&self) -> [u8; BAND_PAYLOAD_LEN] {
        let gain_fixed = (self.gain_db * 10.0).round() as i16;
        let q_fixed = (self.q * 100.0).round() as i16;
        let mut out = [0u8; BAND_PAYLOAD_LEN];
        out[0] = self.index;
        out[1..3].copy_from_slice(&gain_fixed.to_be_bytes());
        out[3..5].copy_from_slice(&self.freq_hz.to_be_bytes());
        out[5..7].copy_from_slice(&q_fixed.to_be_bytes());
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
        let gain_fixed = i16::from_be_bytes([p[1], p[2]]);
        let freq_hz = u16::from_be_bytes([p[3], p[4]]);
        let q_fixed = i16::from_be_bytes([p[5], p[6]]);
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
    /// Payload length did not match what the opcode expects.
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
/// Names confirmed from FIIO's own WebHID site: `0`=Vocal, `1`=Classic,
/// `2`=Bass, `3`=USER1 (custom), `4`=off. Any other byte is preserved as
/// [`PresetState::Raw`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PresetState {
    /// Vocal preset (`0`).
    Vocal,
    /// Classic preset (`1`).
    Classic,
    /// Bass preset (`2`).
    Bass,
    /// USER1 / custom preset (`3`).
    User1,
    /// PEQ disabled (`4`).
    Off,
    /// An unrecognised state byte.
    Raw(u8),
}

impl PresetState {
    /// Encode to the single `0x16` payload byte.
    pub fn to_byte(self) -> u8 {
        match self {
            PresetState::Vocal => 0,
            PresetState::Classic => 1,
            PresetState::Bass => 2,
            PresetState::User1 => 3,
            PresetState::Off => 4,
            PresetState::Raw(b) => b,
        }
    }

    /// Decode from the single `0x16` payload byte.
    pub fn from_byte(b: u8) -> Self {
        match b {
            0 => PresetState::Vocal,
            1 => PresetState::Classic,
            2 => PresetState::Bass,
            3 => PresetState::User1,
            4 => PresetState::Off,
            other => PresetState::Raw(other),
        }
    }

    /// Parse from a CLI string: a name (`vocal`/`classic`/`bass`/`user1`/`off`)
    /// or a numeric slot `0`-`4`.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "vocal" | "0" => Ok(PresetState::Vocal),
            "classic" | "1" => Ok(PresetState::Classic),
            "bass" | "2" => Ok(PresetState::Bass),
            "user1" | "user" | "custom" | "3" => Ok(PresetState::User1),
            "off" | "4" => Ok(PresetState::Off),
            other => Err(format!(
                "invalid preset '{other}' (vocal/classic/bass/user1/off or 0-4)"
            )),
        }
    }
}

impl std::fmt::Display for PresetState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PresetState::Vocal => write!(f, "vocal"),
            PresetState::Classic => write!(f, "classic"),
            PresetState::Bass => write!(f, "bass"),
            PresetState::User1 => write!(f, "user1"),
            PresetState::Off => write!(f, "off"),
            PresetState::Raw(b) => write!(f, "raw({b:#04x})"),
        }
    }
}

/// Encoding of the master-gain (`0x17`) value.
///
/// This is genuinely unresolved: this project's own Android-only static RE read
/// `×10` big-endian, but two independent hardware-facing drivers
/// (`fiiocontrol-oss`, `glacier-eq`) both use `×2560` little-endian and agree
/// with each other — so [`GainEncoding::X2560Le`] is the default until a real
/// JA11 settles it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GainEncoding {
    /// `gain×2560` as a little-endian i16 (two hardware drivers agree; default).
    #[default]
    X2560Le,
    /// `gain×10` as a big-endian i16 (this project's original static RE).
    X10Be,
}

impl GainEncoding {
    /// Encode a master-gain value in dB to the `0x17` payload bytes.
    ///
    /// The fixed-point product is clamped to the `i16` range so out-of-range
    /// input can never panic on the cast.
    pub fn to_payload(self, gain_db: f32) -> Vec<u8> {
        match self {
            GainEncoding::X2560Le => {
                let v = (gain_db * 2560.0).round().clamp(i16::MIN as f32, i16::MAX as f32) as i16;
                v.to_le_bytes().to_vec()
            }
            GainEncoding::X10Be => {
                let v = (gain_db * 10.0).round().clamp(i16::MIN as f32, i16::MAX as f32) as i16;
                v.to_be_bytes().to_vec()
            }
        }
    }

    /// Decode a master-gain value in dB from the `0x17` payload bytes.
    pub fn from_payload(self, p: &[u8]) -> Result<f32, PeqError> {
        if p.len() != 2 {
            return Err(PeqError::BadPayloadLen {
                got: p.len(),
                want: 2,
            });
        }
        Ok(match self {
            GainEncoding::X2560Le => i16::from_le_bytes([p[0], p[1]]) as f32 / 2560.0,
            GainEncoding::X10Be => i16::from_be_bytes([p[0], p[1]]) as f32 / 10.0,
        })
    }
}

/// A full snapshot of the device's PEQ-relevant runtime state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeqState {
    /// The five PEQ bands.
    pub bands: Vec<PeqBand>,
    /// Master / makeup gain in dB.
    pub gain_db: f32,
    /// Active preset / enable state.
    pub preset: PresetState,
}

impl PeqState {
    /// A flat default snapshot (all bands 0 dB, gain 0, USER1 preset).
    pub fn flat() -> Self {
        PeqState {
            bands: (0..BAND_COUNT as u8).map(PeqBand::flat).collect(),
            gain_db: 0.0,
            preset: PresetState::User1,
        }
    }
}

/// Real JA11 default centre frequencies, taken from FIIO's own EQ-screen
/// screenshots (`29 / 81 / 600 / 7460 / 15660 Hz`).
fn default_freq_for_band(index: u8) -> u16 {
    match index {
        0 => 29,
        1 => 81,
        2 => 600,
        3 => 7460,
        4 => 15660,
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
            filter: FilterType::Peak,
        };
        let p = b.to_payload();
        assert_eq!(p.len(), BAND_PAYLOAD_LEN);
        let back = PeqBand::from_payload(&p).unwrap();
        assert_eq!(back, b);
    }

    #[test]
    fn band_payload_field_order_is_gain_freq_q() {
        // gain -3.0 → -30 (BE), freq 1000, Q 0.7 → 70 (BE), filter peak
        let b = PeqBand {
            index: 0,
            freq_hz: 1000,
            gain_db: -3.0,
            q: 0.7,
            filter: FilterType::Peak,
        };
        let p = b.to_payload();
        assert_eq!(p[0], 0); // index
        assert_eq!(&p[1..3], &(-30i16).to_be_bytes()); // gain first
        assert_eq!(&p[3..5], &1000u16.to_be_bytes()); // then freq
        assert_eq!(&p[5..7], &70i16.to_be_bytes()); // then Q
        assert_eq!(p[7], 0); // peak
    }

    #[test]
    fn unknown_filter_byte_round_trips() {
        assert_eq!(FilterType::from_byte(0x42), FilterType::Unknown(0x42));
        assert_eq!(FilterType::Unknown(0x42).to_byte(), 0x42);
    }

    #[test]
    fn preset_names_map_correctly() {
        assert_eq!(PresetState::from_byte(0), PresetState::Vocal);
        assert_eq!(PresetState::from_byte(3), PresetState::User1);
        assert_eq!(PresetState::from_byte(4), PresetState::Off);
        assert_eq!(PresetState::from_byte(9), PresetState::Raw(9));
        assert_eq!(PresetState::Bass.to_byte(), 2);
    }

    #[test]
    fn preset_parse_names_and_numbers() {
        assert_eq!(PresetState::parse("vocal"), Ok(PresetState::Vocal));
        assert_eq!(PresetState::parse("USER1"), Ok(PresetState::User1));
        assert_eq!(PresetState::parse("off"), Ok(PresetState::Off));
        assert_eq!(PresetState::parse("2"), Ok(PresetState::Bass));
        assert!(PresetState::parse("nope").is_err());
    }

    #[test]
    fn gain_encoding_default_is_x2560_le() {
        assert_eq!(GainEncoding::default(), GainEncoding::X2560Le);
        let enc = GainEncoding::X2560Le;
        // 6 dB × 2560 = 15360 = 0x3C00 → LE 0x00 0x3C
        assert_eq!(enc.to_payload(6.0), vec![0x00, 0x3C]);
        assert_eq!(enc.from_payload(&[0x00, 0x3C]).unwrap(), 6.0);
    }

    #[test]
    fn gain_encoding_x10_be_round_trip() {
        let enc = GainEncoding::X10Be;
        assert_eq!(enc.to_payload(-2.5), vec![0xFF, 0xE7]); // -25 BE
        assert_eq!(enc.from_payload(&[0xFF, 0xE7]).unwrap(), -2.5);
    }

    #[test]
    fn gain_encoding_clamps_instead_of_panicking() {
        // 100 dB × 2560 overflows i16; must clamp, not panic.
        let p = GainEncoding::X2560Le.to_payload(100.0);
        assert_eq!(p, i16::MAX.to_le_bytes().to_vec());
    }

    #[test]
    fn filter_type_parse() {
        assert_eq!(FilterType::parse("peak"), Ok(FilterType::Peak));
        assert_eq!(FilterType::parse("low_shelf"), Ok(FilterType::LowShelf));
        assert_eq!(FilterType::parse("HS"), Ok(FilterType::HighShelf));
        assert_eq!(FilterType::parse("7"), Ok(FilterType::Unknown(7)));
        assert!(FilterType::parse("bogus").is_err());
    }
}
