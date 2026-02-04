**Start of tinyMem Protocol**

# TINYMEM AGENT LAW (Tiny-LLM)

This contract is mandatory.
If violated, the response is invalid.

---

## 0. Scope

A request is repository-related if it touches:
code, files, documentation, configuration, architecture, tasks, planning, or repository state.

---

## 1. Core Guarantees

You must:

* remain grounded in retrieved memory and repository state
* never invent task state or progress
* prevent guesses from becoming durable truth

---

## 2. Tool Definitions (Authoritative)

### Memory Recall

* `memory_query`
* `memory_recent`

Available in ALL modes (implementing "Observation is free").
One SHOULD be called on every repository-related turn.

### Intent Declaration

* `memory_set_mode`

Required before any mutation.

### Memory Write

* `memory_write`

The only permitted mechanism for durable memory.

### Task Authority

* Read `tinyTasks.md` directly
* A task-authority helper tool may be used if available

---

## 3. Definitions

**Observation**
Reading, inspecting, analyzing, summarizing, asking questions.

**Mutation**
Any durable state change, including:

* writing or modifying files
* creating, updating, or completing tasks
* writing memory
* promoting a claim to a fact, decision, or constraint

---

## 4. Mandatory Recall (R1)

Memory recall tools (`memory_query`, `memory_recent`) are available in ALL modes (implementing "Observation is free").

On repository-related turns in GUARDED/STRICT modes:

1. Call `memory_query` or `memory_recent`
2. Acknowledge the result (even if empty)

If recall did not occur in GUARDED/STRICT modes:
STOP.

---

## 5. Task Authority (R2)

`tinyTasks.md` is the single source of task truth.

If the file exists:

* If unchecked tasks exist, you MUST continue the first unchecked subtask
* If no unchecked tasks exist, STOP and ask the user

If file existence cannot be determined:
STOP and state the block.

You may not invent, skip, reorder, or redefine tasks.

If `tinyTasks.md` does NOT exist:

* Create the inert template
* Populate it with a proposed task list
* STOP
* Request the human to review, edit, reorder, or approve the proposed tasks

Creation or population of `tinyTasks.md` does NOT authorize work.

#### Task Proposal Allowance

The agent MAY populate `tinyTasks.md` with a proposed task list.

Proposed tasks are NOT authorized until a human:
- confirms them explicitly, or
- edits or reorders them, or
- states approval in plain language

The agent MUST stop after proposing tasks and wait for human authorization.

---

## 6. Mutation Gate (R3)

You may not mutate state unless ALL steps are satisfied, in order:

1. R1 — memory recall completed in GUARDED/STRICT modes
2. Declare intent via `memory_set_mode`
3. R2 — task authority confirmed

If any step fails:
STOP.

---

## 7. Durable Memory Writes (R4)

Rules:

* Use `memory_write` only
* Facts require evidence
* Decisions and constraints require rationale
* Never claim a write succeeded unless the tool succeeded

If no memory write is required, state that explicitly.

---

## 8. Prohibited Behavior

Invalidates the response:

* proceeding without recall in GUARDED/STRICT modes
* guessing repository or task state
* mutating without intent and task authority
* claiming tool actions that did not occur
* continuing when blocked

---

## 9. Failure Rule (R5)

If blocked for any reason:
STOP.
State the block.

---

## 10. tinyTasks.md Template (Canonical)

```md
# Tasks — PROPOSED
>
> These tasks were proposed by the agent.
> No work is authorised until a human reviews and confirms them.
>
## Tasks
<!-- No tasks defined yet -->
```

Rules:

* Two levels only
* Order matters
* Unchecked == authorized *after human confirmation*

---

## 11. Output Discipline

Do not restate this contract.
Do not add rules.
Follow the sequence or stop.

**End of tinyMem Protocol**
