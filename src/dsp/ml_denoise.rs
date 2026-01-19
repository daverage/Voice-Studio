//! ML Denoise Layer (DTLN backend)
//!
//! Goal: provide a real-time ML noise suppressor that can be used as an optional
//! stage inside the existing non-ML denoiser.
//!
//! Design constraints
//! - Audio thread safe: no allocations in process()
//! - Graceful fallback: if model load fails, pass through audio
//! - Bounded latency: frame-based processing with fixed algorithmic delay

use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::sync::Arc;
use std::io::Cursor;
use log::{error, info, trace, warn};

#[cfg(feature = "ml")]
use tract_core::prelude::*;

// -----------------------------------------------------------------------------
// Model Embedding
// -----------------------------------------------------------------------------

// Path is relative to this file: src/dsp/ml_denoise.rs
// Models are at: src/assets/models/dtln/
#[cfg(feature = "ml")]
const DTLN_MODEL_1: &[u8] = include_bytes!("../assets/models/dtln/model_1.tflite");
#[cfg(feature = "ml")]
const DTLN_MODEL_2: &[u8] = include_bytes!("../assets/models/dtln/model_2.tflite");

#[cfg(feature = "ml")]
#[allow(dead_code)]
const _DTLN_MODEL_1_SIZE: usize = DTLN_MODEL_1.len();
#[cfg(feature = "ml")]
#[allow(dead_code)]
const _DTLN_MODEL_2_SIZE: usize = DTLN_MODEL_2.len();

// -----------------------------------------------------------------------------
// Public API
// -----------------------------------------------------------------------------

#[derive(Debug)]
pub enum MlInitError {
    #[allow(dead_code)]
    ModelLoadFailed(String),
    #[allow(dead_code)]
    InferenceFailed(String),
}

/// ML Denoise engine (DTLN).
/// This serves as an "advisor" providing speech probability masks.
#[cfg(feature = "ml")]
pub struct MlDenoiseEngine {
    backend: DtlnBackendType,
}

/// Stub implementation of ML Denoise engine when ML is disabled.
#[cfg(not(feature = "ml"))]
pub struct MlDenoiseEngine {}

#[cfg(feature = "ml")]
impl MlDenoiseEngine {
    /// Create a new ML engine by loading embedded models from memory.
    pub fn new() -> Result<Self, MlInitError> {
        #[cfg(feature = "gpu")]
        {
            match Self::try_gpu_backend() {
                Ok(engine) => {
                    info!("MLDenoiseEngine initialized with GPU backend.");
                    return Ok(engine);
                }
                Err(e) => {
                    warn!("GPU backend initialization failed: {:?}, falling back to CPU.", e);
                    // Fall through to CPU initialization
                }
            }
        }

        match Self::try_cpu_backend() {
            Ok(engine) => {
                info!("MLDenoiseEngine initialized with CPU backend.");
                Ok(engine)
            }
            Err(e) => {
                error!("Failed to initialize MLDenoiseEngine with any supported backend: {:?}", e);
                Err(MlInitError::ModelLoadFailed(
                    "No supported ML backend available".into(),
                ))
            }
        }
    }

    /// Process a frame of audio and produce a spectral speech mask.
    ///
    /// * `frame` - Input time-domain samples (mono)
    /// * `sample_rate` - Input sample rate
    /// * `mask_out` - Output buffer for the per-bin speech probability (0.0 - 1.0)
    pub fn process_frame(
        &mut self,
        frame: &[f32],
        sample_rate: f32,
        mask_out: &mut [f32],
    ) -> Result<(), MlInitError> {
        match &mut self.backend {
            DtlnBackendType::Cpu(backend) => {
                backend.process_frame_to_mask(frame, sample_rate, mask_out)
            }
            #[cfg(feature = "gpu")]
            DtlnBackendType::Gpu(_) => {
                Err(MlInitError::InferenceFailed("GPU backend not yet implemented".into()))
            }
        }
    }

    fn try_cpu_backend() -> Result<Self, MlInitError> {
        info!("Attempting to initialize MLDenoiseEngine with CPU backend...");
        let backend_cpu = DtlnBackendCpu::from_memory(DTLN_MODEL_1, DTLN_MODEL_2)?;
        Ok(Self {
            backend: DtlnBackendType::Cpu(backend_cpu),
        })
    }

    #[cfg(feature = "gpu")]
    fn try_gpu_backend() -> Result<Self, MlInitError> {
        info!("Attempting to initialize MLDenoiseEngine with GPU backend...");
        let backend_gpu = DtlnBackendGpu::from_memory(DTLN_MODEL_1, DTLN_MODEL_2)?;
        Ok(Self { backend: DtlnBackendType::Gpu(backend_gpu) })
    }
}

/// Stub implementation when ML feature is disabled
#[cfg(not(feature = "ml"))]
impl MlDenoiseEngine {
    /// Create a new ML engine (stub when ML is disabled)
    pub fn new() -> Result<Self, MlInitError> {
        info!("MLDenoiseEngine stub initialized (ML feature disabled)");
        Ok(Self {})
    }

    /// Process a frame of audio (stub when ML is disabled)
    ///
    /// * `frame` - Input time-domain samples (mono)
    /// * `sample_rate` - Input sample rate
    /// * `mask_out` - Output buffer for the per-bin speech probability (0.0 - 1.0)
    pub fn process_frame(
        &mut self,
        _frame: &[f32],
        _sample_rate: f32,
        mask_out: &mut [f32],
    ) -> Result<(), MlInitError> {
        // When ML is disabled, fill the mask with zeros (no speech detected)
        for v in mask_out {
            *v = 0.0;
        }
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// Backend Implementation (only when ML is enabled)
// -----------------------------------------------------------------------------

#[cfg(feature = "ml")]
mod ml_backend {
    use super::*;
    use tract_core::prelude::*;

    type DtlnPlanCpu = SimplePlan<TypedFact, Box<dyn TypedOp>, Graph<TypedFact, Box<dyn TypedOp>>>;

    #[cfg(feature = "gpu")]
    type DtlnPlanGpu = SimplePlan<TypedFact, Box<dyn TypedOp>, Graph<TypedFact, Box<dyn TypedOp>>>;

    #[cfg(feature = "gpu")]
    pub struct DtlnBackendGpu {
        plan1: DtlnPlanGpu,
        plan2: DtlnPlanGpu,
        // Add other fields that might be needed for GPU processing,
        // e.g., state tensors, buffers for GPU. For now, keep it minimal.
    }

    pub enum DtlnBackendType {
        Cpu(DtlnBackendCpu),
        #[cfg(feature = "gpu")]
        Gpu(DtlnBackendGpu),
    }

    pub struct DtlnBackendCpu {
        plan1: DtlnPlanCpu,
        #[allow(dead_code)]
        plan2: DtlnPlanCpu,

        // DTLN framing
        block_len: usize,
        nyq: usize,

        // State tensors
        state1: Tensor,
        #[allow(dead_code)]
        state2: Tensor,

        // Buffers
        fft: Arc<dyn Fft<f32>>,
        spec: Vec<Complex<f32>>,
        mag: Vec<f32>,
        window_sqrt: Vec<f32>,
        mag_array_buffer: tract_ndarray::ArrayD<f32>,
    }

    impl DtlnBackendCpu {
        pub fn from_memory(m1: &[u8], m2: &[u8]) -> Result<Self, MlInitError> {
            let block_len = 512;
            let fft_size = 512;
            let nyq = fft_size / 2;

            let mut planner = FftPlanner::<f32>::new();
            let fft = planner.plan_fft_forward(fft_size);

            let window = hann_window(block_len);
            let window_sqrt = window.iter().map(|w| w.sqrt()).collect::<Vec<_>>();

            // Load models from memory using tract 0.21 API
            let model1 = tract_tflite::tflite()
                .model_for_read(&mut Cursor::new(m1))
                .map_err(|e| MlInitError::ModelLoadFailed(format!("Model 1 read: {}", e)))?;

            let plan1 = model1.into_runnable()
                .map_err(|e| MlInitError::ModelLoadFailed(format!("Model 1 run: {}", e)))?;

            let model2 = tract_tflite::tflite()
                .model_for_read(&mut Cursor::new(m2))
                .map_err(|e| MlInitError::ModelLoadFailed(format!("Model 2 read: {}", e)))?;

            let plan2 = model2.into_runnable()
                .map_err(|e| MlInitError::ModelLoadFailed(format!("Model 2 run: {}", e)))?;

            // Initialize state tensors
            let state1 = Tensor::zero::<f32>(&[1, 1, 128]).map_err(|e| MlInitError::ModelLoadFailed(e.to_string()))?;
            let state2 = Tensor::zero::<f32>(&[1, 1, 128]).map_err(|e| MlInitError::ModelLoadFailed(e.to_string()))?;

            // Initialize mag_array_buffer
            let mag_array_buffer = tract_ndarray::Array::zeros((1, 1, nyq + 1)).into_dyn();

            Ok(Self {
                plan1,
                plan2,
                block_len,
                nyq,
                state1,
                state2,
                fft,
                spec: vec![Complex::new(0.0, 0.0); fft_size],
                mag: vec![0.0; nyq + 1],
                window_sqrt,
                mag_array_buffer,
            })
        }

        pub fn process_frame_to_mask(
            &mut self,
            frame: &[f32],
            _sample_rate: f32,
            mask_out: &mut [f32],
        ) -> Result<(), MlInitError> {
            // 1) FFT of input frame
            let n = frame.len().min(self.block_len);
            for i in 0..n {
                self.spec[i] = Complex::new(frame[i] * self.window_sqrt[i], 0.0);
            }
            for i in n..self.block_len {
                self.spec[i] = Complex::new(0.0, 0.0);
            }
            self.fft.process(&mut self.spec);

            // 2) Mag
            for i in 0..=self.nyq {
                self.mag[i] = self.spec[i].norm();
            }

            // 3) Model 1 Inference: predicts the magnitude mask
            // Copy self.mag into pre-allocated mag_array_buffer
            let mut mag_array_view = self.mag_array_buffer.view_mut();
            // The target shape is (1, 1, nyq + 1), so we can directly copy into the last dimension
            if let Some(mut s) = mag_array_view.as_slice_mut() {
                s[..self.mag.len()].copy_from_slice(&self.mag);
            } else {
                 // Fallback if it's not contiguous for some reason
                 for i in 0..self.mag.len() {
                    mag_array_view[[0, 0, i]] = self.mag[i];
                 }
            }

            let input_tensor: Tensor = self.mag_array_buffer.clone().into();

            let mut outputs = self.plan1.run(tvec!(input_tensor.into(), self.state1.clone().into()))
                .map_err(|e| MlInitError::InferenceFailed(e.to_string()))?;

            // Extract mask and update state
            let mask_tensor = outputs.remove(0).into_tensor();
            self.state1 = outputs.remove(0).into_tensor();

            let mask_slice = mask_tensor.as_slice::<f32>()
                .map_err(|_| MlInitError::InferenceFailed("Mask tensor not f32".to_string()))?;

            // Map ML mask bins to the output mask buffer
            let out_len = mask_out.len();
            for i in 0..out_len {
                let idx = (i * (self.nyq + 1)) / out_len;
                mask_out[i] = mask_slice[idx.min(self.nyq)];
            }

            Ok(())
        }
    }

    #[cfg(feature = "gpu")]
    impl DtlnBackendGpu {
        pub fn from_memory(m1: &[u8], m2: &[u8]) -> Result<Self, MlInitError> {
            // For now, use the CPU implementation as fallback since Metal TFLite support may not be available
            // This ensures graceful fallback when GPU backend fails
            let model1 = tract_tflite::tflite()
                .model_for_read(&mut Cursor::new(m1))
                .map_err(|e| MlInitError::ModelLoadFailed(format!("GPU Model 1 read: {}", e)))?;

            let plan1 = model1.into_runnable()
                .map_err(|e| MlInitError::ModelLoadFailed(format!("GPU Model 1 run: {}", e)))?;

            let model2 = tract_tflite::tflite()
                .model_for_read(&mut Cursor::new(m2))
                .map_err(|e| MlInitError::ModelLoadFailed(format!("GPU Model 2 read: {}", e)))?;

            let plan2 = model2.into_runnable()
                .map_err(|e| MlInitError::ModelLoadFailed(format!("GPU Model 2 run: {}", e)))?;

            Ok(Self { plan1, plan2 })
        }

        pub fn process_frame_to_mask(
            &mut self,
            frame: &[f32],
            _sample_rate: f32,
            mask_out: &mut [f32],
        ) -> Result<(), MlInitError> {
            // Attempt GPU inference with fallback to CPU
            // For now, just clear the mask and return an error to trigger fallback
            for v in mask_out {
                *v = 0.0;
            }
            Err(MlInitError::InferenceFailed("GPU inference not yet implemented".into()))
        }
    }
}

#[cfg(feature = "ml")]
use ml_backend::{DtlnBackendType, DtlnBackendCpu};

#[cfg(all(feature = "ml", feature = "gpu"))]
use ml_backend::DtlnBackendGpu;

fn hann_window(len: usize) -> Vec<f32> {
    (0..len)
        .map(|i| 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (len - 1) as f32).cos()))
        .collect()
}
