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
    pub leveler: f32,
    pub breath_control: f32,
    pub rumble: f32,
    pub hiss: f32,
}

#[inline(always)]
fn lerp(x: f32, a: f32, b: f32) -> f32 {
    a + (b - a) * x.clamp(0.0, 1.0)
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn compute_simple_macro_targets(params: &VoiceParams) -> SimpleMacroTargets {
    let x_clean = params.macro_clean.value();
    let x_enhance = params.macro_enhance.value();
    let x_control = params.macro_control.value();

    // --- CLEAN macro mappings ---
    // Rumble: 20 -> 60 Hz. Norm: (60-20)/(120-20) = 0.4
    let rumble_norm = ((x_clean - 0.1) / 0.5).clamp(0.0, 1.0);
    let rumble_param = rumble_norm * rumble_norm * 0.4;

    // Hiss: 0 -> -6 dB. Norm: -6/-24 = 0.25
    let hiss_norm = ((x_clean - 0.35) / 0.45).clamp(0.0, 1.0);
    let hiss_param = hiss_norm * 0.25;

    // Main Denoiser: 0 -> 70%
    let denoise_amt = ((x_clean - 0.45) / 0.55).clamp(0.0, 1.0) * 0.7;

    // --- ENHANCE macro mappings ---
    // Proximity: smooth ramp
    let proximity = smoothstep(0.0, 0.6, x_enhance);
    // Clarity: capped at 40%
    let clarity = smoothstep(0.3, 0.9, x_enhance) * 0.4;

    // --- CONTROL macro mappings ---
    // De-esser
    let deesser = smoothstep(0.2, 0.7, x_control);
    // Leveler
    let leveler = smoothstep(0.0, 1.0, x_control);

    SimpleMacroTargets {
        noise_reduction: denoise_amt,
        reverb_reduction: 0.0, 
        proximity,
        clarity,
        de_esser: deesser,
        leveler,
        breath_control: lerp(x_control, 0.0, 0.5),
        rumble: rumble_param,
        hiss: hiss_param,
    }
}

/// Apply Simple-mode macros to the advanced parameters.
/// This must be called ONLY when `macro_mode == true` from the GUI thread.
pub fn apply_simple_macros(params: &VoiceParams, setter: &ParamSetter<'_>) {
    let x_clean = params.macro_clean.value();
    let x_enhance = params.macro_enhance.value();
    let x_control = params.macro_control.value();

    // 1. CLEAN mappings
    // Rumble: 20 -> 60 Hz. Norm: (60-20)/(120-20) = 0.4
    let rumble_norm = ((x_clean - 0.1) / 0.5).clamp(0.0, 1.0);
    let rumble_param = rumble_norm * rumble_norm * 0.4;
    setter.begin_set_parameter(&params.rumble_amount);
    setter.set_parameter(&params.rumble_amount, rumble_param);
    setter.end_set_parameter(&params.rumble_amount);

    // Hiss: 0 -> -6 dB. Norm: -6/-24 = 0.25
    let hiss_norm = ((x_clean - 0.35) / 0.45).clamp(0.0, 1.0);
    let hiss_param = hiss_norm * 0.25;
    setter.begin_set_parameter(&params.hiss_amount);
    setter.set_parameter(&params.hiss_amount, hiss_param);
    setter.end_set_parameter(&params.hiss_amount);

    // Static Noise: 0 -> 100%
    let static_noise_amt = ((x_clean - 0.6) / 0.4).clamp(0.0, 1.0);
    setter.begin_set_parameter(&params.noise_learn_amount);
    setter.set_parameter(&params.noise_learn_amount, static_noise_amt);
    setter.end_set_parameter(&params.noise_learn_amount);

    // Denoiser
    let denoise_amt = ((x_clean - 0.45) / 0.55).clamp(0.0, 1.0) * 0.7;
    setter.begin_set_parameter(&params.noise_reduction);
    setter.set_parameter(&params.noise_reduction, denoise_amt);
    setter.end_set_parameter(&params.noise_reduction);

    // 2. ENHANCE mappings
    let proximity = smoothstep(0.0, 0.6, x_enhance);
    setter.begin_set_parameter(&params.proximity);
    setter.set_parameter(&params.proximity, proximity);
    setter.end_set_parameter(&params.proximity);

    let clarity = smoothstep(0.3, 0.9, x_enhance) * 0.4;
    setter.begin_set_parameter(&params.clarity);
    setter.set_parameter(&params.clarity, clarity);
    setter.end_set_parameter(&params.clarity);

    // 3. CONTROL mappings
    let deesser = smoothstep(0.2, 0.7, x_control);
    setter.begin_set_parameter(&params.de_esser);
    setter.set_parameter(&params.de_esser, deesser);
    setter.end_set_parameter(&params.de_esser);

    let leveler = smoothstep(0.0, 1.0, x_control);
    setter.begin_set_parameter(&params.leveler);
    setter.set_parameter(&params.leveler, leveler);
    setter.end_set_parameter(&params.leveler);
}
