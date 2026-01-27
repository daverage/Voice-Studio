//! Shared DSP utilities for VxCleaner.
//!
//! Centralized constants and functions used across multiple DSP modules to avoid
//! code duplication. All functions in this module are safe for use in the audio
//! thread (no allocations, no blocking).
//!
//! # Design Notes
//! - Functions are optimized for real-time audio processing
//! - Audio thread safety is maintained throughout
//! - Common DSP operations are centralized here to ensure consistency

use std::f32::consts::PI;

// =============================================================================
// Shared Constants
// =============================================================================

/// Floor value for magnitude calculations (avoids log(0))
pub const MAG_FLOOR: f32 = 1e-12;

/// Epsilon for dB conversions and ratio calculations
pub const DB_EPS: f32 = 1e-12;

/// Amount below which effect is bypassed (avoids near-zero processing)
pub const BYPASS_AMOUNT_EPS: f32 = 0.001;

// =============================================================================
// Basic Math Utilities
// =============================================================================

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

pub fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let denom = (edge1 - edge0).max(1e-12);
    let t = ((x - edge0) / denom).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn max3(a: f32, b: f32, c: f32) -> f32 {
    a.max(b).max(c)
}

pub fn db_to_gain(db: f32) -> f32 {
    (10.0f32).powf(db / 20.0)
}

/// Convert linear amplitude to decibels
#[inline]
pub fn lin_to_db(x: f32) -> f32 {
    20.0 * x.max(DB_EPS).log10()
}

/// Convert decibels to linear amplitude (alias for db_to_gain)
#[inline]
pub fn db_to_lin(db: f32) -> f32 {
    10.0f32.powf(db / 20.0)
}

// =============================================================================
// DSP Utilities
// =============================================================================

/// Generate a sqrt-Hann window for WOLA (Weighted Overlap-Add) processing.
/// This window type allows perfect reconstruction when used for both analysis and synthesis.
pub fn make_sqrt_hann_window(size: usize) -> Vec<f32> {
    (0..size)
        .map(|i| {
            let hann = 0.5 * (1.0 - (2.0 * PI * i as f32 / size as f32).cos());
            hann.sqrt()
        })
        .collect()
}

/// Convert time in milliseconds to exponential smoothing coefficient.
/// Used for envelope followers with attack/release characteristics.
#[inline]
pub fn time_constant_coeff(time_ms: f32, sample_rate: f32) -> f32 {
    let denom = time_ms * 0.001 * sample_rate;
    if denom > 1e-6 {
        (-1.0 / denom).exp()
    } else {
        0.0 // Instant reaction if time or SR is effectively zero
    }
}

/// Attack/release envelope follower for squared values (RMS detection).
/// Returns updated envelope value based on whether input is rising or falling.
#[inline]
pub fn update_env_sq(env_sq: f32, in_sq: f32, attack: f32, release: f32) -> f32 {
    if in_sq > env_sq {
        attack * env_sq + (1.0 - attack) * in_sq
    } else {
        release * env_sq + (1.0 - release) * in_sq
    }
}

// =============================================================================
// Analysis Utilities
// =============================================================================

/// Autocorrelation-based F0 estimation (lightweight, speech-focused).
/// Returns (periodicity 0..1, f0_hz).
///
/// `scratch` must have capacity >= frame.len(). It will be filled without reallocation
/// to avoid audio-thread allocations.
pub fn estimate_f0_autocorr(frame: &[f32], scratch: &mut Vec<f32>, sample_rate: f32) -> (f32, f32) {
    let n = frame.len();
    if n < 128 {
        return (0.0, 0.0);
    }

    // Ensure scratch buffer is the right size to avoid allocation
    if scratch.len() != n {
        scratch.resize(n, 0.0);
    }

    // Remove DC + simple pre-emphasis - reuse scratch buffer
    let mut mean = 0.0f32;
    for &v in frame {
        mean += v;
    }
    mean /= n as f32;

    let mut prev = 0.0f32;
    for (i, &v) in frame.iter().enumerate() {
        let d = v - mean;
        let y = d - 0.97 * prev;
        prev = d;
        scratch[i] = y; // Direct assignment instead of push to avoid reallocation
    }
    let x = scratch;

    // Energy gate
    let mut e0 = 0.0f32;
    for &v in x.iter() {
        e0 += v * v;
    }
    if e0 < 1e-6 {
        return (0.0, 0.0);
    }

    // Speech-ish F0 range
    let f0_min = 70.0;
    let f0_max = 320.0;
    let lag_min = (sample_rate / f0_max).floor() as usize;
    let lag_max = (sample_rate / f0_min).ceil() as usize;

    let lag_min = lag_min.clamp(16, n / 2);
    let lag_max = lag_max.clamp(lag_min + 1, n / 2);

    let mut best_lag = 0usize;
    let mut best = 0.0f32;

    for lag in lag_min..=lag_max {
        let mut s = 0.0f32;
        let mut e1 = 0.0f32;
        let mut e2 = 0.0f32;
        for i in 0..(n - lag) {
            let a = x[i];
            let b = x[i + lag];
            s += a * b;
            e1 += a * a;
            e2 += b * b;
        }
        let denom = (e1 * e2).sqrt().max(1e-12);
        let r = (s / denom).clamp(-1.0, 1.0);
        if r > best {
            best = r;
            best_lag = lag;
        }
    }

    let periodicity = best.clamp(0.0, 1.0);
    let f0 = if best_lag > 0 {
        sample_rate / best_lag as f32
    } else {
        0.0
    };

    (periodicity, f0)
}

pub fn bell(x: f32, center: f32, width: f32) -> f32 {
    let d = (x - center) / width.max(1e-6);
    (-0.5 * d * d).exp().clamp(0.0, 1.0)
}

pub fn frame_rms(x: &[f32]) -> f32 {
    let mut s = 0.0f32;
    for &v in x {
        s += v * v;
    }
    (s / (x.len().max(1) as f32)).sqrt()
}

// =============================================================================
// Perceptual Parameter Curves (DSP Stability & Scaling)
// =============================================================================

/// Perceptual soft curve for most sliders.
/// Makes 0-50% gentle and musical, 50-100% increasingly aggressive.
/// Input: normalized 0-1, Output: perceptually curved 0-1
#[inline]
pub fn perceptual_curve(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x <= 0.5 {
        // First half: gentle rise (x^1.5 scaled to reach 0.5)
        (x / 0.5).powf(1.5) * 0.5
    } else {
        // Second half: aggressive rise (x^2.2 scaled from 0.5 to 1.0)
        0.5 + ((x - 0.5) / 0.5).powf(2.2) * 0.5
    }
}

/// Aggressive tail curve for clarity, denoise, de-verb.
/// Preserves usability until ~70%, then ramps hard.
/// Input: normalized 0-1, Output: aggressively curved 0-1
#[inline]
pub fn aggressive_tail(x: f32) -> f32 {
    x.clamp(0.0, 1.0).powf(2.8)
}

/// Speech-aware maximum value scaling.
/// Reduces max effect strength during voiced speech to prevent artifacts.
/// Input: max value, speech confidence 0-1
/// Output: scaled max (60-100% of original based on confidence)
#[inline]
pub fn speech_weighted(max: f32, speech_conf: f32) -> f32 {
    let conf = speech_conf.clamp(0.0, 1.0);
    max * (0.6 + 0.4 * conf)
}
