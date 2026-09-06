# Hardware validation log — 2026-09-06

First real-hardware session against a physical JA11. Transport: macOS host, dongle passed
through to an OrbStack Ubuntu guest (`orb usb attach`) because `rusb`/libusb **cannot** claim
this device's HID interface natively on macOS — confirmed directly (`claim interface 3 failed:
Access denied`, unaffected by `sudo`), the same kernel-level `IOHIDFamily` wall `ktflash` already
solved once for the CDC interface via its `ktmac` companion. This is the first time this project
needed to actually route around it itself.

## Bugs found and fixed

### 1. Reply length check was too strict (`proto/frame.rs`)

`decode()` required the USB read buffer to be *exactly* `MIN_FRAME_LEN + declared_len` bytes.
Real USB HID interrupt reads return a **fixed-size, zero-padded report** (observed: 21 bytes)
regardless of the frame's own declared payload length — completely normal HID behavior, not a
device quirk. Fixed to accept any buffer *at least* that long and parse only its meaningful
prefix. See the fix's own doc comment for the exact observed byte counts.

### 2. CRC-8 scope was wrong for replies (`proto/frame.rs`)

The static-RE-derived assumption was CRC over `magic..=last payload byte` (excluding only the
leading `0x02`). Brute-forcing every contiguous byte range of two independent, device-computed
reply CRCs (a `0x0B` firmware reply and a `0x16` preset reply — different `cmd`/`len`/payload/seq,
so not a coincidental match) against the real carried CRC byte shows the actual scope is
**`seq_hi..=last payload byte`** — `magic`/`dir` (bytes 1-2) are excluded entirely. Centralized in
`FrameCodec::crc_scope`; both `encode` and `decode` now share it (previously only `encode` did).

*Caveat correctly narrowed, not fully closed*: both samples happened to have `seq_hi == 0x00`,
and a leading zero byte is a provable no-op for this CRC when the running state is already zero
— so these two samples can't distinguish "starts at `seq_hi`" from "starts at `seq_lo`". The
wider (`seq_hi`-inclusive) scope was chosen as the safe default; re-confirm once a session's
`seq` counter has wrapped past 255 and a nonzero `seq_hi` is observed on the wire.

**Important scope note on this finding itself**: the "request" side of my initial brute-force
search was circular — the target CRCs for outgoing requests were bytes `ktctl`'s own `encode()`
had generated using the (then-unverified) old scope, so "confirming" them against that same old
scope proved nothing about what the *device* expects for incoming requests. Only the two
*replies* (CRCs computed by the device itself) are independent evidence. Given that, `encode()`
was changed to the same corrected scope for both directions — safe either way (a no-op if the
device doesn't validate incoming CRCs at all; a real fix if it does and was silently rejecting
writes before this).

### 3. Writes don't get an ACK on this channel (`device/mod.rs`, `device/usb.rs`)

`ktctl gain -3.0` failed with a `bulk IN` timeout — but reading the value back afterward showed
it had, in fact, been applied. The device does not send a reply frame after a write on this
channel; blocking on `read_bulk()` after every write just burns the full I/O timeout for nothing.
Confirmed independently by the reference `fiiocontrol-oss` WebHID driver, whose writes are all
`sendReport()` calls with no paired synchronous read. Fixed: `Transport::send_write()` is a new
trait method (default falls back to the old symmetric behavior, for the fake/test transport);
`UsbTransport` overrides it to skip the read entirely. No caller ever inspected a write's "reply"
payload, so this is a pure bug fix, not a behavior change for any real use.

## What's now confirmed on real hardware (not just static RE)

- **Transport**: HID-class interface (3), 2 endpoints, `out 0x03`/`in 0x83`, exactly as the
  descriptor-discovery heuristic predicted.
- **Frame format**: `02 <AA|BB> <0A|0B> <seq_hi> <seq_lo> <cmd> <len> <payload…> <crc8> EE`,
  byte-exact, both directions.
- **Per-band PEQ field order** (`0x15`): `index, gain×10 BE i16, freq u16 BE, Q×100 BE i16, type`
  — read directly off the wire (band 0: `freq=1000 Hz`, `Q=0.70`, matching the encoded values
  exactly).
- **State-channel opcodes**: `0x02` volume (read `60`, matching the FIIO support-article
  screenshot exactly), `0x09` sample rate, `0x0B` firmware(-ish — see open item below), `0x12`
  mic detect, `0x20` UAC mode — all decode cleanly.
- **Master gain (`0x17`) `×2560` little-endian**: writing `-3.0 dB` encodes to exactly `-7680`
  (int16 LE) and reads back `-2.9 dB` — a single small quantization step off, not the order-of-
  magnitude mismatch `×10 be` would produce if the scale/endianness were actually wrong. Strong
  evidence for `X2560Le` as the correct default (already `ktctl`'s default); not yet confirmed
  by ear/measurement, only by round-trip.

## Open items surfaced by this session

1. **`0x0B` doesn't match the app's displayed firmware version.** Raw reply payload `02 14`
   decodes (via this project's `"{byte0}.{byte1}"` formula) as `2.20`; the FIIO Control app's own
   Status screen shows `1.4` for what should be the same device (per the earlier research
   screenshots). Possibly two distinct version concepts (a protocol/hardware revision vs. an
   app-displayed firmware build), possibly a formatting bug in this project's decode. Unresolved
   — needs comparing against the app's own reading on the *same* device state, not a different
   session.
2. **Save/commit opcode still untested.** `0x19`/`[3]` vs `0x18`/`[1]` (see `ROADMAP.md`) needs a
   write → save → power-cycle → re-read test; a power cycle requires physically unplugging the
   device, not something scriptable from this session.
3. **`seq_hi`-inclusive vs `seq_lo`-only CRC scope** (see bug #2 above) — needs a session where
   `seq` has wrapped past 255.
4. **Master gain still not audio-confirmed** — the round-trip is self-consistent and the
   magnitude is very plausible, but only a listening/measurement test would fully close this out.
