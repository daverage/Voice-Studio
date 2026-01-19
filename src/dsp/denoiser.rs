//! Spectral Denoiser (Wiener + ML Advisor)
//!
//! # Perceptual Contract
//! - **Target Source**: Spoken voice (podcast, voice-over, meeting). Not for music or singing.
//! - **Intended Effect**: Reduce stationary background noise (hiss, hum, fan noise) while preserving voice timbre.
//! - **Failure Modes**:
//!   - "Musical noise" / "space monkeys" if reduction is too aggressive.
//!   - Voice thinning if noise floor estimation tracks speech.
//!   - High-frequency pumping if release time is too fast.
//! - **Will Not Do**:
//!   - Remove non-stationary noise like dog barks, sirens, or keyboard clicks.
//!   - De-clip or de-crackle.
//!
//! # Lifecycle
//! - **Learning**: Active during first 500ms to estimate initial noise floor.
//! - **Active**: Normal operation.
//! - **Holding**: Not explicitly used (noise floor has its own internal ballistics).
//! - **Bypassed**: Passes audio through, but may continue to update noise estimator (background learning).
//!
//! # Noise Reduction Model (Decision-Directed Wiener Filter)
//! This module implements a decision-directed Wiener filtering approach (Ephraim & Malah style):
//!
//! 1. **Statistics**: Uses STFT magnitude estimation with a shared mono proxy.
//! 2. **Noise Floor**: Tracked using minimum statistics with speech-conditioned ballistics.
//! 3. **Speech Presence (ML Advisor)**: Per-bin speech probability masks from `MlDenoiseEngine`.
//! 4. **Wiener Gain**: Decision-directed estimation of a-priori SNR (xi) used to build the Wiener gain curve.
//! 5. **Adaptive Masking**: Heuristic psychoacoustic masking based on spectral peaks.
//!
//! # Assumptions
//! - Background noise is mostly stationary or slowly varying.
//! - Speech is characterized by harmonic structure (voiced) or broadband high-frequency transients (unvoiced).
//! - Impulse noise and non-stationary transients are NOT modeled.

#[cfg(feature = "ml")]
use crate::dsp::ml_denoise::MlDenoiseEngine;
use crate::dsp::utils::{
    bell, db_to_gain, estimate_f0_autocorr, frame_rms, lerp, make_sqrt_hann_window, smoothstep,
    BYPASS_AMOUNT_EPS, MAG_FLOOR,
};
use ringbuf::{Consumer, Producer, RingBuffer};
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::sync::Arc;

// Constants: unless marked "Must not change", these are tunable for behavior.

// Minimum allowed window size.
const WIN_SIZE_MIN: usize = 64;
// Ring buffer capacity multiplier relative to window size.
const RINGBUF_CAP_MULT: usize = 4;
// Nyquist fraction used for normalization (fs/2).
const NYQUIST_FRAC: f32 = 0.5;
// Coarse FFT window divisor.
const COARSE_WIN_DIV: usize = 2;
// Minimum coarse FFT size.
const COARSE_WIN_MIN: usize = 256;
// Initial noise floor estimate.
const NOISE_FLOOR_INIT: f32 = 1e-5;
// Minimum sample rate used for analysis guard.
const SAMPLE_RATE_MIN: f32 = 8000.0;
// Amount threshold for enabling hum removal in analysis.
const HUM_REMOVAL_AMOUNT_THRESH: f32 = 0.05;
// Hum removal main-bin attenuation.
const HUM_REMOVAL_MAIN_SCALE: f32 = 0.1;
// Hum removal side-bin attenuation.
const HUM_REMOVAL_SIDE_SCALE: f32 = 0.5;
// Low-cut frequency for analysis hum removal (Hz).
const HUM_REMOVAL_LOW_CUT_HZ: f32 = 25.0;
// Hum removal target frequencies (Hz).
const HUM_REMOVAL_FREQS: [f32; 6] = [50.0, 60.0, 100.0, 120.0, 150.0, 180.0];
// Startup threshold to detect uninitialized noise floor.
const NOISE_STARTUP_THRESH: f32 = 1e-4;
// Startup noise floor attack/release.
const NOISE_STARTUP_ATT: f32 = 0.6;
const NOISE_STARTUP_REL: f32 = 0.90;
// Base noise floor attack/release.
const NOISE_ATT_BASE: f32 = 0.90;
const NOISE_REL_BASE: f32 = 0.9995;
// Maximum noise floor attack/release.
const NOISE_ATT_MAX: f32 = 0.98;
const NOISE_REL_MAX: f32 = 0.99995;
// Noise model protection weights.
const NOISE_PROTECT_BASE: f32 = 0.35;
const NOISE_PROTECT_RANGE: f32 = 0.55;
// Coarse noise floor attack/release.
const NOISE_COARSE_ATT: f32 = 0.92;
const NOISE_COARSE_REL: f32 = 0.999;
// Tone bias in dB for tilt.
const TONE_BIAS_DB: f32 = 6.0;
// Tone split pivot.
const TONE_SPLIT: f32 = 0.5;
// Tone scaling factor for mapping 0..1 to -1..1.
const TONE_SCALE: f32 = 2.0;
// Voiced speech weighting base/range.
const VOICED_SPEECH_BASE: f32 = 0.35;
const VOICED_SPEECH_RANGE: f32 = 0.65;
// Unvoiced speech weighting base/range.
const UNVOICED_SPEECH_BASE: f32 = 0.25;
const UNVOICED_SPEECH_RANGE: f32 = 0.75;
// Voiced mid-band bell parameters.
const VOICED_MID_CENTER: f32 = 0.22;
const VOICED_MID_WIDTH: f32 = 0.20;
// HF band gate for unvoiced weighting.
const UNVOICED_HF_MIN: f32 = 0.18;
const UNVOICED_HF_MAX: f32 = 0.55;
// Voiced probability threshold.
const VOICED_PROB_MIN: f32 = 0.55;
// Sensitivity scale for noise threshold.
const THRESH_SENS_SCALE: f32 = 5.0;
// Speech-dependent threshold boost.
const SPEECH_THRESH_SCALE: f32 = 1.25;
// Power for raw gain depth shaping.
const RAW_GAIN_POWER: f32 = 2.0;
// Strength shaping range.
const STRENGTH_MIN: f32 = 1.0;
const STRENGTH_MAX: f32 = 3.0;
// Psychoacoustic floor shaping.
const PSYCHO_FLOOR_BASE: f32 = 0.25;
const PSYCHO_FLOOR_RANGE: f32 = 0.65;
const PSYCHO_FLOOR_MIN: f32 = 0.10;
const PSYCHO_FLOOR_MAX: f32 = 0.95;
// Speech floor shaping.
const SPEECH_FLOOR_BASE: f32 = 0.30;
const SPEECH_FLOOR_RANGE: f32 = 0.60;
const SPEECH_FLOOR_MIN: f32 = 0.15;
const SPEECH_FLOOR_MAX: f32 = 0.98;
// Floor scaling with amount.
const FLOOR_SCALE_MIN: f32 = 0.35;
const SPEECH_FLOOR_SCALE_MIN: f32 = 0.60;
// Spectral smoothing strength for voiced/unvoiced.
const SMOOTH_STRENGTH_VOICED: f32 = 0.55;
const SMOOTH_STRENGTH_UNVOICED: f32 = 0.75;
// Temporal release limit range.
const RELEASE_LIMIT_MIN: f32 = 0.85;
const RELEASE_LIMIT_MAX: f32 = 0.92;
// Harmonic protection f0 range (Hz).
const HARMONIC_F0_MIN_HZ: f32 = 50.0;
const HARMONIC_F0_MAX_HZ: f32 = 450.0;
// Harmonic protection max frequency (Hz).
const HARMONIC_MAX_HZ: f32 = 8000.0;
// Harmonic protection base/range.
const HARMONIC_PROTECT_BASE: f32 = 0.55;
const HARMONIC_PROTECT_RANGE: f32 = 0.40;
// Harmonic allow reduction scale.
const HARMONIC_ALLOW_SCALE: f32 = 0.65;
const HARMONIC_ALLOW_MIN: f32 = 0.25;
const HARMONIC_ALLOW_MAX: f32 = 1.0;
// Harmonic minimum gain clamp bounds.
const HARMONIC_MIN_GAIN_MIN: f32 = 0.25;
const HARMONIC_MIN_GAIN_MAX: f32 = 0.98;
// Harmonic band width (bins) at low/high frequencies.
const HARMONIC_WIDTH_MIN: f32 = 3.0;
const HARMONIC_WIDTH_MAX: f32 = 1.5;
// Maximum harmonic count to protect.
const HARMONIC_MAX_COUNT: i32 = 80;
// Speech/tonal/unvoiced weights for speech probability.
const SPEECH_WEIGHT_VOICED: f32 = 0.55;
const SPEECH_WEIGHT_TONAL: f32 = 0.30;
const SPEECH_WEIGHT_UNVOICED: f32 = 0.35;
// Periodicity thresholds for voiced detection.
const PERIODICITY_MIN: f32 = 0.35;
const PERIODICITY_MAX: f32 = 0.80;
// Spectral flatness thresholds for tonal detection.
const FLATNESS_MIN: f32 = 0.25;
const FLATNESS_MAX: f32 = 0.85;
// HF ratio thresholds for unvoiced detection.
const HF_RATIO_MIN: f32 = 0.18;
const HF_RATIO_MAX: f32 = 0.45;
// Energy gate thresholds (RMS).
const ENERGY_GATE_MIN: f32 = 0.003;
const ENERGY_GATE_MAX: f32 = 0.02;
// HF ratio band split.
const HF_SPLIT_FRAC: f32 = 0.25;
// Max number of spectral peaks for masker.
const MASKER_MAX_PEAKS: usize = 64;
// Masker spread radius (bins).
const MASKER_RADIUS_MIN: f32 = 32.0;
const MASKER_RADIUS_MAX: f32 = 10.0;
// Masker falloff alpha range.
const MASKER_ALPHA_MIN: f32 = 10.0;
const MASKER_ALPHA_MAX: f32 = 4.0;
// OLA normalization floor.
const OLA_NORM_EPS: f32 = 1e-6;

// Decision-directed Wiener constants
const DD_ALPHA: f32 = 0.98;
const SNR_EPS: f32 = 1e-10;

/// Configuration for the denoiser
pub struct DenoiseConfig {
    pub amount: f32,
    pub sensitivity: f32,
    pub tone: f32,
    pub sample_rate: f32,
    pub use_ml: bool,
}

/// Stereo-linked FFT denoiser (shared detector + per-channel appliers).
pub struct StereoStreamingDenoiser {
    detector: StereoDenoiserDetector,
    chan_l: StreamingDenoiserChannel,
    chan_r: StreamingDenoiserChannel,

    frame_l: Vec<f32>,
    frame_r: Vec<f32>,
    frame_mono: Vec<f32>,

    win_size: usize,
    hop_size: usize,
}

impl StereoStreamingDenoiser {
    pub fn new(win_size: usize, hop_size: usize) -> Self {
        assert!(win_size >= WIN_SIZE_MIN, "win_size too small");
        assert!(hop_size > 0 && hop_size <= win_size, "invalid hop_size");

        Self {
            detector: StereoDenoiserDetector::new(win_size, hop_size),
            chan_l: StreamingDenoiserChannel::new(win_size, hop_size),
            chan_r: StreamingDenoiserChannel::new(win_size, hop_size),
            frame_l: vec![0.0; win_size],
            frame_r: vec![0.0; win_size],
            frame_mono: vec![0.0; win_size],
            win_size,
            hop_size,
        }
    }

    pub fn process_sample(
        &mut self,
        input_l: f32,
        input_r: f32,
        cfg: &DenoiseConfig,
    ) -> (f32, f32) {
        self.chan_l.push_input(input_l);
        self.chan_r.push_input(input_r);

        if self.chan_l.input_len() >= self.win_size && self.chan_r.input_len() >= self.win_size {
            self.chan_l.peek_frame(&mut self.frame_l);
            self.chan_r.peek_frame(&mut self.frame_r);

            for i in 0..self.win_size {
                let l = self.frame_l[i];
                let r = self.frame_r[i];
                self.frame_mono[i] = if l.abs() >= r.abs() { l } else { r };
            }

            let gains = self.detector.analyze_frame(&self.frame_mono, cfg);

            self.chan_l.process_frame(gains, cfg.sample_rate);
            self.chan_r.process_frame(gains, cfg.sample_rate);

            self.chan_l.discard_input(self.hop_size);
            self.chan_r.discard_input(self.hop_size);
        }

        (self.chan_l.pop_output(), self.chan_r.pop_output())
    }

    #[allow(dead_code)]
    pub fn get_noise_confidence(&self) -> f32 {
        self.detector.noise_confidence
    }
}

/// Shared detector for stereo-linked denoising.
struct StereoDenoiserDetector {
    fft: Arc<dyn Fft<f32>>,
    win_size: usize,
    #[allow(dead_code)]
    hop_size: usize,
    window: Vec<f32>,

    scratch: Vec<Complex<f32>>,
    mag: Vec<f32>,
    prev_mag: Vec<f32>,

    fft_coarse: Arc<dyn Fft<f32>>,
    win_size_coarse: usize,
    window_coarse: Vec<f32>,
    scratch_coarse: Vec<Complex<f32>>,
    noise_floor_coarse: Vec<f32>,

    noise_floor: Vec<f32>,
    prev_gains: Vec<f32>,
    gain_buf: Vec<f32>,
    masker_buf: Vec<f32>,

    frame_time: Vec<f32>,
    peaks_buf: Vec<(usize, f32)>,
    f0_scratch: Vec<f32>,

    noise_confidence: f32,

    // ML advisor (behavior only)
    #[cfg(feature = "ml")]
    ml: Option<MlDenoiseEngine>,
    #[cfg(feature = "ml")]
    ml_init_attempted: bool,

    // ML data always exists
    ml_mask: Vec<f32>,
}

impl StereoDenoiserDetector {
    pub fn new(win_size: usize, hop_size: usize) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(win_size);
        let window = make_sqrt_hann_window(win_size);

        let win_size_coarse = (win_size / COARSE_WIN_DIV)
            .max(COARSE_WIN_MIN)
            .min(win_size);
        let fft_coarse = planner.plan_fft_forward(win_size_coarse);
        let window_coarse = make_sqrt_hann_window(win_size_coarse);

        let nyq = win_size / 2;
        let nyq_c = win_size_coarse / 2;

        Self {
            fft,
            win_size,
            hop_size,
            window,
            scratch: vec![Complex::new(0.0, 0.0); win_size],
            mag: vec![0.0; nyq + 1],
            prev_mag: vec![0.0; nyq + 1],

            fft_coarse,
            win_size_coarse,
            window_coarse,
            scratch_coarse: vec![Complex::new(0.0, 0.0); win_size_coarse],
            noise_floor_coarse: vec![NOISE_FLOOR_INIT; nyq_c + 1],

            noise_floor: vec![NOISE_FLOOR_INIT; nyq + 1],
            prev_gains: vec![1.0; nyq + 1],
            gain_buf: vec![1.0; nyq + 1],
            masker_buf: vec![0.0; nyq + 1],

            frame_time: vec![0.0; win_size],
            peaks_buf: Vec::with_capacity(MASKER_MAX_PEAKS),
            f0_scratch: Vec::with_capacity(win_size),
            noise_confidence: 1.0,

            ml_mask: vec![0.0; nyq + 1],

            #[cfg(feature = "ml")]
            ml: MlDenoiseEngine::new().ok(),
            #[cfg(feature = "ml")]
            ml_init_attempted: false,
        }
    }

    pub fn analyze_frame(&mut self, mono: &[f32], cfg: &DenoiseConfig) -> &[f32] {
        let n = self.win_size;
        let nyq = n / 2;
        let sr = cfg.sample_rate.max(SAMPLE_RATE_MIN);

        let amt = cfg.amount.clamp(0.0, 1.0);
        let sensitivity = cfg.sensitivity.clamp(0.0, 1.0);
        let tone = cfg.tone.clamp(0.0, 1.0);

        // Reset decision-directed history on bypass (Patch 5)
        if amt <= BYPASS_AMOUNT_EPS {
            for i in 0..=nyq {
                self.prev_gains[i] = 1.0;
                self.prev_mag[i] = self.mag[i];
            }
        }

        // 0) Window + FFT input
        for i in 0..n {
            let x = mono[i];
            self.frame_time[i] = x;
            self.scratch[i] = Complex::new(x * self.window[i], 0.0);
        }

        // 1) FFT
        self.fft.process(&mut self.scratch);

        // 2) Magnitudes
        for i in 0..=nyq {
            self.mag[i] = self.scratch[i].norm().max(MAG_FLOOR);
        }

        // Analysis-side hum removal
        if amt > HUM_REMOVAL_AMOUNT_THRESH {
            self.apply_hum_removal_inplace(sr);
            for i in 0..=nyq {
                self.mag[i] = self.scratch[i].norm().max(MAG_FLOOR);
            }
        }

        // 3) Multi-resolution cues
        self.compute_coarse_fft_and_update_noise(mono);

        // 4) DSP speech presence probability + voiced/unvoiced + f0
        let (dsp_speech_prob, voiced_prob, f0_hz) = self.estimate_speech_and_f0(sr);

        // 4b) ML advisor speech mask
        let mut ml_global_spp = 0.0;
        #[cfg(feature = "ml")]
        if cfg.use_ml {
            if self.ml.is_none() && !self.ml_init_attempted {
                self.ml = MlDenoiseEngine::new().ok();
                self.ml_init_attempted = true;
            }
            if let Some(ml) = self.ml.as_mut() {
                // NOTE: ML backend applies its own Hann window internally. (Patch 6)
                // We intentionally pass raw time-domain samples here.
                match ml.process_frame(&self.frame_time, sr, &mut self.ml_mask) {
                    Ok(()) => {
                        ml_global_spp = self.ml_mask.iter().copied().fold(0.0, f32::max);
                    }
                    Err(_) => {
                        // Inference failed, clear mask defensively
                        for v in &mut self.ml_mask {
                            *v = 0.0;
                        }
                    }
                }
            } else {
                // Engine missing: clear mask
                for v in &mut self.ml_mask {
                    *v = 0.0;
                }
            }
        } else {
            // Explicitly clear ML mask when ML is disabled or unavailable (Patch 4)
            for v in &mut self.ml_mask {
                *v = 0.0;
            }
        }
        #[cfg(not(feature = "ml"))]
        {
            // No ML compiled in, mask must be zero
            for v in &mut self.ml_mask {
                *v = 0.0;
            }
        }
        // Conservative SPP fusion: ML rescues DSP uncertainty
        let global_spp = dsp_speech_prob.max(ml_global_spp * 0.85).clamp(0.0, 1.0);

        // 5) Update noise floor
        let startup_mode =
            self.noise_floor[nyq.min(self.noise_floor.len() - 1)] < NOISE_STARTUP_THRESH;
        let (alpha_att, alpha_rel) = if startup_mode {
            (NOISE_STARTUP_ATT, NOISE_STARTUP_REL)
        } else {
            let protect = NOISE_PROTECT_BASE + NOISE_PROTECT_RANGE * global_spp;
            (
                lerp(NOISE_ATT_BASE, NOISE_ATT_MAX, protect),
                lerp(NOISE_REL_BASE, NOISE_REL_MAX, protect),
            )
        };

        let mut stability_sum = 0.0;
        for i in 0..=nyq {
            let mag = self.mag[i];
            let nf = self.noise_floor[i];
            let prev_nf = nf;

            self.noise_floor[i] = if mag < nf {
                nf * alpha_att + mag * (1.0 - alpha_att)
            } else {
                nf * alpha_rel + mag * (1.0 - alpha_rel)
            };
            self.noise_floor[i] = self.noise_floor[i].max(MAG_FLOOR);

            if prev_nf > MAG_FLOOR {
                stability_sum += (self.noise_floor[i] - prev_nf).abs() / prev_nf;
            }
        }

        let avg_change = stability_sum / (nyq as f32 + 1.0).max(1.0); // Guarded Nyquist division (Patch 7)
        let inst_conf = (1.0 - avg_change * 50.0).clamp(0.0, 1.0);
        self.noise_confidence = lerp(self.noise_confidence, inst_conf, 0.05);

        // Scale amount by confidence to prevent artifacts on unstable noise floor
        let effective_amt = amt * (0.2 + 0.8 * self.noise_confidence);

        // 6) Masker curve
        self.compute_masker_curve(sr);

        // 7) Wiener Gain curve build
        let voiced = voiced_prob > VOICED_PROB_MIN;

        for i in 0..=nyq {
            let mag = self.mag[i];
            let nf = self.noise_floor[i];
            let freq_fraction = i as f32 / nyq.max(1) as f32; // Guarded Nyquist division (Patch 7)

            // a-posteriori SNR: gamma = mag² / (noise_floor² + eps)
            let gamma = (mag * mag) / (nf * nf + SNR_EPS);

            // a-priori SNR using decision-directed recursion (Ephraim & Malah)
            // xi = alpha * (prev_gain² * prev_mag² / (noise_floor² + eps)) + (1 - alpha) * max(gamma - 1, 0)
            let pg = self.prev_gains[i];
            let pm = self.prev_mag[i];
            let xi_hist = (pg * pg * pm * pm) / (nf * nf + SNR_EPS);
            let xi_curr = (gamma - 1.0).max(0.0);
            let xi = DD_ALPHA * xi_hist + (1.0 - DD_ALPHA) * xi_curr;

            // Wiener gain: G = xi / (1 + xi)
            let mut wiener_gain = xi / (1.0 + xi);

            // Tone bias logic
            let bias = if tone < TONE_SPLIT {
                let t = (tone * TONE_SCALE).clamp(0.0, 1.0);
                db_to_gain(TONE_BIAS_DB * (1.0 - t) * (1.0 - freq_fraction))
            } else {
                let t = ((tone - TONE_SPLIT) * TONE_SCALE).clamp(0.0, 1.0);
                db_to_gain(TONE_BIAS_DB * t * freq_fraction)
            };

            // Speech weighting: Fusing DSP and ML cues per bin
            let band_weight = if voiced {
                let mid = bell(freq_fraction, VOICED_MID_CENTER, VOICED_MID_WIDTH);
                VOICED_SPEECH_BASE + VOICED_SPEECH_RANGE * mid
            } else {
                let hi = smoothstep(UNVOICED_HF_MIN, UNVOICED_HF_MAX, freq_fraction);
                UNVOICED_SPEECH_BASE + UNVOICED_SPEECH_RANGE * hi
            };

            // Per-bin SPP fusion
            let spp_bin = dsp_speech_prob.max(self.ml_mask[i] * 0.85).clamp(0.0, 1.0) * band_weight;

            // Threshold scaling using fused SPP and sensitivity
            let thresh_scale = (1.0 + sensitivity * THRESH_SENS_SCALE)
                * bias
                * (1.0 + SPEECH_THRESH_SCALE * spp_bin);

            // Adjust Wiener gain by effective reduction amount and threshold
            let gain_depth = if mag <= nf * thresh_scale {
                let d = (mag / (nf * thresh_scale + MAG_FLOOR)).powf(RAW_GAIN_POWER);
                1.0 - (effective_amt * (1.0 - d))
            } else {
                1.0
            };

            wiener_gain =
                wiener_gain.powf(lerp(STRENGTH_MIN, STRENGTH_MAX, effective_amt)) * gain_depth;

            // Psychoacoustic and speech-conditioned floors
            let masker = self.masker_buf[i].max(MAG_FLOOR);
            let mask_ratio = (masker / (masker + nf)).clamp(0.0, 1.0);
            let floor_scale = lerp(1.0, FLOOR_SCALE_MIN, effective_amt);
            let speech_floor_scale = lerp(1.0, SPEECH_FLOOR_SCALE_MIN, effective_amt);

            let psycho_floor = (PSYCHO_FLOOR_BASE + PSYCHO_FLOOR_RANGE * (1.0 - mask_ratio))
                .clamp(PSYCHO_FLOOR_MIN, PSYCHO_FLOOR_MAX)
                * floor_scale;
            let speech_floor = (SPEECH_FLOOR_BASE + SPEECH_FLOOR_RANGE * spp_bin)
                .clamp(SPEECH_FLOOR_MIN, SPEECH_FLOOR_MAX)
                * speech_floor_scale;

            let min_floor = if effective_amt <= BYPASS_AMOUNT_EPS {
                0.0
            } else {
                lerp(psycho_floor, speech_floor, spp_bin)
            };

            self.gain_buf[i] = wiener_gain.max(min_floor);
            self.prev_mag[i] = mag; // Store for next frame
        }

        // 8) Spectral Guardrail: Musical Noise Reduction
        if effective_amt > 0.0 {
            let smooth_strength = if voiced {
                SMOOTH_STRENGTH_VOICED
            } else {
                SMOOTH_STRENGTH_UNVOICED
            };
            let mut prev = self.gain_buf[0];
            for i in 1..nyq.saturating_sub(1) {
                let curr = self.gain_buf[i];
                let next = self.gain_buf[i + 1];
                let sm = (prev + curr + next) / 3.0;
                prev = curr;
                self.gain_buf[i] = lerp(curr, sm, smooth_strength);
            }
        }

        // 9) Temporal Guardrail: High-Frequency Pumping Prevention
        if effective_amt > 0.0 {
            let release_limit = lerp(RELEASE_LIMIT_MIN, RELEASE_LIMIT_MAX, global_spp);
            for i in 0..=nyq {
                if self.gain_buf[i] < self.prev_gains[i] {
                    self.gain_buf[i] = self.gain_buf[i].max(self.prev_gains[i] * release_limit);
                }
                self.prev_gains[i] = self.gain_buf[i];
            }
        }

        // 10) Harmonic Guardrail: Voice Thinning Prevention
        if effective_amt > 0.0 && voiced && f0_hz > HARMONIC_F0_MIN_HZ && f0_hz < HARMONIC_F0_MAX_HZ
        {
            self.apply_harmonic_protection(sr, f0_hz, global_spp, effective_amt);
        }

        &self.gain_buf
    }

    fn compute_coarse_fft_and_update_noise(&mut self, mono: &[f32]) {
        let n2 = self.win_size_coarse;
        let nyq2 = n2 / 2;
        for i in 0..n2 {
            self.scratch_coarse[i] = Complex::new(mono[i] * self.window_coarse[i], 0.0);
        }
        self.fft_coarse.process(&mut self.scratch_coarse);
        for i in 0..=nyq2 {
            let mag = self.scratch_coarse[i].norm().max(MAG_FLOOR);
            let nf = self.noise_floor_coarse[i];
            self.noise_floor_coarse[i] = if mag < nf {
                nf * NOISE_COARSE_ATT + mag * (1.0 - NOISE_COARSE_ATT)
            } else {
                nf * NOISE_COARSE_REL + mag * (1.0 - NOISE_COARSE_REL)
            };
            self.noise_floor_coarse[i] = self.noise_floor_coarse[i].max(MAG_FLOOR);
        }
    }

    fn estimate_speech_and_f0(&mut self, sr: f32) -> (f32, f32, f32) {
        let (periodicity, f0_hz) = estimate_f0_autocorr(&self.frame_time, &mut self.f0_scratch, sr);
        let nyq = (self.win_size / 2).max(1);
        let mut geo = 0.0f32;
        let mut arith = 0.0f32;
        for i in 1..nyq {
            let m = self.mag[i].max(MAG_FLOOR);
            geo += m.ln();
            arith += m;
        }
        let flatness = ((geo / (nyq as f32).max(1.0)).exp()
            / (arith / (nyq as f32).max(1.0) + MAG_FLOOR))
            .clamp(0.0, 1.0); // Guarded Nyquist division (Patch 7)
        let hf_start = (nyq as f32 * HF_SPLIT_FRAC) as usize;
        let mut hf = 0.0f32;
        let mut lf = 0.0f32;
        for i in 1..nyq {
            if i >= hf_start {
                hf += self.mag[i];
            } else {
                lf += self.mag[i];
            }
        }
        let hf_ratio = (hf / (hf + lf + MAG_FLOOR)).clamp(0.0, 1.0);
        let voiced_prob = smoothstep(PERIODICITY_MIN, PERIODICITY_MAX, periodicity);
        let tonal_prob = 1.0 - smoothstep(FLATNESS_MIN, FLATNESS_MAX, flatness);
        let unvoiced_prob = smoothstep(HF_RATIO_MIN, HF_RATIO_MAX, hf_ratio) * (1.0 - voiced_prob);
        let mut speech_prob = (SPEECH_WEIGHT_VOICED * voiced_prob
            + SPEECH_WEIGHT_TONAL * tonal_prob
            + SPEECH_WEIGHT_UNVOICED * unvoiced_prob)
            .clamp(0.0, 1.0);
        speech_prob *= smoothstep(
            ENERGY_GATE_MIN,
            ENERGY_GATE_MAX,
            frame_rms(&self.frame_time),
        );
        (speech_prob, voiced_prob, f0_hz)
    }

    fn compute_masker_curve(&mut self, sr: f32) {
        let n = self.win_size;
        let nyq = n / 2;
        for v in &mut self.masker_buf {
            *v = 0.0;
        }
        self.peaks_buf.clear();
        if nyq >= 6 {
            for i in 2..=(nyq - 2) {
                let m = self.mag[i];
                if m > self.mag[i - 1]
                    && m > self.mag[i + 1]
                    && m > self.mag[i - 2]
                    && m > self.mag[i + 2]
                {
                    self.peaks_buf.push((i, m));
                }
            }
        }
        self.peaks_buf
            .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        self.peaks_buf.truncate(MASKER_MAX_PEAKS);
        let bin_width = sr / n as f32;
        for &(center, amp) in &self.peaks_buf {
            let frac = (center as f32 * bin_width / (sr * NYQUIST_FRAC).max(1.0)).clamp(0.0, 1.0); // Guarded Nyquist division (Patch 7)
            let radius = lerp(MASKER_RADIUS_MIN, MASKER_RADIUS_MAX, frac) as isize;
            let alpha = lerp(MASKER_ALPHA_MIN, MASKER_ALPHA_MAX, frac);
            for d in -radius..=radius {
                let j = center as isize + d;
                if j > 0 && (j as usize) < nyq {
                    let val = amp * (-d.abs() as f32 / alpha.max(0.1)).exp();
                    if val > self.masker_buf[j as usize] {
                        self.masker_buf[j as usize] = val;
                    }
                }
            }
        }
    }

    fn apply_harmonic_protection(&mut self, sr: f32, f0_hz: f32, speech_prob: f32, amt: f32) {
        let n = self.win_size;
        let nyq = n / 2;
        let bin_width = sr / n as f32;
        let protect = (HARMONIC_PROTECT_BASE + HARMONIC_PROTECT_RANGE * speech_prob).clamp(
            HARMONIC_PROTECT_BASE,
            HARMONIC_PROTECT_BASE + HARMONIC_PROTECT_RANGE,
        );
        let min_gain = (protect
            * (1.0 - HARMONIC_ALLOW_SCALE * amt).clamp(HARMONIC_ALLOW_MIN, HARMONIC_ALLOW_MAX))
        .clamp(HARMONIC_MIN_GAIN_MIN, HARMONIC_MIN_GAIN_MAX);
        let max_hz = HARMONIC_MAX_HZ.min(sr * NYQUIST_FRAC);
        let mut h = 1;
        loop {
            let hz = f0_hz * h as f32;
            if hz > max_hz {
                break;
            }
            let center = (hz / bin_width.max(1.0)).round() as isize; // Guarded Nyquist division (Patch 7)
            if center <= 1 || center as usize >= nyq.max(1).saturating_sub(1) {
                break;
            }
            let w = lerp(
                HARMONIC_WIDTH_MIN,
                HARMONIC_WIDTH_MAX,
                (hz / max_hz.max(1.0)).clamp(0.0, 1.0),
            ) as isize; // Guarded Nyquist division (Patch 7)
            for d in -w..=w {
                let bi = (center + d) as usize;
                if bi < nyq {
                    self.gain_buf[bi] = self.gain_buf[bi].max(min_gain);
                }
            }
            h += 1;
            if h > HARMONIC_MAX_COUNT {
                break;
            }
        }
    }

    fn apply_hum_removal_inplace(&mut self, sr: f32) {
        let n = self.win_size;
        let nyq = n / 2;
        let bin_width = sr / n as f32;
        for &freq in &HUM_REMOVAL_FREQS {
            let center = (freq / bin_width.max(1.0)).round() as usize; // Guarded Nyquist division (Patch 7)
            if center > 0 && center < nyq {
                self.scratch[center] *= HUM_REMOVAL_MAIN_SCALE;
                if center + 1 < nyq {
                    self.scratch[center + 1] *= HUM_REMOVAL_SIDE_SCALE;
                }
                if center > 0 {
                    self.scratch[center - 1] *= HUM_REMOVAL_SIDE_SCALE;
                }
            }
        }
        let cut_bin = (HUM_REMOVAL_LOW_CUT_HZ / bin_width.max(1.0)).ceil() as usize; // Guarded Nyquist division (Patch 7)
        for i in 0..cut_bin.min(nyq) {
            self.scratch[i] = Complex::new(0.0, 0.0);
        }
        self.scratch[0].im = 0.0;
        self.scratch[nyq].im = 0.0;
        for k in 1..nyq {
            let a = self.scratch[k];
            self.scratch[n - k] = a.conj();
        }
    }
}

struct StreamingDenoiserChannel {
    fft: Arc<dyn Fft<f32>>,
    ifft: Arc<dyn Fft<f32>>,
    win_size: usize,
    hop_size: usize,
    window: Vec<f32>,
    scratch: Vec<Complex<f32>>,
    overlap: Vec<f32>,
    ola_norm: Vec<f32>,
    input_producer: Producer<f32>,
    input_consumer: Consumer<f32>,
    output_producer: Producer<f32>,
    output_consumer: Consumer<f32>,
}

impl StreamingDenoiserChannel {
    pub fn new(win_size: usize, hop_size: usize) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(win_size);
        let ifft = planner.plan_fft_inverse(win_size);

        // Fixed invalid window construction bug and unified windows (Patch 1, 2, 3)
        let window = make_sqrt_hann_window(win_size);

        let buf_cap = win_size * RINGBUF_CAP_MULT;
        let (in_prod, in_cons) = RingBuffer::<f32>::new(buf_cap).split();
        let (out_prod, out_cons) = RingBuffer::<f32>::new(buf_cap).split();
        let mut initialized_out_prod = out_prod;
        for _ in 0..win_size {
            let _ = initialized_out_prod.push(0.0);
        }
        Self {
            fft,
            ifft,
            win_size,
            hop_size,
            window,
            scratch: vec![Complex::new(0.0, 0.0); win_size],
            overlap: vec![0.0; win_size],
            ola_norm: vec![0.0; win_size],
            input_producer: in_prod,
            input_consumer: in_cons,
            output_producer: initialized_out_prod,
            output_consumer: out_cons,
        }
    }

    pub fn push_input(&mut self, x: f32) {
        let _ = self.input_producer.push(x);
    }
    pub fn input_len(&self) -> usize {
        self.input_consumer.len()
    }
    pub fn peek_frame(&self, out: &mut [f32]) {
        for (i, val) in self.input_consumer.iter().take(self.win_size).enumerate() {
            out[i] = *val;
        }
    }
    pub fn discard_input(&mut self, n: usize) {
        self.input_consumer.discard(n);
    }
    pub fn pop_output(&mut self) -> f32 {
        self.output_consumer.pop().unwrap_or(0.0)
    }

    pub fn process_frame(&mut self, gains: &[f32], _sr: f32) {
        let n = self.win_size;
        let nyq = n / 2;
        for (i, val) in self.input_consumer.iter().take(n).enumerate() {
            self.scratch[i] = Complex::new(*val * self.window[i], 0.0);
        }
        self.fft.process(&mut self.scratch);
        for i in 0..=nyq {
            self.scratch[i] *= gains[i].clamp(0.0, 1.0);
        }
        self.scratch[0].im = 0.0;
        if n % 2 == 0 {
            self.scratch[nyq].im = 0.0;
        }
        for k in 1..nyq {
            let a = self.scratch[k];
            self.scratch[n - k] = a.conj();
        }
        self.ifft.process(&mut self.scratch);
        let norm = 1.0 / n as f32;
        for i in 0..n {
            let w = self.window[i];
            self.overlap[i] += self.scratch[i].re * norm * w;
            self.ola_norm[i] += w * w;
        }
        for i in 0..self.hop_size {
            let _ = self
                .output_producer
                .push(self.overlap[i] / self.ola_norm[i].max(OLA_NORM_EPS));
        }
        self.overlap.copy_within(self.hop_size..n, 0);
        self.ola_norm.copy_within(self.hop_size..n, 0);
        for i in (n - self.hop_size)..n {
            self.overlap[i] = 0.0;
            self.ola_norm[i] = 0.0;
        }
    }
}

#[allow(dead_code)]
pub struct StreamingDenoiser {
    inner: StereoStreamingDenoiser,
}

#[allow(dead_code)]
impl StreamingDenoiser {
    pub fn new(win_size: usize, hop_size: usize) -> Self {
        Self {
            inner: StereoStreamingDenoiser::new(win_size, hop_size),
        }
    }
    pub fn process_sample(&mut self, input: f32, cfg: &DenoiseConfig) -> f32 {
        let (l, _) = self.inner.process_sample(input, input, cfg);
        l
    }
}
