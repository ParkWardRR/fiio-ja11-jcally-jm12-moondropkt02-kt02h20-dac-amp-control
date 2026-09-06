# 🗺️ Roadmap

A **chronological** map of `ktctl` — the phases in the order they happen, what each delivers,
and exactly where the frontier is today.

**Direction:** native on macOS and Linux, no phone/OTG cable required, no Windows path at all
(the protocol was recovered from an *Android* app, not a Windows tool — there is no Windows
vendor utility for this particular channel to fall back on).

**Legend:** ✅ done · 🚧 in progress · ⏳ planned · ❌ not possible · ⭐ pivotal

> [!CAUTION]
> **This project does not touch flash**, with one exception: `peq save` persists EQ settings
> (bands, gain, preset) to the device's own storage — that's the only irreversible-ish write
> this tool does, and it's exactly what the official app's "save" button does too. Nothing here
> writes firmware. If you're looking to change firmware or cross-flash a clone dongle, that's
> [`ktflash`](https://github.com/ParkWardRR/fiio-ja11-jcally-jm12-moondropkt02-kt02h20-dac-amp-toolkit),
> a separate, separately-risky tool.

> ## 📍 You are here
>
> **The software is built and hardware-confirmed.** Phases 0-3 are done; Phase 4 (TUI) is built
> and works against the simulator; Phase 5 (packaging) has CI + release workflows already in
> place, pending a first tagged release. Two real-hardware sessions (2026-09-06) found and fixed
> four implementation bugs, then confirmed a full read → write → save → power-cycle → re-read
> round trip against a physical JA11. Full log:
> [`docs/HARDWARE-VALIDATION.md`](docs/HARDWARE-VALIDATION.md). The only unresolved item from
> either session: master gain's `×2560` little-endian encoding is round-trip-confirmed but not
> yet audio-confirmed. See [How to help](#-how-to-help).

---

## The timeline

```text
Phase 0  Protocol recovery (static RE + hardware validation)  ⭐ ......... ✅ done
Phase 1  Hardware-free protocol core (frame codec, CRC-8) ................ ✅ done
Phase 2  Native USB transport (macOS + Linux) ⭐ .......................... ✅ done, HW-confirmed
Phase 3  CLI: probe / read / write PEQ, gain, preset, state, uac, save ... ✅ done, HW-confirmed
Phase 4  TUI: live dashboard + interactive EQ curve editor ............... ✅ done (sim-tested)
Phase 5  Packaging + release (binaries, udev rules, both platforms) ...... 🚧 CI done, no release yet
```

---

## Phase 0 — Protocol recovery ⭐ ✅

1. ✅ **Static RE from the FiiO Control Android app** (v3.45.0 + v4.0.0, both byte-identical on
   this code path). Full method: `apktool` for manifest/resources, `jadx` for full Java
   decompilation — no `blutter`/Dart RE needed, since the JA11 module and its shared device
   command-frame builder are plain (obfuscated but readable) Java, not compiled into the
   Flutter/Dart `libapp.so`.
2. ✅ **Transport identified**: raw USB bulk transfer via `UsbDeviceConnection.bulkTransfer`
   against a claimed vendor interface — confirmed **not** the CDC-ACM serial port `ktflash`
   uses for firmware flashing (that's a different USB interface on the same device, used only
   during an OTA/bootloader session).
3. ✅ **Frame format** — `02 <AA|BB> <0A|0B> <seq_hi> <seq_lo> <cmd> <len> <payload…> <crc8> EE`,
   **hardware-confirmed** byte-exact in both directions. `AA 0A` write-frame magic, `BB 0B`
   read-frame magic, 16-bit big-endian free-running sequence counter, single-byte opcode,
   single-byte payload length, **CRC-8/MAXIM** (Dallas/Maxim 1-Wire, poly `0x31` reflected,
   init 0) computed over **`seq_hi..=last payload byte`** — **not** `magic..=last payload byte`
   as originally assumed from static RE (`magic`/`dir` are excluded entirely). The `seq_hi`
   boundary itself was also ambiguous until a dedicated probe drove `seq` past 255 in one
   session; five real `seq_hi=0x01` replies settled it. See
   [`docs/HARDWARE-VALIDATION.md`](docs/HARDWARE-VALIDATION.md) for the full brute-force evidence
   on both points.
4. ✅ **Opcodes recovered and confirmed from the wire** for the JA11 (internal product id `109`):
   | cmd | meaning | payload |
   |---|---|---|
   | `0x15` | per-band PEQ get/set | `[index, gain×10dB (i16 BE), freq Hz (u16 BE), Q×100 (i16 BE), filterType]` — band 0 read back `freq=1000 Hz`/`Q=0.70` exactly as written |
   | `0x16` | PEQ enable / active preset | `[value]`: `0`=Vocal, `1`=Classic, `2`=Bass, `3`=USER1 (custom), `4`=off — real names confirmed from FIIO's own WebHID site (not "Classic/Pop/Jazz", a different FIIO product's preset set) |
   | `0x17` | master/global gain | `×2560` little-endian — write `-3.0 dB` → reads back `-2.9 dB` (one quantization step off, not the order-of-magnitude mismatch `×10 be` would produce); the one item not yet audio-confirmed |
   | `0x19` | save/commit to persistent storage | payload `[0x03]` — confirmed: wrote a distinctive band, saved, power-cycled the device for real (USB re-enumerated), read it back unchanged |

   Plus, on the same frame format, a **device-state channel** used by the Status tab: `0x02`
   volume, `0x09` sample-rate/format (indexed into a 15-entry PCM/DSD table), `0x0B` firmware
   version (BCD-encoded second byte — `0x14` → `"1.4"`, an exact match with the official app;
   the first byte is a separate, still-unidentified field), `0x12` in-line mic detect, `0x20`
   UAC 1.0/2.0 mode (read+write). All exposed via `ktctl state` / `ktctl uac`.
5. ✅ **Interface/endpoint discovery resolved and ported to Rust**: no hardcoded numbers — scans
   for the interface with `bInterfaceClass == 3` (HID) and exactly 2 endpoints, picks OUT/IN by
   direction bit, force-claims it. **Confirmed on real hardware, both via a Linux guest and
   directly on the macOS host**: picked interface 3, endpoints `0x03`/`0x83`, exactly as the
   heuristic predicted — `rusb` claims this interface fine natively on macOS, no companion app
   or Linux passthrough required. (An earlier note here, by analogy with `ktflash`'s own
   `IOHIDFamily` wall on a *different* interface, wrongly assumed macOS couldn't do this
   directly — corrected once tested outside an environment that had been silently blocking the
   claim.)
6. ✅ **Filter-type enum resolved**: FIIO's shared PEQ UI defines 7 types (Peak, LowShelf,
   HighShelf, BandPass, LowPass, HighPass, AllPass), but the **JA11's own band-edit screen
   only offers 3** — so on a JA11, `filterType` is `0`=Peak, `1`=LowShelf, `2`=HighShelf.
7. ✅ **Hardware validation, two sessions, 2026-09-06.** Found and fixed four real
   implementation bugs along the way: a too-strict reply-length check (real USB HID reads
   return a fixed, zero-padded report, not one trimmed to the declared length), the CRC scope
   above, writes timing out waiting for an ACK the device never sends, and the firmware-version
   formula. Also found (not yet fixed, no local repro outside the specific OrbStack setup): the
   very first read after a fresh interface claim can return a stale/garbage report — a
   connection-settling artifact. Full log: `docs/HARDWARE-VALIDATION.md`.

**Exit criterion — met**: real reads, a real write/read-back round trip, and a real
save/power-cycle/re-read round trip against a JA11, matching predictions once the bugs above
were fixed.

---

## Phase 1 — Hardware-free protocol core ✅

Everything that doesn't need a device plugged in, so the transport layer sits on something
solid. `ktctl/src/proto/`:

- `frame.rs` — `FrameCodec` (encode/decode, the confirmed CRC scope centralised in one function).
- `crc.rs` — CRC-8/MAXIM, matching the JA11's own table byte-for-byte.
- `peq.rs` — PEQ band model, `GainEncoding` (both candidate scales, selectable), `PresetState`,
  `FilterType`.
- `state.rs` — device-state channel decode (volume/rate/firmware/mic/UAC).
- `opcode.rs` — all recovered opcodes, `SaveCommand` (both candidates, selectable).
- `response.rs` — synthesized EQ response-curve math for the TUI's live chart.
- A `FakeDevice` (`device/fake.rs`) that replies to encoded frames with fixture responses seeded
  from FIIO's own screenshots, so the CLI/TUI are fully usable and testable without hardware
  (`--fake`).
- 67 tests, including byte-exact golden fixtures (`tests/protocol_golden.rs`) and a hardware
  probe (`examples/seq_wrap_probe.rs`) for one-off real-device diagnostics.

---

## Phase 2 — Native USB transport ⭐ ✅ *hardware-confirmed*

1. ✅ Device discovery (`device/usb.rs`, `list_devices`) — enumerates by VID/PID
   (`2972:0102` for JA11; no VID/PID device-filter list exists in the Android app, so KT02H20
   clones aren't auto-identified yet, see Compatibility in the README).
2. ✅ Claims the HID-class interface (not the CDC-ACM one) via the descriptor-discovery
   heuristic from Phase 0 — **confirmed on real hardware**: picked interface 3, endpoints
   `0x03`/`0x83`.
3. ✅ Hardware-confirmed on **both** Linux (via an OrbStack guest) and **directly on macOS** —
   `rusb` claims the HID interface fine natively on macOS, no passthrough, no companion app.
   (This roadmap briefly claimed otherwise, reasoning by analogy with `ktflash`'s own
   `IOHIDFamily` wall on its *different* CDC interface — wrong for this one. The actual gate on
   macOS is a **TCC privacy grant**: the terminal app needs **Input Monitoring** access
   (System Settings → Privacy & Security → Input Monitoring) for `rusb` to claim a raw HID
   device. `claim interface 3 failed: Access denied` on macOS means check that setting, not
   reach for a Linux guest.)
4. ✅ Read/write round-trip against real hardware, including the save/power-cycle/re-read cycle
   — this is the point the whole project stopped being theoretical.

---

## Phase 3 — CLI: probe / read / write ✅ *hardware-confirmed*

All implemented in `ktctl/src/cli/`, confirmed against real hardware this session:

1. ✅ `ktctl probe` — identify a connected device, report firmware version and active preset.
2. ✅ `ktctl peq get` — read all 5 PEQ bands + global gain + active preset, table or `--json`.
3. ✅ `ktctl peq set <band> --freq --gain --q --type` — write a single band.
4. ✅ `ktctl preset <name|0-4>` — switch the active PEQ preset / disable PEQ.
5. ✅ `ktctl gain <dB>` — set global/makeup gain.
6. ✅ `ktctl state` — volume, sample-rate/format, firmware version, mic-detect, UAC mode.
7. ✅ `ktctl uac <1|2>` — switch UAC 1.0/2.0 mode (`0x20`, read+write).
8. ✅ `ktctl peq save` — commit PEQ edits to persistent storage. **Confirmed on real hardware**:
   `cmd 0x19` payload `[0x03]` (`fiiocontrol-oss`'s candidate) is the default and is proven to
   persist. `--save-command 0x18` remains available (`glacier-eq`'s untested alternative) in
   case a future firmware revision needs it.
9. ✅ `ktctl list` — enumerate connected JA11/KT02H20-family devices.
10. ⏳ Safety: client-side range guards exist (freq/gain/Q bounds from FiiO's UI) but the
    device's own true limits (if any differ) aren't yet confirmed.

---

## Phase 4 — TUI: live dashboard ✅ *(built, simulator-tested; not yet hardware-driven end-to-end)*

`ktctl/src/tui/` — `ratatui`-based live view:

1. ✅ Live view of all 5 bands as an EQ response curve, gain, active preset.
2. ✅ Interactive band editing (arrow keys / number entry) with a live-updating curve —
   edits stage locally until `w` writes them.
3. ✅ Preset cycling from the TUI.
4. ⏳ A full hardware-driven TUI session (not just individual CLI commands) hasn't been run yet
   this session — the underlying `Device` layer is the same as the CLI's, so this is expected to
   work, but isn't independently confirmed.

---

## Phase 5 — Packaging + release 🚧

1. ✅ CI (`​.github/workflows/ci.yml`) and a release workflow (`release.yml`) already exist.
2. ✅ Linux udev rules + `packaging/INSTALL.md` drafted.
3. ⏳ No tagged release yet — reasonable to cut one now that the core protocol is
   hardware-confirmed, pending the master-gain audio-check and maybe a first clone
   (JM12/Moondrop) report.
4. ⏳ macOS universal binary + Linux static builds, following `ktflash`'s
   `cargo-zigbuild --features vendored` approach for a dependency-free Linux binary.

---

## 💡 Ideas / nice-to-have

- **Shared crate with `ktflash`** for device enumeration/identification, if the two protocols'
  transport layers turn out to share enough plumbing to be worth de-duplicating.
- **PEQ preset import/export** as a portable file format, so tunings can be shared the way
  firmware images are for `ktflash`.
- **Other FiiO products sharing this same frame shape** — the Android app's command-frame
  builder is reused across many FiiO device modules (BTR7, K9, Q5s, and others were seen
  referencing the same opcode pattern during the RE pass). If there's demand, the same approach
  could extend to those, but this repo's scope stays JA11-only unless that changes.
- **BLE variant**: the Android app's command builder has a USB-flagged branch and a presumed
  BLE sibling (no leading `0x02`, no USB dependency) — out of scope here (the JA11 has no
  radio), but worth noting for any future sibling project targeting FiiO's BLE products.

---

## 🙌 How to help

- **Audio-confirm the master-gain encoding** — the single open item from two hardware sessions.
  Set a gain value, listen (or measure), confirm `×2560`/little-endian produces the expected dB
  change.
- **Try `ktctl` against a JCALLY JM12 or Moondrop KT02H20 clone** — protocol should carry over
  since it's the same silicon, but nobody's confirmed it on a clone yet.
- [Open an issue](https://github.com/ParkWardRR/fiio-ja11-jcally-jm12-moondropkt02-kt02h20-dac-amp-control/issues)
  for anything else — even "this opcode didn't do what was expected" is useful signal.
