#![cfg(feature = "tflite_validate")]
use anyhow::{Context, Result};
use hound::{SampleFormat, WavReader};
use std::path::PathBuf;
use vxcleaner::dsp::dtln_denoiser::StereoDtlnDenoiser;
use vxcleaner::dsp::dtln_denoiser_tflite::StereoDtlnDenoiserTflite;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let input = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("test_data/noisy_speech.wav"));
    let reader = WavReader::open(&input)
        .with_context(|| format!("failed to open validation WAV '{}'", input.display()))?;
    let spec = reader.spec();
    if spec.sample_format != SampleFormat::Int || spec.bits_per_sample != 16 {
        anyhow::bail!("validation only supports 16-bit integer WAV fixtures");
    }
    let samp_rate = spec.sample_rate as f32;
    let mut native = StereoDtlnDenoiser::new(samp_rate);
    let mut baseline = StereoDtlnDenoiserTflite::new(samp_rate);

    let mut it = reader.into_samples::<i16>();
    let mut sample_count = 0usize;
    let mut mae = 0.0f64;
    let mut max_err = 0.0f64;
    while let Some(left_sample) = it.next() {
        let left_sample = left_sample?;
        let right_sample = if spec.channels > 1 {
            it.next().transpose()?.unwrap_or(left_sample)
        } else {
            left_sample
        };
        let left = left_sample as f32 / i16::MAX as f32;
        let right = right_sample as f32 / i16::MAX as f32;
        let strength = 0.85;
        let tone = 0.5;
        let native_out = native.process_sample(left, right, strength, tone);
        let baseline_out = baseline.process_sample(left, right, strength, tone);
        let delta = (native_out.0 - baseline_out.0).abs() as f64;
        mae += delta;
        if delta > max_err {
            max_err = delta;
        }
        sample_count += 1;
    }

    let mae = mae / sample_count as f64;
    println!("Validation summary for '{}':", input.display());
    println!("  frames processed : {}", sample_count);
    println!("  mean abs error   : {:.6}", mae);
    println!("  max abs error    : {:.6}", max_err);
    Ok(())
}
