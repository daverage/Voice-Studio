# Architectural Notes

## 1. Analysis Path vs. Audio Path

The architecture strictly separates signal analysis from signal processing to ensure stability and predictability.

- **Audio Path (Real-time):**
    - The `process()` loop handles audio buffers directly.
    - Must never block or allocate.
    - All DSP state for the audio path is pre-allocated.
    - Modifications are driven by parameters or smoothed control signals.

- **Analysis Path (Sidechain/Profile):**
    - `ProfileAnalyzer` runs on pre-DSP input samples.
    - `SpeechConfidenceEstimator` runs in parallel to determine signal intent.
    - Analysis results (AudioProfile, Conditions) are computed **once per buffer** and are read-only for the DSP modules.
    - No mid-chain re-measurement feeds control logic to prevent feedback loops and oscillation.

## 2. Memory Management

- **No Allocations in `process()`:**
    - All vectors, ring buffers, and FFT scratch spaces are allocated in `new()` or `initialize()`.
    - Rust's ownership model is used to ensure thread safety without locks in the hot path where possible.
    - Where shared state is necessary (e.g., UI metering), atomic primitives with relaxed ordering are preferred.

## 3. Macro System Behavior

- **Macros are Orchestrators:**
    - Macros (Distance, Clarity, Consistency) do not process audio themselves.
    - They map high-level intent to low-level DSP parameters.
    - Macros utilize data-driven calibration (`TargetProfile`) to determine how much processing is actually needed.

- **Audio Buffer Isolation:**
    - `MacroController` calculates target parameter values but never touches the audio buffer.
    - This ensures that the macro logic can be tested and verified independently of the audio stream.

- **Write Locking:**
    - When Macro Mode is active, advanced parameters are effectively "owned" by the macro system.
    - Any manual adjustment of an advanced parameter immediately disables Macro Mode to prevent fighting control signals.
