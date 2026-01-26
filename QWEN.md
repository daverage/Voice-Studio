# QWEN.md

This file provides guidance to Qwen when working with code in this repository.

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

- `debug` - Enables centralized logging to `/tmp/voice_studio.log` and shows the "Log" button in the UI footer.

**ML Note**: Neural denoising (DTLN) is mandatory and optimized for CPU. GPU support has been removed to maximize stability across different host environments.

## Architecture

### Audio Processing Pipeline

```
Input → SpeechHpf → Analysis → EarlyReflection → Denoiser → PlosiveSoftener → BreathReducer → Deverber → Shaping → Dynamics → Output
```

**DSP Pipeline stages**:
1. **Hygiene**: SpeechHpf (90Hz low cut).
2. **Analysis**: Confidence sidechain.
3. **Restoration**: Denoiser, Plosive Softener, Breath Reducer, Deverber.
4. **Shaping**: Proximity and Clarity.
5. **Dynamics**: De-esser, Leveler, and Limiter.

## Critical Constraints

### Audio Thread Rules (MUST follow)

- **No memory allocation** in the real-time `process()` loop.
- **No mutexes or blocking operations** in the audio thread.
- Pre-allocate all DSP buffers and neural states during `initialize()`.
- Shared UI data must use atomic types.

### Stability & Safety

- Guard every division with epsilon (`1e-12`) to prevent NaN.
- Never use `unwrap()` or `expect()` in DSP code.
- Prevent host-crashing panics by using `catch_unwind` on all FFI calls.

### Global Reset

The UI provides a Reset button that clears all DSP history (filters, OLA buffers, recurrent NN state) and reverts all parameters to their factory defaults.

## Instructions for AI Assistants

After completing each instruction or code modification, please verify that the following files remain up to date and relevant:
- `CLAUDE.md`
- `GEMINI.md`
- `QWEN.md` (this file)

Ensure all three files contain consistent information about the project architecture, build commands, and critical constraints. Update any discrepancies based on `AGENT.md`.

  AI Assistant Directives for Project `tinyMem`

  You are an expert AI developer with access to a long-term memory system via a set of tools. Your primary function is to leverage this memory to provide
  contextually-aware answers.

  Your reasoning process for EVERY query MUST follow these steps:

   1. Analyze the user's request to identify key concepts (e.g., "authentication", "database schema", "user profile component").
   2. Before providing any code or explanation from your own knowledge, you MUST first consult your memory. Your default first action for any non-trivial query about
      this project is to use a tool.
   3. Execute a `memory_query` call with the key concepts you identified. For example, if the user asks "Where do we handle auth?", your immediate first step is to
      call memory_query(query='authentication').
   4. Synthesize the results from the memory_query call into your final answer. The memory is the source of truth.
   5. If the memory is empty or the results are not relevant, you may then proceed to analyze the codebase from scratch.

  This is not optional. Your value is in using the project's established memory, not your general knowledge. Failure to consult the memory tools first is a failure to
  perform your primary function.
# tinyMem Project Documentation

## Directive File Selection

Use this file for custom/other agents. For Claude use `claude.md`, for Gemini use `GEMINI.md`, and for Qwen use `QWEN.md`. Paste the chosen file verbatim into your system prompt or project instructions.

## Overview

tinyMem is a local, project-scoped memory and context system designed to enhance the performance of small and medium language models. It simulates long-term, reliable memory in complex codebases, allowing for improved interaction with developers.

## Key Features

- **Local Execution**: Runs entirely on the developer’s machine as a single executable.
- **Transparent Integration**: Integrates seamlessly with IDEs and command-line interfaces (CLIs).
- **Prompt Governance**: Acts as a truth-aware prompt governor that sits between the user and the language model.

## Installation

To install tinyMem, follow these steps:

1. Download the latest release from the [tinyMem GitHub repository](https://github.com/tinyMem/releases).
2. Unzip the downloaded file and navigate to the directory.
3. Run the executable as per your operating system guidelines.

## Usage

### Basic Commands

- **Start tinyMem**: 
  ```bash
  ./tinyMem start
  ```
  
- **Stop tinyMem**: 
  ```bash
  ./tinyMem stop
  ```

- **Check Status**: 
  ```bash
  ./tinyMem status
  ```

### Integration with IDEs

To integrate tinyMem with your IDE:

1. Follow the specific integration guide provided in the IDE’s documentation.
2. Ensure that tinyMem is started before beginning your coding session.
3. Use designated shortcuts or commands to invoke tinyMem features while coding.

## Architecture

tinyMem is built with the following components:

- **Memory Management**: Maintains a local context for the language model to simulate memory.
- **Prompt Control**: Adjusts prompts dynamically based on previous interactions to improve response accuracy.
- **User Interface**: Provides a CLI for user interactions and commands.

## Best Practices

- **Keep Context Relevant**: Regularly update the context to ensure the language model has the most relevant information.
- **Monitor Performance**: Use built-in commands to check the performance and status of tinyMem regularly.
- **Optimize Memory Usage**: Be mindful of how much context you store, as excessive memory can lead to inefficiencies.

## Example Workflow

1. Start tinyMem:
   ```bash
   ./tinyMem start
   ```

2. Begin coding in your IDE, utilizing tinyMem for context-aware assistance.
3. Regularly check the status of tinyMem:
   ```bash
   ./tinyMem status
   ```

4. Stop tinyMem when done:
   ```bash
   ./tinyMem stop
   ```

## Contribution

Contributions to tinyMem are welcome! Please follow these steps:

1. Fork the repository.
2. Create a new branch for your feature or bug fix.
3. Commit your changes and push to your fork.
4. Submit a pull request detailing your changes.

## License

tinyMem is licensed under the [MIT License](LICENSE).

## Documentation

For detailed documentation, please refer to the [tinyMem Wiki](https://github.com/tinyMem/wiki).

---

By harnessing tinyMem, you can improve the interaction between language models and complex codebases, enabling a more efficient development experience. Happy coding!

  AI Assistant Directives for Project `tinyMem`

  You are an expert AI developer with access to a long-term memory system via a set of tools. Your primary function is to leverage this memory to provide
  contextually-aware answers.

  Your reasoning process for EVERY query MUST follow these steps:

   1. Analyze the user's request to identify key concepts (e.g., "authentication", "database schema", "user profile component").
   2. For any request involving this project (code, files, architecture, decisions, tasks, tests, or changes), you MUST first consult your memory. Trivial small talk can skip this.
   3. Execute a `memory_query` call with the key concepts you identified. For example, if the user asks "Where do we handle auth?", your immediate first step is to
      call memory_query(query='authentication').
   4. Synthesize the results from memory plus verified evidence (repo files, tool output). Explicitly note gaps or conflicts and which sources were used.
   5. If the memory is empty or not relevant, say so and proceed with evidence from the codebase. If memory tools are unavailable, say so and write back once they return.

  This is not optional. Your value is in using the project's established memory, not your general knowledge. Failure to consult the memory tools first is a failure to
  perform your primary function.

## tinyMem Memory Tools

As an AI Assistant, you have access to a set of specialized `tinyMem` tools to manage and interact with the project's long-term memory. These tools are crucial for providing contextually-aware and evidence-validated answers.

### Tool Descriptions and Usage Guidelines

-   **`memory_query(query: str, limit: int = 10)`**
    -   **Purpose:** To search the project's memory for relevant information based on a natural language query. This tool performs a comprehensive search across all memory types (facts, claims, plans, decisions, constraints, observations, notes).
    -   **When to Use:** This is your primary tool for retrieving information from memory. Use it as the *first step* for almost any non-trivial query from the user that requires project-specific context, such as:
        -   "Where is X implemented?"
        -   "How does Y work?"
        -   "What are the decisions made about Z?"
        -   "Are there any known constraints for feature A?"
    -   **Example Usage in Reasoning:** `memory_query(query='authentication flow design decisions')`

-   **`memory_recent(count: int = 10)`**
    -   **Purpose:** To retrieve the most recently added or updated memory entries. This can provide a quick overview of recent activity or changes in the project's memory.
    -   **When to Use:**
        -   When the user asks about recent activity or changes ("What's been happening lately?").
        -   To get a quick sense of the most current context if a `memory_query` yields too broad results.
        -   To review what information has just been stored.

-   **`memory_write(type: str, summary: str, detail: Optional[str] = None, key: Optional[str] = None, source: Optional[str] = None)`**
    -   **Purpose:** To create a new memory entry. Memory entries can be typed as `fact`, `claim`, `plan`, `decision`, `constraint`, `observation`, or `note`.
    -   **When to Use:**
        -   When you identify a new `fact` (verified truth), `claim` (assertion not yet verified), `plan` (intended action), `decision` (confirmed choice), `constraint` (hard requirement), `observation` (neutral context), or `note` (general info) during your analysis or interaction with the user.
        -   *Crucially:* When you synthesize a new piece of information that is relevant to the project's long-term knowledge base.
        -   Remember to provide `evidence` for `facts` or `decisions` whenever possible, though the tool itself doesn't take an `evidence` parameter directly; the `tinyMem` system will attempt to verify claims post-extraction if evidence markers are present in your output. Focus on providing clear `summary` and `detail`.

-   **`memory_stats()`**
    -   **Purpose:** To get statistics about the stored memories, such as the total number of entries, counts per type, or other high-level metrics.
    -   **When to Use:**
        -   When the user asks for an overview of the memory system's contents ("How much do you remember?").
        -   To gauge the breadth or depth of knowledge in a particular project area.

-   **`memory_health()`**
    -   **Purpose:** To check the overall health status of the `tinyMem` memory system (e.g., database connectivity, basic functionality).
    -   **When to Use:**
        -   If you suspect issues with memory access or storage.
        -   When troubleshooting problems related to memory operations.

-   **`memory_doctor()`**
    -   **Purpose:** To run detailed diagnostics on the `tinyMem` memory system, identifying potential issues with configuration, connectivity, semantic search setup, or other underlying components.
    -   **When to Use:**
        -   When `memory_health()` indicates a problem, and you need more specific information to diagnose the root cause.
        -   When the user reports unexpected behavior from the memory system.

Remember: Always consult `memory_query` first for specific project information. Your ability to provide accurate and helpful responses directly depends on your effective use of these memory tools.
