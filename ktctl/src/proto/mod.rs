//! Hardware-free protocol core (roadmap Phase 1).
//!
//! Everything in this module is pure logic — frame codec, CRC, PEQ + device
//! state models — and builds and tests without a device present. The transport
//! that carries these frames over USB lives in [`crate::device`].

pub mod crc;
pub mod frame;
pub mod opcode;
pub mod peq;
pub mod response;
pub mod state;

pub use crc::{crc8_maxim, crc8_maxim_table, CRC8_MAXIM_TABLE};
pub use frame::{Direction, Frame, FrameCodec, FrameError};
pub use opcode::{
    opcode_name, CMD_FIRMWARE, CMD_GAIN, CMD_MIC_DETECT, CMD_PEQ_BAND, CMD_PEQ_PRESET,
    CMD_SAMPLE_RATE, CMD_UAC_MODE, CMD_VOLUME, SAVE_CANDIDATES,
};
pub use peq::{FilterType, GainEncoding, PeqBand, PeqError, PeqState, PresetState, BAND_COUNT};
pub use response::{sample_curve, state_response_db};
pub use state::{firmware_version, sample_rate_label, DeviceState, UacMode, SAMPLE_RATE_TABLE};
