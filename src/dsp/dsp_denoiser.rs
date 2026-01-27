//! DSP Denoiser (Wiener + ML Advisor)
//!
//! Traditional spectral denoiser using Wiener filtering with optional ML-based
//! advisory for improved speech probability estimation. Reduces stationary
//! background noise while preserving voice timbre and intelligibility.
//!
//! # Purpose
//! Reduce stationary background noise (hiss, hum, fan noise) while preserving
//! voice timbre and maintaining speech intelligibility. Optionally enhanced
//! with ML-based advisory for more intelligent noise reduction.
//!
//! # Design Notes
//! - Uses traditional Wiener filtering approach
//! - Optional ML advisor for improved speech probability estimation
//! - Focuses on stationary noise reduction
//! - Preserves voice characteristics and intelligibility
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
//! # Speech Detection: Why This Module Has Its Own
//!
//! This module uses `estimate_speech_and_f0()` to compute spectral-domain speech probability.
//! This is INTENTIONALLY separate from the shared `SpeechConfidenceEstimator` because:
//!
//! 1. **Different domain**: Denoiser needs per-bin spectral analysis (f0, periodicity, flatness)
//!    while the shared envelope provides time-domain RMS confidence.
//! 2. **Different purpose**: Protects noise floor tracking from speech contamination.
//! 3. **No behavior change**: Replacing with time-domain envelope would alter sound.
//!
//! The denoiser's `dsp_speech_prob` is used to:
//! - Gate noise floor updates during speech (prevent tracking speech as noise)
//! - Drive harmonic protection based on detected f0
//! - Adjust Wiener gain floor based on voiced/unvoiced state
//!
//! IMPORTANT: Do NOT attempt to replace this with the shared `SpeechSidechain` envelope.
//!
//! # Assumptions
//! - Background noise is mostly stationary or slowly varying.
//! - Speech is characterized by harmonic structure (voiced) or broadband high-frequency transients (unvoiced).
//! - Impulse noise and non-stationary transients are NOT modeled.

use crate::dsp::utils::{
    bell, db_to_gain, estimate_f0_autocorr, frame_rms, lerp, make_sqrt_hann_window,
    perceptual_curve, smoothstep, BYPASS_AMOUNT_EPS, MAG_FLOOR,
};
use ringbuf::{Consumer, Producer, RingBuffer};
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::sync::Arc;

use crate::dsp::denoiser::StereoDenoiser;

// Constants: unless marked "Must not change", these are tunable for behavior.

// Minimum allowed window size.
const WIN_SIZE_MIN: usize = 64;
// Ring buffer capacity multiplier relative to window size.
const RINGBUF_CAP_MULT: usize = 4;
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
const DENOISE_STRENGTH_MULT: f32 = 4.0;
const MAX_DENOISE_AMOUNT: f32 = 4.0;
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
// Harmonic allow reduction scale.
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
/// Fraction of Nyquist where HF overrides activate
const HF_OVERRIDE_FRAC: f32 = 0.25;
/// Speech confidence threshold to drop HF floors
const HF_SILENCE_CONF_THRESHOLD: f32 = 0.2;
/// Speech confidence threshold to relax HF release limits
const HF_RELEASE_CONF_THRESHOLD: f32 = 0.3;
/// Minimum HF gain floor when in confirmed silence
const HF_SILENCE_MIN_GAIN: f32 = 0.02;
// Max number of spectral peaks for masker.
const MASKER_MAX_PEAKS: usize = 64;
// Masker spread radius (bins).
const MASKER_RADIUS_MIN: f32 = 32.0;
const MASKER_RADIUS_MAX: f32 = 10.0;
// Masker falloff alpha range.
const MASKER_ALPHA_MIN: f32 = 10.0;
const MASKER_ALPHA_MAX: f32 = 4.0;
// OLA normalization floor.
const OLA_NORM_EPS: f32 = 1e-3;

// Decision-directed Wiener constants
const DD_ALPHA: f32 = 0.98;
const SNR_EPS: f32 = 1e-10;

// ----------------------------
// SOTA DSP EXTENSIONS
// ----------------------------

// Inter-frame coherence
const COH_EPS: f32 = 1e-12;
const COH_WEIGHT: f32 = 0.35;

// Transient protection (unvoiced consonants)
const TRANSIENT_D_RMS_MIN: f32 = 0.0025;
const TRANSIENT_D_RMS_MAX: f32 = 0.015;
const TRANSIENT_HF_MIN: f32 = 0.18;
const TRANSIENT_HOLD_FRAMES: i32 = 2;

// MMSE-LSA numerical guard
const MMSE_EPS: f32 = 1e-12;

fn expint_e1(x: f32) -> f32 {
    let x = x.max(MMSE_EPS);
    if x < 1.0 {
        let gamma = 0.57721566_f32;
        (-gamma - x.ln()) + x - 0.25 * x * x
    } else {
        (-x).exp() / x
    }
}

fn mmse_lsa_gain(xi: f32, gamma: f32) -> f32 {
    let xi = xi.max(0.0);
    let gamma = gamma.max(0.0);
    let v = gamma * xi / (1.0 + xi + MMSE_EPS);
    let e1 = expint_e1(v);
    let g = (xi / (1.0 + xi + MMSE_EPS)) * (0.5 * e1).exp();
    g.clamp(0.0, 1.0)
}

/// Configuration for the denoiser
#[derive(Debug, Clone, Copy)]
pub struct DenoiseConfig {
    pub amount: f32,
    pub sensitivity: f32,
    pub tone: f32,
    pub sample_rate: f32,
    pub use_dtln: bool,         // Toggle to switch between DSP and DTLN
    pub speech_confidence: f32, // Speech confidence for adaptive behavior
}

/// DSP-based denoiser implementation
pub struct DspDenoiser {
    detector: DspDenoiserDetector,
    chan_l: StreamingDenoiserChannel,
    chan_r: StreamingDenoiserChannel,

    frame_l: Vec<f32>,
    frame_r: Vec<f32>,
    frame_mono: Vec<f32>,

    win_size: usize,
    hop_size: usize,
}

impl DspDenoiser {
    pub fn new(win_size: usize, hop_size: usize) -> Self {
        assert!(win_size >= WIN_SIZE_MIN, "win_size too small");
        assert!(hop_size > 0 && hop_size <= win_size, "invalid hop_size");

        Self {
            detector: DspDenoiserDetector::new(win_size, hop_size),
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
struct DspDenoiserDetector {
    fft: Arc<dyn Fft<f32>>,
    win_size: usize,
    #[allow(dead_code)]
    hop_size: usize,
    window: Vec<f32>,

    scratch: Vec<Complex<f32>>,
    fft_scratch: Vec<Complex<f32>>,
    mag: Vec<f32>,
    prev_mag: Vec<f32>,
    prev_spec: Vec<Complex<f32>>,

    fft_coarse: Arc<dyn Fft<f32>>,
    fft_coarse_scratch: Vec<Complex<f32>>,
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
    prev_rms: f32,
    transient_hold: i32,
}

impl DspDenoiserDetector {
    pub fn new(win_size: usize, hop_size: usize) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(win_size);
        let fft_scratch_len = fft.get_inplace_scratch_len();
        let fft_scratch = vec![Complex::default(); fft_scratch_len];

        let window = make_sqrt_hann_window(win_size);

        let win_size_coarse = (win_size / COARSE_WIN_DIV)
            .max(COARSE_WIN_MIN)
            .min(win_size);
        let fft_coarse = planner.plan_fft_forward(win_size_coarse);
        let fft_coarse_scratch_len = fft_coarse.get_inplace_scratch_len();
        let fft_coarse_scratch = vec![Complex::default(); fft_coarse_scratch_len];

        let window_coarse = make_sqrt_hann_window(win_size_coarse);

        let nyq = win_size / 2;
        let nyq_c = win_size_coarse / 2;

        Self {
            fft,
            win_size,
            hop_size,
            window,
            scratch: vec![Complex::new(0.0, 0.0); win_size],
            fft_scratch,
            mag: vec![0.0; nyq + 1],
            prev_mag: vec![0.0; nyq + 1],
            prev_spec: vec![Complex::new(0.0, 0.0); nyq + 1],

            fft_coarse,
            fft_coarse_scratch,
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
            f0_scratch: vec![0.0; win_size], // Changed from Vec::with_capacity to pre-allocated vector
            noise_confidence: 1.0,
            prev_rms: 0.0,
            transient_hold: 0,
        }
    }

    pub fn analyze_frame(&mut self, mono: &[f32], cfg: &DenoiseConfig) -> &[f32] {
        let n = self.win_size;
        let nyq = n / 2;
        let sr = cfg.sample_rate.max(SAMPLE_RATE_MIN);

        // Realtime safety assertion: ensure buffer sizes match expectations
        debug_assert_eq!(mono.len(), n, "Input frame size mismatch");
        debug_assert_eq!(self.mag.len(), nyq + 1, "Magnitude buffer size mismatch");
        debug_assert_eq!(
            self.prev_mag.len(),
            nyq + 1,
            "Previous magnitude buffer size mismatch"
        );
        debug_assert_eq!(
            self.noise_floor.len(),
            nyq + 1,
            "Noise floor buffer size mismatch"
        );
        debug_assert_eq!(
            self.prev_gains.len(),
            nyq + 1,
            "Previous gains buffer size mismatch"
        );
        debug_assert_eq!(self.gain_buf.len(), nyq + 1, "Gain buffer size mismatch");
        debug_assert_eq!(
            self.masker_buf.len(),
            nyq + 1,
            "Masker buffer size mismatch"
        );
        debug_assert_eq!(
            self.frame_time.len(),
            n,
            "Time domain frame buffer size mismatch"
        );
        debug_assert_eq!(
            self.prev_spec.len(),
            nyq + 1,
            "Previous spectrum buffer size mismatch"
        );

        // Apply perceptual curve to amount before scaling
        let curved_amount = perceptual_curve(cfg.amount.clamp(0.0, 1.0));
        let amt = (curved_amount * DENOISE_STRENGTH_MULT).clamp(0.0, MAX_DENOISE_AMOUNT);
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
        #[cfg(debug_assertions)]
        assert_no_alloc::assert_no_alloc(|| {
            self.fft
                .process_with_scratch(&mut self.scratch, &mut self.fft_scratch);
        });
        #[cfg(not(debug_assertions))]
        self.fft
            .process_with_scratch(&mut self.scratch, &mut self.fft_scratch);

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

        // Conservative SPP: Using only DSP detection
        let global_spp = dsp_speech_prob.clamp(0.0, 1.0);

        // Transient detection
        let rms = frame_rms(&self.frame_time);
        let drms = (rms - self.prev_rms).max(0.0);
        self.prev_rms = rms;
        let unvoiced = voiced_prob <= VOICED_PROB_MIN;
        if smoothstep(TRANSIENT_D_RMS_MIN, TRANSIENT_D_RMS_MAX, drms) > 0.5 && unvoiced {
            self.transient_hold = TRANSIENT_HOLD_FRAMES;
        } else if self.transient_hold > 0 {
            self.transient_hold -= 1;
        }

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
            let mag_p = self.mag[i];
            let nf = self.noise_floor[i];
            let freq_fraction = i as f32 / nyq.max(1) as f32; // Guarded Nyquist division (Patch 7)

            let noise_p = nf * nf + SNR_EPS;
            let gamma = mag_p / noise_p;

            // a-priori SNR using decision-directed recursion (Ephraim & Malah)
            // xi = alpha * (prev_gain² * prev_mag² / (noise_floor² + eps)) + (1 - alpha) * max(gamma - 1, 0)
            let pg = self.prev_gains[i];
            let pm = self.prev_mag[i];
            let xi_hist = (pg * pg) * (pm / noise_p);
            let xi_curr = (gamma - 1.0).max(0.0);
            let xi = DD_ALPHA * xi_hist + (1.0 - DD_ALPHA) * xi_curr;

            let g_lsa = mmse_lsa_gain(xi, gamma);

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
            let spp_bin = dsp_speech_prob.clamp(0.0, 1.0) * band_weight;

            // Threshold scaling using fused SPP and sensitivity
            let thresh_scale = (1.0 + sensitivity * THRESH_SENS_SCALE)
                * bias
                * (1.0 + SPEECH_THRESH_SCALE * spp_bin);

            let mag_a = mag_p.sqrt();
            // Adjust Wiener gain by effective reduction amount and threshold
            let gain_depth = if mag_a <= nf * thresh_scale {
                let d = (mag_a / (nf * thresh_scale + MAG_FLOOR)).powf(RAW_GAIN_POWER);
                1.0 - effective_amt * (1.0 - d)
            } else {
                1.0
            };

            let mut gain = g_lsa * gain_depth;

            // Inter-frame coherence
            let x = self.scratch[i];
            let xp = self.prev_spec[i];
            let coh =
                ((x * xp.conj()).norm().sqrt()) / (x.norm().sqrt() * xp.norm().sqrt() + COH_EPS);
            let coh_adj = 1.0 + COH_WEIGHT * (1.0 - coh.clamp(0.0, 1.0));
            gain = gain.powf(coh_adj);

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

            let mut min_floor = if effective_amt <= BYPASS_AMOUNT_EPS {
                0.0
            } else {
                lerp(psycho_floor, speech_floor, spp_bin)
            };

            if freq_fraction >= HF_OVERRIDE_FRAC
                && cfg.speech_confidence < HF_SILENCE_CONF_THRESHOLD
            {
                min_floor = min_floor.min(HF_SILENCE_MIN_GAIN);
            }

            if self.transient_hold > 0 && freq_fraction >= TRANSIENT_HF_MIN {
                gain = gain.max(0.22);
            }

            self.gain_buf[i] = gain.max(min_floor).min(1.0);
            self.prev_mag[i] = mag_p; // Store for next frame
        }

        // 7b) Low-Frequency Harmonic Protection (≤450Hz)
        // Prevent excessive attenuation on low-frequency voiced bins
        if voiced && effective_amt > 0.0 {
            let bin_width = sr / n as f32;
            let lf_cutoff_hz = 450.0;
            let lf_cutoff_bin = (lf_cutoff_hz / bin_width) as usize;

            for i in 0..=lf_cutoff_bin.min(nyq) {
                // Maximum 70% attenuation (minimum gain 0.3)
                if self.gain_buf[i] < 0.3 {
                    self.gain_buf[i] = 0.3;
                }
            }
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
            let base_release_limit = lerp(RELEASE_LIMIT_MIN, RELEASE_LIMIT_MAX, global_spp);
            for i in 0..=nyq {
                let freq_fraction = i as f32 / nyq.max(1) as f32;
                let release_limit = if freq_fraction >= HF_OVERRIDE_FRAC
                    && cfg.speech_confidence < HF_RELEASE_CONF_THRESHOLD
                {
                    1.0
                } else {
                    base_release_limit
                };

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

        for i in 0..=nyq {
            self.prev_spec[i] = self.scratch[i];
        }

        &self.gain_buf
    }

    fn compute_coarse_fft_and_update_noise(&mut self, mono: &[f32]) {
        let n2 = self.win_size_coarse;
        let nyq2 = n2 / 2;
        for i in 0..n2 {
            self.scratch_coarse[i] = Complex::new(mono[i] * self.window_coarse[i], 0.0);
        }
        #[cfg(debug_assertions)]
        assert_no_alloc::assert_no_alloc(|| {
            self.fft_coarse
                .process_with_scratch(&mut self.scratch_coarse, &mut self.fft_coarse_scratch);
        });
        #[cfg(not(debug_assertions))]
        self.fft_coarse
            .process_with_scratch(&mut self.scratch_coarse, &mut self.fft_coarse_scratch);
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
        let nyq = self.win_size / 2;

        // Estimate periodicity and f0
        let (periodicity, f0_hz) = estimate_f0_autocorr(&self.frame_time, &mut self.f0_scratch, sr);
        let voiced_prob = smoothstep(PERIODICITY_MIN, PERIODICITY_MAX, periodicity);

        // Spectral flatness (measure of tonality)
        let mut sum_log = 0.0;
        let mut sum_linear = 0.0;
        for i in 1..nyq {
            let mag = self.mag[i];
            if mag > MAG_FLOOR {
                sum_log += mag.ln();
                sum_linear += mag;
            }
        }
        let geom_mean = if sum_linear > MAG_FLOOR {
            (sum_log / (nyq as f32 - 1.0)).exp()
        } else {
            0.0
        };
        let arith_mean = sum_linear / (nyq as f32 - 1.0);
        let flatness = if arith_mean > MAG_FLOOR {
            geom_mean / arith_mean
        } else {
            0.0
        };
        let tonal_prob = smoothstep(FLATNESS_MIN, FLATNESS_MAX, flatness);

        // High-frequency content (unvoiced speech indicator)
        let mut lf_sum = 0.0;
        let mut hf_sum = 0.0;
        let split_bin = (nyq as f32 * HF_SPLIT_FRAC) as usize;
        for i in 1..split_bin {
            lf_sum += self.mag[i];
        }
        for i in split_bin..nyq {
            hf_sum += self.mag[i];
        }
        let hf_ratio = if lf_sum > MAG_FLOOR {
            hf_sum / lf_sum
        } else {
            0.0
        };
        let unvoiced_prob = smoothstep(HF_RATIO_MIN, HF_RATIO_MAX, hf_ratio);

        // Energy-based gate
        let rms = frame_rms(&self.frame_time);
        let energy_prob = smoothstep(ENERGY_GATE_MIN, ENERGY_GATE_MAX, rms);

        // Combine probabilities
        let voiced_weight = SPEECH_WEIGHT_VOICED * voiced_prob;
        let tonal_weight = SPEECH_WEIGHT_TONAL * tonal_prob;
        let unvoiced_weight = SPEECH_WEIGHT_UNVOICED * unvoiced_prob;

        let speech_prob = (voiced_weight + tonal_weight + unvoiced_weight) * energy_prob;

        (speech_prob.clamp(0.0, 1.0), voiced_prob, f0_hz)
    }

    fn apply_hum_removal_inplace(&mut self, sr: f32) {
        let bin_width = sr / self.win_size as f32;
        for &freq in &HUM_REMOVAL_FREQS {
            let bin = (freq / bin_width) as usize;
            if bin < self.win_size / 2 {
                // Zero main bin
                if bin > 0 {
                    self.scratch[bin] = Complex::new(
                        self.scratch[bin - 1].re * HUM_REMOVAL_MAIN_SCALE,
                        self.scratch[bin - 1].im * HUM_REMOVAL_MAIN_SCALE,
                    );
                }

                // Reduce side bins
                if bin > 1 {
                    self.scratch[bin - 1] = Complex::new(
                        self.scratch[bin - 1].re * HUM_REMOVAL_SIDE_SCALE,
                        self.scratch[bin - 1].im * HUM_REMOVAL_SIDE_SCALE,
                    );
                }
                if bin < self.win_size - 1 {
                    self.scratch[bin + 1] = Complex::new(
                        self.scratch[bin + 1].re * HUM_REMOVAL_SIDE_SCALE,
                        self.scratch[bin + 1].im * HUM_REMOVAL_SIDE_SCALE,
                    );
                }
            }
        }

        // Also zero bins below low-cut
        let low_cut_bin = (HUM_REMOVAL_LOW_CUT_HZ / bin_width) as usize;
        for i in 1..low_cut_bin.min(self.win_size / 2) {
            self.scratch[i] = Complex::new(0.0, 0.0);
        }
    }

    fn compute_masker_curve(&mut self, sr: f32) {
        let nyq = self.win_size / 2;
        self.masker_buf.fill(0.0);

        // Find spectral peaks - use a fixed-size array instead of Vec to avoid allocation
        let mut peaks_temp: [(usize, f32); MASKER_MAX_PEAKS] = [(0, 0.0); MASKER_MAX_PEAKS];
        let mut peak_count = 0;

        for i in 2..nyq - 2 {
            let m = self.mag[i];
            if m > self.mag[i - 1]
                && m > self.mag[i + 1]
                && m > self.mag[i - 2]
                && m > self.mag[i + 2]
                && m > 1e-6
            // Significant peak
            {
                if peak_count < MASKER_MAX_PEAKS {
                    peaks_temp[peak_count] = (i, m);
                    peak_count += 1;
                } else {
                    break;
                }
            }
        }

        // Sort peaks by magnitude (descending) - using a simple bubble sort for small arrays
        for i in 0..peak_count {
            for j in i + 1..peak_count {
                if peaks_temp[i].1 < peaks_temp[j].1 {
                    peaks_temp.swap(i, j);
                }
            }
        }

        // For each peak, spread its influence
        let bin_width = sr / self.win_size as f32;
        for i in 0..peak_count {
            let (peak_bin, peak_mag) = peaks_temp[i];
            if peak_mag <= MAG_FLOOR {
                continue;
            }

            // Calculate spread parameters based on frequency
            let freq_hz = peak_bin as f32 * bin_width;
            let radius = lerp(MASKER_RADIUS_MIN, MASKER_RADIUS_MAX, freq_hz / 10000.0).max(1.0);
            let alpha = lerp(MASKER_ALPHA_MIN, MASKER_ALPHA_MAX, freq_hz / 10000.0).max(1.0);

            for di in -(radius as isize)..=(radius as isize) {
                let j = peak_bin as isize + di;
                if j < 0 || j >= nyq as isize {
                    continue;
                }
                let j = j as usize;

                let dist = (di as f32).abs() / radius;
                if dist > 1.0 {
                    continue;
                }

                let weight = (-alpha * dist).exp(); // Exponential falloff
                let new_val = self.masker_buf[j].max(peak_mag * weight);
                self.masker_buf[j] = new_val;
            }
        }
    }

    fn apply_harmonic_protection(&mut self, sr: f32, f0_hz: f32, global_spp: f32, strength: f32) {
        if f0_hz <= HARMONIC_F0_MIN_HZ || f0_hz >= HARMONIC_F0_MAX_HZ {
            return;
        }

        let bin_width = sr / self.win_size as f32;
        let nyq = self.win_size / 2;

        // Calculate harmonic positions and apply protection
        let mut harm_num = 1;
        loop {
            let harmonic_freq = f0_hz * harm_num as f32;
            if harmonic_freq > HARMONIC_MAX_HZ || harm_num > HARMONIC_MAX_COUNT {
                break;
            }

            let harmonic_bin = (harmonic_freq / bin_width) as usize;
            if harmonic_bin >= nyq {
                break;
            }

            // Calculate bandwidth for this harmonic
            let bw_factor = 1.0 - (harmonic_freq / HARMONIC_MAX_HZ).min(1.0);
            let bw_bins = lerp(HARMONIC_WIDTH_MIN, HARMONIC_WIDTH_MAX, bw_factor).max(1.0);

            // Protect the harmonic region
            let start_bin = harmonic_bin.saturating_sub(bw_bins as usize);
            let end_bin = (harmonic_bin + bw_bins as usize).min(nyq);

            let allow_reduction = lerp(HARMONIC_ALLOW_MIN, HARMONIC_ALLOW_MAX, global_spp);

            for bin in start_bin..=end_bin {
                let current_gain = self.gain_buf[bin];
                let min_gain = lerp(
                    lerp(HARMONIC_MIN_GAIN_MIN, HARMONIC_MIN_GAIN_MAX, strength),
                    1.0,
                    allow_reduction,
                );
                self.gain_buf[bin] = current_gain.max(min_gain);
            }

            harm_num += 1;
        }
    }
}

impl DspDenoiser {
    pub fn reset_internal(&mut self) {
        // Reset both channels
        self.chan_l.reset();
        self.chan_r.reset();
    }
}

impl StereoDenoiser for DspDenoiser {
    fn process_sample(&mut self, input_l: f32, input_r: f32, amount: f32) -> (f32, f32) {
        // We need to adapt the interface - the original method takes a config struct
        // Create a temporary config with the amount parameter
        use crate::dsp::dsp_denoiser::DenoiseConfig;
        let temp_config = DenoiseConfig {
            amount,
            sensitivity: 0.5,       // default
            tone: 0.5,              // default
            sample_rate: 44100.0,   // default, will be overridden by actual processing
            use_dtln: false,        // we're in DSP mode
            speech_confidence: 0.5, // default
        };

        self.process_sample(input_l, input_r, &temp_config)
    }

    fn reset(&mut self) {
        // Call the actual reset method
        self.reset_internal();
    }
}

/// Per-channel streaming denoiser for WOLA processing
struct StreamingDenoiserChannel {
    input_producer: Producer<f32>,
    input_consumer: Consumer<f32>,
    output_producer: Producer<f32>,
    output_consumer: Consumer<f32>,

    win_size: usize,
    hop_size: usize,

    window: Vec<f32>,
    scratch: Vec<Complex<f32>>,
    fft_scratch: Vec<Complex<f32>>,
    ifft_scratch: Vec<Complex<f32>>,
    overlap: Vec<f32>,
    ola_norm: Vec<f32>,
    frame_in: Vec<f32>,

    fft: Arc<dyn Fft<f32>>,
    ifft: Arc<dyn Fft<f32>>,
}

impl StreamingDenoiserChannel {
    fn new(win_size: usize, hop_size: usize) -> Self {
        let buf_cap = win_size * RINGBUF_CAP_MULT;
        let (in_prod, in_cons) = RingBuffer::<f32>::new(buf_cap).split();
        let (out_prod, out_cons) = RingBuffer::<f32>::new(buf_cap).split();

        // Prime output with zeros
        let mut out_prod_init = out_prod;
        for _ in 0..win_size {
            let _ = out_prod_init.push(0.0);
        }

        let window = make_sqrt_hann_window(win_size);

        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(win_size);
        let ifft = planner.plan_fft_inverse(win_size);

        let fft_scratch_len = fft.get_inplace_scratch_len();
        let ifft_scratch_len = ifft.get_inplace_scratch_len();

        let fft_scratch = vec![Complex::default(); fft_scratch_len];
        let ifft_scratch = vec![Complex::default(); ifft_scratch_len];

        Self {
            input_producer: in_prod,
            input_consumer: in_cons,
            output_producer: out_prod_init,
            output_consumer: out_cons,
            win_size,
            hop_size,
            window,
            scratch: vec![Complex::new(0.0, 0.0); win_size],
            fft_scratch,
            ifft_scratch,
            overlap: vec![0.0; win_size],
            ola_norm: vec![0.0; win_size],
            frame_in: vec![0.0; win_size],
            fft,
            ifft,
        }
    }

    fn push_input(&mut self, sample: f32) {
        let _ = self.input_producer.push(sample);
    }

    fn peek_frame(&mut self, dest: &mut [f32]) {
        for (i, v) in self.input_consumer.iter().take(self.win_size).enumerate() {
            dest[i] = *v;
        }
    }

    fn input_len(&self) -> usize {
        self.input_consumer.len()
    }

    fn discard_input(&mut self, n: usize) {
        self.input_consumer.discard(n);
    }

    fn process_frame(&mut self, gains: &[f32], _sample_rate: f32) {
        if gains.len() != self.win_size / 2 + 1 {
            return;
        }

        // Read the full frame from input consumer by peeking (not consuming)
        // The actual consumption happens via discard_input() after processing
        for i in 0..self.win_size {
            if i < self.input_consumer.len() {
                // Use nth to access the i-th element without consuming
                if let Some(sample) = self.input_consumer.iter().nth(i) {
                    self.frame_in[i] = *sample * self.window[i];
                } else {
                    self.frame_in[i] = 0.0; // Pad with zeros if needed
                }
            } else {
                self.frame_in[i] = 0.0; // Pad with zeros if we don't have enough samples
            }
            self.scratch[i] = Complex::new(self.frame_in[i], 0.0);
        }

        // Forward FFT
        #[cfg(debug_assertions)]
        assert_no_alloc::assert_no_alloc(|| {
            self.fft
                .process_with_scratch(&mut self.scratch, &mut self.fft_scratch);
        });
        #[cfg(not(debug_assertions))]
        self.fft
            .process_with_scratch(&mut self.scratch, &mut self.fft_scratch);

        // Apply gains (only to first half, second half is conjugate symmetry)
        let nyq = self.win_size / 2;
        for i in 0..=nyq {
            self.scratch[i] *= gains[i];
        }

        // Restore conjugate symmetry
        self.scratch[0].im = 0.0;
        if nyq < self.scratch.len() {
            self.scratch[nyq].im = 0.0;
        }
        for k in 1..nyq {
            if k < self.win_size - k {
                let c = self.scratch[k].conj();
                self.scratch[self.win_size - k] = c;
            }
        }

        // Inverse FFT
        #[cfg(debug_assertions)]
        assert_no_alloc::assert_no_alloc(|| {
            self.ifft
                .process_with_scratch(&mut self.scratch, &mut self.ifft_scratch);
        });
        #[cfg(not(debug_assertions))]
        self.ifft
            .process_with_scratch(&mut self.scratch, &mut self.ifft_scratch);

        // Overlap-add synthesis
        let norm = 1.0 / self.win_size as f32;
        for i in 0..self.win_size {
            let y = self.scratch[i].re * norm * self.window[i];
            self.overlap[i] += y;
            self.ola_norm[i] += self.window[i] * self.window[i];
        }

        // Output hop samples
        for i in 0..self.hop_size {
            let denom = self.ola_norm[i].max(OLA_NORM_EPS);
            let _ = self.output_producer.push(self.overlap[i] / denom);
        }

        // Shift overlap buffers
        self.overlap.copy_within(self.hop_size..self.win_size, 0);
        self.ola_norm.copy_within(self.hop_size..self.win_size, 0);
        for i in (self.win_size - self.hop_size)..self.win_size {
            self.overlap[i] = 0.0;
            self.ola_norm[i] = 0.0;
        }
    }

    fn pop_output(&mut self) -> f32 {
        self.output_consumer.pop().unwrap_or(0.0)
    }
}

impl StreamingDenoiserChannel {
    pub fn reset(&mut self) {
        // Clear ring buffers
        while self.input_consumer.pop().is_some() {}
        while self.output_consumer.pop().is_some() {}

        // Repopulate output with zeros to maintain latency
        for _ in 0..self.win_size {
            let _ = self.output_producer.push(0.0);
        }

        // Reset processing state
        self.overlap.fill(0.0);
        self.ola_norm.fill(0.0);
    }
}
