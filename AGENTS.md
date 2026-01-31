# AGENTS.md

This file provides global guidance for any agent working in the Voice Studio (VxCleaner) repository.

## Repository snapshot
- Package version: `vxcleaner` 0.6.0 (`Cargo.toml`, `src/lib.rs::VERSION`).
- Deterministic DSP pipeline for speech restoration; no neural inference path exists in this repo.

## Build & release process
- Standard builds: `cargo build`, `cargo build --release`, `cargo nih-plug bundle vxcleaner --release`, and the debug variant (`--features debug`).
- Release automation lives in `tools/release.sh`; it bundles macOS/Windows/Linux, zips each bundle, and creates a GitHub release (`v0.6.0`). Set `SKIP_LINUX=1` when Docker/cross is unavailable.
- Windows bundles require `xwin`, `clang-cl`, `lld-link`, `llvm-lib`. Linux bundles require Docker with `cross` plus a sysroot path for `pkg-config`.

## Modes & UI
- Simple mode exposes Clean, Enhance, Control macros that drive the Clean & Repair, Shape & Polish, and dynamics stacks.
- Advanced mode exposes every slider (rumble, hiss, static noise, noise reduction, de-verb, breath control, proximity, clarity, de-ess, leveler, gain) with a Quality meter beneath the noise controls.
- The UI code lives in `src/ui` (layout, advanced, simple, components, meters, state) and draws meter state from `src/meters.rs`.

## Web & help assets
- Marketing page: `web/index.html`. It now narrates the deterministic workflow, adds a Modes section with the assets from `web/assets/icons/simple.png` and `web/assets/icons/advanced.png`, and links to the `v0.6.0` release.
- Help page: `web/help.html` documents the macros, sliders, and chains, reuses the same mode assets, and reports `VxCleaner v0.6.0` in the footer.

## Documentation housekeeping
- Core docs live in `README.md`, `docs/`, `docs/agents/`, AGENT contracts, and the LLM guidance files (`CLAUDE.md`, `GEMINI.md`, `QWEN.md`, `AGENT_CONTRACT.md`).
- Historical/deadlined writeups (UI fix/refactor plans, NEXT_STEPS, aggregation tables, dead code notes) now live under `docs/archive/`. The root keeps only working documentation, LLM guidance, and README-level references.
- Always sync `CLAUDE.md`, `GEMINI.md`, `QWEN.md`, and `AGENTS.md` whenever the architecture, release flow, or constraints change.
- Ensure the UI shows the “update available” footer notification when a newer GitHub release exists, as a side effect of the release checker built into the UI footer.

## Agent instructions
- Before touching repository state, run a TinyMem recall (`memory_query`/`memory_recent`) and read `tinyTasks.md` (it is the single source of truth for task status).
- Update `tinyTasks.md` with every meaningful milestone (especially documentation work).
- After release-related edits, bump `Cargo.toml`/`src/lib.rs`, sync `Cargo.lock`’s `vxcleaner` version, and refresh any footer text or help page version references.
- Follow the `AGENT_CONTRACT.md` workflow (hard gate for memory, task checks, durable memory writeback, self-validation checklist).

- No destructive commands (`git reset --hard`, `rm -rf`) unless explicitly requested.
- Do not revert changes you did not make without asking the user.

# TINYMEM AGENT CONTRACT

## Hard Enforcement Specification for Repository-Aware AI Agents

This contract governs **all repository-related behavior**.

It is **authoritative**, **mandatory**, and **self-validating**.
Non-compliance invalidates the response by definition.

---

## 0. Binding Definitions

**Repository-related request**
Any request that touches code, files, documentation, architecture, configuration, tasks, planning, or repository state.

**TinyMem command**
A real, externally executed memory tool invocation (`memory_query`, `memory_recent`, `memory_write`, etc.).
Internal recall, inference, or chat reconstruction does **not** qualify.

**Task Authority**
The `tinyTasks.md` file in the project root is the **single source of truth** for task state.
File operations (`read_file`, `write_file`, `replace`, `cat`) on this file are the only valid way to check or update tasks.

**Valid response**
A response that demonstrates protocol compliance through observable actions and explicit declarations.

---

## 1. Absolute Precondition (Hard Gate)

**Before producing ANY repository-related response, the agent MUST execute at least one TinyMem command AND check task state.**

This is a hard gate.

If no TinyMem command is executed OR `tinyTasks.md` is not read:

* The response is invalid
* The agent must not proceed
* Partial or "best-effort" responses are forbidden

There are no exceptions.

---

## 2. Proof-of-Execution Requirement (Anti-Circumvention)

Every valid repository-related response MUST include **explicit proof** of:

* At least one TinyMem tool invocation
* At least one read of `tinyTasks.md` (e.g., `cat tinyTasks.md`) OR confirmation that it does not exist

Silent execution is forbidden.
Missing proof invalidates the response.

---

## 3. Mandatory Execution Order (Non-Reorderable)

The following steps MUST be executed **in strict order**.
Skipping, merging, or reordering steps is a violation.

---

### Step 1: Memory Recall (MANDATORY, FIRST)

The agent MUST execute **at least one** of the following **before any reasoning**:

```
memory_query("")
memory_recent()
memory_query("<specific topic>")
```

Rules:

* This must be a real tool execution
* Assumed recall is forbidden
* Chat history does not count

No recall → stop immediately.

**Result declaration (one and only one):**

* **"Relevant memory found and applied [to context/decision/implementation]."** (with evidence)
* **"Memory queried. No relevant memory found."**

Omission or paraphrasing invalidates the response.

---

### Step 2: Task Authority Lock (MANDATORY)

The agent MUST read `tinyTasks.md` to check for existing tasks.

Rules:

* The file MUST be read before any action
* Memory MUST NOT override task state
* Task state MUST NOT be inferred

For any non-trivial, multi-step, or stateful request, the agent MUST:

1. Create or update `tinyTasks.md`
2. Resume from the **first unchecked subtask**
3. Update tasks **as execution progresses**
4. Mark tasks complete **only when actually finished**

Required structure (no deviations allowed):

```md
# Tasks – <Goal>

- [ ] Top-level task
  - [ ] Atomic subtask
  - [ ] Atomic subtask
```

Failure to update tasks is a protocol failure.

**Result declaration (one and only one):**

* **"Task state queried. No pending tasks. [New task created]."** (if new task made)
* **"Task state queried. Resuming task from subtask: [description]."** (if resuming)
* **"Task state queried. No tasks exist."** (if none)

Omission or paraphrasing invalidates the response.

---

### Step 3: Autonomous Repair (The Ralph Loop) — WHEN APPLICABLE

For complex, iterative tasks requiring verification (e.g., fixing failing tests), the agent SHOULD invoke `memory_ralph`.

**Control Transfer Contract:**
1. Once `memory_ralph` is invoked, control transfers to tinyMem.
2. The agent may not execute individual shell commands or declare success until the loop returns.
3. Termination is controlled solely by **Evidence Evaluation**.

**Execution Phases:**
- **Execute**: tinyMem runs the verification command.
- **Recall**: On failure, tinyMem retrieves relevant memories and failure patterns.
- **Repair**: tinyMem uses its internal LLM to apply code fixes based on context.
- **Evidence**: Success is declared only if all evidence predicates pass.
  - **Format Requirement**: Predicates MUST use the `type::content` format (e.g., `test_pass::cargo test`, `file_exists::path/to/file`).

**Safety Rules:**
- Agents MUST provide `forbid_paths` for sensitive directories.
- Agents SHOULD set `max_iterations` to prevent runaway token usage.
- After `memory_ralph` completes, update `tinyTasks.md` to update the task status before proceeding.

---

### Step 4: Execution Phase

Only after Steps 1–3 are complete may the agent:

* Perform the requested work
* Modify code, documentation, or plans
* Propose or apply decisions

**During execution:**

* If a task is active, update `tinyTasks.md` to mark progress after each major milestone
* Update task descriptions or status to reflect current state
* Never leave a task in progress with stale information

Any execution before this point invalidates the response.

---

### Step 5: Durable Memory Writeback (MANDATORY WHEN APPLICABLE)

If the response introduces, confirms, or corrects **durable knowledge**, the agent MUST write it to TinyMem **before concluding**.

**Durable knowledge is defined as ANY of:**

* A decision was made (e.g., "chose to use Redis instead of in-memory cache")
* An assumption was corrected or confirmed (e.g., "discovered that X works differently than documented")
* A constraint or invariant was established or clarified
* An architectural rule was applied or amended
* The user explicitly confirmed something (e.g., "yes, this is the desired behavior")
* A technical discovery with implications for future work (e.g., "ringbuf API requires X pattern")

**If no durable knowledge was produced, the agent MUST state verbatim:**

> No durable memory write required for this response.

Missing or altered wording invalidates the response.

---

### Step 6: Task Completion (MANDATORY WHEN APPLICABLE)

If a task was being tracked:

* Verify the current task state in `tinyTasks.md`
* Update `tinyTasks.md` to mark the task or subtasks as `[x]` (completed)
* If subtasks remain, update the task description to reflect next unchecked subtask

---

### Step 7: End-of-Response Self-Validation (MANDATORY)

The response MUST end with an explicit checklist confirming all of the following:

- [ ] TinyMem command executed (tool invocation visible)
- [ ] Memory integrated or explicitly confirmed empty
- [ ] `tinyTasks.md` read and updated if applicable
- [ ] No completed tasks left unchecked
- [ ] No unchecked tasks remain unless explicitly blocked
- [ ] Durable knowledge written to memory OR "No durable memory write required" stated

If any item cannot be affirmed, the agent MUST continue execution.
The response may not terminate.

---

## 4. Error Handling (Fail Closed)

If any tool operation fails (TinyMem, File I/O, Bash, etc.):

* The failure MUST be explicitly declared with the tool name and error
* Memory guarantees are considered degraded
* Task state cannot be assumed
* Planning, restructuring, or irreversible changes are forbidden
* Only safe, local, reversible actions are allowed
* Attempt to retry the failed operation up to 2 times
* If still failing, STOP and ask the user for manual intervention

Silent failure is a critical violation.

---

## 5. Invalid Actions (Automatic Failure)

Any of the following immediately invalidate the response:

* No TinyMem command executed
* No task state query executed (reading `tinyTasks.md`)
* No proof of execution shown
* Claiming memory state without querying
* Inferring task state (saying "I assume there are no tasks")
* Ignoring active tasks (unchecked items in `tinyTasks.md`)
* Declaring completion with unchecked tasks
* Writing speculative, temporary, or reversible information to memory
* Ending the response without explicit self-validation checklist
* Task marked complete when subtasks remain unchecked

---

## 6. Enforcement Invariant (Unbreakable)

> A repository-related response without TinyMem and `tinyTasks.md` operations is not a response. It is a failure.

Memory is mandatory.
Task state is authoritative.
Inference is forbidden.
Execution state lives in files, not in agent inference.

---

**End of Protocol**

# TINYMEM AGENT CONTRACT

## Hard Enforcement Specification for Repository-Aware AI Agents

This contract governs **all repository-related behavior**.

It is **authoritative**, **mandatory**, and **self-validating**.
Non-compliance invalidates the response by definition.

---

## 0. Binding Definitions

**Repository-related request**
Any request that touches code, files, documentation, architecture, configuration, tasks, planning, or repository state.

**TinyMem command**
A real, externally executed memory tool invocation (`memory_query`, `memory_recent`, `memory_write`, etc.).
Internal recall, inference, or chat reconstruction does **not** qualify.

**Task Authority**
The `tinyTasks.md` file in the project root is the **single source of truth** for task state.
File operations (`read_file`, `write_file`, `replace`, `cat`) on this file are the only valid way to check or update tasks.

**Valid response**
A response that demonstrates protocol compliance through observable actions and explicit declarations.

---

## 1. Absolute Precondition (Hard Gate)

**Before producing ANY repository-related response, the agent MUST execute at least one TinyMem command AND check task state.**

This is a hard gate.

If no TinyMem command is executed OR `tinyTasks.md` is not read:

* The response is invalid
* The agent must not proceed
* Partial or "best-effort" responses are forbidden

There are no exceptions.

---

## 2. Proof-of-Execution Requirement (Anti-Circumvention)

Every valid repository-related response MUST include **explicit proof** of:

* At least one TinyMem tool invocation
* At least one read of `tinyTasks.md` (e.g., `cat tinyTasks.md`) OR confirmation that it does not exist

Silent execution is forbidden.
Missing proof invalidates the response.

---

## 3. Mandatory Execution Order (Non-Reorderable)

The following steps MUST be executed **in strict order**.
Skipping, merging, or reordering steps is a violation.

---

### Step 1: Memory Recall (MANDATORY, FIRST)

The agent MUST execute **at least one** of the following **before any reasoning**:

```
memory_query("")
memory_recent()
memory_query("<specific topic>")
```

Rules:

* This must be a real tool execution
* Assumed recall is forbidden
* Chat history does not count

No recall → stop immediately.

**Result declaration (one and only one):**

* **"Relevant memory found and applied [to context/decision/implementation]."** (with evidence)
* **"Memory queried. No relevant memory found."**

Omission or paraphrasing invalidates the response.

---

### Step 2: Task Authority Lock (MANDATORY)

The agent MUST read `tinyTasks.md` to check for existing tasks.

Rules:

* The file MUST be read before any action
* Memory MUST NOT override task state
* Task state MUST NOT be inferred

For any non-trivial, multi-step, or stateful request, the agent MUST:

1. Create or update `tinyTasks.md`
2. Resume from the **first unchecked subtask**
3. Update tasks **as execution progresses**
4. Mark tasks complete **only when actually finished**

Required structure (no deviations allowed):

```md
# Tasks – <Goal>

- [ ] Top-level task
  - [ ] Atomic subtask
  - [ ] Atomic subtask
```

Failure to update tasks is a protocol failure.

**Result declaration (one and only one):**

* **"Task state queried. No pending tasks. [New task created]."** (if new task made)
* **"Task state queried. Resuming task from subtask: [description]."** (if resuming)
* **"Task state queried. No tasks exist."** (if none)

Omission or paraphrasing invalidates the response.

---

### Step 3: Autonomous Repair (The Ralph Loop) — WHEN APPLICABLE

For complex, iterative tasks requiring verification (e.g., fixing failing tests), the agent SHOULD invoke `memory_ralph`.

**Control Transfer Contract:**
1. Once `memory_ralph` is invoked, control transfers to tinyMem.
2. The agent may not execute individual shell commands or declare success until the loop returns.
3. Termination is controlled solely by **Evidence Evaluation**.

**Execution Phases:**
- **Execute**: tinyMem runs the verification command.
- **Recall**: On failure, tinyMem retrieves relevant memories and failure patterns.
- **Repair**: tinyMem uses its internal LLM to apply code fixes based on context.
- **Evidence**: Success is declared only if all evidence predicates pass.
  - **Format Requirement**: Predicates MUST use the `type::content` format (e.g., `test_pass::cargo test`, `file_exists::path/to/file`).

**Safety Rules:**
- Agents MUST provide `forbid_paths` for sensitive directories.
- Agents SHOULD set `max_iterations` to prevent runaway token usage.
- After `memory_ralph` completes, update `tinyTasks.md` to update the task status before proceeding.

---

### Step 4: Execution Phase

Only after Steps 1–3 are complete may the agent:

* Perform the requested work
* Modify code, documentation, or plans
* Propose or apply decisions

**During execution:**

* If a task is active, update `tinyTasks.md` to mark progress after each major milestone
* Update task descriptions or status to reflect current state
* Never leave a task in progress with stale information

Any execution before this point invalidates the response.

---

### Step 5: Durable Memory Writeback (MANDATORY WHEN APPLICABLE)

If the response introduces, confirms, or corrects **durable knowledge**, the agent MUST write it to TinyMem **before concluding**.

**Durable knowledge is defined as ANY of:**

* A decision was made (e.g., "chose to use Redis instead of in-memory cache")
* An assumption was corrected or confirmed (e.g., "discovered that X works differently than documented")
* A constraint or invariant was established or clarified
* An architectural rule was applied or amended
* The user explicitly confirmed something (e.g., "yes, this is the desired behavior")
* A technical discovery with implications for future work (e.g., "ringbuf API requires X pattern")

**If no durable knowledge was produced, the agent MUST state verbatim:**

> No durable memory write required for this response.

Missing or altered wording invalidates the response.

---

### Step 6: Task Completion (MANDATORY WHEN APPLICABLE)

If a task was being tracked:

* Verify the current task state in `tinyTasks.md`
* Update `tinyTasks.md` to mark the task or subtasks as `[x]` (completed)
* If subtasks remain, update the task description to reflect next unchecked subtask

---

### Step 7: End-of-Response Self-Validation (MANDATORY)

The response MUST end with an explicit checklist confirming all of the following:

- [ ] TinyMem command executed (tool invocation visible)
- [ ] Memory integrated or explicitly confirmed empty
- [ ] `tinyTasks.md` read and updated if applicable
- [ ] No completed tasks left unchecked
- [ ] No unchecked tasks remain unless explicitly blocked
- [ ] Durable knowledge written to memory OR "No durable memory write required" stated

If any item cannot be affirmed, the agent MUST continue execution.
The response may not terminate.

---

## 4. Error Handling (Fail Closed)

If any tool operation fails (TinyMem, File I/O, Bash, etc.):

* The failure MUST be explicitly declared with the tool name and error
* Memory guarantees are considered degraded
* Task state cannot be assumed
* Planning, restructuring, or irreversible changes are forbidden
* Only safe, local, reversible actions are allowed
* Attempt to retry the failed operation up to 2 times
* If still failing, STOP and ask the user for manual intervention

Silent failure is a critical violation.

---

## 5. Invalid Actions (Automatic Failure)

Any of the following immediately invalidate the response:

* No TinyMem command executed
* No task state query executed (reading `tinyTasks.md`)
* No proof of execution shown
* Claiming memory state without querying
* Inferring task state (saying "I assume there are no tasks")
* Ignoring active tasks (unchecked items in `tinyTasks.md`)
* Declaring completion with unchecked tasks
* Writing speculative, temporary, or reversible information to memory
* Ending the response without explicit self-validation checklist
* Task marked complete when subtasks remain unchecked

---

## 6. Enforcement Invariant (Unbreakable)

> A repository-related response without TinyMem and `tinyTasks.md` operations is not a response. It is a failure.

Memory is mandatory.
Task state is authoritative.
Inference is forbidden.
Execution state lives in files, not in agent inference.

---

**End of Protocol**
