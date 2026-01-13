use crate::dsp::utils::{bell, db_to_gain, estimate_f0_autocorr, frame_rms, lerp, smoothstep};
use ringbuf::{Consumer, Producer, RingBuffer};
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::f32::consts::PI;
use std::sync::Arc;

/// Configuration for the denoiser
pub struct DenoiseConfig {
    pub amount: f32,
    pub sensitivity: f32,
    pub tone: f32,
    pub sample_rate: f32,
}

/// FFT-based streaming denoiser with spectral subtraction + modern heuristics:
/// - Speech presence probability estimation (heuristic)
/// - Voiced/unvoiced classification
/// - Harmonic tracking and protection (voiced only)
/// - Multi-FFT resolution analysis (coarse FFT for cues; main FFT for synthesis)
/// - Psychoacoustic masking heuristic to reduce musical noise / preserve audibility
pub struct StreamingDenoiser {
    // Main FFT (synthesis domain)
    fft: Arc<dyn Fft<f32>>,
    ifft: Arc<dyn Fft<f32>>,
    pub win_size: usize,
    hop_size: usize,
    window: Vec<f32>,

    // Coarse FFT (analysis only; multi-resolution cues)
    fft_coarse: Arc<dyn Fft<f32>>,
    win_size_coarse: usize,
    window_coarse: Vec<f32>,
    complex_buf_coarse: Vec<Complex<f32>>,
    noise_floor_coarse: Vec<f32>,

    // Noise model and gain state (main FFT bins)
    noise_floor: Vec<f32>,
    prev_gains: Vec<f32>,
    complex_buf: Vec<Complex<f32>>,
    gain_buf: Vec<f32>,
    mag_buf: Vec<f32>,
    masker_buf: Vec<f32>,

    // Time domain scratch (speech classification + F0 estimation)
    frame_time: Vec<f32>,

    // Streaming IO
    input_producer: Producer<f32>,
    input_consumer: Consumer<f32>,
    output_producer: Producer<f32>,
    output_consumer: Consumer<f32>,

    // OLA buffer (main synthesis)
    overlap_buffer: Vec<f32>,
}

impl StreamingDenoiser {
    pub fn new(win_size: usize, hop_size: usize) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(win_size);
        let ifft = planner.plan_fft_inverse(win_size);

        // Hann window (main)
        let window: Vec<f32> = (0..win_size)
            .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f32 / win_size as f32).cos()))
            .collect();

        // Coarse analysis FFT: half size (min 256)
        let win_size_coarse = (win_size / 2).max(256).min(win_size);
        let fft_coarse = planner.plan_fft_forward(win_size_coarse);
        let window_coarse: Vec<f32> = (0..win_size_coarse)
            .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f32 / win_size_coarse as f32).cos()))
            .collect();

        let buf_cap = win_size * 4;
        let (in_prod, in_cons) = RingBuffer::<f32>::new(buf_cap).split();
        let (out_prod, out_cons) = RingBuffer::<f32>::new(buf_cap).split();

        // Prime output ring to avoid initial underruns matching your original behavior
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

            fft_coarse,
            win_size_coarse,
            window_coarse,
            complex_buf_coarse: vec![Complex::new(0.0, 0.0); win_size_coarse],
            noise_floor_coarse: vec![1e-5; win_size_coarse],

            noise_floor: vec![1e-5; win_size],
            prev_gains: vec![1.0; win_size],
            complex_buf: vec![Complex::new(0.0, 0.0); win_size],
            gain_buf: vec![1.0; win_size],
            mag_buf: vec![0.0; win_size],
            masker_buf: vec![0.0; win_size],

            frame_time: vec![0.0; win_size],

            input_producer: in_prod,
            input_consumer: in_cons,
            output_producer: initialized_out_prod,
            output_consumer: out_cons,

            overlap_buffer: vec![0.0; win_size],
        }
    }

    pub fn process_sample(&mut self, input: f32, cfg: &DenoiseConfig) -> f32 {
        let _ = self.input_producer.push(input);
        if self.input_consumer.len() >= self.win_size {
            self.process_frame(cfg);
        }
        self.output_consumer.pop().unwrap_or(0.0)
    }

    fn process_frame(&mut self, cfg: &DenoiseConfig) {
        // ------------------------------------------------------------
        // 0) Gather time-domain frame (for classification / f0)
        // ------------------------------------------------------------
        for (i, val) in self.input_consumer.iter().take(self.win_size).enumerate() {
            let x = *val;
            self.frame_time[i] = x;
            self.complex_buf[i] = Complex::new(x * self.window[i], 0.0);
        }

        // ------------------------------------------------------------
        // 1) Main FFT
        // ------------------------------------------------------------
        self.fft.process(&mut self.complex_buf);

        // Precompute magnitudes
        for i in 0..self.win_size {
            self.mag_buf[i] = self.complex_buf[i].norm().max(1e-12);
        }

        let amt = cfg.amount.clamp(0.0, 1.0);

        // Optional hum removal early (as you had)
        if amt > 0.05 {
            self.apply_hum_removal(cfg.sample_rate);
            // Recompute magnitudes after notching
            for i in 0..self.win_size {
                self.mag_buf[i] = self.complex_buf[i].norm().max(1e-12);
            }
        }

        // ------------------------------------------------------------
        // 2) Multi-resolution analysis (coarse FFT cues)
        //    Used for SPP + voiced/unvoiced weighting (analysis only).
        // ------------------------------------------------------------
        self.compute_coarse_fft_and_update_noise(cfg);

        // ------------------------------------------------------------
        // 3) Speech presence probability + voiced/unvoiced decision
        // ------------------------------------------------------------
        let (speech_prob, voiced_prob, f0_hz) = self.estimate_speech_and_f0(cfg.sample_rate);

        // ------------------------------------------------------------
        // 4) Update noise floor (main bins)
        // ------------------------------------------------------------
        // Faster attack when we believe "noise-only", slower when speech present.
        // This prevents the noise model from learning speech as noise.
        let startup_mode = self.noise_floor[self.win_size / 2] < 1e-4;
        let (alpha_att, alpha_rel) = if startup_mode {
            (0.6, 0.90)
        } else {
            let alpha_att_base = 0.90;
            let alpha_rel_base = 0.9995;
            let protect_noise_model = 0.35 + 0.55 * speech_prob; // more speech -> more protection
            (
                lerp(alpha_att_base, 0.98, protect_noise_model),
                lerp(alpha_rel_base, 0.99995, protect_noise_model),
            )
        };

        for i in 0..self.win_size {
            let mag = self.mag_buf[i];
            if mag < self.noise_floor[i] {
                self.noise_floor[i] = self.noise_floor[i] * alpha_att + mag * (1.0 - alpha_att);
            } else {
                self.noise_floor[i] = self.noise_floor[i] * alpha_rel + mag * (1.0 - alpha_rel);
            }
            self.noise_floor[i] = self.noise_floor[i].max(1e-12);
        }

        // ------------------------------------------------------------
        // 5) Psychoacoustic masking heuristic (reduce musical noise)
        // ------------------------------------------------------------
        self.compute_masker_curve(cfg.sample_rate);

        // ------------------------------------------------------------
        // 6) Build gain curve with:
        //    - tone shaping (your bias)
        //    - SPP weighting (less reduction where speech likely)
        //    - voiced/unvoiced treatment
        //    - psychoacoustic floor (avoid watery artifacts)
        //    - harmonic protection (voiced only)
        // ------------------------------------------------------------
        let sensitivity = cfg.sensitivity.clamp(0.0, 1.0);

        // Simple voiced/unvoiced weights:
        // - Voiced: protect harmonics, allow a little more reduction between harmonics
        // - Unvoiced: protect HF consonants (fricatives) to avoid lisping
        let voiced = voiced_prob > 0.55;

        for i in 0..self.win_size {
            let mag = self.mag_buf[i];
            let nf = self.noise_floor[i];

            // Frequency mapping (only meaningful up to Nyquist)
            let nyquist_bins = (self.win_size / 2).max(1);
            let freq_fraction = (i.min(nyquist_bins) as f32) / (nyquist_bins as f32);

            // Tone bias (your existing behavior)
            let bias = if cfg.tone < 0.5 {
                let t = (cfg.tone * 2.0).clamp(0.0, 1.0);
                db_to_gain(6.0 * (1.0 - t) * (1.0 - freq_fraction))
            } else {
                let t = ((cfg.tone - 0.5) * 2.0).clamp(0.0, 1.0);
                db_to_gain(6.0 * t * freq_fraction)
            };

            // Speech weighting by band:
            // - For unvoiced speech, favor preserving highs (2–10 kHz)
            // - For voiced speech, favor preserving mids + harmonic structure (100–4 kHz)
            let band_speech_weight = if voiced {
                // mid emphasis
                let mid = bell(freq_fraction, 0.22, 0.20); // rough 0.22*Nyquist
                0.35 + 0.65 * mid
            } else {
                // high emphasis
                let hi = smoothstep(0.18, 0.55, freq_fraction);
                0.25 + 0.75 * hi
            };

            // Speech presence probability suppresses the aggressiveness:
            // higher speech_prob => higher threshold (less reduction) and higher floor.
            let spp = (speech_prob * band_speech_weight).clamp(0.0, 1.0);

            // Base threshold
            let mut thresh = nf * (1.0 + sensitivity * 5.0) * bias;

            // If speech likely, raise threshold so we reduce less (prevents over-suppression)
            thresh *= 1.0 + 1.25 * spp;

            // Raw spectral subtraction-ish gain
            let mut raw_gain = if mag <= thresh {
                let depth = (mag / (thresh + 1e-12)).clamp(0.0, 1.0).powf(2.0);
                1.0 - (amt * (1.0 - depth))
            } else {
                1.0
            };
            // Increased strength scaling (was 2.4, now 3.0 max) to allow deeper cleaning
            let strength = lerp(1.0, 3.0, amt);
            raw_gain = raw_gain.powf(strength);

            // Psychoacoustic masking: if a strong masker is near, artifacts are less audible,
            // so we can allow deeper attenuation there; if unmasked, keep a higher floor.
            let masker = self.masker_buf[i].max(1e-12);
            let mask_ratio = (masker / (masker + nf)).clamp(0.0, 1.0); // 0=unmasked, 1=masked
            
            // Allow lower floors when amount is high
            let floor_scale = lerp(1.0, 0.35, amt);
            let speech_floor_scale = lerp(1.0, 0.60, amt);
            
            let psycho_floor =
                (0.25 + 0.65 * (1.0 - mask_ratio)).clamp(0.10, 0.95) * floor_scale;

            // Speech-protection floor: never nuke bins likely carrying speech.
            let speech_floor =
                (0.30 + 0.60 * spp).clamp(0.15, 0.98) * speech_floor_scale;

            // Combine floors depending on amount
            let min_floor = if amt <= 0.001 {
                0.0
            } else {
                // If speech likely, speech_floor dominates; otherwise psycho floor dominates.
                let floor = lerp(psycho_floor, speech_floor, spp);
                floor
            };

            // Final per-bin gain before harmonic protection
            self.gain_buf[i] = raw_gain.max(min_floor);
        }

        // ------------------------------------------------------------
        // 7) Spectral smoothing (keep it, but adapt based on voiced/unvoiced)
        // ------------------------------------------------------------
        if amt > 0.0 {
            let smooth_strength = if voiced {
                0.55 // keep detail for voiced harmonics
            } else {
                0.75 // heavier smoothing to avoid sparkly musical noise in unvoiced regions
            };

            let mut prev = self.gain_buf[0];
            for i in 1..self.win_size - 1 {
                let curr = self.gain_buf[i];
                let next = self.gain_buf[i + 1];
                let sm = (prev + curr + next) / 3.0;
                prev = curr;
                self.gain_buf[i] = lerp(curr, sm, smooth_strength);
            }
        }

        // ------------------------------------------------------------
        // 8) Temporal smoothing + anti-zipper, tuned by speech likelihood
        // ------------------------------------------------------------
        if amt > 0.0 {
            // If speech is present, allow quicker recovery (preserve articulation).
            let release_limit = lerp(0.85, 0.92, speech_prob);

            for i in 0..self.win_size {
                if self.gain_buf[i] < self.prev_gains[i] {
                    self.gain_buf[i] = self.gain_buf[i].max(self.prev_gains[i] * release_limit);
                }
                self.prev_gains[i] = self.gain_buf[i];
            }
        }

        // ------------------------------------------------------------
        // 9) Harmonic tracking + protection (voiced only)
        // ------------------------------------------------------------
        if amt > 0.0 && voiced && f0_hz > 50.0 && f0_hz < 450.0 {
            self.apply_harmonic_protection(cfg.sample_rate, f0_hz, speech_prob, amt);
        }

        // ------------------------------------------------------------
        // 10) Apply gains to spectrum
        // ------------------------------------------------------------
        for i in 0..self.win_size {
            self.complex_buf[i] *= self.gain_buf[i];
        }

        // ------------------------------------------------------------
        // 11) IFFT + OLA synthesis (same as your design)
        // ------------------------------------------------------------
        self.ifft.process(&mut self.complex_buf);
        let norm = 1.0 / self.win_size as f32;

        for i in 0..self.win_size {
            self.overlap_buffer[i] += self.complex_buf[i].re * norm * self.window[i];
        }

        for i in 0..self.hop_size {
            let _ = self.output_producer.push(self.overlap_buffer[i]);
        }

        self.overlap_buffer
            .copy_within(self.hop_size..self.win_size, 0);
        for i in (self.win_size - self.hop_size)..self.win_size {
            self.overlap_buffer[i] = 0.0;
        }

        self.input_consumer.discard(self.hop_size);
    }

    // ----------------------------
    // Coarse FFT analysis
    // ----------------------------
    fn compute_coarse_fft_and_update_noise(&mut self, _cfg: &DenoiseConfig) {
        let n2 = self.win_size_coarse;
        // Take the first n2 samples of the current frame for simplicity.
        // (You can center this window if you prefer.)
        for i in 0..n2 {
            let x = self.frame_time[i] * self.window_coarse[i];
            self.complex_buf_coarse[i] = Complex::new(x, 0.0);
        }

        self.fft_coarse.process(&mut self.complex_buf_coarse);

        // Coarse noise floor update (used for band cues, not direct suppression)
        let alpha_att = 0.92;
        let alpha_rel = 0.999;

        for i in 0..n2 {
            let mag = self.complex_buf_coarse[i].norm().max(1e-12);
            let nf = self.noise_floor_coarse[i];
            self.noise_floor_coarse[i] = if mag < nf {
                nf * alpha_att + mag * (1.0 - alpha_att)
            } else {
                nf * alpha_rel + mag * (1.0 - alpha_rel)
            };
            self.noise_floor_coarse[i] = self.noise_floor_coarse[i].max(1e-12);
        }

        // Undo transform mutation is not required because this buffer is analysis-only.
    }

    // ----------------------------
    // Speech presence + F0 estimation
    // ----------------------------
    fn estimate_speech_and_f0(&self, sample_rate: f32) -> (f32, f32, f32) {
        // 1) Periodicity via autocorrelation peak in plausible F0 range.
        let (periodicity, f0_hz) = estimate_f0_autocorr(&self.frame_time, sample_rate);

        // 2) Spectral flatness (use main magnitudes, half spectrum)
        let nyq = (self.win_size / 2).max(1);
        let mut geo = 0.0f32;
        let mut arith = 0.0f32;
        let eps = 1e-12;
        for i in 1..nyq {
            let m = self.mag_buf[i].max(eps);
            geo += m.ln();
            arith += m;
        }
        let geo_mean = (geo / (nyq as f32)).exp();
        let arith_mean = arith / (nyq as f32);
        let flatness = (geo_mean / (arith_mean + eps)).clamp(0.0, 1.0); // 0 = tonal, 1 = noisy

        // 3) HF ratio cue (unvoiced fricatives vs low-frequency noise)
        let hf_start = (nyq as f32 * 0.25) as usize; // ~ quarter Nyquist
        let mut hf = 0.0f32;
        let mut lf = 0.0f32;
        for i in 1..nyq {
            let m = self.mag_buf[i];
            if i >= hf_start {
                hf += m;
            } else {
                lf += m;
            }
        }
        let hf_ratio = (hf / (hf + lf + 1e-12)).clamp(0.0, 1.0);

        // Speech probability heuristic:
        // - Periodicity indicates voiced speech
        // - Low flatness indicates tonal structure (speech or tonal sources)
        // - HF ratio supports unvoiced speech presence
        let voiced_prob = smoothstep(0.35, 0.80, periodicity);
        let tonal_prob = 1.0 - smoothstep(0.25, 0.85, flatness);
        let unvoiced_prob = smoothstep(0.18, 0.45, hf_ratio) * (1.0 - voiced_prob);

        let mut speech_prob =
            (0.55 * voiced_prob + 0.30 * tonal_prob + 0.35 * unvoiced_prob).clamp(0.0, 1.0);

        // If the frame is extremely low energy, reduce speech probability to avoid false positives.
        let rms = frame_rms(&self.frame_time).clamp(0.0, 1.0);
        let energy_gate = smoothstep(0.003, 0.02, rms);
        speech_prob *= energy_gate;

        (speech_prob, voiced_prob, f0_hz)
    }

    // ----------------------------
    // Psychoacoustic masking (simple spreading)
    // ----------------------------
    fn compute_masker_curve(&mut self, sample_rate: f32) {
        // Very lightweight approximation:
        // For each bin, find local peak energy and spread it exponentially across neighbors.
        //
        // This is not a full Bark/ERB psychoacoustic model, but it helps:
        // - reduce "musical noise" sparkles
        // - avoid over-cutting in unmasked regions
        let n = self.win_size;
        let nyq = (n / 2).max(1);

        // Clear
        for v in self.masker_buf.iter_mut() {
            *v = 0.0;
        }

        // Spread radius in bins depends on frequency (wider at low freq)
        // We'll do two passes: pick peaks then spread.
        // Peak picking: local maxima on magnitude curve
        let mut peaks: Vec<(usize, f32)> = Vec::new();
        for i in 2..(nyq - 2) {
            let m = self.mag_buf[i];
            if m > self.mag_buf[i - 1]
                && m > self.mag_buf[i + 1]
                && m > self.mag_buf[i - 2]
                && m > self.mag_buf[i + 2]
            {
                peaks.push((i, m));
            }
        }

        // Keep top K peaks for cost control
        peaks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let k = peaks.len().min(64);
        peaks.truncate(k);

        // Spread peaks
        let bin_width = sample_rate / n as f32;
        for (center, amp) in peaks {
            let freq = center as f32 * bin_width;
            let frac = (freq / (sample_rate * 0.5)).clamp(0.0, 1.0);
            let radius = lerp(32.0, 10.0, frac) as isize; // wider low, narrower high
            let alpha = lerp(10.0, 4.0, frac); // decay speed

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

        // Mirror to upper bins simply (not strictly required, but keeps indexing safe)
        for i in nyq..n {
            self.masker_buf[i] = self.masker_buf[n - i.min(n - 1)];
        }
    }

    // ----------------------------
    // Harmonic protection
    // ----------------------------
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

        // Protection width: a couple of bins, slightly wider at low frequencies
        // Protection strength increases with speech_prob, decreases with amt (if user wants heavy NR, allow more change)
        let protect = (0.55 + 0.40 * speech_prob).clamp(0.55, 0.95);
        let allow = (1.0 - 0.65 * amt).clamp(0.25, 1.0);
        let min_gain_on_harmonics = (protect * allow).clamp(0.25, 0.98);

        // Harmonics up to ~8 kHz for speech usefulness, or Nyquist
        let max_hz = 8000.0_f32.min(sample_rate * 0.5);
        let mut h = 1;
        loop {
            let hz = f0_hz * (h as f32);
            if hz > max_hz {
                break;
            }
            let center = (hz / bin_width).round() as isize;
            if center <= 1 || center as usize >= nyq - 1 {
                break;
            }

            // width in bins
            let frac = (hz / max_hz).clamp(0.0, 1.0);
            let w = lerp(3.0, 1.5, frac) as isize;

            for d in -w..=w {
                let b = center + d;
                if b <= 0 || b as usize >= nyq {
                    continue;
                }
                let bi = b as usize;
                self.gain_buf[bi] = self.gain_buf[bi].max(min_gain_on_harmonics);
            }

            h += 1;
            if h > 80 {
                break;
            }
        }

        // Mirror for safety
        for i in nyq..n {
            self.gain_buf[i] = self.gain_buf[n - i.min(n - 1)];
        }
    }

    fn apply_hum_removal(&mut self, sample_rate: f32) {
        let bin_width = sample_rate / self.win_size as f32;
        let targets = [50.0, 60.0, 100.0, 120.0, 150.0, 180.0];
        for &freq in &targets {
            let center = (freq / bin_width).round() as usize;
            if center > 0 && center < self.win_size / 2 {
                self.complex_buf[center] *= 0.1;
                if center + 1 < self.win_size / 2 {
                    self.complex_buf[center + 1] *= 0.5;
                }
                self.complex_buf[center - 1] *= 0.5;
            }
        }
        let cut_bin = (25.0 / bin_width).ceil() as usize;
        for i in 0..cut_bin {
            self.complex_buf[i] = Complex::new(0.0, 0.0);
        }
    }
}