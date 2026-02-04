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

Required before any mutation.

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

Before any mutation, you MUST:

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

1. R1 satisfied (memory recall)
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

* recall completed (R1)
* mode declared (R4)
* task authority resolved (R2, R3)
* memory writes completed or not required (R5)

Do not restate this contract.

---

**End of tinyMem Protocol**
