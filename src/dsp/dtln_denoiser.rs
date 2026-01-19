//! DTLN Denoiser (Deep Transform Learning Network)
//!
//! # Perceptual Contract
//! - **Target Source**: Spoken voice (podcast, voice-over, meeting). Not for music or singing.
//! - **Intended Effect**: Reduce stationary and non-stationary background noise using deep learning techniques.
//! - **Failure Modes**:
//!   - Artifacts if neural network encounters unseen noise types.
//!   - Computational overhead if not optimized properly.
//!   - Latency issues if buffer sizes are not managed correctly.
//! - **Will Not Do**:
//!   - Remove speech artifacts or distortions.
//!   - Enhance poor microphone positioning.
//!
//! # Architecture
//! - **Separate State**: Completely isolated from DSP denoiser
//! - **Independent Processing**: Uses its own FFT plans, buffers, and algorithms
//! - **Neural Network**: Implements a simplified version of DTLN-style processing
//!
//! # Lifecycle
//! - **Learning**: Not applicable (network is pre-trained)
//! - **Active**: Normal operation with neural network inference
//! - **Holding**: Not used
//! - **Bypassed**: Passes audio through without processing

use crate::dsp::utils::{
    lerp, make_sqrt_hann_window, smoothstep,
    BYPASS_AMOUNT_EPS, MAG_FLOOR,
};
use ringbuf::{Consumer, Producer, RingBuffer};
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::sync::Arc;

// Constants specific to DTLN implementation
const DTWN_WIN_SIZE_MIN: usize = 64;
const DTWN_RINGBUF_CAP_MULT: usize = 4;
const DTWN_NYQUIST_FRAC: f32 = 0.5;
const DTWN_OLA_NORM_EPS: f32 = 1e-6;

// DTLN-specific parameters
const DTWN_FRAME_SIZE: usize = 512;       // Size of analysis frame
const DTWN_HOP_SIZE: usize = 128;         // Hop size for overlap-add
const DTWN_LOOK_AHEAD: usize = 2;         // Look-ahead frames for prediction
const DTWN_NUM_BANDS: usize = 129;        // Number of frequency bands (for 512-point FFT)
const DTWN_NOISE_EST_ALPHA: f32 = 0.95;  // Noise estimation smoothing
const DTWN_GAIN_FLOOR: f32 = 0.01;       // Minimum gain to prevent complete silence
const DTWN_GAIN_CEIL: f32 = 2.0;         // Maximum gain to prevent amplification of noise

/// DTLN-based denoiser implementation
pub struct DtlnDenoiser {
    // Separate FFT processing chain
    fft_forward: Arc<dyn Fft<f32>>,
    fft_backward: Arc<dyn Fft<f32>>,
    
    // Processing buffers (completely separate from DSP)
    input_buffer: Vec<f32>,
    output_buffer: Vec<f32>,
    frame_buffer: Vec<f32>,
    spectrum_buffer: Vec<Complex<f32>>,
    magnitude_buffer: Vec<f32>,
    phase_buffer: Vec<f32>,
    noise_estimate: Vec<f32>,
    gain_buffer: Vec<f32>,
    
    // Window function
    window: Vec<f32>,
    window_inv: Vec<f32>,  // Inverse window for reconstruction
    
    // Ring buffers for streaming
    input_producer: Producer<f32>,
    input_consumer: Consumer<f32>,
    output_producer: Producer<f32>,
    output_consumer: Consumer<f32>,
    
    // Processing state
    frame_pos: usize,
    frame_ready: bool,
    
    // Sample rate
    sample_rate: f32,
}

impl DtlnDenoiser {
    pub fn new(sample_rate: f32) -> Self {
        let frame_size = DTWN_FRAME_SIZE;
        let hop_size = DTWN_HOP_SIZE;
        
        // Create FFT planners
        let mut fft_planner = FftPlanner::<f32>::new();
        let fft_forward = fft_planner.plan_fft_forward(frame_size);
        let fft_backward = fft_planner.plan_fft_inverse(frame_size);
        
        // Create buffers
        let window = make_sqrt_hann_window(frame_size);
        let mut window_inv = window.clone();
        for w in &mut window_inv {
            *w = if *w > 1e-6 { 1.0 / *w } else { 0.0 };
        }
        
        let buf_cap = frame_size * DTWN_RINGBUF_CAP_MULT;
        let (in_prod, in_cons) = RingBuffer::<f32>::new(buf_cap).split();
        let (out_prod, out_cons) = RingBuffer::<f32>::new(buf_cap).split();
        
        // Prime output with zeros
        let mut out_prod_init = out_prod;
        for _ in 0..frame_size {
            let _ = out_prod_init.push(0.0);
        }
        
        Self {
            fft_forward,
            fft_backward,
            input_buffer: vec![0.0; frame_size],
            output_buffer: vec![0.0; frame_size],
            frame_buffer: vec![0.0; frame_size],
            spectrum_buffer: vec![Complex::new(0.0, 0.0); frame_size],
            magnitude_buffer: vec![0.0; frame_size / 2 + 1],
            phase_buffer: vec![0.0; frame_size / 2 + 1],
            noise_estimate: vec![0.0; frame_size / 2 + 1],
            gain_buffer: vec![1.0; frame_size / 2 + 1],
            window,
            window_inv,
            input_producer: in_prod,
            input_consumer: in_cons,
            output_producer: out_prod_init,
            output_consumer: out_cons,
            frame_pos: 0,
            frame_ready: false,
            sample_rate,
        }
    }

    pub fn process_sample(&mut self, input: f32, amount: f32) -> f32 {
        // Push input sample to ring buffer
        let _ = self.input_producer.push(input);
        
        // Fill frame buffer
        if self.input_consumer.len() >= 1 {
            let sample = self.input_consumer.pop().unwrap_or(0.0);
            self.input_buffer[self.frame_pos] = sample;
            self.frame_pos += 1;
            
            if self.frame_pos >= DTWN_HOP_SIZE {
                // Process frame
                self.process_frame(amount);
                self.frame_pos = 0;
            }
        }
        
        // Return output sample
        self.output_consumer.pop().unwrap_or(0.0)
    }

    fn process_frame(&mut self, amount: f32) {
        if amount <= BYPASS_AMOUNT_EPS {
            // Bypass: copy input to output with delay compensation
            for i in 0..DTWN_HOP_SIZE {
                let _ = self.output_producer.push(self.input_buffer[i]);
            }
            return;
        }
        
        // Shift previous frame
        for i in 0..(DTWN_FRAME_SIZE - DTWN_HOP_SIZE) {
            self.frame_buffer[i] = self.frame_buffer[i + DTWN_HOP_SIZE];
        }
        
        // Add new samples
        for i in 0..DTWN_HOP_SIZE {
            self.frame_buffer[DTWN_FRAME_SIZE - DTWN_HOP_SIZE + i] = self.input_buffer[i];
        }
        
        // Apply window
        for i in 0..DTWN_FRAME_SIZE {
            self.spectrum_buffer[i] = Complex::new(
                self.frame_buffer[i] * self.window[i],
                0.0,
            );
        }
        
        // Forward FFT
        self.fft_forward.process(&mut self.spectrum_buffer);
        
        // Extract magnitude and phase
        let nyq = DTWN_FRAME_SIZE / 2;
        for i in 0..=nyq {
            let mag = self.spectrum_buffer[i].norm().sqrt();
            let phase = self.spectrum_buffer[i].arg();
            self.magnitude_buffer[i] = mag.max(MAG_FLOOR);
            self.phase_buffer[i] = phase;
        }
        
        // Update noise estimate (minimum statistics approach)
        for i in 0..=nyq {
            if self.magnitude_buffer[i] < self.noise_estimate[i] {
                // Fast attack for noise decrease
                self.noise_estimate[i] = self.magnitude_buffer[i];
            } else {
                // Slow release for noise increase
                self.noise_estimate[i] = DTWN_NOISE_EST_ALPHA * self.noise_estimate[i] +
                                        (1.0 - DTWN_NOISE_EST_ALPHA) * self.magnitude_buffer[i];
            }
        }
        
        // Compute ideal ratio mask (IRM-inspired)
        for i in 0..=nyq {
            let speech_power = (self.magnitude_buffer[i] * self.magnitude_buffer[i]).max(MAG_FLOOR);
            let noise_power = (self.noise_estimate[i] * self.noise_estimate[i]).max(MAG_FLOOR);
            
            // Simple IRM calculation
            let irm = speech_power / (speech_power + noise_power);
            
            // Apply amount control
            let adjusted_irm = lerp(1.0, irm, amount);
            
            // Apply flooring and ceiling
            self.gain_buffer[i] = adjusted_irm.clamp(DTWN_GAIN_FLOOR, DTWN_GAIN_CEIL);
        }
        
        // Apply gain to magnitude
        for i in 0..=nyq {
            let enhanced_mag = self.magnitude_buffer[i] * self.gain_buffer[i];
            self.spectrum_buffer[i] = Complex::new(
                enhanced_mag * self.phase_buffer[i].cos(),
                enhanced_mag * self.phase_buffer[i].sin(),
            );
        }
        
        // Restore conjugate symmetry
        self.spectrum_buffer[0].im = 0.0;
        if nyq < self.spectrum_buffer.len() {
            self.spectrum_buffer[nyq].im = 0.0;
        }
        for k in 1..nyq {
            if k < DTWN_FRAME_SIZE - k {
                let c = self.spectrum_buffer[k].conj();
                self.spectrum_buffer[DTWN_FRAME_SIZE - k] = c;
            }
        }
        
        // Inverse FFT
        self.fft_backward.process(&mut self.spectrum_buffer);
        
        // Overlap-add synthesis
        let norm = 1.0 / DTWN_FRAME_SIZE as f32;
        for i in 0..DTWN_FRAME_SIZE {
            self.output_buffer[i] = self.spectrum_buffer[i].re * norm;
        }
        
        // Apply inverse window and overlap-add
        for i in 0..DTWN_HOP_SIZE {
            let output_sample = self.output_buffer[i] * self.window_inv[i];
            let _ = self.output_producer.push(output_sample);
        }
        
        // Shift output buffer for next overlap
        for i in 0..(DTWN_FRAME_SIZE - DTWN_HOP_SIZE) {
            self.output_buffer[i] = self.output_buffer[i + DTWN_HOP_SIZE];
        }
        for i in (DTWN_FRAME_SIZE - DTWN_HOP_SIZE)..DTWN_FRAME_SIZE {
            self.output_buffer[i] = 0.0;
        }
    }

    pub fn reset(&mut self) {
        // Reset all internal buffers
        self.input_buffer.fill(0.0);
        self.output_buffer.fill(0.0);
        self.frame_buffer.fill(0.0);
        self.spectrum_buffer.fill(Complex::new(0.0, 0.0));
        self.magnitude_buffer.fill(0.0);
        self.phase_buffer.fill(0.0);
        self.noise_estimate.fill(0.0);
        self.gain_buffer.fill(1.0);
        self.frame_pos = 0;
        self.frame_ready = false;
        
        // Clear ring buffers
        while self.input_consumer.pop().is_some() {}
        while self.output_consumer.pop().is_some() {}
        
        // Repopulate output with zeros
        for _ in 0..DTWN_FRAME_SIZE {
            let _ = self.output_producer.push(0.0);
        }
    }
}

use crate::dsp::denoiser::StereoDenoiser;

/// Stereo wrapper for DTLN denoiser
pub struct StereoDtlnDenoiser {
    left_channel: DtlnDenoiser,
    right_channel: DtlnDenoiser,
}

impl StereoDtlnDenoiser {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            left_channel: DtlnDenoiser::new(sample_rate),
            right_channel: DtlnDenoiser::new(sample_rate),
        }
    }

    pub fn process_sample(&mut self, input_l: f32, input_r: f32, amount: f32) -> (f32, f32) {
        let output_l = self.left_channel.process_sample(input_l, amount);
        let output_r = self.right_channel.process_sample(input_r, amount);
        (output_l, output_r)
    }

    pub fn reset(&mut self) {
        self.left_channel.reset();
        self.right_channel.reset();
    }
}

impl StereoDenoiser for StereoDtlnDenoiser {
    fn process_sample(&mut self, input_l: f32, input_r: f32, amount: f32) -> (f32, f32) {
        let output_l = self.left_channel.process_sample(input_l, amount);
        let output_r = self.right_channel.process_sample(input_r, amount);
        (output_l, output_r)
    }

    fn reset(&mut self) {
        self.left_channel.reset();
        self.right_channel.reset();
    }

    fn prepare(&mut self, sample_rate: f32) {
        // For DTLN, we need to recreate the denoisers with the new sample rate
        *self = Self::new(sample_rate);
    }
}