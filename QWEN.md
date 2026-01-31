# QWEN.md

This file provides guidance to Qwen (qwen.ai) when working with code in this repository.

## Current Version
- `vxcleaner` package version: `0.6.0` (matches `Cargo.toml` and `src/lib.rs::VERSION`).

## Project Overview
Voice Studio (VxCleaner) is a deterministic vocal restoration and enhancement VST3/CLAP plugin written in Rust with `nih_plug` + `nih_plug_vizia`. There is no neural inference path; every stage is audio-thread safe, pre-allocated, and designed for real speech in real rooms.

## Audio Processing Pipeline
```
Input → SpeechHpf → Analysis → EarlyReflection → Static Noise Learn → Noise Reduction → PlosiveSoftener → BreathReducer → Deverber → Proximity/Clarity → Dynamics (De-esser → Leveler → Limiter) → Output
```
- Noise cleanup is driven by a learned static profile, adaptive spectral gating, and a Quality meter below the noise controls.
- Shaping and dynamics stages (proximity, clarity, de-esser, leveler, limiter) run deterministically in order with smoothing and epsilon guards.

## Modes & UI
- **Simple macros:** Clean, Enhance, Control map to macro adjustments on the Clean & Repair, Shape & Polish, and dynamics stages. These are the only controls in Simple mode.
- **Advanced sliders:** Clean & Repair column (rumble, hiss, static noise, noise reduction, de-verb, breath control) and Shape & Polish column (proximity, clarity, de-ess, leveler) expose every DSP stage.
- The UI uses `src/ui` submodules (`layout`, `advanced`, `simple`, `components`, `meters`, `state`) and `src/meters.rs` for shared meter state.

## Build Commands
```bash
cargo build
cargo build --release
cargo nih-plug bundle vxcleaner --release
cargo nih-plug bundle vxcleaner --release --features debug
cargo test
cargo fmt
```

## Release Command
```bash
SKIP_LINUX=1 ./tools/release.sh   # macOS + Windows only (use when Docker/cross fails)
./tools/release.sh               # full release (requires Docker/cross and xwin)
```
- `tools/release.sh` prompts for a commit message, updates `Cargo.toml`/`src/lib.rs`, bundles VST3/CLAP, zips per OS, and creates a GitHub release (`v0.6.0`).
- Windows bundles rely on `xwin`, `clang-cl`, `lld-link`, `llvm-lib`. Linux bundles expect a Dockerized `cross` session with `pkg-config` pointing into the sysroot.

## Feature Flags
- `debug`: enables `/tmp/voice_studio.log` logging, the footer Log button, Edit/Reload CSS helpers, and the live stylesheet workflow.

## Web & Help
- The marketing page (`web/index.html`) now highlights the Simple vs Advanced modes side-by-side and links straight to `https://github.com/daverage/Voice-Studio/releases/tag/v0.6.0`.
- The help page (`web/help.html`) documents the macros, sliders, noise removal flow, and reuses `web/assets/icons/simple.png` + `web/assets/icons/advanced.png`.
- Keep the `web/assets/icons` directory in sync with any UI mode artwork updates.

## Documentation Notes
- Core documentation lives in `README.md`, `docs/`, `docs/agents/`, and this file. Historical planning docs (UI fix/refactor reports, NEXT_STEPS, CSS styling notes, etc.) now reside under `docs/archive/`.
- Always keep `CLAUDE.md`, `GEMINI.md`, and `QWEN.md` synchronized with any changes to architecture, build steps, or release instructions.

## Critical Constraints
### Audio thread rules
- No heap allocation, mutexes, blocking, or panics in `process()`.
- Use atomic floats (relaxed) for meter/UI sharing, pre-allocate DSP state in `initialize()`.

### Safety contract
- Every division uses a `1e-12` guard.
- No `unwrap()`/`expect()` on the audio thread path.
- Wrap all FFI exports with `catch_unwind`.

## Agent instructions
- Start every repository change by invoking `memory_query`/`memory_recent`, reading `tinyTasks.md`, and following the custom plan/protocol in `AGENT_CONTRACT.md`.
- Report progress by updating `tinyTasks.md` (top-level tasks, substasks). Track documentation cleanup steps deliberately.
- After release-related edits, bump `Cargo.toml`/`src/lib.rs` version, keep `Cargo.lock` `vxcleaner` entry in sync, and update any footer text that shows the version number.
