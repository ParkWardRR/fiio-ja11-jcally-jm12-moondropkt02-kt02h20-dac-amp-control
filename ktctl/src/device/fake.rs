//! An in-memory device simulator (roadmap Phase 1, item 3).
//!
//! [`FakeDevice`] holds a mutable [`PeqState`] and answers encoded request
//! frames with plausible reply frames, so the whole CLI/TUI stack can be
//! exercised end-to-end without any hardware. It mirrors `ktflash`'s
//! `FakeBootloader`.
//!
//! Reply convention (a modelling choice, since we have no capture of what real
//! replies look like yet): the device echoes the request's direction/opcode/seq
//! and returns the *resulting* value as the payload — a read returns the stored
//! value, a write applies then echoes back what was stored.

use super::{DeviceError, Transport};
use crate::proto::frame::{Direction, Frame, FrameCodec};
use crate::proto::opcode::{CMD_GAIN, CMD_PEQ_BAND, CMD_PEQ_PRESET};
use crate::proto::peq::{gain_to_payload, PeqBand, PeqState, PresetState};

/// A stateful fake JA11.
#[derive(Debug)]
pub struct FakeDevice {
    state: PeqState,
    codec: FrameCodec,
    /// If set, the next `transceive` returns this error instead of replying —
    /// used by tests to exercise error paths.
    pub inject_error: Option<String>,
}

impl Default for FakeDevice {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeDevice {
    /// A fake device pre-populated with a flat PEQ state.
    pub fn new() -> Self {
        FakeDevice {
            state: PeqState::flat(),
            codec: FrameCodec::new(),
            inject_error: None,
        }
    }

    /// A fake device seeded with a specific state (for fixtures/tests).
    pub fn with_state(state: PeqState) -> Self {
        FakeDevice {
            state,
            codec: FrameCodec::new(),
            inject_error: None,
        }
    }

    /// Read-only view of the simulated state.
    pub fn state(&self) -> &PeqState {
        &self.state
    }

    /// Build a reply frame echoing the request's seq/opcode with `payload`.
    fn reply(&self, req: &Frame, payload: Vec<u8>) -> Vec<u8> {
        // Replies use the read magic pair by convention; the seq/cmd echo back.
        let frame = Frame {
            direction: Direction::Read,
            seq: req.seq,
            cmd: req.cmd,
            payload,
        };
        self.codec.encode(&frame)
    }

    /// Handle one decoded request frame, mutating state as needed, and return
    /// the encoded reply bytes.
    fn handle(&mut self, req: &Frame) -> Result<Vec<u8>, DeviceError> {
        match req.cmd {
            CMD_PEQ_BAND => self.handle_band(req),
            CMD_GAIN => self.handle_gain(req),
            CMD_PEQ_PRESET => self.handle_preset(req),
            other => Err(DeviceError::UnexpectedOpcode {
                expected: other,
                got: other,
            }),
        }
    }

    fn handle_band(&mut self, req: &Frame) -> Result<Vec<u8>, DeviceError> {
        match req.direction {
            Direction::Write => {
                let band = PeqBand::from_payload(&req.payload)?;
                let idx = band.index as usize;
                if let Some(slot) = self.state.bands.get_mut(idx) {
                    *slot = band;
                }
                Ok(self.reply(req, band.to_payload().to_vec()))
            }
            Direction::Read => {
                let idx = req.payload.first().copied().unwrap_or(0) as usize;
                let band = self
                    .state
                    .bands
                    .get(idx)
                    .copied()
                    .unwrap_or_else(|| PeqBand::flat(idx as u8));
                Ok(self.reply(req, band.to_payload().to_vec()))
            }
        }
    }

    fn handle_gain(&mut self, req: &Frame) -> Result<Vec<u8>, DeviceError> {
        if req.direction == Direction::Write {
            self.state.gain_db = crate::proto::peq::gain_from_payload(&req.payload)?;
        }
        Ok(self.reply(req, gain_to_payload(self.state.gain_db).to_vec()))
    }

    fn handle_preset(&mut self, req: &Frame) -> Result<Vec<u8>, DeviceError> {
        if req.direction == Direction::Write {
            let byte = req.payload.first().copied().unwrap_or(0);
            self.state.preset = PresetState::from_byte(byte);
        }
        Ok(self.reply(req, vec![self.state.preset.to_byte()]))
    }
}

impl Transport for FakeDevice {
    fn transceive(&mut self, request: &[u8]) -> Result<Vec<u8>, DeviceError> {
        if let Some(msg) = self.inject_error.take() {
            return Err(DeviceError::Io(msg));
        }
        let req = self.codec.decode(request)?;
        self.handle(&req)
    }

    fn describe(&self) -> String {
        "fake JA11 (in-memory simulator)".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::Device;
    use crate::proto::peq::FilterType;

    #[test]
    fn write_then_read_band_persists() {
        let mut dev = Device::new(FakeDevice::new());
        let b = PeqBand {
            index: 3,
            freq_hz: 3000,
            gain_db: -8.0,
            q: 2.0,
            filter: FilterType::HighShelf,
        };
        dev.set_band(&b).unwrap();
        assert_eq!(dev.get_band(3).unwrap(), b);
    }

    #[test]
    fn injected_error_surfaces() {
        let mut fake = FakeDevice::new();
        fake.inject_error = Some("simulated timeout".into());
        let mut dev = Device::new(fake);
        let err = dev.get_gain().unwrap_err();
        assert!(matches!(err, DeviceError::Io(_)));
    }

    #[test]
    fn describe_is_stable() {
        assert!(FakeDevice::new().describe().contains("fake"));
    }
}
