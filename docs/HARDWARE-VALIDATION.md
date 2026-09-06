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

**Fully settled** (was left open as of the first pass at this doc): the original two samples both
had `seq_hi == 0x00`, and a leading zero byte is a provable no-op for this CRC — so they couldn't
distinguish "starts at `seq_hi`" from "starts at `seq_lo`". Wrote a one-off probe
(`examples/seq_wrap_probe.rs`) that drives 260 real read requests in one session to push `seq`
past 255; the five replies with `seq_hi == 0x01` all match the `seq_hi`-inclusive scope and none
match the `seq_lo`-only alternative. `seq_hi` is definitively part of the CRC.

**Aside — a real, reproducible transport quirk found along the way**: the very first read
immediately after a fresh interface claim (right after `orb usb attach`/reattach) returned a
CRC that didn't match a repeat of the exact same request one command later — initially looked
like a deeper protocol mystery (a hidden device-side counter?), but reattaching and reading three
times in a row showed read #1 differs while reads #2 and #3 are identical to each other and to
every other capture of that same query. This is a connection-settling artifact, not a protocol
property — the first bulk IN read after claiming the interface can return a stale/garbage report.
Not yet fixed in code (no repro outside this specific OrbStack passthrough setup to test against),
but worth a defensive "discard the first read after opening" if this recurs elsewhere.

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

### 4. Firmware version formula was wrong (`proto/state.rs`)

The `0x0B` reply's raw payload `02 14` was decoded byte-for-byte as `"{byte0}.{byte1}"` = `"2.20"`,
but the FIIO Control app's Status screen shows `"1.4"` for the same device. `0x14`'s two nibbles
are `1` and `4` — the *second* payload byte is BCD (major in the high nibble, minor in the low),
not two independent decimal integers, and it alone is the user-facing version string. The first
byte (`0x02`) decodes to something else, still unidentified. Fixed `firmware_version()` to read
the second byte's nibbles; confirmed against real hardware (`ktctl state` now prints `1.4`,
matching the app exactly).

### 5. Save/commit-to-flash opcode confirmed

Neither the Windows tool nor the Android app decompile ever found a distinct "persist to flash"
command; two external drivers each guessed a different one. Settled by hardware: wrote band 0 to
`3333 Hz / +4.0 dB / Q 0.55 / low-shelf` — values nothing would produce by accident — issued
`ktctl peq save` (`cmd 0x19`, payload `[0x03]`, `fiiocontrol-oss`'s candidate), confirmed a real
power cycle (the device's USB enumeration changed), and read band 0 back: unchanged. Refactored
`Device::save()`/`SaveCommand` from a "try both, return whichever succeeds" loop (which silently
broke once bug #3 above meant writes never error) into an explicit, CLI-selectable choice
(`--save-command 0x19|0x18`) — `Cmd19Payload3` is confirmed and stays the default.

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

1. ~~`0x0B` doesn't match the app's displayed firmware version~~ **resolved**. Raw reply payload
   `02 14` decodes to `1.4` — an exact match with the FIIO app's Status screen — once the *second*
   byte is read as BCD (`0x14`'s nibbles are `1` and `4`) rather than as two independent decimal
   integers. The first byte (`0x02`) is a separate, still-unidentified field, not part of the
   version string. See `proto::state::firmware_version`'s doc comment.
2. ~~Save/commit opcode~~ **resolved**. `Cmd19Payload3` (`cmd 0x19`, payload `[0x03]`) confirmed
   to persist: wrote band 0 to `3333 Hz / +4.0 dB / Q 0.55 / low-shelf` (values nothing would
   produce by accident), issued `save`, confirmed a real power cycle (device re-enumerated with a
   new bus address), and read band 0 back afterward — unchanged. `Cmd18Payload1` was not
   separately tested since the working candidate was found first.
3. ~~`seq_hi`-inclusive vs `seq_lo`-only CRC scope~~ **resolved** — see bug #2 above.
4. **Master gain still not audio-confirmed** — the round-trip is self-consistent and the
   magnitude is very plausible, but only a listening/measurement test would fully close this out.
   This is now the only unresolved item from this session.
