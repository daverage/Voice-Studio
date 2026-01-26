//! Simple-mode Macro Controller
//!
//! Contract:
//! - Macros ONLY push advanced parameters
//! - No reverse mapping
//! - No state
//! - Safe at control/audio rate

use crate::VoiceParams;
use nih_plug::prelude::ParamSetter;

#[derive(Clone, Copy)]
pub struct SimpleMacroTargets {
    pub noise_reduction: f32,
    pub reverb_reduction: f32,
    pub proximity: f32,
    pub clarity: f32,
    pub de_esser: f32,
    pub noise_tone: f32,
    pub leveler: f32,
    pub breath_control: f32,
}

#[inline(always)]
fn lerp(x: f32, a: f32, b: f32) -> f32 {
    a + (b - a) * x.clamp(0.0, 1.0)
}

pub fn compute_simple_macro_targets(params: &VoiceParams) -> SimpleMacroTargets {
    let d = params.macro_distance.value();
    let c = params.macro_clarity.value();
    let k = params.macro_consistency.value();

    SimpleMacroTargets {
        noise_reduction: lerp(d, 0.00, 0.80),
        reverb_reduction: lerp(d, 0.00, 0.40),
        proximity: lerp(d, 0.00, 0.30),
        clarity: lerp(c, 0.00, 1.00),
        de_esser: lerp(c, 0.00, 0.70),
        noise_tone: lerp(c, 0.45, 0.60),
        leveler: lerp(k, 0.00, 0.80),
        breath_control: lerp(k, 0.00, 0.50),
    }
}

/// Apply Simple-mode macros to the advanced parameters.
/// This must be called ONLY when `macro_mode == true` from the GUI thread.
pub fn apply_simple_macros(params: &VoiceParams, setter: &ParamSetter<'_>) {
    let targets = compute_simple_macro_targets(params);

    setter.begin_set_parameter(&params.noise_reduction);
    setter.set_parameter(&params.noise_reduction, targets.noise_reduction);
    setter.end_set_parameter(&params.noise_reduction);

    setter.begin_set_parameter(&params.reverb_reduction);
    setter.set_parameter(&params.reverb_reduction, targets.reverb_reduction);
    setter.end_set_parameter(&params.reverb_reduction);

    setter.begin_set_parameter(&params.proximity);
    setter.set_parameter(&params.proximity, targets.proximity);
    setter.end_set_parameter(&params.proximity);

    setter.begin_set_parameter(&params.clarity);
    setter.set_parameter(&params.clarity, targets.clarity);
    setter.end_set_parameter(&params.clarity);

    setter.begin_set_parameter(&params.de_esser);
    setter.set_parameter(&params.de_esser, targets.de_esser);
    setter.end_set_parameter(&params.de_esser);

    setter.begin_set_parameter(&params.noise_tone);
    setter.set_parameter(&params.noise_tone, targets.noise_tone);
    setter.end_set_parameter(&params.noise_tone);

    setter.begin_set_parameter(&params.leveler);
    setter.set_parameter(&params.leveler, targets.leveler);
    setter.end_set_parameter(&params.leveler);

    setter.begin_set_parameter(&params.breath_control);
    setter.set_parameter(&params.breath_control, targets.breath_control);
    setter.end_set_parameter(&params.breath_control);
}

/// Estimate macro values from advanced parameters (approximate inverse mapping).
/// Used when loading presets to sync macro controls with advanced settings.
pub fn estimate_macros_from_advanced(
    noise_reduction: f32,
    reverb_reduction: f32,
    proximity: f32,
    clarity: f32,
    de_esser: f32,
    leveler: f32,
    breath_control: f32,
) -> (f32, f32, f32) {
    // Distance: average of noise_reduction (0-80%), reverb_reduction (0-40%), proximity (0-30%)
    // Normalized to 0-1 range
    let distance =
        ((noise_reduction / 0.80) + (reverb_reduction / 0.40) + (proximity / 0.30)) / 3.0;

    // Clarity: average of clarity (0-100%) and de_esser (0-70%)
    // Normalized to 0-1 range
    let clarity_macro = (clarity + (de_esser / 0.70)) / 2.0;

    // Consistency: average of leveler (0-80%) and breath_control (0-50%)
    // Normalized to 0-1 range
    let consistency = ((leveler / 0.80) + (breath_control / 0.50)) / 2.0;

    (
        distance.clamp(0.0, 1.0),
        clarity_macro.clamp(0.0, 1.0),
        consistency.clamp(0.0, 1.0),
    )
}
