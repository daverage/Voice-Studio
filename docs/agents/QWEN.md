# Agent Contract for tinyMem

# TINYMEM CONTROL PROTOCOL (Condensed)

## Hard Enforcement Specification for Repository-Aware AI Agents

This protocol governs **all repository-related behavior**. It is **authoritative**, **mandatory**, and **self-validating**.

---

## 1. Mandatory Workflow (Strict Order)

### Step 1: Memory Recall & Acknowledgement
**BEFORE reasoning**, execute `memory_query("")` or `memory_recent()`.
Immediately state verbatim: **“Relevant memory found and applied.”** OR **“Memory queried. No relevant memory found.”**

### Step 2: Task Authority (`tinyTasks.md`)
If it exists, you **must** read it before acting. Update subtasks as you progress. Resume from the first unchecked subtask.

### Step 3: Execution & Repair (The Ralph Loop)
Perform work. For complex iterative fixes, invoke `memory_ralph`. Success is defined solely by **Evidence Evaluation**.

### Step 4: Durable Knowledge & Self-Validation
If new durable knowledge (decisions, invariants) is produced, write it to memory. Otherwise, state: **"No durable memory write required."**
**EVERY response MUST end with this status line:**
`TM: [Query: OK] [Tasks: OK] [Knowledge: OK/None]`

---

## 2. Enforcement Invariants
- **No Tool, No Response:** A response without a tinyMem tool call is a failure.
- **Fail Closed:** If memory tools fail, declare it and stop irreversible changes.
- **Proof of Execution:** Tool calls must be visible.

**End of Protocol**