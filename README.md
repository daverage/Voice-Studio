# Voice Studio (VxCleaner)

**Version:** 0.6.0 — deterministic vocal restoration for messy rooms.

## Overview
Voice Studio (VxCleaner) is a professional vocal restoration and enhancement suite built for podcasting, voice-over, dialogue, and broadcast deliveries. The plugin focuses on clarity, consistency, and intelligibility without introducing neural artifacts; every stage is a deterministic DSP process tuned for real speech.

## Architecture
The pipeline prioritizes safety, transparency, and low latency. Every stage runs on the audio thread with pre-allocated state and defensive guards.

### Audio Processing Pipeline
1. **SpeechHpf (Hygiene)** – audio routing begins with a conversational high-pass filter (about 90 Hz).
2. **Speech Analysis / Confidence** – the chain continuously tracks speech activity and spectral shape.
3. **EarlyReflection Suppressor** – early reflections and desk/room colorations are attenuated conservatively.
4. **Static Noise Learn & Removal** – deterministic subtraction of captured room tone and hum.
5. **Noise Reduction (Spectral Gating)** – spectral gating with magnitude-adaptive envelopes keeps speech intact.
6. **Plosive Softening** – automatic softening of thumps and plosive hits.
7. **Breath Management** – confidence-weighted breath reduction keeps inhales but tames exhale noise.
8. **Deverber / Shaping** – late reverb energy is peeled back and shaping components restore body/air.
9. **Proximity & Clarity Shaping** – separate low-end warmth and high-frequency articulation controls.
10. **Post-Noise Cleanup** – very light, confidence-gated attenuation to tuck residual noise after shaping.
11. **Dynamics Chain** – De-esser, Leveler (linked stereo compressor), and Limiter protect the downstream buss.
12. **Output Gain + Delivery Guardrails** – final level trimming with optional delivery presets (YouTube, Spotify, Broadcast).

## Modes
- **Simple Mode macros** (Clean, Enhance, Control) map a handful of intent-driven buttons to precise adjustments across the entire DSP stack, letting you jump into a mix without hunting sliders.
- **Advanced Mode sliders** unlock every stage (Clean & Repair on the left column, Shape & Polish on the right, dynamics in the footer). The UI highlights noise learn quality, breath control, shaping, and limiting with responsive meters.

The plugin also exposes a dedicated **Quality meter** beneath the noise controls to show how much steady noise is being tracked—keep it near mid-scale to balance suppression vs. artifacts.

## Controls
### Clean & Repair
* **Rumble** – HPF-based control for 20–120 Hz energy.
* **Hiss** – HF attenuation above ~8 kHz without dulling clarity.
* **Static Noise** – learn and clear constant room tone via the Re-learn/Clear buttons.
* **HF Bias** – toggles HF-focused cleanup in the post-noise pass.
* **Hidden FX** – toggles hidden tone stages (pink bias, recovery, post-cleanup, guardrails). On by default.
* **Low End** – toggles denoiser low-end protection (disable if it boosts bass at high reduction).
* **Noise Reduction** – adaptive spectral gating blends aggressively with smoothing.
* **De-Verb** – early reflection suppression.
* **Breath Control** – confidence-weighted breath softening between words.

### Shape & Polish
* **Proximity** – restores low-frequency warmth for close-mic or distant recordings.
* **Clarity** – high-mid sculpting that reduces mud and brings articulation forward.
* **De-Ess** – maps to a sibilance limiter that acts when conditions warrant.
* **Leveler** – linked stereo compressor for transparent loudness smoothing.
* **Gain** – output trim before the limiter, useful for delivery matching.

## Build & Release
### Prerequisites
- Rust 1.70+ toolchain with `cargo` and `cargo-nih-plug` installed (`cargo install cargo-nih-plug`).
- For Windows bundles: `xwin` (managed via `xwin --accept-license splat --output xwin`) plus LLVM tools (`clang-cl`, `lld-link`, `llvm-lib`).
- For Linux bundles: Docker + `cross` to provide a complete sysroot with `pkg-config`.
- The `tools/release.sh` script wraps the per-platform builds, bundling, and GitHub release creation (prompting for commit message).

### Common Commands
```bash
# Local builds
cargo build
cargo build --release

# Bundle for VST3/CLAP (production)
cargo nih-plug bundle vxcleaner --release

# Bundle with debug features (logging + live CSS reloading)
cargo nih-plug bundle vxcleaner --release --features debug

# Run the release pipeline (macOS + Windows + optional Linux via cross)
SKIP_LINUX=1 ./tools/release.sh # use when Linux containers are unavailable
./tools/release.sh             # full release (requires Docker + cross + xwin)
```

## Feature Flags
- `debug`: toggles centralized logging (`/tmp/voice_studio.log`) plus UI helpers:
  - Footer **Log** button opens the log file.
  - **Edit CSS** opens `src/ui.css` in your default editor and writes it to the bundle.
  - **Reload CSS** reloads the stylesheet at runtime while the plugin is open.

## Web & Help Resources
- **Marketing page**: `web/index.html` highlights macOS + Windows bundling, explains the deterministic workflow, and now surfaces both macro (simple) and slider (advanced) modes side-by-side with the mode artwork stored at `web/assets/icons/simple.png` and `web/assets/icons/advanced.png`.
- **In-plugin help page**: `web/help.html` mirrors the noise-removal workflow, macro intent, and slider documentation plus a mode illustration section that reuses the advanced/simple imagery.
- Keep both files synchronized whenever UI copy, macros, or version numbers change; the help footer now reports `VxCleaner v0.6.0` and the download CTA points to `https://github.com/daverage/Voice-Studio/releases/tag/v0.6.0`.

## Documentation housekeeping
- Core references live in `README.md`, the `docs/` folder (release/versioning/publishing specs), and the `docs/agents/` instructions (`CLAUDE.md`, `GEMINI.md`, `QWEN.md`, plus `AGENT_CONTRACT.md`).
- Historical writeups and plans (UI fix/refactor guidance, NEXT_STEPS, CSS styling notes, etc.) now sit under `docs/archive/` so the root contains nothing but working documentation and LLM guidance.

## Agent governance
- Follow the `AGENT_CONTRACT.md` protocol before touching repository state: memory recall, task queries (`tinyTasks.md`), and the finish-time checklist.
- The `AGENTS.md`, `CLAUDE.md`, `GEMINI.md`, and `QWEN.md` files describe the same architecture/build instructions/glossary; keep them synchronized when any release-related detail changes.
- Minor web assets (mode icons in `web/assets/icons/`) power the help and marketing pages, so keep those files in sync with any visual updates.

## License & Acknowledgments
- Licensed under MIT; see [LICENSE](LICENSE).
- Built with [nih-plug](https://github.com/robbert-vdh/nih-plug) and [vizia](https://github.com/vizia/vizia).
