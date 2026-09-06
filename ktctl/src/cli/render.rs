//! Human-readable rendering of PEQ state for the CLI.

use crate::proto::peq::{PeqBand, PeqState};
use crate::proto::state::DeviceState;

/// Print the device Status-screen snapshot.
pub fn print_device_state(s: &DeviceState) {
    println!("volume:      {}", s.volume);
    println!(
        "sample rate: {} (index {})",
        s.sample_rate, s.sample_rate_index
    );
    println!("firmware:    {}", s.firmware);
    println!(
        "mic:         {}",
        if s.mic_present { "present" } else { "absent" }
    );
    println!("UAC mode:    {}", s.uac);
}

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

/// Print the true magnitude-vs-frequency response as an ASCII plot, computed
/// from the RBJ biquad model (including master gain) over a log-frequency grid.
fn print_curve(state: &PeqState) {
    use crate::proto::response::{sample_curve, GRID_MAX_HZ, GRID_MIN_HZ};

    const WIDTH: usize = 60; // columns (log-freq)
    const HEIGHT: usize = 13; // rows (dB), odd so 0 dB sits on a center row
    const SCALE: f64 = 12.0; // dB at top/bottom edge

    let curve = sample_curve(state, WIDTH);
    let mid = (HEIGHT - 1) / 2;

    // Map each column's dB to a row (0 = top).
    let rows: Vec<usize> = curve
        .iter()
        .map(|&(_, db)| {
            let norm = (db / SCALE).clamp(-1.0, 1.0); // -1..1
            let r = mid as f64 - norm * mid as f64;
            r.round().clamp(0.0, (HEIGHT - 1) as f64) as usize
        })
        .collect();

    println!(
        "  response (±{SCALE:.0} dB, {GRID_MIN_HZ:.0} Hz–{:.0} kHz):",
        GRID_MAX_HZ / 1000.0
    );
    for row in 0..HEIGHT {
        let mut line = String::from("   ");
        for &cell in &rows {
            line.push(if cell == row {
                '●'
            } else if row == mid {
                '·'
            } else {
                ' '
            });
        }
        // dB scale label on the axis rows.
        if row == 0 {
            line.push_str(&format!("  +{SCALE:.0}"));
        } else if row == mid {
            line.push_str("   0");
        } else if row == HEIGHT - 1 {
            line.push_str(&format!("  -{SCALE:.0}"));
        }
        println!("{line}");
    }

    // Log-frequency axis ticks at decade-ish marks.
    let mut axis = vec![b' '; WIDTH];
    for (label, hz) in [
        ("20", 20.0),
        ("100", 100.0),
        ("1k", 1000.0),
        ("10k", 10_000.0),
    ] {
        let col = freq_to_col(hz, WIDTH);
        for (k, ch) in label.bytes().enumerate() {
            if col + k < WIDTH {
                axis[col + k] = ch;
            }
        }
    }
    println!("   {}", String::from_utf8_lossy(&axis));
}

/// Column index for a frequency on the log grid used by [`print_curve`].
fn freq_to_col(hz: f64, width: usize) -> usize {
    use crate::proto::response::{GRID_MAX_HZ, GRID_MIN_HZ};
    let t = (hz.log10() - GRID_MIN_HZ.log10()) / (GRID_MAX_HZ.log10() - GRID_MIN_HZ.log10());
    (t * (width - 1) as f64)
        .round()
        .clamp(0.0, (width - 1) as f64) as usize
}
