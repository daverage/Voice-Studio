//! Pink-Reference Spectral Bias (Hidden, Speech-Aware, Capped)
//!
//! A hidden spectral preconditioning stage that gently nudges the speech portion
//! of the input toward a pink-noise-like long-term tilt (-3 dB/oct).
//!
//! # Purpose
//! Improves stability for downstream denoise, de-ess, clarity, and proximity
//! by ensuring the spectral balance is within a reasonable "natural" range.
//!
//! # Architecture
//! - **Analysis**: Windowed FFT (1024/2048) on Mid channel.
//! - **Metric**: Weighted linear regression of log-magnitude vs log-frequency.
//! - **Correction**: Two gentle shelves (Low @ 250Hz, High @ 4kHz) approximating the tilt diff.
//! - **Safety**:
//!   - Gated by speech confidence (only updates/applies during speech).
//!   - Capped at ±2.0 dB total correction.
//!   - Slow ballistics (2.0s tilt averaging, slow gain smoothing).

use crate::dsp::biquad::Biquad;
use crate::dsp::utils::time_constant_coeff;
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::sync::Arc;

// =============================================================================
// Constants
// =============================================================================

const TARGET_TILT_DB_PER_OCT: f32 = -3.0;
const MAX_CORRECTION_DB: f32 = 2.0;

const F_LOW_HZ: f32 = 150.0;
const F_HIGH_HZ: f32 = 6000.0;

// Shelf frequencies for 2-band approximation
const SHELF_LO_FREQ: f32 = 250.0;
const SHELF_HI_FREQ: f32 = 4000.0;
const SHELF_Q: f32 = 0.707;

// Gate thresholds
const SC_ON: f32 = 0.55;
const SC_FULL: f32 = 0.75;

// Smoothing time constants
const GATE_ATTACK_MS: f32 = 200.0;
const GATE_RELEASE_MS: f32 = 800.0;
const TILT_SMOOTHING_SEC: f32 = 2.0;

const GAIN_ATTACK_MS: f32 = 150.0;
const GAIN_RELEASE_MS: f32 = 600.0;

// =============================================================================
// Module
// =============================================================================

pub struct PinkRefBias {
    sample_rate: f32,

    // FFT State
    fft: Arc<dyn Fft<f32>>,
    input_buffer: Vec<f32>,
    window: Vec<f32>,
    fft_scratch: Vec<Complex<f32>>,
    fft_scratch_buf: Vec<Complex<f32>>,
    write_pos: usize,

    // Analysis State
    tilt_est: f32,    // Current estimated spectral tilt (dB/oct)
    gate_smooth: f32, // Smoothed speech gate [0..1]

    // Filter State
    low_shelf_l: Biquad,
    low_shelf_r: Biquad,
    high_shelf_l: Biquad,
    high_shelf_r: Biquad,

    // Gain Smoothing
    target_lo_db: f32,
    target_hi_db: f32,
    current_lo_db: f32,
    current_hi_db: f32,

    // Coefficients
    gate_att_coeff: f32,
    gate_rel_coeff: f32,
    tilt_coeff: f32,
    gain_att_coeff: f32,
    gain_rel_coeff: f32,

    // Bypass Logic
    consecutive_low_gate_frames: usize,
    is_frozen: bool,

    // Coefficient update counter
    coeff_update_counter: u32,
}

impl PinkRefBias {
    pub fn new(sample_rate: f32) -> Self {
        let mut planner = FftPlanner::new();
        let frame_size = if sample_rate > 50000.0 { 2048 } else { 1024 };
        let fft = planner.plan_fft_forward(frame_size);

        let scratch_len = fft.get_inplace_scratch_len();
        let fft_scratch_buf = vec![Complex::default(); scratch_len];

        let window: Vec<f32> = (0..frame_size)
            .map(|i| {
                let denom = (frame_size - 1) as f32;
                0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / denom).cos())
            })
            .collect();

        let mut low_shelf_l = Biquad::new();
        let mut low_shelf_r = Biquad::new();
        low_shelf_l.update_low_shelf(SHELF_LO_FREQ, SHELF_Q, 0.0, sample_rate);
        low_shelf_r.update_low_shelf(SHELF_LO_FREQ, SHELF_Q, 0.0, sample_rate);

        let mut high_shelf_l = Biquad::new();
        let mut high_shelf_r = Biquad::new();
        high_shelf_l.update_high_shelf(SHELF_HI_FREQ, SHELF_Q, 0.0, sample_rate);
        high_shelf_r.update_high_shelf(SHELF_HI_FREQ, SHELF_Q, 0.0, sample_rate);

        // Calculate smoothing coefficients
        // Note: For frame-rate smoothing (tilt), we adjust tau based on hop time
        let hop_time_sec = (frame_size / 2) as f32 / sample_rate;
        let tilt_coeff = (-hop_time_sec / TILT_SMOOTHING_SEC).exp();

        // For sample-rate smoothing (gate, gain), we use standard formula
        // Actually, gate is updated at frame rate too according to prompt "g_s = alpha*g_s..." in frame domain?
        // Prompt: "Apply smoothing to g... alpha = exp(-1/(tau*sr/H))". sr/H is frame rate.
        // So gate smoothing is per-frame.
        let gate_att_coeff = (-hop_time_sec / (GATE_ATTACK_MS * 0.001)).exp();
        let gate_rel_coeff = (-hop_time_sec / (GATE_RELEASE_MS * 0.001)).exp();

        // Gain smoothing is usually per-sample or per-block to avoid zippering.
        // We'll do per-sample for quality.
        let gain_att_coeff = time_constant_coeff(GAIN_ATTACK_MS, sample_rate);
        let gain_rel_coeff = time_constant_coeff(GAIN_RELEASE_MS, sample_rate);

        Self {
            sample_rate,
            fft,
            input_buffer: vec![0.0; frame_size],
            window,
            fft_scratch: vec![Complex::default(); frame_size],
            fft_scratch_buf,
            write_pos: 0,

            tilt_est: TARGET_TILT_DB_PER_OCT, // Start neutral
            gate_smooth: 0.0,

            low_shelf_l,
            low_shelf_r,
            high_shelf_l,
            high_shelf_r,

            target_lo_db: 0.0,
            target_hi_db: 0.0,
            current_lo_db: 0.0,
            current_hi_db: 0.0,

            gate_att_coeff,
            gate_rel_coeff,
            tilt_coeff,
            gain_att_coeff,
            gain_rel_coeff,

            consecutive_low_gate_frames: 0,
            is_frozen: true, // Start frozen at 0
            coeff_update_counter: 0,
        }
    }

    /// Process a stereo sample pair.
    /// Buffers analysis and applies filter.
    #[inline]
    pub fn process(
        &mut self,
        l: f32,
        r: f32,
        speech_confidence: f32,
        proximity_amt: f32,
        deess_amt: f32,
    ) -> (f32, f32) {
        // 1. Buffer for Analysis (Mid channel)
        let mid = 0.5 * (l + r);
        self.input_buffer[self.write_pos] = mid;
        self.write_pos += 1;

        // 2. Run Analysis Frame (Hop)
        if self.write_pos >= self.input_buffer.len() {
            // Shift buffer (50% overlap)
            let hop = self.input_buffer.len() / 2;

            // Analyze current full buffer
            self.analyze_frame(speech_confidence);

            // Rotate: move second half to first half
            self.input_buffer.copy_within(hop.., 0);
            self.write_pos = hop;
        }

        // 3. Update Filter Gains (Smoothly)
        self.update_gains(proximity_amt, deess_amt);

        // 4. Apply Filters
        // Note: Filter coefficients are updated in update_gains only when changed significantly
        // or we can update them every sample. Updating Biquad coeffs is cheap enough.

        // Increment coefficient update counter
        self.coeff_update_counter = self.coeff_update_counter.wrapping_add(1);

        // Apply Low Shelf (separate filters per channel to avoid channel coupling)
        let l_lo = self.low_shelf_l.process(l);
        let r_lo = self.low_shelf_r.process(r);

        // Apply High Shelf (separate filters per channel to avoid channel coupling)
        let l_out = self.high_shelf_l.process(l_lo);
        let r_out = self.high_shelf_r.process(r_lo);

        (l_out, r_out)
    }

    fn analyze_frame(&mut self, speech_conf: f32) {
        let n = self.input_buffer.len();
        let sr = self.sample_rate;

        // 1. Prepare FFT Input (Windowed)
        for i in 0..n {
            self.fft_scratch[i] = Complex::new(self.input_buffer[i] * self.window[i], 0.0);
        }

        // 2. Perform FFT
        self.fft
            .process_with_scratch(&mut self.fft_scratch, &mut self.fft_scratch_buf);

        // 3. Compute Power Spectrum & Tilt
        // Regress S[k] vs log2(f_k)

        let mut sum_w = 0.0;
        let mut sum_wx = 0.0;
        let mut sum_wy = 0.0;
        let mut sum_wxx = 0.0; // Weighted x^2
        let mut sum_wxy = 0.0; // Weighted x*y

        // Band limits in bins
        let bin_hz = sr / n as f32;
        let start_bin = (F_LOW_HZ / bin_hz).max(1.0) as usize;
        let end_bin = (F_HIGH_HZ / bin_hz).min((n / 2) as f32) as usize;

        let f0 = 1000.0f32;
        let log_f0 = f0.log2();
        let bw_log = (6000.0f32 / 150.0).log2();

        for k in start_bin..end_bin {
            let re = self.fft_scratch[k].re;
            let im = self.fft_scratch[k].im;
            let p = re * re + im * im;
            let s_k = 10.0 * (p + 1e-9).log10(); // dB

            let f_k = k as f32 * bin_hz;
            let x_k = f_k.log2();

            // Weighting
            // w_band(k) triangular around 1k
            let dist = (x_k - log_f0).abs();
            let w_band = (1.0 - dist / bw_log).clamp(0.0, 1.0);

            // Accumulate for regression
            // x_bar and y_bar method is numerically roughly equiv to sum method
            // sum(w * (x - x_bar) * (y - y_bar)) = sum(wxy) - sum(wx)*sum(wy)/sum(w)

            let w = w_band; // We'll apply speech gate later globally

            sum_w += w;
            sum_wx += w * x_k;
            sum_wy += w * s_k;
            sum_wxx += w * x_k * x_k;
            sum_wxy += w * x_k * s_k;
        }

        // Calculate Tilt (slope)
        // Slope = (sum(wxy) - sum(wx)*sum(wy)/sum(w)) / (sum(wxx) - sum(wx)^2/sum(w))

        let t_meas = if sum_w > 1e-6 {
            let mean_x = sum_wx / sum_w;
            let mean_y = sum_wy / sum_w;

            // More stable variance form
            // var_x = sum(w * (x - mean_x)^2)
            // cov = sum(w * (x - mean_x) * (y - mean_y))

            // Re-looping is safer for precision but let's use the expanded form with safeguards
            let num = sum_wxy - sum_wx * mean_y; // or sum_wxy - sum_w * mean_x * mean_y
            let den = sum_wxx - sum_wx * mean_x;

            if den.abs() > 1e-9 {
                num / den
            } else {
                TARGET_TILT_DB_PER_OCT // Fallback
            }
        } else {
            TARGET_TILT_DB_PER_OCT
        };

        // 4. Update Gate
        // Map [0.55 .. 0.75] to [0 .. 1]
        let raw_gate = ((speech_conf - SC_ON) / (SC_FULL - SC_ON)).clamp(0.0, 1.0);

        // Smooth Gate
        if raw_gate > self.gate_smooth {
            self.gate_smooth =
                self.gate_att_coeff * self.gate_smooth + (1.0 - self.gate_att_coeff) * raw_gate;
        } else {
            self.gate_smooth =
                self.gate_rel_coeff * self.gate_smooth + (1.0 - self.gate_rel_coeff) * raw_gate;
        }

        // Freeze logic
        if self.gate_smooth < 0.05 {
            self.consecutive_low_gate_frames += 1;
            // ~1 second at ~21ms frames (1024/48k) -> ~46 frames
            if self.consecutive_low_gate_frames > 50 {
                self.is_frozen = true;
            }
        } else {
            self.consecutive_low_gate_frames = 0;
            self.is_frozen = false;
        }

        // 5. Update Tilt Estimate
        // Only update tilt if we are confident it's speech (gate > 0.1) and not frozen
        if self.gate_smooth > 0.1 && !self.is_frozen {
            // Outlier rejection
            let t_meas_safe = t_meas.clamp(-12.0, 12.0);
            self.tilt_est = self.tilt_coeff * self.tilt_est + (1.0 - self.tilt_coeff) * t_meas_safe;
        }

        // 6. Compute Target Correction Gains
        // Error
        let e = TARGET_TILT_DB_PER_OCT - self.tilt_est;

        // Calculate gain at endpoints 200Hz and 5000Hz relative to 1000Hz pivot
        // G_db(f) = e * log2(f / 1000)

        let oct_lo = (200.0f32 / 1000.0).log2(); // approx -2.32
        let oct_hi = (5000.0f32 / 1000.0).log2(); // approx 2.32

        let g_lo_raw = e * oct_lo;
        let g_hi_raw = e * oct_hi;

        // Clamp total swing [-2, 2]
        let g_lo_clamped = g_lo_raw.clamp(-MAX_CORRECTION_DB, MAX_CORRECTION_DB);
        let g_hi_clamped = g_hi_raw.clamp(-MAX_CORRECTION_DB, MAX_CORRECTION_DB);

        // Apply Gate scaling
        // "G_db_final(f) = g_s * clamp(...)"
        // SAFETY: If speech confidence is marginal (< 0.5), force gain to 0.0 to prevent "breathing" on noise
        let safe_gate = if speech_conf < 0.5 { 0.0 } else { self.gate_smooth };

        let g_lo_final = safe_gate * g_lo_clamped;
        let g_hi_final = safe_gate * g_hi_clamped;

        // Map to shelves (Low * 0.9, High * 1.0)
        self.target_lo_db = g_lo_final * 0.9;
        self.target_hi_db = g_hi_final * 1.0;

        // If frozen, force decay to 0
        if self.is_frozen {
            self.target_lo_db = 0.0;
            self.target_hi_db = 0.0;
        }
    }

    fn update_gains(&mut self, proximity_amt: f32, deess_amt: f32) {
        // Safety adjustments based on interactions
        let mut safe_target_lo = self.target_lo_db;
        let mut safe_target_hi = self.target_hi_db;

        // "If proximity slider is high... reduce G_max by 25%"
        // We'll scale down low boost if proximity is high
        if proximity_amt > 0.5 {
            safe_target_lo *= 0.75; // ok to keep as-is if you only want to avoid extra warmth
        }

        // "If de-esser is engaged strongly... reduce high shelf gain by 30-50%"
        // Applies to both + and - to prevent additive harshness
        if deess_amt > 0.5 {
            safe_target_hi *= 0.7; // applies to both + and -
        }

        // Smooth current gains towards targets
        // Simple one-pole per sample (using small coeff)
        let coeff = if safe_target_lo.abs() > self.current_lo_db.abs() {
            self.gain_att_coeff
        } else {
            self.gain_rel_coeff
        };
        self.current_lo_db = coeff * self.current_lo_db + (1.0 - coeff) * safe_target_lo;

        let coeff_h = if safe_target_hi.abs() > self.current_hi_db.abs() {
            self.gain_att_coeff
        } else {
            self.gain_rel_coeff
        };
        self.current_hi_db = coeff_h * self.current_hi_db + (1.0 - coeff_h) * safe_target_hi;

        // Update Biquads
        // Only update if significant change to save CPU? Or every block.
        // We are in process(), which handles 1 sample? No, we call this once per sample in process() wrapper?
        // Wait, the design says "Update at most once per block or once per FFT hop".
        // But `process` is per sample.
        // Actually, `update_gains` is called every sample in my `process` loop.
        // Biquad calc has sin/cos. Doing this per sample is expensive.
        // I should probably rate-limit filter coefficient updates.
        // e.g. every 16 or 32 samples.
        // But for simplicity and smoothness, per-sample smoothing on GAIN is good,
        // but maybe update coefficients every 32 samples?
        // Let's do every 32 samples.

        // Use dedicated counter instead of write_pos which resets
        if (self.coeff_update_counter & 31) == 0 {
            // every 32 samples
            self.low_shelf_l.update_low_shelf(
                SHELF_LO_FREQ,
                SHELF_Q,
                self.current_lo_db,
                self.sample_rate,
            );
            self.low_shelf_r.update_low_shelf(
                SHELF_LO_FREQ,
                SHELF_Q,
                self.current_lo_db,
                self.sample_rate,
            );

            self.high_shelf_l.update_high_shelf(
                SHELF_HI_FREQ,
                SHELF_Q,
                self.current_hi_db,
                self.sample_rate,
            );
            self.high_shelf_r.update_high_shelf(
                SHELF_HI_FREQ,
                SHELF_Q,
                self.current_hi_db,
                self.sample_rate,
            );
        }
    }

    pub fn reset(&mut self) {
        self.input_buffer.fill(0.0);
        self.write_pos = 0;
        self.tilt_est = TARGET_TILT_DB_PER_OCT;
        self.gate_smooth = 0.0;
        self.low_shelf_l.reset();
        self.low_shelf_r.reset();
        self.high_shelf_l.reset();
        self.high_shelf_r.reset();
        self.target_lo_db = 0.0;
        self.target_hi_db = 0.0;
        self.current_lo_db = 0.0;
        self.current_hi_db = 0.0;
        self.consecutive_low_gate_frames = 0;
        self.is_frozen = true;
        self.coeff_update_counter = 0;
    }
}
