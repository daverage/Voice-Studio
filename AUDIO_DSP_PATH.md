# Audio Path Through VxCleaner DSP Chain

## Overview
This document outlines the complete audio processing path through the VxCleaner plugin, detailing each DSP module, its function, and frequency range characteristics.

## Always-On DSP Summary
These modules can modify the signal even when all user sliders are set to 0:
- **`SpeechHpf`**: 90Hz HPF (Q=0.707) hidden hygiene filter.
- **`HissRumble`**: Rumble HPF is always at 20Hz; hiss shelf is flat at 0.
- **`PinkRefBias`**: Speech-gated pink tilt correction, ±2 dB max via low shelf at 250Hz and high shelf at 4kHz.
- **`StereoStreamingDenoiser` / `DspDenoiser`**: MMSE-LSA gain stage still runs; 0 amount only resets history, not the gain calculation.
- **`PlosiveSoftener`**: Dynamic low-shelf at 150Hz, up to 8 dB attenuation on plosives.
- **`RestorationChain::safety_hpf`**: 80Hz HPF safety filter (Q=0.707).
- **`RecoveryStage`**: Speech-gated presence/air shelves (+1.5 to +2.5 dB @ 2.5kHz, +2 to +4 dB @ 10kHz).
- **`SpectralGuardrails`**: Conditional low-mid and high cuts up to 5 dB based on band ratios.
- **`LinkedLimiter`**: True-peak limiter at 0.98 (~-0.18 dBTP) engages on peaks.
- **Loudness compensation**: 10s time constant, gain clamped to 0.9–1.1.
- **Output safety clamp**: Scales output if absolute peak exceeds 4.0.

## Complete Audio Processing Chain

### 0. Input Preprocessing
**0a. Speech HPF (Hidden Hygiene)**
- **Module**: `SpeechHpf`
- **Function**: Removes subsonic energy before any analysis or processing
- **Frequency Range**: High-pass filter below 90Hz (Q=0.707)
- **Purpose**: Eliminates DC offset and subsonic rumble that could interfere with other processing
- **Always-On Note**: This filter is always in the signal path, even with all sliders at 0.

**0b. Envelope Tracking**
- **Module**: `VoiceEnvelopeTracker`
- **Function**: Unified source of truth for dynamics tracking after static noise removal
- **Frequency Range**: Full bandwidth (RMS, peak, slow envelope tracking)
- **Purpose**: Provides envelope information for expander and compressor modules

**0c. Input Profile Analysis**
- **Module**: `ProfileAnalyzer`
- **Function**: Analyzes pre-processing signal for data-driven calibration
- **Frequency Range**: Full bandwidth
- **Purpose**: Measures audio characteristics to inform processing decisions

**0d. Speech Confidence Detection**
- **Module**: `SpeechConfidenceEstimator`
- **Function**: Provides shared speech activity envelope for all modules
- **Frequency Range**: 250Hz - 4kHz (speech band)
- **Purpose**: Enables coordinated processing across multiple modules without duplication

### 0x. Static Noise Learning & Removal
**Module**: `NoiseLearnRemove`
- **Function**: Learns and removes static noise (hum, buzz, computer fans)
- **Frequency Range**: Full bandwidth
- **Purpose**: Removes consistent background noise that doesn't vary over time
 - **Behavior**: Always-on learning (when enabled) gated by low speech confidence with stability checks; a candidate profile must stabilize before updating the learned profile.

### 0x. Hiss & Rumble Processing
**Module**: `HissRumble`
- **Function**: Shapes tonal characteristics of hiss and rumble
- **Frequency Range**: 
  - Rumble: 20Hz - 120Hz HPF sweep
  - Hiss: 8kHz high-shelf cut up to -24 dB
- **Purpose**: Adjusts the tonal balance of noise components before denoising
- **Always-On Note**: Even at 0 settings, the rumble HPF remains at 20Hz and is still in the path.

### 1. Early Reflection Suppression
**Module**: `EarlyReflectionSuppressor`
- **Function**: Reduces short-lag reflections (3-18ms) that cause boxiness/distancing
- **Frequency Range**: Full bandwidth
- **Delay Times**: 3.0ms (desk/table), 7.0ms (side walls), 12.0ms (floor/ceiling), 18.0ms (opposite wall)
- **Purpose**: Makes recordings sound less distant by removing early room reflections

### 2. Speech Expander
**Module**: `SpeechExpander`
- **Function**: Controls pauses and room swell without hard gating
- **Frequency Range**: Full bandwidth
- **Purpose**: Reduces background noise during silent periods based on speech detection

### 3. Pink Reference Bias
**Module**: `PinkRefBias`
- **Function**: Hidden spectral tonal conditioning that gently nudges the speech portion toward a pink-noise-like long-term tilt (-3 dB/octave)
- **Frequency Range**:
  - Analysis: 150Hz - 6kHz (for spectral tilt measurement)
  - Correction: Two-band approach using low-shelf at 250Hz and high-shelf at 4kHz
- **Purpose**: Improves stability for downstream processing (denoise, de-ess, clarity, proximity) by ensuring spectral balance stays within a "natural" range
- **How Pink Noise is Used**: The module estimates the spectral tilt of the input signal and compares it to the target pink noise characteristic (-3 dB/octave). It then applies corrective filtering to gently nudge the signal toward this target. The pink noise reference serves as a "natural" spectral balance that is commonly found in speech and music, helping to maintain a pleasant tonal character.
- **Key Features**:
  - Gated by speech confidence (only active during speech)
  - Capped at ±2.0 dB total correction to prevent over-processing
  - Slow ballistics (2-second tilt averaging, gradual gain smoothing)
  - Safety adjustments when interacting with proximity or de-esser
- **Always-On Note**: Can apply low-shelf changes at 250Hz (and high-shelf at 4kHz) during speech even when all user sliders are 0.

### 4. Restoration Stage

**4a. Denoiser**
- **Module**: `StereoStreamingDenoiser` (based on `DspDenoiser`)
- **Function**: Reduces stationary background noise while preserving voice characteristics
- **Frequency Range**: Full bandwidth (adaptive per frequency bin)
- **Processing Type**: Spectral Wiener filtering with speech probability estimation
- **Special Features**: Harmonic protection, psychoacoustic masking, adaptive noise floor tracking
- **Always-On Note**: The MMSE-LSA gain stage still runs even at 0 amount; 0 only resets history, it does not fully bypass gain calculation.

**4b. Plosive Softener**
- **Module**: `PlosiveSoftener`
- **Function**: Reduces popping sounds from plosive consonants (p, t, k)
- **Frequency Range**: Primarily affects low frequencies (below 500Hz)
- **Purpose**: Removes explosive breath sounds without affecting speech quality
- **Key Values**: 150Hz low-shelf, threshold 0.08, max 8 dB attenuation, attack 1ms, release 50ms
- **Always-On Note**: Always in the path; only engages when plosive bursts are detected.

**4c. Breath Reducer**
- **Module**: `BreathReducer`
- **Function**: Reduces breath sounds during speech
- **Frequency Range**: 200Hz - 1kHz (breath frequency range)
- **Purpose**: Reduces audible breathing while preserving speech

**4d. Safety High-Pass Filter**
- **Module**: `Biquad` (configured as HPF)
- **Function**: Ensures minimum low-frequency roll-off for safety
- **Frequency Range**: Below 80Hz (80Hz HPF, Q=0.707)
- **Purpose**: Prevents excessive low-frequency buildup

**4e. Deverber**
- **Module**: `StreamingDeverber`
- **Function**: Reduces late reverb tail and diffuse room decay (>50ms)
- **Frequency Range**: Full bandwidth with frequency-dependent decay rates
- **Processing Type**: WOLA (Weighted Overlap-Add) processing
- **Special Features**: Harmonic protection, voiced speech detection, spectral masking

### 5. Shaping Stage

**5a. Proximity Effect**
- **Module**: `Proximity`
- **Function**: Restores low-end body (100-300Hz) for close-mic effect
- **Frequency Range**: 
  - Low-shelf boost: 180Hz center frequency
  - High-shelf cut: 8kHz center frequency
- **Purpose**: Simulates proximity effect of close-mic recording

**5b. Clarity Control**
- **Module**: `Clarity` (with `ClarityDetector`)
- **Function**: Reduces low-mid congestion (120-380Hz) during voiced speech
- **Frequency Range**: 
  - Detector: 120Hz - 380Hz (vocal body range)
  - Shaper: 250Hz center frequency with adjustable cut
- **Processing Type**: Dynamic low-mid cleanup triggered by voiced speech detection
- **Purpose**: Removes chesty/muddy sound while preserving natural voice character

### 6. Dynamics Stage

**6a. De-Esser**
- **Module**: `DeEsserBand` with `DeEsserDetector`
- **Function**: Reduces harsh sibilant sounds (s, sh, ch)
- **Frequency Range**: 
  - Sibilance detection: 4.5kHz - 10kHz
  - Primary band: 7kHz center frequency with Q=1.0
  - High-pass for sibilance: 4.5kHz
  - Low-pass for sibilance: 10kHz
- **Processing Type**: Dynamic EQ with dual-band detection
- **Purpose**: Controls harsh sibilants without affecting overall brightness

**6b. Leveler (Compressor)**
- **Module**: `LinkedCompressor`
- **Function**: Stereo-linked compressor for voice level consistency
- **Frequency Range**: Full bandwidth (RMS and peak detection)
- **Processing Type**: Hybrid RMS/Peak compression with program-dependent release
- **Special Features**: Speech-confidence gating, adaptive ratio, makeup gain
- **Purpose**: Maintains consistent loudness while preserving natural speech dynamics

**6c. Spectral Guardrails**
- **Module**: `SpectralGuardrails`
- **Function**: Safety limits for extreme settings to prevent artifacts
- **Frequency Range**: Full bandwidth with specific attention to low-mid (200-500Hz) and high (8-16kHz) bands
- **Purpose**: Prevents processing from creating unnatural or broken sounds
- **Key Values**: Up to 5 dB low-mid cut and 5 dB high cut when ratios exceed thresholds
- **Always-On Note**: Runs continuously and applies corrections only when ratios trip.

### 7. Recovery Stage
**Module**: `RecoveryStage`
- **Function**: Speech-gated EQ after all subtractive processing
- **Frequency Range**: 
  - Presence: ~2kHz-5kHz shelving
  - Air: ~8kHz-12kHz shelving
- **Purpose**: Compensates for losses during subtractive processing during speech
- **Key Values**: Presence +1.5 to +2.5 dB @ 2.5kHz, Air +2 to +4 dB @ 10kHz (speech gated)

### 7b. Post-Noise Cleanup (Second Pass)
**Module**: `PostNoiseCleanup`
- **Function**: Very light, confidence-gated attenuation to tuck residual noise revealed by recovery
- **Frequency Range**: Full bandwidth with a high-shelf bias toward upper bands
- **Behavior**: Engages only when speech confidence is low; falls back to envelope gating if confidence is flatlined
- **Purpose**: Cosmetic cleanup without altering tone or dynamics
- **Key Values**: Max 2–3 dB attenuation, fast attack, slower release, short hold
- **Options**: Optional HF bias toggle; can be bypassed by the Hidden FX toggle.
- **Controls**: Low-end protection toggle affects denoiser voicing protection in the first pass.

### 8. Output Stage

**8a. Limiter**
- **Module**: `LinkedLimiter`
- **Function**: True peak safety limiting
- **Frequency Range**: Full bandwidth
- **Processing Type**: Fast-attack, slow-release true peak limiting
- **Ceiling**: ~-0.18 dBTP (98% of full scale)
- **Purpose**: Prevents clipping while remaining transparent

**8b. Output Gain**
- **Module**: Simple gain multiplication
- **Function**: Final output level adjustment
- **Range**: -12dB to +12dB
- **Purpose**: Allows final level matching

**8c. Final Output Presets**
- **Module**: Integrated with loudness metering
- **Function**: Automatic loudness normalization and true-peak limiting
- **Processing**: LUFS-based gain adjustment with true-peak limiting
- **Purpose**: Delivers consistent loudness levels for distribution

**8d. Loudness Compensation (Always On)**
- **Module**: Internal gain normalization in the main process loop
- **Function**: Preserves pre-processing RMS within ±10% with slow smoothing
- **Processing**: 10-second time constant, gain clamped to 0.9–1.1
- **Purpose**: Prevents long-term loudness drift even when upstream processing changes level

**8e. Output Safety Clamp (Always On)**
- **Module**: Final peak safeguard in the main process loop
- **Function**: Scales output if absolute peak exceeds 4.0
- **Purpose**: Hard safety against runaway values or NaNs

## Key Design Principles

1. **Signal Flow Consistency**: All processing follows a logical sequence from noise reduction to tonal shaping to dynamics control.

2. **Shared Sidechain**: The `SpeechConfidenceEstimator` provides a single source of truth for speech detection across all modules.

3. **Frequency Domain Coordination**: Each module operates on specific frequency ranges to avoid conflicts and double-processing.

4. **Safety First**: Multiple layers of protection prevent artifacts from extreme settings.

5. **Real-Time Optimization**: All modules are designed for real-time processing with minimal computational overhead.

## Inter-Module Coordination

- Proximity and clarity modules coordinate to avoid conflicting adjustments
- De-esser and leveler adjust their behavior based on each other's activity
- De-verb strength is reduced when proximity is high (closer mic needs less de-verb)
- Spectral processing is gated by speech confidence to avoid processing noise
