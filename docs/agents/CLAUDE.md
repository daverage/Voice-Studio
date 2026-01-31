# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Voice Studio (VxCleaner) is a professional vocal restoration and enhancement VST3/CLAP audio plugin written in Rust using the `nih_plug` framework with a `nih_plug_vizia` GUI.

## Build Commands

```bash
# Build plugin (debug)
cargo build

# Build plugin (release)
cargo build --release

# Bundle VST3/CLAP plugins (Production - no logging)
cargo nih-plug bundle vxcleaner --release

# Bundle with Debug features (enables logging and UI Log button)
cargo nih-plug bundle vxcleaner --release --features debug

# Run tests
cargo test

# Format code
cargo fmt
```

**Note:** ringbuf is pinned to 0.2.8 due to API syntax requirements.

## Release Command (Local gh)

```bash
# Build + package macOS/Windows/Linux and create GitHub release (prompts for commit message)
tools/release.sh
```

Prereqs: Docker + `cross` for Linux builds, `xwin` + `lld-link` for Windows builds.

## Feature Flags

- `debug` - Enables development features:
  - Centralized logging to `/tmp/voice_studio.log`
  - "Log" button in UI footer to open the log file
  - "Edit CSS" button to open `src/ui.css` in your system's default text editor
  - "Reload CSS" button to reload styles from disk in real-time

**DSP Note**: All denoising is handled by the deterministic DSP pipeline; there is no neural inference path in this repository.

## Architecture

### Audio Processing Pipeline

```
Input → SpeechHpf → Analysis → EarlyReflection → Denoiser → PlosiveSoftener → BreathReducer → Deverber → Shaping → Dynamics → Output
```

- Spectral denoiser with adaptive magnitude gating (pure DSP)
- Automatic plosive/thump protection
- Confidence-weighted breath softening
- Late reverb reduction

**Shaping stage** (`src/dsp/proximity.rs`, `src/dsp/clarity.rs`):
- Proximity: low-end shaping
- Clarity: high-frequency enhancement

**Dynamics stage** (`src/dsp/de_esser.rs`, `src/dsp/compressor.rs`, `src/dsp/limiter.rs`):
- De-esser: sibilance reduction
- Leveler: linked stereo compression
- Limiter: output safety limiting

## UI Module Structure (`src/ui/`)

The UI is modularized to improve maintainability:
- `mod.rs`: Main entry point and re-exports.
- `layout.rs`: Top-level structure (header, body, footer).
- `state.rs`: Data model, events, and synchronization logic.
- `components.rs`: Reusable UI builders (sliders, dials, buttons).
- `advanced.rs`: Advanced mode panels and tabs.
- `simple.rs`: Simple mode macro controls.
- `meters.rs`: Custom Vizia widgets for metering.

**Note:** All metering data storage is unified in `src/meters.rs`. UI widgets in `src/ui/meters.rs` reference this shared state.

## Live CSS Editing (Debug Mode)

When building with `--features debug`, the UI includes live CSS editing tools. Styles are loaded from `src/ui.css`.

**Vizia CSS Compatibility Notes:**
- Use `child-left`, `child-right`, `child-top`, `child-bottom` instead of `padding`.
- Use `child-space: 1s` for centering instead of `text-align: center`.
- Properties like `cursor`, `transform`, and per-side borders are NOT supported by the Vizia engine used here.
- Use unitless values for `font-size` (e.g., `font-size: 14;`).

## Critical Constraints

### Audio Thread Rules (MUST follow)

- **No memory allocation** in `process()` or any audio-thread code.
- **No mutexes, locks, or blocking operations** in the audio thread.
- Use atomic floats (relaxed ordering) for meter data shared with UI.

### Safety Contract

- All divisions must include epsilon guards (`1e-12`).
- No `unwrap()` or `expect()` in the audio thread path.
- `catch_unwind` must wrap all FFI entry points.

## Instructions for AI Assistants

- Always bump the version in `Cargo.toml` and `src/lib.rs` (VERSION constant) with any release.
- Ensure `CLAUDE.md`, `GEMINI.md`, and `QWEN.md` remain consistent.
- **TINYMEM AGENT CONTRACT**: You MUST follow the protocol in `AGENT_CONTRACT.md` for all repository operations.