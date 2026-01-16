use crate::dsp::utils::{db_to_gain, lin_to_db, smoothstep, DB_EPS};
use crate::dsp::Biquad;

// Constants: unless marked "Must not change", these are tunable for behavior.

// Sibilance band high-pass cutoff (Hz).
// Increasing: targets brighter sibilance; decreasing: includes more upper mids.
const SIBILANCE_HPF_HZ: f32 = 4500.0;
// Sibilance band filter Q (Butterworth-ish).
// Increasing: narrower band; decreasing: wider band.
const SIBILANCE_FILTER_Q: f32 = 0.707;
// Sibilance band low-pass cutoff (Hz).
// Increasing: includes more air; decreasing: tighter sibilance focus.
const SIBILANCE_LPF_HZ: f32 = 10_000.0;
// HF splitter low-pass cutoff (Hz) for "non-sibilant" reference.
// Increasing: broader HF reference; decreasing: narrower reference.
const HF_SPLIT_LPF_HZ: f32 = 3500.0;
// HF splitter filter Q (Butterworth-ish).
// Increasing: narrower split; decreasing: wider split.
const HF_SPLIT_Q: f32 = 0.707;
// Level envelope attack (seconds).
// Increasing: slower level tracking; decreasing: faster tracking.
const LEVEL_ATTACK_SEC: f32 = 0.005;
// Level envelope release (seconds).
// Increasing: smoother decay; decreasing: faster decay.
const LEVEL_RELEASE_SEC: f32 = 0.200;
// Sibilance detector attack (seconds).
// Increasing: slower sibilance pickup; decreasing: faster pickup.
const SIB_ATTACK_SEC: f32 = 0.0004;
// Sibilance detector release (seconds).
// Increasing: smoother decay; decreasing: faster decay.
const SIB_RELEASE_SEC: f32 = 0.060;
// ZCR smoothing time (seconds) for unvoiced weighting.
// Increasing: smoother unvoiced detection; decreasing: more reactive.
const ZC_SMOOTH_SEC: f32 = 0.030;
// Unvoiced ZCR thresholds.
// Increasing min/max: requires more ZCR to mark unvoiced; decreasing: easier to trigger.
const UNVOICED_ZC_MIN: f32 = 0.02;
const UNVOICED_ZC_MAX: f32 = 0.10;
// HF energy weighting in focus calculation.
// Increasing: reduces focus for wideband HF; decreasing: favors sibilance focus.
const FOCUS_HF_WEIGHT: f32 = 0.6;
// Focus curve thresholds.
// Increasing: sibilance focus triggers later; decreasing: earlier.
const FOCUS_MIN: f32 = 0.30;
const FOCUS_MAX: f32 = 0.75;
// Threshold scale relative to level envelope.
// Increasing: harder to trigger de-essing; decreasing: more sensitive.
const LEVEL_THRESH_SCALE: f32 = 0.12;
// Minimum threshold to avoid log(0).
// Increasing: more conservative at very low levels; decreasing: closer to raw.
const LEVEL_THRESH_MIN: f32 = 1e-6;
// De-esser ratio used to compute target reduction.
// Increasing: stronger reduction per dB over; decreasing: gentler.
const DE_ESS_RATIO: f32 = 6.0;
// Maximum reduction in dB at amount=1.0 (DSP contract).
// Must not change: UI amount maps to 18 dB maximum reduction.
const MAX_REDUCTION_DB: f32 = 18.0;
// Minimum allowed gain from the detector (linear).
// Increasing: limits max reduction; decreasing: allows deeper cuts.
const MIN_GAIN: f32 = 0.10;
// Gain smoothing attack (seconds).
// Increasing: slower gain reduction onset; decreasing: faster onset.
const GAIN_ATTACK_SEC: f32 = 0.0015;
// Gain smoothing release (seconds).
// Increasing: slower recovery; decreasing: faster recovery.
const GAIN_RELEASE_SEC: f32 = 0.080;
// De-esser band frequency (Hz).
// Increasing: higher notch; decreasing: lower notch.
const DE_ESS_BAND_HZ: f32 = 7000.0;
// De-esser band Q.
// Increasing: narrower notch; decreasing: broader notch.
const DE_ESS_BAND_Q: f32 = 1.0;
// Amount below which the de-esser is bypassed (slightly higher threshold for de-esser
// to account for its sensitivity to small amounts of processing).
// Increasing: easier to bypass; decreasing: more likely to process.
const DE_ESSER_BYPASS_EPS: f32 = 0.01;
// Input floor to avoid denorm/zero in sibilance analysis.
// Increasing: more conservative at silence; decreasing: closer to raw.
const INPUT_FLOOR: f32 = 1e-25;
// Gain threshold for bypassing the de-esser band filter.
// Increasing: less likely to bypass; decreasing: more likely to bypass.
const BAND_BYPASS_GAIN_EPS: f32 = 0.999;

/// Shared detector for stereo-linked de-essing.
pub struct DeEsserDetector {
    sib_hpf: Biquad,
    sib_lpf: Biquad,

    hf_lpf: Biquad,

    level_env: f32,
    sib_env: f32,
    gain_smooth: f32,

    prev_sib: f32,
    zc_env: f32,

    sample_rate: f32,
    last_sibilance_weight: f32,
    last_over_db: f32,
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

            level_env: 0.0,
            sib_env: 0.0,
            gain_smooth: 1.0,

            prev_sib: 0.0,
            zc_env: 0.0,

            sample_rate: sr,
            last_sibilance_weight: 0.0,
            last_over_db: 0.0,
        }
    }

    fn analyze_sibilance_weight(&mut self, input_l: f32, input_r: f32) -> f32 {
        // ------------------------------------------------------------
        // 1) Stereo-linked input (conservative)
        // ------------------------------------------------------------
        let x = input_l.abs().max(input_r.abs()) + INPUT_FLOOR;

        // ------------------------------------------------------------
        // 2) Extract sibilance band (linked)
        // ------------------------------------------------------------
        let sib = self.sib_lpf.process(self.sib_hpf.process(x));
        let sib_abs = sib.abs();

        // ------------------------------------------------------------
        // 3) Broad HF energy (linked)
        // ------------------------------------------------------------
        let low = self.hf_lpf.process(x);
        let hf = x - low;
        let hf_abs = hf.abs();

        // ------------------------------------------------------------
        // 4) Broadband level envelope (linked)
        // ------------------------------------------------------------
        let level_attack = (-1.0 / (LEVEL_ATTACK_SEC * self.sample_rate)).exp();
        let level_release = (-1.0 / (LEVEL_RELEASE_SEC * self.sample_rate)).exp();

        if x > self.level_env {
            self.level_env = level_attack * self.level_env + (1.0 - level_attack) * x;
        } else {
            self.level_env = level_release * self.level_env + (1.0 - level_release) * x;
        }

        // ------------------------------------------------------------
        // 5) Sibilance detector envelope
        // ------------------------------------------------------------
        let det_attack = (-1.0 / (SIB_ATTACK_SEC * self.sample_rate)).exp();
        let det_release = (-1.0 / (SIB_RELEASE_SEC * self.sample_rate)).exp();

        if sib_abs > self.sib_env {
            self.sib_env = det_attack * self.sib_env + (1.0 - det_attack) * sib_abs;
        } else {
            self.sib_env = det_release * self.sib_env + (1.0 - det_release) * sib_abs;
        }

        // ------------------------------------------------------------
        // 6) Unvoiced weighting (linked ZCR)
        // ------------------------------------------------------------
        let sign = sib.signum();
        let prev_sign = self.prev_sib.signum();
        let zc = if sign != prev_sign { 1.0 } else { 0.0 };
        self.prev_sib = sib;

        let zc_smooth = (-1.0 / (ZC_SMOOTH_SEC * self.sample_rate)).exp();
        self.zc_env = zc_smooth * self.zc_env + (1.0 - zc_smooth) * zc;

        let unvoiced = smoothstep(UNVOICED_ZC_MIN, UNVOICED_ZC_MAX, self.zc_env).clamp(0.0, 1.0);

        // ------------------------------------------------------------
        // 7) Focus weighting (sib vs HF)
        // ------------------------------------------------------------
        let focus =
            (self.sib_env / (self.sib_env + (hf_abs * FOCUS_HF_WEIGHT) + DB_EPS)).clamp(0.0, 1.0);
        let focus_w = smoothstep(FOCUS_MIN, FOCUS_MAX, focus);

        (unvoiced * focus_w).clamp(0.0, 1.0)
    }

    /// amount: 0..1
    pub fn compute_gain(&mut self, input_l: f32, input_r: f32, amount: f32) -> f32 {
        let amount = amount.clamp(0.0, 1.0);
        let sibilance_weight = self.analyze_sibilance_weight(input_l, input_r);
        self.last_sibilance_weight = sibilance_weight;

        // ------------------------------------------------------------
        // 8) Level-relative threshold
        // ------------------------------------------------------------
        let threshold = (self.level_env * LEVEL_THRESH_SCALE).max(LEVEL_THRESH_MIN);

        let env_db = lin_to_db(self.sib_env.max(LEVEL_THRESH_MIN));
        let thr_db = lin_to_db(threshold);
        let over_db = (env_db - thr_db).max(0.0);
        self.last_over_db = over_db;

        if amount < DE_ESSER_BYPASS_EPS {
            self.gain_smooth = 1.0;
            return 1.0;
        }

        let ratio = DE_ESS_RATIO;
        let max_red_db = MAX_REDUCTION_DB * amount;

        let target_red_db = (over_db * (ratio - 1.0)).min(max_red_db);
        let target_gain_db = -target_red_db * sibilance_weight;
        let target_gain = db_to_gain(target_gain_db).clamp(MIN_GAIN, 1.0);

        // ------------------------------------------------------------
        // 9) Gain smoothing
        // ------------------------------------------------------------
        let g_attack = (-1.0 / (GAIN_ATTACK_SEC * self.sample_rate)).exp();
        let g_release = (-1.0 / (GAIN_RELEASE_SEC * self.sample_rate)).exp();

        if target_gain < self.gain_smooth {
            self.gain_smooth = g_attack * self.gain_smooth + (1.0 - g_attack) * target_gain;
        } else {
            self.gain_smooth = g_release * self.gain_smooth + (1.0 - g_release) * target_gain;
        }

        self.gain_smooth
    }
}

/// Per-channel application band
pub struct DeEsserBand {
    filter: Biquad,
    sample_rate: f32,
}

impl DeEsserBand {
    pub fn new(sample_rate: f32) -> Self {
        let mut filter = Biquad::new();
        // Dynamic peaking cut around 7kHz
        filter.update_peaking(DE_ESS_BAND_HZ, DE_ESS_BAND_Q, 0.0, sample_rate);
        Self {
            filter,
            sample_rate,
        }
    }

    pub fn apply(&mut self, input: f32, gain: f32) -> f32 {
        // Always run the filter to keep state updated (prevents pops on engage)
        let cut_db = if gain > BAND_BYPASS_GAIN_EPS {
            0.0
        } else {
            lin_to_db(gain)
        };
        
        self.filter
            .update_peaking(DE_ESS_BAND_HZ, DE_ESS_BAND_Q, cut_db, self.sample_rate);
        self.filter.process(input)
    }

}

impl DeEsserDetector {
    pub fn sibilance_metric(&self) -> f32 {
        self.last_sibilance_weight
    }

    pub fn sibilance_over_db(&self) -> f32 {
        self.last_over_db
    }
}
