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

# TINYMEM AGENT CONTRACT (Governed — Task-Externalised)

This contract governs all repository-related behavior when tinyMem is present.
Non-compliance invalidates the response.

---

## 0. Scope

A request is **repository-related** if it touches:

* code
* files
* documentation
* configuration
* architecture
* tasks
* planning
* repository state

---

## 1. Core Principle

Observation is free.
Sequencing is authority.
Mutation is explicit.

---

## 2. Tool Definitions (Authoritative)

### Memory Recall

* `memory_query`
* `memory_recent`

Available in ALL modes (implementing "Observation is free").
Required before any mutation in GUARDED/STRICT modes.

### Intent Declaration

* `memory_set_mode`

Required before any mutation.

### Memory Write

* `memory_write`

The **only** permitted mechanism for durable memory.

### Task Authority

* `tinyTasks.md` in the project root
* Optional task-authority helper tool

---

## 3. Definitions

### Observation

Reading, inspecting, analyzing, summarizing, or asking questions.

### Mutation

Any durable state change, including:

* writing or modifying files
* creating, updating, or completing tasks
* writing memory
* promoting a claim to a fact, decision, or constraint

### Task Authority

`tinyTasks.md` is the single source of truth for task state.
Task state must never be inferred.

### Task Identification

The moment the agent identifies, implies, or sequences more than one actionable step.

This includes:

* plans
* approaches
* checklists
* ordered bullets
* “first / then / next”
* step-by-step reasoning

---

## 4. Modes (Intent)

You operate in exactly one mode:

* **PASSIVE** — observation only
* **GUARDED** — bounded, reversible mutation
* **STRICT** — maximum caution, full enforcement

Mode MUST be declared via `memory_set_mode` before mutation.

---

## 5. Rule Set (Stable IDs)

### R1 — Recall Before Mutation

Memory recall tools (`memory_query`, `memory_recent`) are available in ALL modes (implementing "Observation is free").

Before any mutation in GUARDED/STRICT modes, you MUST:

* call `memory_query` or `memory_recent`
* acknowledge the result (even if empty)

---

### R2 — Task Externalisation Is Mandatory

The agent may NOT hold a task list internally.

If **Task Identification** occurs:

1. All steps MUST be externalised into `tinyTasks.md`
2. No mutation may occur until task authority is resolved

If `tinyTasks.md` does NOT exist:

* Create the inert template
* Populate it with a proposed task list
* STOP
* Request the human to review, edit, reorder, or approve the proposed tasks

Creation or population of `tinyTasks.md` does NOT authorize work.

Planning in the response body is prohibited once this rule triggers.

#### Task Proposal Allowance

The agent MAY populate `tinyTasks.md` with a proposed task list.

Proposed tasks are NOT authorized until a human:
- confirms them explicitly, or
- edits or reorders them, or
- states approval in plain language

The agent MUST stop after proposing tasks and wait for human authorization.

---

### R3 — Tasks Are Authoritative

If `tinyTasks.md` exists:

* Continue the **first unchecked subtask**
* If no unchecked subtasks exist, STOP and request user input

The agent may NOT:

* skip tasks
* reorder tasks
* redefine tasks
* invent progress

---

### R4 — Mutation Requires Intent

Before any mutation, ALL of the following MUST be true, in order:

1. R1 satisfied (memory recall in GUARDED/STRICT modes)
2. Intent declared via `memory_set_mode`
3. R2 satisfied (task externalised if required)
4. R3 satisfied (task authority confirmed)

---

### R5 — Durable Memory Is Tool-Only

* Use `memory_write` only
* Facts require evidence
* Decisions and constraints require rationale
* Never claim a memory write unless the tool succeeded

---

### R6 — Fail Closed

If recall, intent, task authority, or enforcement is uncertain:

* Continue with observation only, OR
* STOP and request user input

Never guess. Never proceed optimistically.

---

## 6. tinyTasks.md Templates

### Inert Auto-Creation Template

```md
# Tasks — PROPOSED
>
> These tasks were proposed by the agent.
> No work is authorised until a human reviews and confirms them.
>
## Tasks
<!-- No tasks defined yet -->
```

### Active Task Structure

```md
# Tasks – <Goal>

- [ ] Top-level task
  - [ ] Atomic subtask
```

Rules:

* Two levels only
* Order matters
* Unchecked == authorized *after human confirmation*

---

## 7. Enforcement Expectations

Expected to be enforceable at the boundary:

* block mutation without recall
* block mutation without intent
* block mutation when tasks are required but missing
* block mutation when tasks exist but none are unchecked
* track violations for audit

If enforcement is unavailable, self-enforce and fail closed (R6).

---

## 8. Error Handling

If a required tool fails:

1. State the failure
2. Retry up to 2 times
3. STOP and request human intervention

---

## 9. End-of-Response Checklist (When Mutation Occurs)

Confirm explicitly:

* recall completed in GUARDED/STRICT modes (R1)
* mode declared (R4)
* task authority resolved (R2, R3)
* memory writes completed or not required (R5)

Do not restate this contract.

---

**End of tinyMem Protocol**
