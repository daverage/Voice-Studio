//! Output Limiter
//!
//! # Perceptual Contract
//! - **Target Source**: Final output signal.
//! - **Intended Effect**: Catch true peaks to prevent digital clipping (-0.1 dBTP).
//! - **Failure Modes**:
//!   - Audible distortion/crunch if driven too hard (>6dB gain reduction).
//! - **Will Not Do**:
//!   - Color the sound (transparent design).
//!   - Provide "glue" compression (this is purely for safety).
//!
//! # Lifecycle
//! - **Active**: Normal operation.
//! - **Bypassed**: Passes audio through (unsafe!).

use crate::dsp::utils::{db_to_lin, lin_to_db, time_constant_coeff, update_env_sq, DB_EPS};

/// Stereo-linked VO limiter using hybrid RMS + peak detection.
/// Drop-in replacement for LinkedLimiter.
pub struct LinkedLimiter {
    // RMS envelope (squared) per channel
    env_sq_l: f32,
    env_sq_r: f32,

    // Peak envelope (linear)
    peak_env_l: f32,
    peak_env_r: f32,

    // Smoothed applied gain
    gain_smooth: f32,

    sample_rate: f32,
}

impl LinkedLimiter {
    pub fn new(sr: f32) -> Self {
        Self {
            env_sq_l: 0.0,
            env_sq_r: 0.0,
            peak_env_l: 0.0,
            peak_env_r: 0.0,
            gain_smooth: 1.0,
            sample_rate: sr,
        }
    }

    #[inline]
    fn coeff(&self, ms: f32) -> f32 {
        time_constant_coeff(ms, self.sample_rate)
    }

    pub fn compute_gain(&mut self, input_l: f32, input_r: f32) -> f32 {
        let abs_l = input_l.abs();
        let abs_r = input_r.abs();

        // --------------------------------------------------
        // 1. RMS detector (stable loudness reference)
        // --------------------------------------------------
        let rms_atk = self.coeff(10.0);
        let rms_rel = self.coeff(120.0);

        let sq_l = abs_l * abs_l;
        let sq_r = abs_r * abs_r;

        self.env_sq_l = update_env_sq(self.env_sq_l, sq_l, rms_atk, rms_rel);
        self.env_sq_r = update_env_sq(self.env_sq_r, sq_r, rms_atk, rms_rel);

        let rms = self.env_sq_l.max(self.env_sq_r).sqrt().max(DB_EPS);

        // --------------------------------------------------
        // 2. Peak detector (plosive / transient safety)
        // --------------------------------------------------
        let peak_atk = self.coeff(0.3); // extremely fast
        let peak_rel = self.coeff(50.0); // smooth recovery

        self.peak_env_l = if abs_l > self.peak_env_l {
            peak_atk * self.peak_env_l + (1.0 - peak_atk) * abs_l
        } else {
            peak_rel * self.peak_env_l + (1.0 - peak_rel) * abs_l
        };

        self.peak_env_r = if abs_r > self.peak_env_r {
            peak_atk * self.peak_env_r + (1.0 - peak_atk) * abs_r
        } else {
            peak_rel * self.peak_env_r + (1.0 - peak_rel) * abs_r
        };

        let peak = self.peak_env_l.max(self.peak_env_r).max(DB_EPS);

        // --------------------------------------------------
        // 3. Hybrid detection
        // --------------------------------------------------
        let hybrid = (0.7 * rms + 0.3 * peak).max(DB_EPS);

        // --------------------------------------------------
        // 4. Soft limiting curve
        // --------------------------------------------------
        let ceiling = 0.98;
        let knee_db = 1.5;

        let env_db = lin_to_db(hybrid);
        let ceiling_db = lin_to_db(ceiling);
        let over_db = env_db - ceiling_db;

        let target_gain = if over_db <= -knee_db * 0.5 {
            1.0
        } else if over_db >= knee_db * 0.5 {
            db_to_lin(-over_db)
        } else {
            // soft knee
            let x = over_db + knee_db * 0.5;
            let y = (x * x) / (2.0 * knee_db);
            db_to_lin(-y)
        };

        // --------------------------------------------------
        // 5. Gain smoothing (no pumping)
        // --------------------------------------------------
        let atk = self.coeff(1.0);
        let rel = self.coeff(80.0);

        if target_gain < self.gain_smooth {
            self.gain_smooth = atk * self.gain_smooth + (1.0 - atk) * target_gain;
        } else {
            self.gain_smooth = rel * self.gain_smooth + (1.0 - rel) * target_gain;
        }

        self.gain_smooth
    }

    /// Get current gain reduction in dB (for metering)
    #[allow(dead_code)]
    pub fn get_gain_reduction_db(&self) -> f32 {
        lin_to_db(self.gain_smooth).abs()
    }
}
