//! Linked Compressor (Leveler)
//!
//! Stereo-linked compressor designed for voice level consistency, providing
//! smooth gain control to maintain consistent loudness while preserving
//! natural speech dynamics. Combines RMS and peak detection for balanced
//! compression suitable for spoken voice.
//!
//! # Purpose
//! Maintain consistent vocal loudness through stereo-linked RMS+Peak hybrid
//! compression with adaptive behavior based on speech characteristics.
//! Provides smooth, natural-sounding level control for voice applications.
//!
//! # Design Notes
//! - Stereo-linked RMS+Peak hybrid compression
//! - Adaptive behavior based on speech characteristics
//! - Soft knees for transparent operation
//! - Noise-floor gated detection to avoid pumping

use crate::dsp::envelope::VoiceEnvelope;
use crate::dsp::utils::{db_to_lin, lin_to_db, time_constant_coeff, DB_EPS};

// =============================================================================
// Data-Driven Calibration Constants (Task 4)
// =============================================================================

const CREST_ADAPTATION_THRESHOLD_DB: f32 = 22.0;
const LOW_CREST_RATIO_MULT: f32 = 0.7;

// =============================================================================
// Behavior Constants
// =============================================================================

const HALF: f32 = 0.5;

const HYBRID_RMS_WEIGHT: f32 = 0.75;
const HYBRID_PEAK_WEIGHT: f32 = 0.25;

const LEVELER_TARGET_DB: f32 = -24.0;

const LEVELER_RATIO_LOW_DB: f32 = 3.0;
const LEVELER_RATIO_MID_DB: f32 = 8.0;

const LEVELER_RATIO_LOW: f32 = 1.6;
const LEVELER_RATIO_MID: f32 = 2.2;
const LEVELER_RATIO_HIGH: f32 = 3.2;

const LEVELER_KNEE_DB: f32 = 10.0;

const PEAK_TAMER_THRESHOLD_DB: f32 = -12.0;
const PEAK_TAMER_RATIO: f32 = 10.0;
const PEAK_TAMER_KNEE_DB: f32 = 4.0;

const GAIN_REDUCTION_AVG_REL: f32 = 0.995;
const GAIN_REDUCTION_PEAK_REL: f32 = 0.9997;

const MAKEUP_MARGIN_DB: f32 = 12.0;
const MAKEUP_SCALE: f32 = 0.45;
const MAKEUP_MAX_DB: f32 = 4.0;

const COMPRESSOR_BYPASS_EPS: f32 = 0.01;

// Bypass smoothing (prevents clicks when amount crosses eps)
const BYPASS_GAIN_RELEASE_MS: f32 = 60.0;

// Gain reduction smoothing (prevents pumping)
const GAIN_ATTACK_MS: f32 = 40.0; // Slower attack to prevent pumping
const GAIN_RELEASE_MS_FAST: f32 = 400.0; // Fast release for light compression
const GAIN_RELEASE_MS_SLOW: f32 = 900.0; // Slow release for heavy compression

// Peak tamer separate envelope
const PEAK_TAMER_ATTACK_MS: f32 = 5.0; // Very fast attack for peaks
const PEAK_TAMER_RELEASE_MS: f32 = 120.0; // Fast release to avoid pumping

// Silence freeze threshold
const SILENCE_FREEZE_THRESHOLD: f32 = 0.2;

/// Stereo-linked VO compressor with automatic makeup gain.
/// Drop-in replacement for the existing LinkedCompressor.
///
/// ## Metric Ownership (EXCLUSIVE)
/// This module OWNS and is responsible for:
/// - **RMS**: Moves toward target range (0.045 - 0.060)
/// - **Crest factor**: Maintains target range (23.0 - 27.0 dB)
/// - **RMS variance**: Reduces toward target (≤ 0.0015)
///
/// ## Data-Driven Calibration
/// - Adapts attack/ratio based on crest factor
/// - Adapts release/smoothing based on RMS variance
pub struct LinkedCompressor {
    sample_rate: f32,

    // Metering / makeup tracking
    gain_reduction_envelope_db: f32, // averaged reduction (dB)
    peak_gain_reduction_db: f32,     // peak reduction display (dB)

    // Data-driven adaptation (from AudioProfile), smoothed
    crest_factor_db: f32,
    rms_variance: f32,
    adaptation_coeff: f32,

    // Smoothed output gain for bypass/amount transitions
    out_gain_smooth: f32,

    // Smoothed gain reduction to prevent pumping
    reduction_smooth_db: f32,

    // Separate peak tamer envelope
    peak_reduction_smooth_db: f32,
}

impl LinkedCompressor {
    pub fn new(sr: f32) -> Self {
        let adaptation_coeff = time_constant_coeff(100.0, sr); // ~100ms smoothing

        Self {
            sample_rate: sr,
            gain_reduction_envelope_db: 0.0,
            peak_gain_reduction_db: 0.0,
            crest_factor_db: 25.0,
            rms_variance: 0.001,
            adaptation_coeff,
            out_gain_smooth: 1.0,
            reduction_smooth_db: 0.0,
            peak_reduction_smooth_db: 0.0,
        }
    }

    /// Update adaptation parameters from AudioProfile.
    /// Call once per buffer (recommended), not necessarily per sample.
    pub fn update_from_profile(&mut self, crest_factor_db: f32, rms_variance: f32) {
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

    pub fn compute_gain(
        &mut self,
        env_l: &VoiceEnvelope,
        env_r: &VoiceEnvelope,
        amount: f32,
        speech_confidence: f32,
        proximity_amount: f32,
        clarity_amount: f32,
    ) -> f32 {
        let amount = amount.clamp(0.0, 1.0);
        let speech_conf = speech_confidence.clamp(0.0, 1.0);

        // Freeze gain during silence
        if speech_conf < SILENCE_FREEZE_THRESHOLD {
            // Hold current state, don't update envelopes
            return self.out_gain_smooth;
        }

        // Smoothed bypass behavior: when "off", glide to unity without resetting state.
        if amount < COMPRESSOR_BYPASS_EPS {
            let rel = self.coeff(BYPASS_GAIN_RELEASE_MS);
            self.out_gain_smooth = rel * self.out_gain_smooth + (1.0 - rel) * 1.0;

            // Let meters decay naturally
            self.gain_reduction_envelope_db *= GAIN_REDUCTION_AVG_REL;
            self.peak_gain_reduction_db *= GAIN_REDUCTION_PEAK_REL;

            return self.out_gain_smooth;
        }

        // 1. Get shared envelopes
        let noise_floor = env_l.noise_floor.max(env_r.noise_floor);

        let rms_l = env_l.rms;
        let rms_r = env_r.rms;
        let rms_max = rms_l.max(rms_r);

        let peak_l = env_l.fast; // Using Fast envelope as proxy for Peak
        let peak_r = env_r.fast;
        let peak_max = peak_l.max(peak_r);

        // Hybrid detector
        let hybrid = (HYBRID_RMS_WEIGHT * rms_max + HYBRID_PEAK_WEIGHT * peak_max).max(DB_EPS);
        let hybrid_db = lin_to_db(hybrid);
        let peak_db = lin_to_db(peak_max.max(DB_EPS));

        // ---------------------------------------------------------------------
        // 4) Stage 1: Leveler (gentle, wide knee)
        // ---------------------------------------------------------------------
        let over1 = hybrid_db - LEVELER_TARGET_DB;

        // Crest adaptation: reduce ratio when crest is low
        let ratio_mult = if self.crest_factor_db < CREST_ADAPTATION_THRESHOLD_DB {
            LOW_CREST_RATIO_MULT
        } else {
            1.0
        };

        // Speech-confidence-weighted ratio: gentler during speech
        let speech_ratio_scale = 0.7 + 0.3 * speech_conf;

        let ratio1 = if over1 < LEVELER_RATIO_LOW_DB {
            1.0 + (LEVELER_RATIO_LOW - 1.0) * ratio_mult * speech_ratio_scale
        } else if over1 < LEVELER_RATIO_MID_DB {
            1.0 + (LEVELER_RATIO_MID - 1.0) * ratio_mult * speech_ratio_scale
        } else {
            1.0 + (LEVELER_RATIO_HIGH - 1.0) * ratio_mult * speech_ratio_scale
        };

        let red1_db = Self::soft_knee(over1, ratio1, LEVELER_KNEE_DB);

        // ---------------------------------------------------------------------
        // 5) Stage 2: Peak tamer (separate envelope, fast)
        // ---------------------------------------------------------------------
        let over2 = peak_db - PEAK_TAMER_THRESHOLD_DB;
        let red2_db = Self::soft_knee(over2, PEAK_TAMER_RATIO, PEAK_TAMER_KNEE_DB);

        // Separate peak tamer envelope with fast ballistics
        let peak_target_db = red2_db * amount;
        let peak_att = self.coeff(PEAK_TAMER_ATTACK_MS);
        let peak_rel = self.coeff(PEAK_TAMER_RELEASE_MS);

        if peak_target_db > self.peak_reduction_smooth_db {
            self.peak_reduction_smooth_db =
                peak_att * self.peak_reduction_smooth_db + (1.0 - peak_att) * peak_target_db;
        } else {
            self.peak_reduction_smooth_db =
                peak_rel * self.peak_reduction_smooth_db + (1.0 - peak_rel) * peak_target_db;
        }

        // Amount scaling for leveler: apply proportionally
        let leveler_target_db = red1_db * amount;

        // Adaptive release based on gain reduction amount
        // Fast release (400ms) for light compression, slow (900ms) for heavy
        let release_ms = if self.reduction_smooth_db > 6.0 {
            GAIN_RELEASE_MS_SLOW
        } else {
            GAIN_RELEASE_MS_FAST
        };

        // Smooth the leveler gain reduction
        let att = self.coeff(GAIN_ATTACK_MS);
        let rel = self.coeff(release_ms);

        if leveler_target_db > self.reduction_smooth_db {
            // Attack: compressor engaging
            self.reduction_smooth_db =
                att * self.reduction_smooth_db + (1.0 - att) * leveler_target_db;
        } else {
            // Release: compressor releasing
            self.reduction_smooth_db =
                rel * self.reduction_smooth_db + (1.0 - rel) * leveler_target_db;
        }

        // Total applied reduction is sum of both stages
        let applied_reduction_db = self.reduction_smooth_db + self.peak_reduction_smooth_db;

        // ---------------------------------------------------------------------
        // 6) Metering (applied reduction)
        // ---------------------------------------------------------------------
        self.gain_reduction_envelope_db = self.gain_reduction_envelope_db * GAIN_REDUCTION_AVG_REL
            + applied_reduction_db * (1.0 - GAIN_REDUCTION_AVG_REL);

        if applied_reduction_db > self.peak_gain_reduction_db {
            self.peak_gain_reduction_db = applied_reduction_db;
        } else {
            self.peak_gain_reduction_db *= GAIN_REDUCTION_PEAK_REL;
        }

        // ---------------------------------------------------------------------
        // 7) Makeup gain (VO-safe, gated; limited by tonal processing)
        // ---------------------------------------------------------------------
        // Limit makeup gain when proximity or clarity are active to prevent over-boosting
        let makeup_max = if proximity_amount > 0.5 || clarity_amount > 0.5 {
            2.5 // Conservative when tonal processing active
        } else {
            MAKEUP_MAX_DB // Full range when clean
        };

        let margin_db = hybrid_db - lin_to_db(noise_floor.max(DB_EPS));
        let makeup_db = if margin_db > MAKEUP_MARGIN_DB {
            (self.gain_reduction_envelope_db * MAKEUP_SCALE).min(makeup_max)
        } else {
            0.0
        };

        let gain = db_to_lin(-applied_reduction_db);
        let makeup = db_to_lin(makeup_db);

        // Smooth the output gain to avoid zippering when amount moves
        let rel = self.coeff(BYPASS_GAIN_RELEASE_MS);
        let target = (gain * makeup).clamp(0.0, 16.0);
        self.out_gain_smooth = rel * self.out_gain_smooth + (1.0 - rel) * target;

        self.out_gain_smooth
    }

    pub fn get_gain_reduction_db(&self) -> f32 {
        self.gain_reduction_envelope_db
    }

    pub fn reset(&mut self) {
        self.gain_reduction_envelope_db = 0.0;
        self.reduction_smooth_db = 0.0;
        self.peak_reduction_smooth_db = 0.0;
    }
}
