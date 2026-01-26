# What To Do With Optimization Results

## Current Situation

We have a working Bayesian optimization system that can find optimal VST parameters for cleaning noisy audio. **But what's the point?**

---

## Use Case 1: **Validation/Testing** (What it's doing now)

### Purpose
Verify the DSP changes we just implemented actually improve audio quality.

### What it proves
- ✅ VST processes audio (not broken)
- ✅ Parameters affect output (Easy Mode bug found!)
- ✅ Can achieve 96.7% intelligibility (STOI=0.967)
- ✅ Can improve SI-SDR by 24.7 dB over unprocessed
- ✅ Perceptual curves we implemented are working

### Value
**Limited**. This is a one-time test to confirm things work.

### Verdict
**Good for QA, not worth maintaining long-term.**

---

## Use Case 2: **Factory Presets** (Most practical)

### Idea
Use optimized parameters to create built-in presets for common scenarios.

### Implementation
```rust
// In src/presets.rs
pub struct FactoryPreset {
    pub name: &'static str,
    pub noise_reduction: f32,
    pub reverb_reduction: f32,
    pub proximity: f32,
    pub clarity: f32,
    pub de_esser: f32,
    pub leveler: f32,
    pub breath_control: f32,
}

pub const PODCAST_PRESET: FactoryPreset = FactoryPreset {
    name: "Podcast (Noisy Room)",
    noise_reduction: 0.35,   // From optimization results
    reverb_reduction: 0.60,
    proximity: 0.05,
    clarity: 0.15,
    de_esser: 0.0,
    leveler: 0.70,
    breath_control: 0.30,
};

pub const PRESETS: &[FactoryPreset] = &[
    PODCAST_PRESET,
    VOICEOVER_PRESET,
    BROADCAST_PRESET,
    // etc.
];
```

### Workflow
1. Collect different audio types (podcast, voiceover, interview, outdoor, etc.)
2. Run optimization on each type
3. Save best parameters as named presets
4. Ship in VST

### UI Addition
```
┌─────────────────────────────┐
│ Presets: [Podcast v]        │  ← Dropdown
│   - None (Manual)            │
│   - Podcast (Noisy Room)     │
│   - Voiceover (Studio)       │
│   - Interview (Outdoor)      │
│   - Broadcast (Professional) │
└─────────────────────────────┘
```

### Value
**HIGH**. Users love presets - gives them a starting point instead of blank sliders.

### Effort
**LOW**. Just need to:
1. Collect 5-10 representative audio samples
2. Run optimization overnight
3. Hard-code best parameters
4. Add preset dropdown to UI

### Verdict
**✅ RECOMMENDED - Do this**

---

## Use Case 3: **Auto-Tune Button** (Sounds cool, probably not worth it)

### Idea
Add an "Auto" button to the VST that analyzes the current audio and sets optimal parameters automatically.

### How it would work
```
User clicks "Auto" button
  ↓
VST analyzes current audio track (5-10 seconds)
  ↓
Runs quick optimization (10-20 trials, ~30 seconds)
  ↓
Sets sliders to optimal values
  ↓
User can tweak from there
```

### Problems

**1. Speed**
- 100 trials = ~3 minutes (too slow for real-time)
- 10 trials = ~20 seconds (still feels slow)
- Users expect instant results (<2 seconds)

**2. Reference Audio**
- Optimization compares to "clean reference"
- But users don't have clean reference for their audio!
- Would need to guess what "clean" means (subjective)

**3. Complexity**
- Needs to bundle Python/Optuna or reimplement in Rust
- Adds 50+ MB to plugin size
- Increases support burden significantly

**4. User Expectations**
- Users expect magic "make it perfect" button
- Reality: auto-tune can make it worse if input is unusual
- Creates support tickets: "Auto button made my audio sound weird!"

### Alternative: **ML Model Prediction**

Instead of running optimization in real-time, train a model offline:

```python
# Train once on 1000s of samples
model = train_model(audio_features → optimal_parameters)

# Deploy tiny model in VST (fast inference)
optimal_params = model.predict(audio_features)  # <100ms
```

This could work but requires:
- Large training dataset (100s of hours of audio)
- ML expertise (model architecture, training, validation)
- Rust ML inference (tract/onnx)
- Still risk of poor results on unusual audio

### Value
**MEDIUM**. Cool feature but high risk/effort.

### Effort
**VERY HIGH**. Months of work.

### Verdict
**❌ NOT RECOMMENDED for v1.0** - Maybe v2.0 if VST is successful

---

## Use Case 4: **Regression Testing** (CI/CD validation)

### Idea
Use as automated test in CI/CD pipeline.

### Implementation
```yaml
# .github/workflows/test.yml
- name: Audio Quality Regression Test
  run: |
    cargo nih-plug bundle vxcleaner --release
    python3 test_automation/regression_test.py \
      --vst target/bundled/vxcleaner.vst3 \
      --test-suite test_data/regression/

# Fails if:
# - Score drops below 0.55 (regression)
# - Processing time > 2x real-time
# - Crashes on any test file
```

### Test Suite
```
test_data/regression/
  ├── podcast_noisy.wav + reference.wav
  ├── voiceover_hum.wav + reference.wav
  ├── interview_outdoor.wav + reference.wav
  └── expected_scores.json  # Known good scores
```

### Value
**MEDIUM-HIGH**. Catches regressions before release.

### Effort
**MEDIUM**. Need to maintain test suite.

### Verdict
**✅ USEFUL** - Set up before v1.0 release

---

## Use Case 5: **Marketing/Demos** (Show results work)

### Idea
Use results to prove VST works scientifically.

### Marketing Copy
```
"VxCleaner achieves 96.7% speech intelligibility (STOI) and
improves audio quality by 24.7 dB (SI-SDR) compared to raw recordings.

Validated using industry-standard PESQ and STOI metrics
against professionally cleaned reference audio."
```

### Demo Video
Show before/after with waveforms and metrics:
```
Before:  SI-SDR = -26.6 dB, STOI = 44.7%
After:   SI-SDR = -1.9 dB,  STOI = 96.7%
         ↑ 24.7 dB improvement!
```

### Value
**HIGH** for credibility. Numbers sell software.

### Effort
**LOW**. Already have the results.

### Verdict
**✅ DO THIS** - Use in website/demos

---

## Use Case 6: **Competitor Comparison** (Benchmarking)

### Idea
Compare your VST against competitors using same test suite.

### Method
```python
# Test against iZotope RX, Waves NS1, etc.
results = compare_plugins([
    "VxCleaner",
    "iZotope RX 10 Voice De-noise",
    "Waves NS1",
    "Accusonus ERA-N",
], test_suite="test_data/benchmark/")

# Output:
#                      SI-SDR  PESQ  STOI  Speed
# VxCleaner             -1.9   2.83  96.7%  1.2x
# iZotope RX 10         -0.5   3.21  97.1%  8.5x  ← Better quality, much slower
# Waves NS1             -3.2   2.65  94.2%  0.9x  ← Worse quality, faster
```

### Value
**HIGH** - Know where you stand vs. competitors.

### Verdict
**✅ USEFUL** - Do before v1.0 release

---

## Recommended Action Plan

### Phase 1: **Immediate** (This week)
1. ✅ Validate DSP works (DONE - got 0.59 score, 96.7% STOI)
2. ✅ Fix Easy Mode bug (DONE)
3. Create 3-5 factory presets:
   - Podcast (noisy room)
   - Voiceover (studio)
   - Interview (outdoor)
   - Broadcast (clean)

### Phase 2: **Before v1.0 Release** (Next 2 weeks)
4. Set up regression testing in CI/CD
5. Benchmark against competitors
6. Create marketing materials with metrics

### Phase 3: **Maybe v2.0** (Future)
7. Consider ML-based auto-tune (if users request it)
8. Consider real-time parameter suggestions

---

## Bottom Line: What Should You Do?

### Minimum Viable Use
**Just use it for validation** - Confirms your DSP changes work. Then archive it.

### Recommended Use
**Create factory presets** - Low effort, high user value. Do this.

### Optional Use
**Regression testing** - Good practice for quality assurance.

### Skip For Now
**Auto-tune button** - Too complex, too risky for v1.0.

---

## TL;DR Decision Tree

```
Do your DSP changes work?
├─ YES → Create 3-5 factory presets from optimized params
│        └─ Add preset dropdown to UI
│        └─ Ship v1.0
│
└─ NO → Fix bugs, re-run optimization, repeat
```

**The optimization tool is primarily a validation/testing tool.**

**But the results are valuable as factory preset starting points.**

Don't overthink it - users just want presets that "sound good" out of the box.
