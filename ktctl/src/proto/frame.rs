//! Wire frame encode/decode for the JA11 runtime control channel.
//!
//! Frame layout (recovered by static RE — **not yet hardware-confirmed**, see
//! `docs/PROTOCOL.md`):
//!
//! ```text
//!  ┌──────┬────────┬────────┬────────┬────────┬──────┬──────┬─────────┬──────┬──────┐
//!  │ 0x02 │ magic  │  dir   │ seq_hi │ seq_lo │ cmd  │ len  │ payload │ crc8 │ 0xEE │
//!  └──────┴────────┴────────┴────────┴────────┴──────┴──────┴─────────┴──────┴──────┘
//!    lead   AA|BB    0A|0B    u16 big-endian    op    n       n bytes   maxim  term
//! ```
//!
//! * `magic`/`dir` travel as a pair: `AA 0A` = **write**, `BB 0B` = **read/query**.
//! * `seq` is a 16-bit big-endian free-running counter.
//! * `crc8` is [CRC-8/MAXIM](super::crc) computed over the bytes from `magic`
//!   through the last payload byte (inclusive) — i.e. the leading `0x02`, the
//!   `crc8` itself and the `0xEE` terminator are excluded.
//!
//! Open questions still flagged in the roadmap: whether the leading `0x02` is
//! load-bearing, and the exact CRC scope. The scope is centralised in
//! [`FrameCodec::crc_scope`] so a single edit fixes it if hardware disagrees.

use super::crc::crc8_maxim;

/// Fixed leading byte observed on every USB frame.
pub const LEAD: u8 = 0x02;
/// Fixed trailing terminator byte.
pub const TERM: u8 = 0xEE;

/// First byte of the magic pair for a write frame.
pub const MAGIC_WRITE: u8 = 0xAA;
/// Second byte of the magic pair for a write frame.
pub const DIR_WRITE: u8 = 0x0A;
/// First byte of the magic pair for a read/query frame.
pub const MAGIC_READ: u8 = 0xBB;
/// Second byte of the magic pair for a read/query frame.
pub const DIR_READ: u8 = 0x0B;

/// The minimum number of bytes a valid frame can occupy (zero-length payload):
/// lead + magic + dir + seq(2) + cmd + len + crc + term.
pub const MIN_FRAME_LEN: usize = 9;

/// Direction of a frame — write (host → device) or read/query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Host writes a value to the device (`AA 0A`).
    Write,
    /// Host reads/queries a value from the device (`BB 0B`).
    Read,
}

impl Direction {
    /// The `(magic, dir)` byte pair for this direction.
    pub fn magic_pair(self) -> (u8, u8) {
        match self {
            Direction::Write => (MAGIC_WRITE, DIR_WRITE),
            Direction::Read => (MAGIC_READ, DIR_READ),
        }
    }

    /// Recover a direction from a `(magic, dir)` byte pair.
    pub fn from_magic_pair(magic: u8, dir: u8) -> Result<Self, FrameError> {
        match (magic, dir) {
            (MAGIC_WRITE, DIR_WRITE) => Ok(Direction::Write),
            (MAGIC_READ, DIR_READ) => Ok(Direction::Read),
            _ => Err(FrameError::BadMagic { magic, dir }),
        }
    }
}

/// A fully-parsed protocol frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Write vs. read direction.
    pub direction: Direction,
    /// 16-bit sequence counter.
    pub seq: u16,
    /// Single-byte opcode (see [`super::opcode`]).
    pub cmd: u8,
    /// Command payload (`len` bytes on the wire).
    pub payload: Vec<u8>,
}

impl Frame {
    /// Convenience constructor for a write frame.
    pub fn write(seq: u16, cmd: u8, payload: impl Into<Vec<u8>>) -> Self {
        Frame {
            direction: Direction::Write,
            seq,
            cmd,
            payload: payload.into(),
        }
    }

    /// Convenience constructor for a read/query frame.
    pub fn read(seq: u16, cmd: u8, payload: impl Into<Vec<u8>>) -> Self {
        Frame {
            direction: Direction::Read,
            seq,
            cmd,
            payload: payload.into(),
        }
    }
}

/// Errors produced while decoding a frame.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FrameError {
    /// Buffer is shorter than [`MIN_FRAME_LEN`].
    #[error("frame too short: {0} bytes (minimum {MIN_FRAME_LEN})")]
    TooShort(usize),
    /// Leading byte was not [`LEAD`].
    #[error("bad lead byte: {0:#04x} (expected {LEAD:#04x})")]
    BadLead(u8),
    /// Trailing byte was not [`TERM`].
    #[error("bad terminator: {0:#04x} (expected {TERM:#04x})")]
    BadTerm(u8),
    /// Magic/direction pair was neither write nor read.
    #[error("bad magic pair: {magic:#04x} {dir:#04x}")]
    BadMagic {
        /// The offending magic byte.
        magic: u8,
        /// The offending direction byte.
        dir: u8,
    },
    /// The declared payload length does not fit the buffer.
    #[error("declared length {declared} inconsistent with buffer of {buffer} bytes")]
    LengthMismatch {
        /// Value of the `len` field.
        declared: usize,
        /// Actual buffer length.
        buffer: usize,
    },
    /// CRC did not validate.
    #[error("crc mismatch: computed {computed:#04x}, frame carried {carried:#04x}")]
    CrcMismatch {
        /// CRC we computed over the frame.
        computed: u8,
        /// CRC byte present in the frame.
        carried: u8,
    },
}

/// Stateless-ish encoder/decoder holding an auto-incrementing sequence counter.
///
/// The sequence counter is a convenience for callers that want the codec to own
/// framing state; [`FrameCodec::encode`] uses the frame's own `seq` and never
/// touches the counter, while [`FrameCodec::next_seq`] hands out and advances it.
#[derive(Debug, Default)]
pub struct FrameCodec {
    seq: u16,
}

impl FrameCodec {
    /// Create a codec with the sequence counter starting at 0.
    pub fn new() -> Self {
        FrameCodec { seq: 0 }
    }

    /// Return the current sequence value and advance the counter (wrapping).
    pub fn next_seq(&mut self) -> u16 {
        let s = self.seq;
        self.seq = self.seq.wrapping_add(1);
        s
    }

    /// Bytes that the CRC is computed over, given the full serialized frame
    /// *without* the trailing crc+term (i.e. `&frame[LEAD..end_of_payload]`).
    ///
    /// Centralised so the scope can be corrected in one place if hardware
    /// disagrees — and it did: confirmed against a real JA11 (2026-09-06) by
    /// brute-forcing every contiguous byte range against two independent,
    /// device-computed reply CRCs (a `0x0B` firmware reply and a `0x16` preset
    /// reply, different `cmd`/`len`/payload/seq). The scope that satisfies both
    /// is `seq_hi..=last payload byte` — **not** `magic..=last payload byte` as
    /// originally assumed from static RE alone. `magic`/`dir` (bytes 1-2) are
    /// excluded from the CRC entirely.
    ///
    /// Residual gap: both hardware samples had `seq_hi == 0x00`, and a leading
    /// zero byte is a mathematical no-op for this CRC (state stays 0 through a
    /// 0x00 byte when the running CRC is already 0) — so these two samples
    /// can't distinguish "starts at `seq_hi`" from "starts at `seq_lo`". This
    /// picks the wider (`seq_hi`-inclusive) scope as the safe default, since it
    /// is provably correct whenever `seq_hi == 0` regardless of which is
    /// actually right; re-confirm once a session's `seq` counter has wrapped
    /// past 255 and `seq_hi != 0x00` is observed on the wire.
    fn crc_scope(frame_wo_crc_term: &[u8]) -> &[u8] {
        // Skip lead(0)/magic(1)/dir(2); CRC covers seq_hi..=last payload byte.
        &frame_wo_crc_term[3..]
    }

    /// Serialize a [`Frame`] to its on-wire byte representation.
    pub fn encode(&self, frame: &Frame) -> Vec<u8> {
        let (magic, dir) = frame.direction.magic_pair();
        let len = frame.payload.len();
        // lead + magic + dir + seq(2) + cmd + len + payload + crc + term
        let mut buf = Vec::with_capacity(MIN_FRAME_LEN + len);
        buf.push(LEAD);
        buf.push(magic);
        buf.push(dir);
        buf.push((frame.seq >> 8) as u8);
        buf.push((frame.seq & 0xFF) as u8);
        buf.push(frame.cmd);
        buf.push(len as u8);
        buf.extend_from_slice(&frame.payload);
        let crc = crc8_maxim(Self::crc_scope(&buf));
        buf.push(crc);
        buf.push(TERM);
        buf
    }

    /// Parse an on-wire byte buffer into a [`Frame`], validating structure and CRC.
    pub fn decode(&self, buf: &[u8]) -> Result<Frame, FrameError> {
        if buf.len() < MIN_FRAME_LEN {
            return Err(FrameError::TooShort(buf.len()));
        }
        if buf[0] != LEAD {
            return Err(FrameError::BadLead(buf[0]));
        }
        let direction = Direction::from_magic_pair(buf[1], buf[2])?;
        let seq = u16::from_be_bytes([buf[3], buf[4]]);
        let cmd = buf[5];
        let declared = buf[6] as usize;

        // Full frame length implied by the declared payload length. USB HID
        // interrupt/bulk reads on real hardware return a fixed-size report
        // (observed: 21 bytes) zero-padded past the meaningful frame, not a
        // buffer trimmed to exactly the frame's own length — so accept any
        // buffer *at least* this long and only look at its `expected_total`-byte
        // prefix below. (Confirmed against a real JA11, 2026-09-06: a 2-byte
        // `0x0B` version reply arrived in a 21-byte report.)
        let expected_total = MIN_FRAME_LEN + declared;
        if buf.len() < expected_total {
            return Err(FrameError::LengthMismatch {
                declared,
                buffer: buf.len(),
            });
        }

        let payload = buf[7..7 + declared].to_vec();
        let carried_crc = buf[7 + declared];
        let term = buf[8 + declared];
        if term != TERM {
            return Err(FrameError::BadTerm(term));
        }

        // Use the same centralised scope `encode` uses (see `crc_scope`'s docs
        // for why it's `seq_hi..=last payload byte`, not `magic..=...`).
        let computed = crc8_maxim(Self::crc_scope(&buf[..7 + declared]));
        if computed != carried_crc {
            return Err(FrameError::CrcMismatch {
                computed,
                carried: carried_crc,
            });
        }

        Ok(Frame {
            direction,
            seq,
            cmd,
            payload,
        })
    }

    /// Scan `buf` for the first structurally-valid, CRC-passing frame and return
    /// it along with the total number of bytes consumed up to and including its
    /// terminator.
    ///
    /// Unlike [`FrameCodec::decode`], which requires `buf` to be *exactly* one
    /// frame, this tolerates leading garbage and trailing bytes — useful for a
    /// real transport where a bulk IN read may return partial noise or several
    /// batched replies. Returns `None` if no complete valid frame is present
    /// yet (caller should read more bytes and retry).
    pub fn find_and_decode(&self, buf: &[u8]) -> Option<(Frame, usize)> {
        let mut start = 0;
        while start + MIN_FRAME_LEN <= buf.len() {
            // Anchor on a lead byte.
            if buf[start] != LEAD {
                start += 1;
                continue;
            }
            let declared = buf[start + 6] as usize;
            let total = MIN_FRAME_LEN + declared;
            if start + total <= buf.len() {
                let candidate = &buf[start..start + total];
                if let Ok(frame) = self.decode(candidate) {
                    return Some((frame, start + total));
                }
            }
            // Either this lead byte is spurious (bad length/CRC) or the frame is
            // genuinely incomplete — in both cases slide forward and keep looking.
            // If nothing validates, we fall through to `None` (caller reads more).
            start += 1;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_write() {
        let codec = FrameCodec::new();
        let f = Frame::write(0x1234, 0x15, vec![0x00, 0x01, 0x02, 0x03]);
        let bytes = codec.encode(&f);
        assert_eq!(bytes[0], LEAD);
        assert_eq!(bytes[1], MAGIC_WRITE);
        assert_eq!(bytes[2], DIR_WRITE);
        assert_eq!(*bytes.last().unwrap(), TERM);
        let back = codec.decode(&bytes).unwrap();
        assert_eq!(back, f);
    }

    #[test]
    fn round_trip_read_empty_payload() {
        let codec = FrameCodec::new();
        let f = Frame::read(0, 0x16, vec![]);
        let bytes = codec.encode(&f);
        assert_eq!(bytes.len(), MIN_FRAME_LEN);
        assert_eq!(codec.decode(&bytes).unwrap(), f);
    }

    #[test]
    fn seq_counter_wraps() {
        let mut codec = FrameCodec::new();
        codec.seq = u16::MAX;
        assert_eq!(codec.next_seq(), u16::MAX);
        assert_eq!(codec.next_seq(), 0);
        assert_eq!(codec.next_seq(), 1);
    }

    #[test]
    fn detects_crc_corruption() {
        let codec = FrameCodec::new();
        let mut bytes = codec.encode(&Frame::write(1, 0x17, vec![0xDE, 0xAD]));
        let crc_idx = bytes.len() - 2;
        bytes[crc_idx] ^= 0xFF;
        assert!(matches!(
            codec.decode(&bytes),
            Err(FrameError::CrcMismatch { .. })
        ));
    }

    #[test]
    fn detects_bad_lead_and_term() {
        let codec = FrameCodec::new();
        let good = codec.encode(&Frame::read(2, 0x15, vec![0x00]));
        let mut bad_lead = good.clone();
        bad_lead[0] = 0x99;
        assert_eq!(codec.decode(&bad_lead), Err(FrameError::BadLead(0x99)));
        let mut bad_term = good.clone();
        *bad_term.last_mut().unwrap() = 0x00;
        assert_eq!(codec.decode(&bad_term), Err(FrameError::BadTerm(0x00)));
    }

    #[test]
    fn detects_length_mismatch() {
        let codec = FrameCodec::new();
        let mut bytes = codec.encode(&Frame::write(3, 0x15, vec![0x01, 0x02]));
        bytes[6] = 0x7F; // absurd declared length
        assert!(matches!(
            codec.decode(&bytes),
            Err(FrameError::LengthMismatch { .. })
        ));
    }

    #[test]
    fn too_short_rejected() {
        let codec = FrameCodec::new();
        assert_eq!(codec.decode(&[0x02, 0xAA]), Err(FrameError::TooShort(2)));
    }

    #[test]
    fn find_and_decode_skips_leading_garbage() {
        let codec = FrameCodec::new();
        let frame = Frame::write(0x0042, 0x15, vec![1, 2, 3]);
        let good = codec.encode(&frame);
        let mut buf = vec![0x00, 0xFF, 0x13]; // junk
        buf.extend_from_slice(&good);
        buf.extend_from_slice(&[0xAB, 0xCD]); // trailing junk
        let (got, consumed) = codec.find_and_decode(&buf).unwrap();
        assert_eq!(got, frame);
        assert_eq!(consumed, 3 + good.len());
    }

    #[test]
    fn find_and_decode_waits_for_incomplete() {
        let codec = FrameCodec::new();
        let good = codec.encode(&Frame::read(1, 0x16, vec![0]));
        // Truncate mid-frame: no complete frame yet.
        assert!(codec.find_and_decode(&good[..good.len() - 2]).is_none());
    }

    #[test]
    fn find_and_decode_recovers_after_false_lead() {
        let codec = FrameCodec::new();
        let frame = Frame::write(7, 0x17, vec![0x00, 0x3C]);
        let good = codec.encode(&frame);
        // A stray 0x02 that isn't a real frame, then the real one.
        let mut buf = vec![0x02, 0x02, 0x02];
        buf.extend_from_slice(&good);
        let (got, _) = codec.find_and_decode(&buf).unwrap();
        assert_eq!(got, frame);
    }
}
