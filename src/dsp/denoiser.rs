//! Denoiser Orchestrator (DSP vs DTLN Routing)
//!
//! Routes audio through either a traditional DSP-based denoiser or a DTLN-based
//! neural network denoiser based on user configuration. Provides a unified
//! interface for both approaches while keeping the implementations isolated.
//!
//! # Purpose
//! Offers users a choice between traditional DSP methods and machine learning
//! approaches for noise reduction, with seamless switching between approaches.
//!
//! # Design Notes
//! - Completely isolates DSP and DTLN implementations
//! - Provides unified interface for both approaches
//! - Allows seamless switching between methods
//! - Maintains separate state for each implementation

use crate::dsp::{
    dsp_denoiser::{DenoiseConfig as DspDenoiseConfig, DspDenoiser},
    dtln_denoiser::StereoDtlnDenoiser,
};
use crate::vs_log;

/// Trait defining the interface for stereo denoisers
pub trait StereoDenoiser {
    /// Process a single sample pair
    fn process_sample(&mut self, input_l: f32, input_r: f32, amount: f32) -> (f32, f32);

    /// Reset the denoiser state
    fn reset(&mut self);
}

/// Combined denoiser that can switch between DSP and DTLN implementations
pub struct StereoStreamingDenoiser {
    dsp_denoiser: DspDenoiser,
    dtln_denoiser: Option<StereoDtlnDenoiser>,
    current_use_dtln: bool,
    _win_size: usize,
    _hop_size: usize,
    sample_rate: f32,
}

impl StereoStreamingDenoiser {
    pub fn new(win_size: usize, hop_size: usize, sample_rate: f32) -> Self {
        Self {
            dsp_denoiser: DspDenoiser::new(win_size, hop_size),
            dtln_denoiser: None, // Defer heavy model loading
            current_use_dtln: false,
            _win_size: win_size,
            _hop_size: hop_size,
            sample_rate,
        }
    }

    /// Explicitly prepare/initialize the models. Safe to call from initialize().
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        if self.dtln_denoiser.is_none() {
            let mut dtln = StereoDtlnDenoiser::new(sample_rate);
            // Test the models with a simple input to verify they work
            let test_result = Self::test_dtln_models(&mut dtln);
            if test_result {
                self.dtln_denoiser = Some(dtln);
                vs_log!("[DENOISER] DTLN neural network models loaded and verified successfully");
            } else {
                vs_log!("[DENOISER] DTLN models failed verification test");
            }
        }
    }

    fn test_dtln_models(dtln: &mut StereoDtlnDenoiser) -> bool {
        // Test with a simple signal to verify the models work
        let test_input = 0.1; // Small signal
        let (out_l, out_r) = dtln.process_sample(test_input, test_input, 0.5, 0.5);

        // Check if outputs are reasonable (finite and not NaN)
        out_l.is_finite() && out_r.is_finite()
    }

    pub fn process_sample(
        &mut self,
        input_l: f32,
        input_r: f32,
        cfg: &DspDenoiseConfig,
    ) -> (f32, f32) {
        // Check if we need to switch implementations
        if cfg.use_dtln != self.current_use_dtln {
            if cfg.use_dtln && self.dtln_denoiser.is_some() {
                self.current_use_dtln = true;
                if let Some(d) = &mut self.dtln_denoiser {
                    d.reset();
                }
                vs_log!("[DENOISER] Switched to DTLN neural network mode");
            } else {
                self.current_use_dtln = false;
                vs_log!("[DENOISER] Switched to DSP spectral mode");
            }
        }

        if cfg.use_dtln {
            if let Some(d) = &mut self.dtln_denoiser {
                // Route through DTLN implementation with strength and tone parameters
                d.process_sample(input_l, input_r, cfg.amount, cfg.tone)
            } else {
                // DTLN requested but not available - warn user and fall back
                vs_log!("[DENOISER] DTLN not available, falling back to DSP");
                // Potentially notify UI that DTLN isn't working
                self.dsp_denoiser.process_sample(input_l, input_r, cfg)
            }
        } else {
            // Route through DSP implementation
            self.dsp_denoiser.process_sample(input_l, input_r, cfg)
        }
    }

    pub fn reset(&mut self) {
        self.dsp_denoiser.reset();
        if let Some(d) = &mut self.dtln_denoiser {
            d.reset();
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

    pub fn is_dtln_available(&self) -> bool {
        self.dtln_denoiser.is_some()
    }
}

/// Export the config struct for use by callers
pub use crate::dsp::dsp_denoiser::DenoiseConfig;
