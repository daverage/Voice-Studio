//! Unified Envelope Architecture for Voice DSP
//!
//! Provides the single canonical source of truth for time-domain energy tracking
//! used throughout the VxCleaner processing chain. Replaces disparate envelope
//! followers scattered across de-essers, compressors, and expanders with a
//! centralized, efficient implementation.
//!
//! # Purpose
//! Centralizes envelope tracking to ensure consistent behavior across all
//! dynamics processing modules while reducing computational overhead.
//!
//! # Design Notes
//! - Single tracker running upstream, once per sample
//! - Multiple views exposed: Fast, Slow, RMS, and SNR-based Confidence
//! - Optimized for real-time voice processing applications
//! - **No Allocations**: Sample-accurate, deterministic, stack-only.
//!
//! # Time Constants
//! Constants are chosen based on psychoacoustic voice characteristics:
//! - **Fast**: 1ms/60ms. Tracks transients, plosives, and sibilance.
//! - **Slow**: 20ms/300ms. Tracks syllabic and phrase-level loudness.
//! - **RMS**: 20ms window. True energy integration.
//! - **Noise**: asymmetric slew. Tracks the noise floor for SNR calculation.

use crate::dsp::utils::{lin_to_db, time_constant_coeff, DB_EPS};

// =============================================================================
// Time Constants
// =============================================================================

/// Fast Attack (1ms): Preserves transients for limiters and de-essers.
const FAST_ATTACK_MS: f32 = 1.0;
/// Fast Release (60ms): Matches typical sibilance/plosive duration.
const FAST_RELEASE_MS: f32 = 60.0;

/// Slow Attack (20ms): Smooths over phonemes, tracking syllables.
const SLOW_ATTACK_MS: f32 = 20.0;
/// Slow Release (300ms): Bridges gaps between words (phrase level).
const SLOW_RELEASE_MS: f32 = 300.0;

/// RMS Window (20ms): Standard integration window for voice energy.
const RMS_WINDOW_MS: f32 = 20.0;

/// Noise Floor Attack (5s): Very slow rise to avoid tracking speech.
const NOISE_ATTACK_MS: f32 = 5000.0;
/// Noise Floor Release (100ms): Fast drop to catch silence.
const NOISE_RELEASE_MS: f32 = 100.0;

/// Confidence SNR Threshold (12dB): Level above noise floor to be "confident".
const CONFIDENCE_SNR_THRESHOLD_DB: f32 = 12.0;

// =============================================================================
// Data Structures
// =============================================================================

/// Canonical envelope state for a single sample.
/// Passed to downstream DSP modules (consumers).
#[derive(Copy, Clone, Debug)]
pub struct VoiceEnvelope {
    /// Fast envelope (transients, sibilance).
    /// Use for: Limiting, De-essing, Fast expansion.
    pub fast: f32,

    /// Slow envelope (syllables, phrases).
    /// Use for: Leveling, Gating, Macro loudness.
    pub slow: f32,

    /// RMS energy (true power).
    /// Use for: Metering, Threshold detection.
    pub rms: f32,

    /// Signal confidence (0.0 - 1.0).
    /// Heuristic based on SNR (Signal-to-Noise Ratio).
    /// Use for: UI meters, Macro consistency, Sidechain weighting.
    #[allow(dead_code)]
    pub confidence: f32,

    /// Estimated noise floor level.
    pub noise_floor: f32,
}

impl Default for VoiceEnvelope {
    fn default() -> Self {
        Self {
            fast: 0.0,
            slow: 0.0,
            rms: 0.0,
            confidence: 0.0,
            noise_floor: 1e-4,
        }
    }
}

/// The engine that calculates envelopes.
/// Instantiated once per channel in the processor.
pub struct VoiceEnvelopeTracker {
    // Coefficients (calculated on init/prepare)
    fast_att_coeff: f32,
    fast_rel_coeff: f32,
    slow_att_coeff: f32,
    slow_rel_coeff: f32,
    rms_coeff: f32,
    noise_att_coeff: f32,
    noise_rel_coeff: f32,

    // State
    fast_state: f32,
    slow_state: f32,
    rms_sq_state: f32,
    noise_state: f32,
}

impl VoiceEnvelopeTracker {
    /// Create a new tracker for a given sample rate.
    pub fn new(sample_rate: f32) -> Self {
        let mut tracker = Self {
            fast_att_coeff: 0.0,
            fast_rel_coeff: 0.0,
            slow_att_coeff: 0.0,
            slow_rel_coeff: 0.0,
            rms_coeff: 0.0,
            noise_att_coeff: 0.0,
            noise_rel_coeff: 0.0,
            fast_state: 0.0,
            slow_state: 0.0,
            rms_sq_state: 0.0,
            noise_state: 1e-4,
        };
        tracker.prepare(sample_rate);
        tracker
    }

    /// Update coefficients for a new sample rate.
    pub fn prepare(&mut self, sample_rate: f32) {
        // Fast follower
        self.fast_att_coeff = time_constant_coeff(FAST_ATTACK_MS, sample_rate);
        self.fast_rel_coeff = time_constant_coeff(FAST_RELEASE_MS, sample_rate);

        // Slow follower
        self.slow_att_coeff = time_constant_coeff(SLOW_ATTACK_MS, sample_rate);
        self.slow_rel_coeff = time_constant_coeff(SLOW_RELEASE_MS, sample_rate);

        // RMS (Leaky Integrator)
        // Tau = Window / 2 roughly approximates a windowed average in 1-pole
        let rms_samples = (RMS_WINDOW_MS * 0.001 * sample_rate).max(1.0);
        self.rms_coeff = (-1.0 / rms_samples).exp();

        // Noise floor (Asymmetric)
        self.noise_att_coeff = time_constant_coeff(NOISE_ATTACK_MS, sample_rate);
        self.noise_rel_coeff = time_constant_coeff(NOISE_RELEASE_MS, sample_rate);
    }

    /// Process a single sample and return the envelope snapshot.
    /// This should be called *before* any processing that alters dynamics.
    #[inline]
    pub fn process_sample(&mut self, input: f32) -> VoiceEnvelope {
        let x_abs = input.abs();
        let x_sq = x_abs * x_abs;

        // 1. Fast Envelope (Attack/Release)
        // Tracks peaks and transients
        if x_abs > self.fast_state {
            self.fast_state =
                self.fast_att_coeff * self.fast_state + (1.0 - self.fast_att_coeff) * x_abs;
        } else {
            self.fast_state =
                self.fast_rel_coeff * self.fast_state + (1.0 - self.fast_rel_coeff) * x_abs;
        }

        // 2. Slow Envelope (Attack/Release)
        // Tracks syllables and phrases
        if x_abs > self.slow_state {
            self.slow_state =
                self.slow_att_coeff * self.slow_state + (1.0 - self.slow_att_coeff) * x_abs;
        } else {
            self.slow_state =
                self.slow_rel_coeff * self.slow_state + (1.0 - self.slow_rel_coeff) * x_abs;
        }

        // 3. RMS (Energy Integration)
        self.rms_sq_state = self.rms_coeff * self.rms_sq_state + (1.0 - self.rms_coeff) * x_sq;
        // Protect against negative zero / NaN
        if self.rms_sq_state < 0.0 {
            self.rms_sq_state = 0.0;
        }
        let rms_val = self.rms_sq_state.sqrt();

        // 4. Noise Floor Tracking
        // Logic: Drop fast on silence (release), Rise slow on signal (attack)
        // This estimates the constant bottom of the signal.
        if x_abs < self.noise_state {
            // Signal is lower than noise est -> Drop fast (it's actually silence)
            self.noise_state =
                self.noise_rel_coeff * self.noise_state + (1.0 - self.noise_rel_coeff) * x_abs;
        } else {
            // Signal is higher -> Rise very slowly (ignore speech)
            self.noise_state =
                self.noise_att_coeff * self.noise_state + (1.0 - self.noise_att_coeff) * x_abs;
        }

        // 5. Confidence Heuristic
        // Based on SNR: How far is Slow Envelope above Noise Floor?
        let signal_db = lin_to_db(self.slow_state.max(DB_EPS));
        let noise_db = lin_to_db(self.noise_state.max(DB_EPS));
        let snr = signal_db - noise_db;

        // Map SNR to 0.0 - 1.0 confidence
        // < 0dB: 0.0
        // > 12dB: 1.0
        let confidence = (snr / CONFIDENCE_SNR_THRESHOLD_DB).clamp(0.0, 1.0);

        VoiceEnvelope {
            fast: self.fast_state,
            slow: self.slow_state,
            rms: rms_val,
            confidence,
            noise_floor: self.noise_state,
        }
    }

    pub fn reset(&mut self) {
        self.fast_state = 0.0;
        self.slow_state = 0.0;
        self.rms_sq_state = 0.0;
        self.noise_state = 1e-4;
    }
}

// =============================================================================
// Integration Example / Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_envelope_behavior() {
        let mut tracker = VoiceEnvelopeTracker::new(48000.0);

        // 1. Silence
        let env = tracker.process_sample(0.0);
        assert!(env.fast < 1e-6);
        assert!(env.slow < 1e-6);

        // 2. Impulse (Transient)
        let env_impulse = tracker.process_sample(1.0);
        // Fast should react more than slow
        assert!(env_impulse.fast > env_impulse.slow);

        // 3. Steady State
        for _ in 0..1000 {
            tracker.process_sample(0.5);
        }
        let env_steady = tracker.process_sample(0.5);

        // Both should converge near 0.5
        assert!((env_steady.fast - 0.5).abs() < 0.05);
        assert!((env_steady.slow - 0.5).abs() < 0.05);
        assert!(env_steady.confidence > 0.9); // High SNR
    }
}
