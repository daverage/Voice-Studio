use crate::dsp::dtln_weights::{DtlnWeights, LstmLayer, DTLN_WEIGHTS};
use anyhow::Context;

const STAGE1_MAG_SIZE: usize = 257;
const FRAME_SIZE: usize = 512;
const STAGE2_FEATURES: usize = 256;
const ILN_EPS: f32 = 1e-7;

struct LstmState {
    hidden: Vec<f32>,
    cell: Vec<f32>,
}

impl LstmState {
    fn new(hidden_size: usize) -> Self {
        Self {
            hidden: vec![0.0; hidden_size],
            cell: vec![0.0; hidden_size],
        }
    }

    fn reset(&mut self) {
        self.hidden.fill(0.0);
        self.cell.fill(0.0);
    }
}

/// Core DTLN inference without any STFT/WOLA.
pub struct DtlnCore {
    weights: DtlnWeights,
    stage1_states: [LstmState; 2],
    stage2_states: [LstmState; 2],
    gate_buf: Vec<f32>,
    norm_buf: Vec<f32>,
    conv_buf: Vec<f32>,
    dense_buf: Vec<f32>,
}

impl DtlnCore {
    pub fn new() -> anyhow::Result<Self> {
        let weights =
            DtlnWeights::from_bytes(DTLN_WEIGHTS).context("failed to parse DTLN native weights")?;
        Ok(Self::with_weights(weights))
    }

    fn with_weights(weights: DtlnWeights) -> Self {
        let stage1_hidden = weights.stage1_lstm4.hidden_size();
        let stage2_hidden = weights.stage2_lstm6.hidden_size();
        let gate_capacity = (stage1_hidden.max(stage2_hidden)) * 4;
        Self {
            weights,
            stage1_states: [LstmState::new(stage1_hidden), LstmState::new(stage1_hidden)],
            stage2_states: [LstmState::new(stage2_hidden), LstmState::new(stage2_hidden)],
            gate_buf: vec![0.0; gate_capacity],
            norm_buf: vec![0.0; STAGE2_FEATURES],
            conv_buf: vec![0.0; STAGE2_FEATURES],
            dense_buf: vec![0.0; STAGE2_FEATURES],
        }
    }

    pub fn reset(&mut self) {
        for state in &mut self.stage1_states {
            state.reset();
        }
        for state in &mut self.stage2_states {
            state.reset();
        }
        self.gate_buf.fill(0.0);
        self.norm_buf.fill(0.0);
        self.conv_buf.fill(0.0);
        self.dense_buf.fill(0.0);
    }

    pub fn infer_stage1(&mut self, mag: &[f32], mask: &mut [f32]) -> bool {
        if mag.len() != STAGE1_MAG_SIZE || mask.len() != STAGE1_MAG_SIZE {
            return false;
        }
        let [state0, state1] = &mut self.stage1_states;
        if !run_lstm(&mut self.gate_buf, &self.weights.stage1_lstm4, mag, state0) {
            return false;
        }
        if !run_lstm(
            &mut self.gate_buf,
            &self.weights.stage1_lstm5,
            &state0.hidden,
            state1,
        ) {
            return false;
        }
        linear_with_bias(
            &state1.hidden,
            &self.weights.stage1_dense_kernel,
            &self.weights.stage1_dense_bias,
            mask,
        );
        mask.iter_mut().for_each(|v| *v = sigmoid(*v));
        true
    }

    pub fn infer_stage2(&mut self, frame: &[f32], output: &mut [f32]) -> bool {
        if frame.len() != FRAME_SIZE || output.len() != FRAME_SIZE {
            return false;
        }
        if !run_linear(frame, &self.weights.stage2_conv2_kernel, &mut self.conv_buf) {
            return false;
        }
        instant_layer_norm(
            &self.conv_buf,
            &self.weights.stage2_norm_mul,
            &self.weights.stage2_norm_add,
            &mut self.norm_buf,
        );
        let [state0, state1] = &mut self.stage2_states;
        if !run_lstm(
            &mut self.gate_buf,
            &self.weights.stage2_lstm6,
            &self.norm_buf,
            state0,
        ) {
            return false;
        }
        if !run_lstm(
            &mut self.gate_buf,
            &self.weights.stage2_lstm7,
            &state0.hidden,
            state1,
        ) {
            return false;
        }
        linear_with_bias(
            &state1.hidden,
            &self.weights.stage2_dense_kernel,
            &self.weights.stage2_dense_bias,
            &mut self.dense_buf,
        );
        for i in 0..STAGE2_FEATURES {
            self.dense_buf[i] = sigmoid(self.dense_buf[i]) * self.norm_buf[i];
        }
        run_linear(&self.dense_buf, &self.weights.stage2_conv3_kernel, output)
    }

}

fn run_linear(input: &[f32], kernel: &[f32], output: &mut [f32]) -> bool {
    let input_dim = input.len();
    let output_dim = output.len();
    if kernel.len() != input_dim * output_dim {
        return false;
    }
    for out_idx in 0..output_dim {
        let mut sum = 0.0;
        let mut kp = out_idx;
        for in_idx in 0..input_dim {
            sum += input[in_idx] * kernel[kp];
            kp += output_dim;
        }
        output[out_idx] = sum;
    }
    true
}

fn run_lstm(gate_buf: &mut [f32], layer: &LstmLayer, input: &[f32], state: &mut LstmState) -> bool {
    if input.len() != layer.input_size() || state.hidden.len() != layer.hidden_size() {
        return false;
    }
    let hidden_size = layer.hidden_size();
    let gates = hidden_size * 4;
    let gate_slice = &mut gate_buf[..gates];
    gate_slice.fill(0.0);
    for gate in 0..gates {
        let w_start = gate * layer.input_size();
        gate_slice[gate] = dot(&layer.w[w_start..w_start + layer.input_size()], input);
    }
    for gate in 0..gates {
        let r_start = gate * hidden_size;
        gate_slice[gate] += dot(&layer.r[r_start..r_start + hidden_size], &state.hidden);
        gate_slice[gate] += layer.bias_w[gate];
        gate_slice[gate] += layer.bias_r[gate];
    }
    for idx in 0..hidden_size {
        let i = sigmoid(gate_slice[idx]);
        let o = sigmoid(gate_slice[hidden_size + idx]);
        let f = sigmoid(gate_slice[2 * hidden_size + idx]);
        let g = gate_slice[3 * hidden_size + idx].tanh();
        let new_cell = f * state.cell[idx] + i * g;
        state.cell[idx] = new_cell;
        state.hidden[idx] = o * new_cell.tanh();
    }
    true
}

fn dot(lhs: &[f32], rhs: &[f32]) -> f32 {
    lhs.iter().zip(rhs.iter()).map(|(a, b)| a * b).sum()
}

fn linear_with_bias(input: &[f32], kernel: &[f32], bias: &[f32], output: &mut [f32]) {
    let output_dim = output.len();
    let input_dim = input.len();
    debug_assert_eq!(kernel.len(), input_dim * output_dim);
    for out_idx in 0..output_dim {
        let mut sum = bias[out_idx];
        let mut kp = out_idx;
        for in_idx in 0..input_dim {
            sum += input[in_idx] * kernel[kp];
            kp += output_dim;
        }
        output[out_idx] = sum;
    }
}

fn instant_layer_norm(input: &[f32], mul: &[f32], add: &[f32], output: &mut [f32]) {
    let len = input.len();
    if len == 0 {
        return;
    }
    let mean = input.iter().copied().sum::<f32>() / len as f32;
    let variance = input
        .iter()
        .map(|v| {
            let diff = v - mean;
            diff * diff
        })
        .sum::<f32>()
        / len as f32;
    let inv_std = 1.0 / (variance + ILN_EPS).sqrt();
    for i in 0..len {
        let normalized = (input[i] - mean) * inv_std;
        output[i] = normalized * mul[i] + add[i];
    }
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}
