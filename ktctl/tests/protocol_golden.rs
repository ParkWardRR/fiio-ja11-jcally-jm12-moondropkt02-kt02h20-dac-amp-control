//! Golden-fixture tests for the wire protocol (roadmap Phase 1 item 4).
//!
//! These encode known frames and assert their exact bytes. When a real hardware
//! USB capture lands (Phase 0), drop the captured bytes in here as additional
//! golden vectors and this file becomes the byte-for-byte conformance gate.
//!
//! NOTE: the expected bytes below are computed from *this* implementation's
//! understanding of the (statically-reversed, unconfirmed) protocol. They pin
//! the encoder against accidental regressions; they are NOT proof the protocol
//! is correct — that requires hardware. See `docs/PROTOCOL.md`.

use ktctl::device::{Device, Transport};
use ktctl::device::fake::FakeDevice;
use ktctl::proto::crc::crc8_maxim;
use ktctl::proto::frame::{Frame, FrameCodec};
use ktctl::proto::opcode::CMD_PEQ_BAND;
use ktctl::proto::peq::{FilterType, PeqBand, PresetState};

#[test]
fn write_band_frame_is_well_formed() {
    let codec = FrameCodec::new();
    let band = PeqBand {
        index: 0,
        freq_hz: 1000,
        gain_db: -3.0,
        q: 0.7,
        filter: FilterType::Peaking,
    };
    let frame = band.to_write_frame(0x0001);
    let bytes = codec.encode(&frame);

    // Structural expectations:
    assert_eq!(bytes[0], 0x02, "lead");
    assert_eq!(bytes[1], 0xAA, "write magic");
    assert_eq!(bytes[2], 0x0A, "write dir");
    assert_eq!(&bytes[3..5], &[0x00, 0x01], "seq BE");
    assert_eq!(bytes[5], CMD_PEQ_BAND, "opcode");
    assert_eq!(bytes[6], 8, "payload len");
    assert_eq!(*bytes.last().unwrap(), 0xEE, "term");

    // Payload: index, Q×100 (70), gain×10 (-30), freq (1000), filter (0).
    let payload = &bytes[7..15];
    assert_eq!(payload[0], 0x00); // index
    assert_eq!(&payload[1..3], &70i16.to_be_bytes()); // Q ×100
    assert_eq!(&payload[3..5], &(-30i16).to_be_bytes()); // gain ×10
    assert_eq!(&payload[5..7], &1000u16.to_be_bytes()); // freq
    assert_eq!(payload[7], 0x00); // peaking

    // CRC covers bytes[1..15] (magic through payload).
    let expected_crc = crc8_maxim(&bytes[1..15]);
    assert_eq!(bytes[15], expected_crc);
}

#[test]
fn full_frame_byte_exact_golden() {
    // A frozen golden vector: read/query of band 2, seq 0x00FF.
    let codec = FrameCodec::new();
    let frame = Frame::read(0x00FF, CMD_PEQ_BAND, vec![0x02]);
    let bytes = codec.encode(&frame);

    // Recompute the whole thing explicitly and compare.
    let mut expected = vec![0x02, 0xBB, 0x0B, 0x00, 0xFF, 0x15, 0x01, 0x02];
    let crc = crc8_maxim(&expected[1..]);
    expected.push(crc);
    expected.push(0xEE);

    assert_eq!(bytes, expected);
}

#[test]
fn fake_device_full_state_roundtrip() {
    let mut dev = Device::new(FakeDevice::new());

    dev.set_gain(-4.5).unwrap();
    dev.set_preset(PresetState::Slot(2)).unwrap();
    let band = PeqBand {
        index: 4,
        freq_hz: 8000,
        gain_db: 6.0,
        q: 1.41,
        filter: FilterType::HighShelf,
    };
    dev.set_band(&band).unwrap();

    let state = dev.get_state().unwrap();
    assert_eq!(state.gain_db, -4.5);
    assert_eq!(state.preset, PresetState::Slot(2));
    assert_eq!(state.bands[4], band);
    assert_eq!(state.bands.len(), 5);
}

#[test]
fn transport_describe_available() {
    let dev = Device::new(FakeDevice::new());
    assert!(dev.transport().describe().contains("fake"));
}
