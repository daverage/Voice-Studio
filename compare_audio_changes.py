import numpy as np
import librosa
import pyloudnorm as pyln
from scipy.signal import correlate

SR = 48000
N_FFT = 2048
HOP = 512

# ------------------------------------------------
# Core utilities
# ------------------------------------------------

def load(path):
    x, _ = librosa.load(path, sr=SR, mono=True)
    return x

def align(a, b):
    corr = correlate(b, a, mode="full")
    shift = np.argmax(corr) - (len(a) - 1)

    if shift > 0:
        b = b[shift:]
    else:
        a = a[-shift:]

    n = min(len(a), len(b))
    return a[:n], b[:n]

def lufs_match(x):
    meter = pyln.Meter(SR)
    l = meter.integrated_loudness(x)
    return pyln.normalize.loudness(x, l, -23.0)

def stft_db(x):
    S = librosa.stft(x, n_fft=N_FFT, hop_length=HOP, window="hann")
    return librosa.amplitude_to_db(np.abs(S) + 1e-9)

def silence_mask(x, thresh_db=-40):
    frame = int(0.02 * SR)
    hop = frame // 2
    frames = librosa.util.frame(x, frame_length=frame, hop_length=hop)
    rms = np.sqrt(np.mean(frames**2, axis=0))
    db = librosa.amplitude_to_db(rms + 1e-9)
    return db < thresh_db

def band_idx(freqs, lo, hi):
    return np.where((freqs >= lo) & (freqs < hi))[0]

# ------------------------------------------------
# Difference profile
# ------------------------------------------------

def diff_profile(A, B, label):
    A, B = align(A, B)
    A = lufs_match(A)
    B = lufs_match(B)

    SA = stft_db(A)
    SB = stft_db(B)
    delta = SB - SA

    freqs = librosa.fft_frequencies(sr=SR, n_fft=N_FFT)
    silence = silence_mask(A)
    T = min(SA.shape[1], len(silence))

    sil_idx = np.where(silence[:T])[0]
    spc_idx = np.where(~silence[:T])[0]

    bands = {
        "Sub (20–80 Hz)": (20, 80),
        "Low (80–200 Hz)": (80, 200),
        "Low-Mid (200–500 Hz)": (200, 500),
        "Mid (500–2k Hz)": (500, 2000),
        "Presence (2–5 kHz)": (2000, 5000),
        "Air (5–12 kHz)": (5000, 12000),
    }

    print(f"\n=== {label} ===\n")

    for name, (lo, hi) in bands.items():
        idx = band_idx(freqs, lo, hi)

        sil = np.mean(delta[idx][:, sil_idx]) if len(sil_idx) else 0
        spc = np.mean(delta[idx][:, spc_idx]) if len(spc_idx) else 0

        varA = np.var(SA[idx][:, sil_idx]) if len(sil_idx) else 0
        varB = np.var(SB[idx][:, sil_idx]) if len(sil_idx) else 0
        var = 10 * np.log10((varB + 1e-9) / (varA + 1e-9))

        print(f"{name}")
        print(f"  Silence Δ: {sil:+.1f} dB")
        print(f"  Speech  Δ: {spc:+.1f} dB")
        print(f"  Silence variance Δ: {var:+.1f} dB\n")

# ------------------------------------------------
# Entry
# ------------------------------------------------

if __name__ == "__main__":
    original = load("/Users/andrzejmarczewski/Documents/GitHub/Voice-Studio-Rollback/test_data/notclean.wav")
    reference = load("/Users/andrzejmarczewski/Documents/GitHub/Voice-Studio-Rollback/test_data/proclean.wav")
    yours = load("/Users/andrzejmarczewski/Documents/GitHub/Voice-Studio-Rollback/test_data/myclean.wav")

    diff_profile(original, reference, "A → B  (Original → Other Tool)")
    diff_profile(original, yours,     "A → C  (Original → Your Tool)")
    diff_profile(yours, reference,     "C → B  (Your Tool → Other Tool)")
