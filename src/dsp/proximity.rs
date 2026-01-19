//! Proximity Effect (Low-end Restoration)
//!
//! # Perceptual Contract
//! - **Target Source**: Thin, distant, or lapel-mic speech.
//! - **Intended Effect**: Restore low-end body (100-300Hz) associated with close-mic broadcasting.
//! - **Failure Modes**:
//!   - Boomy / muddy sound if over-applied.
//!   - Masking of intelligibility by excessive LF energy.
//! - **Will Not Do**:
//!   - Synthesize sub-harmonics (not a sub-synth).
//!   - Fix physically high-passed inputs (e.g. telephone).
//!
//! # Lifecycle
//! - **Active**: Normal operation.
//! - **Bypassed**: Passes audio through.

use crate::dsp::Biquad;

// Constants for proximity effect tuning

// Low shelf filter frequency (Hz).
// Increasing: higher crossover point; decreasing: lower crossover.
const LOW_SHELF_FREQ_HZ: f32 = 180.0;
// High shelf filter frequency (Hz).
// Increasing: higher rolloff point; decreasing: lower rolloff.
const HF_SHELF_FREQ_HZ: f32 = 8000.0;
// Filter Q (Butterworth-ish).
// Increasing: narrower transition; decreasing: wider transition.
const FILTER_Q: f32 = 0.7;
// Smoothing coefficient for proximity parameter changes.
// Increasing: faster tracking; decreasing: smoother transitions.
const PROX_SMOOTH_COEFF: f32 = 0.01;
// Maximum bass boost at proximity=1.0 (dB).
// Increasing: more bass boost; decreasing: less bass boost.
const MAX_BOOST_DB: f32 = 12.0;
// Proximity threshold above which HF rolloff begins.
// Increasing: HF rolloff starts later; decreasing: starts earlier.
const HF_ROLLOFF_THRESHOLD: f32 = 0.7;
// Range over which HF rolloff scales (from threshold to 1.0).
// Must not change: derived from 1.0 - HF_ROLLOFF_THRESHOLD.
const HF_ROLLOFF_RANGE: f32 = 0.3;
// Maximum HF rolloff at proximity=1.0 (dB).
// Increasing (more negative): stronger rolloff; decreasing: gentler rolloff.
const HF_ROLLOFF_MAX_DB: f32 = -6.0;
// Coefficient update threshold (dB).
// Increasing: fewer updates; decreasing: more frequent updates.
const COEFF_UPDATE_THRESHOLD: f32 = 0.05;
// Bypass threshold for proximity amount.
// Increasing: easier to bypass; decreasing: more likely to process.
const PROXIMITY_BYPASS_EPS: f32 = 0.001;
// De-verb contribution scale for proximity.
// Increasing: more de-verb reduction at high proximity; decreasing: less contribution.
const DEVERB_CONTRIB_SCALE: f32 = 0.4;

/// Low-frequency shaping for "close mic" effect.
///
/// ## Metric Ownership (SHARED with Clarity)
/// This module OWNS and is responsible for:
/// - **Presence ratio**: Contributes to target (≤ 0.01) via LF boost
/// - **Air ratio**: Contributes to target (≤ 0.005) via HF rolloff
///
/// This module must NOT attempt to modify:
/// - RMS, crest factor, RMS variance (owned by Leveler)
/// - Noise floor, SNR (owned by Denoiser)
/// - Early/Late ratio, decay slope (owned by De-reverb)
/// - HF variance (read-only guardrail metric)
///
/// ## Behavioral Rules
/// - Only active when distant mic detected
/// - Disabled entirely when whisper detected
/// - Stops boost when presence target is reached
pub struct Proximity {
    low_shelf: Biquad,
    hf_shelf: Biquad,
    sample_rate: f32,

    // smoothing + update gating
    prox_smoothed: f32,
    last_boost_db: f32,
    last_hf_db: f32,
}

impl Proximity {
    pub fn new(sample_rate: f32) -> Self {
        let mut low = Biquad::new();
        low.update_low_shelf(LOW_SHELF_FREQ_HZ, FILTER_Q, 0.0, sample_rate);

        let mut hf = Biquad::new();
        // IMPORTANT: high shelf, not low shelf
        hf.update_high_shelf(HF_SHELF_FREQ_HZ, FILTER_Q, 0.0, sample_rate);

        Self {
            low_shelf: low,
            hf_shelf: hf,
            sample_rate,
            prox_smoothed: 0.0,
            last_boost_db: 0.0,
            last_hf_db: 0.0,
        }
    }

    pub fn process(&mut self, input: f32, proximity: f32) -> f32 {
        let target = proximity.clamp(0.0, 1.0);

        // Smooth proximity to avoid zippering
        self.prox_smoothed += (target - self.prox_smoothed) * PROX_SMOOTH_COEFF;

        // Ensure we snap to 0.0 if target is 0.0 and we are close enough
        if target <= PROXIMITY_BYPASS_EPS && self.prox_smoothed < PROXIMITY_BYPASS_EPS {
            self.prox_smoothed = 0.0;
        }

        // Bass boost
        let boost_db = MAX_BOOST_DB * self.prox_smoothed;

        // HF rolloff only when very close
        let hf_rolloff_db = if self.prox_smoothed > HF_ROLLOFF_THRESHOLD {
            let close_amount = (self.prox_smoothed - HF_ROLLOFF_THRESHOLD) / HF_ROLLOFF_RANGE;
            HF_ROLLOFF_MAX_DB * close_amount
        } else {
            0.0
        };

        // Only update coefficients when they actually changed enough
        if (boost_db - self.last_boost_db).abs() > COEFF_UPDATE_THRESHOLD {
            self.low_shelf.update_low_shelf(
                LOW_SHELF_FREQ_HZ,
                FILTER_Q,
                boost_db,
                self.sample_rate,
            );
            self.last_boost_db = boost_db;
        }

        if (hf_rolloff_db - self.last_hf_db).abs() > COEFF_UPDATE_THRESHOLD {
            self.hf_shelf.update_high_shelf(
                HF_SHELF_FREQ_HZ,
                FILTER_Q,
                hf_rolloff_db,
                self.sample_rate,
            );
            self.last_hf_db = hf_rolloff_db;
        }

        let s1 = self.low_shelf.process(input);
        self.hf_shelf.process(s1)
    }

    /// If `reverb_amt` is *de-reverb strength* (reverb reduction):
    /// closer mic should need LESS reduction, not more.
    /// Returns a reduction offset (0..DEVERB_CONTRIB_SCALE).
    pub fn get_deverb_contribution(proximity: f32) -> f32 {
        let p = proximity.clamp(0.0, 1.0);
        p * DEVERB_CONTRIB_SCALE
    }
}
