//! DTLN Denoiser (native two-stage inference)
//!
//! Pipeline:
//!   time frame -> STFT -> |X| -> stage1 -> mask -> apply -> iSTFT -> y1
//!   y1 -> stage2 -> refinement -> output
//!
//! Notes:
//! - Inference is executed deterministically on every platform via the native DtlnCore.
//! - This keeps the audio path allocation-free while matching the existing DTLN behavior.

use crate::dsp::{
    biquad::Biquad,
    dtln_core::DtlnCore,
    utils::{make_sqrt_hann_window, perceptual_curve, BYPASS_AMOUNT_EPS, MAG_FLOOR},
};
use ringbuf::{Consumer, Producer, RingBuffer};
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::sync::Arc;

// ---------------------------
// Constants
// ---------------------------

const FRAME_SIZE: usize = 512;
const HOP_SIZE: usize = 128;
const RINGBUF_MULT: usize = 4;
const OLA_EPS: f32 = 1e-6;
const NYQ: usize = FRAME_SIZE / 2;
const MAG_BINS: usize = NYQ + 1;

// ---------------------------
// Mono streaming DTLN denoiser
// ---------------------------

pub struct DtlnDenoiser {
    fft_fwd: Arc<dyn Fft<f32>>,
    fft_inv: Arc<dyn Fft<f32>>,

    window: Vec<f32>,

    // hop buffer
    hop_in: Vec<f32>,
    // rolling frame
    frame: Vec<f32>,
    // FFT scratch
    spec: Vec<Complex<f32>>,

    // OLA
    overlap: Vec<f32>,
    ola_norm: Vec<f32>,

    // streaming buffers
    in_prod: Producer<f32>,
    in_cons: Consumer<f32>,
    out_prod: Producer<f32>,
    out_cons: Consumer<f32>,

    pos: usize,

    mags: Vec<f32>,
    mask: Vec<f32>,
    td_stage1: Vec<f32>,
    td_stage2: Vec<f32>,
    dtln_core: Option<DtlnCore>,

    // Tone control filters
    low_shelf: Biquad,
    high_shelf: Biquad,

    // Tone control state
    tone_db_current: f32,
    tone_db_target: f32,
    sample_rate: f32,
}

impl DtlnDenoiser {
    pub fn new(sample_rate: f32) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft_fwd = planner.plan_fft_forward(FRAME_SIZE);
        let fft_inv = planner.plan_fft_inverse(FRAME_SIZE);

        let window = make_sqrt_hann_window(FRAME_SIZE);

        let cap = FRAME_SIZE * RINGBUF_MULT;
        let (in_prod, in_cons) = RingBuffer::<f32>::new(cap).split();
        let (out_prod, out_cons) = RingBuffer::<f32>::new(cap).split();

        // Prime output to establish deterministic latency
        let mut out_prod_init = out_prod;
        for _ in 0..FRAME_SIZE {
            let _ = out_prod_init.push(0.0);
        }

        let dtln_core = DtlnCore::new().ok();

        // Initialize tone control filters
        let mut low_shelf = Biquad::new();
        let mut high_shelf = Biquad::new();
        low_shelf.update_low_shelf(250.0, 0.707, 0.0, sample_rate);
        high_shelf.update_high_shelf(4000.0, 0.707, 0.0, sample_rate);

        Self {
            fft_fwd,
            fft_inv,
            window,
            hop_in: vec![0.0; HOP_SIZE],
            frame: vec![0.0; FRAME_SIZE],
            spec: vec![Complex::new(0.0, 0.0); FRAME_SIZE],
            overlap: vec![0.0; FRAME_SIZE],
            ola_norm: vec![0.0; FRAME_SIZE],
            in_prod,
            in_cons,
            out_prod: out_prod_init,
            out_cons,
            pos: 0,
            mags: vec![0.0; MAG_BINS],
            mask: vec![1.0; MAG_BINS],
            td_stage1: vec![0.0; FRAME_SIZE],
            td_stage2: vec![0.0; FRAME_SIZE],
            dtln_core,
            low_shelf,
            high_shelf,
            tone_db_current: 0.0,
            tone_db_target: 0.0,
            sample_rate,
        }
    }

    pub fn process_sample(&mut self, input: f32, strength: f32, tone: f32) -> f32 {
        let _ = self.in_prod.push(input);

        // Pull one sample from input consumer if available
        if let Some(s) = self.in_cons.pop() {
            if self.pos < HOP_SIZE {
                self.hop_in[self.pos] = s;
            }
            self.pos += 1;

            if self.pos >= HOP_SIZE {
                self.process_hop(strength, tone);
                self.pos = 0;
            }
        }

        self.out_cons.pop().unwrap_or(0.0)
    }

    fn process_hop(&mut self, strength: f32, tone: f32) {
        let strength = strength.clamp(0.0, 1.0);
        let tone = tone.clamp(0.0, 1.0);
        if strength <= BYPASS_AMOUNT_EPS && tone <= BYPASS_AMOUNT_EPS {
            for i in 0..HOP_SIZE {
                let _ = self.out_prod.push(self.hop_in[i]);
            }
            return;
        }

        self.tone_db_target = (-2.0 + tone * 4.0).clamp(-2.0, 2.0);
        self.tone_db_current = self.tone_db_current * 0.98 + self.tone_db_target * 0.02;

        self.low_shelf.update_low_shelf(
            250.0,
            0.707,
            -self.tone_db_current * 0.5,
            self.sample_rate,
        );
        self.high_shelf.update_high_shelf(
            4000.0,
            0.707,
            self.tone_db_current * 0.5,
            self.sample_rate,
        );

        if self.dtln_core.is_some() {
            if strength <= BYPASS_AMOUNT_EPS {
                for i in 0..HOP_SIZE {
                    let tone_corrected = self
                        .high_shelf
                        .process(self.low_shelf.process(self.hop_in[i]));
                    let _ = self.out_prod.push(tone_corrected);
                }
                return;
            }

            self.frame.copy_within(HOP_SIZE..FRAME_SIZE, 0);
            self.frame[FRAME_SIZE - HOP_SIZE..].copy_from_slice(&self.hop_in);

            for i in 0..FRAME_SIZE {
                self.spec[i] = Complex::new(self.frame[i] * self.window[i], 0.0);
            }
            self.fft_fwd.process(&mut self.spec);

            for i in 0..=NYQ {
                self.mags[i] = self.spec[i].norm().max(MAG_FLOOR);
            }

            let stage1_ready = if let Some(core) = self.dtln_core.as_mut() {
                core.infer_stage1(&self.mags[..=NYQ], &mut self.mask)
            } else {
                false
            };

            for i in 0..=NYQ {
                let gain = if stage1_ready {
                    self.mask[i].clamp(0.0, 1.2)
                } else {
                    1.0
                };
                let scaled = self.spec[i] * gain;
                self.spec[i] = scaled;
                if i != 0 && i != NYQ {
                    self.spec[FRAME_SIZE - i] = scaled.conj();
                } else {
                    self.spec[i].im = 0.0;
                }
            }

            self.fft_inv.process(&mut self.spec);

            let norm = 1.0 / FRAME_SIZE as f32;

            for i in 0..FRAME_SIZE {
                self.td_stage1[i] = self.spec[i].re * norm * self.window[i];
            }

            let mut stage2_ready = false;
            if stage1_ready {
                if let Some(core) = self.dtln_core.as_mut() {
                    stage2_ready = core.infer_stage2(&self.td_stage1, &mut self.td_stage2);
                }
            }
            let stage_output = if stage2_ready {
                &self.td_stage2
            } else {
                &self.td_stage1
            };

            for i in 0..FRAME_SIZE {
                let s = stage_output[i] * self.window[i];
                self.overlap[i] += s;
                self.ola_norm[i] += self.window[i] * self.window[i];
            }

            let blend = (perceptual_curve(strength.clamp(0.0, 1.0)) * 0.85).clamp(0.0, 0.85);
            for i in 0..HOP_SIZE {
                let dtln_sample = self.overlap[i] / self.ola_norm[i].max(OLA_EPS);
                let dry_sample = self.hop_in[i];
                let mixed = dry_sample * (1.0 - blend) + dtln_sample * blend;
                let tone_corrected = self.high_shelf.process(self.low_shelf.process(mixed));
                let _ = self.out_prod.push(tone_corrected);
            }

            self.overlap.copy_within(HOP_SIZE..FRAME_SIZE, 0);
            self.ola_norm.copy_within(HOP_SIZE..FRAME_SIZE, 0);
            self.overlap[FRAME_SIZE - HOP_SIZE..].fill(0.0);
            self.ola_norm[FRAME_SIZE - HOP_SIZE..].fill(0.0);
            return;
        }

        for i in 0..HOP_SIZE {
            let tone_corrected = self
                .high_shelf
                .process(self.low_shelf.process(self.hop_in[i]));
            let _ = self.out_prod.push(tone_corrected);
        }
    }

    pub fn reset(&mut self) {
        self.hop_in.fill(0.0);
        self.frame.fill(0.0);
        self.spec.fill(Complex::new(0.0, 0.0));
        self.overlap.fill(0.0);
        self.ola_norm.fill(0.0);
        self.pos = 0;

        while self.in_cons.pop().is_some() {}
        while self.out_cons.pop().is_some() {}

        // Re-prime output for stable latency
        for _ in 0..FRAME_SIZE {
            let _ = self.out_prod.push(0.0);
        }

        self.mags.fill(0.0);
        self.mask.fill(1.0);
        self.td_stage1.fill(0.0);
        self.td_stage2.fill(0.0);
        if let Some(core) = self.dtln_core.as_mut() {
            core.reset();
        }

        // Reset tone filters
        self.low_shelf.reset();
        self.high_shelf.reset();
        self.tone_db_current = 0.0;
        self.tone_db_target = 0.0;
    }
}

// ---------------------------
// Stereo wrapper
// ---------------------------

pub struct StereoDtlnDenoiser {
    left: DtlnDenoiser,
    right: DtlnDenoiser,
}

impl StereoDtlnDenoiser {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            left: DtlnDenoiser::new(sample_rate),
            right: DtlnDenoiser::new(sample_rate),
        }
    }

    pub fn process_sample(&mut self, l: f32, r: f32, strength: f32, tone: f32) -> (f32, f32) {
        (
            self.left.process_sample(l, strength, tone),
            self.right.process_sample(r, strength, tone),
        )
    }

    pub fn reset(&mut self) {
        self.left.reset();
        self.right.reset();
    }
}
