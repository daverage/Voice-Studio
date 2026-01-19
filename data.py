import json
from pathlib import Path

import numpy as np
import soundfile as sf
from scipy.signal import stft

# -----------------------------
# Config
# -----------------------------

AUDIO_DIR = Path("test_data")
OUTPUT_DIR = Path("analysis_output")
OUTPUT_DIR.mkdir(exist_ok=True)

FRAME_SIZE = 1024
HOP_SIZE = 256

# Frequency bands (Hz)
BANDS = {
    "low": (80, 250),
    "low_mid": (250, 1000),
    "presence": (2000, 5000),
    "air": (6000, 12000),
}

# -----------------------------
# Helpers
# -----------------------------


def rms(x):
    return np.sqrt(np.mean(x**2) + 1e-12)


def db(x):
    return 20 * np.log10(np.maximum(x, 1e-12))


def frame_signal(x, frame, hop):
    frames = []
    for i in range(0, len(x) - frame, hop):
        frames.append(x[i : i + frame])
    return np.array(frames)


def band_energy(freqs, spectrum, band):
    lo, hi = band
    idx = np.logical_and(freqs >= lo, freqs < hi)
    return np.mean(spectrum[idx] ** 2)


# -----------------------------
# Analysis
# -----------------------------


def analyze_file(path):
    audio, sr = sf.read(path)
    if audio.ndim > 1:
        audio = np.mean(audio, axis=1)

    audio = audio / (np.max(np.abs(audio)) + 1e-9)

    # --- Level metrics ---
    peak = np.max(np.abs(audio))
    rms_val = rms(audio)
    crest = db(peak / rms_val)

    frames = frame_signal(audio, FRAME_SIZE, HOP_SIZE)
    frame_rms = np.array([rms(f) for f in frames])
    rms_variance = np.var(frame_rms)

    # --- Noise floor ---
    quiet_frames = frame_rms < np.percentile(frame_rms, 20)
    noise_floor = np.mean(frame_rms[quiet_frames])
    snr = db(rms_val / (noise_floor + 1e-9))

    # --- STFT ---
    freqs, times, Zxx = stft(
        audio,
        fs=sr,
        nperseg=FRAME_SIZE,
        noverlap=FRAME_SIZE - HOP_SIZE,
    )
    mag = np.abs(Zxx)

    # --- Spectral bands ---
    band_energies = {
        name: band_energy(freqs, np.mean(mag, axis=1), band)
        for name, band in BANDS.items()
    }

    total_energy = sum(band_energies.values()) + 1e-12
    band_ratios = {k: v / total_energy for k, v in band_energies.items()}

    # --- High-frequency variance (hiss / artifacts proxy) ---
    hf_band = (freqs >= 6000) & (freqs < 12000)
    hf_energy = mag[hf_band, :]
    hf_variance = np.var(hf_energy)

    # --- Room proxies ---
    # Early vs late energy: first 50ms vs next 200ms
    early_len = int(0.05 * sr)
    late_len = int(0.25 * sr)

    early_energy = rms(audio[:early_len])
    late_energy = rms(audio[early_len:late_len])
    early_late_ratio = early_energy / (late_energy + 1e-9)

    # Decay slope proxy
    energy_envelope = frame_rms
    decay_slope = np.polyfit(np.arange(len(energy_envelope)), db(energy_envelope), 1)[0]

    return {
        "file": path.name,
        "sample_rate": sr,
        "level": {
            "rms": rms_val,
            "peak": peak,
            "crest_db": crest,
            "rms_variance": rms_variance,
        },
        "noise": {
            "noise_floor": noise_floor,
            "snr_db": snr,
        },
        "room": {
            "early_late_ratio": early_late_ratio,
            "decay_slope": decay_slope,
        },
        "tone": {
            "band_ratios": band_ratios,
            "hf_variance": hf_variance,
        },
    }


# -----------------------------
# Run
# -----------------------------


def main():
    results = []

    for wav in AUDIO_DIR.glob("*.wav"):
        profile = analyze_file(wav)
        results.append(profile)

        out_path = OUTPUT_DIR / f"{wav.stem}_profile.json"
        with open(out_path, "w") as f:
            json.dump(profile, f, indent=2)

        print(f"Analyzed {wav.name}")

    # Combined summary
    summary_path = OUTPUT_DIR / "summary.json"
    with open(summary_path, "w") as f:
        json.dump(results, f, indent=2)

    print("\nDone. Profiles written to analysis_output/")


if __name__ == "__main__":
    main()
