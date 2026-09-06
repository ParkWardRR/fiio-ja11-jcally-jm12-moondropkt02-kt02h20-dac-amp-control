//! Portable PEQ preset import/export (roadmap "ideas" — preset file format).
//!
//! Two interchange formats are supported:
//!
//! * **ktctl JSON** — a direct serialization of [`PeqState`], round-trips
//!   losslessly (`.json`).
//! * **AutoEQ / "ParametricEQ" text** — the widely-used format produced by
//!   AutoEQ and consumed by many EQ tools, e.g.
//!
//!   ```text
//!   Preamp: -6.0 dB
//!   Filter 1: ON PK Fc 105 Hz Gain 5.5 dB Q 0.70
//!   Filter 2: ON LSC Fc 105 Hz Gain 5.5 dB Q 0.70
//!   ```
//!
//!   This lets tunings shared for other DACs be loaded onto the JA11 (and vice
//!   versa). Only the first [`BAND_COUNT`] enabled bands are used on import;
//!   export always writes all five plus a `Preamp` line for the master gain.

use std::fmt::Write as _;

use crate::proto::peq::{FilterType, PeqBand, PeqState, BAND_COUNT};

/// Errors from preset (de)serialization.
#[derive(Debug, thiserror::Error)]
pub enum PresetError {
    /// A JSON (de)serialization error.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    /// A malformed AutoEQ text line.
    #[error("could not parse AutoEQ line: {0:?}")]
    BadAutoEqLine(String),
    /// The file/text produced no usable bands.
    #[error("no PEQ bands found in preset")]
    Empty,
}

/// Serialize a [`PeqState`] to pretty ktctl JSON.
pub fn to_json(state: &PeqState) -> Result<String, PresetError> {
    Ok(serde_json::to_string_pretty(state)?)
}

/// Parse a [`PeqState`] from ktctl JSON.
pub fn from_json(text: &str) -> Result<PeqState, PresetError> {
    Ok(serde_json::from_str(text)?)
}

/// Render a [`PeqState`] to AutoEQ / ParametricEQ text.
pub fn to_autoeq(state: &PeqState) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Preamp: {:.1} dB", state.gain_db);
    for (i, b) in state.bands.iter().enumerate() {
        let kind = match b.filter {
            FilterType::Peak | FilterType::Unknown(_) => "PK",
            FilterType::LowShelf => "LSC",
            FilterType::HighShelf => "HSC",
        };
        let _ = writeln!(
            out,
            "Filter {}: ON {} Fc {} Hz Gain {:.1} dB Q {:.2}",
            i + 1,
            kind,
            b.freq_hz,
            b.gain_db,
            b.q
        );
    }
    out
}

/// Parse a [`PeqState`] from AutoEQ / ParametricEQ text.
///
/// Recognises `Preamp:` and `Filter N: ON <TYPE> Fc <f> Hz Gain <g> dB Q <q>`.
/// `OFF` filters are skipped. Types map: `PK`→peak, `LSC`/`LS`→low-shelf,
/// `HSC`/`HS`→high-shelf; unknown types default to peak. At most [`BAND_COUNT`]
/// bands are kept; if fewer are present, remaining bands are filled flat.
pub fn from_autoeq(text: &str) -> Result<PeqState, PresetError> {
    let mut gain_db = 0.0f32;
    let mut bands: Vec<PeqBand> = Vec::new();

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = strip_prefix_ci(line, "preamp:") {
            gain_db =
                parse_leading_f32(rest).ok_or_else(|| PresetError::BadAutoEqLine(raw.into()))?;
            continue;
        }
        if let Some(rest) = strip_prefix_ci(line, "filter") {
            // rest looks like "1: ON PK Fc 105 Hz Gain 5.5 dB Q 0.70"
            let after_colon = match rest.split_once(':') {
                Some((_, a)) => a.trim(),
                None => return Err(PresetError::BadAutoEqLine(raw.into())),
            };
            let toks: Vec<&str> = after_colon.split_whitespace().collect();
            if toks
                .first()
                .map(|s| s.eq_ignore_ascii_case("off"))
                .unwrap_or(false)
            {
                continue; // disabled filter
            }
            let band =
                parse_filter_tokens(&toks).ok_or_else(|| PresetError::BadAutoEqLine(raw.into()))?;
            if bands.len() < BAND_COUNT {
                bands.push(PeqBand {
                    index: bands.len() as u8,
                    ..band
                });
            }
        }
    }

    if bands.is_empty() {
        return Err(PresetError::Empty);
    }
    // Pad to exactly BAND_COUNT so the device always gets a full set.
    while bands.len() < BAND_COUNT {
        bands.push(PeqBand::flat(bands.len() as u8));
    }

    Ok(PeqState {
        bands,
        gain_db,
        preset: PeqState::flat().preset, // default to USER1; import targets custom
    })
}

/// Parse tokens like `["ON","PK","Fc","105","Hz","Gain","5.5","dB","Q","0.70"]`.
fn parse_filter_tokens(toks: &[&str]) -> Option<PeqBand> {
    // Skip a leading ON if present.
    let start = if toks
        .first()
        .map(|s| s.eq_ignore_ascii_case("on"))
        .unwrap_or(false)
    {
        1
    } else {
        0
    };
    let kind = toks.get(start)?;
    let filter = match kind.to_ascii_uppercase().as_str() {
        "PK" | "PEQ" | "PEAK" => FilterType::Peak,
        "LSC" | "LS" | "LOWSHELF" => FilterType::LowShelf,
        "HSC" | "HS" | "HIGHSHELF" => FilterType::HighShelf,
        _ => FilterType::Peak,
    };
    let fc = find_after(toks, "Fc")?;
    let gain = find_after(toks, "Gain")?;
    let q = find_after(toks, "Q").unwrap_or(1.0);
    Some(PeqBand {
        index: 0,
        freq_hz: fc.round().clamp(0.0, u16::MAX as f32) as u16,
        gain_db: gain,
        q,
        filter,
    })
}

/// Find the numeric token immediately following a case-insensitive keyword.
fn find_after(toks: &[&str], key: &str) -> Option<f32> {
    let pos = toks.iter().position(|t| t.eq_ignore_ascii_case(key))?;
    toks.get(pos + 1)?.parse::<f32>().ok()
}

/// Case-insensitive prefix strip, returning the remainder.
fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

/// Parse the first float found in a string (handles a leading `"-6.0 dB"`).
fn parse_leading_f32(s: &str) -> Option<f32> {
    s.split_whitespace().next()?.parse::<f32>().ok()
}

/// Guess a format from a filename extension and export accordingly.
pub fn export_by_extension(state: &PeqState, path: &str) -> Result<String, PresetError> {
    if path.to_ascii_lowercase().ends_with(".json") {
        to_json(state)
    } else {
        Ok(to_autoeq(state))
    }
}

/// Guess a format from content/extension and import accordingly.
pub fn import_auto(text: &str, path: &str) -> Result<PeqState, PresetError> {
    let looks_json =
        path.to_ascii_lowercase().ends_with(".json") || text.trim_start().starts_with('{');
    if looks_json {
        from_json(text)
    } else {
        from_autoeq(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip() {
        let mut state = PeqState::flat();
        state.bands[0].gain_db = 5.5;
        state.gain_db = -2.0;
        let text = to_json(&state).unwrap();
        let back = from_json(&text).unwrap();
        assert_eq!(back, state);
    }

    #[test]
    fn autoeq_parse_basic() {
        let text = "\
Preamp: -6.0 dB
Filter 1: ON PK Fc 105 Hz Gain 5.5 dB Q 0.70
Filter 2: ON LSC Fc 30 Hz Gain 3.0 dB Q 0.71
Filter 3: OFF PK Fc 1000 Hz Gain 0.0 dB Q 1.0
";
        let state = from_autoeq(text).unwrap();
        assert_eq!(state.gain_db, -6.0);
        assert_eq!(state.bands.len(), BAND_COUNT);
        assert_eq!(state.bands[0].freq_hz, 105);
        assert!((state.bands[0].gain_db - 5.5).abs() < 1e-6);
        assert_eq!(state.bands[1].filter, FilterType::LowShelf);
        // OFF filter skipped → band 2 is padded flat.
        assert_eq!(state.bands[2].gain_db, 0.0);
    }

    #[test]
    fn autoeq_round_trip_shape() {
        let mut state = PeqState::flat();
        state.gain_db = -4.0;
        state.bands[0] = PeqBand {
            index: 0,
            freq_hz: 200,
            gain_db: 3.0,
            q: 1.0,
            filter: FilterType::HighShelf,
        };
        let text = to_autoeq(&state);
        let back = from_autoeq(&text).unwrap();
        assert_eq!(back.gain_db, -4.0);
        assert_eq!(back.bands[0].freq_hz, 200);
        assert_eq!(back.bands[0].filter, FilterType::HighShelf);
    }

    #[test]
    fn import_auto_detects_json() {
        let json = to_json(&PeqState::flat()).unwrap();
        assert!(import_auto(&json, "whatever.txt").is_ok());
    }

    #[test]
    fn empty_autoeq_errors() {
        assert!(matches!(
            from_autoeq("# just a comment\n"),
            Err(PresetError::Empty)
        ));
    }
}
