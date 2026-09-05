# 🗺️ Roadmap

A **chronological** map of `ktctl` — the phases in the order they happen, what each delivers,
and exactly where the frontier is today.

**Direction:** native on macOS and Linux, no phone/OTG cable required, no Windows path at all
(the protocol was recovered from an *Android* app, not a Windows tool — there is no Windows
vendor utility for this particular channel to fall back on).

**Legend:** ✅ done · 🚧 in progress · ⏳ planned · ❌ not possible · ⭐ pivotal

> [!CAUTION]
> **This project does not touch flash.** Everything here reads/writes *runtime* state (PEQ
> bands, gain, active preset) held in RAM/config on the device — the same thing the official
> FiiO Control app's EQ screen does. If you're looking to change firmware or cross-flash a
> clone dongle, that's [`ktflash`](https://github.com/ParkWardRR/fiio-ja11-jcally-jm12-moondropkt02-kt02h20-dac-amp-toolkit),
> a separate, separately-risky tool.

> ## 📍 You are here
>
> **Phase 0's static RE is now complete**: frame format, opcodes, interface/endpoint discovery,
> and the filter-type enum are all recovered from the FiiO Control Android app (v3.45.0 and
> v4.0.0) — see
> [ktflash's `research/android-app-re-findings.md` §4](https://github.com/ParkWardRR/fiio-ja11-jcally-jm12-moondropkt02-kt02h20-dac-amp-toolkit/blob/main/research/android-app-re-findings.md)
> for the full write-up this project is built on. **Nothing has been confirmed against real
> hardware yet, and no code exists in this repo.** The frontier is now purely hardware
> validation — a USB capture, or just trying it against a real JA11 — before writing Phase 1's
> fixtures against possibly-wrong assumptions.

---

## The timeline

```text
Phase 0  Protocol recovery (static RE + hardware validation)  ⭐ ... 🚧 static done, HW ⏳
Phase 1  Hardware-free protocol core (frame codec, CRC-8) ............ ⏳ planned
Phase 2  Native USB transport (macOS + Linux, no OrbStack needed) ⭐ .. ⏳ planned
Phase 3  CLI: probe / read / write PEQ, gain, preset .................. ⏳ planned
Phase 4  TUI: live dashboard + interactive EQ curve editor ............ ⏳ planned
Phase 5  Packaging + release (binaries, both platforms) ............... ⏳ planned
```

---

## Phase 0 — Protocol recovery ⭐ 🚧 *(static done, hardware validation pending)*

1. ✅ **Static RE from the FiiO Control Android app** (v3.45.0 + v4.0.0, both byte-identical on
   this code path). Full method: `apktool` for manifest/resources, `jadx` for full Java
   decompilation — no `blutter`/Dart RE needed, since the JA11 module and its shared device
   command-frame builder are plain (obfuscated but readable) Java, not compiled into the
   Flutter/Dart `libapp.so`.
2. ✅ **Transport identified**: raw USB bulk transfer via `UsbDeviceConnection.bulkTransfer`
   against a claimed vendor interface — confirmed **not** the CDC-ACM serial port `ktflash`
   uses for firmware flashing (that's a different USB interface on the same device, used only
   during an OTA/bootloader session).
3. ✅ **Frame format recovered**:
   `02 <AA|BB> <0A|0B> <seq_hi> <seq_lo> <cmd> <len> <payload…> <crc8> EE`
   — `AA 0A` write-frame magic, `BB 0B` read-frame magic, 16-bit big-endian free-running
   sequence counter, single-byte opcode, single-byte payload length, **CRC-8/MAXIM**
   (Dallas/Maxim 1-Wire, poly `0x31` reflected, init 0) over the frame from the magic byte
   through the payload, fixed `0xEE` terminator.
4. ✅ **Opcodes recovered** for the JA11 (internal product id `109`):
   | cmd | meaning | payload |
   |---|---|---|
   | `0x15` | per-band PEQ get/set | `[index, Q×100 (i16 BE), gain×10dB (i16 BE), freq Hz (u16 BE), filterType]` |
   | `0x16` | PEQ enable / active preset slot | `[value]` (0-3 = preset slot, 4 = off — inferred) |
   | `0x17` | global/makeup gain | `[gain×10dB (i16 BE)]` |

   Plus, on a **separate frame-builder "channel"** used by the state tab (same wire format,
   different opcode namespace context — see the RE write-up §4b): `0x02` volume, `0x09`
   sample-rate/format (indexed into a 15-entry PCM/DSD table), `0x0B` firmware version
   (`"{major}.{minor}"`), `0x12` in-line mic detect, `0x20` UAC 1.0/2.0 mode (read+write). None
   of this is PEQ, but it's all reachable the same way and worth exposing from `ktctl` too —
   see Phase 3.
5. ✅ **Interface/endpoint discovery resolved**: no hardcoded numbers — the app scans
   `UsbDevice.getInterface(i)` for the one with `bInterfaceClass == 3` (HID) and exactly 2
   endpoints, then picks OUT/IN by direction bit, force-claiming it (detaches the kernel HID
   driver, same class of problem `ktflash` hit on macOS with `IOHIDFamily`). `ktctl` can use
   the identical heuristic against `rusb`/`nusb` interface descriptors.
6. ✅ **Filter-type enum resolved**: FIIO's shared PEQ UI defines 7 types (Peak, LowShelf,
   HighShelf, BandPass, LowPass, HighPass, AllPass), but the **JA11's own band-edit screen
   only offers 3** — so on a JA11, `filterType` is `0`=Peak, `1`=LowShelf, `2`=HighShelf.
7. ⏳ **Hardware validation** (the actual frontier — everything else in Phase 0 is now static-
   complete): confirm the frame format, opcodes, and field encodings against a **real JA11**,
   ideally via a USB capture (Wireshark + `usbmon` on Linux) of the official Android app doing
   one PEQ read and one PEQ write. Without this, Phase 1 risks building fixtures around a wrong
   assumption the same way `ktflash`'s CRC-32 scope was initially mis-scoped (payload-only vs.
   header+payload) before hardware caught it. Remaining unknowns to settle this way: whether
   the leading `0x02` byte is load-bearing or an artifact of one specific code path, and the
   exact byte range the CRC-8 covers.

**This phase's exit criterion**: one real read and one real write against a JA11, byte-compared
against what §4 of the RE write-up predicts. Until then, everything below is provisional.

---

## Phase 1 — Hardware-free protocol core ⏳

Mirrors `ktflash`'s Phase 1: build everything that doesn't need a device plugged in, so the
transport layer (Phase 2) has something solid to sit on.

1. ⏳ `proto::ktctl` crate module: frame encode/decode (`FrameCodec`), CRC-8/MAXIM
   implementation + table, matching the JA11's own table byte-for-byte (already extracted from
   the Android app's `qg.a.f17478d` — see the RE write-up).
2. ⏳ PEQ band model: freq/gain/Q/filter-type struct with the exact fixed-point scaling
   recovered in Phase 0 (`×10` for gain, `×100` for Q, plain Hz for freq).
3. ⏳ A `FakeDevice` (mirroring `ktflash`'s `FakeBootloader`) that replies to encoded frames
   with plausible fixture responses, so the CLI/TUI can be built and tested without hardware.
4. ⏳ Unit tests for the codec + CRC against golden fixtures captured in Phase 0.

---

## Phase 2 — Native USB transport ⭐ ⏳

1. ⏳ Device discovery: enumerate USB devices, identify a JA11 (and, once Phase 6-equivalent
   compatibility work happens, KT02H20 clones) by descriptor — **no VID/PID device-filter list
   exists in the Android app** (checked during Phase 0's RE pass), so this needs either the
   same VID/PID facts `ktflash` already has (`2972:0102` for JA11) or fresh enumeration.
2. ⏳ Claim the HID-class interface (not the CDC-ACM one) using the known discovery heuristic
   (`bInterfaceClass == 3`, exactly 2 endpoints, OUT/IN picked by direction bit) via `rusb`/
   `nusb` interface descriptors — logic is known (Phase 0), just needs porting to Rust and
   trying against real hardware.
3. ⏳ macOS + Linux support via `rusb`/`nusb`, matching `ktflash`'s "no OrbStack needed on
   either platform" bar it eventually reached — but this project's I/O pattern (interactive
   bulk request/reply, not a bulk firmware transfer) is different enough that its own
   perms/rules story should be checked fresh rather than assumed identical.
4. ⏳ Read/write round-trip against real hardware — the first point this whole project becomes
   real rather than theoretical.

---

## Phase 3 — CLI: probe / read / write ⏳

1. ⏳ `ktctl probe` — identify a connected device, report firmware version if available over
   this channel.
2. ⏳ `ktctl peq get` — read all 5 PEQ bands + global gain + active preset, human-readable and
   `--json`.
3. ⏳ `ktctl peq set <band> --freq --gain --q --type` — write a single band.
4. ⏳ `ktctl preset <0-3|off>` — switch the active PEQ preset / disable PEQ.
5. ⏳ `ktctl gain <dB>` — set global/makeup gain.
6. ⏳ `ktctl state` — volume, sample-rate/format, firmware version, mic-detect, UAC mode (the
   "device state channel" opcodes from Phase 0's §4b: `0x02`/`0x09`/`0x0B`/`0x12`/`0x20`).
7. ⏳ `ktctl uac <1|2>` — switch UAC 1.0/2.0 mode (`0x20`, read+write).
8. ⏳ Safety: refuse out-of-range values client-side (the device's own valid ranges aren't yet
   known — Phase 0/2 hardware validation should surface them).

---

## Phase 4 — TUI: live dashboard ⏳

1. ⏳ `ratatui`-based live view (matching `ktflash`'s TUI stack/style) — read-only dashboard
   showing all 5 bands as an EQ curve, gain, active preset.
2. ⏳ Interactive band editing (arrow keys / number entry) with a live-updating curve.
3. ⏳ Preset switching from the TUI.

---

## Phase 5 — Packaging + release ⏳

1. ⏳ macOS universal binary + Linux `x86_64`/`aarch64` static builds, following `ktflash`'s
   `cargo-zigbuild --features vendored` approach for a dependency-free Linux binary.
2. ⏳ `.deb`/`.rpm` packaging + udev rules, if Phase 2 needs non-root device access on Linux.
3. ⏳ First tagged release once Phase 3 (CLI) is hardware-confirmed — the TUI (Phase 4) doesn't
   need to be done first.

---

## 💡 Ideas / nice-to-have

- **Shared crate with `ktflash`** for device enumeration/identification, if the two protocols'
  transport layers turn out to share enough plumbing to be worth de-duplicating.
- **PEQ preset import/export** as a portable file format, so tunings can be shared the way
  firmware images are for `ktflash`.
- **Other FiiO products sharing this same frame shape** — the Android app's command-frame
  builder (`qa.b`-equivalent) is reused across many FiiO device modules (BTR7, K9, Q5s, and
  others were seen referencing the same `{commandType, payLoadMsg}`/opcode pattern during
  Phase 0's RE pass). If there's demand, the same approach could extend to those, but this
  repo's scope stays JA11-only unless that changes.
- **BLE variant**: the Android app's command builder has an `isUsb`-flagged branch and a
  presumed BLE sibling (no leading `0x02`, no USB dependency) — out of scope here (the JA11 has
  no radio), but worth noting for any future sibling project targeting FiiO's BLE products.

---

## 🙌 How to help

See [README.md § How to help](README.md#-how-to-help) — the single highest-leverage
contribution right now is a **USB capture of the official Android app doing a PEQ read/write
against a real JA11**, which would move Phase 0 from "static, unconfirmed" to "hardware-proven"
the same way `ktflash`'s CDC bootloader protocol was confirmed before its first native write.
