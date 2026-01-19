//! Denoiser Orchestrator (DSP vs DTLN Routing)
//!
//! This module acts as an orchestrator that routes audio through either:
//! - DSP-based denoiser (traditional spectral Wiener filtering)
//! - DTLN-based denoiser (deep transform learning network)
//!
//! # Routing Logic
//! The orchestrator uses the `use_dtln` flag in DenoiseConfig to determine which
//! implementation to use. The two implementations are completely isolated with
//! no shared state, buffers, or processing logic.

use crate::dsp::{dsp_denoiser::{DspDenoiser, DenoiseConfig as DspDenoiseConfig}, dtln_denoiser::StereoDtlnDenoiser};

/// Trait defining the interface for stereo denoisers
pub trait StereoDenoiser {
    /// Process a single sample pair
    fn process_sample(&mut self, input_l: f32, input_r: f32, amount: f32) -> (f32, f32);

    /// Reset the denoiser state
    fn reset(&mut self);

    /// Prepare for a new sample rate (if needed)
    fn prepare(&mut self, sample_rate: f32);
}

/// Combined denoiser that can switch between DSP and DTLN implementations
pub struct StereoStreamingDenoiser {
    dsp_denoiser: DspDenoiser,
    dtln_denoiser: StereoDtlnDenoiser,
    current_use_dtln: bool,
    current_sample_rate: f32,
}

impl StereoStreamingDenoiser {
    pub fn new(win_size: usize, hop_size: usize) -> Self {
        let sample_rate = 44100.0; // Default sample rate

        Self {
            dsp_denoiser: DspDenoiser::new(win_size, hop_size),
            dtln_denoiser: StereoDtlnDenoiser::new(sample_rate),
            current_use_dtln: false,
            current_sample_rate: sample_rate,
        }
    }

    pub fn process_sample(
        &mut self,
        input_l: f32,
        input_r: f32,
        cfg: &DspDenoiseConfig,
    ) -> (f32, f32) {
        // Check if we need to switch implementations
        if cfg.use_dtln != self.current_use_dtln {
            self.current_use_dtln = cfg.use_dtln;
            // Reset the newly selected denoiser to clear any stale state
            if cfg.use_dtln {
                self.dtln_denoiser.reset();
            } else {
                // If switching back to DSP, we might want to reset it too
                // For now, we'll just continue with existing state
            }
        }

        if cfg.use_dtln {
            // Route through DTLN implementation
            self.dtln_denoiser.process_sample(input_l, input_r, cfg.amount)
        } else {
            // Route through DSP implementation
            self.dsp_denoiser.process_sample(input_l, input_r, cfg)
        }
    }

    #[allow(dead_code)]
    pub fn get_noise_confidence(&self) -> f32 {
        if self.current_use_dtln {
            // DTLN doesn't have the same concept of noise confidence as DSP
            // Return a neutral value
            1.0
        } else {
            self.dsp_denoiser.get_noise_confidence()
        }
    }

    /// Prepare for a new sample rate
    pub fn prepare(&mut self, sample_rate: f32) {
        if sample_rate != self.current_sample_rate {
            self.current_sample_rate = sample_rate;
            // Recreate the DTLN denoiser with the new sample rate
            self.dtln_denoiser = StereoDtlnDenoiser::new(sample_rate);
        }
    }
}

/// Export the config struct for use by callers
pub use crate::dsp::dsp_denoiser::DenoiseConfig;