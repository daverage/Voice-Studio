//! Dynamic Low-Mid Congestion Controller & Detector
//!
//! Reduces low-mid congestion (120–380 Hz) during voiced speech to restore
//! perceived clarity after tonal shaping. Uses voiced speech detection to
//! apply targeted reduction only when needed, preserving natural voice character.
//!
//! # Purpose
//! Address chesty, congested, or "muddy" speech with low-mid buildup by
//! dynamically reducing low-mid frequencies during voiced speech segments.
//! Maintains subtractive-only processing to complement upstream processing.
//!
//! # Design Notes
//! - Targets low-mid congestion range (120-380 Hz)
//! - Uses voiced speech detection for intelligent application
//! - Remains subtractive only (no high-frequency boosting)
//! - Works in conjunction with pink reference bias for balanced tonal shaping

use crate::dsp::utils::{
    aggressive_tail, speech_weighted, time_constant_coeff, update_env_sq, DB_EPS,
};
use crate::dsp::Biquad;

// Constants for clarity detector tuning

// Vocal body range filters
// High-pass cutoff (Hz) - bottom of vocal body range.
// Increasing: tighter focus; decreasing: includes more low mids.
const DETECTOR_HPF_HZ: f32 = 120.0;
// Low-pass cutoff (Hz) - top of vocal body range.
// Increasing: includes more upper mids; decreasing: tighter focus.
const DETECTOR_LPF_HZ: f32 = 380.0;
// Filter Q (Butterworth-ish).
// Increasing: narrower band; decreasing: wider band.
const DETECTOR_Q: f32 = 0.7;
// RMS envelope attack time (ms).
// Increasing: slower tracking; decreasing: faster tracking.
const RMS_ATTACK_MS: f32 = 30.0;
// RMS envelope release time (ms).
// Increasing: smoother decay; decreasing: faster decay.
const RMS_RELEASE_MS: f32 = 250.0;
// ZCR smoothing decay factor (per sample).
// Increasing: smoother ZCR; decreasing: more reactive.
const ZC_DECAY: f32 = 0.995;
// ZCR smoothing attack factor (per sample).
// Increasing: faster ZCR update; decreasing: slower update.
const ZC_ATTACK: f32 = 0.005;
// ZCR to voicing scale factor.
// Increasing: ZCR has more influence; decreasing: less influence.
const ZC_VOICED_SCALE: f32 = 6.0;

// Constants for clarity shaper tuning

// Low shelf filter frequency (Hz).
// Increasing: higher crossover; decreasing: lower crossover.
const SHAPER_FREQ_HZ: f32 = 250.0;
// Shaper filter Q.
// Increasing: narrower transition; decreasing: wider transition.
const SHAPER_Q: f32 = 0.7;
// Maximum low-mid cut at full clarity (dB).
// Increasing: stronger cut; decreasing: gentler cut.
const MAX_CUT_DB: f32 = 64.0;
// Coefficient smoothing factor.
// Increasing: faster transitions; decreasing: smoother transitions.
const SMOOTH_COEFF: f32 = 0.02;
// Coefficient update threshold (dB).
// Increasing: fewer updates; decreasing: more frequent updates.
const COEFF_UPDATE_THRESHOLD: f32 = 0.05;
// Bypass threshold for clarity amount.
// Increasing: easier to bypass; decreasing: more likely to process.
const CLARITY_BYPASS_EPS: f32 = 0.001;

/// Shared stereo-linked detector for body energy detection
pub struct ClarityDetector {
    hp: Biquad,
    lp: Biquad,

    env_sq: f32,

    prev_sign: f32,
    zc_energy: f32,

    sample_rate: f32,
}

impl ClarityDetector {
    pub fn new(sample_rate: f32) -> Self {
        let mut hp = Biquad::new();
        hp.update_hpf(DETECTOR_HPF_HZ, DETECTOR_Q, sample_rate);

        let mut lp = Biquad::new();
        lp.update_lpf(DETECTOR_LPF_HZ, DETECTOR_Q, sample_rate);

        Self {
            hp,
            lp,
            env_sq: 0.0,
            prev_sign: 0.0,
            zc_energy: 0.0,
            sample_rate,
        }
    }

    #[inline]
    fn coeff(&self, ms: f32) -> f32 {
        time_constant_coeff(ms, self.sample_rate)
    }

    /// Returns body energy drive signal [0..1]
    pub fn analyze(&mut self, l: f32, r: f32) -> f32 {
        let x = l.abs().max(r.abs());

        // Band-limit to vocal body range
        let band = self.lp.process(self.hp.process(x));

        // RMS envelope (vowel aware)
        let atk = self.coeff(RMS_ATTACK_MS);
        let rel = self.coeff(RMS_RELEASE_MS);
        let sq = band * band;

        self.env_sq = update_env_sq(self.env_sq, sq, atk, rel);

        let rms = self.env_sq.sqrt().max(DB_EPS);

        // Voicing confidence (linked ZCR)
        let sign = x.signum();
        let zc = if sign != self.prev_sign { 1.0 } else { 0.0 };
        self.prev_sign = sign;

        self.zc_energy = self.zc_energy * ZC_DECAY + zc * ZC_ATTACK;
        let voiced = (1.0 - self.zc_energy * ZC_VOICED_SCALE).clamp(0.0, 1.0);

        // Final body energy drive
        (rms * voiced).clamp(0.0, 1.0)
    }
}

/// Per-channel Clarity shaper (dynamic low-mid cleanup)
///
/// ## Metric Ownership (SHARED with Proximity)
/// This module OWNS and is responsible for:
/// - **Presence ratio**: Contributes to target (≤ 0.01) via low-mid cleanup
/// - **Air ratio**: Contributes to target (≤ 0.005) via reduced low-mid masking
///
/// This module must NOT attempt to modify:
/// - RMS, crest factor, RMS variance (owned by Leveler)
/// - Noise floor, SNR (owned by Denoiser)
/// - Early/Late ratio, decay slope (owned by De-reverb)
/// - HF variance (read-only guardrail metric)
///
/// ## Behavioral Rules
/// - Only active if presence < target
/// - Strength scales with SNR
/// - Hard caps: Whisper → 25% max, Noisy → 40% max
pub struct Clarity {
    shaper: Biquad,
    last_cut_db: f32,
    sample_rate: f32,
}

impl Clarity {
    pub fn new(sample_rate: f32) -> Self {
        let mut shaper = Biquad::new();
        shaper.update_low_shelf(SHAPER_FREQ_HZ, SHAPER_Q, 0.0, sample_rate);

        Self {
            shaper,
            last_cut_db: 0.0,
            sample_rate,
        }
    }

    /// clarity: user slider (0..1)
    /// speech_confidence: speech confidence from detector (0..1)
    /// drive: shared detector output (0..1)
    pub fn process(&mut self, input: f32, clarity: f32, speech_confidence: f32, drive: f32) -> f32 {
        // Clarity = reduce low-mid mud (subtractive only)
        // Uses aggressive_tail curve to preserve usability until ~70%

        if clarity <= CLARITY_BYPASS_EPS {
            return input;
        }

        // Apply aggressive tail curve to slider
        let x = aggressive_tail(clarity);

        // Speech-weighted max cut (reduces max during voiced speech)
        let max_cut_db = speech_weighted(MAX_CUT_DB, speech_confidence);

        // Calculate effective cut
        let mut clarity_cut_db = x * max_cut_db * drive;

        // Hard guardrail: never exceed 48dB
        clarity_cut_db = clarity_cut_db.min(48.0);

        // Additional speech protection: if highly confident speech, limit to 30dB
        if speech_confidence > 0.6 {
            clarity_cut_db = clarity_cut_db.min(30.0);
        }

        let target_cut_db = -clarity_cut_db;

        // Smooth coefficient changes
        let cut_db = self.last_cut_db + SMOOTH_COEFF * (target_cut_db - self.last_cut_db);
        self.last_cut_db = cut_db;

        if (cut_db - target_cut_db).abs() > COEFF_UPDATE_THRESHOLD {
            self.shaper
                .update_low_shelf(SHAPER_FREQ_HZ, SHAPER_Q, cut_db, self.sample_rate);
        }

        // This module must remain subtractive only.
        // Presence and air are handled upstream by Pink Reference Bias.
        self.shaper.process(input)
    }
}
