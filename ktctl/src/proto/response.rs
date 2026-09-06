//! Frequency-response computation for a PEQ configuration.
//!
//! This turns the abstract [`PeqBand`]s into an actual magnitude-vs-frequency
//! curve using the standard RBJ "Audio EQ Cookbook" biquad coefficients,
//! evaluated on the unit circle. It's used to draw a *real* EQ curve (not just
//! bar heights) in the CLI and TUI, and to let callers reason about what a
//! tuning actually does.
//!
//! The curve is display-oriented: a fixed reference sample rate is used (the
//! shape across the audible band is what matters for a UI), and each band's dB
//! contribution is summed, plus the master/pre-amp gain as a flat offset.

use super::peq::{FilterType, PeqBand, PeqState};

/// Reference sample rate used when evaluating the biquad response for display.
pub const DISPLAY_SAMPLE_RATE: f64 = 48_000.0;

/// Lowest frequency on the standard display grid, in Hz.
pub const GRID_MIN_HZ: f64 = 20.0;
/// Highest frequency on the standard display grid, in Hz.
pub const GRID_MAX_HZ: f64 = 20_000.0;

/// One biquad's transfer-function coefficients (`a0` normalised out lazily).
#[derive(Debug, Clone, Copy)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a0: f64,
    a1: f64,
    a2: f64,
}

impl Biquad {
    /// RBJ cookbook coefficients for a single band at reference sample rate `fs`.
    fn from_band(band: &PeqBand, fs: f64) -> Self {
        let f0 = (band.freq_hz as f64).clamp(1.0, fs / 2.0 - 1.0);
        let gain_db = band.gain_db as f64;
        let q = (band.q as f64).max(1e-4);
        let a = 10f64.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f64::consts::PI * f0 / fs;
        let cw = w0.cos();
        let sw = w0.sin();
        let alpha = sw / (2.0 * q);

        match band.filter {
            FilterType::Peak | FilterType::Unknown(_) => Biquad {
                b0: 1.0 + alpha * a,
                b1: -2.0 * cw,
                b2: 1.0 - alpha * a,
                a0: 1.0 + alpha / a,
                a1: -2.0 * cw,
                a2: 1.0 - alpha / a,
            },
            FilterType::LowShelf => {
                let sa = 2.0 * a.sqrt() * alpha;
                Biquad {
                    b0: a * ((a + 1.0) - (a - 1.0) * cw + sa),
                    b1: 2.0 * a * ((a - 1.0) - (a + 1.0) * cw),
                    b2: a * ((a + 1.0) - (a - 1.0) * cw - sa),
                    a0: (a + 1.0) + (a - 1.0) * cw + sa,
                    a1: -2.0 * ((a - 1.0) + (a + 1.0) * cw),
                    a2: (a + 1.0) + (a - 1.0) * cw - sa,
                }
            }
            FilterType::HighShelf => {
                let sa = 2.0 * a.sqrt() * alpha;
                Biquad {
                    b0: a * ((a + 1.0) + (a - 1.0) * cw + sa),
                    b1: -2.0 * a * ((a - 1.0) + (a + 1.0) * cw),
                    b2: a * ((a + 1.0) + (a - 1.0) * cw - sa),
                    a0: (a + 1.0) - (a - 1.0) * cw + sa,
                    a1: 2.0 * ((a - 1.0) - (a + 1.0) * cw),
                    a2: (a + 1.0) - (a - 1.0) * cw - sa,
                }
            }
        }
    }

    /// Magnitude (linear) of this biquad at angular frequency `w` (radians/sample).
    fn magnitude(&self, w: f64) -> f64 {
        // Evaluate H(e^{jw}) = N/D with z^-1 = e^{-jw}.
        let (c1, s1) = (w.cos(), w.sin());
        let (c2, s2) = ((2.0 * w).cos(), (2.0 * w).sin());
        // numerator = b0 + b1 e^{-jw} + b2 e^{-2jw}
        let n_re = self.b0 + self.b1 * c1 + self.b2 * c2;
        let n_im = -(self.b1 * s1 + self.b2 * s2);
        let d_re = self.a0 + self.a1 * c1 + self.a2 * c2;
        let d_im = -(self.a1 * s1 + self.a2 * s2);
        let num = (n_re * n_re + n_im * n_im).sqrt();
        let den = (d_re * d_re + d_im * d_im).sqrt();
        if den == 0.0 {
            1.0
        } else {
            num / den
        }
    }

    /// Response of this biquad in dB at frequency `f_hz` (reference `fs`).
    fn response_db(&self, f_hz: f64, fs: f64) -> f64 {
        let w = 2.0 * std::f64::consts::PI * f_hz / fs;
        20.0 * self.magnitude(w).log10()
    }
}

/// Combined dB response of all `bands` at `f_hz` (reference sample rate `fs`),
/// **excluding** any master/pre-amp gain.
pub fn bands_response_db(bands: &[PeqBand], f_hz: f64, fs: f64) -> f64 {
    bands
        .iter()
        .map(|b| Biquad::from_band(b, fs).response_db(f_hz, fs))
        .sum()
}

/// Combined dB response of a full [`PeqState`] at `f_hz`, **including** the
/// master gain as a flat offset, at the display sample rate.
pub fn state_response_db(state: &PeqState, f_hz: f64) -> f64 {
    bands_response_db(&state.bands, f_hz, DISPLAY_SAMPLE_RATE) + state.gain_db as f64
}

/// A log-spaced frequency grid of `points` entries spanning [`GRID_MIN_HZ`,
/// [`GRID_MAX_HZ`]].
pub fn log_frequency_grid(points: usize) -> Vec<f64> {
    if points == 0 {
        return Vec::new();
    }
    if points == 1 {
        return vec![GRID_MIN_HZ];
    }
    let lmin = GRID_MIN_HZ.log10();
    let lmax = GRID_MAX_HZ.log10();
    (0..points)
        .map(|i| {
            let t = i as f64 / (points - 1) as f64;
            10f64.powf(lmin + t * (lmax - lmin))
        })
        .collect()
}

/// Sample a full [`PeqState`]'s response (including master gain) across a
/// log-spaced grid; returns `(freq_hz, gain_db)` pairs.
pub fn sample_curve(state: &PeqState, points: usize) -> Vec<(f64, f64)> {
    log_frequency_grid(points)
        .into_iter()
        .map(|f| (f, state_response_db(state, f)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::peq::{FilterType, PeqBand, PeqState};

    fn peak(freq: u16, gain: f32, q: f32) -> PeqBand {
        PeqBand {
            index: 0,
            freq_hz: freq,
            gain_db: gain,
            q,
            filter: FilterType::Peak,
        }
    }

    #[test]
    fn peaking_hits_target_gain_at_center() {
        let b = peak(1000, 6.0, 1.0);
        let db =
            Biquad::from_band(&b, DISPLAY_SAMPLE_RATE).response_db(1000.0, DISPLAY_SAMPLE_RATE);
        assert!((db - 6.0).abs() < 0.2, "got {db} dB at center");
    }

    #[test]
    fn peaking_is_flat_far_from_center() {
        let b = peak(1000, 6.0, 2.0);
        let low = bands_response_db(&[b], 20.0, DISPLAY_SAMPLE_RATE);
        assert!(
            low.abs() < 0.5,
            "expected ~0 dB far below center, got {low}"
        );
    }

    #[test]
    fn low_shelf_asymptotes() {
        let b = PeqBand {
            index: 0,
            freq_hz: 200,
            gain_db: 6.0,
            q: 0.7,
            filter: FilterType::LowShelf,
        };
        let very_low = bands_response_db(&[b], 20.0, DISPLAY_SAMPLE_RATE);
        let very_high = bands_response_db(&[b], 18_000.0, DISPLAY_SAMPLE_RATE);
        assert!(very_low > 4.0, "low-shelf boost at DC-ish: {very_low}");
        assert!(very_high.abs() < 1.0, "high end ~flat: {very_high}");
    }

    #[test]
    fn master_gain_offsets_whole_curve() {
        let mut state = PeqState::flat();
        state.gain_db = -3.0;
        // Flat bands → response is just the master gain everywhere.
        for (_, db) in sample_curve(&state, 16) {
            assert!((db - (-3.0)).abs() < 0.1, "got {db}");
        }
    }

    #[test]
    fn grid_is_monotonic_and_bounded() {
        let g = log_frequency_grid(64);
        assert_eq!(g.len(), 64);
        assert!((g[0] - GRID_MIN_HZ).abs() < 1e-6);
        assert!((g[63] - GRID_MAX_HZ).abs() < 1e-6);
        assert!(g.windows(2).all(|w| w[1] > w[0]));
    }
}
