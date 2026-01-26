use crate::dsp::biquad::Biquad;
use crate::dsp::utils::time_constant_coeff;

/// Plosive Softener (Hidden Protection)
///
/// Automatically detects and attenuates low-frequency bursts (P/B sounds)
/// using a fast-acting dynamic high-shelf/high-pass approach.
pub struct PlosiveSoftener {
    // Detection path
    low_env: f32,

    // Filters
    plosive_filter: Biquad,

    sample_rate: f32,
    current_reduction_db: f32,
}

impl PlosiveSoftener {
    // Target detection in the "thump" range
    const _DETECTION_LPF_HZ: f32 = 150.0;
    const _DETECTION_Q: f32 = 0.707;

    const ENV_ATTACK_MS: f32 = 1.0;
    const ENV_RELEASE_MS: f32 = 50.0;

    const THRESHOLD_LIN: f32 = 0.08;
    const MAX_SOFTEN_DB: f32 = 8.0;

    pub fn new(sample_rate: f32) -> Self {
        let mut plosive_filter = Biquad::new();
        plosive_filter.update_low_shelf(150.0, 0.707, 0.0, sample_rate);

        Self {
            low_env: 0.0,
            plosive_filter,
            sample_rate,
            current_reduction_db: 0.0,
        }
    }

    pub fn _prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.plosive_filter
            .update_low_shelf(150.0, 0.707, -self.current_reduction_db, sample_rate);
    }

    #[inline]
    pub fn process(&mut self, input: f32) -> f32 {
        let abs_in = input.abs();

        // 1. Fast envelope on full signal (looking for low-end thumps)
        // Since SpeechHpf is already in front, we are looking for valid plosives
        let atk = time_constant_coeff(Self::ENV_ATTACK_MS, self.sample_rate);
        let rel = time_constant_coeff(Self::ENV_RELEASE_MS, self.sample_rate);

        if abs_in > self.low_env {
            self.low_env = atk * self.low_env + (1.0 - atk) * abs_in;
        } else {
            self.low_env = rel * self.low_env + (1.0 - rel) * abs_in;
        }

        // 2. Detection logic
        let over = (self.low_env - Self::THRESHOLD_LIN).max(0.0);
        let target_red = (over * 20.0).min(Self::MAX_SOFTEN_DB);

        // 3. Update filter if changed significantly
        if (target_red - self.current_reduction_db).abs() > 0.1 {
            self.current_reduction_db = target_red;
            self.plosive_filter.update_low_shelf(
                150.0,
                0.707,
                -self.current_reduction_db,
                self.sample_rate,
            );
        }

        self.plosive_filter.process(input)
    }

    pub fn reset(&mut self) {
        self.low_env = 0.0;
        self.current_reduction_db = 0.0;
        self.plosive_filter.reset_state();
        self.plosive_filter
            .update_low_shelf(150.0, 0.707, 0.0, self.sample_rate);
    }
}
