//! Streaming Deverber (Late Reflection Suppression)
//!
//! Reduces late reverb tail and diffuse room decay (>50ms) to improve speech
//! clarity and reduce the perception of distance. Uses WOLA (Weighted Overlap-Add)
//! processing to maintain audio quality while removing late reflections.
//!
//! # Purpose
//! Improves speech intelligibility by reducing late reverb components that
//! contribute to a sense of distance and reduce clarity, while preserving
//! the direct signal and early reflections that contribute to presence.
//!
//! # Design Notes
//! - Handles late reverb tail and diffuse room decay (>50ms)
//! - Uses WOLA processing for high-quality results
//! - Preserves direct signal and early reflections
//! - Works in series with early reflection suppression
//! - Short-lag reflections (0-20ms) - owned by `EarlyReflectionSuppressor`
//! - Desk/wall coloration - owned by `EarlyReflectionSuppressor`
//! - Distinct flutter echoes - not modeled
//!
//! ## Avoiding Double-Reaction
//!
//! Both this module and `EarlyReflectionSuppressor` respond to "distance" cues, but
//! they target different time regions:
//! - Early reflections (0-20ms): Cause coloration/boxiness
//! - Late reflections (>50ms): Cause diffuse room sound
//!
//! The signal chain order (early reflection → denoise → deverb) ensures each
//! processor sees appropriate input without double-processing the same artifact.
//!
//! # Perceptual Contract
//! - **Target Source**: Speech in reverberant environments (rooms, halls).
//! - **Intended Effect**: Reduce late reverberation tail and diffuse room sound to "tighten" the voice.
//! - **Failure Modes**:
//!   - Dryness / artificiality if over-processed.
//!   - "Gated" silence if envelope follower is too fast.
//!   - Loss of natural phrase endings (sustain).
//! - **Will Not Do**:
//!   - Remove distinct echoes (flutter echo).
//!   - Suppress early reflections (handled by `EarlyReflectionSuppressor`).
//!
//! # Lifecycle
//! - **Learning**: No specific learning phase (instant reaction).
//! - **Active**: Normal operation.
//! - **Holding**: Uses `Holding` state implicitly during silence to prevent release envelope drift.
//! - **Bypassed**: Passes audio through.

use crate::dsp::utils::{
    aggressive_tail, estimate_f0_autocorr, lerp, make_sqrt_hann_window, max3, smoothstep,
    BYPASS_AMOUNT_EPS, MAG_FLOOR,
};
use ringbuf::{Consumer, Producer, RingBuffer};
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::sync::Arc;

// Constants: unless marked "Must not change", these are tunable for behavior.

// Nyquist fraction used for normalization (fs/2).
// Must not change: defines Nyquist normalization.
const NYQUIST_FRAC: f32 = 0.5;
// OLA normalization floor.
// Increasing: more conservative to avoid divide-by-zero; decreasing: closer to raw.
const OLA_NORM_EPS: f32 = 1e-6;
// Voicing periodicity threshold.
// Increasing: stricter voicing detection; decreasing: looser voicing detection.
const VOICED_PERIODICITY_MIN: f32 = 0.55;
// Voicing f0 range (Hz).
// Increasing min: ignores lower voices; decreasing: includes lower voices.
const VOICED_F0_MIN_HZ: f32 = 70.0;
// Increasing max: includes higher voices; decreasing: excludes higher voices.
const VOICED_F0_MAX_HZ: f32 = 320.0;
// Spectral flux thresholds for transient detection.
// Increasing: fewer frames marked transient; decreasing: more frames marked transient.
const TRANSIENT_FLUX_MIN: f32 = 0.03;
const TRANSIENT_FLUX_MAX: f32 = 0.18;
// Rise gate scale for early-hold detection.
// Increasing: harder to register rises; decreasing: easier to register rises.
const RISE_GATE_SCALE: f32 = 0.10;
// Rise gate epsilon.
// Increasing: more conservative; decreasing: closer to raw.
const RISE_GATE_EPS: f32 = 1e-6;
// Late decay curve endpoints (low->high frequency).
// Increasing min: slower decay at LF; decreasing: faster decay at LF.
const LATE_DECAY_LOW: f32 = 0.995;
// Increasing max: slower decay at HF; decreasing: faster decay at HF.
const LATE_DECAY_HIGH: f32 = 0.85;
// Late envelope attack (rise).
// Increasing: slower rise; decreasing: faster rise.
const LATE_RISE: f32 = 0.9995;
// Early-hold decay bounds (strength-dependent).
// Increasing min: early-hold decays slower; decreasing: faster.
const EARLY_HOLD_DECAY_MIN: f32 = 0.80;
// Increasing max: early-hold decays slower at high strength; decreasing: faster.
const EARLY_HOLD_DECAY_MAX: f32 = 0.92;
// Late envelope clamp scale.
// Increasing: allows larger late tail; decreasing: tighter clamp.
const LATE_ENV_MAX_SCALE: f32 = 1.10;
// Direct floor scale relative to magnitude.
// Increasing: preserves more direct signal; decreasing: allows deeper attenuation.
const DIRECT_FLOOR_SCALE: f32 = 0.02;
// Floor shaping for transient contribution.
// Increasing: higher floor on transients; decreasing: lower floor.
const FLOOR_TRANSIENT_MIN: f32 = 0.12;
const FLOOR_TRANSIENT_MAX: f32 = 0.55;
// Floor shaping for masker contribution.
// Increasing: higher floor when masker is weak; decreasing: lower floor.
const FLOOR_MASK_MIN: f32 = 0.18;
const FLOOR_MASK_MAX: f32 = 0.60;
// Floor shaping for early-hold contribution.
// Increasing: higher floor when early hold is strong; decreasing: lower floor.
const FLOOR_HOLD_MIN: f32 = 0.10;
const FLOOR_HOLD_MAX: f32 = 0.55;
// Floor clamp max (floor clamp min is computed dynamically based on speech confidence).
// Increasing max: less attenuation cap; decreasing max: tighter cap.
const FLOOR_CLAMP_MAX: f32 = 0.92;
// Gain smoothing attack/release for mask updates.
// Increasing attack: faster gain rise; decreasing: slower rise.
const GAIN_SMOOTH_ATTACK: f32 = 0.35;
// Increasing release: faster gain fall; decreasing: slower fall.
const GAIN_SMOOTH_RELEASE: f32 = 0.06;
// Harmonic protection max frequency (Hz).
// Increasing: protects more HF harmonics; decreasing: protects fewer.
const HARMONIC_PROTECT_MAX_HZ: f32 = 6000.0;
// Harmonic protection bandwidth endpoints (Hz).
// Increasing min: wider protection at LF; decreasing: narrower.
const HARMONIC_BW_MIN_HZ: f32 = 45.0;
// Increasing max: wider protection at HF; decreasing: narrower.
const HARMONIC_BW_MAX_HZ: f32 = 25.0;
// Harmonic protection gain endpoints (linear).
// Increasing min/max: more protection; decreasing: less protection.
const HARMONIC_PROTECT_MIN: f32 = 0.55;
const HARMONIC_PROTECT_MAX: f32 = 0.35;
// Masker peak search radius (bins).
// Increasing: wider masking spread; decreasing: narrower spread.
const MASKER_RADIUS_BINS: isize = 20;
// Masker exponential falloff divisor.
// Increasing: slower falloff; decreasing: faster falloff.
const MASKER_FALLOFF_DENOM: f32 = 8.0;

/// Mono streaming deverber using WOLA.
///
/// ## Metric Ownership (EXCLUSIVE)
/// This module OWNS and is responsible for:
/// - **Early/Late ratio**: Moves toward target range (0.50 - 0.70)
/// - **Decay slope**: Reduces toward target range (-0.0001 to +0.0001)
///
/// ## Dynamics Ownership Boundary
/// - **DOES NOT OWN**: RMS, crest factor, RMS variance (these are owned by `LinkedCompressor`/Leveler)
/// - **COORDINATION**: Works in series with Leveler - receives pre-processed signal, applies reverb reduction,
///   then Leveler applies final dynamics control
/// - **NO OVERLAP**: Does not attempt to control loudness or macro dynamics that belong to Leveler
///
/// This module must NOT attempt to modify:
/// - RMS, crest factor, RMS variance (owned by Leveler)
/// - Noise floor, SNR (owned by Denoiser)
/// - Presence/Air ratios (owned by Proximity + Clarity)
/// - HF variance (read-only guardrail metric)
pub struct StreamingDeverber {
    detector: StereoDeverberDetector, // Reusing the existing detector logic
    fft: Arc<dyn Fft<f32>>,
    ifft: Arc<dyn Fft<f32>>,

    win_size: usize,
    hop_size: usize,
    window: Vec<f32>,

    scratch: Vec<Complex<f32>>,
    fft_scratch: Vec<Complex<f32>>,
    ifft_scratch: Vec<Complex<f32>>,
    overlap: Vec<f32>,
    ola_norm: Vec<f32>,
    frame_in: Vec<f32>,

    input_producer: Producer<f32>,
    input_consumer: Consumer<f32>,
    output_producer: Producer<f32>,
    output_consumer: Consumer<f32>,
}

impl StreamingDeverber {
    pub fn new(win_size: usize, hop_size: usize) -> Self {
        let detector = StereoDeverberDetector::new(win_size, hop_size);

        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(win_size);
        let ifft = planner.plan_fft_inverse(win_size);

        let fft_scratch_len = fft.get_inplace_scratch_len();
        let ifft_scratch_len = ifft.get_inplace_scratch_len();

        let fft_scratch = vec![Complex::default(); fft_scratch_len];
        let ifft_scratch = vec![Complex::default(); ifft_scratch_len];

        let window = make_sqrt_hann_window(win_size);

        let buf_cap = win_size * 4;
        let (in_prod, in_cons) = RingBuffer::<f32>::new(buf_cap).split();
        let (out_prod, out_cons) = RingBuffer::<f32>::new(buf_cap).split();

        // Prime output
        let mut out_prod_init = out_prod;
        for _ in 0..win_size {
            let _ = out_prod_init.push(0.0);
        }

        Self {
            detector,
            fft,
            ifft,
            win_size,
            hop_size,
            window,
            scratch: vec![Complex::new(0.0, 0.0); win_size],
            fft_scratch,
            ifft_scratch,
            overlap: vec![0.0; win_size],
            ola_norm: vec![0.0; win_size],
            frame_in: vec![0.0; win_size],
            input_producer: in_prod,
            input_consumer: in_cons,
            output_producer: out_prod_init,
            output_consumer: out_cons,
        }
    }

    pub fn process_sample(
        &mut self,
        input: f32,
        amount: f32,
        sample_rate: f32,
        speech_confidence: f32,
        clarity_amount: f32,
        proximity_amount: f32,
    ) -> f32 {
        if amount <= BYPASS_AMOUNT_EPS {
            return input;
        }

        // Apply aggressive_tail curve to amount
        let mut strength = aggressive_tail(amount);

        // Inter-module clamp: reduce strength by 25% if clarity > 0.6
        if clarity_amount > 0.6 {
            strength *= 0.75;
        }

        let _ = self.input_producer.push(input);

        if self.input_consumer.len() >= self.win_size {
            // Read frame
            for (i, v) in self.input_consumer.iter().take(self.win_size).enumerate() {
                self.frame_in[i] = *v;
            }

            // Analyze -> Gains
            // (Note: analyze returns slice of gains 0..=nyq)
            // We need to copy gains because analyze borrows self.detector
            // But we can just use the gains immediately if we structure it right.
            // However, we need to apply gains to the FFT of the input.
            // The detector does FFT internally too?
            // StereoDeverberDetector::analyze does FFT on the input frame.
            // So we can reuse that if the detector exposed it, but it computes gains.

            // Let's run analysis
            let gains = self.detector.analyze(
                &self.frame_in,
                strength,
                sample_rate,
                speech_confidence,
                proximity_amount,
            ); // This mutates detector

            // Now we do the application WOLA

            // 1. Window + FFT
            for i in 0..self.win_size {
                self.scratch[i] = Complex::new(self.frame_in[i] * self.window[i], 0.0);
            }
            #[cfg(debug_assertions)]
            assert_no_alloc::assert_no_alloc(|| {
                self.fft
                    .process_with_scratch(&mut self.scratch, &mut self.fft_scratch);
            });
            #[cfg(not(debug_assertions))]
            self.fft
                .process_with_scratch(&mut self.scratch, &mut self.fft_scratch);

            // 2. Apply gains
            let nyq = self.win_size / 2;
            for i in 0..=nyq {
                // gains has length nyq+1
                self.scratch[i] *= gains[i];
            }
            // Hermite
            self.scratch[0].im = 0.0;
            self.scratch[nyq].im = 0.0;
            for k in 1..nyq {
                let c = self.scratch[k].conj();
                self.scratch[self.win_size - k] = c;
            }

            // 3. IFFT + Overlap
            #[cfg(debug_assertions)]
            assert_no_alloc::assert_no_alloc(|| {
                self.ifft
                    .process_with_scratch(&mut self.scratch, &mut self.ifft_scratch);
            });
            #[cfg(not(debug_assertions))]
            self.ifft
                .process_with_scratch(&mut self.scratch, &mut self.ifft_scratch);
            let norm = 1.0 / self.win_size as f32;

            for i in 0..self.win_size {
                let w = self.window[i];
                let y = self.scratch[i].re * norm * w;
                self.overlap[i] += y;
                self.ola_norm[i] += w * w;
            }

            // 4. Output hop
            for i in 0..self.hop_size {
                let d = self.ola_norm[i].max(OLA_NORM_EPS);
                let _ = self.output_producer.push(self.overlap[i] / d);
            }

            // 5. Shift
            self.overlap.copy_within(self.hop_size..self.win_size, 0);
            self.ola_norm.copy_within(self.hop_size..self.win_size, 0);
            for i in (self.win_size - self.hop_size)..self.win_size {
                self.overlap[i] = 0.0;
                self.ola_norm[i] = 0.0;
            }

            self.input_consumer.discard(self.hop_size);
        }

        self.output_consumer.pop().unwrap_or(0.0)
    }

    pub fn reset(&mut self) {
        self.detector.reset();
        self.overlap.fill(0.0);
        self.ola_norm.fill(0.0);
        while self.input_consumer.pop().is_some() {}
        while self.output_consumer.pop().is_some() {}

        // Prime output with zeros again
        for _ in 0..self.win_size {
            let _ = self.output_producer.push(0.0);
        }
    }
}

pub struct StereoDeverberDetector {
    fft: Arc<dyn Fft<f32>>,
    win_size: usize,
    #[allow(dead_code)]
    hop_size: usize,
    window: Vec<f32>,

    // Analysis buffers
    scratch: Vec<Complex<f32>>,
    fft_scratch: Vec<Complex<f32>>,
    mag: Vec<f32>,

    prev_mag: Vec<f32>,
    late_env: Vec<f32>,
    early_hold: Vec<f32>,
    masker: Vec<f32>,
    gain_mask: Vec<f32>,

    frame_time: Vec<f32>,
    gain_smooth: Vec<f32>,

    // Pre-allocated buffer for F0 autocorrelation (avoids audio-thread allocation)
    f0_scratch: Vec<f32>,
}

impl StereoDeverberDetector {
    pub fn new(win_size: usize, hop_size: usize) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(win_size);

        let fft_scratch_len = fft.get_inplace_scratch_len();

        let window = make_sqrt_hann_window(win_size);

        let nyq = win_size / 2;

        Self {
            fft,
            win_size,
            hop_size,
            window,
            scratch: vec![Complex::new(0.0, 0.0); win_size],
            fft_scratch: vec![Complex::default(); fft_scratch_len],
            mag: vec![0.0; nyq + 1],
            prev_mag: vec![0.0; nyq + 1],
            late_env: vec![0.0; nyq + 1],
            early_hold: vec![0.0; nyq + 1],
            masker: vec![0.0; nyq + 1],
            gain_mask: vec![1.0; nyq + 1],
            frame_time: vec![0.0; win_size],
            gain_smooth: vec![1.0; nyq + 1],
            f0_scratch: vec![0.0; win_size], // Changed from Vec::with_capacity to pre-allocated vector
        }
    }

    pub fn analyze(
        &mut self,
        mono: &[f32],
        strength: f32,
        sample_rate: f32,
        speech_confidence: f32,
        proximity_amount: f32,
    ) -> &[f32] {
        let n = self.win_size;
        let nyq = n / 2;
        let sr = sample_rate;

        // Buffer size assertions for real-time safety
        debug_assert_eq!(mono.len(), n, "Input mono buffer size mismatch");
        debug_assert_eq!(self.scratch.len(), n, "Scratch buffer size mismatch");
        debug_assert_eq!(self.frame_time.len(), n, "Frame time buffer size mismatch");
        debug_assert_eq!(self.window.len(), n, "Window buffer size mismatch");
        debug_assert_eq!(self.mag.len(), nyq + 1, "Magnitude buffer size mismatch");
        debug_assert_eq!(
            self.prev_mag.len(),
            nyq + 1,
            "Previous magnitude buffer size mismatch"
        );
        debug_assert_eq!(
            self.late_env.len(),
            nyq + 1,
            "Late envelope buffer size mismatch"
        );
        debug_assert_eq!(
            self.early_hold.len(),
            nyq + 1,
            "Early hold buffer size mismatch"
        );
        debug_assert_eq!(self.masker.len(), nyq + 1, "Masker buffer size mismatch");
        debug_assert_eq!(
            self.gain_mask.len(),
            nyq + 1,
            "Gain mask buffer size mismatch"
        );
        debug_assert_eq!(
            self.gain_smooth.len(),
            nyq + 1,
            "Gain smooth buffer size mismatch"
        );

        // Window + FFT
        for i in 0..n {
            self.frame_time[i] = mono[i];
            self.scratch[i] = Complex::new(mono[i] * self.window[i], 0.0);
        }

        #[cfg(debug_assertions)]
        assert_no_alloc::assert_no_alloc(|| {
            self.fft
                .process_with_scratch(&mut self.scratch, &mut self.fft_scratch);
        });
        #[cfg(not(debug_assertions))]
        self.fft
            .process_with_scratch(&mut self.scratch, &mut self.fft_scratch);

        for i in 0..=nyq {
            self.mag[i] = self.scratch[i].norm().max(MAG_FLOOR);
        }

        // Voicing
        let (periodicity, f0) = estimate_f0_autocorr(&self.frame_time, &mut self.f0_scratch, sr);
        let voiced =
            periodicity > VOICED_PERIODICITY_MIN && f0 > VOICED_F0_MIN_HZ && f0 < VOICED_F0_MAX_HZ;

        // Spectral flux
        let mut flux = 0.0;
        let mut energy = 0.0;
        for i in 1..=nyq {
            flux += (self.mag[i] - self.prev_mag[i]).max(0.0);
            energy += self.mag[i];
        }

        let transient = smoothstep(
            TRANSIENT_FLUX_MIN,
            TRANSIENT_FLUX_MAX,
            flux / (energy + MAG_FLOOR),
        );

        self.compute_masker_curve();

        let bin_width = sr / n as f32;
        let late_k = strength;

        // Speech-aware floor clamping
        let floor_clamp_min = if speech_confidence > 0.5 {
            0.08 // During speech: less aggressive
        } else {
            0.04 // During silence: more aggressive
        };

        let mut gain_sum: f32 = 0.0;
        let mut min_gain: f32 = 1.0;
        for i in 0..=nyq {
            let mag = self.mag[i];
            let prev = self.prev_mag[i];

            let freq = i as f32 * bin_width;
            let frac = (freq / (sr * NYQUIST_FRAC)).clamp(0.0, 1.0);

            // Base decay
            let mut decay = lerp(LATE_DECAY_LOW, LATE_DECAY_HIGH, frac);

            // Inter-module clamp: reduce HF decay by 20% if proximity > 0.6
            // This means decay coefficient increases (slower decay = less deverb)
            if proximity_amount > 0.6 && frac > 0.5 {
                // Scale decay back toward 1.0 (no decay) by 20%
                decay = decay + (1.0 - decay) * 0.2;
            }
            let rise = (mag - prev).max(0.0);
            let rise_gate = smoothstep(0.0, prev * RISE_GATE_SCALE + RISE_GATE_EPS, rise);

            self.early_hold[i] = (self.early_hold[i]
                * lerp(EARLY_HOLD_DECAY_MIN, EARLY_HOLD_DECAY_MAX, 1.0 - strength))
            .max(mag * rise_gate);

            let mut late = self.late_env[i];
            if mag < late {
                late = late * decay + mag * (1.0 - decay);
            } else {
                late = late * LATE_RISE + mag * (1.0 - LATE_RISE);
            }
            late = late.min(mag * LATE_ENV_MAX_SCALE + RISE_GATE_EPS);

            self.late_env[i] = late;
            self.prev_mag[i] = mag;

            let direct = (mag - late_k * late).max(mag * DIRECT_FLOOR_SCALE);
            let mut gain = (direct / mag).clamp(0.0, 1.0);

            let floor = max3(
                lerp(FLOOR_TRANSIENT_MIN, FLOOR_TRANSIENT_MAX, transient),
                lerp(
                    FLOOR_MASK_MIN,
                    FLOOR_MASK_MAX,
                    1.0 - (self.masker[i] / (self.masker[i] + late + MAG_FLOOR)),
                ),
                lerp(
                    FLOOR_HOLD_MIN,
                    FLOOR_HOLD_MAX,
                    smoothstep(0.0, mag * 0.25 + RISE_GATE_EPS, self.early_hold[i]),
                ),
            )
            .clamp(floor_clamp_min, FLOOR_CLAMP_MAX);

            gain = gain.max(floor);

            if voiced {
                gain = self.apply_harmonic_protection(i, gain, f0, sr, strength);
            }

            let prev_g = self.gain_smooth[i];
            self.gain_smooth[i] = if gain > prev_g {
                prev_g + (gain - prev_g) * GAIN_SMOOTH_ATTACK
            } else {
                prev_g + (gain - prev_g) * GAIN_SMOOTH_RELEASE
            };

            self.gain_mask[i] = self.gain_smooth[i];
            gain_sum += self.gain_mask[i];
            min_gain = min_gain.min(self.gain_mask[i]);
        }

        let _avg_gain = gain_sum / (nyq.max(1) as f32);
        let _min_gain = min_gain;

        &self.gain_mask
    }

    fn compute_masker_curve(&mut self) {
        let nyq = self.mag.len() - 1;
        self.masker.fill(0.0);

        for i in 2..nyq - 2 {
            let m = self.mag[i];
            if m > self.mag[i - 1]
                && m > self.mag[i + 1]
                && m > self.mag[i - 2]
                && m > self.mag[i + 2]
            {
                let radius = MASKER_RADIUS_BINS;
                for d in -radius..=radius {
                    let j = (i as isize + d).clamp(0, nyq as isize) as usize;
                    let w = (-((d.abs() as f32) / MASKER_FALLOFF_DENOM)).exp();
                    self.masker[j] = self.masker[j].max(m * w);
                }
            }
        }
    }

    fn apply_harmonic_protection(
        &self,
        bin: usize,
        gain: f32,
        f0: f32,
        sr: f32,
        strength: f32,
    ) -> f32 {
        if f0 <= 0.0 {
            return gain;
        }

        // Bounds check to prevent out-of-bounds access
        let nyq = self.win_size / 2;
        if bin > nyq {
            return gain;
        }

        let bin_hz = bin as f32 * sr / self.win_size as f32;
        if bin_hz > HARMONIC_PROTECT_MAX_HZ {
            return gain;
        }

        let h = (bin_hz / f0).round().max(1.0);
        let dist = (bin_hz - h * f0).abs();
        let bw = lerp(
            HARMONIC_BW_MIN_HZ,
            HARMONIC_BW_MAX_HZ,
            (bin_hz / HARMONIC_PROTECT_MAX_HZ).clamp(0.0, 1.0),
        );
        let near = 1.0 - smoothstep(0.0, bw, dist);

        let protect = lerp(HARMONIC_PROTECT_MIN, HARMONIC_PROTECT_MAX, strength);
        gain.max(lerp(gain, protect, near))
    }

    pub fn reset(&mut self) {
        self.mag.fill(0.0);
        self.prev_mag.fill(0.0);
        self.late_env.fill(0.0);
        self.early_hold.fill(0.0);
        self.masker.fill(0.0);
        self.gain_mask.fill(1.0);
        self.gain_smooth.fill(1.0);
    }
}
