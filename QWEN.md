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
**Start of tinyMem Protocol**
# TINYMEM AGENT CONTRACT

## Memory Governance & Task Authority Specification

This contract governs **all repository-related behavior** when tinyMem is present.

It is **authoritative**, **mandatory**, and **self-validating**.
Non-compliance invalidates the response by definition.

---

## Core Principle

> **Observation is free. Mutation is explicit.**

Reading, inspecting, recalling, and reasoning require no ceremony.
Only durable state mutation requires explicit intent and gating.

---

## 1. Binding Definitions

**Repository-related request**
Any request that touches code, files, documentation, architecture, configuration, planning, tasks, or repository state.

**Durable mutation**
Any action that changes repository state or creates durable project state:
* Writing or modifying files
* Creating, updating, or completing tasks
* Promoting claims to facts
* Writing decision or constraint memories

**Task Authority**
`tinyTasks.md` in the project root is the **single source of truth** for task state.
Task state must never be inferred.

---

## 2. Observation (Always Allowed)

The following require no mode declaration:
* Query memory (`memory_query`, `memory_recent`)
* Check task authority (`memory_check_task_authority`)
* Read health/diagnostics (`memory_health`, `memory_doctor`, `memory_stats`, `memory_run_metadata`)
* Read files
* Analyze code
* Provide guidance
* Ask questions

Memory recall is **strongly recommended** for all repository-related conversations, and **mandatory** before any durable mutation.

**NOTE:** In GUARDED and STRICT modes, memory recall is **enforced** — `memory_write` will fail if `memory_query` or `memory_recent` was not called first. In PASSIVE mode, violations are logged but not blocked.

---

## 3. Mutation (Requires Explicit Intent)

Before performing any durable mutation, you MUST:

1. **Query project memory** using `memory_query` or `memory_recent`
   - Retrieve facts, decisions, constraints, and patterns
   - Ensure work aligns with project truth

2. **Declare intent** by calling `memory_set_mode`
   - The system will enforce the appropriate clearance for the requested mutation

3. **Check task authority** by reading `tinyTasks.md` (or confirming it doesn't exist)
   - **Recommended:** Use `memory_check_task_authority` to get explicit authorization status
   - Alternative: Read `tinyTasks.md` directly via file system tools
   - If unchecked tasks exist, resume from the first unchecked subtask
   - If tasks exist but none are unchecked, refuse execution and request user input
   - If file doesn't exist, you may create it for multi-step work (or the system may auto-create it)

---

## 4. tinyTasks.md (Task Authority)

### When Required
Multi-step work persisting across turns requires task tracking via `tinyTasks.md`.

### Auto-Creation
The system may auto-create `tinyTasks.md` when multi-step work is implied.

**Critical invariants:**
* Presence of `tinyTasks.md` is **not** authorization
* Presence of unchecked, human-authored tasks **is** authorization

If the file exists with no unchecked tasks, refuse execution and request human input.

### Canonical Inert Template
```md
# Tasks — NOT STARTED
>
> This file was created automatically because a multi-step workflow
> may be required.
>
> No work is authorised until a human edits this file and defines tasks.

## Tasks
<!-- No tasks defined yet -->
```

### Required Structure
```md
# Tasks – <Goal>

- [ ] Top-level task
  - [ ] Atomic subtask
```

---

## 5. Durable Memory Writeback (MANDATORY)

### When to Write

**Write memories immediately when:**

1. **User states a preference or decision**
   - Example: "We prefer React over Vue"
   - Action: `memory_write` with type `decision`

2. **A constraint is established**
   - Example: "Never commit secrets to git"
   - Action: `memory_write` with type `constraint`

3. **You discover a verifiable fact**
   - Example: "API runs on port 8080"
   - Action: `memory_write` with type `fact` (include evidence)

4. **Architectural pattern is defined**
   - Example: "All services use dependency injection"
   - Action: `memory_write` with type `decision`

5. **User corrects your understanding**
   - Example: "No, we use PostgreSQL, not MySQL"
   - Action: `memory_write` with type `fact`

### Evidence Requirements

* **Facts** require evidence: `cmd_exit0::test command`, `file_exists::path`, `grep_hit::pattern::file`
* **Decisions and constraints** require rationale in `detail` field
* **Notes and observations** are free-form

### After Writing

Confirm the memory write to the user:
```
✅ Stored decision: "Prefer TypeScript for new features"
```

---

## 6. Error Handling

If a required tool operation fails:
* Declare the failure
* Retry up to 2 times
* Stop and request user intervention
* Do NOT proceed with irreversible actions

---

## 7. Invalid Actions

The following invalidate the response:
* No memory recall executed before repository-related work
* No task-state read when multi-step work is involved (or confirmation of absence)
* Inferring task or memory state
* Ignoring unchecked tasks in `tinyTasks.md`
* Writing speculative memory as durable facts
* Mutating state without explicit intent declaration

---

## 8. End-of-Response Checklist (Multi-Step Work)

When performing multi-step work, validate:

* [ ] Memory recall executed (grounded in project truth)
* [ ] Intent declared via `memory_set_mode`
* [ ] `tinyTasks.md` read (or confirmed missing)
* [ ] Tasks updated if applicable
* [ ] Durable memory written OR explicit declaration of none needed

---

## 9. Enforcement & Tracking

### MCP Boundary Enforcement

The following contract requirements are **enforced at the MCP boundary** (not just recommended):

**Recall Before Mutation (GUARDED/STRICT):**
- `memory_write` will **fail** if `memory_query` or `memory_recent` was not called first
- Error returned: "Memory recall required: Call memory_query or memory_recent before memory_write"
- In PASSIVE mode: Logged as violation but not blocked

**Violation Tracking:**
- All contract violations are tracked in `memory_run_metadata`
- Fields: `violations_count`, `blocked_actions_count`, `enforcement_events[]`
- Violations include: recall not performed, unauthorized task execution attempts
- Use this data to audit agent compliance and identify enforcement gaps

### Helper Tools

**memory_check_task_authority()**
- Returns task file status: `{exists, unchecked_tasks, authorization, task_count, next_task}`
- Authorization values:
  - `"create_allowed"` - file absent, agent may create
  - `"authorized"` - unchecked tasks present, agent may proceed
  - `"unauthorized"` - no unchecked tasks, agent must request user input
- Use this instead of manually parsing `tinyTasks.md` for clearer contract compliance

**memory_run_metadata()**
- Returns enforcement metadata for the current session
- Includes: execution mode, enforcement events, violation counts, success counts
- Use to verify your own contract compliance during execution

---

## 10. Summary

**Simple rules:**

1. **Think freely** — read, query, analyze without restriction
2. **Declare intent** — call `memory_set_mode` before mutation
3. **Respect tasks** — never bypass `tinyTasks.md` authority
4. **Write memories** — capture decisions, constraints, facts as you learn
5. **Fail closed** — if unsure, ask; if blocked, stop

**End of tinyMem Protocol**
