// noise_learn_remove.rs
//! Noise Learn & Remove (Static Spectral Subtraction)
//!
//! A dedicated, optional restoration module that removes a learned
//! stationary noise fingerprint (hiss, room tone, rumble) even during silence.
//!
//! Design goals
//! - Learn is explicit (button), time-limited (10s), and only allowed during low speech confidence.
//! - Removal works during silence, independent of speech-aware denoisers.
//! - Deterministic, real-time safe (no alloc in process), never amplifies, never “chases” speech.
//! - Bounded subtraction: only attenuates, with smoothing to avoid zipper/warble.
//!
//! Usage (per-sample)
//!   let cfg = NoiseLearnRemoveConfig { enabled, amount, learn, clear };
//!   let (l2, r2) = noise_learn_remove.process(l1, r1, cfg, &sidechain);
//!
//! Notes
//! - Place right after Speech HPF (so subsonic junk doesn’t pollute the learned profile).
//! - Do NOT feed its output into speech confidence estimation if you want confidence to remain “truthy”.

use crate::dsp::speech_confidence::SpeechSidechain;
use crate::dsp::utils::{make_sqrt_hann_window, MAG_FLOOR};
use ringbuf::{Consumer, Producer, RingBuffer};
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::sync::Arc;

// -----------------------------------------------------------------------------
// Constants
// -----------------------------------------------------------------------------

const RINGBUF_CAP_MULT: usize = 4;

// Only allow learning when speech confidence is low (room tone / silence)
const LEARN_CONFIDENCE_THRESHOLD: f32 = 0.15;

// Hard stop learning after this many seconds
const LEARN_TIME_LIMIT_SEC: f32 = 10.0;

// EMA time constants (seconds)
const LEARN_EMA_TAU: f32 = 0.5;
const QUALITY_EMA_TAU: f32 = 2.0;

// Gain smoothing per frame
const GAIN_SMOOTH_ALPHA: f32 = 0.2;

// Small epsilon for safe divides
const EPS: f32 = 1e-12;

// -----------------------------------------------------------------------------
// Public Config & API
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct NoiseLearnRemoveConfig {
    pub enabled: bool,
    pub amount: f32, // 0.0 .. 1.0
    pub learn: bool, // momentary button
    pub clear: bool, // reset learned profile
}

pub struct NoiseLearnRemove {
    detector: NoiseLearnRemoveDetector,
    chan_l: StreamingNoiseLearnRemoveChannel,
    chan_r: StreamingNoiseLearnRemoveChannel,

    // Scratch buffers for stereo-to-mono analysis
    frame_l: Vec<f32>,
    frame_r: Vec<f32>,
    frame_mono: Vec<f32>,

    win_size: usize,
    hop_size: usize,
    sample_rate: f32,
}

impl NoiseLearnRemove {
    pub fn new(win: usize, hop: usize, sr: f32) -> Self {
        assert!(win > 0 && hop > 0);
        assert!(hop <= win);

        Self {
            detector: NoiseLearnRemoveDetector::new(win, hop, sr),
            chan_l: StreamingNoiseLearnRemoveChannel::new(win, hop),
            chan_r: StreamingNoiseLearnRemoveChannel::new(win, hop),
            frame_l: vec![0.0; win],
            frame_r: vec![0.0; win],
            frame_mono: vec![0.0; win],
            win_size: win,
            hop_size: hop,
            sample_rate: sr,
        }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr;
        self.detector.set_sample_rate(sr);
        self.reset();
    }

    /// Reset internal DSP buffers (e.g. on playback stop), but KEEP learned profile.
    pub fn reset(&mut self) {
        self.chan_l.reset();
        self.chan_r.reset();
        self.detector.reset_state(); // Only clear history, not profile
    }

    /// Explicitly clear the learned noise profile (user action).
    pub fn clear_profile(&mut self) {
        self.detector.clear_profile();
    }

    /// 0..1 estimate of how “stable” the learned fingerprint is.
    pub fn get_quality(&self) -> f32 {
        self.detector.quality
    }

    /// 0..1 progress through the 10s learn budget (useful for UI)
    pub fn get_learn_progress(&self) -> f32 {
        self.detector.learn_progress()
    }

    /// True if we have a non-trivial learned profile.
    pub fn has_profile(&self) -> bool {
        self.detector.has_profile()
    }

    #[inline]
    pub fn process(
        &mut self,
        l: f32,
        r: f32,
        cfg: NoiseLearnRemoveConfig,
        sidechain: &SpeechSidechain,
    ) -> (f32, f32) {
        // Clear request is instantaneous
        if cfg.clear {
            self.detector.clear_profile();
        }

        // Push input samples
        self.chan_l.push_input(l);
        self.chan_r.push_input(r);

        // Process frame when both channels have enough
        if self.chan_l.input_len() >= self.win_size && self.chan_r.input_len() >= self.win_size {
            self.chan_l.peek_input(&mut self.frame_l);
            self.chan_r.peek_input(&mut self.frame_r);

            // Mono analysis: take the larger absolute channel sample (safer for asymmetric noise)
            for i in 0..self.win_size {
                let al = self.frame_l[i].abs();
                let ar = self.frame_r[i].abs();
                self.frame_mono[i] = if al >= ar {
                    self.frame_l[i]
                } else {
                    self.frame_r[i]
                };
            }

            let gains = self
                .detector
                .analyze_frame(&self.frame_mono, cfg, sidechain);

            self.chan_l.process_frame(gains);
            self.chan_r.process_frame(gains);

            // Advance analysis window
            self.chan_l.discard_input(self.hop_size);
            self.chan_r.discard_input(self.hop_size);
        }

        (self.chan_l.pop_output(), self.chan_r.pop_output())
    }
}

// -----------------------------------------------------------------------------
// Detector
// -----------------------------------------------------------------------------

struct NoiseLearnRemoveDetector {
    fft: Arc<dyn Fft<f32>>,

    // Scratch
    scratch: Vec<Complex<f32>>,
    fft_scratch: Vec<Complex<f32>>,
    window: Vec<f32>,
    current_mag: Vec<f32>,

    // Learned profile (magnitude, nyq+1)
    learned_mag: Vec<f32>,
    learned_energy: f32, // quick “is learned?” check

    // UI metric
    quality: f32,

    // Learn budget
    learn_frames_accum: usize,
    max_learn_frames: usize,

    // Per-bin smoothed gains (nyq+1)
    gain_smooth: Vec<f32>,

    // EMA coefficients
    learn_alpha: f32,
    quality_alpha: f32,

    win_size: usize,
    #[allow(dead_code)]
    // Keep for struct completeness, even if currently unused logic relies on it implicitly
    hop_size: usize,
    sample_rate: f32,
}

impl NoiseLearnRemoveDetector {
    fn new(win: usize, hop: usize, sr: f32) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(win);
        let fft_scratch_len = fft.get_inplace_scratch_len();

        let nyq = win / 2;
        let frame_dt = hop as f32 / sr.max(1.0);

        let learn_alpha = 1.0 - (-frame_dt / LEARN_EMA_TAU).exp();
        let quality_alpha = 1.0 - (-frame_dt / QUALITY_EMA_TAU).exp();

        let max_learn_frames = (LEARN_TIME_LIMIT_SEC / frame_dt).ceil().max(1.0) as usize;

        Self {
            fft,
            scratch: vec![Complex::default(); win],
            fft_scratch: vec![Complex::default(); fft_scratch_len],
            window: make_sqrt_hann_window(win),
            current_mag: vec![0.0; nyq + 1],

            learned_mag: vec![0.0; nyq + 1],
            learned_energy: 0.0,

            quality: 0.0,

            learn_frames_accum: 0,
            max_learn_frames,

            gain_smooth: vec![1.0; nyq + 1],

            learn_alpha,
            quality_alpha,

            win_size: win,
            hop_size: hop,
            sample_rate: sr,
        }
    }

    fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr;
        // Coefficients depend on sr; easiest is full reset.
        self.reset_state(); // Was reset(), changed to preserve profile if possible? No, SR change invalidates profile
        self.clear_profile(); // SR change MUST clear profile because bins align differently to Hz
    }

    /// Clears only DSP state (smoothing, history), preserves learned profile.
    fn reset_state(&mut self) {
        self.gain_smooth.fill(1.0);
        // We do NOT clear learned_mag, learned_energy, quality, learn_frames_accum
    }

    /// Clears the learned profile (destructive).
    fn clear_profile(&mut self) {
        self.learned_mag.fill(0.0);
        self.learned_energy = 0.0;
        self.quality = 0.0;
        self.learn_frames_accum = 0;
        self.gain_smooth.fill(1.0);
    }

    fn has_profile(&self) -> bool {
        self.learned_energy > 1e-6
    }

    fn learn_progress(&self) -> f32 {
        (self.learn_frames_accum as f32 / self.max_learn_frames as f32).clamp(0.0, 1.0)
    }

    fn analyze_frame(
        &mut self,
        input: &[f32],
        cfg: NoiseLearnRemoveConfig,
        sidechain: &SpeechSidechain,
    ) -> &[f32] {
        let nyq = self.win_size / 2;

        // 1) Window + FFT
        for i in 0..self.win_size {
            self.scratch[i] = Complex::new(input[i] * self.window[i], 0.0);
        }
        self.fft
            .process_with_scratch(&mut self.scratch, &mut self.fft_scratch);

        // 2) Magnitude
        for i in 0..=nyq {
            let m = self.scratch[i].norm();
            self.current_mag[i] = m.max(MAG_FLOOR);
        }

        // 3) Learning
        let is_silence = sidechain.speech_conf < LEARN_CONFIDENCE_THRESHOLD;
        let not_finished = self.learn_frames_accum < self.max_learn_frames;

        if cfg.learn && is_silence && not_finished {
            self.learn_frames_accum += 1;

            if self.learn_frames_accum == 1 {
                // initialise
                for i in 0..=nyq {
                    self.learned_mag[i] = self.current_mag[i];
                }
            } else {
                // EMA update
                for i in 0..=nyq {
                    let v = self.learned_mag[i];
                    self.learned_mag[i] = v + self.learn_alpha * (self.current_mag[i] - v);
                }
            }

            // Update learned energy for “profile exists” checks
            let mut e = 0.0;
            for i in 0..=nyq {
                e += self.learned_mag[i];
            }
            self.learned_energy = e;

            // Quality: measure stability of current vs learned (lower delta => higher quality)
            let mut delta_sum = 0.0;
            let mut learned_sum = 0.0;
            for i in 0..=nyq {
                delta_sum += (self.current_mag[i] - self.learned_mag[i]).abs();
                learned_sum += self.learned_mag[i];
            }

            let normalized_delta = if learned_sum > EPS {
                delta_sum / learned_sum
            } else {
                1.0
            };

            // Map to 0..1
            let stability = 1.0 / (1.0 + normalized_delta * 2.0);
            self.quality += self.quality_alpha * (stability - self.quality);
        } else {
            // If not learning, slowly decay quality towards “whatever we already have”.
            // This stops quality getting stuck at 1.0 forever after a lucky learn frame.
            let target = if self.has_profile() {
                self.quality
            } else {
                0.0
            };
            self.quality += self.quality_alpha * (target - self.quality);
        }

        // 4) Subtraction (bounded attenuation only)
        let amount = cfg.amount.clamp(0.0, 1.0);

        // If disabled or no profile, return unity gains (and reset smoother to unity to avoid stale attenuation)
        if !cfg.enabled || amount < 1e-4 || !self.has_profile() {
            for g in &mut self.gain_smooth {
                *g = 1.0;
            }
            return &self.gain_smooth;
        }

        for i in 0..=nyq {
            let noise = self.learned_mag[i].max(MAG_FLOOR);
            let signal = self.current_mag[i].max(MAG_FLOOR);

            // reduction = amount * (noise / signal)
            // gain = clamp(1 - reduction, 0..1)
            let reduction = amount * (noise / (signal + EPS));
            let target_gain = (1.0 - reduction).clamp(0.0, 1.0);

            // Smooth per-bin gain to prevent musical noise/zipper
            let prev = self.gain_smooth[i];
            self.gain_smooth[i] = prev + GAIN_SMOOTH_ALPHA * (target_gain - prev);
        }

        &self.gain_smooth
    }
}

// -----------------------------------------------------------------------------
// Streaming Channel (STFT + overlap-add)
// -----------------------------------------------------------------------------

struct StreamingNoiseLearnRemoveChannel {
    input_prod: Producer<f32>,
    input_cons: Consumer<f32>,
    output_prod: Producer<f32>,
    output_cons: Consumer<f32>,

    fft: Arc<dyn Fft<f32>>,
    ifft: Arc<dyn Fft<f32>>,

    // Scratch
    scratch: Vec<Complex<f32>>,
    fft_scratch: Vec<Complex<f32>>,
    ifft_scratch: Vec<Complex<f32>>,
    window: Vec<f32>,
    overlap: Vec<f32>,

    win_size: usize,
    hop_size: usize,
}

impl StreamingNoiseLearnRemoveChannel {
    fn new(win: usize, hop: usize) -> Self {
        let buf_size = (win * RINGBUF_CAP_MULT).max(win + hop + 16);

        let (ip, ic) = RingBuffer::new(buf_size).split();
        let (op, oc) = RingBuffer::new(buf_size).split();

        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(win);
        let ifft = planner.plan_fft_inverse(win);

        let fft_scratch_len = fft.get_inplace_scratch_len();
        let ifft_scratch_len = ifft.get_inplace_scratch_len();

        let mut ch = Self {
            input_prod: ip,
            input_cons: ic,
            output_prod: op,
            output_cons: oc,

            fft,
            ifft,

            scratch: vec![Complex::default(); win],
            fft_scratch: vec![Complex::default(); fft_scratch_len],
            ifft_scratch: vec![Complex::default(); ifft_scratch_len],
            window: make_sqrt_hann_window(win),
            overlap: vec![0.0; win],

            win_size: win,
            hop_size: hop,
        };

        // Prime output with zeros so initial pops are deterministic
        for _ in 0..win {
            let _ = ch.output_prod.push(0.0);
        }

        ch
    }

    fn reset(&mut self) {
        while self.input_cons.pop().is_some() {}
        while self.output_cons.pop().is_some() {}

        self.overlap.fill(0.0);

        for _ in 0..self.win_size {
            let _ = self.output_prod.push(0.0);
        }
    }

    #[inline]
    fn push_input(&mut self, s: f32) {
        let _ = self.input_prod.push(s);
    }

    #[inline]
    fn input_len(&self) -> usize {
        self.input_cons.len()
    }

    fn peek_input(&mut self, dest: &mut [f32]) {
        // Consumer::iter() does not consume.
        for (i, &s) in self.input_cons.iter().take(self.win_size).enumerate() {
            dest[i] = s;
        }
        // If dest is longer than win_size, keep remainder untouched by design.
    }

    fn discard_input(&mut self, n: usize) {
        // Avoid relying on a discard() method which may not exist for the ringbuf version in use.
        for _ in 0..n {
            let _ = self.input_cons.pop();
        }
    }

    #[inline]
    fn pop_output(&mut self) -> f32 {
        self.output_cons.pop().unwrap_or(0.0)
    }

    fn process_frame(&mut self, gains: &[f32]) {
        let nyq = self.win_size / 2;

        // 1) Read windowed frame
        for (i, &s) in self.input_cons.iter().take(self.win_size).enumerate() {
            self.scratch[i] = Complex::new(s * self.window[i], 0.0);
        }

        // 2) FFT
        self.fft
            .process_with_scratch(&mut self.scratch, &mut self.fft_scratch);

        // 3) Apply gains on positive freqs
        for i in 0..=nyq {
            let g = gains[i].clamp(0.0, 1.0);
            self.scratch[i] *= g;
        }

        // 4) Restore conjugate symmetry for real iFFT
        for i in 1..nyq {
            self.scratch[self.win_size - i] = self.scratch[i].conj();
        }

        // 5) iFFT
        self.ifft
            .process_with_scratch(&mut self.scratch, &mut self.ifft_scratch);

        // 6) Overlap-add (sqrt-hann analysis + synthesis, with 1/N norm)
        let norm = 1.0 / self.win_size as f32;
        for i in 0..self.win_size {
            let val = self.scratch[i].re * norm * self.window[i];
            self.overlap[i] += val;
        }

        // 7) Push hop samples
        for i in 0..self.hop_size {
            let _ = self.output_prod.push(self.overlap[i]);
        }

        // 8) Shift overlap buffer left by hop
        self.overlap.copy_within(self.hop_size..self.win_size, 0);
        for i in (self.win_size - self.hop_size)..self.win_size {
            self.overlap[i] = 0.0;
        }
    }
}
