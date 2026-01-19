# Voice Studio Architecture Invariants

This plugin follows hard real-time audio constraints.

## Audio Thread Rules
- No memory allocation in `process()` or any audio-thread code.
- No mutexes, locks, or blocking operations in the audio thread.
- DSP state must be pre-allocated and deterministic.

## Stage Boundaries
- Restoration stage: denoise, de-verb.
- Shaping/finishing stage: proximity, clarity, de-esser, leveler, limiter.

## Shared Utilities (`src/dsp/utils.rs`)

All common DSP utilities are centralized in `utils.rs` to avoid code duplication:

### Constants (safe for audio thread)
- `MAG_FLOOR` - Floor for magnitude calculations (avoids log(0))
- `DB_EPS` - Epsilon for dB conversions
- `BYPASS_AMOUNT_EPS` - Default bypass threshold

### Functions (safe for audio thread)
- `make_sqrt_hann_window(size)` - Generate sqrt-Hann window for WOLA
- `time_constant_coeff(ms, sr)` - Convert ms to smoothing coefficient
- `update_env_sq(env, in, atk, rel)` - Attack/release envelope follower
- `lin_to_db(x)` / `db_to_lin(db)` - dB conversions
- `lerp`, `smoothstep`, `bell` - Interpolation utilities
- `estimate_f0_autocorr(frame, scratch, sr)` - F0 estimation (requires scratch buffer)
