//! Speech Confidence Estimator (Sidechain Only)
//!
//! Provides the single shared speech activity envelope for the entire VxCleaner plugin.
//! All modules requiring speech/silence awareness must read from this source rather than
//! implementing their own detectors to ensure consistency across the processing chain.
//!
//! # Purpose
//! Delivers a canonical source of speech activity detection that enables coordinated
//! processing across multiple modules without duplicating effort or creating inconsistent
//! behavior.
//!
//! # Design Notes
//! - Provides THE single shared speech activity envelope for the entire plugin
//! - All modules requiring speech/silence awareness must use this source
//! - Sidechain-only processing (does not modify audio directly)
//! - Optimized for real-time voice processing applications
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
//! - All state is pre-allocated in `new()`
//! - No allocations during `process()`
//! - Uses frame-based analysis (20 ms frame, 10 ms hop)

use super::biquad::Biquad;
use super::utils::{time_constant_coeff, DB_EPS};

// =============================================================================
// Constants
// =============================================================================

/// Hop size in milliseconds (overlap)
const HOP_MS: f32 = 10.0;

/// Speech band lower frequency (Hz)
const SPEECH_BAND_LOW: f32 = 250.0;

/// Speech band upper frequency (Hz)
const SPEECH_BAND_HIGH: f32 = 4000.0;

/// Attack time for confidence smoothing (ms)
const CONFIDENCE_ATTACK_MS: f32 = 15.0;

/// Release time for confidence smoothing (ms)
const CONFIDENCE_RELEASE_MS: f32 = 120.0;

/// Silence RMS threshold (linear)
const SILENCE_RMS_THRESHOLD: f32 = 0.0004;

/// Fast release time when in silence (ms)
const SILENCE_RELEASE_MS: f32 = 90.0;

/// Hang time to prevent flicker (ms)
const HANG_TIME_MS: f32 = 80.0;

/// Minimum RMS threshold for speech detection (linear)
const MIN_RMS_THRESHOLD: f32 = 0.001;

/// Noise floor tracking attack (fast catch downward)
const NOISE_FLOOR_ATTACK_MS: f32 = 500.0;

/// Noise floor tracking release (slow drift upward)
const NOISE_FLOOR_RELEASE_MS: f32 = 50.0;

/// “Structured content” threshold (proxy; not true spectral flatness)
const STRUCTURE_SPEECH_THRESHOLD: f32 = 0.4;

/// Minimum speech band ratio for confidence
const MIN_SPEECH_RATIO: f32 = 0.3;

/// Absolute cap on flux contribution (softly normalizes ln ratio)
const FLUX_NORM_DIV: f32 = 3.0;

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
    hop_size: usize,

    // Hop counter
    samples_since_hop: usize,

    // Band-pass filters for speech band extraction
    bp_low_l: Biquad,
    bp_low_r: Biquad,
    bp_high_l: Biquad,
    bp_high_r: Biquad,

    // Feature accumulators (reset per hop analysis)
    frame_energy_total: f32,
    frame_energy_speech: f32,
    frame_sample_count: usize,

    // Previous hop energy for flux calculation
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
    silence_release_coeff: f32,
    noise_attack_coeff: f32,
    noise_release_coeff: f32,

    // Current output
    output: SpeechSidechain,
}

impl SpeechConfidenceEstimator {
    pub fn new(sample_rate: f32) -> Self {
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
            hop_size,
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
            silence_release_coeff: time_constant_coeff(SILENCE_RELEASE_MS, sample_rate),
            noise_attack_coeff: time_constant_coeff(NOISE_FLOOR_ATTACK_MS, sample_rate),
            noise_release_coeff: time_constant_coeff(NOISE_FLOOR_RELEASE_MS, sample_rate),
            output: SpeechSidechain::default(),
        }
    }

    /// Process a stereo sample pair and update speech confidence
    /// This is analysis-only - does not modify audio.
    ///
    /// Edge Cases:
    /// - Silent input: returns low confidence
    /// - Very quiet signals: noise floor adaptation may respond
    /// - DC offset: removed by band-pass analysis filters
    #[inline]
    pub fn process(&mut self, left: f32, right: f32) -> SpeechSidechain {
        let mono = 0.5 * (left + right);

        // Speech band extraction per channel then recombine (keeps stereo symmetry stable)
        let speech_l = self.bp_high_l.process(self.bp_low_l.process(left));
        let speech_r = self.bp_high_r.process(self.bp_low_r.process(right));
        let speech_mono = 0.5 * (speech_l + speech_r);

        // Accumulate energy
        self.frame_energy_total += mono * mono;
        self.frame_energy_speech += speech_mono * speech_mono;
        self.frame_sample_count += 1;

        // Hop scheduling
        self.samples_since_hop += 1;
        if self.samples_since_hop >= self.hop_size {
            self.analyze_hop();
            self.samples_since_hop = 0;
        }

        self.output
    }

    /// Analyze accumulated hop data and update confidence.
    ///
    /// Note: although `FRAME_MS` exists, this implementation uses hop-wise aggregates
    /// with smoothing + hang to achieve the intended 20ms/10ms behavior without needing
    /// an explicit circular buffer or FFT.
    fn analyze_hop(&mut self) {
        if self.frame_sample_count == 0 {
            return;
        }

        let n = self.frame_sample_count as f32;

        // 1) RMS energy
        let rms_total = (self.frame_energy_total / n).sqrt();
        let rms_speech = (self.frame_energy_speech / n).sqrt();

        // 2) Speech band ratio
        let speech_ratio = if rms_total > DB_EPS {
            (rms_speech / rms_total).clamp(0.0, 1.0)
        } else {
            0.0
        };

        // 3) Flux (hop-to-hop energy change) normalized softly
        let flux = if self.prev_frame_energy > DB_EPS {
            let ratio = self.frame_energy_total / (self.prev_frame_energy + DB_EPS);
            // ln ratio is symmetric; normalize gently to avoid over-triggering on noisy material
            (ratio.ln().abs() / FLUX_NORM_DIV).clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.prev_frame_energy = self.frame_energy_total;

        // 4) Structured-content proxy (NOT true spectral flatness; no FFT here).
        // Higher speech_ratio implies more voiced / formant-like structure in 250–4k region.
        let structure_score = if speech_ratio > MIN_SPEECH_RATIO {
            1.0 - ((1.0 - speech_ratio) * 1.5).clamp(0.0, 1.0)
        } else {
            0.0
        };

        // 5) Level gate above noise floor
        let above_floor = if (rms_total * rms_total) > (self.noise_floor_sq * 4.0) {
            let ratio = rms_total / (self.noise_floor_sq.sqrt() + DB_EPS);
            (ratio / 10.0).clamp(0.0, 1.0)
        } else {
            0.0
        };

        // Update noise floor (track minimum energy with asymmetric ballistics)
        let current_sq = self.frame_energy_total / n;
        if current_sq < self.noise_floor_sq {
            // Faster “attack” downward to catch quieter moments
            self.noise_floor_sq = self.noise_attack_coeff * self.noise_floor_sq
                + (1.0 - self.noise_attack_coeff) * current_sq;
        } else {
            // Slower “release” upward so speech doesn’t instantly raise the floor
            self.noise_floor_sq = self.noise_release_coeff * self.noise_floor_sq
                + (1.0 - self.noise_release_coeff) * current_sq;
        }
        self.noise_floor_sq = self.noise_floor_sq.clamp(1e-12, 0.01);

        // Map features into 0..1 components
        let sr_score = if speech_ratio > MIN_SPEECH_RATIO {
            ((speech_ratio - MIN_SPEECH_RATIO) / (1.0 - MIN_SPEECH_RATIO)).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let struct_score = if structure_score > STRUCTURE_SPEECH_THRESHOLD {
            ((structure_score - STRUCTURE_SPEECH_THRESHOLD) / (1.0 - STRUCTURE_SPEECH_THRESHOLD))
                .clamp(0.0, 1.0)
        } else {
            0.0
        };

        // Combine features into raw confidence.
        // Keep weights stable and conservative: ratio is primary, others are supporting evidence.
        let mut raw = if rms_total > MIN_RMS_THRESHOLD {
            0.40 * sr_score + 0.15 * struct_score + 0.25 * flux + 0.20 * above_floor
        } else {
            0.0
        };

        // STATIONARY NOISE PENALTY (Pink Noise Detector)
        // Pink noise has high `sr_score` (speech band energy) and high `above_floor` (loudness),
        // but very low `flux` (steady state).
        // If we see high energy but low flux, we crush the confidence.
        if rms_total > 0.01 && flux < 0.15 {
             // "This is loud but dead steady - it's a test signal or fan"
             raw *= 0.2;
        }

        self.raw_confidence = raw.clamp(0.0, 1.0);

        // Attack/release smoothing with hang:
        // - If raw rises, respond quickly and refresh hang timer.
        // - If raw dips briefly, hold during hang (prevents flicker).
        // - After hang expires, release smoothly.
        let is_silence = rms_total < SILENCE_RMS_THRESHOLD;

        if self.raw_confidence > self.smoothed_confidence {
            self.smoothed_confidence = self.attack_coeff * self.smoothed_confidence
                + (1.0 - self.attack_coeff) * self.raw_confidence;
            self.hang_counter = self.hang_samples;
        } else if self.hang_counter > 0 {
            // Silence should bypass hang and release faster
            if is_silence {
                self.hang_counter = 0;
            } else {
                // Decrement by hop_size so hang time is independent of buffer size and sample rate.
                self.hang_counter = self.hang_counter.saturating_sub(self.hop_size);
            }
        } else {
            let release_coeff = if rms_total < SILENCE_RMS_THRESHOLD {
                self.silence_release_coeff
            } else {
                self.release_coeff
            };
            self.smoothed_confidence = release_coeff * self.smoothed_confidence
                + (1.0 - release_coeff) * self.raw_confidence;
        }

        // Update output
        self.output.speech_conf = self.smoothed_confidence.clamp(0.0, 1.0);
        self.output.noise_floor_db = 10.0 * self.noise_floor_sq.max(DB_EPS).log10();

        // Reset accumulators for next hop analysis
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
        assert!(estimator.hop_size > 0);
    }

    #[test]
    fn test_silence_gives_low_confidence() {
        let mut estimator = SpeechConfidenceEstimator::new(48000.0);

        let mut sidechain = SpeechSidechain::default();
        for _ in 0..4800 {
            sidechain = estimator.process(0.0, 0.0);
        }

        assert!(sidechain.speech_conf < 0.1);
    }

    #[test]
    fn test_noise_floor_tracking() {
        let mut estimator = SpeechConfidenceEstimator::new(48000.0);

        let mut sidechain = SpeechSidechain::default();
        for i in 0..4800 {
            let sample = 0.0001 * (i as f32 * 0.1).sin();
            sidechain = estimator.process(sample, sample);
        }

        assert!(sidechain.noise_floor_db < -40.0);
    }
}
