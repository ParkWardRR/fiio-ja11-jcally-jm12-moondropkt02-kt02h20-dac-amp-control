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
//! ## Save / commit (unresolved — see [`SAVE_CANDIDATES`])
//!
//! All still provisional pending hardware validation (roadmap Phase 0).

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

/// Candidate "commit PEQ edits to persistent storage" commands. The real one is
/// unresolved: two external drivers each claim a different opcode/payload and
/// disagree. The first entry (`fiiocontrol-oss`, marked JA11-working) is tried
/// first; the second (`glacier-eq`, JA11 `Testing`) is the fallback.
pub const SAVE_CANDIDATES: &[(u8, &[u8])] = &[
    (0x19, &[0x03]), // fiiocontrol-oss, claimed working on a real JA11
    (0x18, &[0x01]), // glacier-eq, JA11 status "Testing"/unconfirmed
];

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
