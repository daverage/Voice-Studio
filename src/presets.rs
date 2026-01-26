use nih_plug::prelude::Enum;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// =============================================================================
// DSP FACTORY PRESETS
// =============================================================================

/// Factory presets for common DSP scenarios.
/// Values derived from Bayesian optimization against professional reference audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Enum)]
#[repr(usize)]
pub enum DspPreset {
    #[serde(rename = "Manual")]
    #[name = "Manual"]
    Manual,
    #[serde(rename = "Podcast (Noisy Room)")]
    #[name = "Podcast (Noisy Room)"]
    PodcastNoisy,
    #[serde(rename = "Voiceover (Studio)")]
    #[name = "Voiceover (Studio)"]
    VoiceoverStudio,
    #[serde(rename = "Interview (Outdoor)")]
    #[name = "Interview (Outdoor)"]
    InterviewOutdoor,
    #[serde(rename = "Broadcast (Clean)")]
    #[name = "Broadcast (Clean)"]
    BroadcastClean,
}

impl DspPreset {
    pub fn name(&self) -> &'static str {
        match self {
            DspPreset::Manual => "Manual",
            DspPreset::PodcastNoisy => "Podcast (Noisy Room)",
            DspPreset::VoiceoverStudio => "Voiceover (Studio)",
            DspPreset::InterviewOutdoor => "Interview (Outdoor)",
            DspPreset::BroadcastClean => "Broadcast (Clean)",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            DspPreset::Manual => "Custom settings - no preset applied",
            DspPreset::PodcastNoisy => "Optimized for podcasts recorded in noisy environments",
            DspPreset::VoiceoverStudio => "Balanced settings for studio voiceover work",
            DspPreset::InterviewOutdoor => "Aggressive cleanup for outdoor/field recordings",
            DspPreset::BroadcastClean => "Minimal processing for professional broadcast audio",
        }
    }

    /// Get preset parameter values (noise_reduction, reverb_reduction, proximity, clarity, de_esser, leveler, breath_control)
    pub fn get_values(&self) -> Option<DspPresetValues> {
        match self {
            DspPreset::Manual => None,
            DspPreset::PodcastNoisy => Some(DspPresetValues {
                // Based on optimization trial 0 (score: 0.5908, STOI: 96.7%)
                noise_reduction: 0.35,
                noise_mode: NoiseMode::Normal,
                reverb_reduction: 0.60,
                proximity: 0.05,
                clarity: 0.15,
                de_esser: 0.0,
                leveler: 0.70,
                breath_control: 0.30,
            }),
            DspPreset::VoiceoverStudio => Some(DspPresetValues {
                // Lighter settings for studio - minimal noise, natural proximity
                noise_reduction: 0.20,
                noise_mode: NoiseMode::Normal,
                reverb_reduction: 0.40,
                proximity: 0.10,
                clarity: 0.20,
                de_esser: 0.15,
                leveler: 0.60,
                breath_control: 0.25,
            }),
            DspPreset::InterviewOutdoor => Some(DspPresetValues {
                // Aggressive cleanup for outdoor/field recordings
                noise_reduction: 0.55,
                noise_mode: NoiseMode::Aggressive,
                reverb_reduction: 0.75,
                proximity: 0.0,
                clarity: 0.10,
                de_esser: 0.10,
                leveler: 0.75,
                breath_control: 0.40,
            }),
            DspPreset::BroadcastClean => Some(DspPresetValues {
                // Minimal processing for already-good audio
                noise_reduction: 0.10,
                noise_mode: NoiseMode::Normal,
                reverb_reduction: 0.25,
                proximity: 0.15,
                clarity: 0.25,
                de_esser: 0.20,
                leveler: 0.50,
                breath_control: 0.15,
            }),
        }
    }
}

impl Default for DspPreset {
    fn default() -> Self {
        DspPreset::Manual
    }
}

/// Parameter values for a DSP preset
#[derive(Debug, Clone, Copy)]
pub struct DspPresetValues {
    pub noise_reduction: f32,
    pub noise_mode: NoiseMode,
    pub reverb_reduction: f32,
    pub proximity: f32,
    pub clarity: f32,
    pub de_esser: f32,
    pub leveler: f32,
    pub breath_control: f32,
}

// =============================================================================
// NOISE MODE
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Enum)]
#[repr(usize)]
pub enum NoiseMode {
    #[serde(rename = "Normal")]
    #[name = "Normal"]
    Normal,
    #[serde(rename = "Aggressive")]
    #[name = "Aggressive"]
    Aggressive,
}

impl NoiseMode {
    pub fn name(&self) -> &'static str {
        match self {
            NoiseMode::Normal => "Normal",
            NoiseMode::Aggressive => "Aggressive",
        }
    }
}

impl Default for NoiseMode {
    fn default() -> Self {
        NoiseMode::Normal
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Enum)]
#[repr(usize)]
pub enum OutputPreset {
    #[serde(rename = "None")]
    #[name = "None"]
    None,
    #[serde(rename = "Broadcast")]
    #[name = "Broadcast"]
    Broadcast,
    #[serde(rename = "YouTube")]
    #[name = "YouTube"]
    YouTube,
    #[serde(rename = "Spotify")]
    #[name = "Spotify"]
    Spotify,
}

impl OutputPreset {
    pub fn all_presets() -> [OutputPreset; 4] {
        [
            OutputPreset::None,
            OutputPreset::Broadcast,
            OutputPreset::YouTube,
            OutputPreset::Spotify,
        ]
    }

    pub fn name(&self) -> &'static str {
        match self {
            OutputPreset::None => "None",
            OutputPreset::Broadcast => "Broadcast",
            OutputPreset::YouTube => "YouTube",
            OutputPreset::Spotify => "Spotify",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            OutputPreset::None => "No loudness enforcement",
            OutputPreset::Broadcast => "Optimized for broadcast standards",
            OutputPreset::YouTube => "Optimized for streaming platforms",
            OutputPreset::Spotify => "Optimized for music streaming",
        }
    }

    pub fn get_lufs_target(&self) -> Option<f32> {
        match self {
            OutputPreset::None => None,
            OutputPreset::Broadcast => Some(-23.0),
            OutputPreset::YouTube => Some(-14.0),
            OutputPreset::Spotify => Some(-14.0),
        }
    }

    pub fn get_true_peak_ceiling(&self) -> Option<f32> {
        match self {
            OutputPreset::None => None,
            OutputPreset::Broadcast => Some(-1.0),
            OutputPreset::YouTube => Some(-1.0),
            OutputPreset::Spotify => Some(-1.0),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetValues {
    pub integrated_loudness: Option<f32>,
    pub true_peak_ceiling: Option<f32>,
}

#[derive(Debug)]
pub struct PresetManager {
    presets: HashMap<String, PresetValues>,
}

impl PresetManager {
    /// Load presets from baked-in JSON. This is fallible but non-fatal.
    /// Returns a default manager with a "None" preset if parsing fails.
    pub fn new() -> Self {
        let presets_str = include_str!("../presets.json");
        match serde_json::from_str::<HashMap<String, PresetValues>>(presets_str) {
            Ok(presets) => Self { presets },
            Err(_) => {
                // Return a minimal safe default if the baked-in JSON is somehow malformed
                Self::default()
            }
        }
    }

    /// Returns an empty PresetManager with only the "None" preset.
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn get_preset_values(&self, preset_name: &str) -> Option<&PresetValues> {
        self.presets.get(preset_name)
    }

    pub fn get_lufs_target(&self, preset: OutputPreset) -> Option<f32> {
        self.get_preset_values(preset.name())
            .and_then(|values| values.integrated_loudness)
    }

    pub fn get_true_peak_ceiling(&self, preset: OutputPreset) -> Option<f32> {
        self.get_preset_values(preset.name())
            .and_then(|values| values.true_peak_ceiling)
    }
}

impl Default for PresetManager {
    fn default() -> Self {
        // Fallback to None preset if loading fails
        let mut presets = HashMap::new();
        presets.insert(
            "None".to_string(),
            PresetValues {
                integrated_loudness: None,
                true_peak_ceiling: None,
            },
        );
        Self { presets }
    }
}
