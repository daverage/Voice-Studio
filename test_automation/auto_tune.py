#!/usr/bin/env python3
"""
Automated VST Parameter Optimization for Voice Studio
Uses Bayesian optimization to find optimal settings by comparing against reference audio.

Requirements:
    pip install pedalboard numpy scipy librosa optuna pesq pystoi soundfile

Usage:
    python auto_tune.py --noisy input.wav --reference clean.wav --vst path/to/vxcleaner.vst3
"""

import argparse
import numpy as np
import soundfile as sf
from pathlib import Path
from typing import Dict, Tuple
import optuna
from pedalboard import Pedalboard, load_plugin
from pesq import pesq
from pystoi import stoi
from scipy import signal
import librosa


class AudioMetrics:
    """Calculate various audio quality metrics"""

    @staticmethod
    def calculate_snr(clean: np.ndarray, noisy: np.ndarray) -> float:
        """Signal-to-Noise Ratio in dB"""
        signal_power = np.mean(clean ** 2)
        noise_power = np.mean((clean - noisy) ** 2)
        if noise_power < 1e-10:
            return 100.0
        return 10 * np.log10(signal_power / noise_power)

    @staticmethod
    def calculate_si_sdr(reference: np.ndarray, estimate: np.ndarray) -> float:
        """Scale-Invariant Signal-to-Distortion Ratio"""
        # Remove mean
        reference = reference - np.mean(reference)
        estimate = estimate - np.mean(estimate)

        # Calculate optimal scaling
        alpha = np.dot(estimate, reference) / (np.dot(reference, reference) + 1e-8)

        # Calculate SI-SDR
        s_target = alpha * reference
        e_noise = estimate - s_target

        si_sdr = 10 * np.log10(
            (np.sum(s_target ** 2) + 1e-8) / (np.sum(e_noise ** 2) + 1e-8)
        )
        return si_sdr

    @staticmethod
    def calculate_pesq(reference: np.ndarray, degraded: np.ndarray, sr: int) -> float:
        """PESQ - Perceptual Evaluation of Speech Quality (1.0-4.5, higher is better)"""
        # PESQ requires 8kHz or 16kHz
        if sr not in [8000, 16000]:
            # Resample to 16kHz
            reference_resampled = librosa.resample(reference, orig_sr=sr, target_sr=16000)
            degraded_resampled = librosa.resample(degraded, orig_sr=sr, target_sr=16000)
            sr = 16000
        else:
            reference_resampled = reference
            degraded_resampled = degraded

        return pesq(sr, reference_resampled, degraded_resampled, 'wb')

    @staticmethod
    def calculate_stoi(reference: np.ndarray, degraded: np.ndarray, sr: int) -> float:
        """STOI - Short-Time Objective Intelligibility (0-1, higher is better)"""
        return stoi(reference, degraded, sr, extended=False)

    @staticmethod
    def calculate_spectral_convergence(reference: np.ndarray, estimate: np.ndarray) -> float:
        """Spectral convergence (lower is better, in dB)"""
        # Calculate spectrograms
        f_ref, t_ref, Sxx_ref = signal.spectrogram(reference, nperseg=512)
        f_est, t_est, Sxx_est = signal.spectrogram(estimate, nperseg=512)

        # Ensure same shape
        min_t = min(Sxx_ref.shape[1], Sxx_est.shape[1])
        Sxx_ref = Sxx_ref[:, :min_t]
        Sxx_est = Sxx_est[:, :min_t]

        # Calculate convergence
        numerator = np.sum((Sxx_ref - Sxx_est) ** 2)
        denominator = np.sum(Sxx_ref ** 2) + 1e-8

        return 10 * np.log10(numerator / denominator)


class VSTOptimizer:
    """Optimize VST parameters using Bayesian optimization"""

    def __init__(
        self,
        vst_path: str,
        noisy_audio: np.ndarray,
        reference_audio: np.ndarray,
        sample_rate: int,
        output_dir: Path = Path("optimization_results")
    ):
        self.vst_path = vst_path
        self.noisy_audio = noisy_audio
        self.reference_audio = reference_audio
        self.sample_rate = sample_rate
        self.output_dir = output_dir
        self.output_dir.mkdir(exist_ok=True)

        self.metrics = AudioMetrics()
        self.best_score = -np.inf
        self.best_params = None
        self.trial_count = 0

    def process_audio(self, params: Dict[str, float]) -> np.ndarray:
        """Process audio through VST with given parameters"""
        # Load VST
        try:
            vst = load_plugin(self.vst_path)
        except Exception as e:
            print(f"❌ Failed to load VST: {e}")
            raise

        # Debug info on first trial
        if self.trial_count == 1:
            print(f"\n🔧 VST loaded successfully")
            print(f"   Input audio shape: {self.noisy_audio.shape}")
            print(f"   Sample rate: {self.sample_rate}")

        # CRITICAL: Disable Easy Mode so advanced parameters work!
        vst.easy_mode = False

        # Set parameters (exact names from VST inspection)
        param_mapping = {
            'noise_reduction': 'noise_reduction',
            'noise_mode': 'use_dtln',  # Boolean: False=Normal (DSP), True=Aggressive (DTLN)
            'reverb_reduction': 'de_verb_room',
            'proximity': 'proximity_closeness',
            'clarity': 'clarity',
            'de_esser': 'de_esser',
            'leveler': 'leveler_auto_volume',
            'breath_control': 'breath_control',
        }

        for param_key, param_value in params.items():
            if param_key in param_mapping:
                vst_param_name = param_mapping[param_key]
                # Special handling for boolean parameters
                if param_key == 'noise_mode':
                    # Convert 0/1 to boolean for use_dtln
                    value_to_set = bool(param_value > 0.5)
                else:
                    value_to_set = param_value

                # Set parameter
                try:
                    setattr(vst, vst_param_name, value_to_set)
                    if self.trial_count == 1:
                        print(f"   Set {vst_param_name} = {value_to_set}")
                except AttributeError:
                    if self.trial_count == 1:
                        print(f"⚠️  Parameter '{vst_param_name}' not found in VST")
                except Exception as e:
                    if self.trial_count == 1:
                        print(f"⚠️  Failed to set '{vst_param_name}': {e}")

        # Create pedalboard and process
        board = Pedalboard([vst])

        # Ensure stereo (plugin expects stereo)
        if len(self.noisy_audio.shape) == 1:
            audio_stereo = np.stack([self.noisy_audio, self.noisy_audio])
        else:
            audio_stereo = self.noisy_audio.T

        # Validate input shape
        if audio_stereo.shape[0] != 2:
            raise ValueError(f"Expected stereo audio (2, N), got shape {audio_stereo.shape}")

        # Debug on first trial
        if self.trial_count == 1:
            print(f"   Audio prepared for VST: {audio_stereo.shape}")

        # Process with explicit buffer size
        # Some VSTs need proper buffer sizing
        try:
            processed = board.process(audio_stereo, self.sample_rate, buffer_size=512)
        except Exception as e:
            if self.trial_count == 1:
                print(f"⚠️  Error with buffer_size parameter: {e}, trying direct call...")
            # Fallback to direct call
            processed = board(audio_stereo, self.sample_rate)

        # Debug on first trial
        if self.trial_count == 1:
            print(f"   VST output shape: {processed.shape}")
            print(f"   Expected shape: {audio_stereo.shape}")

        # Validate output shape
        if processed.shape != audio_stereo.shape:
            raise ValueError(f"VST output shape mismatch: expected {audio_stereo.shape}, got {processed.shape}")

        # Convert processed audio to match reference format
        # Processed is (channels, samples), reference is (samples,) or (samples, channels)
        if len(self.reference_audio.shape) == 1:
            # Reference is mono, convert to mono
            processed = np.mean(processed, axis=0)
        else:
            # Reference is stereo, transpose to (samples, channels) format
            processed = processed.T

        if self.trial_count == 1:
            print(f"   Final processed shape: {processed.shape}")
            print(f"   Reference shape: {self.reference_audio.shape}")

        return processed

    def calculate_composite_score(
        self,
        processed: np.ndarray,
        reference: np.ndarray
    ) -> Tuple[float, Dict[str, float]]:
        """Calculate weighted composite score from multiple metrics"""
        # Convert stereo to mono if needed (most metrics require mono)
        if len(processed.shape) == 2:
            processed = np.mean(processed, axis=1)
        if len(reference.shape) == 2:
            reference = np.mean(reference, axis=1)

        # Ensure same length
        min_len = min(len(processed), len(reference))
        processed = processed[:min_len]
        reference = reference[:min_len]

        # Calculate individual metrics
        metrics_dict = {
            'si_sdr': self.metrics.calculate_si_sdr(reference, processed),
            'pesq': self.metrics.calculate_pesq(reference, processed, self.sample_rate),
            'stoi': self.metrics.calculate_stoi(reference, processed, self.sample_rate),
            'snr': self.metrics.calculate_snr(reference, processed),
            'spectral_conv': -self.metrics.calculate_spectral_convergence(reference, processed),  # Negate (lower is better)
        }

        # Weighted composite score
        # Adjust weights based on what matters most for your use case
        weights = {
            'si_sdr': 0.3,      # Scale-invariant quality
            'pesq': 0.25,       # Perceptual quality
            'stoi': 0.25,       # Intelligibility
            'snr': 0.1,         # Basic SNR
            'spectral_conv': 0.1,  # Spectral accuracy
        }

        # Normalize PESQ to 0-1 range (PESQ range is -0.5 to 4.5)
        normalized_pesq = (metrics_dict['pesq'] + 0.5) / 5.0

        # Normalize SI-SDR (typical range -10 to 30)
        normalized_si_sdr = (metrics_dict['si_sdr'] + 10) / 40.0

        # Normalize SNR (typical range 0 to 40)
        normalized_snr = metrics_dict['snr'] / 40.0

        # Normalize spectral convergence (typical range -40 to 0, already negated)
        normalized_spec = (metrics_dict['spectral_conv'] + 40) / 40.0

        # STOI is already 0-1

        composite_score = (
            weights['si_sdr'] * normalized_si_sdr +
            weights['pesq'] * normalized_pesq +
            weights['stoi'] * metrics_dict['stoi'] +
            weights['snr'] * normalized_snr +
            weights['spectral_conv'] * normalized_spec
        )

        return composite_score, metrics_dict

    def objective(self, trial: optuna.Trial) -> float:
        """Objective function for Optuna optimization"""
        self.trial_count += 1

        # Define parameter search space
        # Using 5% steps (0.05) for realistic precision
        params = {
            'noise_reduction': trial.suggest_float('noise_reduction', 0.0, 1.0, step=0.05),
            'noise_mode': trial.suggest_categorical('noise_mode', [0.0, 1.0]),  # Normal or Aggressive
            'reverb_reduction': trial.suggest_float('reverb_reduction', 0.0, 1.0, step=0.05),
            'proximity': trial.suggest_float('proximity', 0.0, 0.5, step=0.05),  # Usually don't want too much
            'clarity': trial.suggest_float('clarity', 0.0, 1.0, step=0.05),
            'de_esser': trial.suggest_float('de_esser', 0.0, 0.7, step=0.05),
            'leveler': trial.suggest_float('leveler', 0.0, 0.8, step=0.05),
            'breath_control': trial.suggest_float('breath_control', 0.0, 0.5, step=0.05),
        }

        try:
            # Process audio
            processed = self.process_audio(params)

            # Calculate score
            score, metrics = self.calculate_composite_score(processed, self.reference_audio)

            # Save if best so far
            if score > self.best_score:
                self.best_score = score
                self.best_params = params.copy()

                # Save best audio
                output_path = self.output_dir / f"best_trial_{self.trial_count}_score_{score:.4f}.wav"
                sf.write(output_path, processed, self.sample_rate)

                print(f"\n🎯 New best! Trial {self.trial_count}, Score: {score:.4f}")
                print(f"   Metrics: SI-SDR={metrics['si_sdr']:.2f}, PESQ={metrics['pesq']:.2f}, "
                      f"STOI={metrics['stoi']:.3f}, SNR={metrics['snr']:.2f}dB")
                print(f"   Params: {params}")
                print(f"   Saved to: {output_path}")

            return score

        except Exception as e:
            print(f"Error in trial {self.trial_count}: {e}")
            return -np.inf

    def optimize(self, n_trials: int = 100) -> Dict[str, float]:
        """Run optimization"""
        print(f"Starting optimization with {n_trials} trials...")
        print(f"VST: {self.vst_path}")
        print(f"Sample rate: {self.sample_rate} Hz")
        print(f"Audio length: {len(self.noisy_audio) / self.sample_rate:.2f} seconds")

        # Create Optuna study
        study = optuna.create_study(
            direction='maximize',
            sampler=optuna.samplers.TPESampler(seed=42)
        )

        # Run optimization
        study.optimize(self.objective, n_trials=n_trials)

        print(f"\n✅ Optimization complete!")

        # Check if we found any valid results
        if self.best_params is None or self.best_score == -np.inf:
            print("❌ No valid trials completed successfully!")
            print("All trials failed. Please check:")
            print("  1. VST path is correct and VST loads properly")
            print("  2. Audio files are compatible")
            print("  3. Check error messages above for details")

            # Save study results anyway for debugging
            study_path = self.output_dir / "optimization_study.csv"
            study.trials_dataframe().to_csv(study_path, index=False)
            print(f"\nStudy results (failed) saved to: {study_path}")

            return None

        print(f"Best score: {self.best_score:.4f}")
        print(f"Best parameters:")
        for param, value in self.best_params.items():
            print(f"  {param}: {value:.3f}")

        # Save study results
        study_path = self.output_dir / "optimization_study.csv"
        study.trials_dataframe().to_csv(study_path, index=False)
        print(f"\nStudy results saved to: {study_path}")

        return self.best_params


def main():
    parser = argparse.ArgumentParser(description='Optimize VST parameters for audio denoising')
    parser.add_argument('--noisy', required=True, help='Path to noisy audio file')
    parser.add_argument('--reference', required=True, help='Path to professionally cleaned reference audio')
    parser.add_argument('--vst', required=True, help='Path to VST3 plugin')
    parser.add_argument('--trials', type=int, default=100, help='Number of optimization trials')
    parser.add_argument('--output', default='optimization_results', help='Output directory')

    args = parser.parse_args()

    # Load audio files
    print("Loading audio files...")
    noisy, sr_noisy = sf.read(args.noisy)
    reference, sr_ref = sf.read(args.reference)

    # Ensure same sample rate
    if sr_noisy != sr_ref:
        print(f"Resampling reference from {sr_ref}Hz to {sr_noisy}Hz")
        reference = librosa.resample(reference, orig_sr=sr_ref, target_sr=sr_noisy)
        sr_ref = sr_noisy

    # Ensure same length (trim to shorter)
    min_len = min(len(noisy), len(reference))
    noisy = noisy[:min_len]
    reference = reference[:min_len]

    print(f"Loaded {min_len / sr_noisy:.2f} seconds of audio at {sr_noisy}Hz")

    # Create optimizer
    optimizer = VSTOptimizer(
        vst_path=args.vst,
        noisy_audio=noisy,
        reference_audio=reference,
        sample_rate=sr_noisy,
        output_dir=Path(args.output)
    )

    # Run optimization
    best_params = optimizer.optimize(n_trials=args.trials)

    # Save best parameters as JSON if successful
    if best_params is not None:
        import json
        params_path = Path(args.output) / "best_parameters.json"
        with open(params_path, 'w') as f:
            json.dump(best_params, f, indent=2)
        print(f"\nBest parameters saved to: {params_path}")
    else:
        print("\n❌ Optimization failed - no parameters to save")
        import sys
        sys.exit(1)


if __name__ == '__main__':
    main()
