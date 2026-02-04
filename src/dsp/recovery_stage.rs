//! Recovery Stage (Speech-Gated EQ)
//!
//! Final recovery stage that applies presence and air shelving EQ during speech
//! to compensate for losses from earlier subtractive processing stages.
//!
//! # Purpose
//! Compensates for energy lost during early reflection suppression, clarity,
//! de-essing, and guardrails by applying targeted EQ during speech segments
//! while avoiding amplification of noise during silence.
//!
//! # Design Notes
//! - Presence shelf: +1.5 to +2.5 dB at ~2.5kHz
//! - Air shelf: +2 to +4 dB at ~10kHz
//! - Only active when speech confidence > 0.6
//! - Hard clamped to 0 when confidence < 0.3

use crate::dsp::biquad::Biquad;
use crate::dsp::utils::time_constant_coeff;

// Constants for recovery EQ
const PRESENCE_FREQ_HZ: f32 = 2500.0;
const AIR_FREQ_HZ: f32 = 10000.0;
const Q: f32 = 0.707;

// Ballistic envelope smoothing (ms)
const ATTACK_MS: f32 = 150.0; // Slow attack: prevents brightness pumping
const RELEASE_MS: f32 = 60.0; // Faster release: prevents dull hangover

// Recovery gain ranges
const MIN_PRESENCE_GAIN_DB: f32 = 1.5;
const MAX_PRESENCE_GAIN_DB: f32 = 2.5;
const MIN_AIR_GAIN_DB: f32 = 2.0;
const MAX_AIR_GAIN_DB: f32 = 4.0;

// Speech confidence thresholds
const SPEECH_ON_THRESHOLD: f32 = 0.6;
const SPEECH_OFF_THRESHOLD: f32 = 0.3;

/// Recovery stage with speech-gated presence and air shelving EQ
pub struct RecoveryStage {
    presence_shelf_l: Biquad,
    presence_shelf_r: Biquad,
    air_shelf_l: Biquad,
    air_shelf_r: Biquad,

    sample_rate: f32,
    last_presence_gain: f32,
    last_air_gain: f32,

    // Ballistic envelope (smoothed recovery activity, 0 … 1)
    gain_smooth: f32,
    attack_coeff: f32,
    release_coeff: f32,
}

impl RecoveryStage {
    pub fn new(sample_rate: f32) -> Self {
        let mut presence_shelf_l = Biquad::new();
        let mut presence_shelf_r = Biquad::new();
        let mut air_shelf_l = Biquad::new();
        let mut air_shelf_r = Biquad::new();

        // Initialize filters flat
        presence_shelf_l.update_high_shelf(PRESENCE_FREQ_HZ, Q, 0.0, sample_rate);
        presence_shelf_r.update_high_shelf(PRESENCE_FREQ_HZ, Q, 0.0, sample_rate);
        air_shelf_l.update_high_shelf(AIR_FREQ_HZ, Q, 0.0, sample_rate);
        air_shelf_r.update_high_shelf(AIR_FREQ_HZ, Q, 0.0, sample_rate);

        Self {
            presence_shelf_l,
            presence_shelf_r,
            air_shelf_l,
            air_shelf_r,
            sample_rate,
            last_presence_gain: 0.0,
            last_air_gain: 0.0,
            gain_smooth: 0.0,
            attack_coeff: time_constant_coeff(ATTACK_MS, sample_rate),
            release_coeff: time_constant_coeff(RELEASE_MS, sample_rate),
        }
    }

    /// Process a stereo sample pair with speech-gated recovery EQ
    ///
    /// * `left`, `right` - Input samples
    /// * `speech_confidence` - Speech confidence from detector (0.0-1.0)
    ///
    /// Returns (processed_left, processed_right)
    pub fn process(
        &mut self,
        left: f32,
        right: f32,
        speech_confidence: f32,
        integrity_score: f32,
    ) -> (f32, f32) {
        let conf = speech_confidence.clamp(0.0, 1.0);
        let integrity = integrity_score.clamp(0.0, 1.0);

        // Raw target activity: 0 at conf ≤ 0.3, ramp to 1 at conf ≥ 0.6, scaled by integrity
        let target_activity = if conf >= SPEECH_ON_THRESHOLD {
            integrity
        } else if conf <= SPEECH_OFF_THRESHOLD {
            0.0
        } else {
            let ramp = (conf - SPEECH_OFF_THRESHOLD) / (SPEECH_ON_THRESHOLD - SPEECH_OFF_THRESHOLD);
            ramp * integrity
        };

        // Ballistic envelope: slow attack prevents brightness pumping,
        // faster release prevents dull hangover after speech ends.
        if target_activity > self.gain_smooth {
            self.gain_smooth =
                self.attack_coeff * self.gain_smooth + (1.0 - self.attack_coeff) * target_activity;
        } else {
            self.gain_smooth = self.release_coeff * self.gain_smooth
                + (1.0 - self.release_coeff) * target_activity;
        }

        // Target gains scale from 0 dB at gain_smooth = 0 to MIN…MAX at gain_smooth = 1.
        // Ensures filters return to flat during silence — no silence lift, no discontinuity.
        let base_presence =
            MIN_PRESENCE_GAIN_DB + (MAX_PRESENCE_GAIN_DB - MIN_PRESENCE_GAIN_DB) * integrity;
        let target_presence_gain = self.gain_smooth * base_presence;

        let base_air = MIN_AIR_GAIN_DB + (MAX_AIR_GAIN_DB - MIN_AIR_GAIN_DB) * integrity;
        let target_air_gain = self.gain_smooth * base_air;

        // Update filter coefficients only when gain crosses 0.05 dB steps
        if (target_presence_gain - self.last_presence_gain).abs() > 0.05 {
            self.presence_shelf_l.update_high_shelf(
                PRESENCE_FREQ_HZ,
                Q,
                target_presence_gain,
                self.sample_rate,
            );
            self.presence_shelf_r.update_high_shelf(
                PRESENCE_FREQ_HZ,
                Q,
                target_presence_gain,
                self.sample_rate,
            );
            self.last_presence_gain = target_presence_gain;
        }

        if (target_air_gain - self.last_air_gain).abs() > 0.05 {
            self.air_shelf_l
                .update_high_shelf(AIR_FREQ_HZ, Q, target_air_gain, self.sample_rate);
            self.air_shelf_r
                .update_high_shelf(AIR_FREQ_HZ, Q, target_air_gain, self.sample_rate);
            self.last_air_gain = target_air_gain;
        }

        // Always run through filters.  At 0 dB they are identity — no early-return
        // discontinuity, no stale filter state on re-entry.
        let out_l = self
            .air_shelf_l
            .process(self.presence_shelf_l.process(left));
        let out_r = self
            .air_shelf_r
            .process(self.presence_shelf_r.process(right));

        (out_l, out_r)
    }

    pub fn reset(&mut self) {
        self.presence_shelf_l.reset();
        self.presence_shelf_r.reset();
        self.air_shelf_l.reset();
        self.air_shelf_r.reset();
        self.last_presence_gain = 0.0;
        self.last_air_gain = 0.0;
        self.gain_smooth = 0.0;
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.attack_coeff = time_constant_coeff(ATTACK_MS, sample_rate);
        self.release_coeff = time_constant_coeff(RELEASE_MS, sample_rate);
        self.reset();
    }
}
