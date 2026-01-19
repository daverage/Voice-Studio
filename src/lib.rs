mod dsp;
mod macro_controller;
mod meters;
mod ui;

use dsp::{
    ChannelProcessor, ClarityDetector, DeEsserDetector, DenoiseConfig, EarlyReflectionSuppressor,
    LinkedCompressor, LinkedLimiter, ProfileAnalyzer, SpectralGuardrails,
    SpeechConfidenceEstimator, SpeechExpander, StereoStreamingDenoiser,
};
use macro_controller::MacroController;
use meters::Meters;
use nih_plug::prelude::*;
use nih_plug_vizia::vizia::prelude::ContextProxy;
use nih_plug_vizia::{create_vizia_editor, ViziaState, ViziaTheming};
use std::sync::{Arc, Mutex};
use ui::build_ui;

const DE_ESS_RMS_TAU_SEC: f32 = 0.01;

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

    #[id = "noise_tone"]
    pub noise_tone: FloatParam,

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

    // -------------------------------------------------------------------------
    // PREVIEW CONTROLS (Subtractive effects only)
    // -------------------------------------------------------------------------
    // Note: Only subtractive effects should have preview - it lets you hear
    // what is being REMOVED from the signal. Additive/gain effects don't
    // have meaningful previews.
    /// Preview denoise removal (hear the noise being cut)
    #[id = "preview_denoise"]
    pub preview_denoise: BoolParam,

    /// Preview de-verb removal (hear the reverb being cut)
    #[id = "preview_deverb"]
    pub preview_deverb: BoolParam,

    /// Preview de-esser removal (hear the sibilance being cut)
    #[id = "preview_deesser"]
    pub preview_deesser: BoolParam,

    #[id = "use_ml"]
    pub use_ml: BoolParam,

    // -------------------------------------------------------------------------
    // MACRO CONTROLS (Easy Mode)
    // -------------------------------------------------------------------------
    #[id = "macro_mode"]
    pub macro_mode: BoolParam,

    #[id = "macro_distance"]
    pub macro_distance: FloatParam,

    #[id = "macro_clarity"]
    pub macro_clarity: FloatParam,

    #[id = "macro_consistency"]
    pub macro_consistency: FloatParam,
}

// Helper to format values as "50%" for the DAW display
fn format_percent(v: f32) -> String {
    format!("{:.0}%", v * 100.0)
}

// Helper to format gain in dB
fn format_db(v: f32) -> String {
    format!("{:.1} dB", v)
}

// Helper to format tone parameter (Rumble <-> Hiss)
fn format_tone(v: f32) -> String {
    let signed = ((v - 0.5) * 200.0).clamp(-100.0, 100.0);
    if signed < -0.5 {
        format!("Rumble ({:.0})", signed)
    } else if signed > 0.5 {
        format!("Hiss (+{:.0})", signed)
    } else {
        "Neutral (0)".to_string()
    }
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

    // Core DSP modules
    denoiser: StereoStreamingDenoiser,
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

    // Profile analyzers for data-driven calibration
    // INVARIANT: input_profile_analyzer processes ONLY pre-DSP samples
    // INVARIANT: output_profile_analyzer processes ONLY post-DSP samples
    // INVARIANT: No mid-chain re-measurement feeds control logic
    input_profile_analyzer: ProfileAnalyzer,
    output_profile_analyzer: ProfileAnalyzer,

    // Macro controller
    macro_controller: MacroController,

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

    // Debug logging (rate-limited)
    debug_log_counter: u32,
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

                noise_tone: FloatParam::new(
                    "Noise Tone",
                    0.5,
                    FloatRange::Linear { min: 0.0, max: 1.0 },
                )
                .with_value_to_string(Arc::new(format_tone))
                .with_smoother(SmoothingStyle::Linear(50.0)),

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

                // Preview controls (subtractive effects only - radio button behavior)
                preview_denoise: BoolParam::new("Preview Noise", false),
                preview_deverb: BoolParam::new("Preview De-Verb", false),
                preview_deesser: BoolParam::new("Preview De-Esser", false),

                use_ml: BoolParam::new("Use ML Advisor", true),

                // Macro controls
                macro_mode: BoolParam::new("Easy Mode", true), // Start in Simple mode
                macro_distance: FloatParam::new(
                    "Distance",
                    0.0,
                    FloatRange::Linear { min: 0.0, max: 1.0 },
                )
                .with_value_to_string(Arc::new(format_percent))
                .with_smoother(SmoothingStyle::Linear(50.0)),
                macro_clarity: FloatParam::new(
                    "Clarity",
                    0.0,
                    FloatRange::Linear { min: 0.0, max: 1.0 },
                )
                .with_value_to_string(Arc::new(format_percent))
                .with_smoother(SmoothingStyle::Linear(50.0)),
                macro_consistency: FloatParam::new(
                    "Consistency",
                    0.0,
                    FloatRange::Linear { min: 0.0, max: 1.0 },
                )
                .with_value_to_string(Arc::new(format_percent))
                .with_smoother(SmoothingStyle::Linear(50.0)),
            }),
            editor_state: ViziaState::new(|| (760, 480)),
            process_l: ChannelProcessor::new(2048, 512, 44100.0),
            process_r: ChannelProcessor::new(2048, 512, 44100.0),
            sample_rate: 44100.0,
            ui_proxy: Arc::new(Mutex::new(None)),

            // Core DSP modules
            denoiser: StereoStreamingDenoiser::new(2048, 512),
            clarity_detector: ClarityDetector::new(44100.0),
            linked_de_esser: DeEsserDetector::new(44100.0),
            linked_compressor: LinkedCompressor::new(44100.0),
            linked_limiter: LinkedLimiter::new(44100.0),

            // New Easy Mode DSP modules
            speech_confidence: SpeechConfidenceEstimator::new(44100.0),
            early_reflection_l: EarlyReflectionSuppressor::new(44100.0),
            early_reflection_r: EarlyReflectionSuppressor::new(44100.0),
            speech_expander: SpeechExpander::new(44100.0),
            spectral_guardrails: SpectralGuardrails::new(44100.0),

            // Profile analyzers for data-driven calibration
            input_profile_analyzer: ProfileAnalyzer::new(44100.0),
            output_profile_analyzer: ProfileAnalyzer::new(44100.0),

            // Macro controller
            macro_controller: MacroController::new(),

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
            debug_log_counter: 0,
        }
    }
}

impl Plugin for VoiceStudioPlugin {
    const NAME: &'static str = "Voice Studio";
    const VENDOR: &'static str = "Andrzej Marczewski";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = "1.0.1";

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
        self.sample_rate = buffer_config.sample_rate;
        self.process_l = ChannelProcessor::new(2048, 512, self.sample_rate);
        self.process_r = ChannelProcessor::new(2048, 512, self.sample_rate);

        // Core DSP modules
        self.denoiser = StereoStreamingDenoiser::new(2048, 512);
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

        // Profile analyzers for data-driven calibration
        self.input_profile_analyzer = ProfileAnalyzer::new(self.sample_rate);
        self.output_profile_analyzer = ProfileAnalyzer::new(self.sample_rate);

        self.de_ess_rms_sq_l = 0.0;
        self.de_ess_rms_sq_r = 0.0;

        // Latency: Denoise (1 win) + Deverb (1 win) = 2 windows
        // Window size is 2048
        _context.set_latency_samples(2048 * 2);
        true
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
        const MAX_GAIN: f32 = 2.0;

        // =====================================================================
        // MACRO MODE HANDLING
        // =====================================================================
        let macro_mode = self.params.macro_mode.value();

        // Update macro controller state
        if macro_mode {
            use macro_controller::MacroState;
            self.macro_controller.set_state(MacroState {
                distance: self.params.macro_distance.value(),
                clarity: self.params.macro_clarity.value(),
                consistency: self.params.macro_consistency.value(),
                active: true,
            });
        } else {
            self.macro_controller.set_active(false);
        }

        // Get detected conditions for control stability
        let conditions = self.macro_controller.get_conditions();
        let whisper = conditions.whisper;
        let noisy = conditions.noisy_environment;

        // Clarity is always from direct parameter (not macro-controlled in the same way)
        let raw_clarity = (self.params.clarity.value() * MAX_GAIN).clamp(0.0, 1.0);

        // Get parameter values - either from macros or direct controls
        let (raw_noise, noise_tone, raw_reverb, raw_prox, raw_de_ess, level_amt) = if macro_mode {
            // Update smoothed macro targets
            let frame_count_est = buffer.samples() as usize;
            let targets = self.macro_controller.update_smooth(frame_count_est);

            // Apply smoothed targets to actual parameters
            self.macro_controller
                .apply_smoothed_targets_to_params(&self.params);

            // Apply macro targets to parameters
            (
                (targets.noise_reduction * MAX_GAIN).clamp(0.0, MAX_GAIN),
                targets.noise_tone,
                targets.reverb_reduction.clamp(0.0, 1.0),
                (targets.proximity * MAX_GAIN).clamp(0.0, MAX_GAIN),
                (targets.de_esser * MAX_GAIN).clamp(0.0, MAX_GAIN),
                (targets.leveler * MAX_GAIN).clamp(0.0, MAX_GAIN),
            )
        } else {
            // Use direct parameter values
            (
                (self.params.noise_reduction.value() * MAX_GAIN).clamp(0.0, MAX_GAIN),
                self.params.noise_tone.value(),
                (self.params.reverb_reduction.value() * MAX_GAIN).clamp(0.0, 1.0),
                (self.params.proximity.value() * MAX_GAIN).clamp(0.0, MAX_GAIN),
                (self.params.de_esser.value() * MAX_GAIN).clamp(0.0, MAX_GAIN),
                (self.params.leveler.value() * MAX_GAIN).clamp(0.0, MAX_GAIN),
            )
        };

        // Apply spectral control slew limiting (prevent warble/artifacts)
        let limited = self.control_limiters.process(
            raw_noise,
            raw_clarity,
            raw_de_ess,
            raw_reverb,
            raw_prox,
            whisper,
            noisy,
        );

        let noise_amt = limited.denoise;
        let clarity_amt = limited.clarity;
        let de_ess_amt = limited.deesser;
        let reverb_amt = limited.reverb;
        let prox_amt = limited.proximity;

        let output_gain_db = self.params.output_gain.value();
        let output_gain_lin = 10.0f32.powf(output_gain_db / 20.0);

        // Per-effect preview mode (radio button behavior - only one can be active)
        let preview_mode = if macro_mode {
            0 // Preview is advanced-only
        } else {
            let preview_denoise = self.params.preview_denoise.value();
            let preview_deverb = self.params.preview_deverb.value();
            let preview_deesser = self.params.preview_deesser.value();
            if preview_denoise {
                1 // Denoise preview
            } else if preview_deverb {
                2 // Deverb preview
            } else if preview_deesser {
                3 // De-esser preview
            } else {
                0 // No preview
            }
        };

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
            tone: noise_tone,
            sample_rate: self.sample_rate,
            use_ml: self.params.use_ml.value(),
        };

        // Peak decay rate: 13 dB/sec (typical for DAW meters)
        let decay_per_sample = 13.0 / self.sample_rate;
        let de_ess_alpha = 1.0 - (-1.0 / (DE_ESS_RMS_TAU_SEC * self.sample_rate)).exp();

        let channels = buffer.as_slice();
        if channels.len() < 2 {
            return ProcessStatus::Normal;
        }

        let (left_slice, right_slice) = channels.split_at_mut(1);
        let left = &mut left_slice[0];
        let right = &mut right_slice[0];
        let frame_count = left.len().min(right.len());

        let mut restoration_delta_energy = 0.0f32;

        for idx in 0..frame_count {
            let input_l = left[idx];
            let input_r = right[idx];

            let input_db_l = 20.0 * input_l.abs().max(1e-6).log10();
            let input_db_r = 20.0 * input_r.abs().max(1e-6).log10();
            self.peak_input_l = self.peak_input_l.max(input_db_l);
            self.peak_input_r = self.peak_input_r.max(input_db_r);

            // 0. INPUT PROFILE ANALYSIS (for data-driven calibration)
            // INVARIANT: Only pre-DSP samples are analyzed here
            // INVARIANT: This feeds condition detection and macro calibration
            self.input_profile_analyzer.process(input_l, input_r);

            // 0b. SPEECH CONFIDENCE (sidechain analysis - no audio modification)
            let sidechain = self.speech_confidence.process(input_l, input_r);

            // 1. EARLY REFLECTION SUPPRESSION (before denoise)
            // This handles short-lag reflections that make recordings sound "distant"
            let early_reflection_amt = if macro_mode {
                // In macro mode, early reflection is driven by distance macro
                self.params.macro_distance.value() * 0.8
            } else {
                // In advanced mode, use reverb reduction parameter
                reverb_amt * 0.5
            };

            let (pre_l, pre_r) = if bypass_restoration || early_reflection_amt < 0.001 {
                (input_l, input_r)
            } else {
                (
                    self.early_reflection_l
                        .process(input_l, early_reflection_amt, &sidechain),
                    self.early_reflection_r
                        .process(input_r, early_reflection_amt, &sidechain),
                )
            };

            // 2. SPEECH EXPANDER (after early reflection, before denoise)
            // Controls pauses and room swell without hard gating
            let expander_amt = if macro_mode {
                // In macro mode, expander is driven by distance macro
                self.params.macro_distance.value() * 0.6
            } else {
                // In advanced mode, disabled (could add dedicated parameter)
                0.0
            };

            let (exp_l, exp_r) = if expander_amt < 0.001 {
                (pre_l, pre_r)
            } else {
                self.speech_expander
                    .process(pre_l, pre_r, expander_amt, &sidechain)
            };

            // A. RESTORATION STAGE (denoise, de-verb)
            let (s1_l, s1_r) = if bypass_restoration {
                (exp_l, exp_r)
            } else {
                self.denoiser.process_sample(exp_l, exp_r, &denoise_cfg)
            };

            let s2_l = if bypass_restoration {
                s1_l
            } else {
                self.process_l.restoration_chain.safety_hpf.process(s1_l)
            };
            let s2_r = if bypass_restoration {
                s1_r
            } else {
                self.process_r.restoration_chain.safety_hpf.process(s1_r)
            };
            let s3_l = if bypass_restoration {
                s2_l
            } else {
                self.process_l.restoration_chain.deverber.process_sample(
                    s2_l,
                    total_deverb,
                    self.sample_rate,
                )
            };
            let s3_r = if bypass_restoration {
                s2_r
            } else {
                self.process_r.restoration_chain.deverber.process_sample(
                    s2_r,
                    total_deverb,
                    self.sample_rate,
                )
            };

            // Preview reference uses exp_l/exp_r (after early reflection + expander)
            // This ensures we capture ONLY what denoise removes, not earlier stages
            let denoise_ref_l = self
                .process_l
                .restoration_chain
                .preview_delay_denoise
                .push(exp_l);
            let denoise_ref_r = self
                .process_r
                .restoration_chain
                .preview_delay_denoise
                .push(exp_r);
            let deverb_ref_l = self
                .process_l
                .restoration_chain
                .preview_delay_deverb
                .push(s2_l);
            let deverb_ref_r = self
                .process_r
                .restoration_chain
                .preview_delay_deverb
                .push(s2_r);
            let denoise_cut_raw_l = if bypass_restoration {
                0.0
            } else if noise_amt > 0.001 {
                denoise_ref_l - s1_l
            } else {
                0.0
            };
            let denoise_cut_raw_r = if bypass_restoration {
                0.0
            } else if noise_amt > 0.001 {
                denoise_ref_r - s1_r
            } else {
                0.0
            };
            // Restoration delta is constructed so (restoration_output + delta) reconstructs
            // the time-aligned restoration input within float tolerance.
            let safety_cut_l = if bypass_restoration { 0.0 } else { s1_l - s2_l };
            let safety_cut_r = if bypass_restoration { 0.0 } else { s1_r - s2_r };
            let pre_deverb_delta_l = denoise_cut_raw_l + safety_cut_l;
            let pre_deverb_delta_r = denoise_cut_raw_r + safety_cut_r;
            let pre_deverb_delta_aligned_l = self
                .process_l
                .restoration_chain
                .preview_delay_post_deverb
                .push(pre_deverb_delta_l);
            let pre_deverb_delta_aligned_r = self
                .process_r
                .restoration_chain
                .preview_delay_post_deverb
                .push(pre_deverb_delta_r);
            let deverb_cut_l = if bypass_restoration {
                0.0
            } else if total_deverb > 0.001 {
                deverb_ref_l - s3_l
            } else {
                0.0
            };
            let deverb_cut_r = if bypass_restoration {
                0.0
            } else if total_deverb > 0.001 {
                deverb_ref_r - s3_r
            } else {
                0.0
            };
            let restoration_delta_l = if bypass_restoration {
                0.0
            } else if total_deverb > 0.001 {
                pre_deverb_delta_aligned_l + deverb_cut_l
            } else {
                pre_deverb_delta_l
            };
            let restoration_delta_r = if bypass_restoration {
                0.0
            } else if total_deverb > 0.001 {
                pre_deverb_delta_aligned_r + deverb_cut_r
            } else {
                pre_deverb_delta_r
            };
            restoration_delta_energy += 0.5
                * (restoration_delta_l * restoration_delta_l
                    + restoration_delta_r * restoration_delta_r);

            // B. SHAPING STAGE (proximity, clarity)
            let (s4_l, s4_r) = if bypass_shaping {
                (s3_l, s3_r)
            } else {
                (
                    self.process_l
                        .shaping_chain
                        .proximity
                        .process(s3_l, prox_amt),
                    self.process_r
                        .shaping_chain
                        .proximity
                        .process(s3_r, prox_amt),
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
                        prox_amt,
                        clarity_drive,
                    ),
                    self.process_r.shaping_chain.clarity.process(
                        s4_r,
                        clarity_amt,
                        prox_amt,
                        clarity_drive,
                    ),
                )
            };

            self.de_ess_rms_sq_l += (s5_l * s5_l - self.de_ess_rms_sq_l) * de_ess_alpha;
            self.de_ess_rms_sq_r += (s5_r * s5_r - self.de_ess_rms_sq_r) * de_ess_alpha;

            // C. DYNAMICS STAGE (de-esser, leveler, limiter)
            // De-esser with delta capture for preview
            let (s6_l, s6_r, deesser_cut_l, deesser_cut_r) = if bypass_dynamics {
                (s5_l, s5_r, 0.0, 0.0)
            } else {
                let de_ess_gain = self.linked_de_esser.compute_gain(s5_l, s5_r, de_ess_amt);
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
                // Capture what de-esser removes (subtractive delta)
                let cut_l = if de_ess_amt > 0.001 {
                    s5_l - out_l
                } else {
                    0.0
                };
                let cut_r = if de_ess_amt > 0.001 {
                    s5_r - out_r
                } else {
                    0.0
                };
                (out_l, out_r, cut_l, cut_r)
            };

            let (s7_l, s7_r) = if bypass_dynamics {
                (s6_l, s6_r)
            } else {
                let leveler_gain = self.linked_compressor.compute_gain(s6_l, s6_r, level_amt);
                (s6_l * leveler_gain, s6_r * leveler_gain)
            };

            // D. SPECTRAL GUARDRAILS (safety layer before limiter)
            // Prevents extreme settings from breaking sound
            let (s7g_l, s7g_r) = self.spectral_guardrails.process(s7_l, s7_r, macro_mode);

            let (s8_l, s8_r) = if bypass_dynamics {
                (s7g_l, s7g_r)
            } else {
                let limiter_gain = self.linked_limiter.compute_gain(s7g_l, s7g_r);
                (s7g_l * limiter_gain, s7g_r * limiter_gain)
            };

            // E. OUTPUT GAIN
            let s9_l = s8_l * output_gain_lin;
            let s9_r = s8_r * output_gain_lin;

            // Per-effect preview output
            // preview_mode: 0=none, 1=denoise, 2=deverb, 3=de-esser
            let (out_l, out_r) = match preview_mode {
                1 => {
                    // Denoise preview: hear only what denoise removes
                    (denoise_cut_raw_l, denoise_cut_raw_r)
                }
                2 => {
                    // Deverb preview: hear only what deverb removes
                    (deverb_cut_l, deverb_cut_r)
                }
                3 => {
                    // De-esser preview: hear only what de-esser removes
                    (deesser_cut_l, deesser_cut_r)
                }
                _ => {
                    // Normal output
                    (s9_l, s9_r)
                }
            };

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
        let _output_profile = self.output_profile_analyzer.get_profile();

        // Update macro controller with INPUT profile for calibration
        // INVARIANT: Control decisions come ONLY from input profile
        if macro_mode {
            self.macro_controller.update_input_profile(input_profile);
        }

        // Update DSP modules with profile-based adaptation
        // METRIC OWNERSHIP: Leveler owns RMS, crest factor, RMS variance
        self.linked_compressor
            .update_from_profile(input_profile.crest_factor_db, input_profile.rms_variance);

        let decay = decay_per_sample * frame_count as f32;
        self.peak_input_l = (self.peak_input_l - decay).max(-80.0);
        self.peak_input_r = (self.peak_input_r - decay).max(-80.0);
        self.peak_output_l = (self.peak_output_l - decay).max(-80.0);
        self.peak_output_r = (self.peak_output_r - decay).max(-80.0);

        let frame_count_f = frame_count.max(1) as f32;
        let restoration_rms = (restoration_delta_energy / frame_count_f).sqrt();
        let restoration_rms_db = 20.0 * (restoration_rms + 1e-8).log10();
        let delta_activity = if restoration_rms_db < -60.0 {
            0.0
        } else if restoration_rms_db < -30.0 {
            1.0
        } else {
            2.0
        };

        // Update meter values atomically (done once per buffer for efficiency)
        self.meters.set_input_peak_l(self.peak_input_l);
        self.meters.set_input_peak_r(self.peak_input_r);
        self.meters.set_output_peak_l(self.peak_output_l);
        self.meters.set_output_peak_r(self.peak_output_r);
        self.meters.set_restoration_delta_rms_db(restoration_rms_db);
        self.meters.set_delta_activity(delta_activity);

        // Get gain reduction from both channel compressors for true stereo metering
        let gr_db = if preview_mode != 0 {
            0.0 // Don't show gain reduction when previewing cuts
        } else {
            self.linked_compressor.get_gain_reduction_db()
        };
        self.meters.set_gain_reduction_l(gr_db);
        self.meters.set_gain_reduction_r(gr_db);

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
        self.meters
            .set_debug_limiter_gr_db(self.linked_limiter.get_gain_reduction_db());

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

        // =====================================================================
        // DEBUG LOGGING - rate-limited to ~1 Hz, writes to /tmp/voice_studio.log
        // =====================================================================
        self.debug_log_counter += 1;
        // Log every ~50 buffers (roughly 1 Hz at 48kHz with 1024-sample buffers)
        if self.debug_log_counter >= 50 {
            self.debug_log_counter = 0;
            // Write directly to log file (no env var needed)
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open("/tmp/voice_studio.log")
            {
                use std::io::Write;
                let _ = writeln!(
                    file,
                    "[DSP] mode={} | noise={:.2} deverb={:.2} prox={:.2} deess={:.2} level={:.2} | \
                     speech={:.2} | deess_gr={:.1}dB limit_gr={:.1}dB",
                    if macro_mode { "SIMPLE" } else { "ADVANCED" },
                    noise_amt,
                    total_deverb,
                    prox_amt,
                    de_ess_amt,
                    level_amt,
                    last_sidechain.speech_conf,
                    self.linked_de_esser.get_gain_reduction_db(),
                    self.linked_limiter.get_gain_reduction_db(),
                );
            }
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for VoiceStudioPlugin {
    const CLAP_ID: &'static str = "com.andrzej.voice-studio";
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
