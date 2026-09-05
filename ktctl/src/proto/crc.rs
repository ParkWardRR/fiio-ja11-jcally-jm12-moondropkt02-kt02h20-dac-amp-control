//! CRC-8/MAXIM (a.k.a. Dallas/Maxim 1-Wire CRC).
//!
//! Catalogue parameters (per the RocksoftⓇ / reveng model `CRC-8/MAXIM`):
//!
//! | param   | value  |
//! |---------|--------|
//! | width   | 8      |
//! | poly    | `0x31` |
//! | init    | `0x00` |
//! | refin   | true   |
//! | refout  | true   |
//! | xorout  | `0x00` |
//! | check   | `0xA1` | (CRC of the ASCII bytes `"123456789"`)
//!
//! Because the model is reflected (`refin`/`refout` both true), the bit-by-bit
//! implementation folds in the *reflected* polynomial `0x8C` (the bit-reversal
//! of `0x31`), processing each input byte LSB-first. This matches the classic
//! 1-Wire reference routine and the table that was extracted from the FiiO
//! Control Android app (`qg.a.f17478d` in the decompiled source — see
//! `docs/PROTOCOL.md`).
//!
//! Two implementations are provided and are kept in lock-step by the test
//! suite: a small bitwise one (`crc8_maxim`) and a table-driven one
//! (`crc8_maxim_table`) backed by [`CRC8_MAXIM_TABLE`].

/// Reflected form of polynomial `0x31`. Used by the bitwise routine.
const REFLECTED_POLY: u8 = 0x8C;

/// Compute CRC-8/MAXIM over `data` using the bitwise (table-free) algorithm.
///
/// This is the reference implementation; [`crc8_maxim_table`] must agree with
/// it for every input (enforced by tests).
pub fn crc8_maxim(data: &[u8]) -> u8 {
    let mut crc: u8 = 0x00;
    for &byte in data {
        crc ^= byte;
        for _ in 0..8 {
            if crc & 0x01 != 0 {
                crc = (crc >> 1) ^ REFLECTED_POLY;
            } else {
                crc >>= 1;
            }
        }
    }
    crc
}

/// Compute CRC-8/MAXIM over `data` using the precomputed [`CRC8_MAXIM_TABLE`].
pub fn crc8_maxim_table(data: &[u8]) -> u8 {
    let mut crc: u8 = 0x00;
    for &byte in data {
        crc = CRC8_MAXIM_TABLE[(crc ^ byte) as usize];
    }
    crc
}

/// Build the 256-entry CRC-8/MAXIM lookup table at compile time.
///
/// Kept `const` so it lives in the binary's read-only data and can be compared
/// byte-for-byte against the table lifted from the Android app.
const fn build_table() -> [u8; 256] {
    let mut table = [0u8; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut crc = i as u8;
        let mut bit = 0;
        while bit < 8 {
            if crc & 0x01 != 0 {
                crc = (crc >> 1) ^ REFLECTED_POLY;
            } else {
                crc >>= 1;
            }
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

/// Precomputed CRC-8/MAXIM lookup table (256 entries).
pub const CRC8_MAXIM_TABLE: [u8; 256] = build_table();

#[cfg(test)]
mod tests {
    use super::*;

    /// The catalogue "check" value: CRC of the ASCII string "123456789".
    #[test]
    fn check_value_matches_catalogue() {
        assert_eq!(crc8_maxim(b"123456789"), 0xA1);
        assert_eq!(crc8_maxim_table(b"123456789"), 0xA1);
    }

    #[test]
    fn empty_input_is_zero() {
        assert_eq!(crc8_maxim(&[]), 0x00);
        assert_eq!(crc8_maxim_table(&[]), 0x00);
    }

    /// The bitwise and table implementations must agree on every single byte
    /// and on a swath of multi-byte inputs.
    #[test]
    fn bitwise_and_table_agree() {
        for b in 0u16..=255 {
            let buf = [b as u8];
            assert_eq!(crc8_maxim(&buf), crc8_maxim_table(&buf), "byte {b:#04x}");
        }
        let mut acc = Vec::new();
        for b in 0u16..=255 {
            acc.push(b as u8);
            assert_eq!(
                crc8_maxim(&acc),
                crc8_maxim_table(&acc),
                "prefix len {}",
                acc.len()
            );
        }
    }

    /// Classic 1-Wire single-byte vector: CRC of 0x00 is 0x00; the routine is
    /// its own well-known fixed point on a run of zeros.
    #[test]
    fn known_small_vectors() {
        assert_eq!(crc8_maxim(&[0x00]), 0x00);
        // Two independently hand-verifiable vectors against the reflected poly.
        assert_eq!(crc8_maxim(&[0x01]), 0x5E);
        assert_eq!(crc8_maxim(&[0xFF]), 0x35);
    }
}
