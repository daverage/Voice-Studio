//! Speech-aware Downward Expander
//!
//! Controls pauses and room swell without hard gating by using speech confidence
//! to weight expansion - applies more attenuation during non-speech sections
//! while preserving natural dynamics during speech.
//!
//! # Purpose
//! Reduces background noise during pauses and quiet sections while maintaining
//! natural speech dynamics and avoiding the pumping artifacts of hard gates.
//!
//! # Design Notes
//! - Uses speech confidence to intelligently apply expansion
//! - More attenuation during non-speech sections, preservation during speech
//! - Gentle approach avoids pumping artifacts of hard gates
//! - Maintains natural dynamics of the voice
//!   - "Chattering" if attack/release is poorly tuned.
//!   - Cutting off breath intakes or soft phrase starts.
//! - **Will Not Do**:
//!   - Hard gate (silence).
//!   - Replace background noise with synthesized silence.
//!
//! # Lifecycle
//! - **Active**: Normal operation.
//! - **Bypassed**: Passes audio through.
//!
//! # Time Scale Separation (Expander vs Leveler)
//!
//! This module operates on SHORTER time scales than the Leveler (`LinkedCompressor`):
//! - **Expander**: Attack 10ms, Release 150ms, Hold 80ms - targets inter-phrase gaps
//! - **Leveler**: Attack 30ms, Release 250ms - targets macroscopic level changes
//!
//! The expander MUST NOT counteract leveler trends. Its role is to:
//! - Attenuate noise during pauses (speech_conf low)
//! - Preserve natural dynamics during speech (speech_conf high)
//!
//! The Leveler is the authoritative long-term gain controller. The expander
//! only shapes the dynamic range within phrases, not across them.
//!
//! ## Audio Thread Safety
//! - No allocations during `process()`
//! - All state pre-initialized in `new()`

use super::envelope::VoiceEnvelope;
use super::speech_confidence::SpeechSidechain;
use super::utils::{db_to_lin, lin_to_db, time_constant_coeff, DB_EPS};

// =============================================================================
// Constants
// =============================================================================

/// Expansion ratio (2:1 means below threshold, output changes 2dB for every 1dB input change)
const EXPANSION_RATIO: f32 = 2.0;

/// Maximum attenuation in dB
const MAX_ATTENUATION_DB: f32 = 12.0;

/// Attack time in milliseconds
const ATTACK_MS: f32 = 10.0;

/// Release time in milliseconds
const RELEASE_MS: f32 = 150.0;

/// Fast release time for re-entry from deep attenuation (Task 5 edge case)
/// Used when gain_env is below this threshold to prevent muffled speech onset
const FAST_RELEASE_MS: f32 = 30.0;

/// Threshold below which fast release is used (prevents muffled re-entry)
const FAST_RELEASE_THRESHOLD: f32 = 0.5;

/// Hold time in milliseconds (prevents chatter)
const HOLD_MS: f32 = 80.0;

/// Threshold offset from noise floor in dB
const THRESHOLD_OFFSET_DB: f32 = 6.0;

/// Minimum threshold in dB (absolute floor)
const MIN_THRESHOLD_DB: f32 = -60.0;

/// Maximum threshold in dB (to prevent over-expansion)
const MAX_THRESHOLD_DB: f32 = -30.0;

/// RMS threshold below which the expander stays transparent during silence
const SILENCE_EXPAND_RMS: f32 = 0.0012;

// =============================================================================
// Speech Expander
// =============================================================================

/// Stereo-linked speech-aware downward expander
pub struct SpeechExpander {
    #[allow(dead_code)] // Stored for potential sample rate change support
    sample_rate: f32,

    // Gain reduction state
    gain_env: f32,
    attack_coeff: f32,
    release_coeff: f32,
    fast_release_coeff: f32, // Task 5: faster release for re-entry from deep attenuation

    // Hold counter (in samples)
    hold_counter: usize,
    hold_samples: usize,

    // Current threshold (adaptive to noise floor)
    threshold_db: f32,

    // Smoothed gain reduction for output
    current_gain: f32,
}

impl SpeechExpander {
    pub fn new(sample_rate: f32) -> Self {
        let hold_samples = ((HOLD_MS * 0.001 * sample_rate) as usize).max(1);

        Self {
            sample_rate,
            gain_env: 1.0,
            attack_coeff: time_constant_coeff(ATTACK_MS, sample_rate),
            release_coeff: time_constant_coeff(RELEASE_MS, sample_rate),
            fast_release_coeff: time_constant_coeff(FAST_RELEASE_MS, sample_rate),
            hold_counter: 0,
            hold_samples,
            threshold_db: MIN_THRESHOLD_DB,
            current_gain: 1.0,
        }
    }

    /// Process a stereo sample pair
    ///
    /// * `left`, `right` - Input samples
    /// * `amount` - Expansion amount (0.0 - 1.0)
    /// * `sidechain` - Speech confidence for weighting
    ///
    /// Returns (processed_left, processed_right)
    #[inline]
    pub fn process(
        &mut self,
        left: f32,
        right: f32,
        amount: f32,
        sidechain: &SpeechSidechain,
        env_l: &VoiceEnvelope,
        env_r: &VoiceEnvelope,
    ) -> (f32, f32) {
        // Bypass if amount is negligible
        if amount < 0.001 {
            return (left, right);
        }

        // Use shared RMS envelope (stereo-linked)
        let rms_l = env_l.rms;
        let rms_r = env_r.rms;

        // Linked RMS (max of both channels)
        let rms = rms_l.max(rms_r);
        let rms_db = lin_to_db(rms);

        if rms < SILENCE_EXPAND_RMS && sidechain.speech_conf < 0.2 {
            return (left, right);
        }

        // Adaptive threshold based on noise floor
        self.threshold_db = (sidechain.noise_floor_db + THRESHOLD_OFFSET_DB)
            .clamp(MIN_THRESHOLD_DB, MAX_THRESHOLD_DB);

        // Calculate target gain reduction
        let target_gain = if rms_db < self.threshold_db {
            // Below threshold: apply expansion
            let diff_db = self.threshold_db - rms_db;
            let reduction_db = diff_db * (EXPANSION_RATIO - 1.0);
            let clamped_reduction = reduction_db.min(MAX_ATTENUATION_DB);

            // Weight by inverse speech confidence
            // More expansion when speech_conf is low (noise/silence)
            let speech_weight = 1.0 - sidechain.speech_conf;
            let effective_reduction = clamped_reduction * speech_weight * amount;

            db_to_lin(-effective_reduction)
        } else {
            // Above threshold: no expansion
            1.0
        };

        // Apply hold logic: don't release immediately after speech
        if target_gain >= self.gain_env * 0.99 {
            // Signal rising or stable - reset hold
            self.hold_counter = self.hold_samples;
        }

        // Smooth gain changes with attack/release
        // Task 5 edge case: Use fast release when coming from deep attenuation
        // to prevent muffled speech onset after long silence
        let smoothed_target = if target_gain < self.gain_env {
            // Attacking (gain decreasing = more attenuation)
            if self.hold_counter > 0 {
                self.hold_counter -= 1;
                self.gain_env // Hold current gain
            } else {
                self.attack_coeff * self.gain_env + (1.0 - self.attack_coeff) * target_gain
            }
        } else {
            // Releasing (gain increasing = less attenuation)
            // Use fast release when coming from deep attenuation (prevents muffled re-entry)
            let release_coeff = if self.gain_env < FAST_RELEASE_THRESHOLD {
                self.fast_release_coeff
            } else {
                self.release_coeff
            };
            release_coeff * self.gain_env + (1.0 - release_coeff) * target_gain
        };

        self.gain_env = smoothed_target.clamp(db_to_lin(-MAX_ATTENUATION_DB), 1.0);
        self.current_gain = self.gain_env;

        (left * self.current_gain, right * self.current_gain)
    }

    /// Reset all state
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.gain_env = 1.0;
        self.hold_counter = 0;
        self.threshold_db = MIN_THRESHOLD_DB;
        self.current_gain = 1.0;
    }

    /// Get current gain reduction in dB (for metering)
    #[inline]
    pub fn get_gain_reduction_db(&self) -> f32 {
        -lin_to_db(self.current_gain.max(DB_EPS))
    }

    /// Get current threshold in dB (for metering/debugging)
    #[inline]
    #[allow(dead_code)]
    pub fn get_threshold_db(&self) -> f32 {
        self.threshold_db
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_creation() {
        let expander = SpeechExpander::new(48000.0);
        assert!(expander.hold_samples > 0);
        assert!(expander.current_gain == 1.0);
    }

    #[test]
    fn test_bypass_at_zero_amount() {
        let mut expander = SpeechExpander::new(48000.0);
        let sidechain = SpeechSidechain {
            speech_conf: 0.0,
            noise_floor_db: -60.0,
        };

        let input_l = 0.5;
        let input_r = 0.3;
        let (out_l, out_r) = expander.process(
            input_l,
            input_r,
            0.0,
            &sidechain,
            &VoiceEnvelope::default(),
            &VoiceEnvelope::default(),
        );

        assert!((out_l - input_l).abs() < 1e-10);
        assert!((out_r - input_r).abs() < 1e-10);
    }

    #[test]
    fn test_no_expansion_during_speech() {
        let mut expander = SpeechExpander::new(48000.0);
        let sidechain = SpeechSidechain {
            speech_conf: 1.0, // Full speech confidence
            noise_floor_db: -60.0,
        };

        // Process some samples at moderate level
        for _ in 0..1000 {
            expander.process(
                0.1,
                0.1,
                1.0,
                &sidechain,
                &VoiceEnvelope::default(),
                &VoiceEnvelope::default(),
            );
        }

        // With speech_conf = 1.0, expansion should be minimal
        // (speech_weight = 1 - 1.0 = 0)
        assert!(expander.get_gain_reduction_db() < 1.0);
    }

    #[test]
    fn test_expansion_during_silence() {
        let mut expander = SpeechExpander::new(48000.0);
        let sidechain = SpeechSidechain {
            speech_conf: 0.0, // No speech
            noise_floor_db: -60.0,
        };

        // RMS must be:
        // 1. Above SILENCE_EXPAND_RMS (0.0012) to not early-return
        // 2. Below threshold_db (-54 dB = -60 + 6) to trigger expansion
        // 0.0015 ≈ -56.5 dB, which satisfies both conditions
        let quiet_env = VoiceEnvelope {
            fast: 0.0015,
            slow: 0.0015,
            rms: 0.0015,
            confidence: 0.0,
            noise_floor: 1e-4,
        };

        // Process very quiet signal with proper envelope
        for _ in 0..10000 {
            expander.process(0.0001, 0.0001, 1.0, &sidechain, &quiet_env, &quiet_env);
        }

        // Should see some gain reduction
        assert!(expander.get_gain_reduction_db() > 0.1);
    }
}
