//! An in-memory device simulator (roadmap Phase 1, item 3).
//!
//! [`FakeDevice`] holds mutable PEQ + device state and answers encoded request
//! frames with plausible reply frames, so the whole CLI/TUI stack can be
//! exercised end-to-end without any hardware. It mirrors `ktflash`'s
//! `FakeBootloader`.
//!
//! Reply convention (a modelling choice, since we have no capture of what real
//! replies look like yet): the device echoes the request's opcode/seq and
//! returns the *resulting* value as the payload — a read returns the stored
//! value, a write applies then echoes back what was stored. Gain payload bytes
//! are stored and echoed verbatim, so the simulator is agnostic to whichever
//! [`crate::proto::peq::GainEncoding`] the caller uses.

use super::{DeviceError, Transport};
use crate::proto::frame::{Direction, Frame, FrameCodec};
use crate::proto::opcode::{
    CMD_FIRMWARE, CMD_GAIN, CMD_MIC_DETECT, CMD_PEQ_BAND, CMD_PEQ_PRESET, CMD_SAMPLE_RATE,
    CMD_UAC_MODE, CMD_VOLUME,
};
use crate::proto::peq::{GainEncoding, PeqBand, PeqState, PresetState};

/// Simulated Status-screen state, seeded from FIIO's own screenshots.
#[derive(Debug, Clone)]
struct FakeStatus {
    volume: u8,
    sample_rate_index: u8,
    firmware: [u8; 2],
    mic_present: bool,
    uac: u8,
}

impl Default for FakeStatus {
    fn default() -> Self {
        FakeStatus {
            volume: 60,           // screenshot value
            sample_rate_index: 7, // "384k" in SAMPLE_RATE_TABLE
            firmware: [0x02, 0x14], // real hardware bytes (2026-09-06); decodes to "1.4" via BCD
            mic_present: true,    // screenshot showed mic ON
            uac: 2,               // screenshot showed UAC 2.0
        }
    }
}

/// A stateful fake JA11.
#[derive(Debug)]
pub struct FakeDevice {
    state: PeqState,
    status: FakeStatus,
    /// Raw `0x17` gain payload, echoed verbatim so encoding never matters here.
    gain_payload: Vec<u8>,
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
            status: FakeStatus::default(),
            gain_payload: GainEncoding::default().to_payload(0.0),
            codec: FrameCodec::new(),
            inject_error: None,
        }
    }

    /// A fake device seeded with a specific PEQ state (for fixtures/tests).
    pub fn with_state(state: PeqState) -> Self {
        FakeDevice {
            state,
            ..Self::new()
        }
    }

    /// Read-only view of the simulated PEQ state.
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
            CMD_VOLUME => Ok(self.rw_byte(req, |s| &mut s.volume)),
            CMD_UAC_MODE => Ok(self.rw_byte(req, |s| &mut s.uac)),
            CMD_SAMPLE_RATE => Ok(self.rw_byte(req, |s| &mut s.sample_rate_index)),
            CMD_MIC_DETECT => {
                // Read-only in practice; echo the stored flag as 0/1.
                Ok(self.reply(req, vec![self.status.mic_present as u8]))
            }
            CMD_FIRMWARE => Ok(self.reply(req, self.status.firmware.to_vec())),
            other => Err(DeviceError::UnexpectedOpcode {
                expected: other,
                got: other,
            }),
        }
    }

    /// Generic read/write of a single status byte selected by `field`.
    fn rw_byte(&mut self, req: &Frame, field: fn(&mut FakeStatus) -> &mut u8) -> Vec<u8> {
        if req.direction == Direction::Write {
            if let Some(b) = req.payload.first().copied() {
                *field(&mut self.status) = b;
            }
        }
        let value = *field(&mut self.status);
        self.reply(req, vec![value])
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
            self.gain_payload = req.payload.clone();
            // Keep the decoded view roughly in sync for `state()` consumers.
            if let Ok(g) = GainEncoding::default().from_payload(&req.payload) {
                self.state.gain_db = g;
            }
        }
        Ok(self.reply(req, self.gain_payload.clone()))
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
    use crate::proto::state::UacMode;

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
    fn gain_round_trips_under_both_encodings() {
        for enc in [GainEncoding::X2560Le, GainEncoding::X10Be] {
            let mut dev = Device::new(FakeDevice::new()).with_gain_encoding(enc);
            dev.set_gain(3.5).unwrap();
            assert!((dev.get_gain().unwrap() - 3.5).abs() < 1e-3, "enc {enc:?}");
        }
    }

    #[test]
    fn device_state_defaults_match_screenshots() {
        let mut dev = Device::new(FakeDevice::new());
        let st = dev.get_device_state().unwrap();
        assert_eq!(st.volume, 60);
        assert_eq!(st.sample_rate, "384k");
        assert_eq!(st.firmware, "1.4");
        assert!(st.mic_present);
        assert_eq!(st.uac, UacMode::Uac2);
    }

    #[test]
    fn uac_write_then_read() {
        let mut dev = Device::new(FakeDevice::new());
        dev.set_uac(UacMode::Uac1).unwrap();
        assert_eq!(dev.get_uac().unwrap(), UacMode::Uac1);
    }

    #[test]
    fn save_errors_when_opcode_unknown_to_fake() {
        let mut dev = Device::new(FakeDevice::new());
        // save's 0x19/0x18 candidates are unknown to the fake, so save should
        // surface an error rather than silently "succeed" against a stub.
        assert!(dev.save().is_err());
    }

    #[test]
    fn injected_error_surfaces() {
        let mut fake = FakeDevice::new();
        fake.inject_error = Some("simulated timeout".into());
        let mut dev = Device::new(fake);
        let err = dev.get_gain().unwrap_err();
        assert!(matches!(err, DeviceError::Io(_)));
    }
}
