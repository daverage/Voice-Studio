# Auto-Tune Optimization Analysis

## 1. How the Best Trial is Decided

### Composite Score Calculation
The optimizer uses a **weighted composite score** from 5 audio quality metrics:

```python
weights = {
    'si_sdr': 0.3,           # 30% - Scale-Invariant Signal-to-Distortion Ratio
    'pesq': 0.25,            # 25% - Perceptual Evaluation of Speech Quality
    'stoi': 0.25,            # 25% - Short-Time Objective Intelligibility
    'snr': 0.1,              # 10% - Signal-to-Noise Ratio
    'spectral_conv': 0.1,    # 10% - Spectral Convergence
}
```

### Score Normalization (0-1 range)
Each metric is normalized before combining:

- **SI-SDR**: Assumes range -10 to 30 dB → normalized as `(value + 10) / 40`
- **PESQ**: Range -0.5 to 4.5 → normalized as `(value + 0.5) / 5.0`
- **STOI**: Already 0-1 (intelligibility percentage)
- **SNR**: Assumes range 0 to 40 dB → normalized as `value / 40`
- **Spectral Convergence**: Range -40 to 0 dB → normalized as `(value + 40) / 40`

### Final Score Formula
```python
composite_score = (
    0.3 * norm_si_sdr +
    0.25 * norm_pesq +
    0.25 * stoi +
    0.1 * norm_snr +
    0.1 * norm_spec_conv
)
```

**Result**: Score of 0.0-1.0 where **higher is better**

### Current Score: 0.2457
This means the processed audio is achieving:
- Only **24.57%** of the maximum possible quality
- SI-SDR = -26.64 dB (very poor - reference is clean audio!)
- PESQ = 2.71 (fair perceptual quality)
- STOI = 0.447 (44.7% intelligibility - poor)
- SNR = -1.73 dB (worse than input!)

**⚠️ WARNING**: These scores suggest the optimization may be broken or the reference audio is incompatible.

---

## 2. Decimal Precision - Are We Being Too Clever?

### Current Precision
Optuna suggests values like: `0.3745401188473625`

### VST Parameter Reality
```rust
// All parameters are 0.0 to 1.0 linear ranges:
noise_reduction: FloatParam::new("Noise Reduction", 0.0,
    FloatRange::Linear { min: 0.0, max: 1.0 })

// Displayed to user as percentages:
// 0.3745401188473625 → "37%" in UI
```

### Is This Too Precise?

**YES - It's misleading precision!**

#### Why Extra Decimals Don't Matter:

1. **Human Perception**: Users can't hear difference between 37.4% and 37.5%
2. **DAW Display**: Most DAWs round to whole percentages
3. **VST Smoothing**: Parameters have 50ms smoothing - transitions blur precision
4. **Perceptual Curve**: Our DSP uses perceptual curves (x^1.5, x^2.2) that compress differences
5. **Practical Control**: Physical knobs/sliders can't achieve 0.01% precision

#### Recommended Precision:
- **0.01 (1%)** steps would be more realistic
- **0.05 (5%)** steps would be totally reasonable for optimization
- **0.1 (10%)** steps would still capture meaningful differences

---

## 3. JSON Values vs VST Parameters

### Current Mapping (Direct 1:1)

```python
# JSON Parameter → VST Parameter
'noise_reduction': 0.375  →  noise_reduction = 37.5%
'reverb_reduction': 0.599 →  de_verb_room = 59.9%
'proximity': 0.078        →  proximity_closeness = 7.8%
'clarity': 0.156          →  clarity = 15.6%
'de_esser': 0.041         →  de_esser = 4.1%
'leveler': 0.693          →  leveler_auto_volume = 69.3%
'breath_control': 0.301   →  breath_control = 30.1%
```

### VST Internal Range (from src/lib.rs)
All parameters are `FloatRange::Linear { min: 0.0, max: 1.0 }`, so:

- **JSON 0.0** = VST 0.0 = **0% in UI** = Minimum effect
- **JSON 1.0** = VST 1.0 = **100% in UI** = Maximum effect
- **JSON 0.5** = VST 0.5 = **50% in UI** = Half effect

### Special Case: noise_mode
```python
'noise_mode': 0.0 or 1.0  →  use_dtln = False or True
```
This is converted to boolean: `bool(value > 0.5)`

### What Actually Happens Inside VST

The 0-1 value is then processed through:

1. **Perceptual curves** (from our DSP stability work):
   ```rust
   perceptual_curve(0.5) = 0.5     // Gentle at midpoint
   perceptual_curve(0.75) = 0.79   // Gets steeper
   perceptual_curve(1.0) = 1.0     // Max

   aggressive_tail(0.5) = 0.125    // Very gentle until 70%
   aggressive_tail(0.7) = 0.385    // Starts ramping
   aggressive_tail(1.0) = 1.0      // Full effect
   ```

2. **Inter-module safety clamps**:
   ```rust
   // Reduce clarity by 30% when proximity > 0.4
   if proximity > 0.4 { clarity *= 0.7 }

   // Reduce deverb by 25% when proximity/clarity > 0.6
   if proximity > 0.6 || clarity > 0.6 { reverb *= 0.75 }
   ```

3. **Speech-aware scaling**:
   ```rust
   // Reduce max during voiced speech (60-100% based on confidence)
   max_cut = speech_weighted(MAX_CUT_DB, speech_confidence)
   ```

**Result**: The 0-1 JSON value goes through multiple transformations before affecting audio!

---

## 4. What Is This Test Actually Trying to Achieve?

### Goal
**Find optimal VST parameter settings** that make the noisy audio match the professionally cleaned reference as closely as possible.

### Input Files
- **Noisy**: `/test_data/notclean.wav` - Raw, unprocessed speech with noise
- **Reference**: `/test_data/proclean.wav` - Professionally cleaned version of same audio

### What Success Looks Like
If optimization works perfectly:
1. **High composite score** (0.7-0.9 range)
2. **SI-SDR > 10 dB** (processed matches reference well)
3. **PESQ > 3.5** (good perceptual quality)
4. **STOI > 0.85** (85%+ intelligibility)
5. **Processed audio sounds like reference**

### Current Problem
**All trials got identical score of 0.2457** which suggests:

#### Possibility 1: Reference/Noisy Mismatch
```bash
# Check if files are actually the same recording
ffprobe test_data/notclean.wav
ffprobe test_data/proclean.wav

# Are they time-aligned?
# Are they the same speaker?
# Is reference actually "professionally cleaned" or just different audio?
```

#### Possibility 2: Metrics Comparing Wrong Thing
```python
# Currently comparing:
processed_cleaned_audio vs professional_reference

# Should be comparing:
processed_noisy_audio vs professional_reference
```

#### Possibility 3: All Parameters Having Same Effect
- Parameters might be in wrong ranges
- VST might not be processing audio
- Metrics might be broken

### What Should Happen

**Trial 1** (low settings):
```json
{"noise_reduction": 0.1, "clarity": 0.2, ...}
→ Minimal processing → Still noisy → Low score
```

**Trial 50** (optimized):
```json
{"noise_reduction": 0.65, "clarity": 0.45, ...}
→ Good processing → Matches reference → High score
```

**Trial 100** (too aggressive):
```json
{"noise_reduction": 1.0, "clarity": 1.0, ...}
→ Over-processing → Artifacts → Medium score
```

### Recommended Next Steps

1. **Verify audio files are actually different**:
```bash
python3 << 'EOF'
import soundfile as sf
import numpy as np

noisy, _ = sf.read('test_data/notclean.wav')
clean, _ = sf.read('test_data/proclean.wav')

print(f"Noisy RMS: {np.sqrt(np.mean(noisy**2)):.6f}")
print(f"Clean RMS: {np.sqrt(np.mean(clean**2)):.6f}")
print(f"Difference: {np.abs(noisy - clean).mean():.6f}")
EOF
```

2. **Test with extreme settings manually**:
```python
# Process with all parameters at 0.0 (off)
# Process with all parameters at 1.0 (max)
# Compare scores - should be VERY different
```

3. **Reduce search precision**:
```python
# Change from:
trial.suggest_float('noise_reduction', 0.0, 1.0)
# To:
trial.suggest_float('noise_reduction', 0.0, 1.0, step=0.05)  # 5% steps
```

4. **Use simpler scoring for debugging**:
```python
# Just use SNR instead of composite
score = metrics['snr'] / 40.0
```

---

## Summary

| Aspect | Current State | Recommendation |
|--------|--------------|----------------|
| **Score Calculation** | Complex weighted composite | ✅ Good for final optimization |
| **Decimal Precision** | 16 decimal places | ❌ Use 0.01 or 0.05 steps |
| **JSON → VST Mapping** | Direct 1:1 (0-1 range) | ✅ Correct |
| **Test Goal** | Match noisy → reference | ✅ Clear goal |
| **Current Results** | All trials = 0.2457 | ❌ Something is wrong |

**Priority**: Debug why all trials get same score before running 100 trials!
