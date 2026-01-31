# Tasks - Split Hiss / Rumble

- [x] **Refactor `src/dsp/hiss_rumble.rs`**
    - [x] Rename/modify `HissRumble` struct to support independent `rumble_hpf` and `hiss_shelf`.
    - [x] Implement `process(input_l, input_r, rumble_amt, hiss_amt, sidechain)`.
    - [x] `Rumble`: HPF 20 Hz -> 120 Hz.
    - [x] `Hiss`: HF Shelf 8 kHz, 0 -> -24 dB, gated by speech confidence.
    - [x] Update debug getters.

- [x] **Update `src/lib.rs` (Parameters & Integration)**
    - [x] Remove `noise_tone`.
    - [x] Add `rumble_amount` (0-1).
    - [x] Add `hiss_amount` (0-1).
    - [x] In `process_internal`, update call to `hiss_rumble.process` with new params.
    - [x] Update `initialize` and `reset` if needed.

- [x] **Update `src/ui.rs`**
    - [x] Remove "Tone" slider.
    - [x] Add "Rumble" slider (Clean & Repair column).
    - [x] Add "Hiss" slider (Clean & Repair column).
    - [x] Ensure layout remains balanced.

- [x] **Update Documentation**
    - [x] Update `README.md` with new "Hiss" and "Rumble" descriptions and frequency ranges.
    - [x] Update `src/help.html` with new controls.

- [x] **Verify**
    - [x] Check `cargo check`.
    - [x] Review architecture compliance (frequency ownership).