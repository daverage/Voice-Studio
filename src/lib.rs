mod debug;
pub mod dsp;
mod macro_controller;
mod meters;
mod presets;
mod ui;
mod version;

use crate::dsp::{
    Biquad, BreathReducer, ChannelProcessor, ClarityDetector, DeEsserDetector, DenoiseConfig,
    EarlyReflectionSuppressor, HissRumble, LinkedCompressor, LinkedLimiter, NoiseLearnRemove,
    NoiseLearnRemoveConfig, PinkRefBias, PlosiveSoftener, ProfileAnalyzer, SpectralGuardrails,
    SpeechConfidenceEstimator, SpeechExpander, SpeechHpf, StereoStreamingDenoiser,
};
use crate::macro_controller::{compute_simple_macro_targets, SimpleMacroTargets};
use crate::meters::Meters;
use assert_no_alloc::permit_alloc;
use ebur128::{EbuR128, Mode};
use nih_plug::prelude::*;
use nih_plug_vizia::vizia::prelude::ContextProxy;
use nih_plug_vizia::{create_vizia_editor, ViziaState, ViziaTheming};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use ui::build_ui;

const DE_ESS_RMS_TAU_SEC: f32 = 0.050;
const DEFAULT_SAMPLE_RATE: f32 = 44100.0;

const LOUDNESS_PUMP_DELTA_DB: f32 = 2.0;
const LIMITER_PUMP_THRESHOLD_DB: f32 = 1.5;
const PUMP_LOG_COOLDOWN_BUFFERS: u32 = 50;

// =============================================================================
// TASK 1: CANONICAL DATA STRUCTURES (Data-Driven Calibration)
// =============================================================================

/// Static target envelope for professional voice-over audio.
/// These ranges define what "good" sounds like - all DSP decisions
/// are driven by distance from these targets.
///
/// IMPORTANT: This struct is immutable at runtime.
#[derive(Clone, Copy, Debug)]
pub struct TargetProfile {
    // Dynamics targets
    pub rms_min: f32,
    pub rms_max: f32,
    pub crest_factor_db_min: f32,
    pub crest_factor_db_max: f32,
    pub rms_variance_max: f32,

    // Noise targets
    pub noise_floor_min: f32,
    pub noise_floor_max: f32,
    pub snr_db_min: f32,

    // Reverb targets
    pub early_late_ratio_min: f32,
    pub early_late_ratio_max: f32,
    pub decay_slope_min: f32,
    pub decay_slope_max: f32,

    // Frequency balance targets
    pub presence_ratio_max: f32,
    pub air_ratio_max: f32,
    pub hf_variance_max: f32,
}

impl Default for TargetProfile {
    fn default() -> Self {
        Self::PROFESSIONAL_VO
    }
}

impl TargetProfile {
    /// Professional voice-over target envelope (from measured reference recordings)
    pub const PROFESSIONAL_VO: TargetProfile = TargetProfile {
        // Dynamics: consistent, moderate level
        rms_min: 0.045,
        rms_max: 0.060,
        crest_factor_db_min: 23.0,
        crest_factor_db_max: 27.0,
        rms_variance_max: 0.0015,

        // Noise: clean but natural
        noise_floor_min: 0.010,
        noise_floor_max: 0.015,
        snr_db_min: 10.0,

        // Reverb: present but controlled
        early_late_ratio_min: 0.50,
        early_late_ratio_max: 0.70,
        decay_slope_min: -0.0001,
        decay_slope_max: 0.0001,

        // Frequency: natural presence, no harshness
        presence_ratio_max: 0.01,
        air_ratio_max: 0.005,
        hf_variance_max: 3e-7,
    };

    /// Check if a value is within a target range
    #[inline]
    pub fn in_range(value: f32, min: f32, max: f32) -> bool {
        value >= min && value <= max
    }

    /// Compute distance from target (negative = below, positive = above, 0 = in range)
    #[inline]
    pub fn distance_from_range(value: f32, min: f32, max: f32) -> f32 {
        if value < min {
            value - min // negative
        } else if value > max {
            value - max // positive
        } else {
            0.0 // in range
        }
    }
}

/// Audio profile computed from signal analysis.
/// Used for both InputProfile (pre-DSP) and OutputProfile (post-DSP).
///
/// IMPORTANT: InputProfile is computed ONCE at block start, pre-DSP.
/// OutputProfile is computed ONCE at block end, post-DSP.
/// No mid-chain re-analysis for control decisions.
#[derive(Clone, Copy, Debug, Default)]
pub struct AudioProfile {
    // Dynamics metrics
    pub rms: f32,
    pub peak: f32,
    pub crest_factor_db: f32,
    pub rms_variance: f32,

    // Noise metrics
    pub noise_floor: f32,
    pub snr_db: f32,

    // Reverb metrics
    pub early_late_ratio: f32,
    pub decay_slope: f32,

    // Frequency balance metrics
    pub presence_ratio: f32,
    pub air_ratio: f32,
    pub hf_variance: f32,
}

impl AudioProfile {
    /// Check if this profile is fully within target bounds (clean audio detection)
    pub fn is_within_target(&self, target: &TargetProfile) -> bool {
        TargetProfile::in_range(self.rms, target.rms_min, target.rms_max)
            && TargetProfile::in_range(
                self.crest_factor_db,
                target.crest_factor_db_min,
                target.crest_factor_db_max,
            )
            && self.rms_variance <= target.rms_variance_max
            && self.snr_db >= target.snr_db_min
            && TargetProfile::in_range(
                self.early_late_ratio,
                target.early_late_ratio_min,
                target.early_late_ratio_max,
            )
            && TargetProfile::in_range(
                self.decay_slope,
                target.decay_slope_min,
                target.decay_slope_max,
            )
            && self.presence_ratio <= target.presence_ratio_max
            && self.air_ratio <= target.air_ratio_max
            && self.hf_variance <= target.hf_variance_max
    }
}

// =============================================================================
// TASK 2: CONDITION DETECTION (Hard Rules)
// =============================================================================

/// Detected audio conditions based on measured metrics.
/// These are read-only signals used for caps and guards.
#[derive(Clone, Copy, Debug, Default)]
pub struct DetectedConditions {
    /// Whisper: HF variance > 1e-6 AND SNR < 15 dB
    pub whisper: bool,

    /// Distant mic: Early/Late ratio < 0.05 AND Decay slope < -0.0005
    pub distant_mic: bool,

    /// Noisy environment: Noise floor > 0.05 AND SNR < 6 dB
    pub noisy_environment: bool,

    /// Clean/already-good: SNR >= 10 dB AND Early/Late >= 0.4 AND HF variance <= 3e-7
    pub clean_audio: bool,
}

impl DetectedConditions {
    /// Detect conditions from an audio profile using hard threshold rules
    pub fn detect(profile: &AudioProfile) -> Self {
        Self {
            // Whisper detection: breathy HF content with low SNR
            whisper: profile.hf_variance > 1e-6 && profile.snr_db < 15.0,

            // Distant mic detection: diffuse reverb field
            distant_mic: profile.early_late_ratio < 0.05 && profile.decay_slope < -0.0005,

            // Noisy environment detection: high noise floor
            noisy_environment: profile.noise_floor > 0.05 && profile.snr_db < 6.0,

            // Clean audio detection: already professional quality
            clean_audio: profile.snr_db >= 10.0
                && profile.early_late_ratio >= 0.4
                && profile.hf_variance <= 3e-7,
        }
    }
}

// -----------------------------------------------------------------------------
// PARAMETERS
// -----------------------------------------------------------------------------
#[derive(Params)]
pub struct VoiceParams {
    #[id = "noise_reduction"]
    pub noise_reduction: FloatParam,

    #[id = "rumble_amount"]
    pub rumble_amount: FloatParam,

    #[id = "hiss_amount"]
    pub hiss_amount: FloatParam,

    #[id = "noise_learn_amount"]
    pub noise_learn_amount: FloatParam,

    #[id = "noise_learn_trigger"]
    pub noise_learn_trigger: BoolParam,

    #[id = "noise_learn_clear"]
    pub noise_learn_clear: BoolParam,

    #[id = "reverb_reduction"]
    pub reverb_reduction: FloatParam,

    #[id = "clarity"]
    pub clarity: FloatParam,

    #[id = "proximity"]
    pub proximity: FloatParam,

    #[id = "de_esser"]
    pub de_esser: FloatParam,

    #[id = "leveler"]
    pub leveler: FloatParam,

    #[id = "output_gain"]
    pub output_gain: FloatParam,

    #[id = "breath_control"]
    pub breath_control: FloatParam,

    #[id = "use_ml"]
    pub use_ml: BoolParam,

    // -------------------------------------------------------------------------
    // MACRO CONTROLS (Easy Mode)
    // -------------------------------------------------------------------------
    #[id = "macro_mode"]
    pub macro_mode: BoolParam,

    #[id = "macro_clean"]
    pub macro_clean: FloatParam,

    #[id = "macro_enhance"]
    pub macro_enhance: FloatParam,

    #[id = "macro_control"]
    pub macro_control: FloatParam,

    /// Trigger a full plugin reset (internal buffers and state)
    #[id = "reset_all"]
    pub reset_all: BoolParam,

    // -------------------------------------------------------------------------
    // DSP FACTORY PRESETS
    // -------------------------------------------------------------------------
    #[id = "dsp_preset"]
    pub dsp_preset: EnumParam<presets::DspPreset>,

    // -------------------------------------------------------------------------
    // FINAL OUTPUT PRESETS
    // -------------------------------------------------------------------------
    #[id = "final_output_preset"]
    pub final_output_preset: EnumParam<presets::OutputPreset>,
}

// Helper to format values as "50%" for the DAW display
fn format_percent(v: f32) -> String {
    format!("{:.0}%", v * 100.0)
}

// Helper to format gain in dB
fn format_db(v: f32) -> String {
    format!("{:.1} dB", v)
}

// -----------------------------------------------------------------------------
// PLUGIN STRUCT
// -----------------------------------------------------------------------------
struct VoiceStudioPlugin {
    params: Arc<VoiceParams>,
    editor_state: Arc<ViziaState>,
    process_l: ChannelProcessor,
    process_r: ChannelProcessor,
    sample_rate: f32,
    ui_proxy: Arc<Mutex<Option<ContextProxy>>>,
    max_supported_block_size: usize,
    current_block_size: usize,

    // Core DSP modules
    denoiser: StereoStreamingDenoiser,
    pink_ref_bias: PinkRefBias,
    clarity_detector: ClarityDetector,
    linked_de_esser: DeEsserDetector,
    linked_compressor: LinkedCompressor,
    linked_limiter: LinkedLimiter,

    // New Easy Mode DSP modules
    speech_confidence: SpeechConfidenceEstimator,
    early_reflection_l: EarlyReflectionSuppressor,
    early_reflection_r: EarlyReflectionSuppressor,
    speech_expander: SpeechExpander,
    spectral_guardrails: SpectralGuardrails,
    hiss_rumble: HissRumble,
    noise_learn_remove: NoiseLearnRemove,

    // Hidden hygiene and automatic protection
    speech_hpf: SpeechHpf,
    plosive_softener_l: PlosiveSoftener,
    plosive_softener_r: PlosiveSoftener,
    breath_reducer_l: BreathReducer,
    breath_reducer_r: BreathReducer,

    // Speech band energy protection (300Hz - 3kHz)
    speech_band_pre_l: Biquad,
    speech_band_pre_r: Biquad,
    speech_band_post_l: Biquad,
    speech_band_post_r: Biquad,
    speech_band_pre_lpf_l: Biquad,
    speech_band_pre_lpf_r: Biquad,
    speech_band_post_lpf_l: Biquad,
    speech_band_post_lpf_r: Biquad,

    // Loudness preservation trackers
    pre_rms_env: f32,
    post_rms_env: f32,
    loudness_comp_gain: f32,

    // Profile analyzers for data-driven calibration
    // INVARIANT: input_profile_analyzer processes ONLY pre-DSP samples
    // INVARIANT: output_profile_analyzer processes ONLY post-DSP samples
    // INVARIANT: No mid-chain re-measurement feeds control logic
    input_profile_analyzer: ProfileAnalyzer,
    output_profile_analyzer: ProfileAnalyzer,

    // Spectral control slew limiters (artifact prevention)
    control_limiters: dsp::SpectralControlLimiters,

    // Metering
    meters: Arc<Meters>,
    peak_input_l: f32,
    peak_input_r: f32,
    peak_output_l: f32,
    peak_output_r: f32,
    de_ess_rms_sq_l: f32,
    de_ess_rms_sq_r: f32,

    // Preset manager
    preset_manager: presets::PresetManager,

    // Preset loudness/true-peak processing
    loudness_meter: Option<EbuR128>,
    preset_gain_db: f32,
    preset_gain_lin: f32,
    last_output_preset: presets::OutputPreset,
    preset_interleaved_buffer: Vec<f32>,

    // Mode switch crossfade
    macro_xfade_samples_left: u32,
    macro_xfade_samples_total: u32,
    macro_xfade_to_macro: bool,
    last_macro_mode: bool,

    // Pump detection cooldown
    pump_log_cooldown: u32,
    prev_loudness_comp_gain: f32,
}

impl Default for VoiceStudioPlugin {
    fn default() -> Self {
        Self {
            params: Arc::new(VoiceParams {
                noise_reduction: FloatParam::new(
                    "Noise Reduction",
                    0.0,
                    FloatRange::Linear { min: 0.0, max: 1.0 },
                )
                .with_value_to_string(Arc::new(format_percent))
                .with_smoother(SmoothingStyle::Linear(50.0)),

                rumble_amount: FloatParam::new(
                    "Rumble",
                    0.0,
                    FloatRange::Linear { min: 0.0, max: 1.0 },
                )
                .with_value_to_string(Arc::new(format_percent))
                .with_smoother(SmoothingStyle::Linear(50.0)),

                hiss_amount: FloatParam::new(
                    "Hiss",
                    0.0,
                    FloatRange::Linear { min: 0.0, max: 1.0 },
                )
                .with_value_to_string(Arc::new(format_percent))
                .with_smoother(SmoothingStyle::Linear(50.0)),

                noise_learn_amount: FloatParam::new(
                    "Static Noise",
                    0.0,
                    FloatRange::Linear { min: 0.0, max: 1.0 },
                )
                .with_smoother(SmoothingStyle::Linear(100.0))
                .with_value_to_string(Arc::new(format_percent)),

                noise_learn_trigger: BoolParam::new("Learn Noise", false).non_automatable(),

                noise_learn_clear: BoolParam::new("Clear Noise", false).non_automatable(),

                reverb_reduction: FloatParam::new(
                    "De-Verb (Room)",
                    0.0,
                    FloatRange::Linear { min: 0.0, max: 1.0 },
                )
                .with_value_to_string(Arc::new(format_percent))
                .with_smoother(SmoothingStyle::Linear(50.0)),

                clarity: FloatParam::new("Clarity", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                    .with_value_to_string(Arc::new(format_percent))
                    .with_smoother(SmoothingStyle::Linear(50.0)),

                proximity: FloatParam::new(
                    "Proximity (Closeness)",
                    0.0,
                    FloatRange::Linear { min: 0.0, max: 1.0 },
                )
                .with_value_to_string(Arc::new(format_percent))
                .with_smoother(SmoothingStyle::Linear(50.0)),

                de_esser: FloatParam::new(
                    "De-Esser",
                    0.0,
                    FloatRange::Linear { min: 0.0, max: 1.0 },
                )
                .with_value_to_string(Arc::new(format_percent))
                .with_smoother(SmoothingStyle::Linear(50.0)),

                leveler: FloatParam::new(
                    "Leveler (Auto Volume)",
                    0.0,
                    FloatRange::Linear { min: 0.0, max: 1.0 },
                )
                .with_value_to_string(Arc::new(format_percent))
                .with_smoother(SmoothingStyle::Linear(50.0)),

                output_gain: FloatParam::new(
                    "Output Gain",
                    0.0,
                    FloatRange::Linear {
                        min: -12.0,
                        max: 12.0,
                    },
                )
                .with_value_to_string(Arc::new(format_db))
                .with_smoother(SmoothingStyle::Linear(50.0)),

                breath_control: FloatParam::new(
                    "Breath Control",
                    0.25,
                    FloatRange::Linear { min: 0.0, max: 1.0 },
                )
                .with_value_to_string(Arc::new(format_percent))
                .with_smoother(SmoothingStyle::Linear(50.0)),

                use_ml: BoolParam::new("Use ML Advisor", true),

                // Macro controls
                macro_mode: BoolParam::new("Easy Mode", true), // Start in Simple mode
                macro_clean: FloatParam::new(
                    "Clean",
                    0.0,
                    FloatRange::Linear { min: 0.0, max: 1.0 },
                )
                .with_value_to_string(Arc::new(format_percent))
                .with_smoother(SmoothingStyle::Linear(50.0)),
                macro_enhance: FloatParam::new(
                    "Enhance",
                    0.0,
                    FloatRange::Linear { min: 0.0, max: 1.0 },
                )
                .with_value_to_string(Arc::new(format_percent))
                .with_smoother(SmoothingStyle::Linear(50.0)),
                macro_control: FloatParam::new(
                    "Control",
                    0.0,
                    FloatRange::Linear { min: 0.0, max: 1.0 },
                )
                .with_value_to_string(Arc::new(format_percent))
                .with_smoother(SmoothingStyle::Linear(50.0)),

                reset_all: BoolParam::new("Reset Plugin", false),

                dsp_preset: EnumParam::new("DSP Preset", presets::DspPreset::Manual),

                final_output_preset: EnumParam::new("Final Output", presets::OutputPreset::None),
            }),
            editor_state: ViziaState::new(|| (900, 550)),
            process_l: ChannelProcessor::new(2048, 512, DEFAULT_SAMPLE_RATE),
            process_r: ChannelProcessor::new(2048, 512, DEFAULT_SAMPLE_RATE),
            sample_rate: DEFAULT_SAMPLE_RATE,
            ui_proxy: Arc::new(Mutex::new(None)),

            // Core DSP modules
            denoiser: StereoStreamingDenoiser::new(2048, 512, DEFAULT_SAMPLE_RATE),
            pink_ref_bias: PinkRefBias::new(DEFAULT_SAMPLE_RATE),
            clarity_detector: ClarityDetector::new(DEFAULT_SAMPLE_RATE),
            linked_de_esser: DeEsserDetector::new(DEFAULT_SAMPLE_RATE),
            linked_compressor: LinkedCompressor::new(DEFAULT_SAMPLE_RATE),
            linked_limiter: LinkedLimiter::new(DEFAULT_SAMPLE_RATE),

            // New Easy Mode DSP modules
            speech_confidence: SpeechConfidenceEstimator::new(DEFAULT_SAMPLE_RATE),
            early_reflection_l: EarlyReflectionSuppressor::new(DEFAULT_SAMPLE_RATE),
            early_reflection_r: EarlyReflectionSuppressor::new(DEFAULT_SAMPLE_RATE),
            speech_expander: SpeechExpander::new(DEFAULT_SAMPLE_RATE),
            spectral_guardrails: SpectralGuardrails::new(DEFAULT_SAMPLE_RATE),
            hiss_rumble: HissRumble::new(DEFAULT_SAMPLE_RATE),
            noise_learn_remove: NoiseLearnRemove::new(2048, 512, DEFAULT_SAMPLE_RATE),

            speech_hpf: SpeechHpf::new(DEFAULT_SAMPLE_RATE),
            plosive_softener_l: PlosiveSoftener::new(DEFAULT_SAMPLE_RATE),
            plosive_softener_r: PlosiveSoftener::new(DEFAULT_SAMPLE_RATE),
            breath_reducer_l: BreathReducer::new(DEFAULT_SAMPLE_RATE),
            breath_reducer_r: BreathReducer::new(DEFAULT_SAMPLE_RATE),

            speech_band_pre_l: Biquad::new(),
            speech_band_pre_r: Biquad::new(),
            speech_band_post_l: Biquad::new(),
            speech_band_post_r: Biquad::new(),
            speech_band_pre_lpf_l: Biquad::new(),
            speech_band_pre_lpf_r: Biquad::new(),
            speech_band_post_lpf_l: Biquad::new(),
            speech_band_post_lpf_r: Biquad::new(),

            pre_rms_env: 0.0,
            post_rms_env: 0.0,
            loudness_comp_gain: 1.0,

            // Profile analyzers for data-driven calibration
            input_profile_analyzer: ProfileAnalyzer::new(DEFAULT_SAMPLE_RATE),
            output_profile_analyzer: ProfileAnalyzer::new(DEFAULT_SAMPLE_RATE),

            // Macro controller

            // Spectral control slew limiters (artifact prevention)
            control_limiters: dsp::SpectralControlLimiters::new(),

            // Metering
            meters: Arc::new(Meters::new()),
            peak_input_l: 0.0,
            peak_input_r: 0.0,
            peak_output_l: 0.0,
            peak_output_r: 0.0,
            de_ess_rms_sq_l: 0.0,
            de_ess_rms_sq_r: 0.0,

            // Preset manager (lightweight initialization)
            preset_manager: presets::PresetManager::empty(),

            loudness_meter: None,
            preset_gain_db: 0.0,
            preset_gain_lin: 1.0,
            last_output_preset: presets::OutputPreset::None,
            preset_interleaved_buffer: Vec::new(),

            macro_xfade_samples_left: 0,
            macro_xfade_samples_total: 0,
            macro_xfade_to_macro: false,
            last_macro_mode: true,
            pump_log_cooldown: 0,
            prev_loudness_comp_gain: 1.0,
            max_supported_block_size: 0,
            current_block_size: 0,
        }
    }
}

impl Plugin for VoiceStudioPlugin {
    const NAME: &'static str = "VxCleaner";
    const VENDOR: &'static str = "Andrzej Marczewski";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = "0.4.0";

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(2),
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        // Initialize logger early so denoiser messages are captured
        #[cfg(feature = "debug")]
        crate::debug::logger::init_logger();

        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.sample_rate = buffer_config.sample_rate;
            self.max_supported_block_size = buffer_config.max_buffer_size as usize;
            self.current_block_size = buffer_config.max_buffer_size as usize;
            self.process_l = ChannelProcessor::new(2048, 512, self.sample_rate);
            self.process_r = ChannelProcessor::new(2048, 512, self.sample_rate);

            // Core DSP modules
            self.denoiser = StereoStreamingDenoiser::new(2048, 512, self.sample_rate);

            self.pink_ref_bias = PinkRefBias::new(self.sample_rate);
            self.clarity_detector = ClarityDetector::new(self.sample_rate);
            self.linked_de_esser = DeEsserDetector::new(self.sample_rate);
            self.linked_compressor = LinkedCompressor::new(self.sample_rate);
            self.linked_limiter = LinkedLimiter::new(self.sample_rate);

            // New Easy Mode DSP modules
            self.speech_confidence = SpeechConfidenceEstimator::new(self.sample_rate);
            self.early_reflection_l = EarlyReflectionSuppressor::new(self.sample_rate);
            self.early_reflection_r = EarlyReflectionSuppressor::new(self.sample_rate);
            self.speech_expander = SpeechExpander::new(self.sample_rate);
            self.spectral_guardrails = SpectralGuardrails::new(self.sample_rate);
            self.hiss_rumble = HissRumble::new(self.sample_rate);
            self.noise_learn_remove = NoiseLearnRemove::new(2048, 512, self.sample_rate);

            self.speech_hpf = SpeechHpf::new(self.sample_rate);
            self.plosive_softener_l = PlosiveSoftener::new(self.sample_rate);
            self.plosive_softener_r = PlosiveSoftener::new(self.sample_rate);
            self.breath_reducer_l = BreathReducer::new(self.sample_rate);
            self.breath_reducer_r = BreathReducer::new(self.sample_rate);

            // Speech band: 300Hz HPF + 3kHz LPF
            self.speech_band_pre_l
                .update_hpf(300.0, 0.5, self.sample_rate);
            self.speech_band_pre_r
                .update_hpf(300.0, 0.5, self.sample_rate);
            self.speech_band_post_l
                .update_hpf(300.0, 0.5, self.sample_rate);
            self.speech_band_post_r
                .update_hpf(300.0, 0.5, self.sample_rate);
            self.speech_band_pre_lpf_l
                .update_lpf(3000.0, 0.5, self.sample_rate);
            self.speech_band_pre_lpf_r
                .update_lpf(3000.0, 0.5, self.sample_rate);
            self.speech_band_post_lpf_l
                .update_lpf(3000.0, 0.5, self.sample_rate);
            self.speech_band_post_lpf_r
                .update_lpf(3000.0, 0.5, self.sample_rate);

            self.pre_rms_env = 0.0;
            self.post_rms_env = 0.0;
            self.loudness_comp_gain = 1.0;

            // Profile analyzers for data-driven calibration
            self.input_profile_analyzer = ProfileAnalyzer::new(self.sample_rate);
            self.output_profile_analyzer = ProfileAnalyzer::new(self.sample_rate);

            self.de_ess_rms_sq_l = 0.0;
            self.de_ess_rms_sq_r = 0.0;

            // Initialize preset manager (non-fatal)
            self.preset_manager = presets::PresetManager::new();
            self.preset_interleaved_buffer =
                permit_alloc(|| vec![0.0; self.max_supported_block_size * 2]);
            self.recreate_loudness_meter();
            self.preset_gain_db = 0.0;
            self.preset_gain_lin = 1.0;
            self.last_output_preset = self.params.final_output_preset.value();

            self.macro_xfade_samples_left = 0;
            self.macro_xfade_samples_total = 0;
            self.macro_xfade_to_macro = self.params.macro_mode.value();
            self.last_macro_mode = self.params.macro_mode.value();

            // Latency: Denoise (1 win) + Deverb (1 win) = 2 windows
            // Window size is 2048
            _context.set_latency_samples(2048 * 2);

            // Flush any initialization log messages to file
            #[cfg(feature = "debug")]
            crate::debug::logger::drain_to_file();

            true
        }))
        .unwrap_or(false)
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        let params = self.params.clone();
        let meters = self.meters.clone();
        let ui_proxy = self.ui_proxy.clone();
        create_vizia_editor(
            self.editor_state.clone(),
            ViziaTheming::default(),
            move |cx, gui_context| {
                build_ui(
                    cx,
                    params.clone(),
                    meters.clone(),
                    ui_proxy.clone(),
                    gui_context,
                );
            },
        )
    }

    fn task_executor(&mut self) -> TaskExecutor<Self> {
        Box::new(move |_| {})
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.process_internal(buffer, _aux, _context)
        }))
        .unwrap_or(ProcessStatus::Normal)
    }

    fn reset(&mut self) {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.denoiser.reset();
            self.pink_ref_bias.reset();
            self.process_l.envelope_tracker.reset();
            self.process_r.envelope_tracker.reset();
            self.process_l.restoration_chain.deverber.reset();
            self.process_r.restoration_chain.deverber.reset();
            self.linked_compressor.reset();
            self.linked_de_esser.reset();
            self.linked_limiter.reset();
            self.speech_confidence.reset();
            self.early_reflection_l.reset();
            self.early_reflection_r.reset();
            self.speech_expander.reset();
            self.spectral_guardrails.reset();
            self.hiss_rumble.reset();
            self.noise_learn_remove.reset();
            self.speech_hpf.reset();
            self.plosive_softener_l.reset();
            self.plosive_softener_r.reset();
            self.breath_reducer_l.reset();
            self.breath_reducer_r.reset();
            self.input_profile_analyzer.reset();
            self.output_profile_analyzer.reset();
            self.meters.reset();

            self.preset_gain_db = 0.0;
            self.preset_gain_lin = 1.0;
            self.last_output_preset = self.params.final_output_preset.value();
            self.macro_xfade_samples_left = 0;
            self.macro_xfade_samples_total = 0;
            self.macro_xfade_to_macro = self.params.macro_mode.value();
            self.last_macro_mode = self.params.macro_mode.value();

            // Reset local peak trackers
            self.peak_input_l = -80.0;
            self.peak_input_r = -80.0;
            self.peak_output_l = -80.0;
            self.peak_output_r = -80.0;
            self.pump_log_cooldown = 0;
            self.prev_loudness_comp_gain = 1.0;
        }))
        .unwrap_or(());
    }
}

impl VoiceStudioPlugin {
    fn recreate_loudness_meter(&mut self) {
        permit_alloc(|| {
            self.loudness_meter =
                EbuR128::new(2, self.sample_rate as u32, Mode::I | Mode::TRUE_PEAK).ok();
        });
    }

    fn process_internal(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        if self.params.reset_all.value() {
            self.reset();
        }

        const MAX_GAIN: f32 = 2.0;

        // Note: DSP preset parameter changes are handled in the UI thread
        // when the user selects a preset from the dropdown. The preset
        // selection itself is stored as a parameter for DAW automation.

        // =====================================================================
        // MACRO MODE HANDLING
        // =====================================================================
        let macro_mode = self.params.macro_mode.value();

        if macro_mode != self.last_macro_mode {
            self.macro_xfade_samples_total = (0.046 * self.sample_rate).round().max(1.0) as u32;
            self.macro_xfade_samples_left = self.macro_xfade_samples_total;
            self.macro_xfade_to_macro = macro_mode;
            self.last_macro_mode = macro_mode;
        }

        // INVARIANT:
        // Macro mode MUST NOT alter DSP topology. It may only change parameter values.

        // Conditions are updated at end-of-buffer via update_input_profile().
        // We read the last-known values here for stability guards.
        let whisper = false;
        let noisy = false;

        // Compute macro targets once per buffer and reuse them.
        let frame_count_est = buffer.samples() as usize;
        self.current_block_size = frame_count_est;

        let macro_targets = compute_simple_macro_targets(&self.params);
        let advanced_targets = SimpleMacroTargets {
            noise_reduction: self.params.noise_reduction.value(),
            reverb_reduction: self.params.reverb_reduction.value(),
            proximity: self.params.proximity.value(),
            clarity: self.params.clarity.value(),
            de_esser: self.params.de_esser.value(),
            leveler: self.params.leveler.value(),
            breath_control: self.params.breath_control.value(),
            rumble: self.params.rumble_amount.value(),
            hiss: self.params.hiss_amount.value(),
        };

        let mut macro_blend = if macro_mode { 1.0 } else { 0.0 };
        if self.macro_xfade_samples_left > 0 {
            let elapsed = (self.macro_xfade_samples_total - self.macro_xfade_samples_left) as f32;
            let t = (elapsed / self.macro_xfade_samples_total as f32).clamp(0.0, 1.0);
            macro_blend = if self.macro_xfade_to_macro {
                t
            } else {
                1.0 - t
            };
            self.macro_xfade_samples_left = self
                .macro_xfade_samples_left
                .saturating_sub(frame_count_est as u32);
        }

        let blend = |a: f32, b: f32| a + (b - a) * macro_blend;

        let raw_noise = (blend(
            advanced_targets.noise_reduction,
            macro_targets.noise_reduction,
        ) * MAX_GAIN)
            .clamp(0.0, MAX_GAIN);

        let rumble_val = blend(self.params.rumble_amount.value(), macro_targets.rumble);
        let hiss_val = blend(self.params.hiss_amount.value(), macro_targets.hiss);

        let raw_reverb = (blend(
            advanced_targets.reverb_reduction,
            macro_targets.reverb_reduction,
        ) * MAX_GAIN)
            .clamp(0.0, 1.0);
        let raw_prox = (blend(advanced_targets.proximity, macro_targets.proximity) * MAX_GAIN)
            .clamp(0.0, MAX_GAIN);
        let raw_de_ess = (blend(advanced_targets.de_esser, macro_targets.de_esser) * MAX_GAIN)
            .clamp(0.0, MAX_GAIN);
        let level_amt = (blend(advanced_targets.leveler, macro_targets.leveler) * MAX_GAIN)
            .clamp(0.0, MAX_GAIN);
        let raw_clarity = (blend(advanced_targets.clarity, macro_targets.clarity) * MAX_GAIN)
            .clamp(0.0, MAX_GAIN);
        let breath_amt = blend(
            advanced_targets.breath_control,
            macro_targets.breath_control,
        )
        .clamp(0.0, 1.0);

        // Apply spectral control slew limiting (prevents warble/artifacts)
        let speech_loss_db = 0.0;
        let limited = self.control_limiters.process(
            raw_noise,
            raw_clarity,
            raw_de_ess,
            raw_reverb,
            raw_prox,
            whisper,
            noisy,
            speech_loss_db,
        );

        // --- Layer 2: Safeguard Interventions ---
        self.meters
            .speech_band_loss_db
            .store(speech_loss_db, Ordering::Relaxed);
        self.meters.speech_protection_active.store(
            if limited.speech_protection_active {
                1
            } else {
                0
            },
            Ordering::Relaxed,
        );
        self.meters
            .speech_protection_scale
            .store(limited.speech_protection_scale, Ordering::Relaxed);
        self.meters.energy_budget_active.store(
            if limited.energy_budget_active { 1 } else { 0 },
            Ordering::Relaxed,
        );
        self.meters
            .energy_budget_scale
            .store(limited.energy_budget_scale, Ordering::Relaxed);

        let mut noise_amt = limited.denoise;
        let mut clarity_amt = limited.clarity;
        let de_ess_amt = limited.deesser;
        let mut reverb_amt = limited.reverb;
        let prox_amt = limited.proximity;

        // Inter-module safety clamps (DSP stability)
        // Prevent destructive parameter interactions

        // Rule 1: Reduce clarity when proximity is active (avoid thinning bass-boosted signal)
        if prox_amt > 0.4 {
            clarity_amt *= 0.7;
        }

        // Rule 2: Reduce deverb when proximity or clarity are high (avoid over-processing)
        if prox_amt > 0.6 || clarity_amt > 0.6 {
            reverb_amt *= 0.75;
        }

        // Rule 3: Reduce denoise when clarity is very high (avoid thinning)
        if clarity_amt > 0.8 {
            noise_amt *= 0.85;
        }

        let output_gain_db = self.params.output_gain.value();
        let output_gain_lin = 10.0f32.powf(output_gain_db / 20.0);

        // --- Layer 1: Resolved Parameters (Post-Macro, Pre-Safeguard) ---
        // These are the values the engine *attempts* to apply before any safeguards
        self.meters
            .noise_reduction_resolved
            .store(raw_noise, Ordering::Relaxed);
        self.meters
            .deverb_resolved
            .store(raw_reverb, Ordering::Relaxed);
        self.meters
            .clarity_resolved
            .store(raw_clarity, Ordering::Relaxed);
        self.meters
            .deesser_resolved
            .store(raw_de_ess, Ordering::Relaxed);
        self.meters
            .proximity_resolved
            .store(raw_prox, Ordering::Relaxed);
        self.meters
            .leveler_resolved
            .store(level_amt, Ordering::Relaxed);
        self.meters
            .breath_reduction_resolved
            .store(breath_amt, Ordering::Relaxed);
        // noise_tone_resolved deprecated
        self.meters
            .noise_tone_resolved
            .store(0.0, Ordering::Relaxed);

        // --- NEW: Loudness Compensation Logic ---
        // Target preservation of pre-processing RMS within ±2 dB (Always on)
        // Slow smoothing for gain compensation (approx 2 second time constant)
        let rms_alpha = 1.0 - (-1.0 / (2.0 * self.sample_rate)).exp();
        // Removed unused energy tracking variables

        let bypass_restoration =
            self.process_l.bypass_restoration || self.process_r.bypass_restoration;
        let bypass_shaping = self.process_l.bypass_shaping || self.process_r.bypass_shaping;
        let bypass_dynamics = self.process_l.bypass_dynamics || self.process_r.bypass_dynamics;

        // Proximity contributes to deverb (closer = more deverb = less room sound)
        use crate::dsp::Proximity;

        // Proximity reduces how much de-verb is needed
        let prox_reduction = Proximity::get_deverb_contribution(prox_amt);
        let total_deverb = (reverb_amt - prox_reduction).clamp(0.0, 1.0);

        // Configs
        let denoise_cfg = DenoiseConfig {
            amount: noise_amt,
            sensitivity: (0.2 + 0.8 * noise_amt).clamp(0.2, 1.0),
            tone: 0.5, // Neutral, cleanup handled by HissRumble
            sample_rate: self.sample_rate,
            speech_confidence: 0.5, // Will be updated per-sample with actual sidechain value
        };

        // Peak decay rate: 13 dB/sec (typical for DAW meters)
        let decay_per_sample = 13.0 / self.sample_rate;
        let de_ess_alpha = 1.0 - (-1.0 / (DE_ESS_RMS_TAU_SEC * self.sample_rate)).exp();

        let channels = buffer.as_slice();
        if channels.len() < 2 {
            return ProcessStatus::Normal;
        }
        let (first_channel, remaining) = channels.split_at_mut(1);
        let left = &mut **first_channel
            .get_mut(0)
            .expect("channel slice should contain left channel");
        let right = &mut **remaining
            .get_mut(0)
            .expect("channel slice should contain right channel");

        let frame_count = self.current_block_size;

        for idx in 0..frame_count {
            let input_l = left[idx];
            let input_r = right[idx];

            let input_db_l = 20.0 * input_l.abs().max(1e-6).log10();
            let input_db_r = 20.0 * input_r.abs().max(1e-6).log10();
            self.peak_input_l = self.peak_input_l.max(input_db_l);
            self.peak_input_r = self.peak_input_r.max(input_db_r);

            // 0a. SPEECH HPF (Hidden hygiene)
            // Removes subsonic energy before any analysis or processing
            let (hpf_l, hpf_r) = self.speech_hpf.process(input_l, input_r);

            // 0d. SPEECH CONFIDENCE (sidechain analysis - no audio modification)
            // Must be computed from HPF, not noise-reduced audio
            let sidechain = self.speech_confidence.process(hpf_l, hpf_r);

            // 0x. NOISE LEARN REMOVE (Static Noise)
            // Independent of speech, works during silence
            let nlr_cfg = NoiseLearnRemoveConfig {
                enabled: self.params.noise_learn_amount.value() > 0.001,
                amount: self.params.noise_learn_amount.value(),
                learn: self.params.noise_learn_trigger.value(),
                clear: self.params.noise_learn_clear.value(),
            };
            let (nlr_l, nlr_r) = self
                .noise_learn_remove
                .process(hpf_l, hpf_r, nlr_cfg, &sidechain);

            // 0b. ENVELOPE TRACKING (Unified Source of Truth)
            // Tracks dynamics after static noise removal for better expander/gate behavior
            let env_l = self.process_l.envelope_tracker.process_sample(nlr_l);
            let env_r = self.process_r.envelope_tracker.process_sample(nlr_r);

            // 0c. INPUT PROFILE ANALYSIS (for data-driven calibration)
            // INVARIANT: Only pre-restoration samples are analyzed here
            // INVARIANT: This feeds condition detection and macro calibration
            // We use HPF signal to capture true noise floor for environment detection
            self.input_profile_analyzer.process(hpf_l, hpf_r);

            // Apply real hiss/rumble shaping here
            // Uses NLR output as base
            let (hr_l, hr_r) = self
                .hiss_rumble
                .process(nlr_l, nlr_r, rumble_val, hiss_val, &sidechain);

            // Track pre-processed speech band energy - Removed unused calculation

            // Update pre-processing RMS envelope for loudness compensation
            let pre_rms = (hpf_l * hpf_l + hpf_r * hpf_r) * 0.5;
            self.pre_rms_env += (pre_rms - self.pre_rms_env) * rms_alpha;

            // Periodically maintain stability to prevent long-term drift
            // Call every ~1000 samples to prevent numerical drift over long sessions
            if idx % 1000 == 0 {
                self.speech_confidence.maintain_stability();
            }

            // 1. EARLY REFLECTION SUPPRESSION (before denoise)
            // This handles short-lag reflections that make recordings sound "distant"
            let early_reflection_amt = (reverb_amt * 0.5).clamp(0.0, 1.0);

            let (pre_l, pre_r) = if bypass_restoration || early_reflection_amt < 0.001 {
                (hr_l, hr_r) // Use hiss/rumble processed signal
            } else {
                (
                    self.early_reflection_l
                        .process(hr_l, early_reflection_amt, &sidechain),
                    self.early_reflection_r
                        .process(hr_r, early_reflection_amt, &sidechain),
                )
            };

            // 2. SPEECH EXPANDER (after early reflection, before denoise)
            // Controls pauses and room swell without hard gating
            let expander_amt = (reverb_amt * 0.6).clamp(0.0, 1.0);

            let (exp_l, exp_r) = if expander_amt < 0.001 {
                (pre_l, pre_r)
            } else {
                self.speech_expander
                    .process(pre_l, pre_r, expander_amt, &sidechain, &env_l, &env_r)
            };

            // 3. PINK REFERENCE BIAS (Hidden Spectral Tonal Conditioning)
            // Gently nudges speech towards -3dB/oct tilt to improve stability.
            // Gated by speech confidence, bypassed if restoration disabled.
            let (bias_l, bias_r) = if bypass_restoration {
                (exp_l, exp_r)
            } else {
                self.pink_ref_bias.process(
                    exp_l,
                    exp_r,
                    sidechain.speech_conf,
                    prox_amt,
                    de_ess_amt,
                )
            };

            // A. RESTORATION STAGE (denoise, de-verb)
            let (s1_l, s1_r) = if bypass_restoration {
                (bias_l, bias_r)
            } else {
                // Update config with per-sample speech confidence
                let mut cfg = denoise_cfg;
                cfg.speech_confidence = sidechain.speech_conf;
                // Denoiser tone is now just 0.5 (neutral) as Hiss/Rumble handles bias
                cfg.tone = 0.5;
                self.denoiser.process_sample(bias_l, bias_r, &cfg)
            };

            // 4. PLOSIVE SOFTENER (after denoise, before breath)
            let s1b_l = self.plosive_softener_l.process(s1_l);
            let s1b_r = self.plosive_softener_r.process(s1_r);

            // 5. BREATH REDUCER (after plosive, before deverb)
            let s1c_l = self
                .breath_reducer_l
                .process(s1b_l, breath_amt, &sidechain, &env_l);
            let s1c_r = self
                .breath_reducer_r
                .process(s1b_r, breath_amt, &sidechain, &env_r);

            let s2_l = if bypass_restoration {
                s1c_l
            } else {
                self.process_l.restoration_chain.safety_hpf.process(s1c_l)
            };
            let s2_r = if bypass_restoration {
                s1c_r
            } else {
                self.process_r.restoration_chain.safety_hpf.process(s1c_r)
            };
            let s3_l = if bypass_restoration {
                s2_l
            } else {
                self.process_l.restoration_chain.deverber.process_sample(
                    s2_l,
                    total_deverb,
                    self.sample_rate,
                    sidechain.speech_conf,
                    clarity_amt,
                    prox_amt,
                )
            };
            let s3_r = if bypass_restoration {
                s2_r
            } else {
                self.process_r.restoration_chain.deverber.process_sample(
                    s2_r,
                    total_deverb,
                    self.sample_rate,
                    sidechain.speech_conf,
                    clarity_amt,
                    prox_amt,
                )
            };

            // B. SHAPING STAGE (proximity, clarity)
            // Proximity: adds low-end warmth (100-300Hz boost) for close-mic effect
            // Clarity: reduces low-mid mud (120-380Hz cut) for cleaner sound
            // These effects are now independent - order is proximity first, then clarity
            let (s4_l, s4_r) = if bypass_shaping {
                (s3_l, s3_r)
            } else {
                (
                    self.process_l.shaping_chain.proximity.process(
                        s3_l,
                        prox_amt,
                        sidechain.speech_conf,
                        clarity_amt,
                    ),
                    self.process_r.shaping_chain.proximity.process(
                        s3_r,
                        prox_amt,
                        sidechain.speech_conf,
                        clarity_amt,
                    ),
                )
            };

            let clarity_drive = if bypass_shaping {
                0.0
            } else {
                self.clarity_detector.analyze(s4_l, s4_r)
            };
            let (s5_l, s5_r) = if bypass_shaping {
                (s4_l, s4_r)
            } else {
                (
                    self.process_l.shaping_chain.clarity.process(
                        s4_l,
                        clarity_amt,
                        sidechain.speech_conf,
                        clarity_drive,
                    ),
                    self.process_r.shaping_chain.clarity.process(
                        s4_r,
                        clarity_amt,
                        sidechain.speech_conf,
                        clarity_drive,
                    ),
                )
            };

            self.de_ess_rms_sq_l += (s5_l * s5_l - self.de_ess_rms_sq_l) * de_ess_alpha;
            self.de_ess_rms_sq_r += (s5_r * s5_r - self.de_ess_rms_sq_r) * de_ess_alpha;

            // C. DYNAMICS STAGE (de-esser, leveler, limiter)
            let (s6_l, s6_r) = if bypass_dynamics {
                (s5_l, s5_r)
            } else {
                let de_ess_gain = self
                    .linked_de_esser
                    .compute_gain(s5_l, s5_r, de_ess_amt, &env_l, &env_r);
                let out_l = self
                    .process_l
                    .dynamics_chain
                    .de_esser_band
                    .apply(s5_l, de_ess_gain);
                let out_r = self
                    .process_r
                    .dynamics_chain
                    .de_esser_band
                    .apply(s5_r, de_ess_gain);
                (out_l, out_r)
            };

            // Control interaction safeguard: Apply leveler gain with consideration of de-esser and limiter activity
            // to prevent multiple systems from fighting each other
            let (s7_l, s7_r) = if bypass_dynamics {
                (s6_l, s6_r)
            } else {
                // Calculate de-esser reduction amount to adjust leveler behavior
                let de_ess_reduction_db = if de_ess_amt > 0.001 {
                    let input_power = (s5_l * s5_l + s5_r * s5_r) * 0.5;
                    let output_power = (s6_l * s6_l + s6_r * s6_r) * 0.5;
                    if output_power > 0.0 && input_power > 0.0 {
                        10.0f32 * (output_power / input_power as f32).log10()
                    } else {
                        0.0
                    }
                } else {
                    0.0
                };

                // Get current limiter gain reduction to adjust leveler behavior
                let limiter_gr_db = self.linked_limiter.get_gain_reduction_db();

                // Adjust leveler behavior based on both de-esser and limiter activity to prevent interaction
                let mut adjusted_level_amt = level_amt;

                if de_ess_reduction_db < -3.0 {
                    // Strong de-esser activity
                    adjusted_level_amt *= 0.7; // Reduce leveler aggression to prevent fight
                }

                if limiter_gr_db > 2.0 {
                    // Strong limiter activity - reduce leveler aggression to prevent pumping
                    adjusted_level_amt *= 0.8;
                }

                let leveler_gain = self.linked_compressor.compute_gain(
                    &env_l,
                    &env_r,
                    adjusted_level_amt,
                    sidechain.speech_conf,
                    prox_amt,
                    clarity_amt,
                );

                // Report pump detection to meters
                self.meters
                    .set_compressor_gain_delta_db(self.linked_compressor.get_gain_delta_db());
                if self.linked_compressor.is_pump_detected() {
                    self.meters.increment_pump_event();
                    self.meters
                        .set_pump_severity_db(self.linked_compressor.get_gain_delta_db());

                    // Log pump event (rate-limited by pump_log_cooldown)
                    if self.pump_log_cooldown == 0 {
                        vs_log!(
                            "[PUMP] delta={:.2}dB leveler_amt={:.2} speech={:.2} comp_gr={:.2}dB",
                            self.linked_compressor.get_gain_delta_db(),
                            adjusted_level_amt,
                            sidechain.speech_conf,
                            self.linked_compressor.get_gain_reduction_db()
                        );
                        self.pump_log_cooldown = 50; // ~1 second at 48kHz/512 buffer
                    }
                }

                (s6_l * leveler_gain, s6_r * leveler_gain)
            };

            // D. SPECTRAL GUARDRAILS (safety layer before limiter)
            // Prevents extreme settings from breaking sound
            // Note: Applied after leveler to ensure gain reduction doesn't exceed limiter threshold
            let (s7g_l, s7g_r) =
                self.spectral_guardrails
                    .process(s7_l, s7_r, true, sidechain.speech_conf);

            let (s8_l, s8_r) = if bypass_dynamics {
                (s7g_l, s7g_r)
            } else {
                let limiter_gain = self.linked_limiter.compute_gain(s7g_l, s7g_r);
                (s7g_l * limiter_gain, s7g_r * limiter_gain)
            };

            // E. OUTPUT GAIN
            let s9_l = s8_l * output_gain_lin;
            let s9_r = s8_r * output_gain_lin;

            // Track post-processed speech band energy - Removed unused calculation

            // Update post-processing RMS envelope
            let post_rms = (s9_l * s9_l + s9_r * s9_r) * 0.5;
            self.post_rms_env += (post_rms - self.post_rms_env) * rms_alpha;

            // Apply loudness compensation gain (Always on)
            let comp_out_l = s9_l * self.loudness_comp_gain;
            let comp_out_r = s9_r * self.loudness_comp_gain;

            let idx2 = idx * 2;
            if idx2 + 1 < frame_count * 2 && idx2 + 1 < self.preset_interleaved_buffer.len() {
                self.preset_interleaved_buffer[idx2] = comp_out_l;
                self.preset_interleaved_buffer[idx2 + 1] = comp_out_r;
            }

            // F. FINAL OUTPUT PRESETS (loudness normalization and true-peak limiting)
            let preset = self.params.final_output_preset.value();
            let (out_l, out_r) = if preset == presets::OutputPreset::None {
                (comp_out_l, comp_out_r)
            } else {
                (
                    comp_out_l * self.preset_gain_lin,
                    comp_out_r * self.preset_gain_lin,
                )
            };

            let mut out_l = out_l;
            let mut out_r = out_r;
            if !out_l.is_finite() || !out_r.is_finite() {
                out_l = 0.0;
                out_r = 0.0;
                self.pre_rms_env = 0.0;
                self.post_rms_env = 0.0;
                self.loudness_comp_gain = 1.0;
            }
            let abs_peak = out_l.abs().max(out_r.abs());
            if abs_peak > 4.0 {
                let scale = 4.0 / abs_peak;
                out_l *= scale;
                out_r *= scale;
            }

            let output_db_l = 20.0 * out_l.abs().max(1e-6).log10();
            let output_db_r = 20.0 * out_r.abs().max(1e-6).log10();
            self.peak_output_l = self.peak_output_l.max(output_db_l);
            self.peak_output_r = self.peak_output_r.max(output_db_r);

            // OUTPUT PROFILE ANALYSIS (for validation/debugging)
            // INVARIANT: Only post-DSP samples are analyzed here
            // INVARIANT: This is NOT used for control decisions
            self.output_profile_analyzer.process(out_l, out_r);

            left[idx] = out_l;
            right[idx] = out_r;
        }

        // =====================================================================
        // PRESET LOUDNESS + TRUE-PEAK UPDATE (end of buffer)
        // =====================================================================
        let preset = self.params.final_output_preset.value();
        if preset != self.last_output_preset {
            self.preset_gain_db = 0.0;
            self.preset_gain_lin = 1.0;
            self.last_output_preset = preset;
        }

        if let Some(meter) = self.loudness_meter.as_mut() {
            let frames = frame_count as usize;
            let needed = frames.saturating_mul(2);
            if needed <= self.max_supported_block_size * 2
                && needed <= self.preset_interleaved_buffer.len()
            {
                let _ = meter.add_frames_f32(&self.preset_interleaved_buffer[..needed]);
            }
        }

        if preset != presets::OutputPreset::None {
            if let Some(meter) = self.loudness_meter.as_mut() {
                let lufs = meter.loudness_global().ok();
                let tp_l = meter.true_peak(0).ok();
                let tp_r = meter.true_peak(1).ok();
                let true_peak_db = match (tp_l, tp_r) {
                    (Some(a), Some(b)) => a.max(b) as f32,
                    (Some(a), None) => a as f32,
                    (None, Some(b)) => b as f32,
                    _ => -120.0,
                };

                let lufs_target = self.preset_manager.get_lufs_target(preset).unwrap_or(0.0);
                let peak_ceiling = self
                    .preset_manager
                    .get_true_peak_ceiling(preset)
                    .unwrap_or(0.0);

                let mut target_gain_db = if let Some(current) = lufs {
                    (lufs_target - current as f32).clamp(-24.0, 24.0)
                } else {
                    0.0
                };

                let tp_limit_db = peak_ceiling - true_peak_db;
                if tp_limit_db.is_finite() {
                    target_gain_db = target_gain_db.min(tp_limit_db);
                }

                const PRESET_GAIN_TAU_SEC: f32 = 0.5;
                let frames = frame_count as f32;
                if frames > 0.0 {
                    let alpha = 1.0 - (-frames / (PRESET_GAIN_TAU_SEC * self.sample_rate)).exp();
                    self.preset_gain_db += (target_gain_db - self.preset_gain_db) * alpha;
                    self.preset_gain_lin = 10.0_f32.powf(self.preset_gain_db / 20.0);
                }
            }
        } else {
            self.preset_gain_db = 0.0;
            self.preset_gain_lin = 1.0;
        }

        // =====================================================================
        // DATA-DRIVEN CALIBRATION UPDATE (end of buffer)
        // =====================================================================
        // INVARIANT: InputProfile computed ONCE, pre-DSP
        // INVARIANT: OutputProfile computed ONCE, post-DSP
        // INVARIANT: Only InputProfile feeds control logic
        // INVARIANT: All condition flags derived from InputProfile only

        // Finalize input profile analysis
        self.input_profile_analyzer.finalize_frame();
        let input_profile = self.input_profile_analyzer.get_profile();

        // Finalize output profile analysis (for validation/debugging only)
        self.output_profile_analyzer.finalize_frame();
        let output_profile = self.output_profile_analyzer.get_profile();

        // --- Layer 3: Audible Outcome Metrics ---
        let output_rms_db = if output_profile.rms > 1e-8 {
            20.0 * output_profile.rms.log10()
        } else {
            -80.0
        };
        let output_peak_db = if output_profile.peak > 1e-8 {
            20.0 * output_profile.peak.log10()
        } else {
            -80.0
        };
        let total_gr_db = self.linked_compressor.get_gain_reduction_db()
            + self.linked_limiter.get_gain_reduction_db();

        self.meters
            .output_rms_db
            .store(output_rms_db, Ordering::Relaxed);
        self.meters
            .output_peak_db
            .store(output_peak_db, Ordering::Relaxed);
        self.meters
            .output_crest_db
            .store(output_profile.crest_factor_db, Ordering::Relaxed);
        self.meters
            .total_gain_reduction_db
            .store(total_gr_db, Ordering::Relaxed);

        // Update loudness compensation gain based on RMS envelopes (Always on)
        // Use more conservative approach to prevent pumping
        if self.post_rms_env > 1e-8 && self.pre_rms_env > 1e-8 {
            let current_ratio = (self.pre_rms_env / self.post_rms_env).sqrt();

            // Use a more conservative target gain (±10% instead of ±100%)
            let target_gain = current_ratio.clamp(0.9, 1.1);

            // Use a much slower slew rate for loudness compensation to prevent pumping
            let slow_rms_alpha = 1.0 - (-1.0 / (10.0 * self.sample_rate)).exp(); // 10 second time constant

            self.loudness_comp_gain += (target_gain - self.loudness_comp_gain) * slow_rms_alpha;
        } else {
            // Use a slower rate to return to unity gain
            let slow_rms_alpha = 1.0 - (-1.0 / (10.0 * self.sample_rate)).exp(); // 10 second time constant
            self.loudness_comp_gain += (1.0 - self.loudness_comp_gain) * slow_rms_alpha;
        }

        let loudness_error_db = if self.post_rms_env > 1e-8 && self.pre_rms_env > 1e-8 {
            10.0 * (self.pre_rms_env / self.post_rms_env).log10()
        } else {
            0.0
        };
        let loudness_comp_db = if self.loudness_comp_gain > 1e-8 {
            20.0 * self.loudness_comp_gain.log10()
        } else {
            0.0
        };
        let loudness_active = loudness_comp_db.abs() > 0.1;

        self.meters
            .loudness_error_db
            .store(loudness_error_db, Ordering::Relaxed);
        self.meters
            .loudness_comp_db
            .store(loudness_comp_db, Ordering::Relaxed);
        self.meters
            .loudness_active
            .store(if loudness_active { 1 } else { 0 }, Ordering::Relaxed);

        // Update DSP modules with profile-based adaptation
        // METRIC OWNERSHIP: Leveler owns RMS, crest factor, RMS variance
        self.linked_compressor
            .update_from_profile(input_profile.crest_factor_db, input_profile.rms_variance);

        let decay = decay_per_sample * frame_count as f32;
        self.peak_input_l = (self.peak_input_l - decay).max(-80.0);
        self.peak_input_r = (self.peak_input_r - decay).max(-80.0);
        self.peak_output_l = (self.peak_output_l - decay).max(-80.0);
        self.peak_output_r = (self.peak_output_r - decay).max(-80.0);

        // Update meter values atomically (done once per buffer for efficiency)
        self.meters.set_input_peak_l(self.peak_input_l);
        self.meters.set_input_peak_r(self.peak_input_r);
        self.meters.set_output_peak_l(self.peak_output_l);
        self.meters.set_output_peak_r(self.peak_output_r);

        // Get gain reduction from both channel compressors for true stereo metering
        let gr_db = self.linked_compressor.get_gain_reduction_db();
        self.meters.set_gain_reduction_l(gr_db);
        self.meters.set_gain_reduction_r(gr_db);

        // Update Quality Meter
        self.meters
            .set_noise_learn_quality(self.noise_learn_remove.get_quality());

        // =====================================================================
        // DEBUG METERS - for DSP analysis and tuning
        // =====================================================================
        // Speech confidence from the last sample
        let last_sidechain = self.speech_confidence.get_output();
        self.meters
            .set_debug_speech_confidence(last_sidechain.speech_conf);
        self.meters
            .set_debug_noise_floor_db(last_sidechain.noise_floor_db);

        // De-esser gain reduction
        self.meters
            .set_debug_deesser_gr_db(self.linked_de_esser.get_gain_reduction_db());

        // Limiter gain reduction
        let limiter_gr_db = self.linked_limiter.get_gain_reduction_db();
        self.meters.set_debug_limiter_gr_db(limiter_gr_db);

        // Early reflection suppression (average of L/R)
        let early_refl_avg = 0.5
            * (self.early_reflection_l.get_suppression()
                + self.early_reflection_r.get_suppression());
        self.meters.set_debug_early_reflection(early_refl_avg);

        // Spectral guardrails corrections
        self.meters
            .set_debug_guardrails_low_cut(self.spectral_guardrails.get_low_mid_cut_db());
        self.meters
            .set_debug_guardrails_high_cut(self.spectral_guardrails.get_high_cut_db());

        // Speech expander attenuation
        self.meters
            .set_debug_expander_atten_db(self.speech_expander.get_gain_reduction_db());

        // Hiss/Rumble processor debug meters
        self.meters
            .set_hiss_db_current(self.hiss_rumble.get_hiss_db_current());
        self.meters
            .set_rumble_hz_current(self.hiss_rumble.get_rumble_hz_current());

        // Detect sudden loudness compensation + limiter movement ("pumping")
        let prev_gain = self.prev_loudness_comp_gain.max(1e-6);
        let loudness_ratio = (self.loudness_comp_gain / prev_gain).max(1e-6);
        let loudness_delta_db = 20.0 * loudness_ratio.log10();

        // Enhanced pump detection with multiple indicators
        let pump_trigger = loudness_delta_db.abs() > LOUDNESS_PUMP_DELTA_DB
            || limiter_gr_db > LIMITER_PUMP_THRESHOLD_DB;

        // Additional pump detection: rapid changes in multiple systems
        let leveler_gr_db = self.linked_compressor.get_gain_reduction_db();

        // Check for correlated gain movements across systems
        let gain_movement_correlation = (leveler_gr_db - self.meters.get_gain_reduction_l()).abs()
            + (limiter_gr_db - self.meters.get_debug_limiter_gr_db()).abs();

        let enhanced_pump_trigger =
            pump_trigger || (gain_movement_correlation > 5.0 && loudness_delta_db.abs() > 1.0);

        // Pump detection - just track cooldown, no audio-thread logging
        if enhanced_pump_trigger && self.pump_log_cooldown == 0 {
            self.pump_log_cooldown = PUMP_LOG_COOLDOWN_BUFFERS;
        }

        if self.pump_log_cooldown > 0 {
            self.pump_log_cooldown -= 1;
        }
        self.prev_loudness_comp_gain = self.loudness_comp_gain;

        // Mode transition event handling (no audio-thread logging)
        #[cfg(feature = "debug")]
        {
            let m = &self.meters;
            const AUDIBLE_CHANGE_TOLERANCE_DB: f32 = 0.1;
            let mode_trans_event = m.mode_transition_event.load(Ordering::Relaxed);
            if mode_trans_event != 0 {
                let current_rms = m.output_rms_db.load(Ordering::Relaxed);
                let pre_switch_rms = m.pre_switch_audible_rms.load(Ordering::Relaxed);
                let audible_change_detected =
                    (current_rms - pre_switch_rms).abs() > AUDIBLE_CHANGE_TOLERANCE_DB;
                m.audible_change_detected.store(
                    if audible_change_detected { 1 } else { 0 },
                    Ordering::Relaxed,
                );
                m.mode_transition_event.store(0, Ordering::Relaxed);
                m.audible_change_detected.store(0, Ordering::Relaxed);
                m.pre_switch_audible_rms.store(-80.0, Ordering::Relaxed);
            }
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for VoiceStudioPlugin {
    const CLAP_ID: &'static str = "com.andrzej.vxcleaner";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Vocal Restoration Suite");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Restoration,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for VoiceStudioPlugin {
    const VST3_CLASS_ID: [u8; 16] = *b"VoiceStudio_Pro1";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[
        Vst3SubCategory::Fx,
        Vst3SubCategory::Restoration,
        Vst3SubCategory::Mastering,
    ];
}

nih_export_clap!(VoiceStudioPlugin);
nih_export_vst3!(VoiceStudioPlugin);
