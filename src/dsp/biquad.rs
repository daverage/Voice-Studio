//! Biquad Filter Implementation (IIR 2nd Order)
//!
//! A standard second-order recursive filter used throughout the audio processing chain
//! for various equalization and filtering tasks. Provides efficient implementation
//! of common filter types including low-pass, high-pass, shelving, and peaking filters.
//!
//! # Design Notes
//! - Optimized for real-time audio processing with minimal CPU overhead
//! - Coefficient updates are designed to be smooth to prevent audible artifacts
//! - All operations are safe for the audio thread (no allocations)

use std::f32::consts::PI;

/// Biquad filter implementation (IIR 2nd order)
#[derive(Debug, Clone, Copy)]
pub struct Biquad {
    a0: f32,
    a1: f32,
    a2: f32,
    b1: f32,
    b2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    pub fn new() -> Self {
        Self {
            a0: 1.0,
            a1: 0.0,
            a2: 0.0,
            b1: 0.0,
            b2: 0.0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    /// Process a single sample
    #[inline]
    pub fn process(&mut self, input: f32) -> f32 {
        let out = input * self.a0 + self.z1;

        // Anti-denormal: tiny DC offset
        self.z1 = input * self.a1 + self.z2 - self.b1 * out + 1e-25;
        self.z2 = input * self.a2 - self.b2 * out + 1e-25;

        out
    }

    /// Explicitly clear filter delay state.
    ///
    /// IMPORTANT:
    /// - This is NOT called automatically by coefficient updates.
    /// - Use this when resetting analyzers, starting new captures,
    ///   or when you want deterministic behavior across runs.
    #[inline]
    #[allow(dead_code)]
    pub fn reset_state(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    /// Alias for reset_state() for API consistency
    #[inline]
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.reset_state();
    }

    // ---------------------------------------------------------------------
    // Filter design helpers (RBJ-style)
    // ---------------------------------------------------------------------

    pub fn update_hpf(&mut self, cutoff: f32, q: f32, sr: f32) {
        let w0 = 2.0 * PI * cutoff / sr;
        let alpha = w0.sin() / (2.0 * q.max(1e-6));
        let cw0 = w0.cos();

        let a0 = 1.0 + alpha;
        let inv_a0 = 1.0 / a0;

        self.a0 = ((1.0 + cw0) * 0.5) * inv_a0;
        self.a1 = -(1.0 + cw0) * inv_a0;
        self.a2 = ((1.0 + cw0) * 0.5) * inv_a0;
        self.b1 = (-2.0 * cw0) * inv_a0;
        self.b2 = (1.0 - alpha) * inv_a0;
    }

    pub fn update_lpf(&mut self, cutoff: f32, q: f32, sr: f32) {
        let w0 = 2.0 * PI * cutoff / sr;
        let alpha = w0.sin() / (2.0 * q.max(1e-6));
        let cw0 = w0.cos();

        let a0 = 1.0 + alpha;
        let inv_a0 = 1.0 / a0;

        self.a0 = ((1.0 - cw0) * 0.5) * inv_a0;
        self.a1 = (1.0 - cw0) * inv_a0;
        self.a2 = ((1.0 - cw0) * 0.5) * inv_a0;
        self.b1 = (-2.0 * cw0) * inv_a0;
        self.b2 = (1.0 - alpha) * inv_a0;
    }

    pub fn update_low_shelf(&mut self, cutoff: f32, q: f32, gain_db: f32, sr: f32) {
        // Bypass when effectively flat
        if gain_db.abs() < 0.01 {
            self.a0 = 1.0;
            self.a1 = 0.0;
            self.a2 = 0.0;
            self.b1 = 0.0;
            self.b2 = 0.0;
            return;
        }

        let a = 10.0_f32.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * cutoff / sr;
        let alpha = w0.sin() / (2.0 * q.max(1e-6));
        let cw0 = w0.cos();
        let sqrt_a = a.sqrt();

        let b0 = a * ((a + 1.0) - (a - 1.0) * cw0 + 2.0 * sqrt_a * alpha);
        let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cw0);
        let b2 = a * ((a + 1.0) - (a - 1.0) * cw0 - 2.0 * sqrt_a * alpha);

        let a0 = (a + 1.0) + (a - 1.0) * cw0 + 2.0 * sqrt_a * alpha;
        let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cw0);
        let a2 = (a + 1.0) + (a - 1.0) * cw0 - 2.0 * sqrt_a * alpha;

        let inv_a0 = 1.0 / a0;

        self.a0 = b0 * inv_a0;
        self.a1 = b1 * inv_a0;
        self.a2 = b2 * inv_a0;
        self.b1 = a1 * inv_a0;
        self.b2 = a2 * inv_a0;
    }

    pub fn update_high_shelf(&mut self, cutoff: f32, q: f32, gain_db: f32, sr: f32) {
        // Bypass when effectively flat
        if gain_db.abs() < 0.01 {
            self.a0 = 1.0;
            self.a1 = 0.0;
            self.a2 = 0.0;
            self.b1 = 0.0;
            self.b2 = 0.0;
            return;
        }

        let a = 10.0_f32.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * cutoff / sr;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();

        // Shelf slope (RBJ S parameter)
        let s = q.max(1e-6);
        let alpha = sin_w0 * 0.5 * ((a + 1.0 / a) * (1.0 / s - 1.0) + 2.0).sqrt();

        let b0 = a * ((a + 1.0) + (a - 1.0) * cos_w0 + 2.0 * alpha);
        let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0);
        let b2 = a * ((a + 1.0) + (a - 1.0) * cos_w0 - 2.0 * alpha);

        let a0 = (a + 1.0) - (a - 1.0) * cos_w0 + 2.0 * alpha;
        let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos_w0);
        let a2 = (a + 1.0) - (a - 1.0) * cos_w0 - 2.0 * alpha;

        let inv_a0 = 1.0 / a0;

        self.a0 = b0 * inv_a0;
        self.a1 = b1 * inv_a0;
        self.a2 = b2 * inv_a0;
        self.b1 = a1 * inv_a0;
        self.b2 = a2 * inv_a0;
    }

    pub fn update_peaking(&mut self, cutoff: f32, q: f32, gain_db: f32, sr: f32) {
        if gain_db.abs() < 0.01 {
            self.a0 = 1.0;
            self.a1 = 0.0;
            self.a2 = 0.0;
            self.b1 = 0.0;
            self.b2 = 0.0;
            return;
        }

        let a = 10.0_f32.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * cutoff / sr;
        let alpha = w0.sin() / (2.0 * q.max(1e-6));
        let cw0 = w0.cos();

        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * cw0;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = -2.0 * cw0;
        let a2 = 1.0 - alpha / a;

        let inv_a0 = 1.0 / a0;

        self.a0 = b0 * inv_a0;
        self.a1 = b1 * inv_a0;
        self.a2 = b2 * inv_a0;
        self.b1 = a1 * inv_a0;
        self.b2 = a2 * inv_a0;
    }
}
