<div align="center">

# 🎚️ ktctl · FiiO JA11 · KTMicro KT02H20 runtime control

### Control your FiiO JA11's PEQ, EQ, and live state from macOS or Linux — no phone, no Windows.

**The FiiO Control Android app is the only way FiiO officially supports tuning the JA11's
5‑band PEQ and gain. This project reverse‑engineers that app's USB control protocol so you
can do the same thing from a terminal — a native Rust CLI + TUI, cross‑platform, no OTG
phone dance required.**

<p>
  <img alt="License: Blue Oak 1.0.0" src="https://img.shields.io/badge/license-Blue%20Oak%201.0.0-1a73e8">
  <img alt="Rust" src="https://img.shields.io/badge/built%20with-Rust-dea584?logo=rust&logoColor=white">
  <img alt="macOS + Linux" src="https://img.shields.io/badge/target-macOS%20%2B%20Linux-000?logo=apple&logoColor=white">
  <img alt="Status: protocol reversed, unimplemented" src="https://img.shields.io/badge/status-protocol%20reversed%2C%20unimplemented-b60205">
  <img alt="Hardware validation: not yet" src="https://img.shields.io/badge/hardware%20validation-not%20yet-b60205">
</p>

</div>

---

## 🎧 The idea

The **FiiO JA11** (and the cheaper KTMicro `KT02H20`-based clones it shares silicon with —
JCALLY JM12, Moondrop's dongle, and more) exposes a **runtime USB control channel** that the
official FiiO Control Android app uses to read and write its 5‑band parametric EQ, EQ
enable/preset state, and global gain. That channel has never been documented — until now.

This is the **sibling project** to
[`ktflash`](https://github.com/ParkWardRR/fiio-ja11-jcally-jm12-moondropkt02-kt02h20-dac-amp-toolkit),
which reverse-engineered the JA11's *firmware-flashing* protocol (cross-flashing JA11 firmware
onto compatible clones). `ktflash` answers "what firmware is on this dongle, and how do I change
it." **`ktctl`** answers "what is this dongle doing *right now*, and how do I tune it" — PEQ
bands, gain, presets — **without ever touching flash**.

| You want to… | Status |
|---|---|
| 🎚️ **Read** the current PEQ bands / gain / preset | ⏳ protocol known, not yet implemented |
| ✏️ **Write** a PEQ band (freq / gain / Q / filter type) | ⏳ protocol known, not yet implemented |
| 🔀 **Switch presets** / toggle PEQ on-off | ⏳ protocol known, not yet implemented |
| 📊 **Live TUI** — see + edit the EQ curve interactively | ⏳ planned, no code yet |
| 🔎 **See how the protocol was reversed** | [`docs/PROTOCOL.md`](docs/PROTOCOL.md) *(coming — ported from the toolkit's `research/android-app-re-findings.md`)* |
| 🗺️ **See the plan** | [ROADMAP.md](ROADMAP.md) |

> [!IMPORTANT]
> **This project is at the "protocol reversed, nothing built yet" stage.** Everything under
> [ROADMAP.md](ROADMAP.md) Phase 1 onward is unimplemented. The protocol itself was recovered by
> *static* reverse-engineering of the FiiO Control Android app (v3.45.0 and v4.0.0) — it has
> **not been confirmed against real JA11 hardware**. Expect to find (and fix) wrong assumptions
> once real hardware is in the loop — see [Phase 0](ROADMAP.md#phase-0--protocol-recovery-) for
> exactly what's confirmed vs. inferred.

---

## 🔍 What's already known (from static RE)

Recovered from the FiiO Control Android app's Java layer (not its Flutter/Dart layer — the
JA11-relevant code turned out to be plain, decompilable Java/Kotlin):

- **Transport**: raw USB bulk transfer against a vendor interface (claimed directly via
  `UsbDeviceConnection`, not the HID class driver, and not the CDC-ACM serial port `ktflash`
  uses for firmware flashing — this is a *different* interface on the same device).
- **Frame format**: `02 <AA|BB> <0A|0B> <seq_hi> <seq_lo> <cmd> <len> <payload…> <crc8> EE` —
  `AA 0A` = write, `BB 0B` = read/query, a 16-bit free-running sequence counter, a
  **CRC-8/MAXIM** (Dallas/Maxim 1-Wire) checksum, and a fixed `0xEE` terminator.
- **Known opcodes**: `0x15` per-PEQ-band get/set (index, Q ×100, gain ×10 dB, freq Hz, filter
  type), `0x16` PEQ enable / active preset slot, `0x17` global/makeup gain (×10 dB).
- **Not yet pinned down**: the exact USB interface/endpoint numbers, whether the leading `0x02`
  is strictly required, and the filter-type enum's exact meaning (peaking / shelf / etc.).

Full detail lives in `ktflash`'s
[`research/android-app-re-findings.md`](https://github.com/ParkWardRR/fiio-ja11-jcally-jm12-moondropkt02-kt02h20-dac-amp-toolkit/blob/main/research/android-app-re-findings.md)
§4 — that write-up will be ported into this repo's `docs/PROTOCOL.md` as Phase 0 work here.

---

## 🚀 Quickstart

There's no binary yet — this repo currently exists to track the plan. Once Phase 1/2 land:

```bash
cd ktctl && cargo build --release
./target/release/ktctl probe          # identify a connected JA11
./target/release/ktctl peq get        # read the 5 PEQ bands + gain + preset
./target/release/ktctl peq set 0 --freq 1000 --gain -3.0 --q 0.7   # write band 0
./target/release/ktctl                # TUI dashboard
```

Follow progress in [ROADMAP.md](ROADMAP.md).

---

## 🙌 How to help

- **A JA11 (or a KT02H20 clone) and a USB capture tool** (Wireshark + `usbmon` on Linux, or a
  hardware USB analyzer) — the single biggest unblock. Recording one PEQ read + one PEQ write
  from the official Android app would confirm or correct everything in
  [Phase 0](ROADMAP.md#phase-0--protocol-recovery-).
- **Rust**: `rusb`/`nusb` experience for the USB transport layer, `ratatui` experience for the
  TUI (this project intentionally mirrors `ktflash`'s stack and style).
- Report findings against real hardware as GitHub issues — even a "this opcode didn't do what
  the app's code implied" is useful signal.

---

## Relationship to `ktflash`

|  | [`ktflash`](https://github.com/ParkWardRR/fiio-ja11-jcally-jm12-moondropkt02-kt02h20-dac-amp-toolkit) | `ktctl` (this repo) |
|---|---|---|
| Question it answers | "What firmware is on this dongle, and how do I change it?" | "What is this dongle doing right now, and how do I tune it?" |
| Protocol | CDC bootloader (`8888:cdc0`), reversed from the Windows vendor tool + independently confirmed from the Android app | Runtime vendor USB interface, reversed from the Android app only |
| Touches flash? | Yes — this is a firmware writer | No — pure runtime control, nothing persisted to flash |
| Status | ✅ native write proven on hardware, v1.2.0 released | ⏳ protocol reversed, nothing implemented yet |

Separate repos on purpose: different risk profile (this never touches flash, so it's much
lower-stakes to run), different protocol, different USB interface — no reason to couple their
release cadences.
