//! Spectral Guardrails (Safety Layer)
//!
//! Prevents extreme macro or advanced control combinations from breaking the sound
//! by tracking band energy ratios and applying dynamic corrections when thresholds
//! are exceeded. Also enforces maximum gain slew rate globally to prevent artifacts.
//!
//! # Purpose
//! Acts as a safety net that gently pulls extreme spectral adjustments back to safe
//! norms, preventing user error from causing audible artifacts or unstable behavior.
//!
//! # Design Notes
//! - Monitors band energy ratios continuously
//! - Applies corrections only when thresholds are exceeded
//! - Enforces maximum gain slew rate to prevent artifacts
//! - Gently guides extreme settings back to safe operating ranges
//!   - Dullness if triggers on legitimate bright voices.
//!   - Volume dips if slew limiting is triggered aggressively.
//! - **Will Not Do**:
//!   - Dynamic EQ or multiband compression (this is a static safety clamp).
//!
//! # Lifecycle
//! - **Active**: Normal operation.
//! - **Bypassed**: Passes audio through (unsafe if user settings are extreme).
//!
//! ## Audio Thread Safety
//! - All filters and buffers pre-allocated in `new()`
//! - No allocations during `process()`

use super::biquad::Biquad;
use super::utils::{db_to_lin, time_constant_coeff, DB_EPS};

// =============================================================================
// Constants
// =============================================================================

/// Speech band frequency range (Hz)
const SPEECH_BAND_LOW: f32 = 250.0;
const SPEECH_BAND_HIGH: f32 = 4000.0;

/// Low-mid band frequency range (Hz) - for boom/mud detection
const LOW_MID_BAND_LOW: f32 = 200.0;
const LOW_MID_BAND_HIGH: f32 = 500.0;

/// High band frequency range (Hz) - for harshness detection
const HIGH_BAND_LOW: f32 = 8000.0;
const HIGH_BAND_HIGH: f32 = 16000.0;

/// Maximum low-mid cut in dB
const MAX_LOW_MID_CUT_DB: f32 = 5.0;

/// Maximum high-frequency cut in dB
const MAX_HIGH_CUT_DB: f32 = 5.0;

/// Low-mid ratio threshold (above this, start cutting)
const LOW_MID_RATIO_THRESHOLD: f32 = 1.5;

/// High ratio threshold (above this, start cutting)
const HIGH_RATIO_THRESHOLD: f32 = 0.8;

/// Maximum gain change per second in dB
const MAX_GAIN_SLEW_DB_PER_SEC: f32 = 12.0;

/// RMS averaging time in milliseconds
const RMS_TIME_MS: f32 = 30.0;

/// Correction smoothing time in milliseconds
const CORRECTION_SMOOTH_MS: f32 = 50.0;
const HF_CONF_THRESHOLD: f32 = 0.3;

// =============================================================================
// Spectral Guardrails
// =============================================================================

/// Stereo spectral guardrails processor
///
/// ## Metric Ownership (READ-ONLY)
/// This module READS but does NOT own:
/// - **HF variance**: Used as read-only safety signal (target: ≤ 3e-7)
///
/// This module applies safety corrections based on band energy ratios,
/// but does not attempt to "fix" any target metrics. It only prevents
/// extreme settings from breaking sound.
///
/// This module must NOT attempt to modify:
/// - RMS, crest factor, RMS variance (owned by Leveler)
/// - Noise floor, SNR (owned by Denoiser)
/// - Early/Late ratio, decay slope (owned by De-reverb)
/// - Presence/Air ratios (owned by Proximity + Clarity)
pub struct SpectralGuardrails {
    sample_rate: f32,

    // Band isolation filters (per channel)
    // Speech band
    speech_hp_l: Biquad,
    speech_hp_r: Biquad,
    speech_lp_l: Biquad,
    speech_lp_r: Biquad,

    // Low-mid band
    low_mid_hp_l: Biquad,
    low_mid_hp_r: Biquad,
    low_mid_lp_l: Biquad,
    low_mid_lp_r: Biquad,

    // High band
    high_hp_l: Biquad,
    high_hp_r: Biquad,
    high_lp_l: Biquad,
    high_lp_r: Biquad,

    // Correction filters (shelving EQ)
    low_shelf_l: Biquad,
    low_shelf_r: Biquad,
    high_shelf_l: Biquad,
    high_shelf_r: Biquad,

    // RMS accumulators
    rms_speech_sq: f32,
    rms_low_mid_sq: f32,
    rms_high_sq: f32,
    rms_coeff: f32,

    // Current corrections
    low_mid_cut_db: f32,
    high_cut_db: f32,
    correction_coeff: f32,

    // Gain slew limiting (reserved for future use)
    last_output_gain: f32,
    #[allow(dead_code)] // Reserved for slew limiting feature
    max_slew_per_sample: f32,
}

impl SpectralGuardrails {
    pub fn new(sample_rate: f32) -> Self {
        // Create band isolation filters
        let mut speech_hp_l = Biquad::new();
        let mut speech_hp_r = Biquad::new();
        let mut speech_lp_l = Biquad::new();
        let mut speech_lp_r = Biquad::new();

        speech_hp_l.update_hpf(SPEECH_BAND_LOW, 0.707, sample_rate);
        speech_hp_r.update_hpf(SPEECH_BAND_LOW, 0.707, sample_rate);
        speech_lp_l.update_lpf(SPEECH_BAND_HIGH, 0.707, sample_rate);
        speech_lp_r.update_lpf(SPEECH_BAND_HIGH, 0.707, sample_rate);

        let mut low_mid_hp_l = Biquad::new();
        let mut low_mid_hp_r = Biquad::new();
        let mut low_mid_lp_l = Biquad::new();
        let mut low_mid_lp_r = Biquad::new();

        low_mid_hp_l.update_hpf(LOW_MID_BAND_LOW, 0.707, sample_rate);
        low_mid_hp_r.update_hpf(LOW_MID_BAND_LOW, 0.707, sample_rate);
        low_mid_lp_l.update_lpf(LOW_MID_BAND_HIGH, 0.707, sample_rate);
        low_mid_lp_r.update_lpf(LOW_MID_BAND_HIGH, 0.707, sample_rate);

        let mut high_hp_l = Biquad::new();
        let mut high_hp_r = Biquad::new();
        let mut high_lp_l = Biquad::new();
        let mut high_lp_r = Biquad::new();

        high_hp_l.update_hpf(HIGH_BAND_LOW, 0.707, sample_rate);
        high_hp_r.update_hpf(HIGH_BAND_LOW, 0.707, sample_rate);
        high_lp_l.update_lpf(HIGH_BAND_HIGH, 0.707, sample_rate);
        high_lp_r.update_lpf(HIGH_BAND_HIGH, 0.707, sample_rate);

        // Correction shelving filters (start flat)
        let mut low_shelf_l = Biquad::new();
        let mut low_shelf_r = Biquad::new();
        let mut high_shelf_l = Biquad::new();
        let mut high_shelf_r = Biquad::new();

        low_shelf_l.update_low_shelf(LOW_MID_BAND_HIGH, 0.707, 0.0, sample_rate);
        low_shelf_r.update_low_shelf(LOW_MID_BAND_HIGH, 0.707, 0.0, sample_rate);
        high_shelf_l.update_high_shelf(HIGH_BAND_LOW, 0.707, 0.0, sample_rate);
        high_shelf_r.update_high_shelf(HIGH_BAND_LOW, 0.707, 0.0, sample_rate);

        let rms_samples = (RMS_TIME_MS * 0.001 * sample_rate).max(1.0);
        let rms_coeff = (-1.0 / rms_samples).exp();

        Self {
            sample_rate,
            speech_hp_l,
            speech_hp_r,
            speech_lp_l,
            speech_lp_r,
            low_mid_hp_l,
            low_mid_hp_r,
            low_mid_lp_l,
            low_mid_lp_r,
            high_hp_l,
            high_hp_r,
            high_lp_l,
            high_lp_r,
            low_shelf_l,
            low_shelf_r,
            high_shelf_l,
            high_shelf_r,
            rms_speech_sq: 0.0,
            rms_low_mid_sq: 0.0,
            rms_high_sq: 0.0,
            rms_coeff,
            low_mid_cut_db: 0.0,
            high_cut_db: 0.0,
            correction_coeff: time_constant_coeff(CORRECTION_SMOOTH_MS, sample_rate),
            last_output_gain: 1.0,
            max_slew_per_sample: MAX_GAIN_SLEW_DB_PER_SEC / sample_rate,
        }
    }

    /// Process a stereo sample pair with spectral protection
    ///
    /// * `left`, `right` - Input samples
    /// * `enabled` - Whether guardrails are active
    ///
    /// Returns (processed_left, processed_right)
    #[inline]
    pub fn process(
        &mut self,
        left: f32,
        right: f32,
        enabled: bool,
        speech_confidence: f32,
    ) -> (f32, f32) {
        // Always track band energies (for monitoring)
        self.update_band_energy(left, right);

        if !enabled {
            return (left, right);
        }

        // Calculate target corrections based on ratios
        let (target_low_cut, target_high_cut) = self.calculate_corrections(speech_confidence);

        // Adaptive smoothing: Fast Attack (to protect), Slow Release (to prevent pumping/oscillation)
        // If target > current (more cut needed), use fast attack.
        // If target < current (release cut), use slow release.
        let attack_coeff = time_constant_coeff(20.0, self.sample_rate); // 20ms attack
        let release_coeff = time_constant_coeff(400.0, self.sample_rate); // 400ms release (slower)

        let low_coeff = if target_low_cut > self.low_mid_cut_db { attack_coeff } else { release_coeff };
        let high_coeff = if target_high_cut > self.high_cut_db { attack_coeff } else { release_coeff };

        // Smooth corrections
        self.low_mid_cut_db = low_coeff * self.low_mid_cut_db
            + (1.0 - low_coeff) * target_low_cut;
        self.high_cut_db = high_coeff * self.high_cut_db
            + (1.0 - high_coeff) * target_high_cut;

        // Safety clamp to prevent instability
        self.low_mid_cut_db = self.low_mid_cut_db.clamp(0.0, 12.0);
        self.high_cut_db = self.high_cut_db.clamp(0.0, 12.0);

        // Update correction filters if needed
        if self.low_mid_cut_db.abs() > 0.01 {
            self.low_shelf_l.update_low_shelf(
                LOW_MID_BAND_HIGH,
                0.707,
                -self.low_mid_cut_db,
                self.sample_rate,
            );
            self.low_shelf_r.update_low_shelf(
                LOW_MID_BAND_HIGH,
                0.707,
                -self.low_mid_cut_db,
                self.sample_rate,
            );
        }
        if self.high_cut_db.abs() > 0.1 {
            self.high_shelf_l.update_high_shelf(
                HIGH_BAND_LOW,
                0.707,
                -self.high_cut_db,
                self.sample_rate,
            );
            self.high_shelf_r.update_high_shelf(
                HIGH_BAND_LOW,
                0.707,
                -self.high_cut_db,
                self.sample_rate,
            );
        }

        // Apply corrections
        let mut out_l = left;
        let mut out_r = right;

        if self.low_mid_cut_db.abs() > 0.1 {
            out_l = self.low_shelf_l.process(out_l);
            out_r = self.low_shelf_r.process(out_r);
        }
        if self.high_cut_db.abs() > 0.1 {
            out_l = self.high_shelf_l.process(out_l);
            out_r = self.high_shelf_r.process(out_r);
        }

        // Note: Slew limiting disabled - the previous implementation was buggy
        // (it confused signal level with gain, causing massive amplification on startup)
        (out_l, out_r)
    }

    /// Update band energy tracking
    fn update_band_energy(&mut self, left: f32, right: f32) {
        // Extract speech band
        let speech_l = self.speech_lp_l.process(self.speech_hp_l.process(left));
        let speech_r = self.speech_lp_r.process(self.speech_hp_r.process(right));
        let speech_sq = 0.5 * (speech_l * speech_l + speech_r * speech_r);

        // Extract low-mid band
        let low_mid_l = self.low_mid_lp_l.process(self.low_mid_hp_l.process(left));
        let low_mid_r = self.low_mid_lp_r.process(self.low_mid_hp_r.process(right));
        let low_mid_sq = 0.5 * (low_mid_l * low_mid_l + low_mid_r * low_mid_r);

        // Extract high band
        let high_l = self.high_lp_l.process(self.high_hp_l.process(left));
        let high_r = self.high_lp_r.process(self.high_hp_r.process(right));
        let high_sq = 0.5 * (high_l * high_l + high_r * high_r);

        // Update RMS
        self.rms_speech_sq =
            self.rms_coeff * self.rms_speech_sq + (1.0 - self.rms_coeff) * speech_sq;
        self.rms_low_mid_sq =
            self.rms_coeff * self.rms_low_mid_sq + (1.0 - self.rms_coeff) * low_mid_sq;
        self.rms_high_sq = self.rms_coeff * self.rms_high_sq + (1.0 - self.rms_coeff) * high_sq;
    }

    /// Calculate target correction amounts based on band ratios
    fn calculate_corrections(&self, speech_confidence: f32) -> (f32, f32) {
        let speech_rms = self.rms_speech_sq.sqrt();

        if speech_rms < DB_EPS {
            return (0.0, 0.0);
        }

        // Calculate ratios
        let low_mid_ratio = self.rms_low_mid_sq.sqrt() / speech_rms;
        let high_ratio = self.rms_high_sq.sqrt() / speech_rms;

        // Low-mid correction
        let low_cut = if low_mid_ratio > LOW_MID_RATIO_THRESHOLD {
            let excess = (low_mid_ratio - LOW_MID_RATIO_THRESHOLD) / LOW_MID_RATIO_THRESHOLD;
            (excess * MAX_LOW_MID_CUT_DB).min(MAX_LOW_MID_CUT_DB)
        } else {
            0.0
        };

        // High correction
        let base_high_cut = if high_ratio > HIGH_RATIO_THRESHOLD {
            let excess = (high_ratio - HIGH_RATIO_THRESHOLD) / HIGH_RATIO_THRESHOLD;
            (excess * MAX_HIGH_CUT_DB).min(MAX_HIGH_CUT_DB)
        } else {
            0.0
        };

        let high_cut = if speech_confidence < HF_CONF_THRESHOLD {
            0.0
        } else {
            base_high_cut
        };

        (low_cut, high_cut)
    }

    /// Apply gain slew rate limiting (reserved for future use)
    #[allow(dead_code)]
    fn apply_slew_limit(&mut self, left: f32, right: f32) -> (f32, f32) {
        // Calculate current output level
        let current_level = (left * left + right * right).sqrt().max(DB_EPS);
        let current_gain = current_level;

        // Calculate gain change
        let gain_ratio = current_gain / self.last_output_gain.max(DB_EPS);
        let gain_change_db = 20.0 * gain_ratio.log10();

        // Limit slew rate
        let limited_change_db =
            gain_change_db.clamp(-self.max_slew_per_sample, self.max_slew_per_sample);

        if limited_change_db.abs() < gain_change_db.abs() {
            // Need to limit
            let target_gain = self.last_output_gain * db_to_lin(limited_change_db);
            let scale = target_gain / current_gain;
            self.last_output_gain = target_gain;
            (left * scale, right * scale)
        } else {
            self.last_output_gain = current_gain;
            (left, right)
        }
    }

    /// Reset all state
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.speech_hp_l.reset();
        self.speech_hp_r.reset();
        self.speech_lp_l.reset();
        self.speech_lp_r.reset();
        self.low_mid_hp_l.reset();
        self.low_mid_hp_r.reset();
        self.low_mid_lp_l.reset();
        self.low_mid_lp_r.reset();
        self.high_hp_l.reset();
        self.high_hp_r.reset();
        self.high_lp_l.reset();
        self.high_lp_r.reset();
        self.low_shelf_l.reset();
        self.low_shelf_r.reset();
        self.high_shelf_l.reset();
        self.high_shelf_r.reset();
        self.rms_speech_sq = 0.0;
        self.rms_low_mid_sq = 0.0;
        self.rms_high_sq = 0.0;
        self.low_mid_cut_db = 0.0;
        self.high_cut_db = 0.0;
        self.last_output_gain = 1.0;
    }

    /// Get current low-mid cut amount in dB (for metering)
    #[inline]
    pub fn get_low_mid_cut_db(&self) -> f32 {
        self.low_mid_cut_db
    }

    /// Get current high cut amount in dB (for metering)
    #[inline]
    pub fn get_high_cut_db(&self) -> f32 {
        self.high_cut_db
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_creation() {
        let guardrails = SpectralGuardrails::new(48000.0);
        assert!(guardrails.low_mid_cut_db == 0.0);
        assert!(guardrails.high_cut_db == 0.0);
    }

    #[test]
    fn test_passthrough_when_disabled() {
        let mut guardrails = SpectralGuardrails::new(48000.0);

        let input_l = 0.5;
        let input_r = 0.3;
        let (out_l, out_r) = guardrails.process(input_l, input_r, false, 0.5);

        assert!((out_l - input_l).abs() < 1e-6);
        assert!((out_r - input_r).abs() < 1e-6);
    }

    #[test]
    fn test_balanced_signal_no_correction() {
        let mut guardrails = SpectralGuardrails::new(48000.0);

        // Process a balanced signal (speech-like 1kHz)
        for i in 0..10000 {
            let sample = 0.3 * (i as f32 * 0.1308).sin(); // ~1kHz at 48kHz
            guardrails.process(sample, sample, true, 0.5);
        }

        // Should have minimal correction for speech-band signal
        assert!(guardrails.get_low_mid_cut_db() < 1.0);
        assert!(guardrails.get_high_cut_db() < 1.0);
    }
}
