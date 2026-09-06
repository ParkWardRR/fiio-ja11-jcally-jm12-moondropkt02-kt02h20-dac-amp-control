//! One-off diagnostic: send enough read requests in a single session for the
//! `seq` counter to push `seq_hi` past zero, then dump the raw reply bytes so
//! the CRC scope's `seq_hi`-vs-`seq_lo` ambiguity (see `docs/HARDWARE-VALIDATION.md`)
//! can be settled by hand. Not part of the normal CLI; run directly with
//! `cargo run --example seq_wrap_probe --features usb`.

use ktctl::device::usb::{UsbConfig, UsbTransport};
use ktctl::device::Transport;
use ktctl::proto::frame::{Frame, FrameCodec};
use ktctl::proto::opcode::CMD_FIRMWARE;

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ")
}

fn main() {
    let mut transport = UsbTransport::open(&UsbConfig::default()).expect("open JA11");
    let mut codec = FrameCodec::new();
    let mut last_seq_hi = 0u8;

    for i in 0..=260u32 {
        let seq = codec.next_seq();
        let req = Frame::read(seq, CMD_FIRMWARE, vec![]);
        let bytes = codec.encode(&req);
        let seq_hi = bytes[3];
        let reply = transport.transceive(&bytes).expect("transceive");

        if seq_hi != last_seq_hi || i >= 254 {
            println!("i={i:3} seq={seq:#06x} (seq_hi={seq_hi:#04x})");
            println!("  > {}", hex(&bytes));
            println!("  < {}", hex(&reply));
            last_seq_hi = seq_hi;
        }
    }
}
