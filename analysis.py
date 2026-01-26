import json
import sys
import numpy as np
import soundfile as sf
import scipy.signal as sps

# -----------------------------
# Utility helpers
# -----------------------------

def rms(x):
    return np.sqrt(np.mean(x**2) + 1e-12)

def crest_factor_db(x):
    peak = np.max(np.abs(x)) + 1e-12
    return 20 * np.log10(peak / rms(x))

def stft_mag(x, sr, n_fft=2048, hop=512):
    _, _, Z = sps.stft(x, sr, nperseg=n_fft, noverlap=n_fft-hop)
    return np.abs(Z)

def spectral_centroid(mag, freqs):
    return np.sum(mag * freqs[:, None], axis=0) / (np.sum(mag, axis=0) + 1e-12)

def spectral_flatness(mag):
    geo = np.exp(np.mean(np.log(mag + 1e-12), axis=0))
    arith = np.mean(mag, axis=0) + 1e-12
    return geo / arith

def band_energy(mag, freqs, f_lo, f_hi):
    idx = np.where((freqs >= f_lo) & (freqs <= f_hi))[0]
    return np.mean(np.sum(mag[idx, :], axis=0))

def estimate_snr(mag, freqs):
    speech = band_energy(mag, freqs, 300, 3000)
    noise = band_energy(mag, freqs, 4000, freqs[-1])
    return 10 * np.log10((speech + 1e-12) / (noise + 1e-12))

# -----------------------------
# Main analysis
# -----------------------------

def analyze(input_wav, output_wav):
    x_in, sr = sf.read(input_wav)
    x_out, sr2 = sf.read(output_wav)

    assert sr == sr2, "Sample rate mismatch"

    # Mono collapse if needed
    if x_in.ndim > 1:
        x_in = np.mean(x_in, axis=1)
    if x_out.ndim > 1:
        x_out = np.mean(x_out, axis=1)

    mag_in = stft_mag(x_in, sr)
    mag_out = stft_mag(x_out, sr)

    freqs = np.linspace(0, sr / 2, mag_in.shape[0])

    report = {
        "levels": {
            "rms_in_db": 20 * np.log10(rms(x_in)),
            "rms_out_db": 20 * np.log10(rms(x_out)),
            "crest_in_db": crest_factor_db(x_in),
            "crest_out_db": crest_factor_db(x_out),
        },
        "spectral": {
            "centroid_in_hz": float(np.mean(spectral_centroid(mag_in, freqs))),
            "centroid_out_hz": float(np.mean(spectral_centroid(mag_out, freqs))),
            "flatness_in": float(np.mean(spectral_flatness(mag_in))),
            "flatness_out": float(np.mean(spectral_flatness(mag_out))),
        },
        "bands": {
            "speech_band_energy_in": band_energy(mag_in, freqs, 300, 3000),
            "speech_band_energy_out": band_energy(mag_out, freqs, 300, 3000),
            "noise_band_energy_in": band_energy(mag_in, freqs, 4000, freqs[-1]),
            "noise_band_energy_out": band_energy(mag_out, freqs, 4000, freqs[-1]),
        },
        "snr": {
            "snr_in_db": estimate_snr(mag_in, freqs),
            "snr_out_db": estimate_snr(mag_out, freqs),
            "snr_delta_db": estimate_snr(mag_out, freqs) - estimate_snr(mag_in, freqs),
        },
        "stability": {
            "hf_variance_in": float(np.var(mag_in[freqs > 6000])),
            "hf_variance_out": float(np.var(mag_out[freqs > 6000])),
        }
    }

    return report

# -----------------------------
# CLI entry
# -----------------------------

if __name__ == "__main__":
    if len(sys.argv) != 3:
        print("usage: analyze.py input.wav output.wav")
        sys.exit(1)

    report = analyze(sys.argv[1], sys.argv[2])
    print(json.dumps(report, indent=2))
