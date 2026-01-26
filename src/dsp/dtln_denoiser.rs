//! Full DTLN Denoiser (embedded TFLite models)
//!
//! A neural network-based denoiser that uses deep learning to separate speech
//! from noise. Implements a two-stage neural pipeline that processes audio
//! through spectral and temporal neural networks for superior noise reduction.
//!
//! # Purpose
//! Provides advanced noise reduction using machine learning when traditional
//! DSP methods are insufficient. Offers more intelligent noise separation
//! compared to conventional spectral subtraction methods.
//!
//! # Design Notes
//! - Two-stage pipeline: STFT → Mask NN → iSTFT → Time NN → OLA
//! - Embedded TFLite models for efficient inference
//! - Control range: 0-90% for speech-safe processing, 90-100% for aggressive suppression
//! - Audio-thread safe with pre-allocated buffers

use crate::dsp::denoiser::StereoDenoiser;
use crate::dsp::utils::{make_sqrt_hann_window, smoothstep, BYPASS_AMOUNT_EPS};
use ringbuf::{Consumer, Producer, RingBuffer};
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::sync::Arc;
use tract_tflite::internal::anyhow;
use tract_tflite::prelude::*;

// --------------------------------------------------
// Embedded models
// --------------------------------------------------

const DTLN_MODEL_1: &[u8] = include_bytes!("../assets/models/dtln/model_1.tflite");
const DTLN_MODEL_2: &[u8] = include_bytes!("../assets/models/dtln/model_2.tflite");

// --------------------------------------------------
// Constants (match model training)
// --------------------------------------------------

const FRAME_SIZE: usize = 512;
const HOP_SIZE: usize = 128;
const BINS: usize = FRAME_SIZE / 2 + 1;
const RINGBUF_MULT: usize = 8;
const OLA_EPS: f32 = 1e-6;
const DENOISE_STRENGTH_MULT: f32 = 4.0;
const MAX_DENOISE_AMOUNT: f32 = 4.0;

// --------------------------------------------------
// Amount remap
// --------------------------------------------------

struct AmountMap {
    wet: f32,
    floor: f32,
}

fn remap_amount(amount: f32, speech_confidence: f32) -> AmountMap {
    let a = amount.clamp(0.0, MAX_DENOISE_AMOUNT);

    // Scale to 0-1 range for mapping
    let normalized = a / MAX_DENOISE_AMOUNT;

    // Cap wet mix at 90% for speech safety
    let wet = if normalized <= 0.9 {
        let t = smoothstep(0.0, 1.0, normalized / 0.9);
        t * 0.85
    } else {
        let t = smoothstep(0.0, 1.0, (normalized - 0.9) / 0.1);
        (0.85 + t * 0.05).min(0.90) // Cap at 90%
    };

    // Speech-aware residual dry floor
    let floor = if speech_confidence > 0.5 {
        0.08 // During speech: preserve more original
    } else {
        0.02 // During silence: allow more aggressive reduction
    };

    AmountMap { wet, floor }
}

// --------------------------------------------------
// Neural model wrapper
// --------------------------------------------------

struct NeuralStage {
    plan: SimplePlan<TypedFact, Box<dyn TypedOp>, TypedModel>,
    state: Vec<Tensor>,
}

impl NeuralStage {
    fn new(bytes: &[u8]) -> TractResult<Self> {
        let plan = tract_tflite::tflite()
            .model_for_read(&mut std::io::Cursor::new(bytes))?
            .into_optimized()?
            .into_runnable()?;

        // Allocate zeroed recurrent state tensors
        let mut state = Vec::new();
        for o in plan.model().outputs.iter().skip(1) {
            let fact = plan.model().outlet_fact(*o)?;
            let shape: Vec<usize> = fact
                .shape
                .iter()
                .map(|d| d.to_i64().map(|i| i as usize).unwrap_or(1))
                .collect();
            state.push(Tensor::zero_dt(fact.datum_type, &shape)?);
        }

        Ok(Self { plan, state })
    }

    fn run(&mut self, input: Tensor) -> TractResult<Tensor> {
        let mut inputs = vec![input];
        inputs.extend(self.state.iter().cloned());

        let tvalues: TVec<TValue> = inputs.into_iter().map(|t| t.into_tvalue()).collect();

        let outputs = self.plan.run(tvalues)?;

        // Output 0 = signal, rest = updated state
        self.state = outputs[1..]
            .iter()
            .map(|t| t.clone().into_tensor())
            .collect();
        Ok(outputs[0].clone().into_tensor())
    }

    fn reset(&mut self) {
        for s in &mut self.state {
            let _ = s.fill_t(0.0);
        }
    }
}

// --------------------------------------------------
// Stereo DTLN Wrapper
// --------------------------------------------------

pub struct StereoDtlnDenoiser {
    left: DtlnDenoiser,
    right: DtlnDenoiser,
}

impl StereoDtlnDenoiser {
    pub fn new(sample_rate: f32) -> TractResult<Self> {
        Ok(Self {
            left: DtlnDenoiser::new(sample_rate)?,
            right: DtlnDenoiser::new(sample_rate)?,
        })
    }
}

impl StereoDenoiser for StereoDtlnDenoiser {
    fn process_sample(&mut self, input_l: f32, input_r: f32, amount: f32) -> (f32, f32) {
        let amt = (amount * DENOISE_STRENGTH_MULT).clamp(0.0, MAX_DENOISE_AMOUNT);
        // TODO: speech_confidence will be passed from orchestrator in main loop integration
        let speech_confidence = 0.5; // Temporary default
        (
            self.left.process_sample(input_l, amt, speech_confidence),
            self.right.process_sample(input_r, amt, speech_confidence),
        )
    }

    fn reset(&mut self) {
        self.left.reset();
        self.right.reset();
    }
}

// --------------------------------------------------
// Mono DTLN
// --------------------------------------------------

pub struct DtlnDenoiser {
    fft_fwd: Arc<dyn Fft<f32>>,
    fft_inv: Arc<dyn Fft<f32>>,

    window: Vec<f32>,
    ola_accum: Vec<f32>,
    ola_norm: Vec<f32>,

    spectrum: Vec<Complex<f32>>,
    frame: Vec<f32>,

    mask_net: NeuralStage,
    time_net: NeuralStage,

    in_prod: Producer<f32>,
    in_cons: Consumer<f32>,
    out_prod: Producer<f32>,
    out_cons: Consumer<f32>,

    write_pos: usize,
}

impl DtlnDenoiser {
    pub fn new(_sample_rate: f32) -> TractResult<Self> {
        let mut planner = FftPlanner::new();
        let fft_fwd = planner.plan_fft_forward(FRAME_SIZE);
        let fft_inv = planner.plan_fft_inverse(FRAME_SIZE);

        let window = make_sqrt_hann_window(FRAME_SIZE);

        let mut ola_norm = vec![0.0; FRAME_SIZE];
        for i in 0..FRAME_SIZE {
            ola_norm[i] += window[i] * window[i];
        }
        for v in &mut ola_norm {
            *v = 1.0 / v.max(OLA_EPS);
        }

        let (in_prod, in_cons) = RingBuffer::new(FRAME_SIZE * RINGBUF_MULT).split();
        let (out_prod, out_cons) = RingBuffer::new(FRAME_SIZE * RINGBUF_MULT).split();

        Ok(Self {
            fft_fwd,
            fft_inv,
            window,
            ola_accum: vec![0.0; FRAME_SIZE],
            ola_norm,
            spectrum: vec![Complex::default(); FRAME_SIZE],
            frame: vec![0.0; FRAME_SIZE],
            mask_net: NeuralStage::new(DTLN_MODEL_1)?,
            time_net: NeuralStage::new(DTLN_MODEL_2)?,
            in_prod,
            in_cons,
            out_prod,
            out_cons,
            write_pos: 0,
        })
    }

    pub fn process_sample(&mut self, input: f32, amount: f32, speech_confidence: f32) -> f32 {
        let _ = self.in_prod.push(input);

        if let Some(x) = self.in_cons.pop() {
            self.frame[self.write_pos] = x;
            self.write_pos += 1;

            if self.write_pos == HOP_SIZE {
                if let Err(_e) = self.process_frame(amount, speech_confidence) {
                    // Fallback: push dry hop on neural failure
                    for i in 0..HOP_SIZE {
                        let _ = self.out_prod.push(self.frame[i]);
                    }
                }
                self.write_pos = 0;
            }
        }

        self.out_cons.pop().unwrap_or(0.0)
    }

    fn process_frame(&mut self, amount: f32, speech_confidence: f32) -> TractResult<()> {
        if amount <= BYPASS_AMOUNT_EPS {
            for i in 0..HOP_SIZE {
                let _ = self.out_prod.push(self.frame[i]);
            }
            return Ok(());
        }

        let map = remap_amount(amount, speech_confidence);

        // Shift frame
        self.frame.rotate_left(HOP_SIZE);

        // Window + FFT
        for i in 0..FRAME_SIZE {
            self.spectrum[i].re = self.frame[i] * self.window[i];
            self.spectrum[i].im = 0.0;
        }

        self.fft_fwd.process(&mut self.spectrum);

        // Magnitude tensor
        let mut mag = vec![0.0f32; BINS];
        for i in 0..BINS {
            mag[i] = self.spectrum[i].norm().sqrt();
        }

        let mag_tensor = Tensor::from_shape(&[1, BINS], &mag)?;

        // Mask NN
        let mask = self.mask_net.run(mag_tensor)?;
        let mask = mask.to_array_view::<f32>()?;

        if mask.shape() != &[1, BINS] {
            return Err(anyhow!(
                "Mask NN output shape mismatch: expected [1, {}], got {:?}",
                BINS,
                mask.shape()
            ));
        }

        // Apply mask
        for i in 0..BINS {
            let g = (1.0 - map.wet) + map.wet * mask[[0, i]].max(map.floor);
            self.spectrum[i] *= g;
            if i > 0 && i < FRAME_SIZE - i {
                self.spectrum[FRAME_SIZE - i] = self.spectrum[i].conj();
            }
        }

        self.fft_inv.process(&mut self.spectrum);

        // OLA
        for i in 0..FRAME_SIZE {
            let v = self.spectrum[i].re / FRAME_SIZE as f32;
            self.ola_accum[i] += v * self.window[i];
        }

        // Time NN on hop
        let hop: Vec<f32> = self.ola_accum[..HOP_SIZE]
            .iter()
            .zip(&self.ola_norm[..HOP_SIZE])
            .map(|(x, n)| x * n)
            .collect();

        let hop_tensor = Tensor::from_shape(&[1, HOP_SIZE], &hop)?;

        let refined = self.time_net.run(hop_tensor)?;
        let refined = refined.to_array_view::<f32>()?;

        if refined.shape() != &[1, HOP_SIZE] {
            return Err(anyhow!(
                "Time NN output shape mismatch: expected [1, {}], got {:?}",
                HOP_SIZE,
                refined.shape()
            ));
        }

        // Voiced frame protection: blend NN output with original during speech
        let blend = if speech_confidence > 0.6 {
            0.15 // 15% original during confident speech
        } else {
            0.0 // Full NN output otherwise
        };

        for i in 0..HOP_SIZE {
            let nn_out = refined[[0, i]];
            let original = hop[i];
            let blended = nn_out * (1.0 - blend) + original * blend;
            let _ = self.out_prod.push(blended);
        }

        self.ola_accum.rotate_left(HOP_SIZE);
        self.ola_accum[FRAME_SIZE - HOP_SIZE..].fill(0.0);
        Ok(())
    }

    pub fn reset(&mut self) {
        self.frame.fill(0.0);
        self.ola_accum.fill(0.0);
        self.mask_net.reset();
        self.time_net.reset();
        while self.in_cons.pop().is_some() {}
        while self.out_cons.pop().is_some() {}
    }
}
