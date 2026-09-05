//! Human-readable rendering of PEQ state for the CLI.

use crate::proto::peq::{PeqBand, PeqState};

/// Print a full PEQ snapshot as an aligned table plus a small ASCII curve.
pub fn print_state(state: &PeqState) {
    println!("gain:   {:+.1} dB", state.gain_db);
    println!("preset: {}", state.preset);
    println!();
    println!("  band     freq        gain      Q     type");
    println!("  ----  --------  ----------  ------  ----------");
    for b in &state.bands {
        println!(
            "  {:>4}  {:>6} Hz  {:>+7.1} dB  {:>6.2}  {}",
            b.index, b.freq_hz, b.gain_db, b.q, b.filter
        );
    }
    println!();
    print_curve(state);
}

/// Print a single band as one aligned line.
pub fn print_band(b: &PeqBand) {
    println!(
        "  {:>4}  {:>6} Hz  {:>+7.1} dB  {:>6.2}  {}",
        b.index, b.freq_hz, b.gain_db, b.q, b.filter
    );
}

/// A crude single-row ASCII sparkline of each band's gain, for a quick glance.
fn print_curve(state: &PeqState) {
    const HEIGHT: i32 = 9; // rows; middle row is 0 dB
    const SCALE: f32 = 12.0; // dB represented by half the height
    let mid = HEIGHT / 2;

    // Column per band.
    let cells: Vec<i32> = state
        .bands
        .iter()
        .map(|b| {
            let norm = (b.gain_db / SCALE).clamp(-1.0, 1.0);
            mid - (norm * mid as f32).round() as i32
        })
        .collect();

    println!("  curve (±{SCALE:.0} dB):");
    for row in 0..HEIGHT {
        let mut line = String::from("   ");
        for (i, &cell) in cells.iter().enumerate() {
            if i > 0 {
                line.push_str("   ");
            }
            line.push(if row == cell {
                '●'
            } else if row == mid {
                '─'
            } else {
                ' '
            });
        }
        println!("{line}");
    }
    // Frequency axis labels.
    let mut axis = String::from("   ");
    for (i, b) in state.bands.iter().enumerate() {
        if i > 0 {
            axis.push_str("  ");
        }
        axis.push_str(&format!("{:>2}", short_hz(b.freq_hz)));
    }
    println!("{axis}");
}

/// Compact frequency label, e.g. 1000 → "1k".
fn short_hz(hz: u16) -> String {
    if hz >= 1000 {
        format!("{}k", hz / 1000)
    } else {
        format!("{hz}")
    }
}
