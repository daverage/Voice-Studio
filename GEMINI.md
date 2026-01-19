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
cargo nih-plug bundle voice_studio --release --features "ml gpu"

# Run tests
cargo test

# Format code
cargo fmt
```

**Note:** ringbuf is pinned to 0.2.8 due to API syntax requirements.

## Architecture

### Audio Processing Pipeline

```
Input → Restoration → Shaping → Dynamics → Output
```

**Restoration stage** (`src/dsp/denoiser.rs`, `src/dsp/deverber.rs`):
- Spectral denoiser with tone control
- Envelope-based reverb reduction
- Exposes "delta" (removed signal) for preview

**Shaping stage** (`src/dsp/proximity.rs`, `src/dsp/clarity.rs`):
- Proximity: low-end shaping for "close mic" effect
- Clarity: high-frequency enhancement

**Dynamics stage** (`src/dsp/de_esser.rs`, `src/dsp/compressor.rs`, `src/dsp/limiter.rs`):
- De-esser: sibilance reduction
- Leveler: linked stereo compression
- Limiter: output safety limiting

### Key Modules

- `src/lib.rs` - Plugin entry point, `process()` loop, parameter definitions
- `src/dsp/mod.rs` - DSP chain orchestration, `ChannelProcessor`
- `src/ui.rs` - Vizia-based GUI
- `src/auto_settings.rs` - Audio analysis for parameter suggestions
- `src/meters.rs` - Thread-safe atomic metering

## Critical Constraints

### Audio Thread Rules (MUST follow)

- **No memory allocation** in `process()` or any audio-thread code
- **No mutexes, locks, or blocking operations** in the audio thread
- DSP state must be pre-allocated in `initialize()` or constructors
- Use atomic floats (relaxed ordering) for meter data shared with UI

### Auto-Suggest Contract

Auto-Suggest may only influence:
- Noise reduction strength
- De-verb strength
- Leveler target bounds

Auto-Suggest must **never** modify:
- Proximity
- Clarity
- De-esser tone
- Any stylistic shaping parameters

All suggestions must flow through a single application function.

### Delta Preview Contract

- Restoration modules must expose the removed signal (delta)
- `input ≈ output + delta` within float tolerance
- No latency or phase mismatch between output and delta paths

## State Management Patterns

- Parameters: `Arc<VoiceParams>` via nih_plug's parameter system
- Metering: `Arc<Meters>` with atomic floats (no locks)
- UI: Declarative Vizia components observing parameters and meters
