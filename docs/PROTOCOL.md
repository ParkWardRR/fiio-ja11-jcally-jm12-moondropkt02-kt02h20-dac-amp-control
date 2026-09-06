# 📡 JA11 runtime control protocol

> [!WARNING]
> **Recovered by *static* reverse-engineering of the FiiO Control Android app
> (v3.45.0 + v4.0.0), semantically corroborated by FIIO's own support
> screenshots, but NOT byte-confirmed against real hardware.** Offsets, opcodes
> and scalings below are the current best understanding. This is the spec the
> code in `ktctl/` is written against; when a hardware USB capture lands,
> correct this file first, then the code, then the golden fixtures in
> `ktctl/tests/protocol_golden.rs`.

Ported/consolidated from §4 of `ktflash`'s
[`research/android-app-re-findings.md`](https://github.com/ParkWardRR/fiio-ja11-jcally-jm12-moondropkt02-kt02h20-dac-amp-toolkit/blob/main/research/android-app-re-findings.md),
scoped to what `ktctl` needs. Last reconciled with `main`'s docs on 2026-09-05
(byte-order fix, preset names, gain-encoding ambiguity, HID-class discovery,
device-state channel).

---

## 1. Transport

* **Channel**: raw USB **bulk** transfer via `UsbDeviceConnection.bulkTransfer`
  against the device's **HID-class interface**, claimed directly (bypassing the
  OS HID driver) — **not** the CDC-ACM serial port `ktflash` uses for flashing.
* **VID/PID**: `2972:0102` for the FiiO JA11 (the Android app carries no VID/PID
  filter list, so match on this).
* **Interface / endpoints** ✅ *resolved (static)*: the app scans for the first
  interface with `bInterfaceClass == 3` (HID) and **exactly 2 endpoints**, then
  picks OUT/IN by direction bit and force-claims it (detaching the kernel HID
  driver — same class of problem `ktflash` hit with `IOHIDFamily` on macOS).
  [`UsbTransport`](../ktctl/src/device/usb.rs) ports this heuristic to `rusb`
  and exposes `UsbConfig { interface, ep_out, ep_in }` overrides.

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
| cmd       | 1     | opcode (see §3/§4).                                          |
| len       | 1     | payload length in bytes.                                     |
| payload   | `len` | opcode-specific.                                            |
| crc8      | 1     | CRC-8/MAXIM over `magic … last payload byte` (excludes lead).|
| term      | 1     | fixed `0xEE`.                                                |

**CRC-8/MAXIM** (Dallas/Maxim 1-Wire): width 8, poly `0x31`, init `0x00`,
refin/refout true, xorout `0x00`, check `0xA1`. Implemented (bitwise + table,
cross-checked) in [`proto/crc.rs`](../ktctl/src/proto/crc.rs). ⏳ The exact CRC
*scope* is the most likely thing to be wrong; it is isolated in
`FrameCodec::crc_scope` for a one-line fix.

## 3. EQ channel (EQ screen)

| cmd    | meaning                    | payload                                                             |
|--------|----------------------------|--------------------------------------------------------------------|
| `0x15` | per-band PEQ get/set       | `[index, gain×10 dB (i16 BE), freq Hz (u16 BE), Q×100 (i16 BE), filterType]` |
| `0x16` | PEQ enable / active preset | `[value]`                                                          |
| `0x17` | master / global gain       | see below — **encoding ambiguous**                                 |

* **`0x15` byte order — CORRECTED 2026-09-05**: gain comes **before** freq/Q (the
  original write-up had them reversed; caught by re-deriving offsets from the
  Android app's `arraycopy` calls and cross-checking `fiiocontrol-oss`). Scaling:
  gain `×10`, Q `×100`, freq plain Hz. Modelled in
  [`proto/peq.rs`](../ktctl/src/proto/peq.rs).
* **`0x15` filter types** ✅ *resolved*: the JA11 offers 3 of FIIO's 7 shared
  types — `0`=Peak, `1`=LowShelf, `2`=HighShelf.
* **`0x16` preset table** ✅ *names confirmed* (FIIO WebHID site): `0`=Vocal,
  `1`=Classic, `2`=Bass, `3`=USER1 (custom), `4`=off.
* **`0x17` gain encoding** ⏳ *ambiguous*: this project's Android-only RE says
  `×10` **big-endian**; two independent hardware-facing drivers
  (`fiiocontrol-oss`, `glacier-eq`) both use `×2560` **little-endian** and agree
  with each other. `ktctl` defaults to `×2560`/LE ([`GainEncoding::X2560Le`]) and
  can switch to `×10`/BE via `Device::with_gain_encoding`.

Real JA11 EQ-screen values (from FIIO screenshots) used as `ktctl` defaults:
bands at `29 / 81 / 600 / 7460 / 15660 Hz`, gain range `±12 dB`.

## 4. Device-state channel (Status screen)

Same wire frame, different opcode namespace context (RE write-up §4b), modelled
in [`proto/state.rs`](../ktctl/src/proto/state.rs):

| cmd    | meaning                | payload / notes                                    |
|--------|------------------------|----------------------------------------------------|
| `0x02` | device volume          | `[value]` (screenshot showed `60`)                 |
| `0x09` | sample rate / format   | `[index]` into a 15-entry PCM/DSD table (`384k`…)   |
| `0x0B` | firmware version       | `"{major}.{minor}"` (screenshot showed `1.4`)      |
| `0x12` | in-line mic detect     | `[0|1]`                                            |
| `0x20` | UAC 1.0 / 2.0 (rd+wr)  | `[1|2]` (screenshot showed `UAC 2.0`)              |

## 5. Save / commit — UNRESOLVED

Neither this project's RE nor the app decompile found a distinct save/commit
opcode. Two external drivers each claim one and **disagree**:

| source            | cmd    | payload | JA11 status         |
|-------------------|--------|---------|---------------------|
| `fiiocontrol-oss` | `0x19` | `[3]`   | claimed **working** |
| `glacier-eq`      | `0x18` | `[1]`   | `Testing`/unconfirmed |

`ktctl`'s `Device::save()` tries `0x19/[3]` first, then `0x18/[1]`
([`opcode::SAVE_CANDIDATES`]). Hardware must settle which is real (or whether
band writes already persist immediately).

## 6. Reply frames

⏳ **Unknown.** No capture of device replies exists. `ktctl`'s fake device models
a reply as "same seq + opcode, payload = resulting value." Hardware must
confirm/replace this.

## 7. Open questions (the frontier)

1. `0x17` gain encoding: `×10`/BE vs `×2560`/LE.
2. Save opcode: `0x19/[3]` vs `0x18/[1]` vs "writes persist immediately."
3. Whether the leading `0x02` is required; exact CRC scope.
4. Reply-frame shape.
5. `0x09` sample-rate table's real ordering/count (`ktctl`'s is inferred).
6. Version discrepancy: Status screen shows `1.4`, `ktflash` sees firmware `V2.2`
   — possibly two different counters.
7. Device-side valid ranges (CLI uses `±12 dB` guards from the screenshots).

Confirming 1–4 with **one real PEQ read + one real PEQ write** captured via
Wireshark + `usbmon` is the exit criterion for roadmap Phase 0.
