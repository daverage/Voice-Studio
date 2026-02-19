//! Post-Noise Cleanup (Second-pass, very light)
//!
//! A gentle, confidence-gated attenuation stage used after shaping to tuck
//! any noise revealed by tone recovery. This is intentionally subtle:
//! - Only engages when speech confidence is low (primary path)
//! - Falls back to envelope-based gating if confidence is flatlined
//! - Reduction is capped to a few dB
//! - Includes a soft HF-weighted option to focus on recovered hiss

use crate::dsp::biquad::Biquad;
use crate::dsp::utils::{db_to_gain, lerp, smoothstep, time_constant_coeff};

const CONF_LOW: f32 = 0.18;
const CONF_HIGH: f32 = 0.38;

const MAX_REDUCTION_DB: f32 = 2.5;
const HF_SHELF_CUTOFF_HZ: f32 = 4500.0;
const HF_SHELF_Q: f32 = 0.7;
const HF_SHELF_BLEND: f32 = 0.6;
const HF_SHELF_SCALE: f32 = 0.9;

const ATTACK_MS: f32 = 6.0;
const RELEASE_MS: f32 = 90.0;
const HOLD_MS: f32 = 25.0;

const CONF_FLATLINE_EPS: f32 = 0.001;
const CONF_FLATLINE_SEC: f32 = 2.0;

const ENV_SNR_LOW: f32 = 1.4;
const ENV_SNR_HIGH: f32 = 3.0;

pub struct PostNoiseCleanup {
    sample_rate: f32,
    gain: f32,
    last_gate: f32,
    hold_samples: usize,
    hold_samples_total: usize,
    attack_coeff: f32,
    release_coeff: f32,
    prev_conf: f32,
    flatline_samples: usize,
    flatline_samples_total: usize,
    hf_shelf_l: Biquad,
    hf_shelf_r: Biquad,
    last_shelf_db: f32,
}

impl PostNoiseCleanup {
    pub fn new(sample_rate: f32) -> Self {
        let mut s = Self {
            sample_rate,
            gain: 1.0,
            last_gate: 0.0,
            hold_samples: 0,
            hold_samples_total: 1,
            attack_coeff: 0.0,
            release_coeff: 0.0,
            prev_conf: 0.0,
            flatline_samples: 0,
            flatline_samples_total: 1,
            hf_shelf_l: Biquad::new(),
            hf_shelf_r: Biquad::new(),
            last_shelf_db: 0.0,
        };
        s.prepare(sample_rate);
        s
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.attack_coeff = time_constant_coeff(ATTACK_MS, self.sample_rate);
        self.release_coeff = time_constant_coeff(RELEASE_MS, self.sample_rate);
        self.hold_samples_total = (HOLD_MS * 0.001 * self.sample_rate).round().max(1.0) as usize;
        self.flatline_samples_total =
            (CONF_FLATLINE_SEC * self.sample_rate).round().max(1.0) as usize;
        self.hf_shelf_l
            .update_high_shelf(HF_SHELF_CUTOFF_HZ, HF_SHELF_Q, 0.0, self.sample_rate);
        self.hf_shelf_r
            .update_high_shelf(HF_SHELF_CUTOFF_HZ, HF_SHELF_Q, 0.0, self.sample_rate);
        self.last_shelf_db = 0.0;
    }

    pub fn reset(&mut self) {
        self.gain = 1.0;
        self.last_gate = 0.0;
        self.hold_samples = 0;
        self.prev_conf = 0.0;
        self.flatline_samples = 0;
        self.hf_shelf_l.reset();
        self.hf_shelf_r.reset();
        self.last_shelf_db = 0.0;
    }

    #[inline]
    pub fn process_sample(
        &mut self,
        input: f32,
        speech_conf: f32,
        env_rms: f32,
        env_noise_floor: f32,
        amount: f32,
        use_hf_bias: bool,
        is_left: bool,
    ) -> f32 {
        let amt = amount.clamp(0.0, 1.0);
        if amt < 1e-4 {
            self.gain = 1.0;
            return input;
        }

        let conf_delta = (speech_conf - self.prev_conf).abs();
        if conf_delta < CONF_FLATLINE_EPS {
            self.flatline_samples = (self.flatline_samples + 1).min(self.flatline_samples_total);
        } else {
            self.flatline_samples = 0;
        }
        self.prev_conf = speech_conf;
        let conf_valid = self.flatline_samples < self.flatline_samples_total;

        let gate = if conf_valid {
            1.0 - smoothstep(CONF_LOW, CONF_HIGH, speech_conf)
        } else {
            let snr = env_rms / (env_noise_floor + 1e-9);
            1.0 - smoothstep(ENV_SNR_LOW, ENV_SNR_HIGH, snr)
        };

        let mut gate = gate.clamp(0.0, 1.0);
        if gate > 0.01 {
            self.hold_samples = self.hold_samples_total;
            self.last_gate = gate;
        } else if self.hold_samples > 0 {
            self.hold_samples -= 1;
            gate = self.last_gate;
        } else {
            self.last_gate = 0.0;
        }

        let max_db = lerp(0.0, MAX_REDUCTION_DB, amt);
        let target_db = -max_db * gate;
        let target_gain = db_to_gain(target_db);

        if target_gain < self.gain {
            self.gain = self.gain * self.attack_coeff + target_gain * (1.0 - self.attack_coeff);
        } else {
            self.gain = self.gain * self.release_coeff + target_gain * (1.0 - self.release_coeff);
        }

        let broad = input * self.gain;
        if !use_hf_bias {
            return broad;
        }

        let shelf_db = -max_db * gate * HF_SHELF_SCALE;
        if (shelf_db - self.last_shelf_db).abs() > 0.02 {
            if is_left {
                self.hf_shelf_l.update_high_shelf(
                    HF_SHELF_CUTOFF_HZ,
                    HF_SHELF_Q,
                    shelf_db,
                    self.sample_rate,
                );
            } else {
                self.hf_shelf_r.update_high_shelf(
                    HF_SHELF_CUTOFF_HZ,
                    HF_SHELF_Q,
                    shelf_db,
                    self.sample_rate,
                );
            }
            self.last_shelf_db = shelf_db;
        }

        let shelf = if is_left {
            self.hf_shelf_l.process(broad)
        } else {
            self.hf_shelf_r.process(broad)
        };

        lerp(broad, shelf, HF_SHELF_BLEND)
    }
}
