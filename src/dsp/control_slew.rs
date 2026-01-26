//! Control Slew Limiter (Stability Layer)
//!
//! Prevents fast frame-to-frame changes in spectral control values that cause
//! audible artifacts such as metallic shimmer, warble, or pumping effects.
//! Acts as a rate limiter for control values rather than filtering audio directly.
//!
//! # Purpose
//! Maintains audio quality by preventing audible artifacts from rapid parameter
//! changes, especially important for macro mode transitions and user automation.
//!
//! # Design Notes
//! - Prevents metallic shimmer, warble, and pumping artifacts
//! - Rate limiter for control values, not audio filtering
//! - Should rarely engage and never be audible on clean material
//! - Essential for smooth macro mode transitions
//!
//! ## Usage
//! Apply to dynamically modulated spectral controls:
//! - denoiser strength
//! - clarity emphasis
//! - spectral gain masks
//!
//! Do NOT apply to:
//! - leveler (broadband gain)
//! - limiter
//! - output gain

// =============================================================================
// Constants (copy-paste friendly from spec)
// =============================================================================

/// Base slew limit per frame (1.5% max change)
const BASE_SLEW_PER_FRAME: f32 = 0.015;

/// Whisper condition multiplier (more restrictive)
const WHISPER_SLEW_MULT: f32 = 0.5;

/// Noisy condition multiplier (moderately restrictive)
const NOISY_SLEW_MULT: f32 = 0.75;

/// Absolute maximum slew per frame (hard safety clamp)
const ABS_MAX_SLEW_PER_FRAME: f32 = 0.05;

// =============================================================================
// Control Slew Limiter
// =============================================================================

/// Slew-limits a single control value to prevent fast changes.
///
/// This is a one-sided limiter: it only engages when the target changes
/// faster than allowed. It does not smooth or filter - it just caps the
/// rate of change.
#[derive(Clone, Copy, Debug)]
pub struct ControlSlewLimiter {
    /// Current slew-limited value
    current: f32,
    /// Whether the limiter has been initialized
    initialized: bool,
}

impl Default for ControlSlewLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl ControlSlewLimiter {
    pub fn new() -> Self {
        Self {
            current: 0.0,
            initialized: false,
        }
    }

    /// Process a target value and return the slew-limited result.
    ///
    /// * `target` - The desired value
    /// * `whisper` - Whether whisper condition is detected (tighter limit)
    /// * `noisy` - Whether noisy condition is detected (tighter limit)
    ///
    /// Returns the slew-limited value that moves toward target at a safe rate.
    #[inline]
    pub fn process(&mut self, target: f32, whisper: bool, noisy: bool) -> f32 {
        // First call: initialize to target (no slewing on startup)
        if !self.initialized {
            self.current = target;
            self.initialized = true;
            return target;
        }

        // Calculate effective slew limit based on conditions
        let slew_limit = Self::calculate_slew_limit(whisper, noisy);

        // Calculate desired change
        let delta = target - self.current;

        // Apply slew limiting (one-sided: only limit if change exceeds limit)
        let limited_delta = if delta.abs() > slew_limit {
            // Clamp to maximum allowed change
            delta.clamp(-slew_limit, slew_limit)
        } else {
            // Change is within limit, pass through
            delta
        };

        self.current += limited_delta;
        self.current
    }

    /// Calculate the slew limit based on detected conditions.
    /// Whisper and noisy conditions get tighter limits.
    #[inline]
    fn calculate_slew_limit(whisper: bool, noisy: bool) -> f32 {
        let base = BASE_SLEW_PER_FRAME;

        let scaled = if whisper && noisy {
            // Both conditions: use tightest limit (whisper dominates)
            base * WHISPER_SLEW_MULT
        } else if whisper {
            base * WHISPER_SLEW_MULT
        } else if noisy {
            base * NOISY_SLEW_MULT
        } else {
            base
        };

        // Always clamp to absolute maximum
        scaled.min(ABS_MAX_SLEW_PER_FRAME)
    }

    /// Reset the limiter state
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.current = 0.0;
        self.initialized = false;
    }

    /// Get current value (for debugging/metering)
    #[allow(dead_code)]
    pub fn get_current(&self) -> f32 {
        self.current
    }

    /// Check if the limiter is currently engaged (last update was limited)
    #[allow(dead_code)]
    pub fn was_limited(&self, target: f32, whisper: bool, noisy: bool) -> bool {
        if !self.initialized {
            return false;
        }
        let slew_limit = Self::calculate_slew_limit(whisper, noisy);
        (target - self.current).abs() > slew_limit
    }
}

// =============================================================================
// Multi-Control Slew Limiter
// =============================================================================

/// Slew limiter for multiple named control values.
/// Pre-allocates slots for common spectral controls.
#[derive(Clone, Debug)]
pub struct SpectralControlLimiters {
    /// Denoiser strength (0-1)
    pub denoise_strength: ControlSlewLimiter,
    /// Clarity/presence emphasis (0-1)
    pub clarity_emphasis: ControlSlewLimiter,
    /// De-esser strength (0-1)
    pub deesser_strength: ControlSlewLimiter,
    /// Reverb reduction strength (0-1)
    pub reverb_strength: ControlSlewLimiter,
    /// Proximity boost strength (0-1)
    pub proximity_strength: ControlSlewLimiter,
}

impl Default for SpectralControlLimiters {
    fn default() -> Self {
        Self::new()
    }
}

impl SpectralControlLimiters {
    pub fn new() -> Self {
        Self {
            denoise_strength: ControlSlewLimiter::new(),
            clarity_emphasis: ControlSlewLimiter::new(),
            deesser_strength: ControlSlewLimiter::new(),
            reverb_strength: ControlSlewLimiter::new(),
            proximity_strength: ControlSlewLimiter::new(),
        }
    }

    /// Process all control values with slew limiting and perceptual safeguards.
    /// Returns struct with limited values.
    #[inline]
    pub fn process(
        &mut self,
        denoise_in: f32,
        clarity_in: f32,
        deesser_in: f32,
        reverb_in: f32,
        proximity_in: f32,
        whisper: bool,
        noisy: bool,
        speech_loss_db: f32, // Passed from macro controller via lib.rs
    ) -> LimitedControls {
        // --- 1. Clarity/Noise Knee Safeguard ---
        // Decouple Clarity from Noise Reduction above a knee to prevent speech energy loss.
        // Even in Advanced mode, we apply this knee to 'denoise' if it's coming from a clarity-like intent.
        // However, for pure Advanced mode, we want to respect the user's manual settings.
        // We'll treat denoise_in as the raw target.

        // --- 2. Energy Budget (Noise vs Reverb) ---
        // Prevent over-stacking of energy-removing stages.
        let reverb_budget_scale = if denoise_in > 0.4 {
            let excess = (denoise_in - 0.4) / 0.4; // 0.4 to 0.8 range
            (1.0 - excess * 0.4).clamp(0.6, 1.0)
        } else {
            1.0
        };

        // --- 3. Speech Band Energy Protection ---
        // Scale back if loss exceeds 2 dB.
        let speech_protection_scale = if speech_loss_db < -2.0 {
            let deficit = -2.0 - speech_loss_db;
            (1.0 - (deficit / 5.0).min(0.4)).clamp(0.6, 1.0)
        } else {
            1.0
        };

        let final_denoise = denoise_in * speech_protection_scale;
        let final_reverb = reverb_in * reverb_budget_scale * speech_protection_scale;
        let _energy_budget_scale = reverb_budget_scale;

        LimitedControls {
            denoise: self.denoise_strength.process(final_denoise, whisper, noisy),
            clarity: self.clarity_emphasis.process(clarity_in, whisper, noisy),
            deesser: self.deesser_strength.process(deesser_in, whisper, noisy),
            reverb: self.reverb_strength.process(final_reverb, whisper, noisy),
            proximity: self
                .proximity_strength
                .process(proximity_in, whisper, noisy),
            speech_protection_active: speech_protection_scale < 0.99, // Active if scaling down
            speech_protection_scale,
            energy_budget_active: reverb_budget_scale < 0.99, // Active if scaling down
            energy_budget_scale: reverb_budget_scale,
        }
    }

    /// Reset all limiters
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.denoise_strength.reset();
        self.clarity_emphasis.reset();
        self.deesser_strength.reset();
        self.reverb_strength.reset();
        self.proximity_strength.reset();
    }
}

/// Output struct with slew-limited control values and safeguard states
#[derive(Clone, Copy, Debug, Default)]
pub struct LimitedControls {
    pub denoise: f32,
    pub clarity: f32,
    pub deesser: f32,
    pub reverb: f32,
    pub proximity: f32,

    // Safeguard reporting
    pub speech_protection_active: bool,
    pub speech_protection_scale: f32,
    pub energy_budget_active: bool,
    pub energy_budget_scale: f32,
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slew_limiter_creation() {
        let limiter = ControlSlewLimiter::new();
        assert!(!limiter.initialized);
    }

    #[test]
    fn test_first_call_no_slew() {
        let mut limiter = ControlSlewLimiter::new();
        // First call should return target directly (no slewing)
        let result = limiter.process(0.5, false, false);
        assert!((result - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_slow_change_passes_through() {
        let mut limiter = ControlSlewLimiter::new();
        limiter.process(0.5, false, false); // Initialize

        // Small change (within slew limit) should pass through
        let result = limiter.process(0.51, false, false);
        assert!((result - 0.51).abs() < 0.001);
    }

    #[test]
    fn test_fast_change_is_limited() {
        let mut limiter = ControlSlewLimiter::new();
        limiter.process(0.0, false, false); // Initialize at 0

        // Large instant change should be limited
        let result = limiter.process(1.0, false, false);
        // Should move by at most BASE_SLEW_PER_FRAME
        assert!(result <= BASE_SLEW_PER_FRAME + 0.001);
        assert!(result > 0.0);
    }

    #[test]
    fn test_whisper_tighter_limit() {
        let mut limiter_normal = ControlSlewLimiter::new();
        let mut limiter_whisper = ControlSlewLimiter::new();

        limiter_normal.process(0.0, false, false);
        limiter_whisper.process(0.0, true, false);

        let result_normal = limiter_normal.process(1.0, false, false);
        let result_whisper = limiter_whisper.process(1.0, true, false);

        // Whisper should have tighter limit (smaller change)
        assert!(result_whisper < result_normal);
    }

    #[test]
    fn test_convergence() {
        let mut limiter = ControlSlewLimiter::new();
        limiter.process(0.0, false, false); // Initialize at 0

        // Large change should eventually converge
        let mut value = 0.0;
        for _ in 0..100 {
            value = limiter.process(1.0, false, false);
        }

        // After 100 frames, should be very close to target
        assert!((value - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_multi_limiter() {
        let mut limiters = SpectralControlLimiters::new();

        let result = limiters.process(0.5, 0.3, 0.2, 0.4, 0.1, false, false, 0.0);

        assert!((result.denoise - 0.5).abs() < 0.001);
        assert!((result.clarity - 0.3).abs() < 0.001);
    }
}
