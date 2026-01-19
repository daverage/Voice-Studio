# DSP Settings Analysis & Debug Meters

**Purpose:** Document all DSP settings, identify potential concerns, and provide debugging capabilities.

---

## 1. Summary of Key Settings by Module

### Denoiser
| Setting | Value | Purpose | Concern Level |
|---------|-------|---------|---------------|
| Harmonic F0 range | 50-450 Hz | Voice fundamental detection | OK - covers most voices |
| Harmonic protection | Up to 8 kHz | Preserve voice harmonics | OK |
| Noise release | 0.9995 (very slow) | Prevent pumping | OK - conservative |
| Speech floor | 0.98 max | Preserve transients | OK |

### Deverber
| Setting | Value | Purpose | Concern Level |
|---------|-------|---------|---------------|
| Late decay HF | 0.85 | Fast HF reverb decay | **WATCH** - may be aggressive |
| F0 range | 70-320 Hz | Voice detection | OK |
| Max suppression | 35% | Limit removal | OK - conservative |
| Harmonic protection | Up to 6 kHz | Preserve formants | OK |

### De-Esser
| Setting | Value | Purpose | Concern Level |
|---------|-------|---------|---------------|
| Sibilance band | 4.5-10 kHz | Target "S" sounds | OK - standard range |
| Ratio | 6:1 | Compression strength | OK - standard for de-essing |
| Max reduction | 18 dB | Maximum cut | OK - matched to UI |
| Band center | 7 kHz | Notch frequency | **WATCH** - may need to be dynamic |

### Compressor/Leveler
| Setting | Value | Purpose | Concern Level |
|---------|-------|---------|---------------|
| Target level | -24 dBFS | Reference loudness | OK |
| Ratios | 1.6/2.2/3.2:1 | Three-stage compression | OK - graduated |
| Knee | 10 dB | Soft knee width | OK - transparent |
| Makeup scale | 0.45 | Conservative makeup | OK |

### Limiter
| Setting | Value | Purpose | Concern Level |
|---------|-------|---------|---------------|
| Ceiling | 0.98 (-0.17 dBFS) | Output limit | **WATCH** - very conservative |
| Peak attack | 0.3 ms | Transient catch | OK |
| Knee | 1.5 dB | Soft limiting | OK |

### Early Reflection Suppressor
| Setting | Value | Purpose | Concern Level |
|---------|-------|---------|---------------|
| Tap delays | 3, 7, 12, 18 ms | Room reflection times | OK - typical small room |
| Max suppression | 35% | Limit effect | OK |
| Min speech conf | 0.2 | Gate threshold | **WATCH** - may be too low |

### Speech Expander
| Setting | Value | Purpose | Concern Level |
|---------|-------|---------|---------------|
| Ratio | 2:1 | Expansion strength | OK - gentle |
| Max attenuation | 12 dB | Limit silence depth | OK |
| Hold time | 80 ms | Prevent chatter | OK |

### Spectral Guardrails
| Setting | Value | Purpose | Concern Level |
|---------|-------|---------|---------------|
| Max low-mid cut | 5 dB | Limit correction | OK - gentle |
| Max high cut | 5 dB | Limit correction | OK - gentle |
| Slew rate | 12 dB/sec | Prevent clicks | OK |
| Low-mid threshold | 1.5:1 ratio | Trigger point | OK |

### Speech Confidence
| Setting | Value | Purpose | Concern Level |
|---------|-------|---------|---------------|
| Frame size | 20 ms | Analysis window | OK - standard |
| Attack | 15 ms | Rise time | OK |
| Release | 120 ms | Fall time | OK |
| Hang time | 80 ms | Prevent false drops | OK |

---

## 2. Potential Concerns to Monitor

### High Priority
1. **Limiter ceiling (0.98)**: Very conservative - only 1.6 dB headroom before clipping. May cause aggressive limiting on peaks. Monitor `debug_limiter_gr_db`.

2. **Late decay HF (0.85)**: Fast decay at high frequencies could make reverb removal too aggressive, potentially affecting voice presence. Monitor deverb behavior on high-reverb material.

### Medium Priority
3. **De-esser band center (7 kHz)**: Fixed frequency may not suit all voices. Female voices may need higher (8-9 kHz), male voices might need lower (5-6 kHz).

4. **Early reflection min_speech_conf (0.2)**: Low threshold means suppression can engage even with weak speech detection. Monitor `debug_speech_confidence` and `debug_early_reflection` together.

### Low Priority
5. **Hum removal (90% reduction)**: Strong reduction at 50/60 Hz harmonics. Fine for most use cases but could affect low male voices near those frequencies.

---

## 3. Debug Meters Added

The following debug meters are now populated in `src/meters.rs` and can be accessed for analysis:

| Meter | Range | Description |
|-------|-------|-------------|
| `debug_speech_confidence` | 0.0 - 1.0 | Speech likelihood from estimator |
| `debug_noise_floor_db` | -80 to 0 dBFS | Estimated noise floor |
| `debug_deesser_gr_db` | 0 - 18 dB | De-esser gain reduction |
| `debug_limiter_gr_db` | 0 - ~20 dB | Limiter gain reduction |
| `debug_early_reflection` | 0.0 - 0.35 | Early reflection suppression |
| `debug_guardrails_low_cut` | 0 - 5 dB | Spectral guardrails low-mid cut |
| `debug_guardrails_high_cut` | 0 - 5 dB | Spectral guardrails high cut |
| `debug_expander_atten_db` | 0 - 12 dB | Speech expander attenuation |

### How to Use Debug Meters

**In Rust code:**
```rust
// Get debug values from meters
let speech_conf = meters.get_debug_speech_confidence();
let deesser_gr = meters.get_debug_deesser_gr_db();
let limiter_gr = meters.get_debug_limiter_gr_db();
// etc.
```

**For UI debugging:**
Add visual indicators in `src/ui.rs` using the getter methods to display real-time values.

---

## 4. Testing Recommendations

### Test Cases for Concern Areas

1. **Limiter Ceiling Test**
   - Input: Hot signal (-6 dBFS peaks)
   - Monitor: `debug_limiter_gr_db`
   - Expected: Should see moderate GR (2-6 dB)
   - Concern if: Constant heavy GR (>10 dB) even on normal speech

2. **High-Frequency Reverb Test**
   - Input: Speech in reverberant room
   - Monitor: Reverb removal vs. voice brightness
   - Expected: Reverb reduced without dulling voice
   - Concern if: Voice sounds "filtered" or lacks air

3. **De-Esser Frequency Test**
   - Input: Various voices (male/female)
   - Monitor: `debug_deesser_gr_db` during sibilants
   - Expected: 3-8 dB reduction on "S" sounds
   - Concern if: Reduction on non-sibilant content, or no reduction on obvious sibilants

4. **Speech Confidence Accuracy Test**
   - Input: Speech with pauses and background noise
   - Monitor: `debug_speech_confidence`
   - Expected: High (>0.7) during speech, low (<0.3) during silence
   - Concern if: Flickers rapidly or stays high during noise

5. **Expander Behavior Test**
   - Input: Speech with room ambience
   - Monitor: `debug_expander_atten_db`
   - Expected: 0 dB during speech, 3-12 dB during pauses
   - Concern if: Attenuates during speech or pumps audibly

---

## 5. Tuning Recommendations

If issues are found, here are suggested adjustments:

### If limiter is too aggressive:
```rust
// In src/dsp/limiter.rs
let ceiling = 0.95;  // Raise from 0.98 to 0.95 (-0.4 dB)
// Or widen the knee:
let knee_db = 2.5;   // Widen from 1.5 to 2.5
```

### If HF reverb removal is too strong:
```rust
// In src/dsp/deverber.rs
const LATE_DECAY_HIGH: f32 = 0.90;  // Slow from 0.85 to 0.90
```

### If de-esser misses some voices:
```rust
// In src/dsp/de_esser.rs
const DE_ESS_BAND_HZ: f32 = 6500.0;  // Lower from 7000 for male voices
// Or make it adaptive based on detected F0
```

### If early reflection suppresses too much:
```rust
// In src/dsp/early_reflection.rs
const MIN_SPEECH_CONF: f32 = 0.35;  // Raise from 0.2 to require more confidence
```

---

## 6. Files Modified for Debug Meters

| File | Changes |
|------|---------|
| `src/meters.rs` | Added 8 debug meter fields with setters/getters |
| `src/dsp/de_esser.rs` | Added `get_gain_reduction_db()` method |
| `src/dsp/limiter.rs` | Added `get_gain_reduction_db()` method |
| `src/lib.rs` | Wired up debug meter updates in process() |

---

## 7. Next Steps

1. **Add UI visualization** for debug meters (optional debug panel)
2. **Run test cases** with various audio material
3. **Adjust settings** based on observed behavior
4. **Document any changes** made to constants
