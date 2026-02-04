//! DSP processing modules for Voice Studio.
//!
//! This module contains all the audio processing components organized into stages:
//!
//! ## Analysis (Sidechain)
//! - [`speech_confidence`] - Speech vs noise/silence detection for automation
//!
//! ## Early Processing Stage
//! - [`early_reflection`] - Short-lag reflection suppression (micro-deverb)
//! - [`speech_expander`] - Speech-aware downward expansion
//!
//! ## Restoration Stage
//! - [`denoiser`] - Spectral noise reduction with tone control
//! - [`deverber`] - Envelope-based reverb reduction (late reflections)
//!
//! ## Shaping Stage
//! - [`proximity`] - Low-end shaping for "close mic" effect
//! - [`clarity`] - High-frequency enhancement
//!
//! ## Dynamics Stage
//! - [`de_esser`] - Sibilance detection and reduction
//! - [`compressor`] - Stereo-linked leveling compression
//! - [`spectral_guardrails`] - Safety limits for extreme settings
//! - [`limiter`] - Output safety limiting
//!
//! ## Utilities
//! - [`biquad`] - Biquad filter implementations
//! - [`control_slew`] - Control value slew limiting (artifact prevention)
//! - [`utils`] - Shared DSP utilities (see ARCHITECTURE.md)

pub mod biquad;
pub mod breath_reducer;
pub mod clarity;
pub mod compressor;
pub mod control_slew;
pub mod de_esser;
pub mod denoiser;
pub mod deverber;
pub mod dsp_denoiser;
pub mod early_reflection;
pub mod envelope;
pub mod hiss_rumble;
pub mod limiter;
pub mod noise_learn_remove;
pub mod pink_ref_bias;
pub mod plosive_softener;
pub mod profile_analyzer;
pub mod proximity;
pub mod recovery_stage;
pub mod spectral_guardrails;
pub mod speech_confidence;
pub mod speech_expander;
pub mod speech_hpf;
pub mod utils;
pub use noise_learn_remove::{NoiseLearnRemove, NoiseLearnRemoveConfig};

pub use biquad::Biquad;
pub use breath_reducer::BreathReducer;
pub use clarity::{Clarity, ClarityDetector};
pub use compressor::LinkedCompressor;
pub use control_slew::SpectralControlLimiters;
pub use de_esser::{DeEsserBand, DeEsserDetector};
pub use denoiser::{DenoiseConfig, StereoStreamingDenoiser};
pub use deverber::StreamingDeverber;
pub use early_reflection::EarlyReflectionSuppressor;
pub use envelope::VoiceEnvelopeTracker;
pub use hiss_rumble::HissRumble;
pub use limiter::LinkedLimiter;
pub use pink_ref_bias::PinkRefBias;
pub use plosive_softener::PlosiveSoftener;
pub use profile_analyzer::ProfileAnalyzer;
pub use proximity::Proximity;
pub use recovery_stage::RecoveryStage;
pub use spectral_guardrails::SpectralGuardrails;
pub use speech_confidence::SpeechConfidenceEstimator;
pub use speech_expander::SpeechExpander;
pub use speech_hpf::SpeechHpf;

/// Lifecycle state model for DSP modules.
/// Ensures predictable behavior during training, active processing, and bypassing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Lifecycle {
    /// Module is analyzing signal to establish baseline (e.g. noise floor learning).
    /// Audio is passed through unchanged or with minimal safe processing.
    Learning,
    /// Module is fully active and processing audio according to parameters.
    Active,
    /// Module is holding state (e.g. during silence) to prevent drift.
    Holding,
    /// Module is bypassed. Audio is passed through, but state may still update (e.g. analysis).
    Bypassed,
}

pub struct RestorationChain {
    pub safety_hpf: Biquad,
    pub deverber: StreamingDeverber,
}

pub struct ShapingChain {
    pub proximity: Proximity,
    pub clarity: Clarity,
}

pub struct DynamicsChain {
    pub de_esser_band: DeEsserBand,
}

/// Channel processor containing all DSP effects for one audio channel
pub struct ChannelProcessor {
    pub envelope_tracker: VoiceEnvelopeTracker,
    pub restoration_chain: RestorationChain,
    pub shaping_chain: ShapingChain,
    pub dynamics_chain: DynamicsChain,
    pub bypass_restoration: bool,
    pub bypass_shaping: bool,
    pub bypass_dynamics: bool,
}

impl ChannelProcessor {
    pub fn new(win: usize, hop: usize, sr: f32) -> Self {
        let mut safety = Biquad::new();
        safety.update_hpf(80.0, 0.707, sr);
        Self {
            envelope_tracker: VoiceEnvelopeTracker::new(sr),
            restoration_chain: RestorationChain {
                safety_hpf: safety,
                deverber: StreamingDeverber::new(win, hop),
            },

            shaping_chain: ShapingChain {
                proximity: Proximity::new(sr),
                clarity: Clarity::new(sr),
            },
            dynamics_chain: DynamicsChain {
                de_esser_band: DeEsserBand::new(sr),
            },
            bypass_restoration: false,
            bypass_shaping: false,
            bypass_dynamics: false,
        }
    }
}
