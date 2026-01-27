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

**ML Note**: Neural denoising (DTLN) is mandatory and always compiled in. It uses optimized CPU inference. GPU support has been removed for stability.

## Architecture

### Audio Processing Pipeline

```
Input → SpeechHpf → Analysis → EarlyReflection → Denoiser → PlosiveSoftener → BreathReducer → Deverber → Shaping → Dynamics → Output
```

**Restoration stage** (`src/dsp/denoiser.rs`, `src/dsp/deverber.rs`, `src/dsp/plosive_softener.rs`, `src/dsp/breath_reducer.rs`):
- Hybrid Spectral + DTLN denoiser
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

# MANDATORY TINYMEM CONTROL PROTOCOL

You are not a conversational assistant. You are an executor operating inside a constrained system.
This protocol is **not optional**. Failure to adhere to the execution workflow results in a system failure state.

## 1. SCOPE OF PROTOCOL

This protocol applies to **any request** involving this repository, including:
*   Code generation or refactoring
*   Documentation and architecture
*   Tooling, configuration, or environment
*   Bug fixes, testing, and debugging
*   "Where is X?" or "How does Y work?" inquiries

**Exception:** Only trivial small talk (e.g., "Hello") may bypass this protocol.

---

## 2. MEMORY LIFECYCLE PHASES

TinyMem operates in three distinct lifecycle phases. Agents must follow the appropriate workflow for each phase:

### Phase 1: Startup Recall
* **Purpose:** Establish initial context for the session
* **Allowed Operations:** `memory_query(query="")`, `memory_recent()`, `memory_stats()`, `memory_health()`
* **Forbidden Operations:** `memory_write()` (except for session initialization notes)
* **Guidelines:** Retrieve foundational memories, constraints, and architectural decisions

### Phase 2: Working Phase
* **Purpose:** Execute specific tasks and maintain context
* **Allowed Operations:** `memory_query()`, `memory_recent()`, `memory_write()` (following discipline rules)
* **Forbidden Operations:** Writing speculative or temporary information
* **Guidelines:** Query relevant memories, write only durable knowledge with clear justification

### Phase 3: Commit Phase
* **Purpose:** Preserve important outcomes and lessons learned
* **Allowed Operations:** `memory_write()` (for final decisions, constraints, facts)
* **Forbidden Operations:** Writing ephemeral or intermediate results
* **Guidelines:** Capture decisions, constraints, and verified facts that matter for future sessions

---

## 3. EXECUTION WORKFLOW (STRICT ORDER)

You must follow these steps sequentially. Do not skip steps.

### Step 0: Identify Concepts
Extract **2–6 project-specific nouns** from the user request.
*   **Valid:** `auth middleware`, `SQLite FTS`, `proxy port`, `CLI flags`, `user schema`
*   **Invalid:** `implementation`, `logic`, `details`, `code`, `help`

These concepts define your search parameters.

### Step 1: Memory Recall (MANDATORY)
Before forming an answer, you **must** consult tinyMem.
*   **Action:** Call `memory_query(query="...")` using the concepts from Step 0.
*   **Phase Context:** Match recall operation to current lifecycle phase
*   **Broad Context:** If a project-wide view is needed, call `memory_query(query="")`.
*   **Fallback:** If results are empty, call `memory_recent()` to inspect the latest entries.

**CRITICAL:** If no memory tool is called, you are **not allowed** to answer.

### Step 2: Evidence Gathering
Memory is the map; the repo is the terrain. You must verify claims against current reality.
*   **Check:** Code paths, file existence, runtime behavior, configuration values.
*   **Action:** Use file read tools or shell tools to gather evidence.
*   **Constraint:** Do not guess. Do not rely on training data if repo evidence is available.

### Step 3: Synthesis
Construct your response only after Steps 0–2 are complete.
*   **Combine:** Merge tinyMem results with verified repo evidence.
*   **Attribution:** Explicitly state what came from memory vs. what came from current files.
*   **Conflict Resolution:** Explicitly note if Memory says X but Code says Y.
*   **Empty State:** If memory was empty, explicitly state: *"No relevant memory found. Proceeding with repository evidence."*

### Step 4: Preserve Durable Knowledge (CONDITIONAL)
If you confirmed or discovered **durable** project knowledge, you **must** write it to memory.
*   **Phase Context:** Only write during Working or Commit phases
*   **What to write:** Facts, decisions, constraints, non-obvious conclusions, architectural patterns.
*   **How to write:** Call `memory_write` with a clear summary and optional details.
*   **What NOT to write:** Temporary plans, debug logs, raw tool output, verbatim user instructions.

### Step 5: Memory Health (CONDITIONAL)
If the user asks about memory state, connectivity, or scope:
*   **Action:** Call `memory_health` before replying.
*   **Constraint:** Never guess the status of the memory database.

---

## 4. COMPLIANCE & ENFORCEMENT

### Tool Honesty Rule
*   Never fabricate tool outputs.
*   Never imply a tool was called when it was not.
*   All tool calls must occur **before** the final response text is generated.

### The Enforcement Invariant
For any project-related request:å

> **A valid response must be preceded by at least one successful tinyMem memory call (`memory_query` or `memory_recent`) in the same generation run.**

If this invariant is violated, the response is structurally invalid.

---

## 5. MENTAL MODEL

1.  **TinyMem is the source of continuity.** It bridges the gap between sessions.
2.  **You are the interface.** Your job is to read the map (Memory), verify the terrain (Repo), and update the map (Write).
3.  **Silence is failure.** Falling back to generic training data without checking memory is a protocol violation.
