# Testing & Validation Checklist

## 1. Perceptual Test Cases

Run the following scenarios and verify behavior:

- [ ] **Close-mic Podcast**: Voice should remain warm and natural. Denoise and De-verb should do almost nothing.
- [ ] **Roomy Zoom Call**: Distance macro should pull the voice forward. Early reflection suppression should reduce boxiness.
- [ ] **Outdoor Noise Floor**: Denoise should significantly reduce wind/traffic noise without making the voice "watery".
- [ ] **HVAC Hum**: Verify that hum removal (if active) or spectral denoise effectively kills 50/60Hz harmonics.
- [ ] **Sibilant Voice**: De-esser should catch sharp 's' sounds without causing a lisp.

## 2. Edge Case Stability

- [ ] **Whisper + Noise**: Verify that noise reduction doesn't "eat" the whisper harmonics (should be capped).
- [ ] **Long Silence**: Ensure noise floor doesn't drift to infinity.
- [ ] **Sudden Loud Input**: Limiter should catch peaks without "crunchy" distortion.
- [ ] **Rapid Macro Dragging**: No clicks, pops, or audio dropouts during parameter automation.

## 3. Known Bad Inputs (Out of Scope)

The following inputs are known to cause artifacts and are NOT targeted by this plugin:

- **Music / Multi-instrumental**: Denoiser and Leveler will cause pumping and harmonic distortion.
- **Singing**: High-energy vocal melodies may be misclassified as noise or sibilance in extreme cases.
- **Heavy Reverb Vocals (Church/Cathedral)**: Beyond the 18ms early reflection window and late de-verb limits, the sound will remain diffuse.
- **Clipped / Distorted Source**: This plugin does not de-clip. Restoration may exaggerate existing distortion.
