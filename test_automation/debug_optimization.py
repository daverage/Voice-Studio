#!/usr/bin/env python3
"""
Debug why all trials get the same score
"""
import numpy as np
import soundfile as sf
from pedalboard import Pedalboard, load_plugin
from pathlib import Path

def test_audio_files(noisy_path, clean_path):
    """Check if audio files are actually different"""
    print("=" * 60)
    print("1. Testing Audio File Differences")
    print("=" * 60)

    noisy, sr_noisy = sf.read(noisy_path)
    clean, sr_clean = sf.read(clean_path)

    print(f"\nNoisy audio:")
    print(f"  Shape: {noisy.shape}")
    print(f"  Sample rate: {sr_noisy} Hz")
    print(f"  Duration: {len(noisy) / sr_noisy:.2f} seconds")
    print(f"  RMS: {np.sqrt(np.mean(noisy**2)):.6f}")
    print(f"  Peak: {np.abs(noisy).max():.6f}")

    print(f"\nClean (reference) audio:")
    print(f"  Shape: {clean.shape}")
    print(f"  Sample rate: {sr_clean} Hz")
    print(f"  Duration: {len(clean) / sr_clean:.2f} seconds")
    print(f"  RMS: {np.sqrt(np.mean(clean**2)):.6f}")
    print(f"  Peak: {np.abs(clean).max():.6f}")

    # Check if they're different
    if noisy.shape != clean.shape:
        print(f"\n⚠️  Shape mismatch! Need to resample/trim")
        min_len = min(len(noisy), len(clean))
        noisy = noisy[:min_len]
        clean = clean[:min_len]

    diff = np.abs(noisy - clean).mean()
    print(f"\nMean absolute difference: {diff:.6f}")

    if diff < 1e-6:
        print("❌ Files are identical or nearly identical!")
        print("   Optimization won't work - they need to be different recordings")
        return False
    else:
        print(f"✅ Files are different (good!)")
        return True


def test_vst_processing(vst_path, audio_path):
    """Test if VST actually changes audio with different settings"""
    print("\n" + "=" * 60)
    print("2. Testing VST Processing With Different Settings")
    print("=" * 60)

    audio, sr = sf.read(audio_path)
    vst = load_plugin(vst_path)

    # CRITICAL: Disable Easy Mode!
    vst.easy_mode = False
    print("  Disabled Easy Mode - using advanced parameters")

    # Prepare stereo
    if len(audio.shape) == 1:
        audio_stereo = np.stack([audio, audio])
    else:
        audio_stereo = audio.T

    # Test 1: All parameters OFF (0.0)
    print("\n📊 Test 1: All parameters at 0.0 (OFF)")
    vst.noise_reduction = 0.0
    vst.de_verb_room = 0.0
    vst.proximity_closeness = 0.0
    vst.clarity = 0.0
    vst.de_esser = 0.0
    vst.leveler_auto_volume = 0.0

    board = Pedalboard([vst])
    output_off = board(audio_stereo, sr)

    rms_off = np.sqrt(np.mean(output_off**2))
    print(f"  Output RMS: {rms_off:.6f}")

    # Test 2: All parameters MAX (1.0)
    print("\n📊 Test 2: All parameters at 1.0 (MAX)")
    vst.noise_reduction = 1.0
    vst.de_verb_room = 1.0
    vst.proximity_closeness = 0.5  # Don't go full on proximity
    vst.clarity = 1.0
    vst.de_esser = 0.7
    vst.leveler_auto_volume = 0.8

    board = Pedalboard([vst])
    output_max = board(audio_stereo, sr)

    rms_max = np.sqrt(np.mean(output_max**2))
    print(f"  Output RMS: {rms_max:.6f}")

    # Test 3: Medium settings
    print("\n📊 Test 3: All parameters at 0.5 (MEDIUM)")
    vst.noise_reduction = 0.5
    vst.de_verb_room = 0.5
    vst.proximity_closeness = 0.25
    vst.clarity = 0.5
    vst.de_esser = 0.35
    vst.leveler_auto_volume = 0.4

    board = Pedalboard([vst])
    output_med = board(audio_stereo, sr)

    rms_med = np.sqrt(np.mean(output_med**2))
    print(f"  Output RMS: {rms_med:.6f}")

    # Compare
    print("\n📈 Comparison:")
    diff_off_max = np.abs(output_off - output_max).mean()
    diff_off_med = np.abs(output_off - output_med).mean()
    diff_med_max = np.abs(output_med - output_max).mean()

    print(f"  OFF vs MAX difference:    {diff_off_max:.6f}")
    print(f"  OFF vs MEDIUM difference: {diff_off_med:.6f}")
    print(f"  MEDIUM vs MAX difference: {diff_med_max:.6f}")

    if diff_off_max < 1e-4:
        print("\n❌ VST is NOT changing audio!")
        print("   All settings produce same output")
        return False
    else:
        print(f"\n✅ VST is working! Different settings = different audio")
        return True


def test_metric_calculation():
    """Test if metrics work correctly"""
    print("\n" + "=" * 60)
    print("3. Testing Metric Calculations")
    print("=" * 60)

    # Create test signals
    sr = 44100
    duration = 1.0  # 1 second
    t = np.linspace(0, duration, int(sr * duration))

    # Reference: clean sine wave
    reference = 0.5 * np.sin(2 * np.pi * 440 * t)

    # Test 1: Identical signal (should score high)
    identical = reference.copy()

    # Test 2: Noisy signal (should score low)
    noisy = reference + 0.1 * np.random.randn(len(reference))

    # Test 3: Heavily distorted (should score very low)
    distorted = reference + 0.5 * np.random.randn(len(reference))

    print("\nCalculating SNR for test signals:")
    snr_identical = 10 * np.log10(np.mean(reference**2) / (np.mean((identical - reference)**2) + 1e-10))
    snr_noisy = 10 * np.log10(np.mean(reference**2) / (np.mean((noisy - reference)**2) + 1e-10))
    snr_distorted = 10 * np.log10(np.mean(reference**2) / (np.mean((distorted - reference)**2) + 1e-10))

    print(f"  Identical:  SNR = {snr_identical:.2f} dB (should be very high)")
    print(f"  Noisy:      SNR = {snr_noisy:.2f} dB (should be moderate)")
    print(f"  Distorted:  SNR = {snr_distorted:.2f} dB (should be low)")

    if snr_identical > snr_noisy > snr_distorted:
        print("\n✅ Metrics working correctly (scores decrease with quality)")
        return True
    else:
        print("\n❌ Metrics broken! Scores don't match quality")
        return False


if __name__ == '__main__':
    import argparse

    parser = argparse.ArgumentParser(description='Debug optimization issues')
    parser.add_argument('--noisy', required=True, help='Path to noisy audio')
    parser.add_argument('--reference', required=True, help='Path to clean reference audio')
    parser.add_argument('--vst', required=True, help='Path to VST3 plugin')

    args = parser.parse_args()

    print("\n🔍 OPTIMIZATION DEBUG REPORT")
    print("=" * 60)

    # Run tests
    test1 = test_audio_files(args.noisy, args.reference)
    test2 = test_vst_processing(args.vst, args.noisy)
    test3 = test_metric_calculation()

    # Summary
    print("\n" + "=" * 60)
    print("SUMMARY")
    print("=" * 60)
    print(f"Audio files different:     {'✅ PASS' if test1 else '❌ FAIL'}")
    print(f"VST processes audio:       {'✅ PASS' if test2 else '❌ FAIL'}")
    print(f"Metrics work correctly:    {'✅ PASS' if test3 else '❌ FAIL'}")

    if test1 and test2 and test3:
        print("\n✅ All tests passed - optimization should work!")
        print("   If trials still get same score, check:")
        print("   1. Parameter ranges in auto_tune.py")
        print("   2. Metric normalization ranges")
    else:
        print("\n❌ Issues found - fix these before running optimization")

    print("=" * 60)
