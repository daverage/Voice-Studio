# DSP Stability & Perceptual Scaling - Complete Implementation Spec

## Status: IN PROGRESS
**Completed**: Curve helpers, Clarity, Proximity
**Remaining**: De-verb, Denoiser, Leveler, Main loop integration

---

## 1. COMPLETED WORK

### ✅ src/dsp/utils.rs - Perceptual Curve Helpers
```rust
/// Gentle 0-50%, aggressive 50-100%
pub fn perceptual_curve(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x <= 0.5 {
        (x / 0.5).powf(1.5) * 0.5
    } else {
        0.5 + ((x - 0.5) / 0.5).powf(2.2) * 0.5
    }
}

/// Preserves usability until ~70%, then ramps hard
pub fn aggressive_tail(x: f32) -> f32 {
    x.clamp(0.0, 1.0).powf(2.8)
}

/// Reduces max during voiced speech (60-100%)
pub fn speech_weighted(max: f32, speech_conf: f32) -> f32 {
    max * (0.6 + 0.4 * speech_conf.clamp(0.0, 1.0))
}
```

### ✅ src/dsp/clarity.rs - Aggressive Tail + Speech Protection
**Changes**:
- Import: `aggressive_tail, speech_weighted`
- Function signature: `process(input, clarity, speech_confidence, drive)`
- Apply `x = aggressive_tail(clarity)`
- Max cut: `speech_weighted(64.0, speech_confidence)`
- Hard limit: 48dB guardrail
- Voiced speech: 30dB max when `speech_confidence > 0.6`

### ✅ src/dsp/proximity.rs - Two-Stage Curve + Speech Damping
**Changes**:
- Import: `perceptual_curve, lerp`
- Function signature: `process(input, proximity, speech_confidence, clarity_amount)`
- Apply `x = perceptual_curve(proximity)`
- Two-stage boost:
  - 0-50%: `lerp(0.0, 6.0, x / 0.5)`
  - 50-100%: `lerp(6.0, 18.0, (x - 0.5) / 0.5)`
- Speech damping: `boost * (0.8 + 0.2 * speech_conf)`
- HF rolloff disabled when `clarity_amount > 0.6`

---

## 2. REMAINING WORK

### 🔨 src/dsp/deverber.rs - Aggressive Tail + Inter-Module Clamps

**Import additions**:
```rust
use crate::dsp::utils::aggressive_tail;
```

**Function signature change**:
```rust
// OLD
pub fn process_sample(&mut self, input: f32, amount: f32, sample_rate: f32) -> f32

// NEW
pub fn process_sample(
    &mut self,
    input: f32,
    amount: f32,
    speech_confidence: f32,
    clarity_amount: f32,
    proximity_amount: f32,
    sample_rate: f32
) -> f32
```

**Implementation changes** (around line 300-350 in process_sample):
```rust
// Apply aggressive tail curve to amount
let x = aggressive_tail(amount);

// Speech-aware floor clamping
let min_floor = if speech_confidence > 0.5 {
    0.08  // Higher floor during voiced speech
} else {
    0.04  // Lower floor during silence/noise
};

// Inter-module clamp 1: If clarity > 0.6, reduce de-verb strength by 25%
let clarity_reduction = if clarity_amount > 0.6 { 0.75 } else { 1.0 };

// Inter-module clamp 2: If proximity > 0.6, reduce HF decay aggression by 20%
let proximity_reduction = if proximity_amount > 0.6 { 0.8 } else { 1.0 };

// Apply reductions
let effective_amount = x * clarity_reduction;

// When computing floor (in the gain calculation section):
let floor = floor.max(min_floor);  // Apply speech-aware floor

// When computing decay rates for HF (late decay section):
let late_decay_high_adjusted = lerp(
    LATE_DECAY_HIGH,
    LATE_DECAY_LOW,
    1.0 - proximity_reduction
);
```

**Key locations to modify**:
1. Line ~200: Add aggressive_tail to amount
2. Line ~350: Apply speech-aware floor to computed gain floor
3. Line ~280: Adjust late_decay based on proximity
4. Line ~200: Apply clarity reduction multiplier to amount

---

### 🔨 src/dsp/dsp_denoiser.rs - Perceptual Curve + Harmonic Protection

**Import additions**:
```rust
use crate::dsp::utils::perceptual_curve;
```

**Changes in `process_sample` method** (around line 410-450):
```rust
// OLD
let amt = (cfg.amount * DENOISE_STRENGTH_MULT).clamp(0.0, MAX_DENOISE_AMOUNT);

// NEW - Apply perceptual curve
let x = perceptual_curve(cfg.amount);
let strength = x * 4.0;  // Map to 0-4.0 range
let amt = strength.clamp(0.0, MAX_DENOISE_AMOUNT);
```

**Changes in gain calculation** (around line 550-600, in the bin loop):
```rust
// After computing wiener_gain, before applying:

// Harmonic bin protection (≤ 450 Hz)
let bin_freq = (i as f32 * sample_rate) / (2.0 * nyq as f32);
if bin_freq <= 450.0 {
    // For voiced bins, max 70% attenuation
    let speech_conf = /* get from cfg or sidechain */;
    if speech_conf > 0.5 {
        // Never exceed 70% attenuation on voiced bins
        wiener_gain = wiener_gain.max(0.3);
    }
}
```

**Note**: May need to pass speech_confidence through DenoiseConfig struct.

---

### 🔨 src/dsp/dtln_denoiser.rs - Wet Mix Cap + Dry Floor Protection

**Import additions**:
```rust
use crate::dsp::utils::perceptual_curve;
```

**Changes in `remap_amount` function** (line 56-72):
```rust
fn remap_amount(amount: f32, speech_confidence: f32) -> AmountMap {
    let a = amount.clamp(0.0, MAX_DENOISE_AMOUNT);
    let normalized = a / MAX_DENOISE_AMOUNT;

    // Wet mix cap at 90%
    let wet = if normalized <= 0.9 {
        let t = smoothstep(0.0, 1.0, normalized / 0.9);
        (t * 0.85).min(0.9)  // Cap at 90%
    } else {
        let t = smoothstep(0.0, 1.0, (normalized - 0.9) / 0.1);
        (0.85 + t * 0.15).min(0.9)  // Still cap at 90%
    };

    // Residual dry anchor - higher floor during voiced speech
    let dry_floor = if speech_confidence > 0.5 {
        0.08
    } else {
        0.02
    };

    AmountMap { wet, floor: dry_floor }
}
```

**Changes in `process_frame` method** (line 278-286):
```rust
// When applying mask:
for i in 0..BINS {
    let nn_mask = mask[[0, i]].max(map.floor);
    let g = (1.0 - map.wet) + map.wet * nn_mask;

    // Voiced frame protection - blend with original
    let final_g = g * (1.0 - map.floor) + map.floor;  // Always preserve some dry

    self.spectrum[i] *= final_g;
    // ... rest of code
}
```

**Signature changes needed**:
```rust
// process_sample needs speech_confidence
fn process_sample(&mut self, input: f32, amount: f32, speech_confidence: f32) -> f32

// process_frame needs speech_confidence
fn process_frame(&mut self, amount: f32, speech_confidence: f32) -> TractResult<()>

// StereoDtlnDenoiser::process_sample needs speech_confidence
fn process_sample(&mut self, input_l: f32, input_r: f32, amount: f32, speech_confidence: f32) -> (f32, f32)
```

---

### 🔨 src/dsp/compressor.rs - MAJOR Anti-Pumping Rewrite

**Import additions**:
```rust
use crate::dsp::utils::lerp;
```

**Constants to add**:
```rust
// Adaptive release time constants
const GAIN_RELEASE_FAST_MS: f32 = 400.0;  // When GR > 6dB
const GAIN_RELEASE_SLOW_MS: f32 = 900.0;  // When GR <= 6dB
const GAIN_ATTACK_MS: f32 = 30.0;

// Envelope detector constants
const ENV_ATTACK_MS: f32 = 30.0;
const ENV_RELEASE_FAST_MS: f32 = 400.0;
const ENV_RELEASE_SLOW_MS: f32 = 900.0;

// Peak tamer separate envelope
const PEAK_TAMER_ATTACK_MS: f32 = 5.0;
const PEAK_TAMER_RELEASE_MS: f32 = 120.0;

// Makeup gain limits
const MAKEUP_MAX_NORMAL_DB: f32 = 4.0;
const MAKEUP_MAX_CONSTRAINED_DB: f32 = 2.5;
```

**Struct changes**:
```rust
pub struct LinkedCompressor {
    sample_rate: f32,

    // Metering
    gain_reduction_envelope_db: f32,
    peak_gain_reduction_db: f32,

    // Data-driven adaptation
    crest_factor_db: f32,
    rms_variance: f32,
    adaptation_coeff: f32,

    // Smoothed output gain
    out_gain_smooth: f32,
    reduction_smooth_db: f32,

    // NEW: Separate peak tamer state
    peak_tamer_reduction_db: f32,

    // NEW: Gain freeze during silence
    frozen_gain: f32,
}
```

**compute_gain method rewrite** (major changes):
```rust
pub fn compute_gain(
    &mut self,
    env_l: &VoiceEnvelope,
    env_r: &VoiceEnvelope,
    amount: f32,
    speech_confidence: f32,  // NEW parameter
    proximity_amount: f32,   // NEW parameter
    clarity_amount: f32,     // NEW parameter
) -> f32 {
    let amount = amount.clamp(0.0, 1.0);

    // Freeze gain during silence
    if speech_confidence < 0.2 {
        // Hold last frozen gain, don't compute new reduction
        return self.frozen_gain;
    }

    // ... rest of bypass check ...

    // 1. Hybrid detector (unchanged)
    let hybrid = (HYBRID_RMS_WEIGHT * rms_max + HYBRID_PEAK_WEIGHT * peak_max).max(DB_EPS);
    let hybrid_db = lin_to_db(hybrid);
    let peak_db = lin_to_db(peak_max.max(DB_EPS));

    // 2. Speech-confidence-weighted ratio
    let speech_factor = (1.0 - speech_confidence).powf(2.0);
    let ratio_mult = if self.crest_factor_db < CREST_ADAPTATION_THRESHOLD_DB {
        LOW_CREST_RATIO_MULT
    } else {
        1.0
    };

    let ratio1 = lerp(1.0, LEVELER_RATIO_HIGH, speech_factor) * ratio_mult;

    // 3. Stage 1: Leveler with speech-aware ratio
    let over1 = hybrid_db - LEVELER_TARGET_DB;
    let red1_db = Self::soft_knee(over1, ratio1, LEVELER_KNEE_DB);

    // 4. Stage 2: Peak tamer with SEPARATE envelope
    let over2 = peak_db - PEAK_TAMER_THRESHOLD_DB;
    let red2_db_target = Self::soft_knee(over2, PEAK_TAMER_RATIO, PEAK_TAMER_KNEE_DB);

    // Smooth peak tamer reduction separately
    let peak_att = self.coeff(PEAK_TAMER_ATTACK_MS);
    let peak_rel = self.coeff(PEAK_TAMER_RELEASE_MS);
    if red2_db_target > self.peak_tamer_reduction_db {
        self.peak_tamer_reduction_db = peak_att * self.peak_tamer_reduction_db
            + (1.0 - peak_att) * red2_db_target;
    } else {
        self.peak_tamer_reduction_db = peak_rel * self.peak_tamer_reduction_db
            + (1.0 - peak_rel) * red2_db_target;
    }

    let total_reduction_db = (red1_db + self.peak_tamer_reduction_db).max(0.0);
    let target_reduction_db = total_reduction_db * amount;

    // 5. Adaptive release based on GR amount
    let att = self.coeff(GAIN_ATTACK_MS);
    let rel = if self.reduction_smooth_db > 6.0 {
        self.coeff(GAIN_RELEASE_FAST_MS)  // 400ms when heavy GR
    } else {
        self.coeff(GAIN_RELEASE_SLOW_MS)  // 900ms when light GR
    };

    if target_reduction_db > self.reduction_smooth_db {
        self.reduction_smooth_db = att * self.reduction_smooth_db + (1.0 - att) * target_reduction_db;
    } else {
        self.reduction_smooth_db = rel * self.reduction_smooth_db + (1.0 - rel) * target_reduction_db;
    }

    let applied_reduction_db = self.reduction_smooth_db;

    // 6. Makeup gain limiter
    let max_makeup = if proximity_amount > 0.5 || clarity_amount > 0.5 {
        MAKEUP_MAX_CONSTRAINED_DB  // 2.5 dB
    } else {
        MAKEUP_MAX_NORMAL_DB  // 4.0 dB
    };

    let margin_db = hybrid_db - lin_to_db(noise_floor.max(DB_EPS));
    let makeup_db = if margin_db > MAKEUP_MARGIN_DB {
        (self.gain_reduction_envelope_db * MAKEUP_SCALE).min(max_makeup)
    } else {
        0.0
    };

    // ... rest of gain calculation and smoothing ...

    // Update frozen gain for next silence period
    self.frozen_gain = self.out_gain_smooth;

    self.out_gain_smooth
}
```

**reset method**:
```rust
pub fn reset(&mut self) {
    self.gain_reduction_envelope_db = 0.0;
    self.reduction_smooth_db = 0.0;
    self.peak_tamer_reduction_db = 0.0;  // NEW
    self.frozen_gain = 1.0;  // NEW
}
```

---

### 🔨 src/dsp/denoiser.rs - Orchestrator Updates

**Update process_sample to pass speech_confidence**:
```rust
pub fn process_sample(
    &mut self,
    input_l: f32,
    input_r: f32,
    cfg: &DspDenoiseConfig,
    speech_confidence: f32,  // NEW
) -> (f32, f32) {
    // ... switching logic unchanged ...

    if cfg.use_dtln && self.dtln_denoiser.is_some() {
        if let Some(d) = &mut self.dtln_denoiser {
            d.process_sample(input_l, input_r, cfg.amount, speech_confidence)
        } else {
            // Fallback - need to pass speech_conf through
            // For now, assume DSP mode doesn't need it in the trait
            self.dsp_denoiser.process_sample(input_l, input_r, cfg)
        }
    } else {
        self.dsp_denoiser.process_sample(input_l, input_r, cfg)
    }
}
```

**Note**: May need to extend DenoiseConfig to include speech_confidence.

---

### 🔨 src/lib.rs - Main Process Loop Integration

**Major changes around line 800-1100** (process loop):

#### A. Extract values for inter-module communication:
```rust
// After parameter extraction (around line 830):
let speech_conf = sidechain.speech_conf;
let clarity_amt = raw_clarity / MAX_GAIN;  // Normalized 0-1
let prox_amt_norm = raw_prox / MAX_GAIN;   // Normalized 0-1
```

#### B. Inter-module safety clamps (NEW section before processing):
```rust
// INTER-MODULE SAFETY CLAMPS
let (clarity_amt_safe, prox_amt_safe) = if clarity_amt > 0.6 && prox_amt_norm > 0.6 {
    // Reduce both by 20%
    (clarity_amt * 0.8, prox_amt_norm * 0.8)
} else {
    (clarity_amt, prox_amt_norm)
};

// For denoiser
let use_dtln = self.params.noise_mode.value() == presets::NoiseMode::Aggressive;
let dtln_floor_boost = if use_dtln && noise_amt > 0.7 && reverb_amt > 0.6 {
    0.10  // Raise DTLN floor to 10%
} else {
    0.0  // No boost
};

// For leveler-based clarity clamp
let leveler_gr = self.linked_compressor.get_gain_reduction_db();
let clarity_max_cut_override = if leveler_gr > 8.0 {
    Some(24.0)  // Clamp clarity to max 24dB cut
} else {
    None
};

// For speech confidence extreme tail disable
let disable_extreme_tails = speech_conf > 0.7;
```

#### C. Update denoiser calls (around line 1010-1015):
```rust
// OLD
let (s1_l, s1_r) = if bypass_restoration {
    (bias_l, bias_r)
} else {
    self.denoiser.process_sample(bias_l, bias_r, &denoise_cfg)
};

// NEW
let (s1_l, s1_r) = if bypass_restoration {
    (bias_l, bias_r)
} else {
    self.denoiser.process_sample(bias_l, bias_r, &denoise_cfg, speech_conf)
};
```

#### D. Update deverber calls (around line 1035-1055):
```rust
// OLD
let s3_l = if bypass_restoration {
    s2_l
} else {
    self.process_l.restoration_chain.deverber.process_sample(
        s2_l,
        total_deverb,
        self.sample_rate,
    )
};

// NEW
let s3_l = if bypass_restoration {
    s2_l
} else {
    self.process_l.restoration_chain.deverber.process_sample(
        s2_l,
        total_deverb,
        speech_conf,
        clarity_amt_safe,
        prox_amt_safe,
        self.sample_rate,
    )
};
// Same for s3_r
```

#### E. Update proximity calls (around line 1060-1075):
```rust
// OLD
let (s4_l, s4_r) = if bypass_shaping {
    (s3_l, s3_r)
} else {
    (
        self.process_l.shaping_chain.proximity.process(s3_l, prox_amt),
        self.process_r.shaping_chain.proximity.process(s3_r, prox_amt),
    )
};

// NEW
let (s4_l, s4_r) = if bypass_shaping {
    (s3_l, s3_r)
} else {
    (
        self.process_l.shaping_chain.proximity.process(s3_l, prox_amt_safe, speech_conf, clarity_amt_safe),
        self.process_r.shaping_chain.proximity.process(s3_r, prox_amt_safe, speech_conf, clarity_amt_safe),
    )
};
```

#### F. Update clarity calls (around line 1084-1097):
```rust
// OLD
let (s5_l, s5_r) = if bypass_shaping {
    (s4_l, s4_r)
} else {
    (
        self.process_l.shaping_chain.clarity.process(
            s4_l,
            clarity_amt,
            prox_amt,
            clarity_drive,
        ),
        self.process_r.shaping_chain.clarity.process(
            s4_r,
            clarity_amt,
            prox_amt,
            clarity_drive,
        ),
    )
};

// NEW
let (s5_l, s5_r) = if bypass_shaping {
    (s4_l, s4_r)
} else {
    (
        self.process_l.shaping_chain.clarity.process(
            s4_l,
            clarity_amt_safe,
            speech_conf,
            clarity_drive,
        ),
        self.process_r.shaping_chain.clarity.process(
            s4_r,
            clarity_amt_safe,
            speech_conf,
            clarity_drive,
        ),
    )
};
```

#### G. Update leveler call (around line 1135-1140):
```rust
// OLD
let comp_gain = self.linked_compressor.compute_gain(&env_l, &env_r, level_amt);

// NEW
let comp_gain = self.linked_compressor.compute_gain(
    &env_l,
    &env_r,
    level_amt,
    speech_conf,
    prox_amt_safe,
    clarity_amt_safe,
);
```

---

## 3. VALIDATION CHECKLIST

After implementation, test:

### ✅ 50% Test
- [ ] Speech remains full, stable, intelligible
- [ ] No pumping artifacts
- [ ] No spectral collapse
- [ ] Settings feel musical and usable

### ✅ 100% Torture Test
- [ ] Can sound extreme but not collapse into silence
- [ ] No noise smear
- [ ] Voiced speech still recognizable

### ✅ Toggle Invariance
- [ ] SIMPLE ↔ ADVANCED switching doesn't change sound
- [ ] Parameter sync working correctly

### ✅ Meter Sanity
- [ ] No oscillating GR meter
- [ ] No sustained >10dB GR unless speech_conf < 0.3
- [ ] Activity LEDs behave reasonably

---

## 4. TESTING PROCEDURE

```bash
# Build with debug features
cargo nih-plug bundle vxcleaner --release --features debug

# Test files
noisy: test_data/notclean.wav
clean: test_data/proclean.wav

# Run automated optimization
cd test_automation
python auto_tune.py \
    --noisy ../test_data/notclean.wav \
    --reference ../test_data/proclean.wav \
    --vst /Library/Audio/Plug-Ins/VST3/vxcleaner.vst3 \
    --trials 50
```

---

## 5. ROLLBACK PLAN

If stability issues arise:

1. Git branch: `git checkout -b dsp-stability-rollback`
2. Revert utils.rs curve additions
3. Revert all function signature changes
4. Restore linear parameter mappings

Original behavior preserved in git history at commit before this work.

---

## 6. KNOWN ISSUES / NOTES

- Speech confidence comes from `SpeechSidechain` - already computed
- Some modules may need `pub` visibility changes for cross-module access
- DTLN may need speech_conf passed through `DenoiseConfig` struct
- Compressor changes are most complex - test incrementally
- Inter-module clamps in lib.rs are order-dependent

---

**Total files modified**: 8
**Total lines changed**: ~800-1000
**Estimated implementation time**: 2-3 hours
**Testing time**: 1-2 hours

**Priority order**:
1. Leveler (fixes pumping immediately)
2. Main loop integration (makes other changes functional)
3. Denoiser updates (DTLN stability)
4. De-verb updates (completes the picture)
