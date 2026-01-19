# Voice Studio – Audit Task List

## P0 – Critical

- [x] Verify decay and clamp of speech confidence noise floor tracking
  Files: dsp/speech_confidence.rs
  **Why:** Prevents long-session saturation in noise floor tracking
  **Sound risk:** None if properly implemented, Low if adjusted

- [x] Add bounds checking to harmonic protection in deverber
  **Files:** dsp/deverber.rs, apply_harmonic_protection method
  **Why:** Prevents potential out-of-bounds access during harmonic calculations
  **Sound risk:** None

## P1 – Important

- [x] Document dynamics ownership boundary explicitly
  **Files:** dsp/compressor.rs, dsp/deverber.rs
  **Why:** Prevents future control fighting between modules
  **Sound risk:** None

- [x] Review ML feature conditional compilation paths
  **Files:** dsp/denoiser.rs
  **Why:** Ensure consistent behavior between ML-enabled and ML-disabled builds
  **Sound risk:** None

- [x] Add buffer size assertions in DSP modules
  **Files:** Multiple DSP modules
  **Why:** Catch unexpected frame size mismatches early
  **Sound risk:** None

## P2 – Clarity, Documentation, or Future-proofing

- [x] Document derivation of key constants in macro controller
  **Files:** macro_controller.rs
  **Why:** Enable future tuning with understanding of original rationale
  **Sound risk:** None

- [x] Standardize naming conventions across DSP modules
  **Files:** Various DSP modules
  **Why:** Reduce cognitive load for future maintainers
  **Sound risk:** None

- [x] Add documentation for edge cases in DSP modules
  **Files:** Various DSP modules
  **Why:** Clarify behavior when inputs are at extremes
  **Sound risk:** None

- [x] Identify commonly used constants that should be documented or shared
  **Files:** Various DSP modules
  **Why:** Reduce duplication and improve maintainability
  **Sound risk:** None
