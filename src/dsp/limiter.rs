//! Output Limiter
//!
//! True peak safety limiter.
//! Designed to be completely transparent and inert unless the signal
//! exceeds the ceiling. No loudness riding, no pumping.

use crate::dsp::utils::{db_to_lin, lin_to_db, time_constant_coeff, DB_EPS};

pub struct LinkedLimiter {
    // Peak envelope (linear, stereo linked)
    peak_env_l: f32,
    peak_env_r: f32,

    // Smoothed applied gain
    gain_smooth: f32,
    gain_reduction_db: f32,

    sample_rate: f32,
}

impl LinkedLimiter {
    pub fn new(sr: f32) -> Self {
        Self {
            peak_env_l: 0.0,
            peak_env_r: 0.0,
            gain_smooth: 1.0,
            gain_reduction_db: 0.0,
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
        // 1. True peak detector (fast attack, gentle release)
        // --------------------------------------------------
        let peak_atk = self.coeff(0.1); // extremely fast catch
        let peak_rel = self.coeff(60.0); // smooth envelope decay

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
        // 2. Limiting curve (only engages above ceiling)
        // --------------------------------------------------
        let ceiling = 0.98; // ~ -0.18 dBTP
        let knee_db = 1.0;

        let env_db = lin_to_db(peak);
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
        // 3. Gain smoothing (limiter-style)
        // --------------------------------------------------
        let atk = self.coeff(0.5); // fast clamp
        let rel = self.coeff(400.0); // slow, boring recovery

        if target_gain < self.gain_smooth {
            // Gain reduction engages quickly
            self.gain_smooth = atk * self.gain_smooth + (1.0 - atk) * target_gain;
        } else {
            // Release only when safely below ceiling
            self.gain_smooth = rel * self.gain_smooth + (1.0 - rel) * target_gain;
        }

        self.gain_reduction_db = -lin_to_db(self.gain_smooth.max(DB_EPS));

        self.gain_smooth
    }

    pub fn get_gain_reduction_db(&self) -> f32 {
        self.gain_reduction_db
    }

    pub fn reset(&mut self) {
        self.peak_env_l = 0.0;
        self.peak_env_r = 0.0;
        self.gain_smooth = 1.0;
        self.gain_reduction_db = 0.0;
    }
}
