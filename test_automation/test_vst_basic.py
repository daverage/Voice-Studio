#!/usr/bin/env python3
"""
Basic VST3 loading and processing test
"""
import numpy as np
import soundfile as sf
from pedalboard import Pedalboard, load_plugin
from pathlib import Path

def test_vst_loading(vst_path, audio_path):
    """Test basic VST loading and processing"""

    print("=" * 60)
    print("VST3 Basic Functionality Test")
    print("=" * 60)

    # 1. Load VST
    print(f"\n1. Loading VST from: {vst_path}")
    try:
        vst = load_plugin(str(vst_path))
        print(f"   ✅ VST loaded successfully")
        print(f"   Type: {type(vst)}")
    except Exception as e:
        print(f"   ❌ Failed to load VST: {e}")
        return False

    # 2. Print available parameters
    print(f"\n2. VST Parameters:")
    try:
        params = [attr for attr in dir(vst) if not attr.startswith('_')]
        print(f"   Found {len(params)} parameters/methods")
        # Print first 20
        for p in params[:20]:
            print(f"   - {p}")
        if len(params) > 20:
            print(f"   ... and {len(params) - 20} more")
    except Exception as e:
        print(f"   ⚠️  Could not list parameters: {e}")

    # 3. Load audio
    print(f"\n3. Loading audio from: {audio_path}")
    try:
        audio, sr = sf.read(audio_path)
        print(f"   ✅ Audio loaded")
        print(f"   Shape: {audio.shape}")
        print(f"   Sample rate: {sr} Hz")
        print(f"   Duration: {len(audio) / sr:.2f} seconds")
    except Exception as e:
        print(f"   ❌ Failed to load audio: {e}")
        return False

    # 4. Prepare stereo audio
    print(f"\n4. Preparing audio for VST")
    if len(audio.shape) == 1:
        audio_stereo = np.stack([audio, audio])
        print(f"   Converted mono to stereo: {audio_stereo.shape}")
    else:
        audio_stereo = audio.T
        print(f"   Transposed to (channels, samples): {audio_stereo.shape}")

    # 5. Test with small chunk first
    print(f"\n5. Testing with small 1-second chunk")
    chunk_samples = sr  # 1 second
    audio_chunk = audio_stereo[:, :chunk_samples]
    print(f"   Chunk shape: {audio_chunk.shape}")

    try:
        board = Pedalboard([vst])

        # Try different processing methods
        print(f"\n   Trying: board(audio, sr)")
        result1 = board(audio_chunk, sr)
        print(f"   ✅ Success! Output shape: {result1.shape}")

        if result1.shape != audio_chunk.shape:
            print(f"   ⚠️  WARNING: Output shape mismatch!")
            print(f"      Expected: {audio_chunk.shape}")
            print(f"      Got:      {result1.shape}")
            return False

    except Exception as e:
        print(f"   ❌ Processing failed: {e}")
        print(f"   Error type: {type(e).__name__}")
        import traceback
        traceback.print_exc()
        return False

    # 6. Test with full audio
    print(f"\n6. Testing with full audio ({audio_stereo.shape[1]} samples)")
    try:
        result_full = board(audio_stereo, sr)
        print(f"   ✅ Success! Output shape: {result_full.shape}")

        if result_full.shape != audio_stereo.shape:
            print(f"   ⚠️  WARNING: Output shape mismatch!")
            print(f"      Expected: {audio_stereo.shape}")
            print(f"      Got:      {result_full.shape}")
            return False

    except Exception as e:
        print(f"   ❌ Processing failed: {e}")
        import traceback
        traceback.print_exc()
        return False

    # 7. Verify output is different from input
    print(f"\n7. Verifying VST actually processed audio")
    diff = np.abs(result_full - audio_stereo).mean()
    print(f"   Mean absolute difference: {diff:.6f}")

    if diff < 1e-8:
        print(f"   ⚠️  Output is identical to input - VST may not be processing")
    else:
        print(f"   ✅ VST is processing audio (output differs from input)")

    print(f"\n" + "=" * 60)
    print("✅ All tests passed!")
    print("=" * 60)
    return True


if __name__ == '__main__':
    import argparse

    parser = argparse.ArgumentParser(description='Test VST3 basic functionality')
    parser.add_argument('--vst', required=True, help='Path to VST3 plugin')
    parser.add_argument('--audio', required=True, help='Path to test audio file')

    args = parser.parse_args()

    success = test_vst_loading(args.vst, args.audio)

    if not success:
        print("\n❌ VST test failed - auto_tune.py will not work correctly")
        exit(1)
    else:
        print("\n✅ VST test passed - auto_tune.py should work")
        exit(0)
