//! Opcodes recovered for the JA11 (internal product id `109`).
//!
//! | cmd    | meaning                          |
//! |--------|----------------------------------|
//! | `0x15` | per-band PEQ get/set             |
//! | `0x16` | PEQ enable / active preset slot  |
//! | `0x17` | global / makeup gain             |
//!
//! All still provisional pending hardware validation (roadmap Phase 0).

/// Per-band PEQ get (read frame) / set (write frame).
pub const CMD_PEQ_BAND: u8 = 0x15;
/// PEQ enable flag / active preset slot selection.
pub const CMD_PEQ_PRESET: u8 = 0x16;
/// Global / makeup gain.
pub const CMD_GAIN: u8 = 0x17;

/// Human-readable name for an opcode, for logging / `--verbose` output.
pub fn opcode_name(cmd: u8) -> &'static str {
    match cmd {
        CMD_PEQ_BAND => "PEQ_BAND",
        CMD_PEQ_PRESET => "PEQ_PRESET",
        CMD_GAIN => "GAIN",
        _ => "UNKNOWN",
    }
}
