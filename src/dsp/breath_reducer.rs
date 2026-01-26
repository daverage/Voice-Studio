use crate::dsp::envelope::VoiceEnvelope;
use crate::dsp::speech_confidence::SpeechSidechain;
use crate::dsp::utils::time_constant_coeff;

/// Breath Reducer (Advanced Control)
///
/// Softens breaths during low speech confidence periods without gating or muting.
pub struct BreathReducer {
    envelope: f32,
    gain_smooth: f32,
    sample_rate: f32,
}

impl BreathReducer {
    // Detection constants
    const ENV_ATTACK_MS: f32 = 5.0;
    const ENV_RELEASE_MS: f32 = 40.0;

    // Gain ballistics
    const GAIN_ATTACK_MS: f32 = 30.0;
    const GAIN_RELEASE_MS: f32 = 100.0;

    // Thresholds
    const BREATH_MAX_REDUCTION_DB: f32 = 10.0;

    pub fn new(sample_rate: f32) -> Self {
        Self {
            envelope: 0.0,
            gain_smooth: 1.0,
            sample_rate,
        }
    }

    pub fn _prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    #[inline]
    pub fn process(
        &mut self,
        input: f32,
        amount: f32,
        sidechain: &SpeechSidechain,
        _env: &VoiceEnvelope,
    ) -> f32 {
        let abs_in = input.abs();

        // 1. Independent envelope tracking for breath detection
        let atk = time_constant_coeff(Self::ENV_ATTACK_MS, self.sample_rate);
        let rel = time_constant_coeff(Self::ENV_RELEASE_MS, self.sample_rate);

        if abs_in > self.envelope {
            self.envelope = atk * self.envelope + (1.0 - atk) * abs_in;
        } else {
            self.envelope = rel * self.envelope + (1.0 - rel) * abs_in;
        }

        // 2. Breath logic: Low speech confidence + significant energy = likely breath
        // We invert speech confidence to get "silence/breath confidence"
        let breath_prob = (1.0 - sidechain.speech_conf).powf(4.0); // Bias strongly towards low confidence

        // 3. Compute target gain
        // Softly attenuate based on amount and breath probability
        let target_reduction_db = amount * breath_prob * Self::BREATH_MAX_REDUCTION_DB;
        let target_gain = 10.0f32.powf(-target_reduction_db / 20.0);

        // 4. Smooth gain independently
        let g_atk = time_constant_coeff(Self::GAIN_ATTACK_MS, self.sample_rate);
        let g_rel = time_constant_coeff(Self::GAIN_RELEASE_MS, self.sample_rate);

        if target_gain < self.gain_smooth {
            self.gain_smooth = g_atk * self.gain_smooth + (1.0 - g_atk) * target_gain;
        } else {
            self.gain_smooth = g_rel * self.gain_smooth + (1.0 - g_rel) * target_gain;
        }

        input * self.gain_smooth
    }

    pub fn reset(&mut self) {
        self.envelope = 0.0;
        self.gain_smooth = 1.0;
    }
}
