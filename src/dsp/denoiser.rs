//! Denoiser orchestrator (DSP only)
//!
//! This module currently exists to keep the existing public API stable while
//! routing all stereo denoising through the single traditional DSP implementation.

use crate::dsp::dsp_denoiser::{DenoiseConfig as DspDenoiseConfig, DspDenoiser};

/// Stereo denoiser wrapper exposing the old API surface.
pub struct StereoStreamingDenoiser {
    dsp_denoiser: DspDenoiser,
}

impl StereoStreamingDenoiser {
    pub fn new(win_size: usize, hop_size: usize, _sample_rate: f32) -> Self {
        Self {
            dsp_denoiser: DspDenoiser::new(win_size, hop_size),
        }
    }

    /// Placeholder for the previous DTLN model loading step.
    pub fn prepare(&mut self, _sample_rate: f32) {}

    pub fn process_sample(
        &mut self,
        input_l: f32,
        input_r: f32,
        cfg: &DspDenoiseConfig,
    ) -> (f32, f32) {
        self.dsp_denoiser.process_sample(input_l, input_r, cfg)
    }

    pub fn reset(&mut self) {
        self.dsp_denoiser.reset();
    }
}

pub use crate::dsp::dsp_denoiser::DenoiseConfig;
