use crate::dsp::utils::{
    bell, db_to_gain, estimate_f0_autocorr, frame_rms, lerp, make_sqrt_hann_window, smoothstep,
    BYPASS_AMOUNT_EPS, MAG_FLOOR,
};
use ringbuf::{Consumer, Producer, RingBuffer};
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::sync::Arc;

// Constants: unless marked "Must not change", these are tunable for behavior.

// Minimum allowed window size.
// Increasing: requires larger FFTs; decreasing: allows smaller FFTs.
const WIN_SIZE_MIN: usize = 64;
// Ring buffer capacity multiplier relative to window size.
// Increasing: more buffering; decreasing: less buffering.
const RINGBUF_CAP_MULT: usize = 4;
// Nyquist fraction used for normalization (fs/2).
// Must not change: defines Nyquist normalization.
const NYQUIST_FRAC: f32 = 0.5;
// Coarse FFT window divisor.
// Increasing: smaller coarse FFT; decreasing: larger coarse FFT.
const COARSE_WIN_DIV: usize = 2;
// Minimum coarse FFT size.
// Increasing: higher coarse resolution; decreasing: lower resolution.
const COARSE_WIN_MIN: usize = 256;
// Initial noise floor estimate.
// Increasing: more conservative at startup; decreasing: more sensitive.
const NOISE_FLOOR_INIT: f32 = 1e-5;
// Minimum sample rate used for analysis guard.
// Increasing: more conservative at low SR; decreasing: more permissive.
const SAMPLE_RATE_MIN: f32 = 8000.0;
// Amount threshold for enabling hum removal in analysis.
// Increasing: hum removal triggers later; decreasing: triggers sooner.
const HUM_REMOVAL_AMOUNT_THRESH: f32 = 0.05;
// Hum removal main-bin attenuation.
// Increasing: stronger hum cut; decreasing: lighter hum cut.
const HUM_REMOVAL_MAIN_SCALE: f32 = 0.1;
// Hum removal side-bin attenuation.
// Increasing: stronger hum cut; decreasing: lighter hum cut.
const HUM_REMOVAL_SIDE_SCALE: f32 = 0.5;
// Low-cut frequency for analysis hum removal (Hz).
// Increasing: removes more LF; decreasing: preserves more LF.
const HUM_REMOVAL_LOW_CUT_HZ: f32 = 25.0;
// Hum removal target frequencies (Hz).
// Increasing values: targets higher mains harmonics; decreasing: lower harmonics.
const HUM_REMOVAL_FREQS: [f32; 6] = [50.0, 60.0, 100.0, 120.0, 150.0, 180.0];
// Startup threshold to detect uninitialized noise floor.
// Increasing: stays in startup longer; decreasing: exits sooner.
const NOISE_STARTUP_THRESH: f32 = 1e-4;
// Startup noise floor attack/release.
// Increasing attack: faster noise floor drop; decreasing: slower.
const NOISE_STARTUP_ATT: f32 = 0.6;
// Increasing release: faster noise floor rise; decreasing: slower.
const NOISE_STARTUP_REL: f32 = 0.90;
// Base noise floor attack/release.
// Increasing attack: faster noise floor drop; decreasing: slower.
const NOISE_ATT_BASE: f32 = 0.90;
// Increasing release: faster noise floor rise; decreasing: slower.
const NOISE_REL_BASE: f32 = 0.9995;
// Maximum noise floor attack/release.
// Increasing attack: stronger noise model protection; decreasing: weaker.
const NOISE_ATT_MAX: f32 = 0.98;
// Increasing release: stronger noise model protection; decreasing: weaker.
const NOISE_REL_MAX: f32 = 0.99995;
// Noise model protection weights.
// Increasing base: protects noise model more; decreasing: less protection.
const NOISE_PROTECT_BASE: f32 = 0.35;
// Increasing range: stronger speech-conditioned protection; decreasing: weaker.
const NOISE_PROTECT_RANGE: f32 = 0.55;
// Coarse noise floor attack/release.
// Increasing attack: faster coarse noise drop; decreasing: slower.
const NOISE_COARSE_ATT: f32 = 0.92;
// Increasing release: faster coarse noise rise; decreasing: slower.
const NOISE_COARSE_REL: f32 = 0.999;
// Tone bias in dB for tilt.
// Increasing: stronger tonal bias; decreasing: weaker bias.
const TONE_BIAS_DB: f32 = 6.0;
// Tone split pivot.
// Increasing: shifts bias toward highs; decreasing: toward lows.
const TONE_SPLIT: f32 = 0.5;
// Tone scaling factor for mapping 0..1 to -1..1.
// Increasing: steeper tone mapping; decreasing: gentler mapping.
const TONE_SCALE: f32 = 2.0;
// Voiced speech weighting base/range.
// Increasing base: more voiced preservation; decreasing: less.
const VOICED_SPEECH_BASE: f32 = 0.35;
// Increasing range: stronger mid emphasis; decreasing: weaker.
const VOICED_SPEECH_RANGE: f32 = 0.65;
// Unvoiced speech weighting base/range.
// Increasing base: more unvoiced preservation; decreasing: less.
const UNVOICED_SPEECH_BASE: f32 = 0.25;
// Increasing range: stronger HF emphasis; decreasing: weaker.
const UNVOICED_SPEECH_RANGE: f32 = 0.75;
// Voiced mid-band bell parameters.
// Increasing center: shifts band up; decreasing: shifts down.
const VOICED_MID_CENTER: f32 = 0.22;
// Increasing width: broader band; decreasing: narrower band.
const VOICED_MID_WIDTH: f32 = 0.20;
// HF band gate for unvoiced weighting.
// Increasing min/max: requires more HF energy; decreasing: easier to trigger.
const UNVOICED_HF_MIN: f32 = 0.18;
const UNVOICED_HF_MAX: f32 = 0.55;
// Voiced probability threshold.
// Increasing: stricter voiced detection; decreasing: looser.
const VOICED_PROB_MIN: f32 = 0.55;
// Sensitivity scale for noise threshold.
// Increasing: more aggressive noise reduction; decreasing: more conservative.
const THRESH_SENS_SCALE: f32 = 5.0;
// Speech-dependent threshold boost.
// Increasing: protects speech more; decreasing: less protection.
const SPEECH_THRESH_SCALE: f32 = 1.25;
// Power for raw gain depth shaping.
// Increasing: steeper reduction; decreasing: softer reduction.
const RAW_GAIN_POWER: f32 = 2.0;
// Strength shaping range.
// Increasing min: gentler reduction at low amount; decreasing: stronger.
const STRENGTH_MIN: f32 = 1.0;
// Increasing max: stronger reduction at high amount; decreasing: gentler.
const STRENGTH_MAX: f32 = 3.0;
// Psychoacoustic floor shaping.
// Increasing base: higher noise floor; decreasing: lower floor.
const PSYCHO_FLOOR_BASE: f32 = 0.25;
// Increasing range: more floor modulation; decreasing: less modulation.
const PSYCHO_FLOOR_RANGE: f32 = 0.65;
// Psycho floor clamp bounds.
// Increasing min: less attenuation; decreasing: more attenuation.
const PSYCHO_FLOOR_MIN: f32 = 0.10;
// Increasing max: less attenuation cap; decreasing: tighter cap.
const PSYCHO_FLOOR_MAX: f32 = 0.95;
// Speech floor shaping.
// Increasing base: higher speech floor; decreasing: lower speech floor.
const SPEECH_FLOOR_BASE: f32 = 0.30;
// Increasing range: more floor modulation; decreasing: less modulation.
const SPEECH_FLOOR_RANGE: f32 = 0.60;
// Speech floor clamp bounds.
// Increasing min: less attenuation; decreasing: more attenuation.
const SPEECH_FLOOR_MIN: f32 = 0.15;
// Increasing max: less attenuation cap; decreasing: tighter cap.
const SPEECH_FLOOR_MAX: f32 = 0.98;
// Floor scaling with amount.
// Increasing min: stronger floor at high amount; decreasing: weaker.
const FLOOR_SCALE_MIN: f32 = 0.35;
// Increasing min: stronger speech floor at high amount; decreasing: weaker.
const SPEECH_FLOOR_SCALE_MIN: f32 = 0.60;
// Spectral smoothing strength for voiced/unvoiced.
// Increasing voiced: smoother for voiced; decreasing: sharper.
const SMOOTH_STRENGTH_VOICED: f32 = 0.55;
// Increasing unvoiced: smoother for unvoiced; decreasing: sharper.
const SMOOTH_STRENGTH_UNVOICED: f32 = 0.75;
// Temporal release limit range.
// Increasing min: slower release; decreasing: faster.
const RELEASE_LIMIT_MIN: f32 = 0.85;
// Increasing max: slower release; decreasing: faster.
const RELEASE_LIMIT_MAX: f32 = 0.92;
// Harmonic protection f0 range (Hz).
// Increasing min: ignores lower voices; decreasing: includes lower voices.
const HARMONIC_F0_MIN_HZ: f32 = 50.0;
// Increasing max: includes higher voices; decreasing: excludes higher voices.
const HARMONIC_F0_MAX_HZ: f32 = 450.0;
// Harmonic protection max frequency (Hz).
// Increasing: protects more HF harmonics; decreasing: fewer.
const HARMONIC_MAX_HZ: f32 = 8000.0;
// Harmonic protection base/range.
// Increasing base: more protection; decreasing: less.
const HARMONIC_PROTECT_BASE: f32 = 0.55;
// Increasing range: more protection with speech; decreasing: less.
const HARMONIC_PROTECT_RANGE: f32 = 0.40;
// Harmonic allow reduction scale.
// Increasing: more attenuation at high amount; decreasing: less.
const HARMONIC_ALLOW_SCALE: f32 = 0.65;
// Harmonic allow clamp bounds.
// Increasing min: less attenuation; decreasing: more attenuation.
const HARMONIC_ALLOW_MIN: f32 = 0.25;
// Increasing max: less attenuation; decreasing: more attenuation.
const HARMONIC_ALLOW_MAX: f32 = 1.0;
// Harmonic minimum gain clamp bounds.
// Increasing min: preserves more harmonics; decreasing: allows deeper cuts.
const HARMONIC_MIN_GAIN_MIN: f32 = 0.25;
// Increasing max: preserves more harmonics; decreasing: allows deeper cuts.
const HARMONIC_MIN_GAIN_MAX: f32 = 0.98;
// Harmonic band width (bins) at low/high frequencies.
// Increasing min: wider protection at LF; decreasing: narrower.
const HARMONIC_WIDTH_MIN: f32 = 3.0;
// Increasing max: wider protection at HF; decreasing: narrower.
const HARMONIC_WIDTH_MAX: f32 = 1.5;
// Maximum harmonic count to protect.
// Increasing: protects more harmonics; decreasing: fewer.
const HARMONIC_MAX_COUNT: i32 = 80;
// Speech/tonal/unvoiced weights for speech probability.
// Increasing voiced: more voiced bias; decreasing: less.
const SPEECH_WEIGHT_VOICED: f32 = 0.55;
// Increasing tonal: more tonal bias; decreasing: less.
const SPEECH_WEIGHT_TONAL: f32 = 0.30;
// Increasing unvoiced: more unvoiced bias; decreasing: less.
const SPEECH_WEIGHT_UNVOICED: f32 = 0.35;
// Periodicity thresholds for voiced detection.
// Increasing min/max: stricter voicing; decreasing: looser.
const PERIODICITY_MIN: f32 = 0.35;
const PERIODICITY_MAX: f32 = 0.80;
// Spectral flatness thresholds for tonal detection.
// Increasing min/max: stricter tonal detection; decreasing: looser.
const FLATNESS_MIN: f32 = 0.25;
const FLATNESS_MAX: f32 = 0.85;
// HF ratio thresholds for unvoiced detection.
// Increasing min/max: stricter HF detection; decreasing: looser.
const HF_RATIO_MIN: f32 = 0.18;
const HF_RATIO_MAX: f32 = 0.45;
// Energy gate thresholds (RMS).
// Increasing min/max: requires louder input; decreasing: triggers easier.
const ENERGY_GATE_MIN: f32 = 0.003;
const ENERGY_GATE_MAX: f32 = 0.02;
// HF ratio band split.
// Increasing: shifts split upward; decreasing: downward.
const HF_SPLIT_FRAC: f32 = 0.25;
// Max number of spectral peaks for masker.
// Increasing: more peaks considered; decreasing: fewer.
const MASKER_MAX_PEAKS: usize = 64;
// Masker spread radius (bins).
// Increasing min: broader low-frequency masking; decreasing: narrower.
const MASKER_RADIUS_MIN: f32 = 32.0;
// Increasing max: broader high-frequency masking; decreasing: narrower.
const MASKER_RADIUS_MAX: f32 = 10.0;
// Masker falloff alpha range.
// Increasing min: slower falloff at LF; decreasing: faster.
const MASKER_ALPHA_MIN: f32 = 10.0;
// Increasing max: slower falloff at HF; decreasing: faster.
const MASKER_ALPHA_MAX: f32 = 4.0;
// OLA normalization floor.
// Increasing: more conservative to avoid divide-by-zero; decreasing: closer to raw.
const OLA_NORM_EPS: f32 = 1e-6;

/// Configuration for the denoiser
pub struct DenoiseConfig {
    pub amount: f32,
    pub sensitivity: f32,
    pub tone: f32,
    pub sample_rate: f32,
}

/// Stereo-linked FFT denoiser (shared detector + per-channel appliers).
///
/// Key properties:
/// - Stereo-linked analysis using a shared mono proxy frame:
///     mono[i] = (abs-larger sample of L/R, preserving sign)
/// - Shared noise model, speech presence, voicing, F0, masking, and gain curve
/// - Per-channel FFT/IFFT/WOLA application using identical per-bin gains
/// - Correct real-signal FFT handling:
///     operate on unique bins 0..=Nyquist and enforce Hermitian symmetry
/// - Proper WOLA using sqrt-Hann + per-sample overlap normalization
pub struct StereoStreamingDenoiser {
    detector: StereoDenoiserDetector,
    chan_l: StreamingDenoiserChannel,
    chan_r: StreamingDenoiserChannel,

    // Frame scratch for building mono proxy
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

    /// Process one stereo sample. Returns (out_l, out_r).
    ///
    /// True bypass when cfg.amount is effectively zero:
    /// - passthrough audio
    /// - detector/appliers do not accumulate state (keeps behavior predictable)
    pub fn process_sample(
        &mut self,
        input_l: f32,
        input_r: f32,
        cfg: &DenoiseConfig,
    ) -> (f32, f32) {
        // Push samples to both channels
        self.chan_l.push_input(input_l);
        self.chan_r.push_input(input_r);

        // When a full frame is available on both channels, run a linked frame step
        if self.chan_l.input_len() >= self.win_size && self.chan_r.input_len() >= self.win_size {
            // Gather aligned frames from both channels (no discard yet)
            self.chan_l.peek_frame(&mut self.frame_l);
            self.chan_r.peek_frame(&mut self.frame_r);

            // Build mono proxy frame for detector (choose larger-magnitude sample, preserve sign)
            for i in 0..self.win_size {
                let l = self.frame_l[i];
                let r = self.frame_r[i];
                self.frame_mono[i] = if l.abs() >= r.abs() { l } else { r };
            }

            // Shared detector produces per-bin gains (unique bins 0..=nyq)
            let gains = self.detector.analyze_frame(&self.frame_mono, cfg);

            // Apply to each channel (per-channel FFT/IFFT/WOLA) using identical gains
            self.chan_l.process_frame(gains, cfg.sample_rate);
            self.chan_r.process_frame(gains, cfg.sample_rate);

            // Consume hop on both channels (keeps them aligned)
            self.chan_l.discard_input(self.hop_size);
            self.chan_r.discard_input(self.hop_size);
        }

        // Pop output samples
        (self.chan_l.pop_output(), self.chan_r.pop_output())
    }
}

/// Shared detector for stereo-linked denoising.
/// Owns all “perceptual” state: noise model, SPP, voiced/unvoiced, F0, masker, gains.
struct StereoDenoiserDetector {
    // Main analysis FFT (shared)
    fft: Arc<dyn Fft<f32>>,
    win_size: usize,
    #[allow(dead_code)]
    hop_size: usize,
    window: Vec<f32>, // sqrt-Hann

    // Unique-bin analysis buffers (0..=nyq)
    scratch: Vec<Complex<f32>>, // full length win_size for FFT
    mag: Vec<f32>,              // nyq+1

    // Coarse FFT (analysis only; multi-resolution cues)
    fft_coarse: Arc<dyn Fft<f32>>,
    win_size_coarse: usize,
    window_coarse: Vec<f32>,           // sqrt-Hann
    scratch_coarse: Vec<Complex<f32>>, // length win_size_coarse
    noise_floor_coarse: Vec<f32>,      // coarse nyq+1

    // Noise model + gain state (main unique bins)
    noise_floor: Vec<f32>, // nyq+1
    prev_gains: Vec<f32>,  // nyq+1
    gain_buf: Vec<f32>,    // nyq+1
    masker_buf: Vec<f32>,  // nyq+1

    // Time-domain scratch (speech classification + F0 estimation)
    frame_time: Vec<f32>, // win_size

    // Pre-allocated buffer for masker peak detection (avoids audio-thread allocation)
    peaks_buf: Vec<(usize, f32)>, // capacity MASKER_MAX_PEAKS

    // Pre-allocated buffer for F0 autocorrelation (avoids audio-thread allocation)
    f0_scratch: Vec<f32>, // capacity win_size
}

impl StereoDenoiserDetector {
    pub fn new(win_size: usize, hop_size: usize) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(win_size);

        // sqrt-Hann for correct WOLA behavior across variable hop sizes
        let window = make_sqrt_hann_window(win_size);

        // Coarse analysis FFT: half size (min 256)
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
        }
    }

    /// Analyze one mono proxy frame and return unique-bin gains (0..=nyq).
    pub fn analyze_frame(&mut self, mono: &[f32], cfg: &DenoiseConfig) -> &[f32] {
        let n = self.win_size;
        let nyq = n / 2;
        let sr = cfg.sample_rate.max(SAMPLE_RATE_MIN); // guard

        let amt = cfg.amount.clamp(0.0, 1.0);
        let sensitivity = cfg.sensitivity.clamp(0.0, 1.0);
        let tone = cfg.tone.clamp(0.0, 1.0);

        // 0) Window + FFT input
        for i in 0..n {
            let x = mono[i];
            self.frame_time[i] = x;
            self.scratch[i] = Complex::new(x * self.window[i], 0.0);
        }

        // 1) FFT
        self.fft.process(&mut self.scratch);

        // 2) Magnitudes (unique bins)
        for i in 0..=nyq {
            self.mag[i] = self.scratch[i].norm().max(MAG_FLOOR);
        }

        // Optional hum removal (analysis-side): notch the *analysis spectrum* before noise learning
        // This mirrors your earlier behavior but keeps it detector-only.
        if amt > HUM_REMOVAL_AMOUNT_THRESH {
            self.apply_hum_removal_inplace(sr);
            for i in 0..=nyq {
                self.mag[i] = self.scratch[i].norm().max(MAG_FLOOR);
            }
        }

        // 3) Multi-resolution cues (coarse FFT + coarse noise floor)
        self.compute_coarse_fft_and_update_noise(mono);

        // 4) Speech presence probability + voiced/unvoiced + f0
        let (speech_prob, voiced_prob, f0_hz) = self.estimate_speech_and_f0(sr);

        // 5) Update noise floor (main unique bins)
        // Faster attack when we believe "noise-only", slower when speech present.
        let startup_mode =
            self.noise_floor[nyq.min(self.noise_floor.len() - 1)] < NOISE_STARTUP_THRESH;
        let (alpha_att, alpha_rel) = if startup_mode {
            (NOISE_STARTUP_ATT, NOISE_STARTUP_REL)
        } else {
            let alpha_att_base = NOISE_ATT_BASE;
            let alpha_rel_base = NOISE_REL_BASE;
            let protect_noise_model = NOISE_PROTECT_BASE + NOISE_PROTECT_RANGE * speech_prob;
            (
                lerp(alpha_att_base, NOISE_ATT_MAX, protect_noise_model),
                lerp(alpha_rel_base, NOISE_REL_MAX, protect_noise_model),
            )
        };

        for i in 0..=nyq {
            let mag = self.mag[i];
            let nf = self.noise_floor[i];
            self.noise_floor[i] = if mag < nf {
                nf * alpha_att + mag * (1.0 - alpha_att)
            } else {
                nf * alpha_rel + mag * (1.0 - alpha_rel)
            };
            self.noise_floor[i] = self.noise_floor[i].max(MAG_FLOOR);
        }

        // 6) Masker curve (unique bins)
        self.compute_masker_curve(sr);

        // 7) Gain curve build (unique bins)
        let voiced = voiced_prob > VOICED_PROB_MIN;

        for i in 0..=nyq {
            let mag = self.mag[i];
            let nf = self.noise_floor[i];

            let freq_fraction = (i as f32) / (nyq.max(1) as f32);

            // Tone bias (existing behavior)
            let bias = if tone < TONE_SPLIT {
                let t = (tone * TONE_SCALE).clamp(0.0, 1.0);
                db_to_gain(TONE_BIAS_DB * (1.0 - t) * (1.0 - freq_fraction))
            } else {
                let t = ((tone - TONE_SPLIT) * TONE_SCALE).clamp(0.0, 1.0);
                db_to_gain(TONE_BIAS_DB * t * freq_fraction)
            };

            // Speech weighting by band
            let band_speech_weight = if voiced {
                let mid = bell(freq_fraction, VOICED_MID_CENTER, VOICED_MID_WIDTH);
                VOICED_SPEECH_BASE + VOICED_SPEECH_RANGE * mid
            } else {
                let hi = smoothstep(UNVOICED_HF_MIN, UNVOICED_HF_MAX, freq_fraction);
                UNVOICED_SPEECH_BASE + UNVOICED_SPEECH_RANGE * hi
            };

            let spp = (speech_prob * band_speech_weight).clamp(0.0, 1.0);

            // Base threshold
            let mut thresh = nf * (1.0 + sensitivity * THRESH_SENS_SCALE) * bias;

            // If speech likely, raise threshold so we reduce less
            thresh *= 1.0 + SPEECH_THRESH_SCALE * spp;

            // Raw gain (subtraction-ish)
            let mut raw_gain = if mag <= thresh {
                let depth = (mag / (thresh + MAG_FLOOR))
                    .clamp(0.0, 1.0)
                    .powf(RAW_GAIN_POWER);
                1.0 - (amt * (1.0 - depth))
            } else {
                1.0
            };

            // Strength shaping
            let strength = lerp(STRENGTH_MIN, STRENGTH_MAX, amt);
            raw_gain = raw_gain.powf(strength);

            // Psychoacoustic masking heuristic
            let masker = self.masker_buf[i].max(MAG_FLOOR);
            let mask_ratio = (masker / (masker + nf)).clamp(0.0, 1.0);

            let floor_scale = lerp(1.0, FLOOR_SCALE_MIN, amt);
            let speech_floor_scale = lerp(1.0, SPEECH_FLOOR_SCALE_MIN, amt);

            let psycho_floor = (PSYCHO_FLOOR_BASE + PSYCHO_FLOOR_RANGE * (1.0 - mask_ratio))
                .clamp(PSYCHO_FLOOR_MIN, PSYCHO_FLOOR_MAX)
                * floor_scale;

            let speech_floor = (SPEECH_FLOOR_BASE + SPEECH_FLOOR_RANGE * spp)
                .clamp(SPEECH_FLOOR_MIN, SPEECH_FLOOR_MAX)
                * speech_floor_scale;

            let min_floor = if amt <= BYPASS_AMOUNT_EPS {
                0.0
            } else {
                lerp(psycho_floor, speech_floor, spp)
            };

            self.gain_buf[i] = raw_gain.max(min_floor);
        }

        // 8) Spectral smoothing (unique bins)
        if amt > 0.0 {
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

        // 9) Temporal smoothing (unique bins)
        if amt > 0.0 {
            let release_limit = lerp(RELEASE_LIMIT_MIN, RELEASE_LIMIT_MAX, speech_prob);
            for i in 0..=nyq {
                if self.gain_buf[i] < self.prev_gains[i] {
                    self.gain_buf[i] = self.gain_buf[i].max(self.prev_gains[i] * release_limit);
                }
                self.prev_gains[i] = self.gain_buf[i];
            }
        }

        // 10) Harmonic protection (voiced only)
        if amt > 0.0 && voiced && f0_hz > HARMONIC_F0_MIN_HZ && f0_hz < HARMONIC_F0_MAX_HZ {
            self.apply_harmonic_protection(sr, f0_hz, speech_prob, amt);
        }

        &self.gain_buf
    }

    fn compute_coarse_fft_and_update_noise(&mut self, mono: &[f32]) {
        let n2 = self.win_size_coarse;
        let nyq2 = n2 / 2;

        // Take first n2 samples (simple + stable)
        for i in 0..n2 {
            let x = mono[i] * self.window_coarse[i];
            self.scratch_coarse[i] = Complex::new(x, 0.0);
        }

        self.fft_coarse.process(&mut self.scratch_coarse);

        // Coarse noise floor update (unique bins)
        let alpha_att = NOISE_COARSE_ATT;
        let alpha_rel = NOISE_COARSE_REL;

        for i in 0..=nyq2 {
            let mag = self.scratch_coarse[i].norm().max(MAG_FLOOR);
            let nf = self.noise_floor_coarse[i];
            self.noise_floor_coarse[i] = if mag < nf {
                nf * alpha_att + mag * (1.0 - alpha_att)
            } else {
                nf * alpha_rel + mag * (1.0 - alpha_rel)
            };
            self.noise_floor_coarse[i] = self.noise_floor_coarse[i].max(MAG_FLOOR);
        }
    }

    fn estimate_speech_and_f0(&mut self, sample_rate: f32) -> (f32, f32, f32) {
        let (periodicity, f0_hz) = estimate_f0_autocorr(&self.frame_time, &mut self.f0_scratch, sample_rate);

        let nyq = (self.win_size / 2).max(1);
        let eps = MAG_FLOOR;

        // Spectral flatness using unique bins
        let mut geo = 0.0f32;
        let mut arith = 0.0f32;
        for i in 1..nyq {
            let m = self.mag[i].max(eps);
            geo += m.ln();
            arith += m;
        }
        let geo_mean = (geo / (nyq as f32)).exp();
        let arith_mean = arith / (nyq as f32);
        let flatness = (geo_mean / (arith_mean + eps)).clamp(0.0, 1.0);

        // HF ratio cue using unique bins
        let hf_start = (nyq as f32 * HF_SPLIT_FRAC) as usize;
        let mut hf = 0.0f32;
        let mut lf = 0.0f32;
        for i in 1..nyq {
            let m = self.mag[i];
            if i >= hf_start {
                hf += m;
            } else {
                lf += m;
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

        let rms = frame_rms(&self.frame_time).clamp(0.0, 1.0);
        let energy_gate = smoothstep(ENERGY_GATE_MIN, ENERGY_GATE_MAX, rms);
        speech_prob *= energy_gate;

        (speech_prob, voiced_prob, f0_hz)
    }

    fn compute_masker_curve(&mut self, sample_rate: f32) {
        let n = self.win_size;
        let nyq = (n / 2).max(1);

        for v in self.masker_buf.iter_mut() {
            *v = 0.0;
        }

        // Peak pick on magnitude (unique bins) - reuse pre-allocated buffer
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

        self.peaks_buf.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        self.peaks_buf.truncate(self.peaks_buf.len().min(MASKER_MAX_PEAKS));

        let bin_width = sample_rate / n as f32;
        for &(center, amp) in &self.peaks_buf {
            let freq = center as f32 * bin_width;
            let frac = (freq / (sample_rate * NYQUIST_FRAC)).clamp(0.0, 1.0);
            let radius = lerp(MASKER_RADIUS_MIN, MASKER_RADIUS_MAX, frac) as isize;
            let alpha = lerp(MASKER_ALPHA_MIN, MASKER_ALPHA_MAX, frac);

            let c = center as isize;
            for d in -radius..=radius {
                let j = c + d;
                if j <= 0 || j as usize >= nyq {
                    continue;
                }
                let w = (-((d.abs() as f32) / alpha)).exp();
                let val = amp * w;
                let jj = j as usize;
                if val > self.masker_buf[jj] {
                    self.masker_buf[jj] = val;
                }
            }
        }
    }

    fn apply_harmonic_protection(
        &mut self,
        sample_rate: f32,
        f0_hz: f32,
        speech_prob: f32,
        amt: f32,
    ) {
        let n = self.win_size;
        let nyq = (n / 2).max(1);
        let bin_width = sample_rate / n as f32;

        let protect =
            (HARMONIC_PROTECT_BASE + HARMONIC_PROTECT_RANGE * speech_prob).clamp(
                HARMONIC_PROTECT_BASE,
                HARMONIC_PROTECT_BASE + HARMONIC_PROTECT_RANGE,
            );
        let allow = (1.0 - HARMONIC_ALLOW_SCALE * amt)
            .clamp(HARMONIC_ALLOW_MIN, HARMONIC_ALLOW_MAX);
        let min_gain_on_harmonics =
            (protect * allow).clamp(HARMONIC_MIN_GAIN_MIN, HARMONIC_MIN_GAIN_MAX);

        let max_hz = HARMONIC_MAX_HZ.min(sample_rate * NYQUIST_FRAC);
        let mut h = 1;
        loop {
            let hz = f0_hz * (h as f32);
            if hz > max_hz {
                break;
            }

            let center = (hz / bin_width).round() as isize;
            if center <= 1 || center as usize >= nyq.saturating_sub(1) {
                break;
            }

            let frac = (hz / max_hz).clamp(0.0, 1.0);
            let w = lerp(HARMONIC_WIDTH_MIN, HARMONIC_WIDTH_MAX, frac) as isize;

            for d in -w..=w {
                let b = center + d;
                if b <= 0 || b as usize >= nyq {
                    continue;
                }
                let bi = b as usize;
                self.gain_buf[bi] = self.gain_buf[bi].max(min_gain_on_harmonics);
            }

            h += 1;
            if h > HARMONIC_MAX_COUNT {
                break;
            }
        }
    }

    fn apply_hum_removal_inplace(&mut self, sample_rate: f32) {
        let n = self.win_size;
        let nyq = n / 2;
        let bin_width = sample_rate / n as f32;

        for &freq in &HUM_REMOVAL_FREQS {
            let center = (freq / bin_width).round() as usize;
            if center > 0 && center < nyq {
                self.scratch[center] *= HUM_REMOVAL_MAIN_SCALE;
                if center + 1 < nyq {
                    self.scratch[center + 1] *= HUM_REMOVAL_SIDE_SCALE;
                }
                self.scratch[center - 1] *= HUM_REMOVAL_SIDE_SCALE;
            }
        }

        let cut_bin = (HUM_REMOVAL_LOW_CUT_HZ / bin_width).ceil() as usize;
        for i in 0..cut_bin.min(nyq) {
            self.scratch[i] = Complex::new(0.0, 0.0);
        }

        // Re-enforce real-signal constraints on analysis spectrum (keeps it sane)
        self.scratch[0].im = 0.0;
        self.scratch[nyq].im = 0.0;
        for k in 1..nyq {
            let a = self.scratch[k];
            self.scratch[n - k] = a.conj();
        }
    }
}

/// Per-channel streaming applier. Owns FFT/IFFT/WOLA and streaming buffers.
/// Applies a shared gain curve (unique bins 0..=nyq).
struct StreamingDenoiserChannel {
    fft: Arc<dyn Fft<f32>>,
    ifft: Arc<dyn Fft<f32>>,
    win_size: usize,
    hop_size: usize,
    window: Vec<f32>, // sqrt-Hann

    scratch: Vec<Complex<f32>>, // full spectrum
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

        let window = make_sqrt_hann_window(win_size);

        let buf_cap = win_size * RINGBUF_CAP_MULT;
        let (in_prod, in_cons) = RingBuffer::<f32>::new(buf_cap).split();
        let (out_prod, out_cons) = RingBuffer::<f32>::new(buf_cap).split();

        // Prime output ring to avoid initial underruns (matches prior behavior)
        let mut initialized_out_prod: Producer<f32> = out_prod;
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

    #[inline]
    pub fn push_input(&mut self, x: f32) {
        let _ = self.input_producer.push(x);
    }

    #[inline]
    pub fn input_len(&self) -> usize {
        self.input_consumer.len()
    }

    pub fn peek_frame(&self, out: &mut [f32]) {
        let n = self.win_size;
        for (i, val) in self.input_consumer.iter().take(n).enumerate() {
            out[i] = *val;
        }
    }

    pub fn discard_input(&mut self, n: usize) {
        self.input_consumer.discard(n);
    }

    #[inline]
    pub fn pop_output(&mut self) -> f32 {
        self.output_consumer.pop().unwrap_or(0.0)
    }

    /// Process one frame using the shared per-bin gains (0..=nyq).
    /// Note: sample_rate parameter kept for API parity and potential future use.
    pub fn process_frame(&mut self, gains: &[f32], _sample_rate: f32) {
        let n = self.win_size;
        let nyq = n / 2;
        let has_nyquist = n % 2 == 0;

        // 0) Window input into scratch (time -> complex)
        for (i, val) in self.input_consumer.iter().take(n).enumerate() {
            let x = *val;
            self.scratch[i] = Complex::new(x * self.window[i], 0.0);
        }

        // 1) FFT
        self.fft.process(&mut self.scratch);

        // 2) Apply gains to unique bins and enforce Hermitian symmetry
        for i in 0..=nyq {
            self.scratch[i] *= gains[i].clamp(0.0, 1.0);
        }

        self.scratch[0].im = 0.0;
        if has_nyquist {
            self.scratch[nyq].im = 0.0;
        }

        for k in 1..nyq {
            let a = self.scratch[k];
            self.scratch[n - k] = a.conj();
        }

        // 3) IFFT + WOLA (sqrt-Hann synthesis + per-sample normalization)
        self.ifft.process(&mut self.scratch);
        let norm_ifft = 1.0 / n as f32;

        for i in 0..n {
            let w = self.window[i];
            let y = self.scratch[i].re * norm_ifft * w;
            self.overlap[i] += y;
            self.ola_norm[i] += w * w;
        }

        // 4) Emit hop samples with normalization
        for i in 0..self.hop_size {
            let denom = self.ola_norm[i].max(OLA_NORM_EPS);
            let out = self.overlap[i] / denom;
            let _ = self.output_producer.push(out);
        }

        // 5) Shift OLA buffers
        self.overlap.copy_within(self.hop_size..n, 0);
        self.ola_norm.copy_within(self.hop_size..n, 0);

        for i in (n - self.hop_size)..n {
            self.overlap[i] = 0.0;
            self.ola_norm[i] = 0.0;
        }
    }
}

// -----------------------------------------------------------------------------
// Optional: mono wrapper for compatibility
// -----------------------------------------------------------------------------
/// Backwards-compatible mono denoiser wrapper.
///
/// If you previously had one `StreamingDenoiser` per channel in a stereo pipeline,
/// replace that usage with `StereoStreamingDenoiser` to avoid stereo image drift.
///
/// This wrapper is for genuinely mono pipelines.
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
        // Feed mono into both sides; use left output
        let (l, _r) = self.inner.process_sample(input, input, cfg);
        l
    }
}
