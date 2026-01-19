//! Macro Controller (Parameter Orchestrator)
//!
//! Drives existing DSP parameters using intent-based macros and data-driven calibration.
//! This module provides three macro controls:
//!
//! - **Distance**: Makes voice sound closer (controls reverb_reduction, proximity)
//! - **Clarity**: Improves intelligibility (controls noise_reduction, de_esser, noise_tone)
//! - **Consistency**: Evens out volume (controls leveler)
//!
//! ## Priority & Interaction Rules
//!
//! When macros affect overlapping perceptual regions, the following priorities apply:
//!
//! 1. **Restoration First**: Noise and reverb reduction (driven by Clarity/Distance) happen before shaping.
//!    This prevents enhancing noise.
//! 2. **Frequency Bands**:
//!    - **Low End**: Proximity (Distance) boosts <300Hz.
//!    - **Presence**: Clarity boosts 2-5kHz.
//!    - **Air**: Clarity boosts >8kHz.
//!    - **Sibilance**: De-esser (Clarity) cuts 4-10kHz dynamically.
//! 3. **Conflict Resolution**:
//!    - If Clarity wants to boost HF (Presence/Air) but De-esser wants to cut HF (Sibilance),
//!      the De-esser takes precedence dynamically (it sits later in the chain).
//!
//! ## Data-Driven Calibration (Task 5)
//!
//! The macro controller now uses distance-to-target calculations:
//! - Each owned metric has a target range from TargetProfile
//! - Parameter strength is proportional to distance from target
//! - Clean audio rule: if input is fully within target, macros resolve to near-zero
//!
//! ## Metric Ownership (Task 3)
//!
//! | Metric          | Owning Module        |
//! |-----------------|----------------------|
//! | RMS             | Leveler              |
//! | Crest factor    | Leveler              |
//! | RMS variance    | Leveler              |
//! | Noise floor     | Denoiser             |
//! | SNR             | Denoiser             |
//! | Early/Late      | De-reverb            |
//! | Decay slope     | De-reverb            |
//! | Presence/Air    | Proximity + Clarity  |
//! | HF variance     | Guardrail (read-only)|
//!
//! ## Important
//! - Macros ONLY write parameters, they never touch audio buffers
//! - When macro mode is active, advanced controls should be read-only
//! - If an advanced control is touched, macro mode automatically disables

use crate::dsp::utils::smoothstep;
use crate::{AudioProfile, DetectedConditions, TargetProfile};

// =============================================================================
// Constants - Mapping Curves
// =============================================================================

// Distance macro constants
// All MIN values are 0.0 so macro=0 always maps to advanced=0
const DISTANCE_REVERB_MIN: f32 = 0.0;
const DISTANCE_REVERB_MAX: f32 = 0.85;
const DISTANCE_PROXIMITY_MIN: f32 = 0.0;
const DISTANCE_PROXIMITY_MAX: f32 = 0.7;

// Clarity macro constants
// All MIN values are 0.0 so macro=0 always maps to advanced=0
const CLARITY_NOISE_MIN: f32 = 0.0;
const CLARITY_NOISE_MAX: f32 = 0.75;
const CLARITY_DEESS_MIN: f32 = 0.0;
const CLARITY_DEESS_MAX: f32 = 0.6;
const CLARITY_TONE_CENTER: f32 = 0.5; // Neutral
const CLARITY_TONE_RANGE: f32 = 0.15; // Max deviation from center

// Consistency macro constants
const CONSISTENCY_LEVELER_MIN: f32 = 0.0;
const CONSISTENCY_LEVELER_MAX: f32 = 0.8;

// Approx ~100–150 ms convergence at 48 kHz with typical buffer sizes

const MACRO_SMOOTH_RATE: f32 = 0.002;

// =============================================================================
// Macro State
// =============================================================================

/// Current state of macro controls
#[derive(Clone, Copy, Debug, Default)]
pub struct MacroState {
    /// Distance control (0.0 = far, 1.0 = close)
    pub distance: f32,
    /// Clarity control (0.0 = natural, 1.0 = maximum clarity)
    pub clarity: f32,
    /// Consistency control (0.0 = dynamic, 1.0 = very even)
    pub consistency: f32,
    /// Whether macro mode is active
    pub active: bool,
}

/// Output of macro mapping - target values for DSP parameters
#[derive(Clone, Copy, Debug, Default)]
pub struct MacroTargets {
    pub noise_reduction: f32,
    pub noise_tone: f32,
    pub reverb_reduction: f32,
    pub proximity: f32,
    pub de_esser: f32,
    pub leveler: f32,
}

/// Calibration factors computed from distance-to-target metrics
/// These scale the macro outputs based on measured signal properties
#[derive(Clone, Copy, Debug, Default)]
pub struct CalibrationFactors {
    /// Scale for noise reduction (0-1, based on SNR distance from target)
    pub noise_scale: f32,

    /// Scale for reverb reduction (0-1, based on Early/Late ratio distance)
    pub reverb_scale: f32,

    /// Scale for proximity (0-1, based on Early/Late ratio for distant detection)
    pub proximity_scale: f32,

    /// Scale for clarity (0-1, based on presence ratio distance)
    pub clarity_scale: f32,

    /// Scale for de-esser (0-1, based on HF variance and conditions)
    pub deesser_scale: f32,

    /// Scale for leveler (0-1, based on RMS variance distance)
    pub leveler_scale: f32,

    /// Overall attenuation for clean audio (0 = no processing, 1 = full processing)
    pub clean_audio_attenuation: f32,
}

// =============================================================================
// Task 6: Explainability / Debug Info (developer-only, zero cost when unused)
// =============================================================================

/// Reason why a scale factor is at its current value
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScaleReason {
    /// Metric is within target range, no processing needed
    AtTarget,
    /// Metric is outside target, scale proportional to distance
    DistanceProportional,
    /// Scale is in soft-landing zone near target
    SoftLanding,
    /// Scale was capped due to whisper detection
    CappedWhisper,
    /// Scale was capped due to noisy environment
    CappedNoisy,
    /// Scale was capped due to whisper + noisy combined
    CappedWhisperNoisy,
    /// Scale was capped due to low crest factor
    CappedCrestFactor,
    /// Clean audio rule: processing disabled
    CleanAudio,
    /// Distant mic condition triggered proximity
    DistantMicDetected,
}

impl Default for ScaleReason {
    fn default() -> Self {
        Self::AtTarget
    }
}

/// Distance-to-target for each metric (negative = below target, positive = above, 0 = in range)
#[derive(Clone, Copy, Debug, Default)]
pub struct MetricDistances {
    /// SNR distance from target minimum (dB)
    pub snr_db: f32,
    /// Early/Late ratio distance from target minimum
    pub early_late_ratio: f32,
    /// Presence ratio distance from target maximum
    pub presence_ratio: f32,
    /// HF variance distance from target maximum
    pub hf_variance: f32,
    /// RMS variance distance from target maximum
    pub rms_variance: f32,
}

/// Debug info explaining why each scale factor is at its current value
/// This struct is only computed when explicitly requested (zero cost otherwise)
#[derive(Clone, Copy, Debug, Default)]
pub struct CalibrationDebugInfo {
    /// Distance-to-target for each metric
    pub distances: MetricDistances,

    /// Reason for noise_scale value
    pub noise_reason: ScaleReason,
    /// Reason for reverb_scale value
    pub reverb_reason: ScaleReason,
    /// Reason for proximity_scale value
    pub proximity_reason: ScaleReason,
    /// Reason for clarity_scale value
    pub clarity_reason: ScaleReason,
    /// Reason for deesser_scale value
    pub deesser_reason: ScaleReason,
    /// Reason for leveler_scale value
    pub leveler_reason: ScaleReason,

    /// Which condition caps were triggered
    pub caps_triggered: CapsTriggered,
}

/// Bit flags for which caps were triggered during calibration
#[derive(Clone, Copy, Debug, Default)]
pub struct CapsTriggered {
    pub whisper: bool,
    pub noisy: bool,
    pub whisper_noisy_combined: bool,
    pub crest_factor: bool,
    pub clean_audio: bool,
    pub distant_mic: bool,
}

impl CalibrationFactors {
    /// Smoothly interpolate toward target factors
    /// smooth_coeff: 0.0 = instant, 1.0 = never change
    fn smooth_toward(&mut self, target: &CalibrationFactors, smooth_coeff: f32) {
        let blend = 1.0 - smooth_coeff;
        self.noise_scale += (target.noise_scale - self.noise_scale) * blend;
        self.reverb_scale += (target.reverb_scale - self.reverb_scale) * blend;
        self.proximity_scale += (target.proximity_scale - self.proximity_scale) * blend;
        self.clarity_scale += (target.clarity_scale - self.clarity_scale) * blend;
        self.deesser_scale += (target.deesser_scale - self.deesser_scale) * blend;
        self.leveler_scale += (target.leveler_scale - self.leveler_scale) * blend;
        self.clean_audio_attenuation +=
            (target.clean_audio_attenuation - self.clean_audio_attenuation) * blend;
    }
}

/// Smoothing coefficient for calibration factor transitions
/// Higher = slower transitions (0.95 = ~20 buffers to converge)
const CALIBRATION_SMOOTH_COEFF: f32 = 0.92;

/// Hysteresis margin for clean audio detection (prevents rapid switching)
const CLEAN_AUDIO_HYSTERESIS: f32 = 0.1;

// =============================================================================
// Distant Detection Hysteresis (Task 2.2)
// =============================================================================

/// Distant detection entry threshold for early/late ratio
const DISTANT_ENTER_EARLY_LATE: f32 = 0.05;
/// Distant detection exit threshold for early/late ratio (higher = harder to exit)
const DISTANT_EXIT_EARLY_LATE: f32 = 0.10;

/// Distant detection entry threshold for decay slope
const DISTANT_ENTER_DECAY_SLOPE: f32 = -0.0005;
/// Distant detection exit threshold for decay slope (less negative = harder to exit)
const DISTANT_EXIT_DECAY_SLOPE: f32 = -0.0002;

/// Hold time in milliseconds (prevents flicker after brief improvements)
const DISTANT_HOLD_MS: f32 = 300.0;

/// Frames per buffer estimate (used for hold counter)
const FRAMES_PER_BUFFER_EST: usize = 512;

/// Compute asymptotic approach factor for "soft landing" near targets
/// Returns 1.0 far from target, approaches 0.0 as distance approaches 0
#[inline]
fn soft_landing(distance: f32, threshold: f32) -> f32 {
    if distance <= 0.0 {
        return 0.0; // Already at or past target
    }
    if distance >= threshold {
        return 1.0; // Far from target, full strength
    }
    // Smooth cubic ease-out for soft landing
    let t = distance / threshold;
    t * t * (3.0 - 2.0 * t)
}

// =============================================================================
// Macro Controller
// =============================================================================

/// Main macro controller that maps macro values to DSP parameters
pub struct MacroController {
    state: MacroState,
    targets: MacroTargets,
    smoothed: MacroTargets,

    // Data-driven calibration state
    target_profile: TargetProfile,
    input_profile: AudioProfile,
    conditions: DetectedConditions,

    // Calibration factors (computed from distance-to-target)
    // Uses smoothing for stable, non-jumpy transitions
    calibration: CalibrationFactors,
    target_calibration: CalibrationFactors,

    // Hysteresis state for clean audio detection
    was_clean_audio: bool,

    // Hysteresis state for distant detection (prevents flicker)
    was_distant: bool,
    distant_hold_counter: usize,
}

impl MacroController {
    pub fn new() -> Self {
        let initial_calibration = CalibrationFactors {
            noise_scale: 1.0,
            reverb_scale: 1.0,
            proximity_scale: 1.0,
            clarity_scale: 1.0,
            deesser_scale: 1.0,
            leveler_scale: 1.0,
            clean_audio_attenuation: 1.0,
        };

        Self {
            state: MacroState::default(),
            targets: MacroTargets::default(),
            smoothed: MacroTargets::default(),
            target_profile: TargetProfile::PROFESSIONAL_VO,
            input_profile: AudioProfile::default(),
            conditions: DetectedConditions::default(),
            calibration: initial_calibration,
            target_calibration: initial_calibration,
            was_clean_audio: false,
            was_distant: false,
            distant_hold_counter: 0,
        }
    }

    /// Update the input profile and recalculate calibration factors
    /// Call this once per buffer with the pre-DSP audio profile
    ///
    /// Uses smooth transitions to prevent jumpy behavior when conditions change.
    /// Applies hysteresis to clean-audio and distant detection to prevent rapid switching.
    pub fn update_input_profile(&mut self, profile: AudioProfile) {
        self.input_profile = profile;
        self.conditions = DetectedConditions::detect(&profile);

        // Apply hysteresis to distant detection (Task 2.2)
        // Entry is easier than exit to prevent flicker
        let distant_with_hysteresis = self.compute_distant_with_hysteresis(&profile);
        self.conditions.distant_mic = distant_with_hysteresis;

        // Compute target calibration factors (what we're moving toward)
        self.compute_target_calibration();

        // Smoothly interpolate current calibration toward target
        // This prevents jumpy transitions when conditions change
        self.calibration
            .smooth_toward(&self.target_calibration, CALIBRATION_SMOOTH_COEFF);

        // Update macro targets with smoothed calibration
        if self.state.active {
            self.compute_targets();
        }
    }

    /// Compute target calibration factors based on distance-to-target metrics
    /// These are the values we smoothly transition toward
    fn compute_target_calibration(&mut self) {
        let target = &self.target_profile;
        let input = &self.input_profile;
        let cond = &self.conditions;

        // CLEAN AUDIO RULE with HYSTERESIS (mandatory):
        // If input is fully within target, all macros resolve to near-zero
        // Hysteresis prevents rapid switching near the boundary
        let is_clean = cond.clean_audio || input.is_within_target(target);

        // Apply hysteresis: require stronger signal to exit clean state
        let use_clean_mode = if self.was_clean_audio {
            // Currently in clean mode: require significant deviation to exit
            // This prevents oscillation at the boundary
            is_clean || self.calibration.clean_audio_attenuation < (0.1 + CLEAN_AUDIO_HYSTERESIS)
        } else {
            // Currently in processing mode: enter clean mode if conditions met
            is_clean
        };

        self.was_clean_audio = use_clean_mode;

        if use_clean_mode {
            self.target_calibration = CalibrationFactors {
                noise_scale: 0.05,            // Minimal cleaning only
                reverb_scale: 0.0,            // No reverb reduction needed
                proximity_scale: 0.0,         // No proximity boost needed
                clarity_scale: 0.05,          // Minimal clarity only
                deesser_scale: 0.0,           // No de-essing needed
                leveler_scale: 0.1,           // Minimal leveling allowed
                clean_audio_attenuation: 0.1, // 90% reduction
            };
            return;
        }

        // Noise scale: based on distance from target SNR
        // Uses soft landing to prevent overshoot near target
        // Task 4: Scale goes to 0 when target is reached (no floor)
        let snr_distance = target.snr_db_min - input.snr_db;
        let noise_scale = if snr_distance > 0.0 {
            // Need noise reduction: scale by how far below target
            // Apply soft landing as we approach target
            let base_scale = (snr_distance / 10.0).clamp(0.0, 1.0);
            base_scale * soft_landing(snr_distance, 5.0) // Soft land in last 5 dB
        } else {
            // Already at or above target SNR: no processing needed
            0.0
        };

        // Task 5 edge case: Whisper + noisy combined
        // Whisper alone: halve noise reduction (preserve breathy texture)
        // Whisper + noisy: more conservative (noise reduction can grab whisper harmonics)
        let noise_scale = if cond.whisper && cond.noisy_environment {
            noise_scale * 0.35 // 35% max for whisper in noisy environment
        } else if cond.whisper {
            noise_scale * 0.5 // 50% max for whisper alone
        } else if cond.noisy_environment {
            noise_scale * 0.8 // 80% max for noisy (avoid over-processing)
        } else {
            noise_scale
        };

        // Reverb scale: based on Early/Late ratio distance from target
        // Uses soft landing to prevent overshoot
        let early_late_distance = target.early_late_ratio_min - input.early_late_ratio;
        let reverb_scale = if early_late_distance > 0.0 {
            // Need deverb: scale by how far below target
            let base_scale = (early_late_distance / 0.5).clamp(0.0, 1.0);
            base_scale * soft_landing(early_late_distance, 0.2) // Soft land in last 0.2
        } else {
            // Already in target range: stop processing
            0.0
        };

        // Proximity scale: only active when distant detected
        // Uses soft landing to prevent overshoot
        //
        // Task 5 edge case: Loud but distant speech
        // Proximity is based on reverb characteristics (early_late_ratio), NOT level.
        // This is intentional: a loud signal can still be "distant" if it has diffuse reverb.
        // The soft_landing prevents over-correction as we approach target.
        let proximity_scale = if cond.distant_mic {
            // Strong proximity for distant mic, but with soft landing
            let distance = target.early_late_ratio_min - input.early_late_ratio;
            0.8 * soft_landing(distance.max(0.0), 0.3)
        } else if input.early_late_ratio < target.early_late_ratio_min {
            // Mild proximity for slightly distant
            let distance = target.early_late_ratio_min - input.early_late_ratio;
            distance.clamp(0.0, 0.5) * soft_landing(distance, 0.2)
        } else {
            0.0 // Not distant, no proximity needed
        };

        // Whisper detection: disable proximity entirely (avoid unnaturally boosting breathy speech)
        let proximity_scale = if cond.whisper { 0.0 } else { proximity_scale };

        // Clarity scale: based on presence ratio distance from target
        // Uses soft landing to prevent overshoot as we approach target
        let presence_distance = target.presence_ratio_max - input.presence_ratio;
        let clarity_scale = if presence_distance < 0.0 {
            // Presence already above target: no clarity boost
            0.0
        } else {
            // Need clarity: scale by SNR (higher SNR = more clarity allowed)
            // Apply soft landing as we approach target presence
            let snr_factor = (input.snr_db / 15.0).clamp(0.0, 1.0);
            snr_factor * soft_landing(presence_distance, 0.005) // Soft land in last 0.005 ratio units
        };

        // Task 5 edge case: Apply condition caps for clarity
        // Whisper + noisy combined gets the most conservative cap
        let clarity_scale = if cond.whisper && cond.noisy_environment {
            clarity_scale * 0.15 // 15% max for whisper in noisy (avoid boosting noise as "brightness")
        } else if cond.whisper {
            clarity_scale * 0.25 // 25% max for whisper alone
        } else if cond.noisy_environment {
            clarity_scale * 0.40 // 40% max for noisy alone
        } else {
            clarity_scale
        };

        // De-esser scale: based on HF variance and sibilance level
        // Uses soft landing to prevent overshoot as HF variance approaches target
        // Task 4: Scale goes to 0 when HF variance is within target (no floor)
        let deesser_scale = if cond.whisper {
            0.0 // Never de-ess whisper
        } else {
            // Scale by how much HF variance exceeds target
            let hf_excess = input.hf_variance - target.hf_variance_max;
            if hf_excess > 0.0 {
                // Apply soft landing as we approach target HF variance
                let base_scale = (hf_excess / 1e-6).clamp(0.0, 1.0);
                base_scale * soft_landing(hf_excess, 5e-7) // Soft land in last 0.5e-6
            } else {
                // HF variance within target: no de-essing needed
                0.0
            }
        };

        // Leveler scale: based on RMS variance distance from target
        // Uses soft landing to prevent overshoot as RMS variance approaches target
        // Task 4: Scale goes to 0 when variance is within target (no floor)
        // Note: Clean-audio case provides its own minimal floor (0.1)
        let variance_distance = input.rms_variance - target.rms_variance_max;
        let leveler_scale = if variance_distance > 0.0 {
            // Need leveling: scale by how much above target
            // Apply soft landing as we approach target variance
            let base_scale = (variance_distance / 0.002).clamp(0.0, 1.0);
            base_scale * soft_landing(variance_distance, 0.0005) // Soft land in last 0.0005
        } else {
            // RMS variance within target: no additional leveling needed
            0.0
        };

        // Crest factor adjustment: if crest < 22 dB, reduce leveler aggressiveness
        let leveler_scale = if input.crest_factor_db < 22.0 {
            leveler_scale * 0.7 // Reduce to avoid over-compression
        } else {
            leveler_scale
        };

        self.target_calibration = CalibrationFactors {
            noise_scale,
            reverb_scale,
            proximity_scale,
            clarity_scale,
            deesser_scale,
            leveler_scale,
            clean_audio_attenuation: 1.0,
        };
    }

    /// Get current calibration factors (for external inspection)
    #[allow(dead_code)]
    pub fn get_calibration(&self) -> CalibrationFactors {
        self.calibration
    }

    /// Get current detected conditions
    #[allow(dead_code)]
    pub fn get_conditions(&self) -> DetectedConditions {
        self.conditions
    }

    /// Update macro state (call from UI parameter changes)
    pub fn set_state(&mut self, state: MacroState) {
        self.state = state;
        if state.active {
            self.compute_targets();
        }
    }

    /// Set individual macro values (for per-parameter UI binding)
    #[allow(dead_code)]
    pub fn set_distance(&mut self, value: f32) {
        self.state.distance = value.clamp(0.0, 1.0);
        if self.state.active {
            self.compute_targets();
        }
    }

    #[allow(dead_code)]
    pub fn set_clarity(&mut self, value: f32) {
        self.state.clarity = value.clamp(0.0, 1.0);
        if self.state.active {
            self.compute_targets();
        }
    }

    #[allow(dead_code)]
    pub fn set_consistency(&mut self, value: f32) {
        self.state.consistency = value.clamp(0.0, 1.0);
        if self.state.active {
            self.compute_targets();
        }
    }

    /// Enable or disable macro mode
    pub fn set_active(&mut self, active: bool) {
        self.state.active = active;
        if active {
            self.compute_targets();
        }
    }

    /// Check if macro mode is active
    #[allow(dead_code)]
    pub fn is_active(&self) -> bool {
        self.state.active
    }

    /// Get current macro state
    #[allow(dead_code)]
    pub fn get_state(&self) -> MacroState {
        self.state
    }

    /// Get current smoothed target values (call per-buffer or per-sample)
    #[allow(dead_code)]
    pub fn get_smoothed_targets(&self) -> MacroTargets {
        self.smoothed
    }

    /// Update smoothed values (call once per buffer)
    /// Returns the current smoothed targets
    pub fn update_smooth(&mut self, samples_in_buffer: usize) -> MacroTargets {
        if !self.state.active {
            return self.smoothed;
        }

        // Smooth each target value
        let smooth_amount = (MACRO_SMOOTH_RATE * samples_in_buffer as f32).min(1.0);

        self.smoothed.noise_reduction +=
            (self.targets.noise_reduction - self.smoothed.noise_reduction) * smooth_amount;
        self.smoothed.noise_tone +=
            (self.targets.noise_tone - self.smoothed.noise_tone) * smooth_amount;
        self.smoothed.reverb_reduction +=
            (self.targets.reverb_reduction - self.smoothed.reverb_reduction) * smooth_amount;
        self.smoothed.proximity +=
            (self.targets.proximity - self.smoothed.proximity) * smooth_amount;
        self.smoothed.de_esser += (self.targets.de_esser - self.smoothed.de_esser) * smooth_amount;
        self.smoothed.leveler += (self.targets.leveler - self.smoothed.leveler) * smooth_amount;

        self.smoothed
    }

    /// Compute target values from current macro state
    /// Applies calibration factors based on distance-to-target metrics
    fn compute_targets(&mut self) {
        let cal = &self.calibration;
        let atten = cal.clean_audio_attenuation;

        // Distance macro mapping (with calibration)
        let base_reverb = map_distance_to_reverb(self.state.distance);
        let base_proximity = map_distance_to_proximity(self.state.distance);

        self.targets.reverb_reduction = base_reverb * cal.reverb_scale * atten;
        self.targets.proximity = base_proximity * cal.proximity_scale * atten;

        // Clarity macro mapping (with calibration)
        let base_noise = map_clarity_to_noise(self.state.clarity);
        let base_deess = map_clarity_to_deess(self.state.clarity);

        self.targets.noise_reduction = base_noise * cal.noise_scale * atten;
        self.targets.de_esser = base_deess * cal.deesser_scale * atten;
        self.targets.noise_tone = map_clarity_to_tone(self.state.clarity);

        // Consistency macro mapping (with calibration)
        // Leveler is always at least minimally active for consistency
        let base_leveler = map_consistency_to_leveler(self.state.consistency);
        self.targets.leveler = base_leveler * cal.leveler_scale;
        // Leveler not affected by clean_audio_attenuation (minimal leveling allowed)
    }

    /// Snap smoothed values to targets (for instant changes)
    #[allow(dead_code)]
    pub fn snap_to_targets(&mut self) {
        if self.state.active {
            self.smoothed = self.targets;
        }
    }

    /// Apply currently smoothed macro targets to the plugin's VoiceParams.
    /// This is used to synchronize the advanced sliders when in macro mode,
    /// so that switching to advanced mode doesn't cause a sudden jump in parameters.
    pub fn apply_smoothed_targets_to_params(&self, params: &VoiceParams) {
        // This implicitly assumes MAX_GAIN is consistent between here and where VoiceStudioPlugin reads it.
        // It's currently hardcoded to 2.0 in lib.rs for noise, proximity, de-esser, leveler, clarity, reverb.
        // If MAX_GAIN changes in lib.rs, it must also be updated here.
        const MAX_GAIN: f32 = 2.0;

        params.noise_reduction.set_value(self.smoothed.noise_reduction / MAX_GAIN);
        params.noise_tone.set_value(self.smoothed.noise_tone);
        params.reverb_reduction.set_value(self.smoothed.reverb_reduction / MAX_GAIN);
        params.proximity.set_value(self.smoothed.proximity / MAX_GAIN);
        params.de_esser.set_value(self.smoothed.de_esser / MAX_GAIN);
        params.leveler.set_value(self.smoothed.leveler / MAX_GAIN);
    }

    // =========================================================================
    // Task 2.2: Distant Detection with Hysteresis
    // =========================================================================

    /// Compute distant detection with asymmetric hysteresis.
    ///
    /// Entry conditions (easy to enter):
    /// - early_late_ratio < 0.05 AND decay_slope < -0.0005
    ///
    /// Exit conditions (harder to exit):
    /// - early_late_ratio > 0.10 OR decay_slope > -0.0002
    ///
    /// Hold behavior:
    /// - Once distant is detected, hold for DISTANT_HOLD_MS before allowing exit
    fn compute_distant_with_hysteresis(&mut self, profile: &AudioProfile) -> bool {
        // Check entry condition (same as original but with named thresholds)
        let meets_entry = profile.early_late_ratio < DISTANT_ENTER_EARLY_LATE
            && profile.decay_slope < DISTANT_ENTER_DECAY_SLOPE;

        // Check exit condition (higher thresholds = harder to exit)
        let meets_exit = profile.early_late_ratio > DISTANT_EXIT_EARLY_LATE
            || profile.decay_slope > DISTANT_EXIT_DECAY_SLOPE;

        if self.was_distant {
            // Currently in distant state
            if meets_exit {
                // Check hold counter
                if self.distant_hold_counter > 0 {
                    self.distant_hold_counter -= 1;
                    // Still holding, stay distant
                    true
                } else {
                    // Hold expired and exit conditions met
                    self.was_distant = false;
                    false
                }
            } else {
                // Exit conditions not met, stay distant and reset hold
                let hold_frames =
                    ((DISTANT_HOLD_MS / 1000.0) * 48000.0 / FRAMES_PER_BUFFER_EST as f32) as usize;
                self.distant_hold_counter = hold_frames;
                true
            }
        } else {
            // Currently NOT in distant state
            if meets_entry {
                // Enter distant state
                self.was_distant = true;
                let hold_frames =
                    ((DISTANT_HOLD_MS / 1000.0) * 48000.0 / FRAMES_PER_BUFFER_EST as f32) as usize;
                self.distant_hold_counter = hold_frames;
                true
            } else {
                // Stay in non-distant state
                false
            }
        }
    }

    // =========================================================================
    // Task 6: Explainability API (developer-only, zero cost when not called)
    // =========================================================================

    /// Compute debug info explaining why calibration factors are at their current values.
    ///
    /// This method is ONLY for developer debugging/logging. It has zero cost when not called.
    /// Call this after `update_input_profile()` to inspect the decision-making process.
    ///
    /// Returns:
    /// - Distance-to-target for each metric
    /// - Reason why each scale factor stopped (at target, soft landing, capped, etc.)
    /// - Which condition caps were triggered
    #[allow(dead_code)]
    pub fn get_debug_info(&self) -> CalibrationDebugInfo {
        let target = &self.target_profile;
        let input = &self.input_profile;
        let cond = &self.conditions;
        let cal = &self.target_calibration;

        // Compute distances to target
        let snr_distance = target.snr_db_min - input.snr_db;
        let early_late_distance = target.early_late_ratio_min - input.early_late_ratio;
        let presence_distance = target.presence_ratio_max - input.presence_ratio;
        let hf_variance_distance = input.hf_variance - target.hf_variance_max;
        let rms_variance_distance = input.rms_variance - target.rms_variance_max;

        let distances = MetricDistances {
            snr_db: snr_distance,
            early_late_ratio: early_late_distance,
            presence_ratio: presence_distance,
            hf_variance: hf_variance_distance,
            rms_variance: rms_variance_distance,
        };

        // Determine caps triggered
        let caps_triggered = CapsTriggered {
            whisper: cond.whisper,
            noisy: cond.noisy_environment,
            whisper_noisy_combined: cond.whisper && cond.noisy_environment,
            crest_factor: input.crest_factor_db < 22.0,
            clean_audio: self.was_clean_audio,
            distant_mic: cond.distant_mic,
        };

        // Determine reason for each scale factor
        let noise_reason = if self.was_clean_audio {
            ScaleReason::CleanAudio
        } else if cond.whisper && cond.noisy_environment {
            ScaleReason::CappedWhisperNoisy
        } else if cond.whisper {
            ScaleReason::CappedWhisper
        } else if cond.noisy_environment {
            ScaleReason::CappedNoisy
        } else if snr_distance <= 0.0 {
            ScaleReason::AtTarget
        } else if snr_distance < 5.0 {
            ScaleReason::SoftLanding
        } else {
            ScaleReason::DistanceProportional
        };

        let reverb_reason = if self.was_clean_audio {
            ScaleReason::CleanAudio
        } else if early_late_distance <= 0.0 {
            ScaleReason::AtTarget
        } else if early_late_distance < 0.2 {
            ScaleReason::SoftLanding
        } else {
            ScaleReason::DistanceProportional
        };

        let proximity_reason = if self.was_clean_audio {
            ScaleReason::CleanAudio
        } else if cond.whisper {
            ScaleReason::CappedWhisper
        } else if cond.distant_mic {
            ScaleReason::DistantMicDetected
        } else if early_late_distance <= 0.0 {
            ScaleReason::AtTarget
        } else if early_late_distance < 0.2 {
            ScaleReason::SoftLanding
        } else {
            ScaleReason::DistanceProportional
        };

        let clarity_reason = if self.was_clean_audio {
            ScaleReason::CleanAudio
        } else if cond.whisper && cond.noisy_environment {
            ScaleReason::CappedWhisperNoisy
        } else if cond.whisper {
            ScaleReason::CappedWhisper
        } else if cond.noisy_environment {
            ScaleReason::CappedNoisy
        } else if presence_distance < 0.0 {
            ScaleReason::AtTarget
        } else if presence_distance < 0.005 {
            ScaleReason::SoftLanding
        } else {
            ScaleReason::DistanceProportional
        };

        let deesser_reason = if self.was_clean_audio {
            ScaleReason::CleanAudio
        } else if cond.whisper {
            ScaleReason::CappedWhisper
        } else if hf_variance_distance <= 0.0 {
            ScaleReason::AtTarget
        } else if hf_variance_distance < 5e-7 {
            ScaleReason::SoftLanding
        } else {
            ScaleReason::DistanceProportional
        };

        let leveler_reason = if self.was_clean_audio {
            ScaleReason::CleanAudio
        } else if input.crest_factor_db < 22.0 && cal.leveler_scale > 0.0 {
            ScaleReason::CappedCrestFactor
        } else if rms_variance_distance <= 0.0 {
            ScaleReason::AtTarget
        } else if rms_variance_distance < 0.0005 {
            ScaleReason::SoftLanding
        } else {
            ScaleReason::DistanceProportional
        };

        CalibrationDebugInfo {
            distances,
            noise_reason,
            reverb_reason,
            proximity_reason,
            clarity_reason,
            deesser_reason,
            leveler_reason,
            caps_triggered,
        }
    }

    /// Format debug info as a human-readable string (for logging)
    #[allow(dead_code)]
    pub fn format_debug_info(&self) -> String {
        let info = self.get_debug_info();
        let cal = &self.calibration;

        format!(
            "=== Calibration Debug Info ===\n\
             Distances to target:\n\
               SNR: {:.2} dB (target: >= {:.1} dB)\n\
               Early/Late: {:.3} (target: >= {:.2})\n\
               Presence: {:.4} (target: <= {:.4})\n\
               HF variance: {:.2e} (target: <= {:.2e})\n\
               RMS variance: {:.4} (target: <= {:.4})\n\
             \n\
             Scale factors (reason):\n\
               noise_scale: {:.3} ({:?})\n\
               reverb_scale: {:.3} ({:?})\n\
               proximity_scale: {:.3} ({:?})\n\
               clarity_scale: {:.3} ({:?})\n\
               deesser_scale: {:.3} ({:?})\n\
               leveler_scale: {:.3} ({:?})\n\
             \n\
             Caps triggered: whisper={}, noisy={}, whisper+noisy={}, crest={}, clean={}, distant={}",
            info.distances.snr_db, self.target_profile.snr_db_min,
            info.distances.early_late_ratio, self.target_profile.early_late_ratio_min,
            info.distances.presence_ratio, self.target_profile.presence_ratio_max,
            info.distances.hf_variance, self.target_profile.hf_variance_max,
            info.distances.rms_variance, self.target_profile.rms_variance_max,
            cal.noise_scale, info.noise_reason,
            cal.reverb_scale, info.reverb_reason,
            cal.proximity_scale, info.proximity_reason,
            cal.clarity_scale, info.clarity_reason,
            cal.deesser_scale, info.deesser_reason,
            cal.leveler_scale, info.leveler_reason,
            info.caps_triggered.whisper,
            info.caps_triggered.noisy,
            info.caps_triggered.whisper_noisy_combined,
            info.caps_triggered.crest_factor,
            info.caps_triggered.clean_audio,
            info.caps_triggered.distant_mic,
        )
    }
}

impl Default for MacroController {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Public Helper - Compute targets from macro values
// =============================================================================

/// Compute what the advanced parameter values should be given macro slider values.
/// Used when switching from Simple mode to Advanced mode to sync the sliders.
pub fn compute_targets_from_macros(distance: f32, clarity: f32, consistency: f32) -> MacroTargets {
    MacroTargets {
        noise_reduction: map_clarity_to_noise(clarity),
        noise_tone: map_clarity_to_tone(clarity),
        reverb_reduction: map_distance_to_reverb(distance),
        proximity: map_distance_to_proximity(distance),
        de_esser: map_clarity_to_deess(clarity),
        leveler: map_consistency_to_leveler(consistency),
    }
}

// =============================================================================
// Mapping Functions
// =============================================================================

/// Map distance macro (0=far, 1=close) to reverb reduction
///
/// Uses a curved response that biases toward early removal -
/// most of the reverb reduction happens in the first half of the range.
/// Smooth mapping: 0→0, 1→max (no discontinuities).
fn map_distance_to_reverb(distance: f32) -> f32 {
    // Curve: faster rise at the beginning (early reflections matter most)
    let curved = smoothstep(0.0, 1.0, distance);
    let biased = curved.powf(0.7); // Bias toward higher values earlier
    DISTANCE_REVERB_MIN + (DISTANCE_REVERB_MAX - DISTANCE_REVERB_MIN) * biased
}

/// Map distance macro to proximity EQ
///
/// Nonlinear mapping - subtle at low values, stronger at high values
/// to prevent harshness from over-proximity.
fn map_distance_to_proximity(distance: f32) -> f32 {
    // Curve: gentle start, steeper at the end
    let curved = distance * distance; // Quadratic - gentle at start
    DISTANCE_PROXIMITY_MIN + (DISTANCE_PROXIMITY_MAX - DISTANCE_PROXIMITY_MIN) * curved
}

/// Map clarity macro to noise reduction
///
/// Smooth mapping: 0→0, 1→max (no discontinuities).
fn map_clarity_to_noise(clarity: f32) -> f32 {
    // Linear mapping from 0 to max
    CLARITY_NOISE_MIN + (CLARITY_NOISE_MAX - CLARITY_NOISE_MIN) * clarity
}

/// Map clarity macro to de-esser depth
///
/// Subtle at low values, more aggressive at high clarity settings.
fn map_clarity_to_deess(clarity: f32) -> f32 {
    // Gentle curve - de-esser can be aggressive, so start slow
    let curved = smoothstep(0.0, 1.0, clarity);
    CLARITY_DEESS_MIN + (CLARITY_DEESS_MAX - CLARITY_DEESS_MIN) * curved
}

/// Map clarity macro to noise tone bias
///
/// At clarity=0: neutral tone (0.5) - no bias
/// At higher clarity: slight hiss bias (>0.5) to preserve brightness
/// Note: Tone is different from other mappings - 0.5 is the "zero effect" value.
fn map_clarity_to_tone(clarity: f32) -> f32 {
    // At clarity=0, return neutral (0.5)
    // Smoothly transition from neutral toward slight hiss bias at high clarity
    let bias = clarity * CLARITY_TONE_RANGE; // 0 at clarity=0, up to CLARITY_TONE_RANGE at clarity=1
    (CLARITY_TONE_CENTER + bias).clamp(0.0, 1.0)
}

/// Map consistency macro to leveler depth
///
/// Nonlinear to avoid pumping at high settings.
fn map_consistency_to_leveler(consistency: f32) -> f32 {
    // S-curve to smooth the middle range (where pumping is most noticeable)
    let curved = smoothstep(0.0, 1.0, consistency);
    CONSISTENCY_LEVELER_MIN + (CONSISTENCY_LEVELER_MAX - CONSISTENCY_LEVELER_MIN) * curved
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_macro_controller_creation() {
        let controller = MacroController::new();
        assert!(!controller.is_active());
        assert!(controller.state.distance == 0.0);
    }

    #[test]
    fn test_distance_mapping_range() {
        // At 0, reverb should be 0 (macro=0 → advanced=0)
        assert!((map_distance_to_reverb(0.0) - 0.0).abs() < 0.001);

        // At 1, reverb should be at maximum
        assert!((map_distance_to_reverb(1.0) - DISTANCE_REVERB_MAX).abs() < 0.01);

        // Monotonically increasing from 0
        let mut prev = map_distance_to_reverb(0.0);
        for i in 1..=10 {
            let val = map_distance_to_reverb(i as f32 / 10.0);
            assert!(val >= prev, "reverb not monotonic at {}", i);
            prev = val;
        }
    }

    #[test]
    fn test_clarity_noise_zero_at_zero() {
        // At clarity=0, noise reduction should be 0 (macro=0 → advanced=0)
        let noise = map_clarity_to_noise(0.0);
        assert!((noise - 0.0).abs() < 0.001);

        // Scales smoothly from 0
        let noise_small = map_clarity_to_noise(0.1);
        assert!(noise_small > 0.0);
        assert!(noise_small < map_clarity_to_noise(0.5));
    }

    #[test]
    fn test_all_macros_zero_maps_to_zero() {
        // Verify macro=0 always maps to advanced=0 for all affected parameters
        assert!((map_distance_to_reverb(0.0) - 0.0).abs() < 0.001, "reverb");
        assert!(
            (map_distance_to_proximity(0.0) - 0.0).abs() < 0.001,
            "proximity"
        );
        assert!((map_clarity_to_noise(0.0) - 0.0).abs() < 0.001, "noise");
        assert!((map_clarity_to_deess(0.0) - 0.0).abs() < 0.001, "deesser");
        assert!(
            (map_consistency_to_leveler(0.0) - 0.0).abs() < 0.001,
            "leveler"
        );
        // Tone is special: 0.5 is neutral (no bias), which is correct
        assert!(
            (map_clarity_to_tone(0.0) - 0.5).abs() < 0.001,
            "tone should be neutral at 0"
        );
    }

    #[test]
    fn test_consistency_mapping_range() {
        // At 0, leveler should be at minimum
        assert!((map_consistency_to_leveler(0.0) - CONSISTENCY_LEVELER_MIN).abs() < 0.01);

        // At 1, leveler should be at maximum
        assert!((map_consistency_to_leveler(1.0) - CONSISTENCY_LEVELER_MAX).abs() < 0.01);
    }

    #[test]
    fn test_macro_activation() {
        let mut controller = MacroController::new();

        controller.set_distance(0.5);
        controller.set_clarity(0.5);
        controller.set_consistency(0.5);
        controller.set_active(true);

        assert!(controller.is_active());

        // Snap and verify targets are computed
        controller.snap_to_targets();
        let targets = controller.get_smoothed_targets();

        assert!(targets.reverb_reduction > 0.0);
        assert!(targets.noise_reduction > 0.0);
        assert!(targets.leveler > 0.0);
    }

    #[test]
    fn test_debug_info_at_target() {
        use crate::AudioProfile;

        let mut controller = MacroController::new();

        // Create a "clean" audio profile that's within target
        let clean_profile = AudioProfile {
            rms: 0.05,
            peak: 0.3,
            crest_factor_db: 25.0,
            rms_variance: 0.001,
            noise_floor: 0.012,
            snr_db: 15.0,          // Above target (10 dB)
            early_late_ratio: 0.6, // Within target
            decay_slope: 0.0,
            presence_ratio: 0.008,
            air_ratio: 0.003,
            hf_variance: 2e-7, // Within target
        };

        controller.update_input_profile(clean_profile);
        let debug_info = controller.get_debug_info();

        // Clean audio should be detected
        assert!(debug_info.caps_triggered.clean_audio);

        // All reasons should be CleanAudio
        assert_eq!(debug_info.noise_reason, ScaleReason::CleanAudio);
        assert_eq!(debug_info.reverb_reason, ScaleReason::CleanAudio);
    }

    #[test]
    fn test_debug_info_whisper_detection() {
        use crate::AudioProfile;

        let mut controller = MacroController::new();

        // Create a whisper profile (high HF variance, low SNR)
        let whisper_profile = AudioProfile {
            rms: 0.02,
            peak: 0.1,
            crest_factor_db: 20.0,
            rms_variance: 0.003,
            noise_floor: 0.015,
            snr_db: 10.0,          // Low SNR (< 15 dB for whisper)
            early_late_ratio: 0.3, // Below target
            decay_slope: -0.0003,
            presence_ratio: 0.015,
            air_ratio: 0.008,
            hf_variance: 2e-6, // High (> 1e-6 for whisper)
        };

        controller.update_input_profile(whisper_profile);
        let debug_info = controller.get_debug_info();

        // Whisper should be detected
        assert!(debug_info.caps_triggered.whisper);

        // Noise and clarity should be capped for whisper
        assert_eq!(debug_info.noise_reason, ScaleReason::CappedWhisper);
        assert_eq!(debug_info.proximity_reason, ScaleReason::CappedWhisper);
    }

    #[test]
    fn test_debug_info_format() {
        let controller = MacroController::new();
        let formatted = controller.format_debug_info();

        // Should contain key sections
        assert!(formatted.contains("Calibration Debug Info"));
        assert!(formatted.contains("Distances to target"));
        assert!(formatted.contains("Scale factors"));
        assert!(formatted.contains("Caps triggered"));
    }
}
