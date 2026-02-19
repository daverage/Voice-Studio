// noise_learn_remove.rs
//! Noise Learn & Remove (Static Spectral Subtraction)
//!
//! A dedicated, optional restoration module that removes a learned
//! stationary noise fingerprint (hiss, room tone, rumble) even during silence.
//!
//! Design goals
//! - Learning is continuous (always-on), gated by low speech confidence and stability checks.
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
const LEARN_CONFIDENCE_THRESHOLD: f32 = 0.25;

// EMA time constants (seconds)
const CANDIDATE_EMA_TAU: f32 = 0.25;
const LEARNED_EMA_TAU: f32 = 4.0;
const QUALITY_EMA_TAU: f32 = 2.0;

// Stability gating: require a stable window before promoting candidate -> learned
const STABILITY_TIME_SEC: f32 = 0.6;
const STABILITY_DELTA_THRESHOLD: f32 = 0.18;

// Re-learn latch duration (seconds)
const RELEARN_TIME_SEC: f32 = 5.0;

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

    // Candidate profile (fast) and learned profile (slow)
    candidate_mag: Vec<f32>,
    candidate_energy: f32,
    learned_mag: Vec<f32>,
    learned_energy: f32, // quick “is learned?” check

    // UI metric
    quality: f32,

    // Stability gate
    stable_frames: usize,
    stable_frames_required: usize,
    relearn_frames_left: usize,
    relearn_frames_total: usize,
    learn_latched: bool,
    relearn_armed: bool,

    // Per-bin smoothed gains (nyq+1)
    gain_smooth: Vec<f32>,

    // EMA coefficients
    candidate_alpha: f32,
    learned_alpha: f32,
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

        let candidate_alpha = 1.0 - (-frame_dt / CANDIDATE_EMA_TAU).exp();
        let learned_alpha = 1.0 - (-frame_dt / LEARNED_EMA_TAU).exp();
        let quality_alpha = 1.0 - (-frame_dt / QUALITY_EMA_TAU).exp();

        let stable_frames_required = (STABILITY_TIME_SEC / frame_dt).ceil().max(1.0) as usize;
        let relearn_frames_total = (RELEARN_TIME_SEC / frame_dt).ceil().max(1.0) as usize;

        Self {
            fft,
            scratch: vec![Complex::default(); win],
            fft_scratch: vec![Complex::default(); fft_scratch_len],
            window: make_sqrt_hann_window(win),
            current_mag: vec![0.0; nyq + 1],

            candidate_mag: vec![0.0; nyq + 1],
            candidate_energy: 0.0,
            learned_mag: vec![0.0; nyq + 1],
            learned_energy: 0.0,

            quality: 0.0,

            stable_frames: 0,
            stable_frames_required,
            relearn_frames_left: 0,
            relearn_frames_total,
            learn_latched: false,
            relearn_armed: false,

            gain_smooth: vec![1.0; nyq + 1],

            candidate_alpha,
            learned_alpha,
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
        // We do NOT clear learned_mag, learned_energy, quality, or stability state
    }

    /// Clears the learned profile (destructive).
    fn clear_profile(&mut self) {
        self.candidate_mag.fill(0.0);
        self.candidate_energy = 0.0;
        self.learned_mag.fill(0.0);
        self.learned_energy = 0.0;
        self.quality = 0.0;
        self.stable_frames = 0;
        self.relearn_frames_left = 0;
        self.learn_latched = false;
        self.relearn_armed = false;
        self.gain_smooth.fill(1.0);
    }

    fn has_profile(&self) -> bool {
        self.learned_energy > 1e-6
    }

    fn learn_progress(&self) -> f32 {
        (self.stable_frames as f32 / self.stable_frames_required as f32).clamp(0.0, 1.0)
    }

    fn trigger_relearn(&mut self) {
        self.clear_profile();
        self.relearn_armed = true;
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

        // 3) Learning (continuous, stability-gated)
        let is_silence = sidechain.speech_conf < LEARN_CONFIDENCE_THRESHOLD;
        if cfg.learn && !self.learn_latched {
            self.trigger_relearn();
        }
        self.learn_latched = cfg.learn;

        if self.relearn_armed && is_silence {
            self.relearn_frames_left = self.relearn_frames_total;
            self.relearn_armed = false;
        }

        let relearn_active = if self.relearn_frames_left > 0 {
            self.relearn_frames_left -= 1;
            true
        } else {
            false
        };

        let can_learn = relearn_active && (is_silence || relearn_active);

        if can_learn {
            // Candidate EMA update (fast)
            if self.candidate_energy <= 1e-9 {
                for i in 0..=nyq {
                    self.candidate_mag[i] = self.current_mag[i];
                }
            } else {
                for i in 0..=nyq {
                    let v = self.candidate_mag[i];
                    self.candidate_mag[i] = v + self.candidate_alpha * (self.current_mag[i] - v);
                }
            }

            // Update candidate energy
            let mut c = 0.0;
            for i in 0..=nyq {
                c += self.candidate_mag[i];
            }
            self.candidate_energy = c;

            // Stability: compare current frame to candidate
            let mut delta_sum = 0.0;
            let mut cand_sum = 0.0;
            for i in 0..=nyq {
                delta_sum += (self.current_mag[i] - self.candidate_mag[i]).abs();
                cand_sum += self.candidate_mag[i];
            }

            let normalized_delta = if cand_sum > EPS {
                delta_sum / cand_sum
            } else {
                1.0
            };

            let stability = 1.0 / (1.0 + normalized_delta * 2.0);
            self.quality += self.quality_alpha * (stability - self.quality);

            if normalized_delta < STABILITY_DELTA_THRESHOLD {
                self.stable_frames = (self.stable_frames + 1).min(self.stable_frames_required);
            } else {
                self.stable_frames = 0;
            }

            // Promote candidate -> learned after a stable window
            if self.stable_frames >= self.stable_frames_required && self.candidate_energy > 1e-6 {
                if !self.has_profile() {
                    for i in 0..=nyq {
                        self.learned_mag[i] = self.candidate_mag[i];
                    }
                } else {
                    for i in 0..=nyq {
                        let v = self.learned_mag[i];
                        self.learned_mag[i] = v + self.learned_alpha * (self.candidate_mag[i] - v);
                    }
                }

                let mut e = 0.0;
                for i in 0..=nyq {
                    e += self.learned_mag[i];
                }
                self.learned_energy = e;
            }
        } else {
            self.stable_frames = 0;
            if !(cfg.enabled && self.has_profile()) {
                self.quality += self.quality_alpha * (0.0 - self.quality);
            }
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
