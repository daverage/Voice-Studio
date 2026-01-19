# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Voice Studio is a professional vocal restoration and enhancement VST3/CLAP audio plugin written in Rust using the `nih_plug` framework with a `nih_plug_vizia` GUI.

## Build Commands

```bash
# Build plugin (debug)
cargo build

# Build plugin (release)
cargo build --release

# Bundle VST3/CLAP plugins (requires cargo-nih-plug)
cargo nih-plug bundle voice_studio --release

# Bundle with ML features (larger binary, optional)
cargo nih-plug bundle voice_studio --release --features "ml gpu"

# Run tests
cargo test

# Format code
cargo fmt
```

**Note:** ringbuf is pinned to 0.2.8 due to API syntax requirements.

## Feature Flags

- `ml` - Enables ML-based denoising advisor (adds tract-core, tract-tflite dependencies)
- `gpu` - Enables GPU acceleration via Metal (macOS only, implies `ml`)

Default build has no ML features for a lighter binary.

## Architecture

### Audio Processing Pipeline

```
Input → Early Processing → Restoration → Shaping → Dynamics → Output
```

**Early Processing** (`src/dsp/early_reflection.rs`, `src/dsp/speech_expander.rs`):
- Short-lag reflection suppression (micro-deverb)
- Speech-aware downward expansion

**Restoration stage** (`src/dsp/denoiser.rs`, `src/dsp/deverber.rs`):
- Spectral denoiser with tone control (Wiener filter + optional ML advisor)
- Envelope-based reverb reduction
- Exposes "delta" (removed signal) for preview

**Shaping stage** (`src/dsp/proximity.rs`, `src/dsp/clarity.rs`):
- Proximity: low-end shaping for "close mic" effect
- Clarity: high-frequency enhancement

**Dynamics stage** (`src/dsp/de_esser.rs`, `src/dsp/compressor.rs`, `src/dsp/limiter.rs`):
- De-esser: sibilance reduction
- Leveler: linked stereo compression
- Limiter: output safety limiting

**Analysis (sidechain)** (`src/dsp/speech_confidence.rs`, `src/dsp/profile_analyzer.rs`):
- Speech vs noise/silence detection for automation
- Real-time audio profile computation for calibration

### Key Modules

- `src/lib.rs` - Plugin entry point, `process()` loop, parameter definitions, `TargetProfile`/`AudioProfile` structs
- `src/dsp/mod.rs` - DSP chain orchestration, module exports
- `src/macro_controller.rs` - Simple mode macro-to-parameter mapping
- `src/ui.rs` - Vizia-based GUI
- `src/meters.rs` - Thread-safe atomic metering

### Macro Controller

Intent-based "Simple mode" that maps 3 user-facing macros to underlying parameters:
- **Distance** → reverb_reduction, proximity
- **Clarity** → noise_reduction, de_esser, noise_tone, clarity_cap
- **Consistency** → leveler

Invariants:
- Macros write parameters only, never touch audio directly
- All macro effects resolve to advanced parameters
- Macro mode never changes DSP topology

## Critical Constraints

### Audio Thread Rules (MUST follow)

- **No memory allocation** in `process()` or any audio-thread code
- **No mutexes, locks, or blocking operations** in the audio thread
- DSP state must be pre-allocated in `initialize()` or constructors
- Use atomic floats (relaxed ordering) for meter data shared with UI

### Control Slew Limiting

Spectral parameters (denoise, clarity, de-esser) use slew limiting (`src/dsp/control_slew.rs`) to prevent audible artifacts from rapid parameter changes. This is essential for macro mode transitions.

### Delta Preview Contract

- Restoration modules must expose the removed signal (delta)
- `input ≈ output + delta` within float tolerance
- No latency or phase mismatch between output and delta paths

### ML Denoiser Contract (when `ml` feature enabled)

- `MlDenoiseEngine` serves as an **advisor** providing speech probability masks
- Falls back gracefully to CPU if GPU init fails
- Models are embedded at compile time (`src/assets/models/dtln/`)
- Audio-thread safe: no allocations in `process_frame()`

## State Management Patterns

- Parameters: `Arc<VoiceParams>` via nih_plug's parameter system
- Metering: `Arc<Meters>` with atomic floats (no locks)
- UI: Declarative Vizia components observing parameters and meters
- Calibration: `AudioProfile` computed once per block, compared against `TargetProfile`
