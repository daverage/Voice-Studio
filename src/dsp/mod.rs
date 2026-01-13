pub mod biquad;
pub mod compressor;
pub mod de_esser;
pub mod denoiser;
pub mod deverber;
pub mod limiter;
pub mod mud_body;
pub mod proximity;
pub mod utils;

pub use biquad::Biquad;
pub use compressor::Compressor;
pub use de_esser::DeEsser;
pub use denoiser::{DenoiseConfig, StreamingDenoiser};
pub use deverber::StreamingDeverber;
pub use limiter::FastLimiter;
pub use mud_body::MudBody;
pub use proximity::Proximity;

pub struct PreviewDelay {
    buf: Vec<f32>,
    idx: usize,
}

impl PreviewDelay {
    pub fn new(len: usize) -> Self {
        assert!(len > 0, "preview delay length must be > 0");
        Self {
            buf: vec![0.0; len],
            idx: 0,
        }
    }

    pub fn push(&mut self, sample: f32) -> f32 {
        let out = self.buf[self.idx];
        self.buf[self.idx] = sample;
        self.idx = (self.idx + 1) % self.buf.len();
        out
    }
}

/// Channel processor containing all DSP effects for one audio channel
pub struct ChannelProcessor {
    pub denoiser: StreamingDenoiser,
    pub safety_hpf: Biquad,
    pub deverber: StreamingDeverber,
    pub proximity: Proximity,
    pub mud_body: MudBody,
    pub de_esser: DeEsser,
    pub compressor: Compressor,
    pub limiter: FastLimiter,
    pub preview_delay_denoise: PreviewDelay,
    pub preview_delay_deverb: PreviewDelay,
}

impl ChannelProcessor {
    pub fn new(win: usize, hop: usize, sr: f32) -> Self {
        let mut safety = Biquad::new();
        safety.update_hpf(80.0, 0.707, sr);
        Self {
            denoiser: StreamingDenoiser::new(win, hop),
            safety_hpf: safety,
            deverber: StreamingDeverber::new(win, hop),
            proximity: Proximity::new(sr),
            mud_body: MudBody::new(sr),
            de_esser: DeEsser::new(sr),
            compressor: Compressor::new(sr),
            limiter: FastLimiter::new(),
            preview_delay_denoise: PreviewDelay::new(win),
            preview_delay_deverb: PreviewDelay::new(win),
        }
    }
}
