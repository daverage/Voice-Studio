use crate::dsp::Biquad;

const CAPTURE_SECONDS: f32 = 30.0;
const FAST_TAU_SEC: f32 = 0.02;
const SLOW_TAU_SEC: f32 = 0.25;
const SILENCE_HOLD_SEC: f32 = 0.6;
const SILENCE_THRESHOLD: f32 = 0.0005;

#[derive(Clone, Copy, Default)]
pub struct SuggestProgress {
    pub seconds: f32,
    pub target_seconds: f32,
    pub active: bool,
}

pub struct SuggestedSettings {
    pub noise_reduction: f32,
    pub noise_tone: f32,
    pub reverb_reduction: f32,
    pub mud_reduction: f32,
    pub proximity: f32,
    pub de_esser: f32,
    pub leveler: f32,
    pub output_gain_db: f32,
}

pub struct AutoSettingsAnalyzer {
    sample_rate: f32,
    capture_samples: usize,
    seen: usize,
    started: bool,
    silence_samples: usize,
    env_fast: f32,
    env_slow: f32,
    min_env_fast: f32,
    max_env_fast: f32,
    sum_env_fast: f32,
    sum_env_slow: f32,
    total_energy: f32,
    low_energy: f32,
    high_energy: f32,
    lowpass: Biquad,
    highpass: Biquad,
}

impl AutoSettingsAnalyzer {
    pub fn new(sample_rate: f32) -> Self {
        let mut analyzer = Self {
            sample_rate,
            capture_samples: 0,
            seen: 0,
            started: false,
            silence_samples: 0,
            env_fast: 0.0,
            env_slow: 0.0,
            min_env_fast: f32::MAX,
            max_env_fast: 0.0,
            sum_env_fast: 0.0,
            sum_env_slow: 0.0,
            total_energy: 0.0,
            low_energy: 0.0,
            high_energy: 0.0,
            lowpass: Biquad::new(),
            highpass: Biquad::new(),
        };
        analyzer.reset(sample_rate);
        analyzer
    }

    pub fn reset(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.capture_samples = (sample_rate * CAPTURE_SECONDS) as usize;
        self.seen = 0;
        self.started = false;
        self.silence_samples = 0;
        self.env_fast = 0.0;
        self.env_slow = 0.0;
        self.min_env_fast = f32::MAX;
        self.max_env_fast = 0.0;
        self.sum_env_fast = 0.0;
        self.sum_env_slow = 0.0;
        self.total_energy = 0.0;
        self.low_energy = 0.0;
        self.high_energy = 0.0;
        self.lowpass.update_lpf(250.0, 0.707, sample_rate);
        self.highpass.update_hpf(5000.0, 0.707, sample_rate);
    }

    pub fn process_sample(&mut self, sample: f32) {
        if self.seen >= self.capture_samples || self.capture_samples == 0 {
            return;
        }

        let abs = sample.abs();
        let alpha_fast = 1.0 - (-1.0 / (FAST_TAU_SEC * self.sample_rate)).exp();
        let alpha_slow = 1.0 - (-1.0 / (SLOW_TAU_SEC * self.sample_rate)).exp();

        self.env_fast += (abs - self.env_fast) * alpha_fast;
        self.env_slow += (abs - self.env_slow) * alpha_slow;

        let above_threshold = self.env_slow >= SILENCE_THRESHOLD;
        if above_threshold {
            self.started = true;
            self.silence_samples = 0;
        } else if self.started {
            self.silence_samples += 1;
        }

        if self.started {
            self.min_env_fast = self.min_env_fast.min(self.env_fast);
            self.max_env_fast = self.max_env_fast.max(self.env_fast);
            self.sum_env_fast += self.env_fast;
            self.sum_env_slow += self.env_slow;

            let low = self.lowpass.process(sample);
            let high = self.highpass.process(sample);
            let s2 = sample * sample;
            self.total_energy += s2;
            self.low_energy += low * low;
            self.high_energy += high * high;

            self.seen += 1;
        }
    }

    pub fn is_done(&self) -> bool {
        if self.capture_samples == 0 {
            return false;
        }
        if self.seen >= self.capture_samples {
            return true;
        }
        if self.started {
            let silence_stop = (self.sample_rate * SILENCE_HOLD_SEC) as usize;
            return self.silence_samples >= silence_stop;
        }
        false
    }

    pub fn has_data(&self) -> bool {
        self.seen > 0
    }

    pub fn progress(&self) -> SuggestProgress {
        let seconds = self.seen as f32 / self.sample_rate.max(1.0);
        SuggestProgress {
            seconds,
            target_seconds: CAPTURE_SECONDS,
            active: self.started,
        }
    }

    pub fn finish(&mut self) -> SuggestedSettings {
        let eps = 1e-8;
        let count = self.seen.max(1) as f32;
        let avg_fast = self.sum_env_fast / count;
        let avg_slow = self.sum_env_slow / count;
        let total_energy = self.total_energy.max(eps);
        let low_ratio = (self.low_energy / total_energy).clamp(0.0, 1.0);
        let high_ratio = (self.high_energy / total_energy).clamp(0.0, 1.0);

        let noise_ratio = (self.min_env_fast / (avg_fast + eps)).clamp(0.0, 1.0);
        let noise_reduction = ((noise_ratio - 0.03) / 0.25).clamp(0.0, 0.8);

        // Ratio ~ 1.0 means slow envelope equals fast envelope (sustained sound/reverb).
        // Ratio > 1.2 means slow envelope is higher than fast envelope (dynamic speech with gaps).
        // So: Low Ratio = Wet (Apply Reduction), High Ratio = Dry (No Reduction).
        let reverb_ratio = (avg_slow / (avg_fast + eps)).clamp(0.0, 3.0);
        let reverb_reduction = ((1.25 - reverb_ratio) / 0.25).clamp(0.0, 0.9);

        let mud_reduction = ((low_ratio - 0.22) / 0.25).clamp(0.0, 0.8);
        let proximity = ((0.22 - low_ratio) / 0.22).clamp(0.0, 0.6);
        let de_esser = ((high_ratio - 0.12) / 0.2).clamp(0.0, 0.8);

        let dyn_db = 20.0 * ((self.max_env_fast + eps) / (avg_fast + eps)).log10();
        let leveler = ((dyn_db - 6.0) / 12.0).clamp(0.0, 0.8);

        let noise_tone = if low_ratio > high_ratio * 1.3 {
            0.25
        } else if high_ratio > low_ratio * 1.3 {
            0.75
        } else {
            0.5
        };

        let rms = (total_energy / count).sqrt();
        let rms_db = 20.0 * (rms + eps).log10();
        let output_gain_db = if rms_db < -60.0 {
            0.0
        } else {
            (-18.0 - rms_db).clamp(-12.0, 12.0)
        };

        self.reset(self.sample_rate);

        SuggestedSettings {
            noise_reduction,
            noise_tone,
            reverb_reduction,
            mud_reduction,
            proximity,
            de_esser,
            leveler,
            output_gain_db,
        }
    }
}
