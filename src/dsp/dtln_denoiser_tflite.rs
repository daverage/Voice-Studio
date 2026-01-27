#![cfg(feature = "tflite_validate")]
//! Legacy TFLite-based DTLN pipeline used only for validation.
//!
//! Mirrors the original implementation so outputs can be compared against the native core.

use crate::dsp::{
    biquad::Biquad,
    utils::{make_sqrt_hann_window, perceptual_curve, BYPASS_AMOUNT_EPS, MAG_FLOOR},
};
use ringbuf::{Consumer, Producer, RingBuffer};
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::sync::Arc;

use tflite::{
    ops::builtin::BuiltinOpResolver, FlatBufferModel, Interpreter, InterpreterBuilder, TensorIndex,
};

const FRAME_SIZE: usize = 512;
const HOP_SIZE: usize = 128;
const RINGBUF_MULT: usize = 4;
const OLA_EPS: f32 = 1e-6;
const NYQ: usize = FRAME_SIZE / 2;
const MAG_BINS: usize = NYQ + 1;

const DTLN_MODEL_1: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/assets/models/dtln/model_1.tflite"
));

const DTLN_MODEL_2: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/assets/models/dtln/model_2.tflite"
));

struct DtlnBackendDtflite {
    interp1: Interpreter<'static, BuiltinOpResolver>,
    interp2: Interpreter<'static, BuiltinOpResolver>,
    state_pairs_1: Vec<(TensorIndex, TensorIndex)>,
    state_pairs_2: Vec<(TensorIndex, TensorIndex)>,
    state_bufs_1: Vec<Vec<f32>>,
    state_bufs_2: Vec<Vec<f32>>,
    mask: Vec<f32>,
    td_stage1: Vec<f32>,
    td_stage2: Vec<f32>,
    mags: Vec<f32>,
}

impl DtlnBackendDtflite {
    fn new() -> Option<Self> {
        let interp1 = Self::build_interpreter(DTLN_MODEL_1)?;
        let interp2 = Self::build_interpreter(DTLN_MODEL_2)?;
        let (state_pairs_1, state_bufs_1) = Self::collect_state_pairs(&interp1);
        let (state_pairs_2, state_bufs_2) = Self::collect_state_pairs(&interp2);
        let mask = vec![1.0; MAG_BINS];
        let td_stage1 = vec![0.0; FRAME_SIZE];
        let td_stage2 = vec![0.0; FRAME_SIZE];
        let mags = vec![0.0; MAG_BINS];
        Some(Self {
            interp1,
            interp2,
            state_pairs_1,
            state_pairs_2,
            state_bufs_1,
            state_bufs_2,
            mask,
            td_stage1,
            td_stage2,
            mags,
        })
    }

    fn reset(&mut self) {
        for buf in &mut self.state_bufs_1 {
            buf.fill(0.0);
        }
        for buf in &mut self.state_bufs_2 {
            buf.fill(0.0);
        }
        self.mask.fill(1.0);
        self.td_stage1.fill(0.0);
        self.td_stage2.fill(0.0);
        self.mags.fill(0.0);
    }

    fn infer_stage1_mask_inplace(&mut self, mags: &[f32]) -> bool {
        if mags.len() != MAG_BINS {
            return false;
        }
        let input_idx = self.interp1.inputs()[0];
        if !Self::write_to_tensor(&mut self.interp1, input_idx, mags) {
            return false;
        }
        if !Self::write_state_inputs(&mut self.interp1, &self.state_pairs_1, &self.state_bufs_1) {
            return false;
        }
        if self.interp1.invoke().is_err() {
            return false;
        }
        let output_idx = self.interp1.outputs()[0];
        if !Self::read_from_output(&mut self.interp1, output_idx, &mut self.mask) {
            return false;
        }
        for val in &mut self.mask {
            *val = val.clamp(0.0, 1.2);
        }
        Self::read_state_outputs(
            &mut self.interp1,
            &self.state_pairs_1,
            &mut self.state_bufs_1,
        )
    }

    fn infer_stage2_time_inplace(&mut self, frame: &[f32]) -> bool {
        if frame.len() != FRAME_SIZE {
            return false;
        }
        let input_idx = self.interp2.inputs()[0];
        if !Self::write_to_tensor(&mut self.interp2, input_idx, frame) {
            return false;
        }
        if !Self::write_state_inputs(&mut self.interp2, &self.state_pairs_2, &self.state_bufs_2) {
            return false;
        }
        if self.interp2.invoke().is_err() {
            return false;
        }
        let output_idx = self.interp2.outputs()[0];
        if !Self::read_from_output(&mut self.interp2, output_idx, &mut self.td_stage2) {
            return false;
        }
        Self::read_state_outputs(
            &mut self.interp2,
            &self.state_pairs_2,
            &mut self.state_bufs_2,
        )
    }

    fn build_interpreter(model: &[u8]) -> Option<Interpreter<'static, BuiltinOpResolver>> {
        let model_buffer = FlatBufferModel::build_from_buffer(model.to_vec()).ok()?;
        let resolver = BuiltinOpResolver::default();
        let builder = InterpreterBuilder::new(model_buffer, resolver).ok()?;
        builder.build().ok()
    }

    fn collect_state_pairs(
        interp: &Interpreter<'static, BuiltinOpResolver>,
    ) -> (Vec<(TensorIndex, TensorIndex)>, Vec<Vec<f32>>) {
        let inputs = interp.inputs();
        let outputs = interp.outputs();
        let mut pairs = Vec::new();
        let mut bufs = Vec::new();
        for idx in 1..inputs.len() {
            if idx >= outputs.len() {
                continue;
            }
            let input_idx = inputs[idx];
            let output_idx = outputs[idx];
            if let Some(info) = interp.tensor_info(input_idx) {
                let elements = info.dims.iter().copied().product::<usize>().max(1);
                pairs.push((input_idx, output_idx));
                bufs.push(vec![0.0; elements]);
            }
        }
        (pairs, bufs)
    }

    fn write_to_tensor(
        interp: &mut Interpreter<'static, BuiltinOpResolver>,
        idx: TensorIndex,
        data: &[f32],
    ) -> bool {
        if let Ok(slice) = interp.tensor_data_mut::<f32>(idx) {
            let copy_len = data.len().min(slice.len());
            slice[..copy_len].copy_from_slice(&data[..copy_len]);
            for v in &mut slice[copy_len..] {
                *v = 0.0;
            }
            return true;
        }
        false
    }

    fn write_state_inputs(
        interp: &mut Interpreter<'static, BuiltinOpResolver>,
        pairs: &[(TensorIndex, TensorIndex)],
        bufs: &[Vec<f32>],
    ) -> bool {
        for (i, (input_idx, _)) in pairs.iter().enumerate() {
            if let Some(buf) = bufs.get(i) {
                if let Ok(slice) = interp.tensor_data_mut::<f32>(*input_idx) {
                    let copy_len = buf.len().min(slice.len());
                    slice[..copy_len].copy_from_slice(&buf[..copy_len]);
                    for v in &mut slice[copy_len..] {
                        *v = 0.0;
                    }
                }
            }
        }
        true
    }

    fn read_from_output(
        interp: &mut Interpreter<'static, BuiltinOpResolver>,
        idx: TensorIndex,
        target: &mut [f32],
    ) -> bool {
        if let Ok(slice) = interp.tensor_data::<f32>(idx) {
            let copy_len = slice.len().min(target.len());
            target[..copy_len].copy_from_slice(&slice[..copy_len]);
            for v in &mut target[copy_len..] {
                *v = 0.0;
            }
            return true;
        }
        false
    }

    fn read_state_outputs(
        interp: &mut Interpreter<'static, BuiltinOpResolver>,
        pairs: &[(TensorIndex, TensorIndex)],
        bufs: &mut [Vec<f32>],
    ) -> bool {
        for (i, (_, output_idx)) in pairs.iter().enumerate() {
            if let Some(buf) = bufs.get_mut(i) {
                if let Ok(slice) = interp.tensor_data::<f32>(*output_idx) {
                    let copy_len = slice.len().min(buf.len());
                    buf[..copy_len].copy_from_slice(&slice[..copy_len]);
                }
            }
        }
        true
    }
}

pub struct DtlnDenoiserTflite {
    fft_fwd: Arc<dyn Fft<f32>>,
    fft_inv: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    hop_in: Vec<f32>,
    frame: Vec<f32>,
    spec: Vec<Complex<f32>>,
    overlap: Vec<f32>,
    ola_norm: Vec<f32>,
    in_prod: Producer<f32>,
    in_cons: Consumer<f32>,
    out_prod: Producer<f32>,
    out_cons: Consumer<f32>,
    pos: usize,
    ml: Option<DtlnBackendDtflite>,
    low_shelf: Biquad,
    high_shelf: Biquad,
    tone_db_current: f32,
    tone_db_target: f32,
    sample_rate: f32,
}

impl DtlnDenoiserTflite {
    pub fn new(sample_rate: f32) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft_fwd = planner.plan_fft_forward(FRAME_SIZE);
        let fft_inv = planner.plan_fft_inverse(FRAME_SIZE);
        let window = make_sqrt_hann_window(FRAME_SIZE);
        let cap = FRAME_SIZE * RINGBUF_MULT;
        let (in_prod, in_cons) = RingBuffer::<f32>::new(cap).split();
        let (out_prod, out_cons) = RingBuffer::<f32>::new(cap).split();
        let mut out_prod_init = out_prod;
        for _ in 0..FRAME_SIZE {
            let _ = out_prod_init.push(0.0);
        }
        let ml = DtlnBackendDtflite::new();
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
            ml,
            low_shelf,
            high_shelf,
            tone_db_current: 0.0,
            tone_db_target: 0.0,
            sample_rate,
        }
    }

    pub fn process_sample(&mut self, input: f32, strength: f32, tone: f32) -> f32 {
        let _ = self.in_prod.push(input);
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
        if let Some(ml) = self.ml.as_mut() {
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
                ml.mags[i] = self.spec[i].norm().max(MAG_FLOOR);
            }
            let mags_copy: Vec<f32> = ml.mags[..=NYQ].to_vec();
            let stage1_ready = ml.infer_stage1_mask_inplace(&mags_copy);
            for i in 0..=NYQ {
                let gain = if stage1_ready { ml.mask[i] } else { 1.0 };
                let gain = gain.clamp(0.0, 1.2);
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
                ml.td_stage1[i] = self.spec[i].re * norm * self.window[i];
            }
            let td_stage1_copy: Vec<f32> = ml.td_stage1.clone();
            let stage2_ready = stage1_ready && ml.infer_stage2_time_inplace(&td_stage1_copy);
            let stage_output = if stage2_ready {
                &ml.td_stage2
            } else {
                &ml.td_stage1
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
        for _ in 0..FRAME_SIZE {
            let _ = self.out_prod.push(0.0);
        }
        if let Some(ml) = self.ml.as_mut() {
            ml.reset();
        }
        self.low_shelf.reset();
        self.high_shelf.reset();
        self.tone_db_current = 0.0;
        self.tone_db_target = 0.0;
    }
}

pub struct StereoDtlnDenoiserTflite {
    left: DtlnDenoiserTflite,
    right: DtlnDenoiserTflite,
}

impl StereoDtlnDenoiserTflite {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            left: DtlnDenoiserTflite::new(sample_rate),
            right: DtlnDenoiserTflite::new(sample_rate),
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
