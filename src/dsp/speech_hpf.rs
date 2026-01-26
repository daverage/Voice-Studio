use crate::dsp::biquad::Biquad;

/// Speech HPF (Hidden Hygiene)
///
/// Removes subsonic energy below the human voice range to prevent
/// contamination of downstream analysis and processing.
pub struct SpeechHpf {
    filter_l: Biquad,
    filter_r: Biquad,
    _sample_rate: f32,
}

impl SpeechHpf {
    const CUTOFF_HZ: f32 = 90.0;
    const Q: f32 = 0.707;

    pub fn new(sample_rate: f32) -> Self {
        let mut filter_l = Biquad::new();
        let mut filter_r = Biquad::new();
        filter_l.update_hpf(Self::CUTOFF_HZ, Self::Q, sample_rate);
        filter_r.update_hpf(Self::CUTOFF_HZ, Self::Q, sample_rate);

        Self {
            filter_l,
            filter_r,
            _sample_rate: sample_rate,
        }
    }

    pub fn _prepare(&mut self, sample_rate: f32) {
        self._sample_rate = sample_rate;
        self.filter_l
            .update_hpf(Self::CUTOFF_HZ, Self::Q, sample_rate);
        self.filter_r
            .update_hpf(Self::CUTOFF_HZ, Self::Q, sample_rate);
    }

    #[inline]
    pub fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        (self.filter_l.process(left), self.filter_r.process(right))
    }

    pub fn reset(&mut self) {
        self.filter_l.reset_state();
        self.filter_r.reset_state();
    }
}
