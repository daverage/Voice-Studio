//! Speech Confidence Estimator (Sidechain Only)
//!
//! # SHARED SPEECH ACTIVITY ENVELOPE (Canonical Source)
//!
//! This module provides THE single shared speech activity envelope for the entire plugin.
//! All modules requiring speech/silence awareness MUST read from this source rather than
//! implementing their own detectors.
//!
//! ## Envelope Contract
//!
//! - **Source signal**: Pre-processing input (before denoise, de-verb, expander, leveler)
//! - **Domain**: Linear amplitude (RMS-based), output 0.0–1.0
//! - **Lifecycle**: Computed once per sample, read-only for all consumers
//! - **Decay**: Moves toward zero during silence (controlled release)
//! - **Bounded**: Clamped to prevent numerical drift over 8+ hour sessions
//!
//! ## Authorized Consumers
//!
//! The following modules MAY read `SpeechSidechain`:
//! - `EarlyReflectionSuppressor` - gates suppression during non-speech
//! - `SpeechExpander` - weights expansion by inverse speech confidence
//! - `StreamingDeverber` - MAY use for gating (currently does not)
//!
//! ## Modules That Must NOT Use This Envelope
//!
//! - `LinkedCompressor` (Leveler) - MUST retain independent energy detection
//!   (see `compressor.rs` for dynamics ownership boundary)
//! - `StereoDenoiserDetector` - uses spectral-domain speech probability
//!   (different domain, changing would affect sound)
//!
//! ## Audio Thread Safety
//! - All buffers are pre-allocated in `new()`
//! - No allocations during `process()`
//! - Uses frame-based analysis (20 ms frame, 10 ms hop)

use super::biquad::Biquad;
use super::utils::{time_constant_coeff, DB_EPS};

// =============================================================================
// Constants
// =============================================================================

/// Frame size in milliseconds for analysis
const FRAME_MS: f32 = 20.0;

/// Hop size in milliseconds (overlap)
const HOP_MS: f32 = 10.0;

/// Maximum frame size in samples (for 96kHz)
const MAX_FRAME_SAMPLES: usize = 2048;

/// Speech band lower frequency (Hz)
const SPEECH_BAND_LOW: f32 = 250.0;

/// Speech band upper frequency (Hz)
const SPEECH_BAND_HIGH: f32 = 4000.0;

/// Attack time for confidence smoothing (ms)
const CONFIDENCE_ATTACK_MS: f32 = 15.0;

/// Release time for confidence smoothing (ms)
const CONFIDENCE_RELEASE_MS: f32 = 120.0;

/// Hang time to prevent flicker (ms)
const HANG_TIME_MS: f32 = 80.0;

/// Minimum RMS threshold for speech detection (linear)
const MIN_RMS_THRESHOLD: f32 = 0.001;

/// Noise floor tracking attack (very slow)
const NOISE_FLOOR_ATTACK_MS: f32 = 500.0;

/// Noise floor tracking release
const NOISE_FLOOR_RELEASE_MS: f32 = 50.0;

/// Spectral flatness threshold for speech vs noise
const FLATNESS_SPEECH_THRESHOLD: f32 = 0.4;

/// Minimum speech band ratio for confidence
const MIN_SPEECH_RATIO: f32 = 0.3;

// =============================================================================
// Output Structure
// =============================================================================

/// Read-only sidechain output available to other DSP modules
#[derive(Clone, Copy, Debug, Default)]
pub struct SpeechSidechain {
    /// Speech confidence level (0.0 = noise/silence, 1.0 = confident speech)
    pub speech_conf: f32,
    /// Estimated noise floor in dB
    pub noise_floor_db: f32,
}

// =============================================================================
// Main Estimator
// =============================================================================

/// Speech confidence estimator for automation and gating
pub struct SpeechConfidenceEstimator {
    #[allow(dead_code)] // Stored for potential sample rate change support
    sample_rate: f32,
    frame_size: usize,
    hop_size: usize,

    // Input buffer for frame accumulation
    input_buffer: [f32; MAX_FRAME_SAMPLES],
    buffer_pos: usize,
    samples_since_hop: usize,

    // Band-pass filters for speech band extraction
    bp_low_l: Biquad,
    bp_low_r: Biquad,
    bp_high_l: Biquad,
    bp_high_r: Biquad,

    // Feature accumulators (reset per frame)
    frame_energy_total: f32,
    frame_energy_speech: f32,
    frame_sample_count: usize,

    // Previous frame data for flux calculation
    prev_frame_energy: f32,

    // Smoothed outputs
    raw_confidence: f32,
    smoothed_confidence: f32,
    noise_floor_sq: f32,

    // Hang logic
    hang_counter: usize,
    hang_samples: usize,

    // Smoothing coefficients
    attack_coeff: f32,
    release_coeff: f32,
    noise_attack_coeff: f32,
    noise_release_coeff: f32,

    // Current output
    output: SpeechSidechain,
}

impl SpeechConfidenceEstimator {
    pub fn new(sample_rate: f32) -> Self {
        let frame_size = ((FRAME_MS * 0.001 * sample_rate) as usize).min(MAX_FRAME_SAMPLES);
        let hop_size = ((HOP_MS * 0.001 * sample_rate) as usize).max(1);
        let hang_samples = ((HANG_TIME_MS * 0.001 * sample_rate) as usize).max(1);

        // Create band-pass filters for speech band (250 Hz - 4 kHz)
        let mut bp_low_l = Biquad::new();
        let mut bp_low_r = Biquad::new();
        let mut bp_high_l = Biquad::new();
        let mut bp_high_r = Biquad::new();

        // High-pass at 250 Hz, low-pass at 4 kHz for speech band
        bp_low_l.update_hpf(SPEECH_BAND_LOW, 0.707, sample_rate);
        bp_low_r.update_hpf(SPEECH_BAND_LOW, 0.707, sample_rate);
        bp_high_l.update_lpf(SPEECH_BAND_HIGH, 0.707, sample_rate);
        bp_high_r.update_lpf(SPEECH_BAND_HIGH, 0.707, sample_rate);

        Self {
            sample_rate,
            frame_size,
            hop_size,
            input_buffer: [0.0; MAX_FRAME_SAMPLES],
            buffer_pos: 0,
            samples_since_hop: 0,
            bp_low_l,
            bp_low_r,
            bp_high_l,
            bp_high_r,
            frame_energy_total: 0.0,
            frame_energy_speech: 0.0,
            frame_sample_count: 0,
            prev_frame_energy: 0.0,
            raw_confidence: 0.0,
            smoothed_confidence: 0.0,
            noise_floor_sq: 1e-8,
            hang_counter: 0,
            hang_samples,
            attack_coeff: time_constant_coeff(CONFIDENCE_ATTACK_MS, sample_rate),
            release_coeff: time_constant_coeff(CONFIDENCE_RELEASE_MS, sample_rate),
            noise_attack_coeff: time_constant_coeff(NOISE_FLOOR_ATTACK_MS, sample_rate),
            noise_release_coeff: time_constant_coeff(NOISE_FLOOR_RELEASE_MS, sample_rate),
            output: SpeechSidechain::default(),
        }
    }

    /// Prepare the estimator for a new sample rate
    pub fn prepare(&mut self, sample_rate: f32) {
        // Update sample rate dependent parameters
        self.sample_rate = sample_rate;
        self.frame_size = ((FRAME_MS * 0.001 * sample_rate) as usize).min(MAX_FRAME_SAMPLES);
        self.hop_size = ((HOP_MS * 0.001 * sample_rate) as usize).max(1);
        self.hang_samples = ((HANG_TIME_MS * 0.001 * sample_rate) as usize).max(1);

        // Update filter coefficients for the new sample rate
        self.bp_low_l.update_hpf(SPEECH_BAND_LOW, 0.707, sample_rate);
        self.bp_low_r.update_hpf(SPEECH_BAND_LOW, 0.707, sample_rate);
        self.bp_high_l.update_lpf(SPEECH_BAND_HIGH, 0.707, sample_rate);
        self.bp_high_r.update_lpf(SPEECH_BAND_HIGH, 0.707, sample_rate);

        // Update smoothing coefficients
        self.attack_coeff = time_constant_coeff(CONFIDENCE_ATTACK_MS, sample_rate);
        self.release_coeff = time_constant_coeff(CONFIDENCE_RELEASE_MS, sample_rate);
        self.noise_attack_coeff = time_constant_coeff(NOISE_FLOOR_ATTACK_MS, sample_rate);
        self.noise_release_coeff = time_constant_coeff(NOISE_FLOOR_RELEASE_MS, sample_rate);

        // Reset frame counters and accumulators to ensure clean state after sample rate change
        self.buffer_pos = 0;
        self.samples_since_hop = 0;
        self.frame_energy_total = 0.0;
        self.frame_energy_speech = 0.0;
        self.frame_sample_count = 0;
        self.prev_frame_energy = 0.0;
        self.raw_confidence = 0.0;
        self.smoothed_confidence = 0.0;
        self.noise_floor_sq = 1e-8;
        self.hang_counter = 0;
        self.output = SpeechSidechain::default();
    }

    /// Process a stereo sample pair and update speech confidence
    /// This is analysis-only - does not modify audio
    ///
    /// ## Edge Cases Handled:
    /// - Silent input (0.0, 0.0): Returns low speech confidence
    /// - Extremely quiet signals: May trigger noise floor adaptation
    /// - DC offset: Bandpass filtering removes DC before analysis
    /// - Sample rate changes: Requires calling prepare() method
    #[inline]
    pub fn process(&mut self, left: f32, right: f32) -> SpeechSidechain {
        let mono = 0.5 * (left + right);

        // Band-pass filter for speech band energy
        let speech_l = self.bp_high_l.process(self.bp_low_l.process(left));
        let speech_r = self.bp_high_r.process(self.bp_low_r.process(right));
        let speech_mono = 0.5 * (speech_l + speech_r);

        // Accumulate energy
        let total_sq = mono * mono;
        let speech_sq = speech_mono * speech_mono;

        self.frame_energy_total += total_sq;
        self.frame_energy_speech += speech_sq;
        self.frame_sample_count += 1;

        // Store in circular buffer
        self.input_buffer[self.buffer_pos] = mono;
        self.buffer_pos = (self.buffer_pos + 1) % self.frame_size;
        self.samples_since_hop += 1;

        // Process frame on hop boundary
        if self.samples_since_hop >= self.hop_size {
            self.analyze_frame();
            self.samples_since_hop = 0;
        }

        self.output
    }

    /// Analyze accumulated frame data and update confidence
    fn analyze_frame(&mut self) {
        if self.frame_sample_count == 0 {
            return;
        }

        let n = self.frame_sample_count as f32;

        // 1. RMS Energy
        let rms_total = (self.frame_energy_total / n).sqrt();
        let rms_speech = (self.frame_energy_speech / n).sqrt();

        // 2. Speech band ratio
        let speech_ratio = if rms_total > DB_EPS {
            (rms_speech / rms_total).clamp(0.0, 1.0)
        } else {
            0.0
        };

        // 3. Spectral flux (energy change between frames)
        let flux = if self.prev_frame_energy > DB_EPS {
            let ratio = self.frame_energy_total / (self.prev_frame_energy + DB_EPS);
            // Normalize flux: speech has moderate variations, noise is flat
            (ratio.ln().abs() / 2.0).clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.prev_frame_energy = self.frame_energy_total;

        // 4. Spectral flatness approximation
        // (Using speech band ratio as proxy - real flatness would need FFT)
        // Speech: high speech_ratio, moderate flatness
        // Noise: lower speech_ratio, high flatness
        let flatness_score = if speech_ratio > MIN_SPEECH_RATIO {
            // More speech band energy suggests structured (speech-like) content
            1.0 - ((1.0 - speech_ratio) * 1.5).clamp(0.0, 1.0)
        } else {
            0.0
        };

        // 5. Level gate - must be above noise floor
        let above_floor = if rms_total * rms_total > self.noise_floor_sq * 4.0 {
            let ratio = rms_total / (self.noise_floor_sq.sqrt() + DB_EPS);
            (ratio / 10.0).clamp(0.0, 1.0)
        } else {
            0.0
        };

        // Update noise floor (slow tracking of minimum energy)
        let current_sq = self.frame_energy_total / n;
        if current_sq < self.noise_floor_sq {
            // Fast attack to catch quieter moments
            self.noise_floor_sq = self.noise_attack_coeff * self.noise_floor_sq
                + (1.0 - self.noise_attack_coeff) * current_sq;
        } else {
            // Slow release to follow the signal up
            self.noise_floor_sq = self.noise_release_coeff * self.noise_floor_sq
                + (1.0 - self.noise_release_coeff) * current_sq;
        }
        // Clamp noise floor to reasonable range
        self.noise_floor_sq = self.noise_floor_sq.clamp(1e-12, 0.01);

        // Combine features into raw confidence
        // Weight: speech_ratio (0.4) + flatness (0.2) + flux (0.2) + level (0.2)
        let raw = if rms_total > MIN_RMS_THRESHOLD {
            let sr_score = if speech_ratio > MIN_SPEECH_RATIO {
                ((speech_ratio - MIN_SPEECH_RATIO) / (1.0 - MIN_SPEECH_RATIO)).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let flat_score = if flatness_score > FLATNESS_SPEECH_THRESHOLD {
                ((flatness_score - FLATNESS_SPEECH_THRESHOLD) / (1.0 - FLATNESS_SPEECH_THRESHOLD))
                    .clamp(0.0, 1.0)
            } else {
                0.0
            };

            0.4 * sr_score + 0.2 * flat_score + 0.2 * flux + 0.2 * above_floor
        } else {
            0.0
        };

        self.raw_confidence = raw.clamp(0.0, 1.0);

        // Apply attack/release smoothing with hang
        if self.raw_confidence > self.smoothed_confidence {
            // Attack: rising confidence
            self.smoothed_confidence = self.attack_coeff * self.smoothed_confidence
                + (1.0 - self.attack_coeff) * self.raw_confidence;
            self.hang_counter = self.hang_samples;
        } else if self.hang_counter > 0 {
            // Hang: hold confidence during brief dips
            self.hang_counter -= self.hop_size.min(self.hang_counter);
        } else {
            // Release: falling confidence after hang
            self.smoothed_confidence = self.release_coeff * self.smoothed_confidence
                + (1.0 - self.release_coeff) * self.raw_confidence;
        }

        // Update output
        self.output.speech_conf = self.smoothed_confidence.clamp(0.0, 1.0);
        self.output.noise_floor_db = 10.0 * self.noise_floor_sq.max(DB_EPS).log10();

        // Reset frame accumulators
        self.frame_energy_total = 0.0;
        self.frame_energy_speech = 0.0;
        self.frame_sample_count = 0;
    }

    /// Get current sidechain output (non-mutating)
    #[inline]
    pub fn get_output(&self) -> SpeechSidechain {
        self.output
    }

    /// Reset estimator state
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.input_buffer = [0.0; MAX_FRAME_SAMPLES];
        self.buffer_pos = 0;
        self.samples_since_hop = 0;
        self.frame_energy_total = 0.0;
        self.frame_energy_speech = 0.0;
        self.frame_sample_count = 0;
        self.prev_frame_energy = 0.0;
        self.raw_confidence = 0.0;
        self.smoothed_confidence = 0.0;
        self.noise_floor_sq = 1e-8;
        self.hang_counter = 0;
        self.output = SpeechSidechain::default();
        self.bp_low_l.reset();
        self.bp_low_r.reset();
        self.bp_high_l.reset();
        self.bp_high_r.reset();
    }

    /// Perform long-running stability maintenance - periodically reset learned states
    /// to prevent drift over multi-hour sessions
    pub fn maintain_stability(&mut self) {
        // Clamp noise floor to prevent extreme drift
        self.noise_floor_sq = self.noise_floor_sq.clamp(1e-12, 0.01);

        // Clamp confidence values to prevent numerical drift
        self.raw_confidence = self.raw_confidence.clamp(0.0, 1.0);
        self.smoothed_confidence = self.smoothed_confidence.clamp(0.0, 1.0);

        // Reset hang counter if it gets too large
        if self.hang_counter > self.hang_samples * 100 {
            self.hang_counter = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_speech_confidence_creation() {
        let estimator = SpeechConfidenceEstimator::new(48000.0);
        assert!(estimator.frame_size > 0);
        assert!(estimator.hop_size > 0);
        assert!(estimator.frame_size <= MAX_FRAME_SAMPLES);
    }

    #[test]
    fn test_silence_gives_low_confidence() {
        let mut estimator = SpeechConfidenceEstimator::new(48000.0);

        // Process silence
        for _ in 0..4800 {
            estimator.process(0.0, 0.0);
        }

        assert!(estimator.get_output().speech_conf < 0.1);
    }

    #[test]
    fn test_noise_floor_tracking() {
        let mut estimator = SpeechConfidenceEstimator::new(48000.0);

        // Process very quiet signal
        for i in 0..4800 {
            let sample = 0.0001 * (i as f32 * 0.1).sin();
            estimator.process(sample, sample);
        }

        // Noise floor should be very low
        assert!(estimator.get_output().noise_floor_db < -40.0);
    }
}
