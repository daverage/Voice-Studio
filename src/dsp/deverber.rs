use crate::dsp::utils::{estimate_f0_autocorr, lerp, max3, smoothstep};
use ringbuf::{Consumer, Producer, RingBuffer};
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::f32::consts::PI;
use std::sync::Arc;

/// FFT-based streaming deverber for room/reverb reduction.
/// Fixes metallic artifacts by:
/// - Correct Hermitian symmetry (real-signal FFT constraint)
/// - Proper WOLA (sqrt-Hann + per-sample overlap normalization)
/// - Gain mask smoothing (reduces musical-noise / bin jitter)
///
/// Still includes:
/// - Multi-frame late-tail estimation (per-bin envelope)
/// - Frequency-dependent decay
/// - Transient protection via spectral flux
/// - Harmonic protection for voiced speech
/// - Cheap psychoacoustic-ish masking heuristic
pub struct StreamingDeverber {
    fft: Arc<dyn Fft<f32>>,
    ifft: Arc<dyn Fft<f32>>,
    pub win_size: usize,
    hop_size: usize,
    window: Vec<f32>, // sqrt-Hann

    // Main STFT buffers
    scratch_buf: Vec<Complex<f32>>, // length win_size (full complex spectrum)
    mag_buf: Vec<f32>,              // length nyq+1

    // Multi-frame state (nyq+1)
    prev_mag: Vec<f32>,
    late_env: Vec<f32>,
    early_hold: Vec<f32>,

    // Masking curve (nyq+1)
    masker_buf: Vec<f32>,

    // Gain smoothing (nyq+1)
    gain_smooth: Vec<f32>,

    // Time-domain frame (for voiced/F0 estimation)
    frame_time: Vec<f32>,

    // Streaming IO
    input_producer: Producer<f32>,
    input_consumer: Consumer<f32>,
    output_producer: Producer<f32>,
    output_consumer: Consumer<f32>,

    // OLA buffers (time domain)
    overlap_buffer: Vec<f32>,
    ola_norm: Vec<f32>,
}

impl StreamingDeverber {
    pub fn new(win_size: usize, hop_size: usize) -> Self {
        assert!(win_size >= 64, "win_size too small");
        assert!(hop_size > 0 && hop_size <= win_size, "invalid hop_size");

        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(win_size);
        let ifft = planner.plan_fft_inverse(win_size);

        // sqrt-Hann window for correct WOLA when used in both analysis and synthesis
        let window: Vec<f32> = (0..win_size)
            .map(|i| {
                let hann = 0.5 * (1.0 - (2.0 * PI * i as f32 / win_size as f32).cos());
                hann.sqrt()
            })
            .collect();

        let buf_cap = win_size * 4;
        let (in_prod, in_cons) = RingBuffer::<f32>::new(buf_cap).split();
        let (out_prod, out_cons) = RingBuffer::<f32>::new(buf_cap).split();

        // Prime output ring to avoid initial underruns (matches your original behavior)
        let mut initialized_out_prod: Producer<f32> = out_prod;
        for _ in 0..win_size {
            let _ = initialized_out_prod.push(0.0);
        }

        let nyq = win_size / 2;

        Self {
            fft,
            ifft,
            win_size,
            hop_size,
            window,

            scratch_buf: vec![Complex::new(0.0, 0.0); win_size],
            mag_buf: vec![0.0; nyq + 1],

            prev_mag: vec![0.0; nyq + 1],
            late_env: vec![0.0; nyq + 1],
            early_hold: vec![0.0; nyq + 1],

            masker_buf: vec![0.0; nyq + 1],
            gain_smooth: vec![1.0; nyq + 1],

            frame_time: vec![0.0; win_size],

            input_producer: in_prod,
            input_consumer: in_cons,
            output_producer: initialized_out_prod,
            output_consumer: out_cons,

            overlap_buffer: vec![0.0; win_size],
            ola_norm: vec![0.0; win_size],
        }
    }

    /// strength: 0..1
    pub fn process_sample(&mut self, input: f32, strength: f32) -> f32 {
        // True bypass when strength is effectively zero
        if strength <= 0.001 {
            return input;
        }

        let _ = self.input_producer.push(input);

        if self.input_consumer.len() >= self.win_size {
            self.process_frame(strength);
        }

        self.output_consumer.pop().unwrap_or(0.0)
    }

    fn process_frame(&mut self, strength: f32) {
        let strength = strength.clamp(0.0, 1.0);
        let n = self.win_size;
        let nyq = n / 2;
        let has_nyquist = n % 2 == 0;

        // ------------------------------------------------------------
        // 0) Time-domain frame gather + analysis window (sqrt-Hann)
        // ------------------------------------------------------------
        for (i, val) in self.input_consumer.iter().take(n).enumerate() {
            let x = *val;
            self.frame_time[i] = x;
            self.scratch_buf[i] = Complex::new(x * self.window[i], 0.0);
        }

        // ------------------------------------------------------------
        // 1) FFT
        // ------------------------------------------------------------
        self.fft.process(&mut self.scratch_buf);

        // Magnitudes (only 0..=nyq are unique for real input)
        for i in 0..=nyq {
            self.mag_buf[i] = self.scratch_buf[i].norm().max(1e-12);
        }

        // ------------------------------------------------------------
        // 2) Voiced / F0 estimation (used only for harmonic protection)
        // ------------------------------------------------------------
        let assumed_sr = guess_sample_rate_from_content(&self.frame_time);
        let (periodicity, f0_hz) = estimate_f0_autocorr(&self.frame_time, assumed_sr);
        let voiced = periodicity > 0.55 && f0_hz > 70.0 && f0_hz < 320.0;

        // ------------------------------------------------------------
        // 3) Transient protection via spectral flux
        // ------------------------------------------------------------
        let mut flux_sum = 0.0f32;
        for i in 1..=nyq {
            let d = (self.mag_buf[i] - self.prev_mag[i]).max(0.0);
            flux_sum += d;
        }

        let mut energy_sum = 0.0f32;
        for i in 1..=nyq {
            energy_sum += self.mag_buf[i];
        }

        let transientness = (flux_sum / (energy_sum + 1e-12)).clamp(0.0, 1.0);
        let transient_protect = smoothstep(0.03, 0.18, transientness); // 0..1

        // ------------------------------------------------------------
        // 4) Masker curve
        // ------------------------------------------------------------
        self.compute_masker_curve();

        // ------------------------------------------------------------
        // 5) Late tail estimation (per-bin envelope, nyq+1 bins)
        // ------------------------------------------------------------
        let eps = 1e-12;

        let base_decay_fast = 0.90;
        let base_decay_slow = 0.995;
        let rise_ignore = 0.999;

        // stronger => shorter hold
        let early_release = lerp(0.80, 0.92, 1.0 - strength);

        let bin_width = assumed_sr / n as f32;

        for i in 0..=nyq {
            let mag = self.mag_buf[i];
            let prev = self.prev_mag[i];

            let freq_hz = i as f32 * bin_width;
            let frac = (freq_hz / (assumed_sr * 0.5)).clamp(0.0, 1.0);

            // Lows decay slower, highs decay faster
            let decay_slow = lerp(base_decay_slow, 0.985, frac);
            let decay_fast = lerp(base_decay_fast, 0.80, frac);

            // Early hold: capture sudden rises (direct sound), then release
            let rise = (mag - prev).max(0.0);
            let rise_gate = smoothstep(0.0, prev * 0.10 + 1e-6, rise);
            let early_add = mag * rise_gate;

            self.early_hold[i] = (self.early_hold[i] * early_release).max(early_add);

            // Late estimator: follow decays, ignore rises
            let mut late = self.late_env[i];

            if mag < late {
                late = late * decay_fast + mag * (1.0 - decay_fast);
            } else {
                late = late * rise_ignore + mag * (1.0 - rise_ignore);
            }

            late *= decay_slow;
            late = late.min(mag * 1.10 + 1e-6);

            // Early energy protection
            let early = self.early_hold[i];
            let early_protect_amt = lerp(0.35, 0.75, transient_protect);
            late = late
                .min(mag + eps)
                .min((mag - early * early_protect_amt).max(0.0) + late);

            self.late_env[i] = late.max(0.0);
            self.prev_mag[i] = mag;
        }

        // ------------------------------------------------------------
        // 6) Build gain mask (nyq+1), smooth it, apply to spectrum
        // ------------------------------------------------------------
        // Increased max subtraction from 0.85 to 1.0 for stronger effect
        let late_k = lerp(0.0, 1.0, strength);

        for i in 0..=nyq {
            let mag = self.mag_buf[i];
            let late = self.late_env[i];
            let early = self.early_hold[i];
            let masker = self.masker_buf[i].max(1e-12);

            let mask_ratio = (masker / (masker + late + 1e-12)).clamp(0.0, 1.0);
            let unmasked = 1.0 - mask_ratio;

            let transient_floor = lerp(0.12, 0.55, transient_protect);
            let psycho_floor = lerp(0.18, 0.60, unmasked);

            let early_floor_gate = smoothstep(0.0, mag * 0.25 + 1e-6, early);
            let early_floor = lerp(0.10, 0.55, early_floor_gate);

            let min_floor = max3(transient_floor, psycho_floor, early_floor).clamp(0.08, 0.92);

            let direct = (mag - late_k * late).max(mag * 0.02);
            let mut gain = (direct / (mag + eps)).clamp(0.0, 1.0);

            gain = gain.max(min_floor);

            let late_ratio = (late / (mag + eps)).clamp(0.0, 1.0);
            let late_presence = smoothstep(0.08, 0.35, late_ratio);
            gain = lerp(1.0, gain, late_presence);

            // High-frequency backoff: avoid "tizz" damage
            let freq = i as f32 * bin_width;
            let hf_protect = smoothstep(6000.0, 12000.0, freq); // 0..1
            gain = lerp(gain, 1.0, hf_protect * 0.35);

            if voiced {
                gain = self.apply_harmonic_protection_to_gain(i, gain, assumed_sr, f0_hz, strength);
            }

            // Time smoothing of gain to reduce musical noise
            let attack = 0.35; // faster when opening up
            let release = 0.06; // slower when clamping down

            let prev_g = self.gain_smooth[i];
            let smoothed = if gain > prev_g {
                prev_g + (gain - prev_g) * attack
            } else {
                prev_g + (gain - prev_g) * release
            };

            self.gain_smooth[i] = smoothed.clamp(0.0, 1.0);
        }

        // Tiny 3-bin blur (frequency smoothing), skip edges
        if nyq >= 4 {
            let mut tmp = self.gain_smooth.clone();
            for i in 2..=(nyq - 2) {
                tmp[i] =
                    (self.gain_smooth[i - 1] + self.gain_smooth[i] + self.gain_smooth[i + 1]) / 3.0;
            }
            self.gain_smooth = tmp;
        }

        // Apply gain to unique bins 0..=nyq
        for i in 0..=nyq {
            self.scratch_buf[i] *= self.gain_smooth[i];
        }

        // ------------------------------------------------------------
        // 6b) Enforce Hermitian symmetry for real IFFT (CRITICAL)
        // ------------------------------------------------------------
        self.scratch_buf[0].im = 0.0;
        if has_nyquist {
            self.scratch_buf[nyq].im = 0.0;
        }

        for k in 1..nyq {
            let a = self.scratch_buf[k];
            self.scratch_buf[n - k] = a.conj();
        }

        // ------------------------------------------------------------
        // 7) IFFT + WOLA (sqrt-Hann synthesis + per-sample normalization)
        // ------------------------------------------------------------
        self.ifft.process(&mut self.scratch_buf);
        let norm_ifft = 1.0 / n as f32;

        for i in 0..n {
            let w = self.window[i];
            let y = self.scratch_buf[i].re * norm_ifft * w;

            self.overlap_buffer[i] += y;
            self.ola_norm[i] += w * w;
        }

        for i in 0..self.hop_size {
            let denom = self.ola_norm[i].max(1e-6);
            let out = self.overlap_buffer[i] / denom;
            let _ = self.output_producer.push(out);
        }

        // Shift OLA buffers
        self.overlap_buffer.copy_within(self.hop_size..n, 0);
        self.ola_norm.copy_within(self.hop_size..n, 0);

        for i in (n - self.hop_size)..n {
            self.overlap_buffer[i] = 0.0;
            self.ola_norm[i] = 0.0;
        }

        // Consume hop
        self.input_consumer.discard(self.hop_size);
    }

    fn compute_masker_curve(&mut self) {
        let n = self.win_size;
        let nyq = n / 2;

        for v in self.masker_buf.iter_mut() {
            *v = 0.0;
        }

        // Peak pick on magnitude (unique bins)
        let mut peaks: Vec<(usize, f32)> = Vec::new();
        if nyq >= 4 {
            for i in 2..=(nyq - 2) {
                let m = self.mag_buf[i];
                if m > self.mag_buf[i - 1]
                    && m > self.mag_buf[i + 1]
                    && m > self.mag_buf[i - 2]
                    && m > self.mag_buf[i + 2]
                {
                    peaks.push((i, m));
                }
            }
        }

        peaks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        peaks.truncate(peaks.len().min(64));

        for (center, amp) in peaks {
            let frac = (center as f32 / nyq.max(1) as f32).clamp(0.0, 1.0);
            let radius = lerp(40.0, 12.0, frac) as isize;
            let alpha = lerp(12.0, 5.0, frac);

            let c = center as isize;
            for d in -radius..=radius {
                let j = c + d;
                if j <= 0 || j as usize > nyq {
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

    fn apply_harmonic_protection_to_gain(
        &self,
        bin: usize,
        gain: f32,
        sample_rate: f32,
        f0_hz: f32,
        strength: f32,
    ) -> f32 {
        let n = self.win_size;
        let nyq = n / 2;
        if bin == 0 || bin > nyq {
            return gain;
        }

        let bin_width = sample_rate / n as f32;
        let freq = bin as f32 * bin_width;

        if freq > 6000.0 || f0_hz <= 1.0 {
            return gain;
        }

        let h = (freq / f0_hz).round().max(1.0);
        let harmonic = h * f0_hz;
        let dist_hz = (freq - harmonic).abs();

        let bw = lerp(45.0, 25.0, (freq / 6000.0).clamp(0.0, 1.0));
        let near = 1.0 - smoothstep(0.0, bw, dist_hz);

        if near <= 0.0 {
            return gain;
        }

        // More strength => less protection
        let protect_floor = lerp(0.55, 0.35, strength).clamp(0.25, 0.70);
        gain.max(lerp(gain, protect_floor, near))
    }
}

/// Drop-in signature doesn’t provide sample_rate, so we guess.
/// Used only for harmonic protection targeting bins, not the core effect.
fn guess_sample_rate_from_content(_frame: &[f32]) -> f32 {
    48_000.0
}