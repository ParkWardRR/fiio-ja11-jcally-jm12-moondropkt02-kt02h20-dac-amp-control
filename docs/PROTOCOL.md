# 📡 JA11 runtime control protocol

> [!WARNING]
> **Recovered by *static* reverse-engineering of the FiiO Control Android app
> (v3.45.0 + v4.0.0). NOT confirmed against real hardware.** Every byte offset,
> opcode and scaling factor below is the current best understanding and may be
> wrong. This document is the specification the code in `ktctl/` is written
> against; when a hardware USB capture lands, correct this file first, then the
> code, then the golden fixtures in `ktctl/tests/protocol_golden.rs`.

This is the ported-and-consolidated form of §4 of `ktflash`'s
[`research/android-app-re-findings.md`](https://github.com/ParkWardRR/fiio-ja11-jcally-jm12-moondropkt02-kt02h20-dac-amp-toolkit/blob/main/research/android-app-re-findings.md),
scoped to what `ktctl` needs.

---

## 1. Transport

* **Channel**: raw USB **bulk** transfer via `UsbDeviceConnection.bulkTransfer`
  against a *claimed vendor interface* — **not** the CDC-ACM serial port that
  `ktflash` uses for firmware flashing (that is a different interface on the same
  physical device, active only during an OTA/bootloader session).
* **VID/PID**: `2972:0102` for the FiiO JA11 (from `ktflash`; the Android app
  itself carries **no** VID/PID filter list, so this is the value to match on).
* **Interface / endpoints**: ⏳ **unknown**. The decompiled app resolves them at
  runtime from the claimed interface's descriptors rather than hardcoding them.
  `ktctl`'s [`UsbTransport`](../ktctl/src/device/usb.rs) therefore auto-discovers
  a vendor-class (`0xFF`) interface exposing a bulk IN + bulk OUT endpoint pair,
  with manual overrides available in `UsbConfig` once a capture pins the numbers.

## 2. Frame format

```text
 ┌──────┬────────┬────────┬────────┬────────┬──────┬──────┬─────────┬──────┬──────┐
 │ 0x02 │ magic  │  dir   │ seq_hi │ seq_lo │ cmd  │ len  │ payload │ crc8 │ 0xEE │
 └──────┴────────┴────────┴────────┴────────┴──────┴──────┴─────────┴──────┴──────┘
   lead   AA|BB    0A|0B    u16 big-endian    op    n       n bytes   maxim  term
```

| field     | bytes | notes                                                        |
|-----------|-------|--------------------------------------------------------------|
| lead      | 1     | fixed `0x02`. ⏳ *load-bearing?* — unconfirmed.               |
| magic+dir | 2     | `AA 0A` = write, `BB 0B` = read/query.                       |
| seq       | 2     | 16-bit **big-endian** free-running counter.                  |
| cmd       | 1     | opcode (see §3).                                             |
| len       | 1     | payload length in bytes.                                     |
| payload   | `len` | opcode-specific (see §3).                                    |
| crc8      | 1     | CRC-8/MAXIM over `magic … last payload byte` (excludes lead).|
| term      | 1     | fixed `0xEE`.                                                |

**CRC-8/MAXIM** (Dallas/Maxim 1-Wire): width 8, poly `0x31`, init `0x00`,
refin/refout true, xorout `0x00`, check `0xA1`. Implemented (bitwise + table,
cross-checked) in [`proto/crc.rs`](../ktctl/src/proto/crc.rs). ⏳ The exact CRC
*scope* (does it really start at the magic byte, excluding the `0x02`?) is the
single most likely thing to be wrong; it is isolated in `FrameCodec::crc_scope`
for a one-line fix.

## 3. Opcodes (JA11, internal product id `109`)

| cmd    | meaning                         | payload                                                                        |
|--------|---------------------------------|-------------------------------------------------------------------------------|
| `0x15` | per-band PEQ get/set            | `[index, Q×100 (i16 BE), gain×10 dB (i16 BE), freq Hz (u16 BE), filterType]`   |
| `0x16` | PEQ enable / active preset slot | `[value]` — `0..=3` preset slot, `4` = off (inferred)                          |
| `0x17` | global / makeup gain            | `[gain×10 dB (i16 BE)]`                                                        |

**Fixed-point scaling**: gain `×10`, Q `×100`, freq plain Hz. Modelled in
[`proto/peq.rs`](../ktctl/src/proto/peq.rs).

**Filter-type enum** (⏳ inferred): `0` peaking, `1` low-shelf, `2` high-shelf;
any other byte is preserved verbatim (`FilterType::Unknown`).

## 4. Reply frames

⏳ **Unknown.** We have no capture of what the device sends back. `ktctl`'s
fake device *models* a reply as: same seq + opcode as the request, payload =
the resulting stored value. This is a guess purely so the CLI/TUI stack can be
built; a hardware capture must confirm/replace it.

## 5. Open questions (the frontier)

1. Real USB interface number + bulk IN/OUT endpoint addresses.
2. Whether the leading `0x02` is required.
3. Exact CRC scope (with/without the `0x02`).
4. Filter-type enum's true mapping.
5. Reply-frame shape.
6. Device-side valid ranges for freq/gain/Q (CLI currently uses conservative
   client-side guards in `cli/mod.rs::limits`).

Confirming items 1–5 with **one real PEQ read + one real PEQ write** captured
via Wireshark + `usbmon` is the exit criterion for roadmap Phase 0.
