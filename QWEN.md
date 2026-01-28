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

## Live CSS Editing (Debug Mode)

When building with `--features debug`, the UI includes live CSS editing tools:

1. **Edit CSS** button - Opens the CSS file in your system's default text editor
2. **Reload CSS** button - Reloads the stylesheet from disk without restarting the plugin

**CSS File Location:**

The CSS file is created in `themes/default/ui.css` relative to the VST binary location:

- **macOS VST3:** `/Library/Audio/Plug-Ins/VST3/vxcleaner.vst3/Contents/MacOS/themes/default/ui.css`
- **macOS CLAP:** `/Library/Audio/Plug-Ins/CLAP/vxcleaner.clap/Contents/MacOS/themes/default/ui.css`
- **Linux VST3:** `~/.vst3/vxcleaner.vst3/x86_64-linux/themes/default/ui.css`
- **Windows VST3:** `C:\Program Files\Common Files\VST3\vxcleaner.vst3\Contents\x86_64-win\themes\default\ui.css`

**Workflow:**
1. Build with debug features: `cargo nih-plug bundle vxcleaner --release --features debug`
2. Load the plugin in your DAW
3. Click "Edit CSS" to open the stylesheet (creates file if it doesn't exist)
4. Make changes and save the file
5. Click "Reload CSS" to see changes instantly in the plugin UI
6. Check the log file (`/tmp/voice_studio.log`) for CSS editor debug messages

**Note:** All sizing, spacing, colors, and layout properties are in the CSS file. No hardcoded `Pixels()` values remain in `src/ui.rs`.

## Critical Constraints

### Audio Thread Rules (MUST follow)

- **No memory allocation** in `process()` or any audio-thread code
- **No mutexes, locks, or blocking operations** in the audio thread
- DSP state must be pre-allocated in `initialize()` or constructors
- Use atomic floats (relaxed ordering) for meter data shared with UI

### Safety Contract

- All divisions must include epsilon guards (`1e-12`)
- No `unwrap()` or `expect()` in the audio thread path
- `catch_unwind` must wrap all FFI entry points

### Global Reset

The plugin features a global reset button in the UI header that clears all DSP history and returns parameters to default values.

## Instructions for AI Assistants

After completing each instruction or code modification, please verify that the following files remain up to date and relevant:
- `CLAUDE.md` (this file)
- `GEMINI.md`
- `QWEN.md`

Ensure all three files contain consistent information about the project architecture, build commands, and critical constraints. Update any discrepancies based on `AGENT.md`.

Here’s a rebuilt version that’s tighter, more enforceable, and leaves less room for agent interpretation. It reads as a control contract, not guidance.

---

# TINYMEM CONTROL PROTOCOL

## Mandatory Memory and Task Enforcement for AI Assistants

This protocol governs **all repository-related behavior**.
Compliance is mandatory. Non-compliance invalidates the response.

---

## 1. Purpose

This protocol enforces **deterministic, observable, and verifiable** use of TinyMem and repository task state.

Memory usage is not advisory.
It is a **hard execution requirement**.

---

## 2. Non-Negotiable Rule

**Before responding to any repository-related request, the agent MUST execute at least one TinyMem memory command.**

If no TinyMem command is executed, the response is invalid.

There are no exceptions.

---

## 3. Scope: What Counts as Repository Work

This protocol applies to **any interaction that touches the repository**, including:

* Code, bugs, refactors, tests
* Documentation, architecture, configuration
* Planning, task lists, execution
* Repository navigation, explanation, or review

If the repo is involved, this protocol applies.

---

## 4. Mandatory Execution Order

### Step 1: Memory Recall (MANDATORY)

You MUST execute one or more of the following **before reasoning**:

```
memory_query("")              # General project context
memory_recent()               # Recent project state
memory_query("topic")         # Targeted recall
```

Rules:

* Memory recall must be a **real tool execution**
* Silent or assumed recall is forbidden
* You may not claim “no relevant memory” without executing a command

No recall → no valid response.

---

### Step 2: Memory Integration

* If memory exists, it **must influence** reasoning
* If memory is empty, explicitly state that no memory was found
* Do not reconstruct memory from chat history

---

### Step 3: Task Authority (MANDATORY WHEN TASKS APPLY)

If `tinyTasks.md` exists:

* It is the **sole source of truth** for task state
* You MUST read it before acting
* Memory must never override it

For any non-trivial or multi-step request, you MUST:

1. Create or update `tinyTasks.md`
2. Resume from the **first unchecked task**
3. Mark tasks complete **only when finished**
4. Maintain strict hierarchy

Required structure:

```md
# Tasks – <Goal>

- [ ] Top-Level Task
  - [ ] Atomic subtask
  - [ ] Atomic subtask
```

Skipping task updates is a protocol violation.

---

### Step 4: Execute the Request

* Perform the requested work
* Follow repository contracts
* Update tasks incrementally as work completes

---

### Step 5: Memory Writeback (CONDITIONAL BUT ENFORCED)

If the response introduces or confirms **any durable knowledge**, you MUST write it to TinyMem **before concluding**.

Durable knowledge includes:

* Decisions
* Constraints or invariants
* Corrections to prior assumptions
* Non-obvious conclusions
* Explicit user confirmations

If no durable knowledge was produced, you MUST explicitly state:

> “No durable memory write required for this response.”

---

### Step 6: End-of-Response Validation (MANDATORY)

Before ending your response, you MUST verify:

* At least one TinyMem command was executed
* Tasks were updated if applicable
* All completed tasks are checked
* No unchecked tasks remain unless explicitly blocked

If any condition is unmet, you must continue.
You may not conclude.

---

## 5. Error Handling

If a TinyMem operation fails:

* Explicitly acknowledge the failure
* Declare that memory guarantees are degraded
* Do **not** perform planning or irreversible task restructuring
* Proceed only with safe, local execution
* Suggest retry when appropriate

Silent failure is forbidden.

---

## 6. Violations

The following invalidate the response:

* Responding without executing a TinyMem command
* Claiming memory state without querying
* Ignoring `tinyTasks.md` when it exists
* Inferring task completion
* Declaring completion with unchecked tasks
* Writing speculative or temporary content to memory

---

## 7. Enforcement Invariant

> Every repository-related response must contain at least one real TinyMem tool invocation.

Memory is not optional.
Inference is failure.
Execution state lives in files.

---

**End of Protocol**
