#!/usr/bin/env python3
"""
Batch process multiple audio files with optimized parameters.
Use this after you've found optimal parameters with auto_tune.py

Usage:
    python batch_process.py --vst path/to/plugin.vst3 --params best_parameters.json --input audio_folder/ --output processed/
"""

import argparse
import json
import soundfile as sf
import numpy as np
from pathlib import Path
from pedalboard import Pedalboard, load_plugin
from tqdm import tqdm


def process_file(vst_path: str, audio_path: Path, params: dict, output_path: Path):
    """Process a single audio file"""
    # Load VST
    vst = load_plugin(vst_path)

    # Set parameters
    param_mapping = {
        'noise_reduction': 'Noise Reduction',
        'noise_mode': 'Noise Mode',
        'reverb_reduction': 'De-Verb',
        'proximity': 'Proximity',
        'clarity': 'Clarity',
        'de_esser': 'De-Esser',
        'leveler': 'Leveler',
        'breath_control': 'Breath Control',
    }

    for param_key, param_value in params.items():
        if param_key in param_mapping:
            vst_param_name = param_mapping[param_key]
            try:
                setattr(vst, vst_param_name, param_value)
            except AttributeError:
                print(f"Warning: Parameter '{vst_param_name}' not found")

    # Load audio
    audio, sr = sf.read(audio_path)

    # Ensure stereo
    if len(audio.shape) == 1:
        audio_stereo = np.stack([audio, audio])
    else:
        audio_stereo = audio.T

    # Process
    board = Pedalboard([vst])
    processed = board(audio_stereo, sr)

    # Convert back to original format
    if len(audio.shape) == 1:
        processed = np.mean(processed, axis=0)

    # Save
    sf.write(output_path, processed, sr)


def main():
    parser = argparse.ArgumentParser(description='Batch process audio files with VST')
    parser.add_argument('--vst', required=True, help='Path to VST3 plugin')
    parser.add_argument('--params', required=True, help='Path to parameters JSON file')
    parser.add_argument('--input', required=True, help='Input directory or file')
    parser.add_argument('--output', required=True, help='Output directory')
    parser.add_argument('--pattern', default='*.wav', help='File pattern to match (default: *.wav)')

    args = parser.parse_args()

    # Load parameters
    with open(args.params, 'r') as f:
        params = json.load(f)

    print(f"Loaded parameters: {params}\n")

    # Get input files
    input_path = Path(args.input)
    if input_path.is_file():
        audio_files = [input_path]
    else:
        audio_files = list(input_path.glob(args.pattern))

    if not audio_files:
        print(f"No audio files found matching pattern: {args.pattern}")
        return

    # Create output directory
    output_dir = Path(args.output)
    output_dir.mkdir(parents=True, exist_ok=True)

    print(f"Processing {len(audio_files)} files...")
    print(f"Output directory: {output_dir}\n")

    # Process files
    for audio_file in tqdm(audio_files, desc="Processing"):
        try:
            output_file = output_dir / audio_file.name
            process_file(args.vst, audio_file, params, output_file)
        except Exception as e:
            print(f"\nError processing {audio_file.name}: {e}")

    print(f"\n✅ Done! Processed files saved to: {output_dir}")


if __name__ == '__main__':
    main()
