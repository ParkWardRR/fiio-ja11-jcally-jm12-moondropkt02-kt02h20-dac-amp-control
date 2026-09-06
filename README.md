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
  <img alt="Status: core protocol confirmed on hardware" src="https://img.shields.io/badge/status-core%20protocol%20confirmed%20on%20hardware-2ea44f">
  <img alt="Hardware validation: first pass done 2026-09-06" src="https://img.shields.io/badge/hardware%20validation-first%20pass%20done-2ea44f">
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
> **The core protocol is now confirmed against a real JA11** (2026-09-06) — transport, frame
> format, CRC, and per-band PEQ round-trip all work end-to-end on real hardware, after fixing
> three bugs the static-RE-only implementation had (see
> [`docs/HARDWARE-VALIDATION.md`](docs/HARDWARE-VALIDATION.md)). Still open: the save/commit
> opcode (needs a power-cycle test) and the exact firmware-version semantics — see
> [Phase 0](ROADMAP.md#phase-0--protocol-recovery-) for the full confirmed-vs-open breakdown.

> [!NOTE]
> **Why not iOS?** Confirmed straight from FIIO's own support docs, not just platform
> speculation: [*"How to control the JA11 via the FiiO Control APP in Android mobile
> phone?"*](https://fiiosupport.freshdesk.com/support/solutions/articles/69000869868-how-to-control-the-ja11-via-the-fiio-control-app-in-android-mobile-phone-)
> states plainly — **"The JA11 could not be controlled via the iOS version FiiO Control APP."**
> Apple gives third-party apps no general USB-host API (only `ExternalAccessory` for
> MFi-certified accessories, which this isn't) — the same wall FIIO itself hit. That's why this
> project targets **macOS + Linux** as native desktop apps instead of iOS.

---

## 🎯 What `ktctl` should replicate

Confirmed against FIIO's own support docs
([*"How to control the JA11 via the FiiO Control APP in Android mobile phone?"*](https://fiiosupport.freshdesk.com/support/solutions/articles/69000869868-how-to-control-the-ja11-via-the-fiio-control-app-in-android-mobile-phone-),
screenshots included) — this is the actual, real-device UI `ktctl`'s CLI/TUI is targeting
feature-parity with:

| Screen | What it shows / does |
|---|---|
| **My devices** | Device card: name (`JadeAudio JA11`) + connection status (`connected`). |
| **EQ** | Live curve graph over a 5-band PEQ (bands seen at `29 / 81 / 600 / 7460 / 15660 Hz`, gains `±12 dB` range), a **master gain** slider (separate from the bands, `0 dB` center), an EQ on/off toggle, `Custom` / `Advanced Settings` / `save` actions. |
| **Status** | Device name, **firmware version**, **sample rate** (e.g. `384k`), **in-line microphone** detect (on/off), **UAC version** selector (`UAC 1.0` / `UAC 2.0`, tap to switch), **device volume** (tap to open a detail view). |
| **Guide** | Static help/tutorial content — low priority, not really "control." |

This maps directly onto the opcodes in [ROADMAP.md Phase 0](ROADMAP.md#phase-0--protocol-recovery-):
EQ screen → `0x15`/`0x16`/`0x17`; Status screen → `0x02`/`0x09`/`0x0B`/`0x12`/`0x20`. The
screenshots are strong **semantic** corroboration of what each opcode does (the values on
screen — volume `60`, sample rate `384k`, mic detect `ON`, `UAC 2.0` — line up exactly with
what the RE predicted), though they don't confirm the **wire bytes** — that still needs
hardware. One open discrepancy worth tracking: the Status screen's displayed version (`1.4`)
doesn't match the `V2.2` firmware image analyzed in `ktflash`'s RE — possibly a
protocol/hardware revision string distinct from the flashable firmware build; needs a real
device to resolve.

---

## 🔍 What's already known (from static RE)

Recovered from the FiiO Control Android app's Java layer (not its Flutter/Dart layer — the
JA11-relevant code turned out to be plain, decompilable Java/Kotlin):

- **Transport**: raw USB bulk transfer against the device's **HID-class interface**, claimed
  directly via `UsbDeviceConnection` (bypassing the OS HID class driver), not the CDC-ACM serial
  port `ktflash` uses for firmware flashing — a *different* interface on the same device. The
  interface/endpoints are **descriptor-discovered, not hardcoded**: the first interface with
  `bInterfaceClass == 3` (HID) and exactly 2 endpoints, OUT/IN picked by direction bit.
- **Frame format**: `02 <AA|BB> <0A|0B> <seq_hi> <seq_lo> <cmd> <len> <payload…> <crc8> EE` —
  `AA 0A` = write, `BB 0B` = read/query, a 16-bit free-running sequence counter, a
  **CRC-8/MAXIM** (Dallas/Maxim 1-Wire) checksum, and a fixed `0xEE` terminator.
  **Hardware-confirmed 2026-09-06** — see [`docs/HARDWARE-VALIDATION.md`](docs/HARDWARE-VALIDATION.md).
  The CRC scope was wrong in earlier static-RE-only passes of this doc: it's `seq_hi..=last
  payload byte`, **not** `magic..=last payload byte` — `magic`/`dir` are excluded.
- **Known opcodes**: `0x15` per-PEQ-band get/set (`index, gain ×10 dB, freq Hz, Q ×100, filter
  type` — byte order confirmed against real hardware, band 0 read back `freq=1000 Hz`/`Q=0.70`
  exactly as written), `0x16` PEQ enable / active preset (`0`=Vocal, `1`=Classic, `2`=Bass,
  `3`=USER1/custom, `4`=off — real names confirmed from FIIO's own WebHID site), `0x17`
  master/global gain, `×2560` little-endian — strong hardware evidence (write `-3.0 dB` → reads
  back `-2.9 dB`, a single quantization step off, not the order-of-magnitude mismatch the
  alternative `×10 be` encoding would produce).
- **Filter types**: the JA11 supports 3 of FIIO's 7 shared filter types —
  `0`=Peak, `1`=LowShelf, `2`=HighShelf.
- **Writes don't get an ACK on this channel** — confirmed on hardware; `ktctl` no longer waits
  for one (see `docs/HARDWARE-VALIDATION.md` bug #3).
- **Save/commit-to-flash confirmed**: `cmd 0x19` payload `[0x03]` genuinely persists PEQ edits
  across a power cycle — verified by writing a distinctive band value, saving, confirming a real
  power cycle, and reading it back unchanged. `--save-command 0x18` remains available as an
  untested fallback.
- **Firmware version resolved**: the wire value now matches the app exactly (`1.4`) once the
  second payload byte is read as BCD rather than decimal.

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
