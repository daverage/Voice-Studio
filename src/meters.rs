//! Thread-safe metering utilities for real-time audio processing.
//!
//! This module provides atomic float storage for sharing meter data between
//! the audio thread and UI thread without locks. Some getters are currently
//! unused but are kept for debugging and future UI integration.

use std::sync::atomic::{AtomicU32, Ordering};

/// Thread-safe metering for input/output levels and gain reduction.
#[derive(Default)]
pub struct Meters {
    input_peak_l: AtomicU32,
    input_peak_r: AtomicU32,
    output_peak_l: AtomicU32,
    output_peak_r: AtomicU32,
    gain_reduction_l: AtomicU32,
    gain_reduction_r: AtomicU32,
    restoration_delta_rms_db: AtomicU32,
    delta_activity: AtomicU32,

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

    pub fn set_restoration_delta_rms_db(&self, val: f32) {
        self.restoration_delta_rms_db
            .store(val.to_bits(), Ordering::Relaxed);
    }

    pub fn set_delta_activity(&self, val: f32) {
        self.delta_activity.store(val.to_bits(), Ordering::Relaxed);
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

    #[allow(dead_code)]
    pub fn get_restoration_delta_rms_db(&self) -> f32 {
        f32::from_bits(self.restoration_delta_rms_db.load(Ordering::Relaxed))
    }

    pub fn get_delta_activity(&self) -> f32 {
        f32::from_bits(self.delta_activity.load(Ordering::Relaxed))
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
}
