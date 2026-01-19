//! DeepFilterNet Denoiser (Drop-in, Orchestrator-Compatible)
//!
//! This module is a **drop-in replacement** for `dtln_denoiser.rs` with the same
//! public surface area and runtime behavior guarantees:
//! - `StereoDeepFilterNetDenoiser::new(sample_rate)`
//! - `process_sample(l, r, amount) -> (l, r)`
//! - `reset()`
//!
//! # Reality check
//! DeepFilterNet itself is not “just a mask”. In most real deployments it produces
//! complex spectral filters / gains with internal temporal state.
//!
//! This file is written to be:
//! - **Safe and usable today** (never outputs silence unless input is silence)
//! - **Deterministic and RT-safe** (no allocations in `process_sample()`)
//! - **Easy to upgrade** to a real DeepFilterNet backend later without touching the orchestrator
//!
//! # How this integrates
//! Your orchestrator currently expects a stereo denoiser like `StereoDtlnDenoiser`.
//! This matches that interface exactly so you can route to it the same way.

use crate::dsp::utils::{lerp, make_sqrt_hann_window, BYPASS_AMOUNT_EPS, MAG_FLOOR};
use ringbuf::{Consumer, Producer, RingBuffer};
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::sync::Arc;

// =============================================================================
// Config (kept local to keep this denoiser isolated)
// =============================================================================

const DFN_FRAME_SIZE: usize = 512;
const DFN_HOP_SIZE: usize = 128;
const DFN_RINGBUF_CAP_MULT: usize = 4;
const DFN_OLA_NORM_EPS: f32 = 1e-6;

/// Conservative floor. DeepFilterNet-style denoisers *can* go lower, but this
/// prevents “dead air” surprises and keeps A/B testing honest.
const DFN_GAIN_FLOOR: f32 = 0.02;

/// This placeholder implements a “stronger-than-Wiener” style suppression curve
/// to stand in for ML until you wire in a real DFN model. It is intentionally
/// bounded to avoid wrecking voice timbre.
const DFN_STRONG_POWER: f32 = 1.6;
const DFN_NOISE_ALPHA: f32 = 0.96;

// =============================================================================
// Mono core
// =============================================================================

pub struct DeepFilterNetDenoiser {
    fft_forward: Arc<dyn Fft<f32>>,
    fft_backward: Arc<dyn Fft<f32>>,

    // Streaming
    input_producer: Producer<f32>,
    input_consumer: Consumer<f32>,
    output_producer: Producer<f32>,
    output_consumer: Consumer<f32>,

    // Frame buffers
    in_hop: [f32; DFN_HOP_SIZE],
    hop_pos: usize,

    frame: [f32; DFN_FRAME_SIZE],
    window: [f32; DFN_FRAME_SIZE],

    spec: [Complex<f32>; DFN_FRAME_SIZE],
    mag: [f32; DFN_FRAME_SIZE / 2 + 1],
    noise: [f32; DFN_FRAME_SIZE / 2 + 1],
    gains: [f32; DFN_FRAME_SIZE / 2 + 1],

    // OLA buffers
    ola: [f32; DFN_FRAME_SIZE],
    ola_norm: [f32; DFN_FRAME_SIZE],

    // Sample rate stored for future DFN backends
    #[allow(dead_code)]
    sample_rate: f32,
}

impl DeepFilterNetDenoiser {
    pub fn new(sample_rate: f32) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft_forward = planner.plan_fft_forward(DFN_FRAME_SIZE);
        let fft_backward = planner.plan_fft_inverse(DFN_FRAME_SIZE);

        // Window
        let w = make_sqrt_hann_window(DFN_FRAME_SIZE);
        let mut window = [0.0f32; DFN_FRAME_SIZE];
        window.copy_from_slice(&w);

        // Ring buffers
        let cap = DFN_FRAME_SIZE * DFN_RINGBUF_CAP_MULT;
        let (in_prod, in_cons) = RingBuffer::<f32>::new(cap).split();
        let (out_prod, out_cons) = RingBuffer::<f32>::new(cap).split();

        // Prime output to avoid initial underflow “silence pops”
        let mut out_prod_init = out_prod;
        for _ in 0..DFN_FRAME_SIZE {
            let _ = out_prod_init.push(0.0);
        }

        Self {
            fft_forward,
            fft_backward,
            input_producer: in_prod,
            input_consumer: in_cons,
            output_producer: out_prod_init,
            output_consumer: out_cons,
            in_hop: [0.0; DFN_HOP_SIZE],
            hop_pos: 0,
            frame: [0.0; DFN_FRAME_SIZE],
            window,
            spec: [Complex::new(0.0, 0.0); DFN_FRAME_SIZE],
            mag: [0.0; DFN_FRAME_SIZE / 2 + 1],
            noise: [MAG_FLOOR; DFN_FRAME_SIZE / 2 + 1],
            gains: [1.0; DFN_FRAME_SIZE / 2 + 1],
            ola: [0.0; DFN_FRAME_SIZE],
            ola_norm: [0.0; DFN_FRAME_SIZE],
            sample_rate,
        }
    }

    #[inline]
    pub fn process_sample(&mut self, input: f32, amount: f32) -> f32 {
        // Push to input ringbuf
        let _ = self.input_producer.push(input);

        // Pull exactly one sample into the hop buffer if available
        if let Some(s) = self.input_consumer.pop() {
            self.in_hop[self.hop_pos] = s;
            self.hop_pos += 1;

            if self.hop_pos == DFN_HOP_SIZE {
                self.process_hop(amount);
                self.hop_pos = 0;
            }
        }

        // Output one sample
        self.output_consumer.pop().unwrap_or(0.0)
    }

    #[inline]
    fn process_hop(&mut self, amount: f32) {
        // If bypassed, do a clean low-latency passthrough with the same hop cadence.
        if amount <= BYPASS_AMOUNT_EPS {
            for i in 0..DFN_HOP_SIZE {
                let _ = self.output_producer.push(self.in_hop[i]);
            }
            return;
        }

        // Slide frame left by hop, append new hop at end
        self.frame.copy_within(DFN_HOP_SIZE..DFN_FRAME_SIZE, 0);
        for i in 0..DFN_HOP_SIZE {
            self.frame[DFN_FRAME_SIZE - DFN_HOP_SIZE + i] = self.in_hop[i];
        }

        // Window + FFT
        for i in 0..DFN_FRAME_SIZE {
            self.spec[i] = Complex::new(self.frame[i] * self.window[i], 0.0);
        }
        self.fft_forward.process(&mut self.spec);

        // Magnitudes
        let nyq = DFN_FRAME_SIZE / 2;
        for i in 0..=nyq {
            self.mag[i] = self.spec[i].norm().max(MAG_FLOOR);
        }

        // Noise estimate (simple, stable). Real DFN would not do this, but we keep it
        // to provide a meaningful A/B path until the model is wired.
        for i in 0..=nyq {
            let m = self.mag[i];
            let n = self.noise[i];
            self.noise[i] = if m < n {
                // fast attack downward
                m
            } else {
                // slow release upward
                DFN_NOISE_ALPHA * n + (1.0 - DFN_NOISE_ALPHA) * m
            }
            .max(MAG_FLOOR);
        }

        // Gain curve (placeholder “DFN-like”: stronger suppression with smooth shaping)
        // A real DFN backend will fill `self.gains` from model inference.
        for i in 0..=nyq {
            let speech_pow = self.mag[i] * self.mag[i];
            let noise_pow = self.noise[i] * self.noise[i] + MAG_FLOOR;

            // Soft ratio (0..1), then shaped to be more decisive
            let ratio = (speech_pow / (speech_pow + noise_pow)).clamp(0.0, 1.0);
            let shaped = ratio.powf(DFN_STRONG_POWER);

            // Amount blends between passthrough and suppression
            let g = lerp(1.0, shaped, amount).clamp(DFN_GAIN_FLOOR, 1.0);
            self.gains[i] = g;
        }

        // Apply gains to spectrum (magnitude scaling, preserve complex phase)
        for i in 0..=nyq {
            self.spec[i] *= self.gains[i];
        }

        // Restore conjugate symmetry
        self.spec[0].im = 0.0;
        self.spec[nyq].im = 0.0;
        for k in 1..nyq {
            self.spec[DFN_FRAME_SIZE - k] = self.spec[k].conj();
        }

        // iFFT
        self.fft_backward.process(&mut self.spec);

        // OLA accumulate
        let norm = 1.0 / DFN_FRAME_SIZE as f32;
        for i in 0..DFN_FRAME_SIZE {
            let x = self.spec[i].re * norm;
            let w = self.window[i];
            self.ola[i] += x * w;
            self.ola_norm[i] += w * w;
        }

        // Emit hop
        for i in 0..DFN_HOP_SIZE {
            let y = self.ola[i] / self.ola_norm[i].max(DFN_OLA_NORM_EPS);
            let _ = self.output_producer.push(y);
        }

        // Slide OLA buffers
        self.ola.copy_within(DFN_HOP_SIZE..DFN_FRAME_SIZE, 0);
        self.ola_norm.copy_within(DFN_HOP_SIZE..DFN_FRAME_SIZE, 0);
        for i in (DFN_FRAME_SIZE - DFN_HOP_SIZE)..DFN_FRAME_SIZE {
            self.ola[i] = 0.0;
            self.ola_norm[i] = 0.0;
        }
    }

    pub fn reset(&mut self) {
        self.in_hop.fill(0.0);
        self.hop_pos = 0;

        self.frame.fill(0.0);
        self.spec.fill(Complex::new(0.0, 0.0));
        self.mag.fill(0.0);
        self.noise.fill(MAG_FLOOR);
        self.gains.fill(1.0);

        self.ola.fill(0.0);
        self.ola_norm.fill(0.0);

        // Clear ring buffers
        while self.input_consumer.pop().is_some() {}
        while self.output_consumer.pop().is_some() {}

        // Repopulate output with zeros (stable startup)
        for _ in 0..DFN_FRAME_SIZE {
            let _ = self.output_producer.push(0.0);
        }
    }
}

// =============================================================================
// Stereo wrapper (drop-in surface area)
// =============================================================================

pub struct StereoDeepFilterNetDenoiser {
    left: DeepFilterNetDenoiser,
    right: DeepFilterNetDenoiser,
}

impl StereoDeepFilterNetDenoiser {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            left: DeepFilterNetDenoiser::new(sample_rate),
            right: DeepFilterNetDenoiser::new(sample_rate),
        }
    }

    #[inline]
    pub fn process_sample(&mut self, input_l: f32, input_r: f32, amount: f32) -> (f32, f32) {
        let out_l = self.left.process_sample(input_l, amount);
        let out_r = self.right.process_sample(input_r, amount);
        (out_l, out_r)
    }

    pub fn reset(&mut self) {
        self.left.reset();
        self.right.reset();
    }
}
