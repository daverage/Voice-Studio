

# TINYMEM CONTROL PROTOCOL

## Hard Enforcement Specification for Repository-Aware AI Agents

This protocol governs **all repository-related behavior**.

It is **authoritative**, **mandatory**, and **self-validating**.
Non-compliance invalidates the response by definition.

---

## 0. Binding Definitions

**Repository-related request**
Any request that touches code, files, documentation, architecture, configuration, tasks, planning, or repository state.

**TinyMem command**
A real, externally executed memory tool invocation (`memory_query`, `memory_recent`, etc.).
Internal recall, inference, or chat reconstruction does **not** qualify.

**Valid response**
A response that demonstrates protocol compliance through observable actions and explicit declarations.

---

## 1. Absolute Precondition (Hard Gate)

**Before producing ANY repository-related response, the agent MUST execute at least one TinyMem command.**

This is a hard gate.

If no TinyMem command is executed:

* The response is invalid
* The agent must not proceed
* Partial or “best-effort” responses are forbidden

There are no exceptions.

---

## 2. Proof-of-Execution Requirement (Anti-Circumvention)

Every valid repository-related response MUST include **explicit proof** of TinyMem execution.

Acceptable proof consists of one of the following:

* A visible TinyMem tool invocation in the response
* A verifiable execution record emitted by the environment

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
* “No relevant memory” is illegal unless a query was executed

No recall → stop immediately.

---

### Step 2: Memory Acknowledgement (MANDATORY)

Immediately after recall, the agent MUST explicitly state **one and only one** of the following:

* **“Relevant memory found and applied.”**
* **“Memory queried. No relevant memory found.”**

Omission or paraphrasing invalidates the response.

---

### Step 3: Task Authority Lock (MANDATORY WHEN APPLICABLE)

If `tinyTasks.md` exists, it is **exclusive and authoritative**.

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

---

### Step 3.5: Autonomous Repair (The Ralph Loop)

For complex, iterative tasks requiring verification (e.g., fixing failing tests), the agent SHOULD invoke `memory_ralph`.

**Control Transfer Contract:**
1. Once `memory_ralph` is invoked, control transfers to tinyMem.
2. The agent may not execute individual shell commands or declare success until the loop returns.
3. Termination is controlled solely by **Evidence Evaluation**.

**Execution Phases:**
- **Execute**: tinyMem runs the verification command (e.g., `go test`).
- **Recall**: On failure, tinyMem retrieves relevant memories and failure patterns.
- **Repair**: tinyMem uses its internal LLM to apply code fixes based on context.
- **Evidence**: Success is declared only if all evidence predicates pass.
  - **Format Requirement**: Predicates MUST use the `type::content` format (e.g., `test_pass::go test ./...`, `file_exists::path/to/file`).

**Safety Rules:**
- Agents MUST provide `forbid_paths` for sensitive directories.
- Agents SHOULD set `max_iterations` to prevent runaway token usage.

---

### Step 4: Execution Phase

Only after Steps 1–3 are complete may the agent:

* Perform the requested work
* Modify code, documentation, or plans
* Propose or apply decisions

Any execution before this point invalidates the response.

---

### Step 5: Durable Memory Writeback (MANDATORY WHEN APPLICABLE)

If the response introduces, confirms, or corrects **durable knowledge**, the agent MUST write it to TinyMem **before concluding**.

Durable knowledge includes:

* Decisions
* Constraints or invariants
* Architectural rules
* Corrections to assumptions
* Explicit user confirmations
* Non-obvious conclusions

If and only if no durable knowledge was produced, the agent MUST state verbatim:

> No durable memory write required for this response.

Missing or altered wording invalidates the response.

---

### Step 6: End-of-Response Self-Validation (MANDATORY)

The response MUST end with an explicit checklist confirming all of the following:

* TinyMem command executed
* Memory integrated or explicitly confirmed empty
* `tinyTasks.md` read and updated if applicable
* No completed tasks left unchecked
* No unchecked tasks remain unless explicitly blocked

If any item cannot be affirmed, the agent MUST continue execution.
The response may not terminate.

---

## 4. Error Handling (Fail Closed)

If any TinyMem operation fails:

* The failure MUST be explicitly declared
* Memory guarantees are considered degraded
* Planning, restructuring, or irreversible changes are forbidden
* Only safe, local, reversible actions are allowed

Silent failure is a critical violation.

---

## 5. Invalid Actions (Automatic Failure)

Any of the following immediately invalidate the response:

* No TinyMem command executed
* No proof of execution shown
* Claiming memory state without querying
* Inferring task state
* Ignoring `tinyTasks.md`
* Declaring completion with unchecked tasks
* Writing speculative, temporary, or reversible information to memory
* Ending the response without explicit self-validation

---

## 6. Enforcement Invariant (Unbreakable)

> A repository-related response without a TinyMem tool invocation is not a response. It is a failure.

Memory is mandatory.
Inference is failure.
Execution state lives in files.

---

**End of Protocol**
