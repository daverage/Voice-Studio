//! De-Esser (Sibilance Reducer)
//!
//! A dynamic equalizer designed to reduce harsh sibilant sounds (s, sh, ch) that occur
//! in vocal recordings. Uses frequency-domain analysis to detect sibilance and applies
//! targeted reduction in the 4.5-10kHz range.
//!
//! # Purpose
//! Controls harsh sibilant sounds without affecting the overall tonal balance of the voice.
//! Operates as part of the dynamics processing stage in the signal chain.
//!
//! # Design Notes
//! - Uses dual-band detection to distinguish sibilance from other high-frequency content
//! - Applies reduction only when sibilance is detected above the threshold
//! - Maintains natural consonant sounds while reducing harshness

use crate::dsp::envelope::VoiceEnvelope;
use crate::dsp::utils::{db_to_gain, lin_to_db, smoothstep, DB_EPS};
use crate::dsp::Biquad;

// ---------------- Constants ----------------

const SIBILANCE_HPF_HZ: f32 = 4500.0;
const SIBILANCE_FILTER_Q: f32 = 0.707;
const SIBILANCE_LPF_HZ: f32 = 10_000.0;

const HF_SPLIT_LPF_HZ: f32 = 3500.0;
const HF_SPLIT_Q: f32 = 0.707;

const SIB_ATTACK_SEC: f32 = 0.0004;
const SIB_RELEASE_SEC: f32 = 0.060;

const ZC_SMOOTH_SEC: f32 = 0.030;
const UNVOICED_ZC_MIN: f32 = 0.02;
const UNVOICED_ZC_MAX: f32 = 0.10;

const FOCUS_HF_WEIGHT: f32 = 0.6;
const FOCUS_MIN: f32 = 0.30;
const FOCUS_MAX: f32 = 0.75;

const LEVEL_THRESH_SCALE: f32 = 0.12;
const LEVEL_THRESH_MIN: f32 = 1e-6;
const LEVEL_THRESH_DB_FLOOR: f32 = -45.0;

const DE_ESS_RATIO: f32 = 6.0;
const MAX_REDUCTION_DB: f32 = 24.0;
const MIN_GAIN: f32 = 0.10;

const GAIN_ATTACK_SEC: f32 = 0.0015;
const GAIN_RELEASE_SEC: f32 = 0.080;

const DE_ESS_BAND_HZ: f32 = 7000.0;
const DE_ESS_BAND_Q: f32 = 1.0;

const DE_ESSER_BYPASS_EPS: f32 = 0.01;
const INPUT_FLOOR: f32 = 1e-10;

// ---------------- Detector ----------------

pub struct DeEsserDetector {
    sib_hpf: Biquad,
    sib_lpf: Biquad,
    hf_lpf: Biquad,

    sib_env: f32,
    gain_smooth: f32,

    prev_hf: f32,
    zc_env: f32,

    sample_rate: f32,

    pub last_sibilance_weight: f32,
    pub last_over_db: f32,
    pub last_reduction_db: f32,
}

impl DeEsserDetector {
    pub fn new(sr: f32) -> Self {
        let mut sib_hpf = Biquad::new();
        sib_hpf.update_hpf(SIBILANCE_HPF_HZ, SIBILANCE_FILTER_Q, sr);

        let mut sib_lpf = Biquad::new();
        sib_lpf.update_lpf(SIBILANCE_LPF_HZ, SIBILANCE_FILTER_Q, sr);

        let mut hf_lpf = Biquad::new();
        hf_lpf.update_lpf(HF_SPLIT_LPF_HZ, HF_SPLIT_Q, sr);

        Self {
            sib_hpf,
            sib_lpf,
            hf_lpf,
            sib_env: 0.0,
            gain_smooth: 1.0,
            prev_hf: 0.0,
            zc_env: 0.0,
            sample_rate: sr,
            last_sibilance_weight: 0.0,
            last_over_db: 0.0,
            last_reduction_db: 0.0,
        }
    }

    fn analyze_sibilance_weight(&mut self, x: f32) -> f32 {
        let sib = self.sib_lpf.process(self.sib_hpf.process(x));
        let sib_abs = sib.abs();

        let low = self.hf_lpf.process(x);
        let hf = x - low;
        let hf_abs = hf.abs();

        let atk = (-1.0 / (SIB_ATTACK_SEC * self.sample_rate)).exp();
        let rel = (-1.0 / (SIB_RELEASE_SEC * self.sample_rate)).exp();
        self.sib_env = if sib_abs > self.sib_env {
            atk * self.sib_env + (1.0 - atk) * sib_abs
        } else {
            rel * self.sib_env + (1.0 - rel) * sib_abs
        };

        let sign = hf.signum();
        let prev = self.prev_hf.signum();
        let zc = if sign != prev { 1.0 } else { 0.0 };
        self.prev_hf = hf;

        let zc_s = (-1.0 / (ZC_SMOOTH_SEC * self.sample_rate)).exp();
        self.zc_env = zc_s * self.zc_env + (1.0 - zc_s) * zc;

        let unvoiced = smoothstep(UNVOICED_ZC_MIN, UNVOICED_ZC_MAX, self.zc_env).clamp(0.0, 1.0);

        let focus =
            (self.sib_env / (self.sib_env + hf_abs * FOCUS_HF_WEIGHT + DB_EPS)).clamp(0.0, 1.0);
        let focus_w = smoothstep(FOCUS_MIN, FOCUS_MAX, focus);

        (unvoiced * focus_w).clamp(0.0, 1.0)
    }

    pub fn compute_gain(
        &mut self,
        l: f32,
        r: f32,
        amount: f32,
        env_l: &VoiceEnvelope,
        env_r: &VoiceEnvelope,
    ) -> f32 {
        let amount = amount.clamp(0.0, 1.0);
        let x = l.abs().max(r.abs()) + INPUT_FLOOR;

        let weight = self.analyze_sibilance_weight(x);
        self.last_sibilance_weight = weight;

        // Use shared slow envelope (max of L/R) for level threshold
        let level_env = env_l.slow.max(env_r.slow);
        let lin_thr = (level_env * LEVEL_THRESH_SCALE).max(LEVEL_THRESH_MIN);
        let thr_db = lin_to_db(lin_thr).max(LEVEL_THRESH_DB_FLOOR);

        let env_db = lin_to_db(self.sib_env.max(LEVEL_THRESH_MIN));
        let over_db = (env_db - thr_db).max(0.0);
        self.last_over_db = over_db;

        if amount < DE_ESSER_BYPASS_EPS {
            let rel = (-1.0 / (GAIN_RELEASE_SEC * self.sample_rate)).exp();
            self.gain_smooth = rel * self.gain_smooth + (1.0 - rel);
            return self.gain_smooth;
        }

        let knee = smoothstep(0.0, 6.0, over_db);
        let target_red = (knee * over_db * (DE_ESS_RATIO - 1.0)).min(MAX_REDUCTION_DB * amount);

        let target_gain = db_to_gain(-target_red * weight).clamp(MIN_GAIN, 1.0);

        let atk = (-1.0 / (GAIN_ATTACK_SEC * self.sample_rate)).exp();
        let rel = (-1.0 / (GAIN_RELEASE_SEC * self.sample_rate)).exp();

        self.gain_smooth = if target_gain < self.gain_smooth {
            atk * self.gain_smooth + (1.0 - atk) * target_gain
        } else {
            rel * self.gain_smooth + (1.0 - rel) * target_gain
        };

        self.last_reduction_db = -lin_to_db(self.gain_smooth).max(-MAX_REDUCTION_DB);

        self.gain_smooth
    }

    pub fn get_gain_reduction_db(&self) -> f32 {
        self.last_reduction_db
    }

    pub fn reset(&mut self) {
        self.last_reduction_db = 0.0;
    }
}

// ---------------- Band ----------------

pub struct DeEsserBand {
    filter: Biquad,
    last_cut_db: f32,
    sample_rate: f32,
}

impl DeEsserBand {
    pub fn new(sr: f32) -> Self {
        let mut filter = Biquad::new();
        filter.update_peaking(DE_ESS_BAND_HZ, DE_ESS_BAND_Q, 0.0, sr);
        Self {
            filter,
            last_cut_db: 0.0,
            sample_rate: sr,
        }
    }

    pub fn update(&mut self, gain: f32) {
        let cut_db = lin_to_db(gain).max(-MAX_REDUCTION_DB);
        if (cut_db - self.last_cut_db).abs() > 0.1 {
            self.filter
                .update_peaking(DE_ESS_BAND_HZ, DE_ESS_BAND_Q, cut_db, self.sample_rate);
            self.last_cut_db = cut_db;
        }
    }

    pub fn process(&mut self, x: f32) -> f32 {
        self.filter.process(x)
    }

    pub fn apply(&mut self, sample: f32, gain: f32) -> f32 {
        self.update(gain);
        self.process(sample)
    }
}
