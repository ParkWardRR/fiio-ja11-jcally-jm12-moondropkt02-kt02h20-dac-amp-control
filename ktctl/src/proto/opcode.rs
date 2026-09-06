//! Opcodes recovered for the JA11 (internal product id `109`).
//!
//! Two opcode groups share the exact same wire frame (see [`super::frame`]):
//!
//! ## EQ channel (the EQ screen)
//! | cmd    | meaning                          |
//! |--------|----------------------------------|
//! | `0x15` | per-band PEQ get/set             |
//! | `0x16` | PEQ enable / active preset       |
//! | `0x17` | master / global gain             |
//!
//! ## Device-state channel (the Status screen, RE write-up §4b)
//! | cmd    | meaning                          |
//! |--------|----------------------------------|
//! | `0x02` | device volume                    |
//! | `0x09` | sample rate / format (table idx) |
//! | `0x0B` | firmware version (`"maj.min"`)   |
//! | `0x12` | in-line mic detect               |
//! | `0x20` | UAC 1.0 / 2.0 mode (read+write)  |
//!
//! ## Save / commit (unresolved — see [`SaveCommand`])
//!
//! All still provisional pending hardware validation (roadmap Phase 0).

use serde::{Deserialize, Serialize};

// ── EQ channel ───────────────────────────────────────────────────────────────

/// Per-band PEQ get (read frame) / set (write frame).
pub const CMD_PEQ_BAND: u8 = 0x15;
/// PEQ enable flag / active preset selection.
pub const CMD_PEQ_PRESET: u8 = 0x16;
/// Master / global gain.
pub const CMD_GAIN: u8 = 0x17;

// ── Device-state channel ─────────────────────────────────────────────────────

/// Device volume.
pub const CMD_VOLUME: u8 = 0x02;
/// Sample rate / format, indexed into a 15-entry PCM/DSD table.
pub const CMD_SAMPLE_RATE: u8 = 0x09;
/// Firmware version, reported as `"{major}.{minor}"`.
pub const CMD_FIRMWARE: u8 = 0x0B;
/// In-line microphone detect.
pub const CMD_MIC_DETECT: u8 = 0x12;
/// UAC 1.0 / 2.0 mode (read + write).
pub const CMD_UAC_MODE: u8 = 0x20;

// ── Save / commit ────────────────────────────────────────────────────────────

/// "Commit PEQ edits to persistent storage".
///
/// **Confirmed on real hardware, 2026-09-06**: `Cmd19Payload3` (`cmd 0x19`,
/// payload `[0x03]`) genuinely persists PEQ writes. Test: wrote band 0 to a
/// distinctive, un-mistakable-for-default value (`3333 Hz, +4.0 dB, Q 0.55,
/// low-shelf`), issued `save`, confirmed a real power cycle (the device's
/// enumeration changed), then read band 0 back — still `3333 Hz`/`+4.0 dB`/
/// `0.55`/`low-shelf`. `Cmd18Payload1` (`glacier-eq`'s override) was not
/// separately tested since the working candidate was found first; no reason
/// to suspect it's also needed.
///
/// This used to be an auto-try-both loop, but that approach broke once real
/// hardware showed this device never ACKs writes on this channel at all (see
/// `docs/HARDWARE-VALIDATION.md` bug #3) — every write "succeeds" from the
/// host's point of view regardless of what the device did with it, so a
/// try-until-no-error loop couldn't distinguish the candidates. Kept as an
/// explicit, CLI-selectable choice (mirroring [`super::peq::GainEncoding`]'s
/// pattern) rather than reverting to an auto-pick now that one is confirmed,
/// in case a future firmware revision needs the other one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SaveCommand {
    /// `cmd 0x19`, payload `[0x03]` — **confirmed persists across a power cycle
    /// on real hardware** (2026-09-06). **Default.**
    #[default]
    Cmd19Payload3,
    /// `cmd 0x18`, payload `[0x01]` (`glacier-eq`'s JA11 override) — not tested
    /// against real hardware; `Cmd19Payload3` was confirmed first.
    Cmd18Payload1,
}

impl SaveCommand {
    /// The `(cmd, payload)` pair to send for this candidate.
    pub fn to_frame_parts(self) -> (u8, Vec<u8>) {
        match self {
            SaveCommand::Cmd19Payload3 => (0x19, vec![0x03]),
            SaveCommand::Cmd18Payload1 => (0x18, vec![0x01]),
        }
    }

    /// Parse a `--save-command` CLI value (`0x19`, `19`, `0x18`, `18`).
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "0x19" | "19" => Some(SaveCommand::Cmd19Payload3),
            "0x18" | "18" => Some(SaveCommand::Cmd18Payload1),
            _ => None,
        }
    }
}

/// Human-readable name for an opcode, for logging / `--verbose` output.
pub fn opcode_name(cmd: u8) -> &'static str {
    match cmd {
        CMD_PEQ_BAND => "PEQ_BAND",
        CMD_PEQ_PRESET => "PEQ_PRESET",
        CMD_GAIN => "GAIN",
        CMD_VOLUME => "VOLUME",
        CMD_SAMPLE_RATE => "SAMPLE_RATE",
        CMD_FIRMWARE => "FIRMWARE",
        CMD_MIC_DETECT => "MIC_DETECT",
        CMD_UAC_MODE => "UAC_MODE",
        0x19 | 0x18 => "SAVE?",
        _ => "UNKNOWN",
    }
}
