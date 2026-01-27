//! Hiss / Rumble Processor
//!
//! Dedicated broadband cleanup that does NOT rely on denoiser speech gating.
//! This module performs real, measurable noise removal:
//!
//! - RUMBLE: raises a high-pass filter cutoff
//! - HISS: applies a high-frequency shelf cut, relaxed during speech
//!
//! This guarantees hiss/rumble reduction even during silence.

use crate::dsp::biquad::Biquad;
use crate::dsp::speech_confidence::SpeechSidechain;

// -----------------------------
// Tunables (safe, conservative)
// -----------------------------

const RUMBLE_MIN_HZ: f32 = 20.0;
const RUMBLE_MAX_HZ: f32 = 70.0; // or 60.0

const HISS_SHELF_HZ: f32 = 8000.0;
const HISS_MAX_CUT_DB: f32 = -24.0;

const SMOOTH_COEFF: f32 = 0.02; // ~50 ms time constant

// -----------------------------
// Processor
// -----------------------------

pub struct HissRumble {
    // Filters
    rumble_hpf: Biquad,
    hiss_shelf: Biquad,

    sample_rate: f32,

    // Smoothed state
    rumble_hz_current: f32,
    rumble_hz_target: f32,

    hiss_db_current: f32,
    hiss_db_target: f32,

    // Update throttling
    update_counter: u32,
}

impl HissRumble {
    pub fn new(sample_rate: f32) -> Self {
        let mut rumble_hpf = Biquad::new();
        let hiss_shelf = Biquad::new();

        // Start flat
        rumble_hpf.update_hpf(RUMBLE_MIN_HZ, 0.707, sample_rate);

        Self {
            rumble_hpf,
            hiss_shelf,
            sample_rate,

            rumble_hz_current: RUMBLE_MIN_HZ,
            rumble_hz_target: RUMBLE_MIN_HZ,

            hiss_db_current: 0.0,
            hiss_db_target: 0.0,

            update_counter: 0,
        }
    }

    #[inline]
    pub fn process(
        &mut self,
        input_l: f32,
        input_r: f32,
        tone: f32,
        sidechain: &SpeechSidechain,
    ) -> (f32, f32) {
        // -----------------------------
        // Bipolar mapping
        // -----------------------------
        // tone: 0.0 = max rumble removal
        // tone: 0.5 = neutral
        // tone: 1.0 = max hiss removal
        let t = (tone.clamp(0.0, 1.0) - 0.5) * 2.0; // -1 .. +1

        let rumble_amt = (-t).max(0.0); // 0..1
        let hiss_amt = (t).max(0.0); // 0..1

        // -----------------------------
        // Targets
        // -----------------------------

        // Rumble = raise HPF cutoff
        self.rumble_hz_target = RUMBLE_MIN_HZ + (RUMBLE_MAX_HZ - RUMBLE_MIN_HZ) * rumble_amt;

        // Hiss = HF shelf cut
        // Relax during speech to protect sibilance
        let speech_relax = (1.0 - sidechain.speech_conf).clamp(0.0, 1.0);
        self.hiss_db_target = HISS_MAX_CUT_DB * hiss_amt * speech_relax;

        // -----------------------------
        // Smooth parameters
        // -----------------------------

        self.rumble_hz_current += (self.rumble_hz_target - self.rumble_hz_current) * SMOOTH_COEFF;

        self.hiss_db_current += (self.hiss_db_target - self.hiss_db_current) * SMOOTH_COEFF;

        // -----------------------------
        // Update filters (throttled)
        // -----------------------------

        if self.update_counter & 31 == 0 {
            self.rumble_hpf
                .update_hpf(self.rumble_hz_current, 0.707, self.sample_rate);

            self.hiss_shelf.update_high_shelf(
                HISS_SHELF_HZ,
                0.707,
                self.hiss_db_current,
                self.sample_rate,
            );
        }
        self.update_counter = self.update_counter.wrapping_add(1);

        // -----------------------------
        // Process audio
        // -----------------------------

        let l = self.hiss_shelf.process(self.rumble_hpf.process(input_l));
        let r = self.hiss_shelf.process(self.rumble_hpf.process(input_r));

        (l, r)
    }

    pub fn reset(&mut self) {
        self.rumble_hpf.reset();
        self.hiss_shelf.reset();

        self.rumble_hz_current = RUMBLE_MIN_HZ;
        self.rumble_hz_target = RUMBLE_MIN_HZ;
        self.hiss_db_current = 0.0;
        self.hiss_db_target = 0.0;

        self.update_counter = 0;
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.reset();
    }

    // -----------------------------
    // Debug / Meter access
    // -----------------------------

    pub fn current_rumble_hz(&self) -> f32 {
        self.rumble_hz_current
    }

    pub fn current_hiss_cut_db(&self) -> f32 {
        self.hiss_db_current
    }

    pub fn get_hiss_db_current(&self) -> f32 {
        self.current_hiss_cut_db()
    }

    pub fn get_rumble_hz_current(&self) -> f32 {
        self.current_rumble_hz()
    }
}
