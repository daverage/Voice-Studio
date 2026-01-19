//! Early Reflection Suppressor (Micro-Deverb)
//!
//! Reduces perceived distance by suppressing short-lag reflections (3–18ms)
//! without killing tone. Targets early reflections that make recordings
//! sound distant or boxy, while preserving the direct signal.
//!
//! # RESPONSIBILITY BOUNDARY: Early Reflections Only
//!
//! This module handles SHORT-LAG reflections (approximately 0–20ms):
//! - Desk/table reflections (~3ms / ~1.0m)
//! - Nearby side walls (~7ms / ~2.4m)
//! - Floor/ceiling (~12ms / ~4.1m)
//! - Opposite wall in medium rooms (~18ms / ~6.2m)
//!
//! This module DOES NOT handle:
//! - Late reverb tail (>50ms) - owned by `StreamingDeverber`
//! - Diffuse room decay - owned by `StreamingDeverber`
//! - Distinct flutter echoes - not modeled
//!
//! ## Avoiding Double-Reaction
//!
//! Both this module and `StreamingDeverber` respond to "distance" cues, but they
//! target different time regions. Neither module should attempt to solve both problems:
//! - Early reflections cause coloration/boxiness (this module)
//! - Late reflections cause diffuse "room sound" (deverber)
//!
//! The signal chain order (early reflection → denoise → deverb) ensures each
//! processor sees the appropriate input without double-processing.
//!
//! # Perceptual Contract
//! - **Target Source**: Speech in small/boxy rooms.
//! - **Intended Effect**: Suppress short-lag reflections (3-18ms) that cause coloration.
//! - **Failure Modes**:
//!   - Comb filtering artifacts if delay estimation is wrong (rare, fixed delays used).
//!   - "Phasey" sound if suppression is too strong.
//! - **Will Not Do**:
//!   - Reduce long reverb tails (handled by `StreamingDeverber`).
//!
//! # Lifecycle
//! - **Active**: Normal operation.
//! - **Bypassed**: Passes audio through.
//!
//! ## Audio Thread Safety
//! - All delay lines pre-allocated in `new()`
//! - No allocations during `process()`

use super::speech_confidence::SpeechSidechain;
use super::utils::{time_constant_coeff, DB_EPS};

// =============================================================================
// Constants
// =============================================================================

/// Maximum delay time in milliseconds (for buffer sizing)
const MAX_DELAY_MS: f32 = 25.0;

/// Maximum sample rate supported
const MAX_SAMPLE_RATE: f32 = 96_000.0;

/// Maximum delay buffer size
const MAX_DELAY_SAMPLES: usize = ((MAX_DELAY_MS * 0.001 * MAX_SAMPLE_RATE) as usize) + 1;

/// Delay tap times in milliseconds (early reflection region).
/// Choices are based on typical physical room distances:
/// - 3.0 ms (~1.0m): Desk/table reflections
/// - 7.0 ms (~2.4m): Nearby side walls
/// - 12.0 ms (~4.1m): Floor/ceiling or further walls
/// - 18.0 ms (~6.2m): Opposite wall in medium rooms
const TAP_DELAYS_MS: [f32; 4] = [3.0, 7.0, 12.0, 18.0];

/// Tap weights for reflection estimate (sum ≈ 1.0)
const TAP_WEIGHTS: [f32; 4] = [0.35, 0.30, 0.20, 0.15];

/// Maximum suppression amount (35%)
const MAX_SUPPRESSION: f32 = 0.35;

/// Attack time for suppression envelope (ms)
const ATTACK_MS: f32 = 60.0;

/// Release time for suppression envelope (ms)
const RELEASE_MS: f32 = 250.0;

/// Correlation threshold for reflection detection
const CORRELATION_THRESHOLD: f32 = 0.15;

/// Minimum speech confidence to apply suppression
const MIN_SPEECH_CONF: f32 = 0.2;

// =============================================================================
// Early Reflection Suppressor
// =============================================================================

/// Per-channel early reflection suppressor
pub struct EarlyReflectionSuppressor {
    sample_rate: f32,

    // Delay line for taps
    delay_buffer: [f32; MAX_DELAY_SAMPLES],
    write_pos: usize,

    // Tap positions (samples)
    tap_positions: [usize; 4],

    // Correlation tracking
    correlation_acc: f32,
    input_energy_acc: f32,
    reflection_energy_acc: f32,
    frame_samples: usize,

    // Suppression envelope
    suppression_env: f32,
    attack_coeff: f32,
    release_coeff: f32,

    // Smoothed suppression amount
    current_suppression: f32,
}

impl EarlyReflectionSuppressor {
    pub fn new(sample_rate: f32) -> Self {
        debug_assert!(
            sample_rate <= MAX_SAMPLE_RATE,
            "EarlyReflectionSuppressor: unsupported sample rate"
        );

        let mut tap_positions = [0usize; 4];
        for (i, &delay_ms) in TAP_DELAYS_MS.iter().enumerate() {
            tap_positions[i] =
                ((delay_ms * 0.001 * sample_rate) as usize).min(MAX_DELAY_SAMPLES - 1);
        }

        Self {
            sample_rate,
            delay_buffer: [0.0; MAX_DELAY_SAMPLES],
            write_pos: 0,
            tap_positions,
            correlation_acc: 0.0,
            input_energy_acc: 0.0,
            reflection_energy_acc: 0.0,
            frame_samples: 0,
            suppression_env: 0.0,
            attack_coeff: time_constant_coeff(ATTACK_MS, sample_rate),
            release_coeff: time_constant_coeff(RELEASE_MS, sample_rate),
            current_suppression: 0.0,
        }
    }

    /// Process a single sample with early reflection suppression
    ///
    /// * `input` - Input sample
    /// * `amount` - Suppression control (0.0–1.0)
    /// * `sidechain` - Speech confidence sidechain
    #[inline]
    pub fn process(&mut self, input: f32, amount: f32, sidechain: &SpeechSidechain) -> f32 {
        // Write input to delay line
        self.delay_buffer[self.write_pos] = input;

        // Estimate early reflections from taps
        let mut reflection_estimate = 0.0f32;
        for (i, &tap_pos) in self.tap_positions.iter().enumerate() {
            let read_pos = if self.write_pos >= tap_pos {
                self.write_pos - tap_pos
            } else {
                MAX_DELAY_SAMPLES - (tap_pos - self.write_pos)
            };

            // Alternate sign to reduce combing
            let tap = if (i & 1) == 0 {
                self.delay_buffer[read_pos]
            } else {
                -self.delay_buffer[read_pos]
            };

            reflection_estimate += tap * TAP_WEIGHTS[i];
        }

        // Correlation tracking
        self.correlation_acc += input * reflection_estimate;
        self.input_energy_acc += input * input;
        self.reflection_energy_acc += reflection_estimate * reflection_estimate;
        self.frame_samples += 1;

        // Periodic envelope update (~5ms)
        let frame_size = ((0.005 * self.sample_rate) as usize).max(8);
        if self.frame_samples >= frame_size {
            self.update_suppression_envelope(amount, sidechain);
            self.correlation_acc = 0.0;
            self.input_energy_acc = 0.0;
            self.reflection_energy_acc = 0.0;
            self.frame_samples = 0;
        }

        // Advance delay line
        self.write_pos = (self.write_pos + 1) % MAX_DELAY_SAMPLES;

        // Apply suppression with safety clamp
        let cancelled = reflection_estimate * self.current_suppression;
        let cancelled = cancelled.clamp(-input.abs(), input.abs());

        input - cancelled
    }

    /// Update suppression envelope based on normalized correlation and speech confidence
    fn update_suppression_envelope(&mut self, amount: f32, sidechain: &SpeechSidechain) {
        if self.frame_samples == 0 {
            return;
        }

        // Proper normalized correlation
        let denom = (self.input_energy_acc * self.reflection_energy_acc).sqrt();
        let norm_corr = if denom > DB_EPS {
            (self.correlation_acc / denom).clamp(-1.0, 1.0)
        } else {
            0.0
        };

        // Speech confidence gate
        let speech_gate = if sidechain.speech_conf > MIN_SPEECH_CONF {
            ((sidechain.speech_conf - MIN_SPEECH_CONF) / (1.0 - MIN_SPEECH_CONF)).clamp(0.0, 1.0)
        } else {
            0.0
        };

        // Correlation gate
        let correlation_factor = if norm_corr > CORRELATION_THRESHOLD {
            ((norm_corr - CORRELATION_THRESHOLD) / (1.0 - CORRELATION_THRESHOLD)).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let target = MAX_SUPPRESSION * speech_gate * correlation_factor * amount;

        // Attack / release smoothing
        if target > self.suppression_env {
            self.suppression_env =
                self.attack_coeff * self.suppression_env + (1.0 - self.attack_coeff) * target;
        } else {
            self.suppression_env =
                self.release_coeff * self.suppression_env + (1.0 - self.release_coeff) * target;
        }

        self.current_suppression = self.suppression_env.clamp(0.0, MAX_SUPPRESSION);
    }

    /// Reset all internal state
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.delay_buffer = [0.0; MAX_DELAY_SAMPLES];
        self.write_pos = 0;
        self.correlation_acc = 0.0;
        self.input_energy_acc = 0.0;
        self.reflection_energy_acc = 0.0;
        self.frame_samples = 0;
        self.suppression_env = 0.0;
        self.current_suppression = 0.0;
    }

    /// Current suppression amount (for metering)
    #[inline]
    pub fn get_suppression(&self) -> f32 {
        self.current_suppression
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_creation() {
        let suppressor = EarlyReflectionSuppressor::new(48_000.0);
        assert!(suppressor.tap_positions[0] > 0);
        assert!(suppressor.tap_positions[3] > suppressor.tap_positions[0]);
    }

    #[test]
    fn test_silence_passthrough() {
        let mut suppressor = EarlyReflectionSuppressor::new(48_000.0);
        let sidechain = SpeechSidechain::default();

        for _ in 0..1_000 {
            let out = suppressor.process(0.0, 1.0, &sidechain);
            assert!(out.abs() < 1e-10);
        }
    }

    #[test]
    fn test_no_suppression_at_zero_amount() {
        let mut suppressor = EarlyReflectionSuppressor::new(48_000.0);
        let sidechain = SpeechSidechain {
            speech_conf: 0.8,
            noise_floor_db: -60.0,
        };

        // Prime delay line
        for i in 0..1_000 {
            let input = (i as f32 * 0.01).sin();
            suppressor.process(input, 0.0, &sidechain);
        }

        for i in 0..100 {
            let input = (i as f32 * 0.01).sin();
            let out = suppressor.process(input, 0.0, &sidechain);
            assert!((out - input).abs() < 1e-6);
        }
    }
}
