//! Thread-safe metering utilities for real-time audio processing.
//!
//! This module provides atomic float storage for sharing meter data between
//! the audio thread and UI thread without locks. Some getters are currently
//! unused but are kept for debugging and future UI integration.

use std::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering};

#[derive(Debug)]
pub struct AtomicF32 {
    inner: AtomicU32,
}

impl AtomicF32 {
    pub const fn new(value: f32) -> Self {
        Self {
            inner: AtomicU32::new(value.to_bits()),
        }
    }

    pub fn store(&self, value: f32, order: Ordering) {
        self.inner.store(value.to_bits(), order);
    }

    #[allow(dead_code)]
    pub fn load(&self, order: Ordering) -> f32 {
        f32::from_bits(self.inner.load(order))
    }
}

impl Default for AtomicF32 {
    fn default() -> Self {
        Self::new(0.0)
    }
}

/// Thread-safe metering for input/output levels and gain reduction.
#[derive(Default)]
pub struct Meters {
    input_peak_l: AtomicU32,
    input_peak_r: AtomicU32,
    output_peak_l: AtomicU32,
    output_peak_r: AtomicU32,
    gain_reduction_l: AtomicU32,
    gain_reduction_r: AtomicU32,

    // Debug meters for DSP analysis
    /// Speech confidence from estimator (0.0 - 1.0)
    debug_speech_confidence: AtomicU32,
    /// De-esser gain reduction in dB (0.0 to ~18.0)
    debug_deesser_gr_db: AtomicU32,
    /// Limiter gain reduction in dB
    debug_limiter_gr_db: AtomicU32,
    /// Early reflection suppression amount (0.0 - 0.35)
    debug_early_reflection: AtomicU32,
    /// Spectral guardrails low-mid cut in dB
    debug_guardrails_low_cut: AtomicU32,
    /// Spectral guardrails high cut in dB
    debug_guardrails_high_cut: AtomicU32,
    /// Denoise noise floor estimate in dB
    debug_noise_floor_db: AtomicU32,
    /// Speech expander attenuation in dB
    debug_expander_atten_db: AtomicU32,

    // Layer 1: Resolved Parameters
    pub(crate) noise_reduction_resolved: AtomicF32,
    pub(crate) noise_tone_resolved: AtomicF32,
    pub(crate) deverb_resolved: AtomicF32,
    pub(crate) clarity_resolved: AtomicF32,
    pub(crate) deesser_resolved: AtomicF32,
    pub(crate) proximity_resolved: AtomicF32,
    pub(crate) leveler_resolved: AtomicF32,
    pub(crate) breath_reduction_resolved: AtomicF32,

    // Layer 2: Safeguard Interventions
    pub(crate) loudness_comp_db: AtomicF32,
    pub(crate) loudness_error_db: AtomicF32,
    pub(crate) loudness_active: AtomicI32,
    pub(crate) speech_band_loss_db: AtomicF32,
    pub(crate) speech_protection_active: AtomicI32,
    pub(crate) speech_protection_scale: AtomicF32,
    pub(crate) energy_budget_active: AtomicI32,
    pub(crate) energy_budget_scale: AtomicF32,

    // Layer 3: Audible Outcome Metrics
    pub(crate) output_rms_db: AtomicF32,
    pub(crate) output_peak_db: AtomicF32,
    pub(crate) output_crest_db: AtomicF32,
    pub(crate) total_gain_reduction_db: AtomicF32,

    // Layer 4: Mode Switch Integrity
    pub(crate) mode_transition_event: AtomicI32,
    pub(crate) params_hash_before: AtomicU64,
    pub(crate) params_hash_after: AtomicU64,
    pub(crate) audible_change_detected: AtomicI32,
    pub(crate) pre_switch_audible_rms: AtomicF32,

    // DTLN availability status
    pub(crate) dtln_available: AtomicI32,

    // Pump detection meters
    pub(crate) pump_event_count: AtomicI32,
    pub(crate) pump_severity_db: AtomicF32,
    pub(crate) compressor_gain_delta_db: AtomicF32,
}

impl Meters {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_input_peak_l(&self, val: f32) {
        self.input_peak_l.store(val.to_bits(), Ordering::Relaxed);
    }

    pub fn set_input_peak_r(&self, val: f32) {
        self.input_peak_r.store(val.to_bits(), Ordering::Relaxed);
    }

    pub fn set_output_peak_l(&self, val: f32) {
        self.output_peak_l.store(val.to_bits(), Ordering::Relaxed);
    }

    pub fn set_output_peak_r(&self, val: f32) {
        self.output_peak_r.store(val.to_bits(), Ordering::Relaxed);
    }

    pub fn set_gain_reduction_l(&self, val: f32) {
        self.gain_reduction_l
            .store(val.to_bits(), Ordering::Relaxed);
    }

    pub fn set_gain_reduction_r(&self, val: f32) {
        self.gain_reduction_r
            .store(val.to_bits(), Ordering::Relaxed);
    }

    pub fn get_input_peak_l(&self) -> f32 {
        f32::from_bits(self.input_peak_l.load(Ordering::Relaxed))
    }

    pub fn get_input_peak_r(&self) -> f32 {
        f32::from_bits(self.input_peak_r.load(Ordering::Relaxed))
    }

    pub fn get_output_peak_l(&self) -> f32 {
        f32::from_bits(self.output_peak_l.load(Ordering::Relaxed))
    }

    pub fn get_output_peak_r(&self) -> f32 {
        f32::from_bits(self.output_peak_r.load(Ordering::Relaxed))
    }

    pub fn get_gain_reduction_l(&self) -> f32 {
        f32::from_bits(self.gain_reduction_l.load(Ordering::Relaxed))
    }

    pub fn get_gain_reduction_r(&self) -> f32 {
        f32::from_bits(self.gain_reduction_r.load(Ordering::Relaxed))
    }

    // =========================================================================
    // Debug Meters - for DSP analysis and tuning
    // =========================================================================

    pub fn set_debug_speech_confidence(&self, val: f32) {
        self.debug_speech_confidence
            .store(val.to_bits(), Ordering::Relaxed);
    }

    #[allow(dead_code)]
    pub fn get_debug_speech_confidence(&self) -> f32 {
        f32::from_bits(self.debug_speech_confidence.load(Ordering::Relaxed))
    }

    pub fn set_debug_deesser_gr_db(&self, val: f32) {
        self.debug_deesser_gr_db
            .store(val.to_bits(), Ordering::Relaxed);
    }

    #[allow(dead_code)]
    pub fn get_debug_deesser_gr_db(&self) -> f32 {
        f32::from_bits(self.debug_deesser_gr_db.load(Ordering::Relaxed))
    }

    pub fn set_debug_limiter_gr_db(&self, val: f32) {
        self.debug_limiter_gr_db
            .store(val.to_bits(), Ordering::Relaxed);
    }

    #[allow(dead_code)]
    pub fn get_debug_limiter_gr_db(&self) -> f32 {
        f32::from_bits(self.debug_limiter_gr_db.load(Ordering::Relaxed))
    }

    pub fn set_debug_early_reflection(&self, val: f32) {
        self.debug_early_reflection
            .store(val.to_bits(), Ordering::Relaxed);
    }

    #[allow(dead_code)]
    pub fn get_debug_early_reflection(&self) -> f32 {
        f32::from_bits(self.debug_early_reflection.load(Ordering::Relaxed))
    }

    pub fn set_debug_guardrails_low_cut(&self, val: f32) {
        self.debug_guardrails_low_cut
            .store(val.to_bits(), Ordering::Relaxed);
    }

    #[allow(dead_code)]
    pub fn get_debug_guardrails_low_cut(&self) -> f32 {
        f32::from_bits(self.debug_guardrails_low_cut.load(Ordering::Relaxed))
    }

    pub fn set_debug_guardrails_high_cut(&self, val: f32) {
        self.debug_guardrails_high_cut
            .store(val.to_bits(), Ordering::Relaxed);
    }

    #[allow(dead_code)]
    pub fn get_debug_guardrails_high_cut(&self) -> f32 {
        f32::from_bits(self.debug_guardrails_high_cut.load(Ordering::Relaxed))
    }

    pub fn set_debug_noise_floor_db(&self, val: f32) {
        self.debug_noise_floor_db
            .store(val.to_bits(), Ordering::Relaxed);
    }

    #[allow(dead_code)]
    pub fn get_debug_noise_floor_db(&self) -> f32 {
        f32::from_bits(self.debug_noise_floor_db.load(Ordering::Relaxed))
    }

    pub fn set_debug_expander_atten_db(&self, val: f32) {
        self.debug_expander_atten_db
            .store(val.to_bits(), Ordering::Relaxed);
    }

    #[allow(dead_code)]
    pub fn get_debug_expander_atten_db(&self) -> f32 {
        f32::from_bits(self.debug_expander_atten_db.load(Ordering::Relaxed))
    }

    // =========================================================================
    // Pump Detection Meters
    // =========================================================================

    pub fn increment_pump_event(&self) {
        self.pump_event_count.fetch_add(1, Ordering::Relaxed);
    }

    #[allow(dead_code)]
    pub fn get_pump_event_count(&self) -> i32 {
        self.pump_event_count.load(Ordering::Relaxed)
    }

    pub fn set_pump_severity_db(&self, val: f32) {
        self.pump_severity_db.store(val, Ordering::Relaxed);
    }

    #[allow(dead_code)]
    pub fn get_pump_severity_db(&self) -> f32 {
        self.pump_severity_db.load(Ordering::Relaxed)
    }

    pub fn set_compressor_gain_delta_db(&self, val: f32) {
        self.compressor_gain_delta_db.store(val, Ordering::Relaxed);
    }

    #[allow(dead_code)]
    pub fn get_compressor_gain_delta_db(&self) -> f32 {
        self.compressor_gain_delta_db.load(Ordering::Relaxed)
    }

    pub fn reset(&self) {
        self.input_peak_l.store(0.0f32.to_bits(), Ordering::Relaxed);
        self.input_peak_r.store(0.0f32.to_bits(), Ordering::Relaxed);
        self.output_peak_l
            .store(0.0f32.to_bits(), Ordering::Relaxed);
        self.output_peak_r
            .store(0.0f32.to_bits(), Ordering::Relaxed);
        self.gain_reduction_l
            .store(0.0f32.to_bits(), Ordering::Relaxed);
        self.gain_reduction_r
            .store(0.0f32.to_bits(), Ordering::Relaxed);

        self.debug_speech_confidence
            .store(0.0f32.to_bits(), Ordering::Relaxed);
        self.debug_deesser_gr_db
            .store(0.0f32.to_bits(), Ordering::Relaxed);
        self.debug_limiter_gr_db
            .store(0.0f32.to_bits(), Ordering::Relaxed);
        self.debug_early_reflection
            .store(0.0f32.to_bits(), Ordering::Relaxed);
        self.debug_guardrails_low_cut
            .store(0.0f32.to_bits(), Ordering::Relaxed);
        self.debug_guardrails_high_cut
            .store(0.0f32.to_bits(), Ordering::Relaxed);
        self.debug_noise_floor_db
            .store(0.0f32.to_bits(), Ordering::Relaxed);
        self.debug_expander_atten_db
            .store(0.0f32.to_bits(), Ordering::Relaxed);

        self.noise_reduction_resolved.store(0.0, Ordering::Relaxed);
        self.noise_tone_resolved.store(0.0, Ordering::Relaxed);
        self.deverb_resolved.store(0.0, Ordering::Relaxed);
        self.clarity_resolved.store(0.0, Ordering::Relaxed);
        self.deesser_resolved.store(0.0, Ordering::Relaxed);
        self.proximity_resolved.store(0.0, Ordering::Relaxed);
        self.leveler_resolved.store(0.0, Ordering::Relaxed);
        self.breath_reduction_resolved.store(0.0, Ordering::Relaxed);

        self.loudness_comp_db.store(0.0, Ordering::Relaxed);
        self.loudness_error_db.store(0.0, Ordering::Relaxed);
        self.loudness_active.store(0, Ordering::Relaxed);
        self.speech_band_loss_db.store(0.0, Ordering::Relaxed);
        self.speech_protection_active.store(0, Ordering::Relaxed);
        self.speech_protection_scale.store(1.0, Ordering::Relaxed);
        self.energy_budget_active.store(0, Ordering::Relaxed);
        self.energy_budget_scale.store(1.0, Ordering::Relaxed);

        self.output_rms_db.store(-80.0, Ordering::Relaxed);
        self.output_peak_db.store(-80.0, Ordering::Relaxed);
        self.output_crest_db.store(0.0, Ordering::Relaxed);
        self.total_gain_reduction_db.store(0.0, Ordering::Relaxed);

        self.mode_transition_event.store(0, Ordering::Relaxed);
        self.params_hash_before.store(0, Ordering::Relaxed);
        self.params_hash_after.store(0, Ordering::Relaxed);
        self.audible_change_detected.store(0, Ordering::Relaxed);
        self.pre_switch_audible_rms.store(-80.0, Ordering::Relaxed);
    }

    pub fn set_dtln_available(&self, available: bool) {
        self.dtln_available
            .store(if available { 1 } else { 0 }, Ordering::Relaxed);
    }

    pub fn is_dtln_available(&self) -> bool {
        self.dtln_available.load(Ordering::Relaxed) != 0
    }
}
