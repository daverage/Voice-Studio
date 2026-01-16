mod dsp;
mod meters;
mod ui;

use dsp::{
    ChannelProcessor, ClarityDetector, DeEsserDetector, DenoiseConfig, LinkedCompressor,
    LinkedLimiter, StereoStreamingDenoiser,
};
use meters::Meters;
use nih_plug::prelude::*;
use nih_plug_vizia::{create_vizia_editor, ViziaState, ViziaTheming};
use nih_plug_vizia::vizia::prelude::ContextProxy;
use std::sync::{Arc, Mutex};
use ui::build_ui;

const AUTO_RMS_TAU_SEC: f32 = 0.02;
const DE_ESS_RMS_TAU_SEC: f32 = 0.01;

#[derive(Clone, Copy, Default)]
pub struct AutoSuggestValue {
    pub value: f32,
    pub confidence: f32,
}

#[derive(Clone, Copy, Default)]
pub struct AutoSuggestTargets {
    pub noise_reduction: AutoSuggestValue,
    pub reverb_reduction: AutoSuggestValue,
    pub de_esser: AutoSuggestValue,
    pub leveler: AutoSuggestValue,
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

    #[id = "preview_cuts"]
    pub preview_cuts: BoolParam,

    #[id = "preview_noise_reduction"]
    pub preview_noise_reduction: BoolParam,

    #[id = "preview_reverb_reduction"]
    pub preview_reverb_reduction: BoolParam,

    #[id = "preview_clarity"]
    pub preview_clarity: BoolParam,

    #[id = "preview_proximity"]
    pub preview_proximity: BoolParam,

    #[id = "preview_de_esser"]
    pub preview_de_esser: BoolParam,

    #[id = "preview_leveler"]
    pub preview_leveler: BoolParam,

    #[id = "preview_output_gain"]
    pub preview_output_gain: BoolParam,
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
    suggested_settings: Arc<Mutex<Option<AutoSuggestTargets>>>,
    ui_proxy: Arc<Mutex<Option<ContextProxy>>>,
    denoiser: StereoStreamingDenoiser,
    clarity_detector: ClarityDetector,
    linked_de_esser: DeEsserDetector,
    linked_compressor: LinkedCompressor,
    linked_limiter: LinkedLimiter,
    meters: Arc<Meters>,
    peak_input_l: f32,
    peak_input_r: f32,
    peak_output_l: f32,
    peak_output_r: f32,
    de_ess_rms_sq_l: f32,
    de_ess_rms_sq_r: f32,
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

                clarity: FloatParam::new(
                    "Clarity",
                    0.0,
                    FloatRange::Linear { min: 0.0, max: 1.0 },
                )
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

                preview_cuts: BoolParam::new("Preview Cuts", false),
                preview_noise_reduction: BoolParam::new("Preview Noise Reduction", true),
                preview_reverb_reduction: BoolParam::new("Preview De-Verb", true),
                preview_clarity: BoolParam::new("Preview Clarity", true),
                preview_proximity: BoolParam::new("Preview Proximity", true),
                preview_de_esser: BoolParam::new("Preview De-Esser", true),
                preview_leveler: BoolParam::new("Preview Leveler", true),
                preview_output_gain: BoolParam::new("Preview Output Gain", true),
            }),
            editor_state: ViziaState::new(|| (760, 480)),
            process_l: ChannelProcessor::new(2048, 512, 44100.0),
            process_r: ChannelProcessor::new(2048, 512, 44100.0),
            sample_rate: 44100.0,
            suggested_settings: Arc::new(Mutex::new(None)),
            ui_proxy: Arc::new(Mutex::new(None)),
            denoiser: StereoStreamingDenoiser::new(2048, 512),
            clarity_detector: ClarityDetector::new(44100.0),
            linked_de_esser: DeEsserDetector::new(44100.0),
            linked_compressor: LinkedCompressor::new(44100.0),
            linked_limiter: LinkedLimiter::new(44100.0),
            meters: Arc::new(Meters::new()),
            peak_input_l: 0.0,
            peak_input_r: 0.0,
            peak_output_l: 0.0,
            peak_output_r: 0.0,
            de_ess_rms_sq_l: 0.0,
            de_ess_rms_sq_r: 0.0,
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
        self.denoiser = StereoStreamingDenoiser::new(2048, 512);
        self.clarity_detector = ClarityDetector::new(self.sample_rate);
        self.linked_de_esser = DeEsserDetector::new(self.sample_rate);
        self.linked_compressor = LinkedCompressor::new(self.sample_rate);
        self.linked_limiter = LinkedLimiter::new(self.sample_rate);
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
        let suggested_settings = self.suggested_settings.clone();
        let ui_proxy = self.ui_proxy.clone();
        create_vizia_editor(
            self.editor_state.clone(),
            ViziaTheming::default(),
            move |cx, gui_context| {
                build_ui(
                    cx,
                    params.clone(),
                    meters.clone(),
                    suggested_settings.clone(),
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

        let noise_amt = (self.params.noise_reduction.value() * MAX_GAIN).clamp(0.0, MAX_GAIN);
        let noise_tone = self.params.noise_tone.value();
        // Deverber and clarity are mix parameters, so they should be clamped at 1.0
        let reverb_amt = (self.params.reverb_reduction.value() * MAX_GAIN).clamp(0.0, 1.0);
        let clarity_amt = (self.params.clarity.value() * MAX_GAIN).clamp(0.0, 1.0);
        let prox_amt = (self.params.proximity.value() * MAX_GAIN).clamp(0.0, MAX_GAIN);
        let de_ess_amt = (self.params.de_esser.value() * MAX_GAIN).clamp(0.0, MAX_GAIN);
        let level_amt = (self.params.leveler.value() * MAX_GAIN).clamp(0.0, MAX_GAIN);
        let output_gain_db = self.params.output_gain.value();
        let output_gain_lin = 10.0f32.powf(output_gain_db / 20.0);
        let preview_cuts = self.params.preview_cuts.value();
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

            // A. RESTORATION STAGE (denoise, de-verb)
            let (s1_l, s1_r) = if bypass_restoration {
                (input_l, input_r)
            } else {
                self.denoiser.process_sample(input_l, input_r, &denoise_cfg)
            };

            let s2_l = if bypass_restoration {
                s1_l
            } else {
                self.process_l
                    .restoration_chain
                    .safety_hpf
                    .process(s1_l)
            };
            let s2_r = if bypass_restoration {
                s1_r
            } else {
                self.process_r
                    .restoration_chain
                    .safety_hpf
                    .process(s1_r)
            };
            let s3_l = if bypass_restoration {
                s2_l
            } else {
                self.process_l
                    .restoration_chain
                    .deverber
                    .process_sample(s2_l, total_deverb, self.sample_rate)
            };
            let s3_r = if bypass_restoration {
                s2_r
            } else {
                self.process_r
                    .restoration_chain
                    .deverber
                    .process_sample(s2_r, total_deverb, self.sample_rate)
            };

            let denoise_ref_l = self
                .process_l
                .restoration_chain
                .preview_delay_denoise
                .push(input_l);
            let denoise_ref_r = self
                .process_r
                .restoration_chain
                .preview_delay_denoise
                .push(input_r);
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
            restoration_delta_energy +=
                0.5 * (restoration_delta_l * restoration_delta_l
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
                    self.process_l
                        .shaping_chain
                        .clarity
                        .process(s4_l, clarity_amt, prox_amt, clarity_drive),
                    self.process_r
                        .shaping_chain
                        .clarity
                        .process(s4_r, clarity_amt, prox_amt, clarity_drive),
                )
            };

            self.de_ess_rms_sq_l += (s5_l * s5_l - self.de_ess_rms_sq_l) * de_ess_alpha;
            self.de_ess_rms_sq_r += (s5_r * s5_r - self.de_ess_rms_sq_r) * de_ess_alpha;
            // Detector uses stereo inputs directly
            // C. DYNAMICS STAGE (de-esser, leveler, limiter)
            let (s6_l, s6_r) = if bypass_dynamics {
                (s5_l, s5_r)
            } else {
                let de_ess_gain = self.linked_de_esser.compute_gain(s5_l, s5_r, de_ess_amt);
                (
                    self.process_l
                        .dynamics_chain
                        .de_esser_band
                        .apply(s5_l, de_ess_gain),
                    self.process_r
                        .dynamics_chain
                        .de_esser_band
                        .apply(s5_r, de_ess_gain),
                )
            };

            let (s7_l, s7_r) = if bypass_dynamics {
                (s6_l, s6_r)
            } else {
                let leveler_gain = self
                    .linked_compressor
                    .compute_gain(s6_l, s6_r, level_amt);
                (s6_l * leveler_gain, s6_r * leveler_gain)
            };

            let (s8_l, s8_r) = if bypass_dynamics {
                (s7_l, s7_r)
            } else {
                let limiter_gain = self.linked_limiter.compute_gain(s7_l, s7_r);
                (s7_l * limiter_gain, s7_r * limiter_gain)
            };

            // C. OUTPUT GAIN
            let s9_l = s8_l * output_gain_lin;
            let s9_r = s8_r * output_gain_lin;

            if preview_cuts {
                let output_db_l = 20.0 * restoration_delta_l.abs().max(1e-6).log10();
                let output_db_r = 20.0 * restoration_delta_r.abs().max(1e-6).log10();
                self.peak_output_l = self.peak_output_l.max(output_db_l);
                self.peak_output_r = self.peak_output_r.max(output_db_r);

                left[idx] = restoration_delta_l;
                right[idx] = restoration_delta_r;
                continue;
            }

            let output_db_l = 20.0 * s9_l.abs().max(1e-6).log10();
            let output_db_r = 20.0 * s9_r.abs().max(1e-6).log10();
            self.peak_output_l = self.peak_output_l.max(output_db_l);
            self.peak_output_r = self.peak_output_r.max(output_db_r);

            left[idx] = s9_l;
            right[idx] = s9_r;
        }

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
        self.meters
            .set_restoration_delta_rms_db(restoration_rms_db);
        self.meters.set_delta_activity(delta_activity);

        // Get gain reduction from both channel compressors for true stereo metering
        let gr_db = if preview_cuts {
            0.0
        } else {
            self.linked_compressor.get_gain_reduction_db()
        };
        self.meters.set_gain_reduction_l(gr_db);
        self.meters.set_gain_reduction_r(gr_db);

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
