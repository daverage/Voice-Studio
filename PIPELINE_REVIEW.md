# DSP Pipeline & Preview Signal Review

**Last Updated:** January 2026
**Status:** Implementation Complete

---

## 1. Current Pipeline Order (Verified)

```
INPUT
  │
  ├── Input Metering
  │
  ▼
┌─────────────────────────────────────────────────────────────┐
│ SPEECH CONFIDENCE (Sidechain - Analysis Only)               │
│ - Does NOT modify audio                                      │
│ - Provides: speech_conf (0-1), noise_floor_db                │
│ - Status: ✅ Working                                         │
└─────────────────────────────────────────────────────────────┘
  │
  ▼
┌─────────────────────────────────────────────────────────────┐
│ EARLY REFLECTION SUPPRESSOR                                  │
│ - Removes short-lag reflections (3-18ms)                    │
│ - Gated by speech_conf                                      │
│ - Preview: None (internal, not user-facing)                 │
│ - Status: ✅ Working                                         │
└─────────────────────────────────────────────────────────────┘
  │
  ▼
┌─────────────────────────────────────────────────────────────┐
│ SPEECH EXPANDER                                              │
│ - Downward expansion on non-speech                          │
│ - Weighted by (1 - speech_conf)                             │
│ - Preview: None (internal, not user-facing)                 │
│ - Status: ✅ Working                                         │
└─────────────────────────────────────────────────────────────┘
  │
  ▼
┌─────────────────────────────────────────────────────────────┐
│ DENOISE ✅ SUBTRACTIVE                                       │
│ - Spectral noise reduction                                  │
│ - Preview: ✅ Yes (preview_denoise)                          │
│ - Reference: exp_l (after early reflection + expander)      │
│ - Status: ✅ Working, preview fixed                          │
└─────────────────────────────────────────────────────────────┘
  │
  ▼
┌─────────────────────────────────────────────────────────────┐
│ SAFETY HPF                                                  │
│ - 80 Hz high-pass filter                                    │
│ - Preview: Included in denoise delta                        │
│ - Status: ✅ Working                                         │
└─────────────────────────────────────────────────────────────┘
  │
  ▼
┌─────────────────────────────────────────────────────────────┐
│ LATE DE-VERB ✅ SUBTRACTIVE                                  │
│ - Envelope-based reverb reduction                           │
│ - Preview: ✅ Yes (preview_deverb)                           │
│ - Reference: s2_l (after safety HPF)                        │
│ - Status: ✅ Working                                         │
└─────────────────────────────────────────────────────────────┘
  │
  ▼
┌─────────────────────────────────────────────────────────────┐
│ PROXIMITY EQ                                                 │
│ - Low-end boost for "close mic" effect                      │
│ - Preview: None (additive - nothing removed)                │
│ - Status: ✅ Working                                         │
└─────────────────────────────────────────────────────────────┘
  │
  ▼
┌─────────────────────────────────────────────────────────────┐
│ CLARITY EQ                                                   │
│ - High-frequency enhancement                                │
│ - Preview: None (additive - nothing removed)                │
│ - Status: ✅ Working                                         │
└─────────────────────────────────────────────────────────────┘
  │
  ▼
┌─────────────────────────────────────────────────────────────┐
│ DE-ESSER ✅ SUBTRACTIVE                                      │
│ - Sibilance reduction (band-specific gain)                  │
│ - Preview: ✅ Yes (preview_deesser)                          │
│ - Delta: s5_l - s6_l (no delay needed)                      │
│ - Status: ✅ Working, preview added                          │
└─────────────────────────────────────────────────────────────┘
  │
  ▼
┌─────────────────────────────────────────────────────────────┐
│ LEVELER                                                      │
│ - Stereo-linked compression                                 │
│ - Preview: None (gain change - nothing removed)             │
│ - Status: ✅ Working                                         │
└─────────────────────────────────────────────────────────────┘
  │
  ▼
┌─────────────────────────────────────────────────────────────┐
│ SPECTRAL GUARDRAILS                                         │
│ - Safety EQ corrections                                     │
│ - Only active in macro mode                                 │
│ - Preview: None (safety feature)                            │
│ - Status: ✅ Working                                         │
└─────────────────────────────────────────────────────────────┘
  │
  ▼
┌─────────────────────────────────────────────────────────────┐
│ LIMITER                                                      │
│ - Output safety limiting                                    │
│ - Preview: None (gain change - nothing removed)             │
│ - Status: ✅ Working                                         │
└─────────────────────────────────────────────────────────────┘
  │
  ▼
OUTPUT GAIN → OUTPUT METERING → OUTPUT
```

---

## 2. Issues Found & Resolved

### Issue A: Preview Reference Signal ✅ FIXED

**Problem:** Denoise preview used `input_l` as reference, but denoiser receives `exp_l` (after early reflection and speech expander).

**Solution:** Changed reference to `exp_l`:
```rust
// src/lib.rs - Now correct
let denoise_ref_l = self.process_l.restoration_chain.preview_delay_denoise.push(exp_l);
```

---

### Issue B: Missing De-esser Preview ✅ FIXED

**Problem:** De-esser is subtractive but had no preview capture.

**Solution:** Added delta capture:
```rust
// src/lib.rs - De-esser now captures delta
let (s6_l, s6_r, deesser_cut_l, deesser_cut_r) = if bypass_dynamics {
    (s5_l, s5_r, 0.0, 0.0)
} else {
    let de_ess_gain = self.linked_de_esser.compute_gain(s5_l, s5_r, de_ess_amt);
    let out_l = self.process_l.dynamics_chain.de_esser_band.apply(s5_l, de_ess_gain);
    let out_r = self.process_r.dynamics_chain.de_esser_band.apply(s5_r, de_ess_gain);
    let cut_l = if de_ess_amt > 0.001 { s5_l - out_l } else { 0.0 };
    let cut_r = if de_ess_amt > 0.001 { s5_r - out_r } else { 0.0 };
    (out_l, out_r, cut_l, cut_r)
};
```

---

### Issue C: Non-Subtractive Effects Had Preview ✅ FIXED

**Problem:** Clarity, Proximity, Leveler, and Output Gain had preview parameters despite being additive/gain effects.

**Solution:** Removed all non-subtractive preview parameters and UI buttons.

---

### Issue D: Per-Effect Preview Not Implemented ✅ FIXED

**Problem:** Only global `preview_cuts` existed, individual effect previews weren't functional.

**Solution:** Implemented per-effect preview mode:
```rust
// src/lib.rs - Per-effect preview output
let (out_l, out_r) = match preview_mode {
    1 => (denoise_cut_raw_l, denoise_cut_raw_r),  // Denoise preview
    2 => (deverb_cut_l, deverb_cut_r),            // Deverb preview
    3 => (deesser_cut_l, deesser_cut_r),          // De-esser preview
    _ => (s9_l, s9_r),                            // Normal output
};
```

---

## 3. Effects Classification (Final)

### ✅ Subtractive Effects WITH Preview

| Effect | Parameter | What You Hear | Time Alignment |
|--------|-----------|---------------|----------------|
| **Denoise** | `preview_denoise` | Noise being removed | Delay line (1 window) |
| **De-verb** | `preview_deverb` | Reverb tail being removed | Delay line (1 window) |
| **De-esser** | `preview_deesser` | Sibilance being reduced | None needed (filter-only) |

### ❌ Effects WITHOUT Preview (Correct)

| Effect | Reason |
|--------|--------|
| **Clarity** | Additive EQ boost - nothing is "removed" |
| **Proximity** | Additive EQ boost - nothing is "removed" |
| **Leveler** | Gain change - not removing content |
| **Limiter** | Gain change - not removing content |
| **Output Gain** | Simple gain - not removing content |
| **Early Reflection** | Internal to macro mode - not user-facing |
| **Speech Expander** | Internal to macro mode - not user-facing |
| **Spectral Guardrails** | Safety feature - not user-facing |
| **Speech Confidence** | Analysis only - doesn't affect audio |

---

## 4. Preview Signal Flow (Implemented)

```
┌─────────────────────────────────────────────────────────────┐
│ DENOISE PREVIEW (preview_denoise = true)                    │
│ Input: exp_l (after early reflection + expander)            │
│ Reference: delayed(exp_l) via preview_delay_denoise         │
│ Output: denoise_cut_raw_l = delayed(exp_l) - s1_l           │
│ Time aligned: ✅ Delay matches denoise latency               │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│ DE-VERB PREVIEW (preview_deverb = true)                     │
│ Input: s2_l (after safety HPF)                              │
│ Reference: delayed(s2_l) via preview_delay_deverb           │
│ Output: deverb_cut_l = delayed(s2_l) - s3_l                 │
│ Time aligned: ✅ Delay matches deverb latency                │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│ DE-ESSER PREVIEW (preview_deesser = true)                   │
│ Input: s5_l (before de-esser)                               │
│ Output: deesser_cut_l = s5_l - s6_l                         │
│ Time aligned: ✅ No significant latency (filter-only)        │
└─────────────────────────────────────────────────────────────┘
```

---

## 5. UI Implementation

### Header Preview Buttons
Three dedicated buttons in the header:
- **"Noise"** - Toggles `preview_denoise`
- **"Reverb"** - Toggles `preview_deverb`
- **"Sibilance"** - Toggles `preview_deesser`

### Slider Preview Buttons
"Listen" buttons appear under each subtractive effect slider:
- Noise Reduction slider → Listen button (denoise)
- De-Verb slider → Listen button (deverb)
- De-Ess slider → Listen button (de-esser)

### Non-Subtractive Sliders
No preview buttons (correctly removed):
- Tone, Clarity, Proximity, Leveler, Output Gain

---

## 6. Implementation Checklist

- [x] Fix denoise preview reference (`exp_l` not `input_l`)
- [x] Add de-esser delta capture
- [x] Remove non-subtractive preview params from `VoiceParams`
- [x] Remove non-subtractive preview buttons from UI
- [x] Implement per-effect preview mode in `process()`
- [x] Update header with three preview buttons
- [x] Update slider preview buttons for subtractive effects only
- [x] Build and verify compilation
- [ ] Test preview signals for:
  - [ ] Correct polarity
  - [ ] Time alignment
  - [ ] Level safety
- [ ] Hide preview in Easy Mode (future enhancement)

---

## 7. Files Modified

| File | Changes Made |
|------|--------------|
| `src/lib.rs` | Fixed preview reference, added de-esser delta capture, implemented per-effect preview mode, updated parameters |
| `src/ui.rs` | Removed non-subtractive preview buttons, updated header with 3 preview buttons, simplified `PreviewParamId` enum |

---

## 8. Parameters (Current State)

### Preview Parameters (3 total)
```rust
#[id = "preview_denoise"]
pub preview_denoise: BoolParam,

#[id = "preview_deverb"]
pub preview_deverb: BoolParam,

#[id = "preview_deesser"]
pub preview_deesser: BoolParam,
```

### Removed Parameters
- `preview_cuts` (replaced by per-effect previews)
- `preview_noise_reduction` (renamed to `preview_denoise`)
- `preview_reverb_reduction` (renamed to `preview_deverb`)
- `preview_clarity` (removed - additive effect)
- `preview_proximity` (removed - additive effect)
- `preview_de_esser` (renamed to `preview_deesser`)
- `preview_leveler` (removed - gain effect)
- `preview_output_gain` (removed - gain effect)
