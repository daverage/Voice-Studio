//! Profile Analyzer (Metric Computation for Data-Driven Calibration)
//!
//! Computes all metrics needed for AudioProfile from audio samples.
//! Used to generate InputProfile (pre-DSP) and OutputProfile (post-DSP).
//!
//! ## Audio Thread Safety
//! - All buffers are pre-allocated in `new()`
//! - No allocations during `process()`
//! - Frame-based analysis for efficiency

use super::biquad::Biquad;
use super::utils::{time_constant_coeff, DB_EPS};

// =============================================================================
// Constants
// =============================================================================

/// Analysis frame size in milliseconds
const FRAME_MS: f32 = 50.0;

/// Maximum frame size in samples (for 96kHz)
const MAX_FRAME_SAMPLES: usize = 4800;

/// Early reflection window (0-50ms) - samples at 48kHz
const EARLY_WINDOW_MS: f32 = 50.0;

/// Presence band: 2-5 kHz
const PRESENCE_LOW_HZ: f32 = 2000.0;
const PRESENCE_HIGH_HZ: f32 = 5000.0;

/// Air band: 8-16 kHz
const AIR_LOW_HZ: f32 = 8000.0;
const AIR_HIGH_HZ: f32 = 16000.0;

/// HF variance tracking band: 6-12 kHz (whisper detection)
const HF_VAR_LOW_HZ: f32 = 6000.0;
const HF_VAR_HIGH_HZ: f32 = 12000.0;

/// Full band reference: 100 Hz - 8 kHz
const FULLBAND_LOW_HZ: f32 = 100.0;
const FULLBAND_HIGH_HZ: f32 = 8000.0;

/// RMS variance tracking window (number of frames)
const RMS_VARIANCE_FRAMES: usize = 20;

/// Noise floor tracking coefficients
const NOISE_FLOOR_ATTACK_MS: f32 = 500.0;
const NOISE_FLOOR_RELEASE_MS: f32 = 100.0;

/// Decay slope measurement delay after speech onset (prevents plosive artifacts)
const DECAY_SLOPE_DELAY_MS: f32 = 75.0;

/// Minimum measurement window for decay slope (needs enough frames to be meaningful)
const DECAY_SLOPE_WINDOW_MS: f32 = 200.0;

/// Speech activity threshold multiplier (RMS must be > noise_floor * this to be "speech")
const SPEECH_ACTIVITY_MULT: f32 = 2.5;

// =============================================================================
// Profile Analyzer
// =============================================================================

/// Stereo profile analyzer for computing AudioProfile metrics
pub struct ProfileAnalyzer {
    #[allow(dead_code)] // Stored for potential sample rate change support
    sample_rate: f32,
    frame_size: usize,

    // Accumulated samples for frame analysis
    sample_count: usize,

    // Band-pass filters for frequency analysis
    // Presence band (2-5 kHz)
    presence_hp_l: Biquad,
    presence_hp_r: Biquad,
    presence_lp_l: Biquad,
    presence_lp_r: Biquad,

    // Air band (8-16 kHz)
    air_hp_l: Biquad,
    air_hp_r: Biquad,
    air_lp_l: Biquad,
    air_lp_r: Biquad,

    // HF variance band (6-12 kHz)
    hf_var_hp_l: Biquad,
    hf_var_hp_r: Biquad,
    hf_var_lp_l: Biquad,
    hf_var_lp_r: Biquad,

    // Full band reference
    fullband_hp_l: Biquad,
    fullband_hp_r: Biquad,
    fullband_lp_l: Biquad,
    fullband_lp_r: Biquad,

    // Energy accumulators (per frame)
    energy_total: f32,
    energy_presence: f32,
    energy_air: f32,
    energy_hf: f32,
    energy_fullband: f32,
    peak_abs: f32,

    // HF variance tracking
    hf_energy_history: [f32; 16],
    hf_history_idx: usize,

    // RMS variance tracking
    rms_history: [f32; RMS_VARIANCE_FRAMES],
    rms_history_idx: usize,

    // Noise floor tracking
    noise_floor_sq: f32,
    noise_attack_coeff: f32,
    noise_release_coeff: f32,

    // Early/Late energy tracking (for reverb analysis)
    early_energy: f32,
    late_energy: f32,
    early_samples: usize,
    early_window_samples: usize,

    // Decay slope tracking (with speech gating)
    prev_rms: f32,
    decay_slope_delay_samples: usize,
    decay_slope_window_frames: usize,
    decay_accumulator: f32,
    decay_count: usize,

    // Speech activity gating for decay slope
    speech_active: bool,
    speech_onset_frames: usize, // Frames since speech started
    stable_decay_slope: f32,    // Last stable measurement (used when gated)

    // Current computed profile
    current_profile: crate::AudioProfile,
}

impl ProfileAnalyzer {
    pub fn new(sample_rate: f32) -> Self {
        let frame_size = ((FRAME_MS * 0.001 * sample_rate) as usize).min(MAX_FRAME_SAMPLES);
        let early_window_samples = (EARLY_WINDOW_MS * 0.001 * sample_rate) as usize;
        let decay_slope_delay_samples = (DECAY_SLOPE_DELAY_MS * 0.001 * sample_rate) as usize;

        // Calculate decay slope window in frames (not samples)
        // FRAME_MS is the analysis frame duration, so we need frames not samples
        let decay_slope_window_frames = (DECAY_SLOPE_WINDOW_MS / FRAME_MS).ceil() as usize;

        // Create band-pass filters
        // Presence band
        let mut presence_hp_l = Biquad::new();
        let mut presence_hp_r = Biquad::new();
        let mut presence_lp_l = Biquad::new();
        let mut presence_lp_r = Biquad::new();
        presence_hp_l.update_hpf(PRESENCE_LOW_HZ, 0.707, sample_rate);
        presence_hp_r.update_hpf(PRESENCE_LOW_HZ, 0.707, sample_rate);
        presence_lp_l.update_lpf(PRESENCE_HIGH_HZ, 0.707, sample_rate);
        presence_lp_r.update_lpf(PRESENCE_HIGH_HZ, 0.707, sample_rate);

        // Air band
        let mut air_hp_l = Biquad::new();
        let mut air_hp_r = Biquad::new();
        let mut air_lp_l = Biquad::new();
        let mut air_lp_r = Biquad::new();
        air_hp_l.update_hpf(AIR_LOW_HZ, 0.707, sample_rate);
        air_hp_r.update_hpf(AIR_LOW_HZ, 0.707, sample_rate);
        air_lp_l.update_lpf(AIR_HIGH_HZ, 0.707, sample_rate);
        air_lp_r.update_lpf(AIR_HIGH_HZ, 0.707, sample_rate);

        // HF variance band
        let mut hf_var_hp_l = Biquad::new();
        let mut hf_var_hp_r = Biquad::new();
        let mut hf_var_lp_l = Biquad::new();
        let mut hf_var_lp_r = Biquad::new();
        hf_var_hp_l.update_hpf(HF_VAR_LOW_HZ, 0.707, sample_rate);
        hf_var_hp_r.update_hpf(HF_VAR_LOW_HZ, 0.707, sample_rate);
        hf_var_lp_l.update_lpf(HF_VAR_HIGH_HZ, 0.707, sample_rate);
        hf_var_lp_r.update_lpf(HF_VAR_HIGH_HZ, 0.707, sample_rate);

        // Full band reference
        let mut fullband_hp_l = Biquad::new();
        let mut fullband_hp_r = Biquad::new();
        let mut fullband_lp_l = Biquad::new();
        let mut fullband_lp_r = Biquad::new();
        fullband_hp_l.update_hpf(FULLBAND_LOW_HZ, 0.707, sample_rate);
        fullband_hp_r.update_hpf(FULLBAND_LOW_HZ, 0.707, sample_rate);
        fullband_lp_l.update_lpf(FULLBAND_HIGH_HZ, 0.707, sample_rate);
        fullband_lp_r.update_lpf(FULLBAND_HIGH_HZ, 0.707, sample_rate);

        Self {
            sample_rate,
            frame_size,
            sample_count: 0,

            presence_hp_l,
            presence_hp_r,
            presence_lp_l,
            presence_lp_r,

            air_hp_l,
            air_hp_r,
            air_lp_l,
            air_lp_r,

            hf_var_hp_l,
            hf_var_hp_r,
            hf_var_lp_l,
            hf_var_lp_r,

            fullband_hp_l,
            fullband_hp_r,
            fullband_lp_l,
            fullband_lp_r,

            energy_total: 0.0,
            energy_presence: 0.0,
            energy_air: 0.0,
            energy_hf: 0.0,
            energy_fullband: 0.0,
            peak_abs: 0.0,

            hf_energy_history: [0.0; 16],
            hf_history_idx: 0,

            rms_history: [0.0; RMS_VARIANCE_FRAMES],
            rms_history_idx: 0,

            noise_floor_sq: 1e-8,
            noise_attack_coeff: time_constant_coeff(NOISE_FLOOR_ATTACK_MS, sample_rate),
            noise_release_coeff: time_constant_coeff(NOISE_FLOOR_RELEASE_MS, sample_rate),

            early_energy: 0.0,
            late_energy: 0.0,
            early_samples: 0,
            early_window_samples,

            prev_rms: 0.0,
            decay_slope_delay_samples,
            decay_slope_window_frames,
            decay_accumulator: 0.0,
            decay_count: 0,

            // Speech activity gating for decay slope
            speech_active: false,
            speech_onset_frames: 0,
            stable_decay_slope: 0.0,

            current_profile: crate::AudioProfile::default(),
        }
    }

    /// Prepare the analyzer for a new sample rate
    pub fn prepare(&mut self, sample_rate: f32) {
        // Update sample rate dependent parameters
        self.sample_rate = sample_rate;
        self.frame_size = ((FRAME_MS * 0.001 * sample_rate) as usize).min(MAX_FRAME_SAMPLES);
        self.early_window_samples = (EARLY_WINDOW_MS * 0.001 * sample_rate) as usize;
        self.decay_slope_delay_samples = (DECAY_SLOPE_DELAY_MS * 0.001 * sample_rate) as usize;
        self.decay_slope_window_frames = (DECAY_SLOPE_WINDOW_MS / FRAME_MS).ceil() as usize;

        // Update noise floor tracking coefficients
        self.noise_attack_coeff = time_constant_coeff(NOISE_FLOOR_ATTACK_MS, sample_rate);
        self.noise_release_coeff = time_constant_coeff(NOISE_FLOOR_RELEASE_MS, sample_rate);

        // Update all filter coefficients for the new sample rate
        self.presence_hp_l.update_hpf(PRESENCE_LOW_HZ, 0.707, sample_rate);
        self.presence_hp_r.update_hpf(PRESENCE_LOW_HZ, 0.707, sample_rate);
        self.presence_lp_l.update_lpf(PRESENCE_HIGH_HZ, 0.707, sample_rate);
        self.presence_lp_r.update_lpf(PRESENCE_HIGH_HZ, 0.707, sample_rate);

        self.air_hp_l.update_hpf(AIR_LOW_HZ, 0.707, sample_rate);
        self.air_hp_r.update_hpf(AIR_LOW_HZ, 0.707, sample_rate);
        self.air_lp_l.update_lpf(AIR_HIGH_HZ, 0.707, sample_rate);
        self.air_lp_r.update_lpf(AIR_HIGH_HZ, 0.707, sample_rate);

        self.hf_var_hp_l.update_hpf(HF_VAR_LOW_HZ, 0.707, sample_rate);
        self.hf_var_hp_r.update_hpf(HF_VAR_LOW_HZ, 0.707, sample_rate);
        self.hf_var_lp_l.update_lpf(HF_VAR_HIGH_HZ, 0.707, sample_rate);
        self.hf_var_lp_r.update_lpf(HF_VAR_HIGH_HZ, 0.707, sample_rate);

        self.fullband_hp_l.update_hpf(FULLBAND_LOW_HZ, 0.707, sample_rate);
        self.fullband_hp_r.update_hpf(FULLBAND_LOW_HZ, 0.707, sample_rate);
        self.fullband_lp_l.update_lpf(FULLBAND_HIGH_HZ, 0.707, sample_rate);
        self.fullband_lp_r.update_lpf(FULLBAND_HIGH_HZ, 0.707, sample_rate);

        // Reset frame counters and accumulators to ensure clean state after sample rate change
        self.sample_count = 0;
        self.energy_total = 0.0;
        self.energy_presence = 0.0;
        self.energy_air = 0.0;
        self.energy_hf = 0.0;
        self.energy_fullband = 0.0;
        self.peak_abs = 0.0;
        self.early_energy = 0.0;
        self.late_energy = 0.0;
        self.early_samples = 0;
        self.prev_rms = 0.0;
        self.decay_accumulator = 0.0;
        self.decay_count = 0;
        self.speech_active = false;
        self.speech_onset_frames = 0;
        self.stable_decay_slope = 0.0;
    }

    /// Process a stereo sample pair and update profile metrics
    /// Call this for every sample in the input buffer (pre-DSP)
    #[inline]
    pub fn process(&mut self, left: f32, right: f32) {
        let mono = 0.5 * (left + right);
        let mono_sq = mono * mono;

        // Track peak
        let abs_mono = mono.abs();
        self.peak_abs = self.peak_abs.max(abs_mono);

        // Total energy
        self.energy_total += mono_sq;

        // Band energies
        let presence_l = self.presence_lp_l.process(self.presence_hp_l.process(left));
        let presence_r = self
            .presence_lp_r
            .process(self.presence_hp_r.process(right));
        self.energy_presence += 0.5 * (presence_l * presence_l + presence_r * presence_r);

        let air_l = self.air_lp_l.process(self.air_hp_l.process(left));
        let air_r = self.air_lp_r.process(self.air_hp_r.process(right));
        self.energy_air += 0.5 * (air_l * air_l + air_r * air_r);

        let hf_l = self.hf_var_lp_l.process(self.hf_var_hp_l.process(left));
        let hf_r = self.hf_var_lp_r.process(self.hf_var_hp_r.process(right));
        self.energy_hf += 0.5 * (hf_l * hf_l + hf_r * hf_r);

        let fb_l = self.fullband_lp_l.process(self.fullband_hp_l.process(left));
        let fb_r = self
            .fullband_lp_r
            .process(self.fullband_hp_r.process(right));
        self.energy_fullband += 0.5 * (fb_l * fb_l + fb_r * fb_r);

        // Early/Late energy tracking (simplified envelope following)
        if self.early_samples < self.early_window_samples {
            self.early_energy += mono_sq;
            self.early_samples += 1;
        } else {
            self.late_energy += mono_sq;
        }

        self.sample_count += 1;

        // Analyze frame when we have enough samples
        if self.sample_count >= self.frame_size {
            self.analyze_frame();
        }
    }

    /// Analyze accumulated frame data and update profile
    fn analyze_frame(&mut self) {
        if self.sample_count == 0 {
            return;
        }

        let n = self.sample_count as f32;

        // 1. RMS and Peak
        let rms = (self.energy_total / n).sqrt();
        let peak = self.peak_abs;
        let crest_factor_db = if rms > DB_EPS {
            20.0 * (peak / rms).log10()
        } else {
            0.0
        };

        // 2. RMS Variance (track over multiple frames)
        self.rms_history[self.rms_history_idx] = rms;
        self.rms_history_idx = (self.rms_history_idx + 1) % RMS_VARIANCE_FRAMES;

        let rms_mean: f32 = self.rms_history.iter().sum::<f32>() / RMS_VARIANCE_FRAMES as f32;
        let rms_variance: f32 = self
            .rms_history
            .iter()
            .map(|&x| (x - rms_mean) * (x - rms_mean))
            .sum::<f32>()
            / RMS_VARIANCE_FRAMES as f32;

        // 3. Noise floor tracking (using minimum energy approach)
        let frame_energy_sq = self.energy_total / n;
        if frame_energy_sq < self.noise_floor_sq {
            // Fast attack to catch quieter moments
            self.noise_floor_sq = self.noise_attack_coeff * self.noise_floor_sq
                + (1.0 - self.noise_attack_coeff) * frame_energy_sq;
        } else {
            // Slow release
            self.noise_floor_sq = self.noise_release_coeff * self.noise_floor_sq
                + (1.0 - self.noise_release_coeff) * frame_energy_sq;
        }
        self.noise_floor_sq = self.noise_floor_sq.clamp(1e-12, 0.1);
        let noise_floor = self.noise_floor_sq.sqrt();

        // 4. SNR
        let snr_db = if noise_floor > DB_EPS {
            20.0 * (rms / noise_floor).log10()
        } else {
            60.0 // Very clean signal
        };

        // 5. Early/Late ratio
        let early_late_ratio = if self.late_energy > DB_EPS {
            (self.early_energy / self.early_window_samples as f32)
                / (self.late_energy / (self.sample_count - self.early_window_samples).max(1) as f32)
        } else if self.early_energy > DB_EPS {
            1.0 // All energy is early (very dry)
        } else {
            0.5 // No energy
        };

        // 6. Decay slope (rate of energy decay) with speech gating
        // This prevents false "distant" detection from plosives, phrase endings, and silence
        let decay_slope = {
            // Check speech activity: RMS must be significantly above noise floor
            let speech_threshold = noise_floor * SPEECH_ACTIVITY_MULT;
            let is_speech_frame = rms > speech_threshold;

            // Track speech onset for delay
            if is_speech_frame && !self.speech_active {
                // Speech just started
                self.speech_active = true;
                self.speech_onset_frames = 0;
            } else if is_speech_frame && self.speech_active {
                // Continuing speech
                self.speech_onset_frames += 1;
            } else if !is_speech_frame {
                // Silence/noise - reset speech tracking
                self.speech_active = false;
                self.speech_onset_frames = 0;
            }

            // Calculate delay in frames (decay_slope_delay_samples / frame_size)
            let delay_frames = (self.decay_slope_delay_samples / self.frame_size).max(1);

            // Only measure decay slope if:
            // 1. Speech is active
            // 2. We've waited past the delay period (skip plosives/attack)
            // 3. Both current and previous RMS are valid
            let should_measure = self.speech_active
                && self.speech_onset_frames >= delay_frames
                && self.prev_rms > DB_EPS
                && rms > DB_EPS;

            if should_measure {
                let slope = (rms - self.prev_rms) / self.prev_rms;
                // Accumulate for smoothing
                self.decay_accumulator += slope;
                self.decay_count += 1;

                // Update stable value once we have enough measurements
                if self.decay_count >= self.decay_slope_window_frames {
                    self.stable_decay_slope = self.decay_accumulator / self.decay_count as f32;
                    self.decay_accumulator = 0.0;
                    self.decay_count = 0;
                }
            }

            self.prev_rms = rms;

            // Return stable value (holds last good measurement when gated)
            self.stable_decay_slope
        };

        // 7. Presence ratio (presence band energy / fullband energy)
        let presence_ratio = if self.energy_fullband > DB_EPS {
            self.energy_presence / self.energy_fullband
        } else {
            0.0
        };

        // 8. Air ratio (air band energy / fullband energy)
        let air_ratio = if self.energy_fullband > DB_EPS {
            self.energy_air / self.energy_fullband
        } else {
            0.0
        };

        // 9. HF variance (variance of HF energy over time - whisper detection)
        let hf_energy_frame = self.energy_hf / n;
        self.hf_energy_history[self.hf_history_idx] = hf_energy_frame;
        self.hf_history_idx = (self.hf_history_idx + 1) % 16;

        let hf_mean: f32 = self.hf_energy_history.iter().sum::<f32>() / 16.0;
        let hf_variance: f32 = self
            .hf_energy_history
            .iter()
            .map(|&x| (x - hf_mean) * (x - hf_mean))
            .sum::<f32>()
            / 16.0;

        // Update current profile
        self.current_profile = crate::AudioProfile {
            rms,
            peak,
            crest_factor_db,
            rms_variance,
            noise_floor,
            snr_db,
            early_late_ratio: early_late_ratio.clamp(0.0, 2.0),
            decay_slope,
            presence_ratio,
            air_ratio,
            hf_variance,
        };

        // Reset frame accumulators
        self.sample_count = 0;
        self.energy_total = 0.0;
        self.energy_presence = 0.0;
        self.energy_air = 0.0;
        self.energy_hf = 0.0;
        self.energy_fullband = 0.0;
        self.peak_abs = 0.0;
        self.early_energy = 0.0;
        self.late_energy = 0.0;
        self.early_samples = 0;
    }

    /// Get the current computed profile
    #[inline]
    pub fn get_profile(&self) -> crate::AudioProfile {
        self.current_profile
    }

    /// Force finalize the current frame (call at end of buffer if needed)
    pub fn finalize_frame(&mut self) {
        if self.sample_count > 0 {
            self.analyze_frame();
        }
    }

    /// Reset all state
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.sample_count = 0;
        self.energy_total = 0.0;
        self.energy_presence = 0.0;
        self.energy_air = 0.0;
        self.energy_hf = 0.0;
        self.energy_fullband = 0.0;
        self.peak_abs = 0.0;
        self.early_energy = 0.0;
        self.late_energy = 0.0;
        self.early_samples = 0;
        self.hf_energy_history = [0.0; 16];
        self.hf_history_idx = 0;
        self.rms_history = [0.0; RMS_VARIANCE_FRAMES];
        self.rms_history_idx = 0;
        self.noise_floor_sq = 1e-8;
        self.prev_rms = 0.0;
        self.decay_accumulator = 0.0;
        self.decay_count = 0;
        self.speech_active = false;
        self.speech_onset_frames = 0;
        self.stable_decay_slope = 0.0;
        self.current_profile = crate::AudioProfile::default();

        // Reset filters
        self.presence_hp_l.reset();
        self.presence_hp_r.reset();
        self.presence_lp_l.reset();
        self.presence_lp_r.reset();
        self.air_hp_l.reset();
        self.air_hp_r.reset();
        self.air_lp_l.reset();
        self.air_lp_r.reset();
        self.hf_var_hp_l.reset();
        self.hf_var_hp_r.reset();
        self.hf_var_lp_l.reset();
        self.hf_var_lp_r.reset();
        self.fullband_hp_l.reset();
        self.fullband_hp_r.reset();
        self.fullband_lp_l.reset();
        self.fullband_lp_r.reset();
    }

    /// Perform long-running stability maintenance - periodically reset learned states
    /// to prevent drift over multi-hour sessions
    pub fn maintain_stability(&mut self) {
        // Clamp noise floor to prevent extreme drift
        self.noise_floor_sq = self.noise_floor_sq.clamp(1e-12, 0.1);

        // Reset decay accumulator if it's accumulated for too long without processing
        if self.decay_count > self.decay_slope_window_frames * 2 {
            self.decay_accumulator = 0.0;
            self.decay_count = 0;
        }

        // Reset speech tracking if no speech has been detected for a long time
        if self.speech_onset_frames > self.decay_slope_window_frames * 10 {
            self.speech_active = false;
            self.speech_onset_frames = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_analyzer_creation() {
        let analyzer = ProfileAnalyzer::new(48000.0);
        assert!(analyzer.frame_size > 0);
        assert!(analyzer.frame_size <= MAX_FRAME_SAMPLES);
    }

    #[test]
    fn test_silence_profile() {
        let mut analyzer = ProfileAnalyzer::new(48000.0);

        // Process silence
        for _ in 0..4800 {
            analyzer.process(0.0, 0.0);
        }
        analyzer.finalize_frame();

        let profile = analyzer.get_profile();
        assert!(profile.rms < 0.001);
        assert!(profile.peak < 0.001);
    }

    #[test]
    fn test_sine_wave_profile() {
        let mut analyzer = ProfileAnalyzer::new(48000.0);

        // Process 1kHz sine wave
        for i in 0..4800 {
            let sample = 0.5 * (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / 48000.0).sin();
            analyzer.process(sample, sample);
        }
        analyzer.finalize_frame();

        let profile = analyzer.get_profile();
        // RMS of 0.5 amplitude sine is ~0.354
        assert!(profile.rms > 0.3 && profile.rms < 0.4);
        // Peak should be ~0.5
        assert!(profile.peak > 0.45 && profile.peak < 0.55);
        // Crest factor of sine is ~3 dB
        assert!(profile.crest_factor_db > 2.0 && profile.crest_factor_db < 4.0);
    }
}
