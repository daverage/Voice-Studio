//! Linked Compressor (Leveler)
//!
//! Stereo-linked compressor designed for voice level consistency, providing
//! smooth gain control to maintain consistent loudness while preserving
//! natural speech dynamics. Combines RMS and peak detection for balanced
//! compression suitable for spoken voice.
//!
//! # Perceptual Contract
//! - **No pumping**: Program-dependent release (PDR) and smooth transitions
//! - **Releases to unity in silence**: When speech_confidence drops, gain smoothly
//!   returns to unity rather than freezing at reduced gain
//! - **Speech confidence gates the detector**: Low confidence reduces compression
//!   activity rather than freezing output gain
//!
//! # Design Notes
//! - Stereo-linked RMS+Peak hybrid compression
//! - Speech-confidence-weighted detection (not hard freeze)
//! - Program-dependent release with hold for stability
//! - Smoothed peak control signal to reduce jitter
//! - Gated makeup gain that respects silence

use crate::dsp::envelope::VoiceEnvelope;
use crate::dsp::utils::{db_to_lin, lin_to_db, time_constant_coeff, DB_EPS};

// =============================================================================
// Data-Driven Calibration Constants
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

const COMPRESSOR_BYPASS_EPS: f32 = 0.01;

// Bypass/output smoothing
const BYPASS_GAIN_RELEASE_MS: f32 = 60.0;

// Gain reduction smoothing - Program Dependent Release (PDR)
const GAIN_ATTACK_MS: f32 = 40.0;
const GAIN_RELEASE_MS_FAST: f32 = 400.0;
const GAIN_RELEASE_MS_SLOW: f32 = 900.0;

// Peak tamer envelope
const PEAK_TAMER_ATTACK_MS: f32 = 5.0;
const PEAK_TAMER_RELEASE_MS: f32 = 120.0;

// Peak control signal smoothing (reduces jitter)
const PEAK_CTRL_DECAY_MS: f32 = 40.0;

// Silence release mode constants
const SILENCE_THRESHOLD: f32 = 0.25;
const SILENCE_RELEASE_MS: f32 = 350.0;
const SILENCE_PEAK_RELEASE_MS: f32 = 200.0;

// Speech confidence gating thresholds
const SC_GATE_ON: f32 = 0.2;
const SC_GATE_FULL: f32 = 0.6;

// Release hold for heavy compression (prevents pumping on attack events)
const RELEASE_HOLD_MS: f32 = 80.0;
const RELEASE_HOLD_THRESHOLD_DB: f32 = 3.0;

// Makeup gain constants
const MAKEUP_SCALE: f32 = 0.35;
const MAKEUP_MAX_DB: f32 = 4.0;
const MAKEUP_ATTACK_MS: f32 = 300.0;
const MAKEUP_RELEASE_MS: f32 = 800.0;
const MAKEUP_THRESHOLD_DB: f32 = 1.5;

// Hard clamps for safety
const MAX_LEVELER_REDUCTION_DB: f32 = 18.0;
const MAX_PEAK_REDUCTION_DB: f32 = 12.0;
const MAX_TOTAL_REDUCTION_DB: f32 = 24.0;

/// Stereo-linked VO compressor with automatic makeup gain.
///
/// ## Perceptual Behavior
/// - Releases smoothly to unity gain during silence (no "sticky" reduced gain)
/// - Speech confidence gates detection activity (low confidence = less compression)
/// - Program-dependent release prevents pumping on dynamic material
/// - Release hold prevents gain oscillation on attack transients
pub struct LinkedCompressor {
    sample_rate: f32,

    // Metering / makeup tracking
    gain_reduction_envelope_db: f32,
    peak_gain_reduction_db: f32,

    // Data-driven adaptation (from AudioProfile), smoothed
    crest_factor_db: f32,
    rms_variance: f32,
    adaptation_coeff: f32,

    // Smoothed output gain for bypass/amount transitions
    out_gain_smooth: f32,

    // Smoothed gain reduction (leveler stage)
    reduction_smooth_db: f32,

    // Separate peak tamer envelope
    peak_reduction_smooth_db: f32,

    // Smoothed peak control signal (reduces detector jitter)
    peak_ctrl: f32,

    // Release hold countdown (samples remaining)
    release_hold_samples: u32,

    // Smoothed makeup gain envelope
    makeup_smooth_db: f32,

    // Previous reduction for hold detection
    prev_reduction_db: f32,

    // Pump detection
    prev_out_gain: f32,
    gain_delta_db: f32,
    pump_detected: bool,
}

impl LinkedCompressor {
    pub fn new(sr: f32) -> Self {
        let adaptation_coeff = time_constant_coeff(100.0, sr);

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
            peak_ctrl: 0.0,
            release_hold_samples: 0,
            makeup_smooth_db: 0.0,
            prev_reduction_db: 0.0,
            prev_out_gain: 1.0,
            gain_delta_db: 0.0,
            pump_detected: false,
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

    /// Attempt to call this once at init or when sample rate changes.
    /// Returns samples for release hold based on sample rate.
    #[inline]
    fn hold_samples(&self) -> u32 {
        ((RELEASE_HOLD_MS * 0.001 * self.sample_rate) as u32).max(1)
    }

    /// Cheap smoothstep approximation: 3t² - 2t³
    /// Input should be clamped to [0, 1]
    #[inline]
    fn smoothstep_01(t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    /// Smoothstep with edge parameters
    #[inline]
    fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
        let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
        Self::smoothstep_01(t)
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

    /// Linear interpolation
    #[inline]
    fn lerp(a: f32, b: f32, t: f32) -> f32 {
        a + (b - a) * t
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

        // =====================================================================
        // (A) SILENCE RELEASE MODE
        // When speech_confidence is low, we DON'T freeze - we RELEASE toward unity.
        // This prevents "sticky" gain reduction when speech ends.
        // =====================================================================
        let in_silence = speech_conf < SILENCE_THRESHOLD;

        if in_silence {
            // Release reduction_smooth_db toward 0 (unity gain)
            let silence_rel = self.coeff(SILENCE_RELEASE_MS);
            self.reduction_smooth_db = silence_rel * self.reduction_smooth_db;

            // Release peak_reduction_smooth_db toward 0
            let silence_peak_rel = self.coeff(SILENCE_PEAK_RELEASE_MS);
            self.peak_reduction_smooth_db = silence_peak_rel * self.peak_reduction_smooth_db;

            // Release makeup toward 0 (no boost in silence)
            let makeup_rel = self.coeff(MAKEUP_RELEASE_MS);
            self.makeup_smooth_db = makeup_rel * self.makeup_smooth_db;

            // Glide output gain toward unity
            let out_rel = self.coeff(BYPASS_GAIN_RELEASE_MS);
            self.out_gain_smooth = out_rel * self.out_gain_smooth + (1.0 - out_rel) * 1.0;

            // Decay meters naturally
            self.gain_reduction_envelope_db *= GAIN_REDUCTION_AVG_REL;
            self.peak_gain_reduction_db *= GAIN_REDUCTION_PEAK_REL;

            // Decay peak control
            let peak_decay = self.coeff(PEAK_CTRL_DECAY_MS);
            self.peak_ctrl *= peak_decay;

            // Decrement hold if active
            self.release_hold_samples = self.release_hold_samples.saturating_sub(1);

            // Update pump detection
            let current_gain_db = lin_to_db(self.out_gain_smooth.max(DB_EPS));
            let prev_gain_db = lin_to_db(self.prev_out_gain.max(DB_EPS));
            self.gain_delta_db = (current_gain_db - prev_gain_db).abs();
            self.prev_out_gain = self.out_gain_smooth;
            self.pump_detected = false;

            return self.out_gain_smooth;
        }

        // =====================================================================
        // BYPASS MODE: amount near zero
        // =====================================================================
        if amount < COMPRESSOR_BYPASS_EPS {
            let rel = self.coeff(BYPASS_GAIN_RELEASE_MS);
            self.out_gain_smooth = rel * self.out_gain_smooth + (1.0 - rel) * 1.0;

            // Let internal states decay
            self.reduction_smooth_db *= self.coeff(SILENCE_RELEASE_MS);
            self.peak_reduction_smooth_db *= self.coeff(SILENCE_PEAK_RELEASE_MS);
            self.makeup_smooth_db *= self.coeff(MAKEUP_RELEASE_MS);

            self.gain_reduction_envelope_db *= GAIN_REDUCTION_AVG_REL;
            self.peak_gain_reduction_db *= GAIN_REDUCTION_PEAK_REL;

            // Update pump detection
            let current_gain_db = lin_to_db(self.out_gain_smooth.max(DB_EPS));
            let prev_gain_db = lin_to_db(self.prev_out_gain.max(DB_EPS));
            self.gain_delta_db = (current_gain_db - prev_gain_db).abs();
            self.prev_out_gain = self.out_gain_smooth;
            self.pump_detected = false;

            return self.out_gain_smooth;
        }

        // =====================================================================
        // (C) SPEECH CONFIDENCE GATING
        // Instead of hard freeze, we use a smooth detector weight.
        // Low confidence = less compression activity, not frozen output.
        // =====================================================================
        let detector_weight = Self::smoothstep(SC_GATE_ON, SC_GATE_FULL, speech_conf);

        // =====================================================================
        // (D) DETECTOR WITH SMOOTHED PEAK CONTROL
        // =====================================================================
        let _noise_floor = env_l.noise_floor.max(env_r.noise_floor);

        let rms_l = env_l.rms;
        let rms_r = env_r.rms;
        let rms_max = rms_l.max(rms_r);

        let peak_l = env_l.fast;
        let peak_r = env_r.fast;
        let peak_max = peak_l.max(peak_r);

        // Smooth peak control signal to reduce jitter
        let peak_decay = self.coeff(PEAK_CTRL_DECAY_MS);
        self.peak_ctrl = self.peak_ctrl.max(peak_max) * peak_decay + peak_max * (1.0 - peak_decay);

        // Hybrid detector using smoothed peak
        let hybrid =
            (HYBRID_RMS_WEIGHT * rms_max + HYBRID_PEAK_WEIGHT * self.peak_ctrl).max(DB_EPS);
        let hybrid_db = lin_to_db(hybrid);
        let peak_db = lin_to_db(self.peak_ctrl.max(DB_EPS));

        // =====================================================================
        // STAGE 1: LEVELER (gentle, wide knee)
        // =====================================================================
        let over1 = hybrid_db - LEVELER_TARGET_DB;

        // Crest adaptation: reduce ratio when crest is low (already compressed material)
        let ratio_mult = if self.crest_factor_db < CREST_ADAPTATION_THRESHOLD_DB {
            LOW_CREST_RATIO_MULT
        } else {
            1.0
        };

        // (C) Fixed speech ratio: high confidence = stable compression, not harsher
        // Low confidence reduces compression via detector_weight, not via ratio
        let ratio_scale = 0.85 + 0.15 * speech_conf;

        let ratio1 = if over1 < LEVELER_RATIO_LOW_DB {
            1.0 + (LEVELER_RATIO_LOW - 1.0) * ratio_mult * ratio_scale
        } else if over1 < LEVELER_RATIO_MID_DB {
            1.0 + (LEVELER_RATIO_MID - 1.0) * ratio_mult * ratio_scale
        } else {
            1.0 + (LEVELER_RATIO_HIGH - 1.0) * ratio_mult * ratio_scale
        };

        // Compute reduction and apply detector weight + clamp
        let red1_raw = Self::soft_knee(over1, ratio1, LEVELER_KNEE_DB);
        let red1_db = (red1_raw * detector_weight).min(MAX_LEVELER_REDUCTION_DB);

        // =====================================================================
        // STAGE 2: PEAK TAMER (fast, separate envelope)
        // =====================================================================
        let over2 = peak_db - PEAK_TAMER_THRESHOLD_DB;
        let red2_raw = Self::soft_knee(over2, PEAK_TAMER_RATIO, PEAK_TAMER_KNEE_DB);
        let red2_db = (red2_raw * detector_weight).min(MAX_PEAK_REDUCTION_DB);

        // Peak tamer envelope
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

        // =====================================================================
        // (B) PROGRAM-DEPENDENT RELEASE (PDR)
        // Smooth mapping from light to heavy compression release times.
        // Release hold prevents oscillation on attack events.
        // =====================================================================
        let leveler_target_db = red1_db * amount;

        // Check for attack event (significant increase in target reduction)
        let reduction_increase = leveler_target_db - self.prev_reduction_db;
        if reduction_increase > RELEASE_HOLD_THRESHOLD_DB {
            // Significant attack - reset hold counter
            self.release_hold_samples = self.hold_samples();
        }
        self.prev_reduction_db = leveler_target_db;

        // PDR: smooth mapping based on current reduction level
        // t=0 at 2dB reduction, t=1 at 12dB reduction
        let pdr_t = ((self.reduction_smooth_db - 2.0) / 10.0).clamp(0.0, 1.0);
        let release_ms = Self::lerp(GAIN_RELEASE_MS_FAST, GAIN_RELEASE_MS_SLOW, pdr_t);

        let att = self.coeff(GAIN_ATTACK_MS);
        let rel = self.coeff(release_ms);

        if leveler_target_db > self.reduction_smooth_db {
            // Attack: compressor engaging
            self.reduction_smooth_db =
                att * self.reduction_smooth_db + (1.0 - att) * leveler_target_db;
        } else if self.release_hold_samples > 0 {
            // In hold period: don't release yet (prevents pumping on transients)
            self.release_hold_samples -= 1;
            // Hold at current level (no change)
        } else {
            // Release: compressor releasing
            self.reduction_smooth_db =
                rel * self.reduction_smooth_db + (1.0 - rel) * leveler_target_db;
        }

        // (F) Total applied reduction with hard clamp
        let applied_reduction_db =
            (self.reduction_smooth_db + self.peak_reduction_smooth_db).min(MAX_TOTAL_REDUCTION_DB);

        // =====================================================================
        // METERING
        // =====================================================================
        self.gain_reduction_envelope_db = self.gain_reduction_envelope_db * GAIN_REDUCTION_AVG_REL
            + applied_reduction_db * (1.0 - GAIN_REDUCTION_AVG_REL);

        if applied_reduction_db > self.peak_gain_reduction_db {
            self.peak_gain_reduction_db = applied_reduction_db;
        } else {
            self.peak_gain_reduction_db *= GAIN_REDUCTION_PEAK_REL;
        }

        // =====================================================================
        // (E) SAFER MAKEUP GAIN
        // Gated by speech confidence, uses slower envelope, never boosts in silence.
        // =====================================================================
        let makeup_max = if proximity_amount > 0.5 || clarity_amount > 0.5 {
            2.5
        } else {
            MAKEUP_MAX_DB
        };

        // Makeup target: only apply when reduction exceeds threshold and speech is present
        let makeup_target_db = if self.gain_reduction_envelope_db > MAKEUP_THRESHOLD_DB {
            ((self.gain_reduction_envelope_db - MAKEUP_THRESHOLD_DB) * MAKEUP_SCALE).min(makeup_max)
                * detector_weight
        } else {
            0.0
        };

        // Smooth makeup with asymmetric attack/release
        let makeup_att = self.coeff(MAKEUP_ATTACK_MS);
        let makeup_rel = self.coeff(MAKEUP_RELEASE_MS);

        if makeup_target_db > self.makeup_smooth_db {
            self.makeup_smooth_db =
                makeup_att * self.makeup_smooth_db + (1.0 - makeup_att) * makeup_target_db;
        } else {
            self.makeup_smooth_db =
                makeup_rel * self.makeup_smooth_db + (1.0 - makeup_rel) * makeup_target_db;
        }

        // (F) Final gain computation with safety clamp
        let gain = db_to_lin(-applied_reduction_db);
        let makeup = db_to_lin(self.makeup_smooth_db);
        let target = (gain * makeup).clamp(0.0, 16.0);

        // Smooth output gain
        let out_rel = self.coeff(BYPASS_GAIN_RELEASE_MS);
        self.out_gain_smooth = out_rel * self.out_gain_smooth + (1.0 - out_rel) * target;

        // Pump detection
        let current_gain_db = lin_to_db(self.out_gain_smooth.max(DB_EPS));
        let prev_gain_db = lin_to_db(self.prev_out_gain.max(DB_EPS));
        self.gain_delta_db = (current_gain_db - prev_gain_db).abs();
        self.prev_out_gain = self.out_gain_smooth;

        const PUMP_THRESHOLD_DB: f32 = 0.5;
        self.pump_detected = self.gain_delta_db > PUMP_THRESHOLD_DB && speech_conf > 0.3;

        self.out_gain_smooth
    }

    pub fn get_gain_reduction_db(&self) -> f32 {
        self.gain_reduction_envelope_db
    }

    /// Get the rate of gain change (dB per sample block)
    pub fn get_gain_delta_db(&self) -> f32 {
        self.gain_delta_db
    }

    /// Check if pump was detected this cycle
    pub fn is_pump_detected(&self) -> bool {
        self.pump_detected
    }

    /// (G) Reset ALL smoothing states to prevent sticky behavior
    pub fn reset(&mut self) {
        self.gain_reduction_envelope_db = 0.0;
        self.peak_gain_reduction_db = 0.0;
        self.out_gain_smooth = 1.0;
        self.reduction_smooth_db = 0.0;
        self.peak_reduction_smooth_db = 0.0;
        self.peak_ctrl = 0.0;
        self.release_hold_samples = 0;
        self.makeup_smooth_db = 0.0;
        self.prev_reduction_db = 0.0;
        self.prev_out_gain = 1.0;
        self.gain_delta_db = 0.0;
        self.pump_detected = false;
    }
}
