# Silence Contract – Checklist Task

This task list is derived from the strict rebuild prompt for Voice Studio. Follow each step exactly to keep the diff targeted. Work only inside the specified files.

## 1. `speech_confidence.rs` – Silence Escape Hatch
- [ ] Add constants and state for a dedicated silence release path (threshold, faster release coefficient).
- [ ] When RMS drops below the silence threshold, force the hang timer to zero and apply the fast release so confidence decays to ≤0.1 within ~200 ms.
- [ ] Ensure speech confidence can reach near-zero in silence, with no forced floor above 0.05.

## 2. `spectral_guardrails.rs` – Frequency-aware protection
- [ ] Extend `process()` to accept the shared `speech_confidence` value.
- [ ] Zero or relax the HF correction when `speech_confidence < 0.3` while keeping low/mid behavior unchanged.
- [ ] Leave mid-band protection untouched (only HF behavior changes).

## 3. `speech_expander.rs` – Silence neutrality
- [ ] Skip expansion entirely when RMS is below the silence threshold and `speech_confidence < 0.2` so the module becomes effectively transparent.
- [ ] Keep release/attack behavior unchanged during speech.

## 4. `profile_analyzer.rs` – Silence relaxation bias fix
- [ ] Track sustained silence and, once confirmed, use a faster noise-floor release path so baselines relax faster.
- [ ] Do not reset all stats—just adjust the release coefficient during silence.

## 5. `dsp_denoiser.rs` – HF authority & release guardrail
- [ ] Allow HF bins (above ~5–6 kHz) to drop minimum gain to ~0.01–0.03 whenever `speech_confidence < 0.2`.
- [ ] When that low-confidence condition holds, also relax the temporal release limit for those bins (e.g., release_limit=1.0) so HF hiss keeps decaying.
- [ ] Keep other guards (hum removal, harmonic protection) untouched.

### Acceptance

- [ ] Build with `--features debug` and verify `/tmp/voice_studio.log` can report `[PUMP DETECT]` (existing work) while the new silence contract restrictions fire only in silence.
- [ ] Confirm noise/hiss drops aggressively during room tone while speech remains protected.
