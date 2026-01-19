//! Linked Compressor (Leveler)
//!
//! # Perceptual Contract
//! - **Target Source**: Dynamic speech with inconsistent levels.
//! - **Intended Effect**: Transparent leveling of macroscopic volume changes (consistency).
//! - **Failure Modes**:
//!   - "Pumping" or "breathing" if release is too fast for the gain reduction amount.
//!   - Raising background noise floor during pauses.
//! - **Will Not Do**:
//!   - Brickwall limiting (handled by Limiter).
//!   - Transient shaping (attack is relatively slow to preserve consonants).
//!
//! # Lifecycle
//! - **Active**: Normal operation.
//! - **Bypassed**: Passes audio through.
//!
//! # Loudness Target Philosophy
//! The goal of this module is "perceived conversational consistency" rather than strict LUFS
//! matching or broadcast compliance. It aims to make speech feel stable and effortless to
//! listen to in varied environments (e.g. mobile speakers, noisy rooms) without the fatigue
//! caused by excessive pumping or unnatural density.
//!
//! # Gain Staging
//! 1. **Pre-Leveler**: Input arrives after restoration (denoise/de-verb) and shaping (clarity/proximity).
//! 2. **Leveler Stage**: Transparently manages macroscopic volume changes using a hybrid RMS+Peak detector.
//! 3. **Post-Leveler**: Output gain is applied after leveling to bring the signal into the final range.
//! 4. **Limiter**: Purely for safety; it does not contribute to the "sound" or "tone" of the plugin.
//!
//! # DYNAMICS OWNERSHIP BOUNDARY (Critical)
//!
//! This module is the AUTHORITATIVE long-term gain controller. It owns:
//! - RMS target (0.045 - 0.060)
//! - Crest factor maintenance (23.0 - 27.0 dB)
//! - RMS variance reduction (≤ 0.0015)
//!
//! ## Hard Rule: Speech Envelope Exclusion
//!
//! The shared `SpeechSidechain` envelope from `SpeechConfidenceEstimator` MUST NOT:
//! - Drive gain reduction directly
//! - Be the source of truth for level detection
//! - Replace the internal RMS/Peak envelope followers
//!
//! The speech envelope MAY only be used to:
//! - Gate dynamics behavior (e.g., hold gain during detected silence)
//! - Bias reaction speed (slower release during speech transitions)
//! - Prevent reactions during silence (makeup gain gating)
//!
//! Currently this module does NOT read the shared speech envelope, which is CORRECT.
//! The internal noise floor tracking and gating is sufficient and independent.
//!
//! ## Rationale
//!
//! If the speech envelope drove dynamics directly, it would create a hidden feedback
//! loop where restoration affects speech detection which affects leveling which affects
//! the signal that restoration sees. This leads to pumping and gain fighting.

use crate::dsp::utils::{db_to_lin, lin_to_db, time_constant_coeff, update_env_sq, DB_EPS};

// =============================================================================
// Data-Driven Calibration Constants (Task 4)
// =============================================================================

/// Crest factor threshold below which we slow attack and reduce ratio
/// From spec: "If crest < 22 dB → slow attack / reduce ratio"
const CREST_ADAPTATION_THRESHOLD_DB: f32 = 22.0;

/// Attack multiplier when crest is low (slower attack preserves dynamics)
const LOW_CREST_ATTACK_MULT: f32 = 1.5;

/// Ratio reduction factor when crest is low (gentler compression)
const LOW_CREST_RATIO_MULT: f32 = 0.7;

/// RMS variance threshold above which we increase smoothing
/// From spec: "If RMS variance > target → increase smoothing"
const RMS_VARIANCE_THRESHOLD: f32 = 0.0015;

/// Release multiplier when RMS variance is high (more smoothing)
const HIGH_VARIANCE_RELEASE_MULT: f32 = 1.3;

// Constants: unless marked "Must not change", these are tunable for behavior.
// Initial noise floor estimate for gating.
// Increasing: gate opens later (less sensitive); decreasing: gate opens earlier.
const NOISE_FLOOR_INIT: f32 = 1e-4;
// Half scalar used in knee computation.
// Must not change: knee math relies on 0.5.
const HALF: f32 = 0.5;
// Time constant (ms) for noise floor to fall.
// Increasing: slower tracking of quieter noise; decreasing: faster tracking.
const NOISE_FLOOR_DOWN_MS: f32 = 300.0;
// Time constant (ms) for noise floor to rise.
// Increasing: slower tracking of louder noise; decreasing: faster tracking.
const NOISE_FLOOR_UP_MS: f32 = 8000.0;
// Gate multiplier applied to noise floor for detector gating.
// Increasing: stricter gating; decreasing: more sensitive gating.
const GATE_FLOOR_MULT: f32 = 2.5;
// RMS detector attack (ms).
// Increasing: slower reaction to level; decreasing: faster reaction.
const RMS_ATTACK_MS: f32 = 30.0;
// RMS detector release (ms).
// Increasing: smoother decay; decreasing: faster decay.
const RMS_RELEASE_MS: f32 = 250.0;
// Peak detector attack (ms).
// Increasing: slower reaction to peaks; decreasing: faster reaction.
const PEAK_ATTACK_MS: f32 = 10.0;
// Peak detector release (ms).
// Increasing: smoother decay; decreasing: faster decay.
const PEAK_RELEASE_MS: f32 = 120.0;
// Hybrid detector weights (RMS vs peak).
// Increasing RMS weight: smoother detector; increasing peak weight: more transient-sensitive.
const HYBRID_RMS_WEIGHT: f32 = 0.75;
const HYBRID_PEAK_WEIGHT: f32 = 0.25;
// Target level for the gentle leveler stage (dBFS).
// Increasing (less negative): less leveling; decreasing: more leveling.
const LEVELER_TARGET_DB: f32 = -24.0;
// Over-threshold regions for ratio staging (dB).
// Increasing: ratios switch later; decreasing: ratios switch sooner.
const LEVELER_RATIO_LOW_DB: f32 = 3.0;
const LEVELER_RATIO_MID_DB: f32 = 8.0;
// Ratios for staged leveling.
// Increasing: stronger compression; decreasing: gentler compression.
const LEVELER_RATIO_LOW: f32 = 1.6;
const LEVELER_RATIO_MID: f32 = 2.2;
const LEVELER_RATIO_HIGH: f32 = 3.2;
// Knee width for the leveler stage (dB).
// Increasing: softer knee; decreasing: harder knee.
const LEVELER_KNEE_DB: f32 = 10.0;
// Peak tamer threshold (dBFS).
// Increasing (less negative): more peak limiting; decreasing: less peak limiting.
const PEAK_TAMER_THRESHOLD_DB: f32 = -12.0;
// Peak tamer ratio.
// Increasing: stronger peak control; decreasing: gentler peak control.
const PEAK_TAMER_RATIO: f32 = 10.0;
// Knee width for peak tamer (dB).
// Increasing: softer knee; decreasing: harder knee.
const PEAK_TAMER_KNEE_DB: f32 = 4.0;
// Gain reduction envelope smoothing (0..1).
// Increasing: slower meter decay; decreasing: faster meter decay.
const GAIN_REDUCTION_AVG_REL: f32 = 0.995;
// Peak gain reduction display decay (0..1).
// Increasing: slower peak decay; decreasing: faster decay.
const GAIN_REDUCTION_PEAK_REL: f32 = 0.9997;
// Makeup gate margin (dB) above noise floor.
// Increasing: requires louder material for makeup; decreasing: applies makeup more often.
const MAKEUP_MARGIN_DB: f32 = 12.0;
// Makeup scale factor for reduction compensation.
// Increasing: more makeup; decreasing: less makeup.
const MAKEUP_SCALE: f32 = 0.45;
// Maximum makeup gain (dB).
// Increasing: louder makeup; decreasing: more conservative.
const MAKEUP_MAX_DB: f32 = 4.0;
// Amount below which compressor is bypassed (slightly higher than default BYPASS_AMOUNT_EPS
// to account for the compressor's makeup gain sensitivity).
// Increasing: easier to bypass; decreasing: more likely to process.
const COMPRESSOR_BYPASS_EPS: f32 = 0.01;

/// Stereo-linked VO compressor with automatic makeup gain.
/// Drop-in replacement for the existing LinkedCompressor.
///
/// ## Metric Ownership (EXCLUSIVE)
/// This module OWNS and is responsible for:
/// - **RMS**: Moves toward target range (0.045 - 0.060)
/// - **Crest factor**: Maintains target range (23.0 - 27.0 dB)
/// - **RMS variance**: Reduces toward target (≤ 0.0015)
///
/// This module must NOT attempt to modify:
/// - Noise floor (owned by Denoiser)
/// - Early/Late ratio (owned by De-reverb)
/// - Presence/Air ratios (owned by Proximity + Clarity)
/// - HF variance (read-only guardrail metric)
///
/// ## Data-Driven Calibration
/// - Adapts attack/ratio based on crest factor
/// - Adapts release/smoothing based on RMS variance
pub struct LinkedCompressor {
    // RMS envelope (squared) per channel
    env_sq_l: f32,
    env_sq_r: f32,

    // Peak envelope (linear) per channel
    peak_env_l: f32,
    peak_env_r: f32,

    // Noise floor estimate (linear)
    noise_floor: f32,

    sample_rate: f32,

    // Metering / makeup tracking
    gain_reduction_envelope: f32,
    peak_gain_reduction_db: f32,

    // Data-driven adaptation (from AudioProfile)
    // Smoothed to prevent sudden changes
    crest_factor_db: f32,
    rms_variance: f32,
    adaptation_coeff: f32,
}

impl LinkedCompressor {
    pub fn new(sr: f32) -> Self {
        // Adaptation coefficient: ~100ms smoothing
        let adaptation_coeff = time_constant_coeff(100.0, sr);

        Self {
            env_sq_l: 0.0,
            env_sq_r: 0.0,
            peak_env_l: 0.0,
            peak_env_r: 0.0,
            noise_floor: NOISE_FLOOR_INIT,
            sample_rate: sr,
            gain_reduction_envelope: 0.0,
            peak_gain_reduction_db: 0.0,
            crest_factor_db: 25.0, // Default: middle of target range
            rms_variance: 0.001,   // Default: below threshold
            adaptation_coeff,
        }
    }

    /// Update adaptation parameters from AudioProfile
    /// Call this once per buffer with the input profile metrics
    pub fn update_from_profile(&mut self, crest_factor_db: f32, rms_variance: f32) {
        // Smooth the adaptation parameters to prevent sudden changes
        self.crest_factor_db = self.adaptation_coeff * self.crest_factor_db
            + (1.0 - self.adaptation_coeff) * crest_factor_db;
        self.rms_variance = self.adaptation_coeff * self.rms_variance
            + (1.0 - self.adaptation_coeff) * rms_variance;
    }

    #[inline]
    fn coeff(&self, time_ms: f32) -> f32 {
        time_constant_coeff(time_ms, self.sample_rate)
    }

    #[inline]
    fn soft_knee(over_db: f32, ratio: f32, knee_db: f32) -> f32 {
        let half = HALF * knee_db;
        if over_db <= -half {
            0.0
        } else if over_db >= half {
            over_db * (1.0 - 1.0 / ratio)
        } else {
            let x = over_db + half;
            let y = (x * x) / (2.0 * knee_db);
            y * (1.0 - 1.0 / ratio)
        }
    }

    pub fn compute_gain(&mut self, input_l: f32, input_r: f32, amount: f32) -> f32 {
        // Bypass semantics preserved
        if amount < COMPRESSOR_BYPASS_EPS {
            self.env_sq_l = 0.0;
            self.env_sq_r = 0.0;
            self.peak_env_l = 0.0;
            self.peak_env_r = 0.0;
            self.noise_floor = NOISE_FLOOR_INIT;
            self.gain_reduction_envelope = 0.0;
            self.peak_gain_reduction_db = 0.0;
            return 1.0;
        }

        // ---------------------------------------------------------------------
        // 1. Noise floor tracking (down fast, up very slow)
        // ---------------------------------------------------------------------
        let abs_l = input_l.abs();
        let abs_r = input_r.abs();
        let abs_max = abs_l.max(abs_r).max(DB_EPS);

        let nf_down = self.coeff(NOISE_FLOOR_DOWN_MS);
        let nf_up = self.coeff(NOISE_FLOOR_UP_MS);
        if abs_max < self.noise_floor {
            self.noise_floor = nf_down * self.noise_floor + (1.0 - nf_down) * abs_max;
        } else {
            self.noise_floor = nf_up * self.noise_floor + (1.0 - nf_up) * abs_max;
        }

        // Gate for analysis only (prevents breaths / room tone driving detector)
        let gated_l = if abs_l < self.noise_floor * GATE_FLOOR_MULT {
            0.0
        } else {
            abs_l
        };
        let gated_r = if abs_r < self.noise_floor * GATE_FLOOR_MULT {
            0.0
        } else {
            abs_r
        };

        // ---------------------------------------------------------------------
        // 2. RMS envelopes (per channel, stereo-linked via max)
        // ---------------------------------------------------------------------
        // Data-driven adaptation: adjust attack/release based on profile
        let attack_mult = if self.crest_factor_db < CREST_ADAPTATION_THRESHOLD_DB {
            LOW_CREST_ATTACK_MULT // Slower attack for low crest
        } else {
            1.0
        };
        let release_mult = if self.rms_variance > RMS_VARIANCE_THRESHOLD {
            HIGH_VARIANCE_RELEASE_MULT // More smoothing for high variance
        } else {
            1.0
        };

        let rms_atk = self.coeff(RMS_ATTACK_MS * attack_mult);
        let rms_rel = self.coeff(RMS_RELEASE_MS * release_mult);

        let sq_l = gated_l * gated_l;
        let sq_r = gated_r * gated_r;

        self.env_sq_l = update_env_sq(self.env_sq_l, sq_l, rms_atk, rms_rel);
        self.env_sq_r = update_env_sq(self.env_sq_r, sq_r, rms_atk, rms_rel);

        let rms_l = self.env_sq_l.sqrt();
        let rms_r = self.env_sq_r.sqrt();
        let rms_max = rms_l.max(rms_r);

        // ---------------------------------------------------------------------
        // 3. Peak envelopes (for plosive / shout awareness)
        // ---------------------------------------------------------------------
        let peak_atk = self.coeff(PEAK_ATTACK_MS);
        let peak_rel = self.coeff(PEAK_RELEASE_MS);

        self.peak_env_l = if gated_l > self.peak_env_l {
            peak_atk * self.peak_env_l + (1.0 - peak_atk) * gated_l
        } else {
            peak_rel * self.peak_env_l + (1.0 - peak_rel) * gated_l
        };

        self.peak_env_r = if gated_r > self.peak_env_r {
            peak_atk * self.peak_env_r + (1.0 - peak_atk) * gated_r
        } else {
            peak_rel * self.peak_env_r + (1.0 - peak_rel) * gated_r
        };

        let peak_max = self.peak_env_l.max(self.peak_env_r);

        // Hybrid detector (speech-appropriate)
        let hybrid = (HYBRID_RMS_WEIGHT * rms_max + HYBRID_PEAK_WEIGHT * peak_max).max(DB_EPS);
        let hybrid_db = lin_to_db(hybrid);
        let peak_db = lin_to_db(peak_max.max(DB_EPS));

        // ---------------------------------------------------------------------
        // 4. Stage 1: VO leveler (gentle, wide knee)
        // ---------------------------------------------------------------------
        let target_db = LEVELER_TARGET_DB;
        let over1 = hybrid_db - target_db;

        // Data-driven adaptation: reduce ratio for low crest factor
        let ratio_mult = if self.crest_factor_db < CREST_ADAPTATION_THRESHOLD_DB {
            LOW_CREST_RATIO_MULT // Gentler compression for low crest
        } else {
            1.0
        };

        let ratio1 = if over1 < LEVELER_RATIO_LOW_DB {
            1.0 + (LEVELER_RATIO_LOW - 1.0) * ratio_mult
        } else if over1 < LEVELER_RATIO_MID_DB {
            1.0 + (LEVELER_RATIO_MID - 1.0) * ratio_mult
        } else {
            1.0 + (LEVELER_RATIO_HIGH - 1.0) * ratio_mult
        };

        let red1_db = Self::soft_knee(over1, ratio1, LEVELER_KNEE_DB);

        // ---------------------------------------------------------------------
        // 5. Stage 2: Peak tamer (transparent, fast)
        // ---------------------------------------------------------------------
        let over2 = peak_db - PEAK_TAMER_THRESHOLD_DB;
        let red2_db = Self::soft_knee(over2, PEAK_TAMER_RATIO, PEAK_TAMER_KNEE_DB);

        let total_reduction_db = (red1_db + red2_db).max(0.0);
        let gain = db_to_lin(-total_reduction_db);

        // ---------------------------------------------------------------------
        // 6. Metering + automatic makeup (VO-safe)
        // ---------------------------------------------------------------------
        let avg_rel = GAIN_REDUCTION_AVG_REL;
        self.gain_reduction_envelope =
            self.gain_reduction_envelope * avg_rel + total_reduction_db * (1.0 - avg_rel);

        let display_rel = GAIN_REDUCTION_PEAK_REL;
        if total_reduction_db > self.peak_gain_reduction_db {
            self.peak_gain_reduction_db = total_reduction_db;
        } else {
            self.peak_gain_reduction_db *= display_rel;
        }

        // Conservative makeup: only compensate leveler, never room tone
        let margin_db = hybrid_db - lin_to_db(self.noise_floor.max(DB_EPS));
        let makeup_db = if margin_db > MAKEUP_MARGIN_DB {
            (self.gain_reduction_envelope * MAKEUP_SCALE).min(MAKEUP_MAX_DB)
        } else {
            0.0
        };

        let makeup = db_to_lin(makeup_db);

        gain * makeup
    }

    pub fn get_gain_reduction_db(&self) -> f32 {
        self.peak_gain_reduction_db
    }
}
