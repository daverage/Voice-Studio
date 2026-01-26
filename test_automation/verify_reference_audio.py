#!/usr/bin/env python3
"""
Verify that reference audio is actually clean and matches the noisy audio.
"""
import numpy as np
import soundfile as sf
import librosa
from pathlib import Path

def analyze_noise_profile(audio: np.ndarray, sr: int, name: str):
    """Analyze noise characteristics of audio"""
    print(f"\n{'='*60}")
    print(f"ANALYZING: {name}")
    print(f"{'='*60}")

    # Convert to mono if stereo
    if len(audio.shape) == 2:
        audio = np.mean(audio, axis=1)

    # Basic stats
    rms = np.sqrt(np.mean(audio**2))
    peak = np.abs(audio).max()
    crest_factor_db = 20 * np.log10(peak / (rms + 1e-10))

    print(f"\nBasic Metrics:")
    print(f"  RMS Level:     {rms:.6f} ({20*np.log10(rms+1e-10):.2f} dBFS)")
    print(f"  Peak Level:    {peak:.6f} ({20*np.log10(peak+1e-10):.2f} dBFS)")
    print(f"  Crest Factor:  {crest_factor_db:.2f} dB")

    # Estimate noise floor (bottom 10% of samples by amplitude)
    sorted_abs = np.sort(np.abs(audio))
    noise_floor_samples = sorted_abs[:len(sorted_abs)//10]
    noise_floor = np.mean(noise_floor_samples)
    noise_floor_db = 20 * np.log10(noise_floor + 1e-10)

    # Estimate SNR
    signal_power = rms ** 2
    noise_power = noise_floor ** 2
    snr_db = 10 * np.log10(signal_power / (noise_power + 1e-10))

    print(f"\nNoise Analysis:")
    print(f"  Noise Floor:   {noise_floor:.6f} ({noise_floor_db:.2f} dBFS)")
    print(f"  Estimated SNR: {snr_db:.2f} dB")

    # Spectral analysis
    # Find silent regions (below -40dB)
    threshold = 10**(-40/20)  # -40 dBFS
    silent_samples = np.abs(audio) < threshold
    silence_percentage = 100 * silent_samples.sum() / len(audio)

    print(f"  Silence (<-40dB): {silence_percentage:.1f}%")

    # Frequency analysis
    fft = np.fft.rfft(audio)
    magnitude_db = 20 * np.log10(np.abs(fft) + 1e-10)
    freqs = np.fft.rfftfreq(len(audio), 1/sr)

    # Check low-frequency noise (0-100 Hz)
    low_freq_mask = freqs < 100
    low_freq_energy = np.mean(magnitude_db[low_freq_mask])

    # Check high-frequency noise (8kHz+)
    high_freq_mask = freqs > 8000
    high_freq_energy = np.mean(magnitude_db[high_freq_mask])

    # Speech range (200-4000 Hz)
    speech_mask = (freqs >= 200) & (freqs <= 4000)
    speech_energy = np.mean(magnitude_db[speech_mask])

    print(f"\nSpectral Energy:")
    print(f"  Low Freq (0-100Hz):   {low_freq_energy:.1f} dB")
    print(f"  Speech (200-4000Hz):  {speech_energy:.1f} dB")
    print(f"  High Freq (>8kHz):    {high_freq_energy:.1f} dB")

    # Broadband noise indicator
    spectral_flatness = np.exp(np.mean(np.log(np.abs(fft) + 1e-10))) / (np.mean(np.abs(fft)) + 1e-10)

    print(f"\nSpectral Flatness: {spectral_flatness:.4f}")
    print(f"  (0.0 = pure tone, 1.0 = white noise)")

    return {
        'rms': rms,
        'noise_floor': noise_floor,
        'snr_db': snr_db,
        'spectral_flatness': spectral_flatness,
        'low_freq_energy': low_freq_energy,
        'high_freq_energy': high_freq_energy,
    }


def compare_audio_files(noisy_path: str, reference_path: str):
    """Compare noisy and reference audio to see if they match"""
    print(f"\n{'='*60}")
    print(f"FILE COMPARISON")
    print(f"{'='*60}")

    # Load audio
    noisy, sr_noisy = sf.read(noisy_path)
    reference, sr_ref = sf.read(reference_path)

    print(f"\nFile Info:")
    print(f"  Noisy:     {noisy.shape} @ {sr_noisy}Hz")
    print(f"  Reference: {reference.shape} @ {sr_ref}Hz")

    # Convert to mono
    if len(noisy.shape) == 2:
        noisy = np.mean(noisy, axis=1)
    if len(reference.shape) == 2:
        reference = np.mean(reference, axis=1)

    # Resample if needed
    if sr_noisy != sr_ref:
        reference = librosa.resample(reference, orig_sr=sr_ref, target_sr=sr_noisy)
        sr = sr_noisy
    else:
        sr = sr_noisy

    # Trim to same length
    min_len = min(len(noisy), len(reference))
    noisy = noisy[:min_len]
    reference = reference[:min_len]

    # Check if files are identical
    diff = np.abs(noisy - reference)
    mean_diff = np.mean(diff)
    max_diff = np.max(diff)

    print(f"\nDifference Analysis:")
    print(f"  Mean Absolute Diff: {mean_diff:.6f}")
    print(f"  Max Absolute Diff:  {max_diff:.6f}")

    if mean_diff < 1e-6:
        print(f"\n❌ PROBLEM: Files are IDENTICAL or nearly identical!")
        print(f"   The 'reference' is the same as the 'noisy' file.")
        print(f"   You need a professionally cleaned version of the same recording.")
        return False

    # Cross-correlation to check alignment
    correlation = np.correlate(noisy, reference, mode='valid')
    max_corr = np.max(correlation)
    corr_normalized = max_corr / (np.sqrt(np.sum(noisy**2) * np.sum(reference**2)) + 1e-10)

    print(f"\nCross-Correlation: {corr_normalized:.4f}")
    print(f"  (1.0 = perfectly aligned, <0.5 = likely different recordings)")

    if corr_normalized < 0.5:
        print(f"\n⚠️  WARNING: Low correlation suggests files might be:")
        print(f"   - Different recordings (not the same speaker/words)")
        print(f"   - Severely misaligned (different timestamps)")
        print(f"   - Different processing applied")
        return False

    # Analyze both files
    noisy_metrics = analyze_noise_profile(noisy, sr, "NOISY FILE")
    ref_metrics = analyze_noise_profile(reference, sr, "REFERENCE FILE")

    # Compare noise floors
    print(f"\n{'='*60}")
    print(f"VERDICT")
    print(f"{'='*60}")

    snr_improvement = ref_metrics['snr_db'] - noisy_metrics['snr_db']

    print(f"\nSNR Improvement: {snr_improvement:.2f} dB")

    if snr_improvement < 3.0:
        print(f"\n❌ PROBLEM: Reference is NOT significantly cleaner!")
        print(f"\n   Expected: Reference should have at least 6-10 dB better SNR")
        print(f"   Actual:   Only {snr_improvement:.2f} dB improvement")
        print(f"\n   This explains why the optimizer sets noise_reduction=0:")
        print(f"   - The reference has similar noise to the input")
        print(f"   - Denoising makes processed audio LESS like the reference")
        print(f"   - So the optimizer avoids denoising")
        print(f"\n   ⚠️  You need a truly CLEAN reference recording!")
        return False

    if ref_metrics['noise_floor'] > 0.01:
        print(f"\n⚠️  WARNING: Reference has noticeable noise floor")
        print(f"   Noise floor: {ref_metrics['noise_floor']:.6f}")
        print(f"   For a 'clean' reference, expect < 0.005")

    if ref_metrics['spectral_flatness'] > 0.3:
        print(f"\n⚠️  WARNING: Reference has high spectral flatness")
        print(f"   Spectral flatness: {ref_metrics['spectral_flatness']:.4f}")
        print(f"   Suggests broadband noise is present")

    if snr_improvement >= 10.0:
        print(f"\n✅ GOOD: Reference is significantly cleaner ({snr_improvement:.1f} dB)")
        print(f"   Optimization should work correctly with these files.")
        return True
    elif snr_improvement >= 6.0:
        print(f"\n⚠️  MARGINAL: Reference is somewhat cleaner ({snr_improvement:.1f} dB)")
        print(f"   Optimization may work but results could be unreliable.")
        return True
    else:
        print(f"\n❌ BAD: Reference is barely cleaner ({snr_improvement:.1f} dB)")
        print(f"   Optimization will not work properly.")
        return False


def main():
    import argparse

    parser = argparse.ArgumentParser(description='Verify reference audio is clean')
    parser.add_argument('--noisy', required=True, help='Path to noisy audio file')
    parser.add_argument('--reference', required=True, help='Path to clean reference audio')

    args = parser.parse_args()

    print("="*60)
    print("REFERENCE AUDIO VERIFICATION")
    print("="*60)
    print(f"\nThis script checks if your reference audio is actually clean")
    print(f"and properly aligned with the noisy audio.")

    result = compare_audio_files(args.noisy, args.reference)

    print(f"\n{'='*60}")
    if result:
        print("✅ Files appear suitable for optimization")
    else:
        print("❌ Files are NOT suitable for optimization")
        print("\nRECOMMENDATION:")
        print("  You need a professional 'clean' reference that is:")
        print("  1. The SAME recording (same speaker, same words)")
        print("  2. Professionally cleaned/denoised")
        print("  3. Has >10 dB better SNR than the noisy version")
        print("  4. Time-aligned with the noisy version")
    print("="*60)


if __name__ == '__main__':
    main()
