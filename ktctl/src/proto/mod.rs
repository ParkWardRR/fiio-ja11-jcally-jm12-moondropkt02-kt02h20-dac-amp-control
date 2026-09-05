//! Hardware-free protocol core (roadmap Phase 1).
//!
//! Everything in this module is pure logic — frame codec, CRC, PEQ model — and
//! builds and tests without a device present. The transport that carries these
//! frames over USB lives in [`crate::device`].

pub mod crc;
pub mod frame;
pub mod opcode;
pub mod peq;

pub use crc::{crc8_maxim, crc8_maxim_table, CRC8_MAXIM_TABLE};
pub use frame::{Direction, Frame, FrameCodec, FrameError};
pub use opcode::{opcode_name, CMD_GAIN, CMD_PEQ_BAND, CMD_PEQ_PRESET};
pub use peq::{
    gain_from_payload, gain_to_payload, FilterType, PeqBand, PeqError, PeqState, PresetState,
    BAND_COUNT,
};
