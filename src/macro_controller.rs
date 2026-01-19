//! Macro Controller (Parameter Orchestrator)
//!
//! Intent-based macro layer with data-driven calibration.
//! Macros write parameters only. They never touch audio.
//!
//! Macros:
//! - Distance    -> reverb_reduction, proximity
//! - Clarity     -> noise_reduction, de_esser, noise_tone, clarity_cap (light touch)
//! - Consistency -> leveler
//!
//! Invariants:
//! - Macro mode never changes DSP topology
//! - All macro effects resolve to advanced parameters
//! - Macro Clarity does NOT force "clarity amount" – it only caps it gently
//!
//! # MACRO INTERACTION SAFETY
//!
//! ## Hard Caps (Prevent Extreme Stacking)
//!
//! Each macro output is capped to prevent edge-case stacking:
//! - `DIST_REVERB_MAX = 0.85` - leaves headroom for manual adjustment
//! - `DIST_PROX_MAX = 0.70` - proximity boost is subtle by design
//! - `CLARITY_NOISE_MAX = 0.70` - prevents over-aggressive denoising
//! - `CLARITY_DEESS_MAX = 0.55` - de-esser is conservative
//! - `CLARITY_CAP_MAX = 0.25` - clarity is a ceiling, not an amount
//! - `LEVELER_MAX = 0.80` - leveler never fully saturates
//!
//! ## Cross-Macro Independence
//!
//! Macros target orthogonal signal characteristics:
//! - Distance: spatial/room (reverb, proximity)
//! - Clarity: spectral/noise (denoise, de-ess)
//! - Consistency: dynamics (leveler)
//!
//! These operate on different DSP stages and do not compound on the same
//! signal path. High Distance + High Clarity is valid (noisy reverberant room).
//!
//! ## Condition-Based Guards
//!
//! Calibration scales down outputs based on detected conditions:
//! - Clean audio: minimal processing (noise=0.05, reverb=0)
//! - Whisper: noise*0.5, clarity_cap*0.25, proximity=0, deesser=0
//! - Noisy environment: clarity_cap*0.4, noise*0.8
//! - Low crest factor: leveler*0.7
//!
//! These guards prevent over-processing when the source doesn't need it.

use crate::dsp::utils::smoothstep;
use crate::{AudioProfile, DetectedConditions, TargetProfile};

// =============================================================================
// Mapping limits (hard safety caps)
// =============================================================================

const DIST_REVERB_MAX: f32 = 0.85;
const DIST_PROX_MAX: f32 = 0.70;

const CLARITY_NOISE_MAX: f32 = 0.70;
const CLARITY_DEESS_MAX: f32 = 0.55;

// Macro clarity is intentionally subtle: this is a ceiling for the *advanced* clarity control.
const CLARITY_CAP_MAX: f32 = 0.25;

// Tone bias: push a little towards "Hiss" as clarity rises (keep subtle).
const CLARITY_TONE_RANGE: f32 = 0.15;

const LEVELER_MAX: f32 = 0.80;

// Smoothing
const MACRO_SMOOTH_RATE: f32 = 0.002;
const CALIBRATION_SMOOTH_COEFF: f32 = 0.92;

// Clean audio hysteresis
const CLEAN_EXIT_SNR_MARGIN: f32 = 1.0;
const CLEAN_EXIT_EARLY_LATE_MARGIN: f32 = 0.05;
const CLEAN_EXIT_HF_MULT: f32 = 1.5;

// Distant detection (buffer-based)
const DISTANT_ENTER_EARLY_LATE: f32 = 0.05;
const DISTANT_EXIT_EARLY_LATE: f32 = 0.10;
const DISTANT_ENTER_DECAY: f32 = -0.0005;
const DISTANT_EXIT_DECAY: f32 = -0.0002;
const DISTANT_HOLD_BUFFERS: usize = 30;

// =============================================================================
// Macro state
// =============================================================================

#[derive(Clone, Copy, Debug, Default)]
pub struct MacroState {
    pub distance: f32,
    pub clarity: f32,
    pub consistency: f32,
    pub active: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MacroTargets {
    pub noise_reduction: f32,
    pub noise_tone: f32,
    pub reverb_reduction: f32,
    pub proximity: f32,
    pub de_esser: f32,
    pub leveler: f32,

    /// A ceiling for the *advanced* clarity amount (light touch).
    /// Caller should do: `clarity_amt = user_clarity.min(targets.clarity_cap)`.
    pub clarity_cap: f32,
}

// =============================================================================
// Calibration
// =============================================================================

#[derive(Clone, Copy, Debug, Default)]
struct Calibration {
    noise: f32,
    reverb: f32,
    proximity: f32,
    clarity_cap: f32,
    deesser: f32,
    leveler: f32,
    atten: f32,
}

// =============================================================================
// Controller
// =============================================================================

pub struct MacroController {
    state: MacroState,
    targets: MacroTargets,
    smoothed: MacroTargets,

    profile: AudioProfile,
    target: TargetProfile,
    conditions: DetectedConditions,

    cal: Calibration,
    cal_target: Calibration,

    was_clean: bool,
    was_distant: bool,
    distant_hold: usize,
}

impl MacroController {
    pub fn new() -> Self {
        Self {
            state: MacroState::default(),
            targets: MacroTargets::default(),
            smoothed: MacroTargets::default(),
            profile: AudioProfile::default(),
            target: TargetProfile::PROFESSIONAL_VO,
            conditions: DetectedConditions::default(),
            cal: Calibration {
                noise: 1.0,
                reverb: 1.0,
                proximity: 1.0,
                clarity_cap: 1.0,
                deesser: 1.0,
                leveler: 1.0,
                atten: 1.0,
            },
            cal_target: Calibration::default(),
            was_clean: false,
            was_distant: false,
            distant_hold: 0,
        }
    }

    // -------------------------------------------------------------------------
    // Public API
    // -------------------------------------------------------------------------

    pub fn set_state(&mut self, state: MacroState) {
        self.state = MacroState {
            distance: state.distance.clamp(0.0, 1.0),
            clarity: state.clarity.clamp(0.0, 1.0),
            consistency: state.consistency.clamp(0.0, 1.0),
            active: state.active,
        };

        if self.state.active {
            self.compute_targets();
        }
    }

    pub fn set_active(&mut self, active: bool) {
        self.state.active = active;
        if active {
            self.compute_targets();
        }
    }

    pub fn update_input_profile(&mut self, profile: AudioProfile) {
        self.profile = profile;
        self.conditions = DetectedConditions::detect(&profile);
        self.conditions.distant_mic = self.detect_distant();

        self.compute_calibration();

        let a = 1.0 - CALIBRATION_SMOOTH_COEFF;
        self.cal.noise += (self.cal_target.noise - self.cal.noise) * a;
        self.cal.reverb += (self.cal_target.reverb - self.cal.reverb) * a;
        self.cal.proximity += (self.cal_target.proximity - self.cal.proximity) * a;
        self.cal.clarity_cap += (self.cal_target.clarity_cap - self.cal.clarity_cap) * a;
        self.cal.deesser += (self.cal_target.deesser - self.cal.deesser) * a;
        self.cal.leveler += (self.cal_target.leveler - self.cal.leveler) * a;
        self.cal.atten += (self.cal_target.atten - self.cal.atten) * a;

        if self.state.active {
            self.compute_targets();
        }
    }

    /// Smoothly approach current targets. Call once per buffer (or per block).
    pub fn update_smooth(&mut self, samples: usize) -> MacroTargets {
        if !self.state.active {
            return self.smoothed;
        }

        let k = (MACRO_SMOOTH_RATE * samples as f32).min(1.0);

        self.smoothed.noise_reduction +=
            (self.targets.noise_reduction - self.smoothed.noise_reduction) * k;
        self.smoothed.noise_tone += (self.targets.noise_tone - self.smoothed.noise_tone) * k;
        self.smoothed.reverb_reduction +=
            (self.targets.reverb_reduction - self.smoothed.reverb_reduction) * k;
        self.smoothed.proximity += (self.targets.proximity - self.smoothed.proximity) * k;
        self.smoothed.de_esser += (self.targets.de_esser - self.smoothed.de_esser) * k;
        self.smoothed.leveler += (self.targets.leveler - self.smoothed.leveler) * k;
        self.smoothed.clarity_cap += (self.targets.clarity_cap - self.smoothed.clarity_cap) * k;

        self.smoothed
    }

    pub fn get_conditions(&self) -> DetectedConditions {
        self.conditions
    }

    // -------------------------------------------------------------------------
    // Calibration logic
    // -------------------------------------------------------------------------

    fn compute_calibration(&mut self) {
        let p = &self.profile;
        let t = &self.target;
        let c = &self.conditions;

        // "Clean" is a guardrail state: do as little as possible.
        let clean_now = p.snr_db >= t.snr_db_min
            && p.early_late_ratio >= t.early_late_ratio_min
            && p.hf_variance <= t.hf_variance_max;

        let clean_exit_guard = p.snr_db >= (t.snr_db_min - CLEAN_EXIT_SNR_MARGIN)
            && p.early_late_ratio >= (t.early_late_ratio_min - CLEAN_EXIT_EARLY_LATE_MARGIN)
            && p.hf_variance <= (t.hf_variance_max * CLEAN_EXIT_HF_MULT);

        let use_clean = if self.was_clean {
            clean_now && clean_exit_guard
        } else {
            clean_now
        };
        self.was_clean = use_clean;

        if use_clean {
            // Stay extremely light.
            self.cal_target = Calibration {
                noise: 0.05,
                reverb: 0.0,
                proximity: 0.0,
                clarity_cap: 0.10, // allow a tiny bit of user clarity
                deesser: 0.0,
                leveler: 0.10,
                atten: 0.10,
            };
            return;
        }

        // Distances/excesses (all >= 0.0)
        let snr_deficit = (t.snr_db_min - p.snr_db).max(0.0);
        let early_deficit = (t.early_late_ratio_min - p.early_late_ratio).max(0.0);

        // Presence ratio is a "too much" measure in your TargetProfile (max),
        // but we want to allow a bit more clarity cap when presence is LOW.
        // So treat "presence deficit" as (max - current).
        let presence_deficit = (t.presence_ratio_max - p.presence_ratio).max(0.0);

        let hf_excess = (p.hf_variance - t.hf_variance_max).max(0.0);
        let var_excess = (p.rms_variance - t.rms_variance_max).max(0.0);

        // Base scales 0..1
        let mut noise = soft(snr_deficit, 5.0);
        let mut clarity_cap = soft(presence_deficit, 0.005);
        let mut deesser = soft(hf_excess, 5e-7);
        let mut reverb = soft(early_deficit, 0.2);

        let mut proximity = if c.distant_mic {
            soft(early_deficit, 0.3)
        } else {
            0.0
        };

        let mut leveler = soft(var_excess, 0.0005);

        // Condition-based guards
        if c.whisper {
            noise *= 0.5;
            clarity_cap *= 0.25;
            proximity = 0.0;
            deesser = 0.0;
        }

        if c.noisy_environment {
            clarity_cap *= 0.4;
            noise *= 0.8;
        }

        // If crest factor is low, avoid pumping artifacts.
        if p.crest_factor_db < 22.0 {
            leveler *= 0.7;
        }

        self.cal_target = Calibration {
            noise,
            reverb,
            proximity,
            clarity_cap,
            deesser,
            leveler,
            atten: 1.0,
        };
    }

    fn detect_distant(&mut self) -> bool {
        let p = &self.profile;

        let enter =
            p.early_late_ratio < DISTANT_ENTER_EARLY_LATE && p.decay_slope < DISTANT_ENTER_DECAY;
        let exit =
            p.early_late_ratio > DISTANT_EXIT_EARLY_LATE || p.decay_slope > DISTANT_EXIT_DECAY;

        if self.was_distant {
            if exit && self.distant_hold == 0 {
                self.was_distant = false;
                false
            } else {
                self.distant_hold = self.distant_hold.saturating_sub(1);
                true
            }
        } else if enter {
            self.was_distant = true;
            self.distant_hold = DISTANT_HOLD_BUFFERS;
            true
        } else {
            false
        }
    }

    // -------------------------------------------------------------------------
    // Target mapping
    // -------------------------------------------------------------------------

    fn compute_targets(&mut self) {
        let s = &self.state;
        let c = &self.cal;

        // Distance -> room control + proximity
        self.targets.reverb_reduction =
            smoothstep(0.0, 1.0, s.distance) * DIST_REVERB_MAX * c.reverb * c.atten;

        self.targets.proximity = (s.distance * s.distance) * DIST_PROX_MAX * c.proximity * c.atten;

        // Clarity macro -> restoration helpers
        self.targets.noise_reduction = s.clarity * CLARITY_NOISE_MAX * c.noise * c.atten;

        self.targets.de_esser =
            smoothstep(0.0, 1.0, s.clarity) * CLARITY_DEESS_MAX * c.deesser * c.atten;

        // Light-touch clarity cap: this does not “apply clarity”, it only permits some of it.
        self.targets.clarity_cap =
            smoothstep(0.0, 1.0, s.clarity) * CLARITY_CAP_MAX * c.clarity_cap * c.atten;

        // Tone bias: gentle move towards hiss as clarity rises
        self.targets.noise_tone = (0.5 + s.clarity * CLARITY_TONE_RANGE).clamp(0.0, 1.0);

        // Consistency -> leveler
        self.targets.leveler = smoothstep(0.0, 1.0, s.consistency) * LEVELER_MAX * c.leveler;
    }
}

pub fn compute_targets_from_macros(distance: f32, clarity: f32, consistency: f32) -> MacroTargets {
    MacroTargets {
        noise_reduction: clarity * CLARITY_NOISE_MAX,
        noise_tone: (0.5 + clarity * CLARITY_TONE_RANGE).clamp(0.0, 1.0),
        reverb_reduction: smoothstep(0.0, 1.0, distance) * DIST_REVERB_MAX,
        proximity: (distance * distance) * DIST_PROX_MAX,
        de_esser: smoothstep(0.0, 1.0, clarity) * CLARITY_DEESS_MAX,
        leveler: smoothstep(0.0, 1.0, consistency) * LEVELER_MAX,
        clarity_cap: smoothstep(0.0, 1.0, clarity) * CLARITY_CAP_MAX,
    }
}

// =============================================================================
// Helpers
// =============================================================================

#[inline]
fn soft(distance: f32, threshold: f32) -> f32 {
    if distance <= 0.0 {
        0.0
    } else if distance >= threshold {
        1.0
    } else {
        let t = distance / threshold;
        t * t * (3.0 - 2.0 * t)
    }
}

impl Default for MacroController {
    fn default() -> Self {
        Self::new()
    }
}
