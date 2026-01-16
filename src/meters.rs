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
        self.gain_reduction_l.store(val.to_bits(), Ordering::Relaxed);
    }

    pub fn set_gain_reduction_r(&self, val: f32) {
        self.gain_reduction_r.store(val.to_bits(), Ordering::Relaxed);
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
}
