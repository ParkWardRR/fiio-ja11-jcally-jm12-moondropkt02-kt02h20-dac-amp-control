//! Device transport + a high-level [`Device`] wrapper (roadmap Phase 2/3).
//!
//! A [`Transport`] is the raw "write these bytes, read some bytes back" channel.
//! [`Device`] sits on top and speaks in protocol terms (query a band, set gain).
//! Two transports exist: [`fake::FakeDevice`] (always available, no hardware)
//! and [`usb::UsbTransport`] (behind the `usb` feature).

pub mod fake;

#[cfg(feature = "usb")]
pub mod usb;

use crate::proto::frame::{Frame, FrameCodec, FrameError};
use crate::proto::opcode::{CMD_GAIN, CMD_PEQ_PRESET};
use crate::proto::peq::{
    gain_from_payload, gain_to_payload, PeqBand, PeqError, PeqState, PresetState, BAND_COUNT,
};

/// USB vendor id shared by the JA11 / KT02H20 family (from `ktflash`).
pub const JA11_VID: u16 = 0x2972;
/// USB product id for the FiiO JA11 (from `ktflash`).
pub const JA11_PID: u16 = 0x0102;

/// Errors from the transport / device layer.
#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    /// No matching device was found on the bus.
    #[error("no JA11 / KT02H20 device found (looked for {vid:#06x}:{pid:#06x})")]
    NotFound {
        /// VID searched for.
        vid: u16,
        /// PID searched for.
        pid: u16,
    },
    /// A transport I/O failure (timeout, disconnect, permission).
    #[error("transport I/O error: {0}")]
    Io(String),
    /// The device replied with a frame we could not parse.
    #[error(transparent)]
    Frame(#[from] FrameError),
    /// The device replied with a well-formed frame but the wrong opcode.
    #[error("unexpected reply opcode: expected {expected:#04x}, got {got:#04x}")]
    UnexpectedOpcode {
        /// Opcode we asked about.
        expected: u8,
        /// Opcode that came back.
        got: u8,
    },
    /// A PEQ payload could not be decoded.
    #[error(transparent)]
    Peq(#[from] PeqError),
}

/// A raw request/reply byte channel to the device.
pub trait Transport {
    /// Send one encoded frame and return the raw reply bytes.
    fn transceive(&mut self, request: &[u8]) -> Result<Vec<u8>, DeviceError>;

    /// A short human-readable identifier for logging (e.g. "fake" or a bus addr).
    fn describe(&self) -> String {
        "transport".to_string()
    }
}

/// High-level protocol operations over any [`Transport`].
pub struct Device<T: Transport> {
    transport: T,
    codec: FrameCodec,
}

impl<T: Transport> Device<T> {
    /// Wrap a transport in the protocol layer.
    pub fn new(transport: T) -> Self {
        Device {
            transport,
            codec: FrameCodec::new(),
        }
    }

    /// Access the underlying transport (for `describe`, tests, etc.).
    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// Encode a request frame, send it, and decode+validate the reply frame.
    fn exchange(&mut self, req: Frame) -> Result<Frame, DeviceError> {
        let expected_cmd = req.cmd;
        let bytes = self.codec.encode(&req);
        let reply_bytes = self.transport.transceive(&bytes)?;
        let reply = self.codec.decode(&reply_bytes)?;
        if reply.cmd != expected_cmd {
            return Err(DeviceError::UnexpectedOpcode {
                expected: expected_cmd,
                got: reply.cmd,
            });
        }
        Ok(reply)
    }

    /// Read a single PEQ band by index.
    pub fn get_band(&mut self, index: u8) -> Result<PeqBand, DeviceError> {
        let seq = self.codec.next_seq();
        let reply = self.exchange(PeqBand::query_frame(index, seq))?;
        Ok(PeqBand::from_payload(&reply.payload)?)
    }

    /// Write a single PEQ band.
    pub fn set_band(&mut self, band: &PeqBand) -> Result<(), DeviceError> {
        let seq = self.codec.next_seq();
        self.exchange(band.to_write_frame(seq))?;
        Ok(())
    }

    /// Read the global / makeup gain in dB.
    pub fn get_gain(&mut self) -> Result<f32, DeviceError> {
        let seq = self.codec.next_seq();
        let reply = self.exchange(Frame::read(seq, CMD_GAIN, vec![]))?;
        Ok(gain_from_payload(&reply.payload)?)
    }

    /// Set the global / makeup gain in dB.
    pub fn set_gain(&mut self, gain_db: f32) -> Result<(), DeviceError> {
        let seq = self.codec.next_seq();
        let payload = gain_to_payload(gain_db).to_vec();
        self.exchange(Frame::write(seq, CMD_GAIN, payload))?;
        Ok(())
    }

    /// Read the active preset / enable state.
    pub fn get_preset(&mut self) -> Result<PresetState, DeviceError> {
        let seq = self.codec.next_seq();
        let reply = self.exchange(Frame::read(seq, CMD_PEQ_PRESET, vec![]))?;
        let byte = reply.payload.first().copied().unwrap_or(0);
        Ok(PresetState::from_byte(byte))
    }

    /// Set the active preset / enable state.
    pub fn set_preset(&mut self, preset: PresetState) -> Result<(), DeviceError> {
        let seq = self.codec.next_seq();
        self.exchange(Frame::write(seq, CMD_PEQ_PRESET, vec![preset.to_byte()]))?;
        Ok(())
    }

    /// Read the full runtime PEQ snapshot (all bands + gain + preset).
    pub fn get_state(&mut self) -> Result<PeqState, DeviceError> {
        let mut bands = Vec::with_capacity(BAND_COUNT);
        for i in 0..BAND_COUNT as u8 {
            bands.push(self.get_band(i)?);
        }
        Ok(PeqState {
            bands,
            gain_db: self.get_gain()?,
            preset: self.get_preset()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::fake::FakeDevice;
    use crate::proto::peq::FilterType;

    #[test]
    fn round_trip_band_via_fake() {
        let mut dev = Device::new(FakeDevice::new());
        let band = PeqBand {
            index: 1,
            freq_hz: 250,
            gain_db: 4.5,
            q: 1.4,
            filter: FilterType::Peaking,
        };
        dev.set_band(&band).unwrap();
        let got = dev.get_band(1).unwrap();
        assert_eq!(got, band);
    }

    #[test]
    fn round_trip_gain_and_preset() {
        let mut dev = Device::new(FakeDevice::new());
        dev.set_gain(-6.0).unwrap();
        assert_eq!(dev.get_gain().unwrap(), -6.0);
        dev.set_preset(PresetState::Slot(2)).unwrap();
        assert_eq!(dev.get_preset().unwrap(), PresetState::Slot(2));
        dev.set_preset(PresetState::Off).unwrap();
        assert_eq!(dev.get_preset().unwrap(), PresetState::Off);
    }

    #[test]
    fn full_state_snapshot() {
        let mut dev = Device::new(FakeDevice::new());
        let state = dev.get_state().unwrap();
        assert_eq!(state.bands.len(), BAND_COUNT);
    }
}
