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
                noise_reduction: 0.35,
                reverb_reduction: 0.60,
                proximity: 0.05,
                clarity: 0.15,
                de_esser: 0.0,
                leveler: 0.70,
                breath_control: 0.30,
                macro_clean: 0.60,
                macro_enhance: 0.55,
                macro_control: 0.45,
            }),
            DspPreset::VoiceoverStudio => Some(DspPresetValues {
                noise_reduction: 0.20,
                reverb_reduction: 0.40,
                proximity: 0.10,
                clarity: 0.20,
                de_esser: 0.15,
                leveler: 0.60,
                breath_control: 0.25,
                macro_clean: 0.85,
                macro_enhance: 0.75,
                macro_control: 0.80,
            }),
            DspPreset::InterviewOutdoor => Some(DspPresetValues {
                noise_reduction: 0.55,
                reverb_reduction: 0.75,
                proximity: 0.0,
                clarity: 0.10,
                de_esser: 0.10,
                leveler: 0.75,
                breath_control: 0.40,
                macro_clean: 0.90,
                macro_enhance: 0.40,
                macro_control: 0.60,
            }),
            DspPreset::BroadcastClean => Some(DspPresetValues {
                noise_reduction: 0.10,
                reverb_reduction: 0.25,
                proximity: 0.15,
                clarity: 0.25,
                de_esser: 0.20,
                leveler: 0.50,
                breath_control: 0.15,
                macro_clean: 0.35,
                macro_enhance: 0.25,
                macro_control: 0.20,
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
    pub reverb_reduction: f32,
    pub proximity: f32,
    pub clarity: f32,
    pub de_esser: f32,
    pub leveler: f32,
    pub breath_control: f32,
    pub macro_clean: f32,
    pub macro_enhance: f32,
    pub macro_control: f32,
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
