<div align="center">

# 🎚️ ktctl

**Control the FiiO JA11's parametric EQ, presets, and gain from your terminal — on macOS and Linux, with no Android phone, no Windows, and no firmware flashing.**

A native Rust **CLI + TUI** that speaks the JA11's runtime USB control protocol directly. Also targets the KTMicro **KT02H20** clones that share the same silicon (JCALLY JM12, Moondrop dongles).

<p>
  <img alt="License: Blue Oak 1.0.0" src="https://img.shields.io/badge/license-Blue%20Oak%201.0.0-1a73e8">
  <img alt="Built with Rust" src="https://img.shields.io/badge/Rust-dea584?logo=rust&logoColor=white">
  <img alt="macOS + Linux" src="https://img.shields.io/badge/macOS%20%2B%20Linux-000?logo=apple&logoColor=white">
  <img alt="Status: core protocol confirmed on hardware" src="https://img.shields.io/badge/core%20protocol-confirmed%20on%20hardware-2ea44f">
  <img alt="Hardware validation: two sessions done" src="https://img.shields.io/badge/hardware%20validation-2%20sessions%20done-2ea44f">
</p>

</div>

---

## The interactive TUI

Run `ktctl` with no arguments for a two-tab live dashboard — `Status` and `EQ`, mirroring the
official FiiO Control app's own tab structure (its third tab, "Guide", is just static help text
and isn't reproduced here). `Tab` switches views; arrow keys navigate and edit within each.
Visual theme matches `ktflash`'s TUI on purpose, so the two tools feel like a matched pair.

![ktctl TUI: a live 5-band EQ bar chart with keyboard editing](docs/media/tui.gif)

> The recording above predates the two-tab redesign — regenerate with `vhs docs/media/tui.tape`
> once the `.tape` script is updated to show both views (see "How to help").

## The CLI

Every setting is scriptable, with `--json` for automation.

![ktctl CLI: probe, peq get, peq set, and state commands](docs/media/cli.gif)

> Both recordings above run against the built-in simulator (`--fake`) — no hardware required to reproduce them.

---

## Why this exists

FiiO only ships EQ control for the JA11 as an **Android** app. There's no iOS app — Apple gives
third-party apps no general USB-host API (only `ExternalAccessory` for MFi-certified accessories,
which this isn't), and [FiiO's own support docs confirm the JA11 can't be controlled from the iOS
FiiO Control app](https://fiiosupport.freshdesk.com/support/solutions/articles/69000869868-how-to-control-the-ja11-via-the-fiio-control-app-in-android-mobile-phone-)
— and there's no first-class desktop tool either. `ktctl` reverse-engineers that Android app's
USB control protocol so you get the same 5-band PEQ, preset, and gain control from a Mac or Linux
terminal. It only touches **runtime** state — the same thing the app's EQ screen changes — never
firmware.

---

## Install

Requires a [Rust toolchain](https://rustup.rs) (1.75+). USB support is on by default.

```bash
git clone https://github.com/ParkWardRR/fiio-ja11-jcally-jm12-moondropkt02-kt02h20-dac-amp-control.git
cd fiio-ja11-jcally-jm12-moondropkt02-kt02h20-dac-amp-control/ktctl
cargo build --release          # → target/release/ktctl
```

Try it immediately with no hardware — `--fake` swaps in an in-memory simulator:

```bash
./target/release/ktctl --fake peq get     # read the EQ
./target/release/ktctl --fake             # launch the TUI
```

With a JA11 plugged in, drop `--fake`. **`rusb` claims this device's HID interface natively on
macOS** — confirmed on real hardware (2026-09-06), full read/write round trip, no OrbStack, no
companion app needed. If you hit `claim interface 3 failed: Access denied`, grant your
terminal app **Input Monitoring** access (macOS System Settings → Privacy & Security → Input
Monitoring) — that's the actual gate on raw HID device access, not a hard platform limitation.
(An earlier pass of this doc claimed macOS couldn't do this directly at all, by analogy with
`ktflash`'s own `IOHIDFamily` wall on a *different* interface — wrong for this one; corrected
once the real cause, a missing TCC grant, was identified.) **On Linux**, USB access may need a
udev rule or `sudo` the first time (packaged rules are planned — see
[ROADMAP](ROADMAP.md#phase-5--packaging--release-)).

---

## Commands

```
ktctl [--fake] [--json] [-v] [COMMAND]

  (no command)     Launch the interactive TUI dashboard
  probe            Identify the device (firmware + active preset)
  peq get          Read all 5 bands + gain + preset (table or --json)
  peq set <n>      Write band n: --freq <Hz> --gain <dB> --q <Q> --type <t>
  peq save         Commit edits to the device's persistent storage
  preset <p>       vocal | classic | bass | user1 | off   (or 0-4)
  gain <dB>        Set master / makeup gain
  state            Volume, sample rate, firmware, mic, UAC mode
  uac <1|2>        Switch USB Audio Class mode
  list             List connected JA11 / KT02H20-family USB devices
```

Examples:

```bash
ktctl peq set 2 --gain 4 --q 1.4 --type peak    # boost 600 Hz +4 dB
ktctl peq set 4 --type high-shelf --gain -2      # tame the top end
ktctl preset bass
ktctl gain -3.0
ktctl peq save                                    # persist edits to flash
ktctl --json peq get | jq '.bands'               # machine-readable
ktctl -v probe                                    # dump raw USB frames
```

- **Filter types:** `peak` (0), `low-shelf` (1), `high-shelf` (2).
- **Presets:** `vocal` (0), `classic` (1), `bass` (2), `user1` (3, custom), `off` (4).
- **Client-side guards:** freq 20–20000 Hz, gain ±12 dB, Q 0.1–20 (conservative bounds from FiiO's UI).

### TUI keys

`Tab` / `1` / `2` switch between the two views; `q` / `Esc` quits from either.

**Status view** — read-only device info plus two settings that apply immediately (no "unsaved"
step, matching the app's own Status screen):

| Key | Action |
|---|---|
| `↑` `↓` / `k` `j` | Volume ±1 (applied immediately) |
| `u` | Toggle UAC 1.0 / 2.0 (applied immediately) |
| `r` | Refresh from the device |

**EQ view** — edits stage locally until `w`:

| Key | Action | Key | Action |
|---|---|---|---|
| `←` `→` / `h` `l` | Select band | `p` | Cycle preset |
| `↑` `↓` / `k` `j` | Gain ±0.5 dB | `w` | Write to device |
| `[` `]` | Frequency | `s` | Save (commit to persistent storage) |
| `,` `.` | Q | `r` | Reload from device (discards unsaved edits) |
| `t` | Cycle filter type | | |

The TUI uses the same device layer as the CLI, so `--fake` works here too.

---

## Status

**Core protocol confirmed on real hardware** (two sessions, 2026-09-06) — not just static
reverse-engineering anymore. Against a physical JA11 — both via a Linux guest passthrough and,
later, directly on the macOS host (`rusb` claims the HID interface fine natively; see
[Install](#install)) — found and fixed four real bugs the static-RE-only implementation had,
then confirmed a full read/write/save round trip. See
[`docs/HARDWARE-VALIDATION.md`](docs/HARDWARE-VALIDATION.md) for the complete log.

| Question | Status |
|---|---|
| Frame format, transport, interface discovery | ✅ confirmed byte-exact on real hardware |
| CRC-8 scope (`seq_hi..=payload`, excludes `magic`/`dir`) | ✅ confirmed — brute-forced against real device-computed CRCs, including a probe that drove `seq` past 255 to settle the last ambiguity |
| Per-band PEQ byte order (`index, gain, freq, Q, type`) | ✅ confirmed — band 0 read back exactly as written |
| Save/commit opcode (`cmd 0x19`, payload `[0x03]`) | ✅ confirmed — wrote a distinctive band, saved, power-cycled the device for real, read it back unchanged |
| Firmware version format (BCD, not decimal) | ✅ confirmed — now prints `1.4`, an exact match with the official app |
| Writes don't get an ACK on this channel | ✅ confirmed — `ktctl` no longer waits for one |
| Master-gain encoding (`×2560` little-endian) | 🟡 round-trip-confirmed, not yet audio-confirmed — the only open item |

Nothing here touches flash except the explicit `peq save` step, so it remains low-stakes to test.

---

## Compatibility

| Device | Silicon | Status |
|---|---|---|
| **FiiO JA11** (JadeAudio) | KTMicro KT02H20 | Primary target — VID/PID `2972:0102`, hardware-confirmed |
| **JCALLY JM12** | KTMicro KT02H20 | Shares silicon; expected, unverified |
| **Moondrop** KT02H20 dongles | KTMicro KT02H20 | Same family; expected, unverified |

The USB interface is descriptor-discovered (first HID-class interface with 2 endpoints), not hardcoded, so KT02H20 siblings have a good chance of working.

---

## How it works

`ktctl` speaks the JA11's runtime control protocol: a raw USB **bulk transfer** on the device's **HID-class interface** (not the CDC serial port used for flashing), with a compact framed protocol:

```text
 0x02  <AA|BB> <0A|0B>  <seq_hi seq_lo>  <cmd>  <len>  <payload…>  <crc8>  0xEE
 lead   magic    dir       16-bit seq      op    len     n bytes    MAXIM   term
```

`AA 0A` = write, `BB 0B` = read; CRC-8/MAXIM checksum over `seq_hi..=payload`; `0xEE` terminator.
EQ opcodes `0x15`/`0x16`/`0x17` (band, preset, gain); status opcodes `0x02`/`0x09`/`0x0B`/`0x12`/`0x20`
(volume, sample rate, firmware, mic, UAC); `0x19` (save). Full spec, including how each byte was
pinned down, in **[`docs/HARDWARE-VALIDATION.md`](docs/HARDWARE-VALIDATION.md)** and
**[`docs/PROTOCOL.md`](docs/PROTOCOL.md)**.

---

## How to help

- **Audio-confirm the master-gain encoding** — the last open item. Set a gain value, listen (or
  measure), and confirm `×2560`/little-endian produces the expected dB change.
- **Try `ktctl` against a JCALLY JM12 or Moondrop KT02H20 clone** — the protocol should carry
  over since it's the same silicon, but nobody's confirmed it on a clone yet.
- [Open an issue](https://github.com/ParkWardRR/fiio-ja11-jcally-jm12-moondropkt02-kt02h20-dac-amp-control/issues)
  for anything else — even "this opcode didn't do what was expected" is useful signal.

> Regenerate the demo GIFs with [`vhs`](https://github.com/charmbracelet/vhs): `vhs docs/media/tui.tape` and `vhs docs/media/cli.tape` from the repo root.

---

## Relationship to `ktflash`

[**`ktflash`**](https://github.com/ParkWardRR/fiio-ja11-jcally-jm12-moondropkt02-kt02h20-dac-amp-toolkit) is the sibling project for **firmware** (reading and cross-flashing the JA11).

|  | `ktflash` | `ktctl` (this repo) |
|---|---|---|
| Answers | "What firmware is on this dongle, and how do I change it?" | "What is this dongle doing now, and how do I tune it?" |
| Protocol | CDC bootloader (`8888:cdc0`) | Runtime HID vendor channel |
| Touches flash? | **Yes** — firmware writer | **No** — runtime only (except the explicit `peq save`, which persists EQ settings, not firmware) |
| Status | ✅ proven on hardware, released | ✅ core protocol hardware-confirmed |

Separate repos on purpose: different risk profile, different protocol, different USB interface — no reason to couple their release cadences.

---

## License

[Blue Oak Model License 1.0.0](LICENSE.md).
