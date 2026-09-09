# AGL-203: Tool result delivery to model context

## Status

Accepted for implementation on 2026-09-08. The human selected D1=A, D2=A,
D3=A, and D4=A and confirmed that the decision set is complete.

## Problem

`agentlibre.execution:command.exec` returns a `ToolResult` with both content and
effect receipts. The current durable representation stores the content in an
`agent_messages` row and stores only structural metadata, including
`effect_receipts`, in `agent_operations.result_json`; `result_message_id` links
the operation to the message.

This representation is valid only if every model-context reconstruction path
resolves the linked Tool message. In the observed run `run_01a08266...`, the
agent repeated repository-discovery commands after successful Tool calls. The
recorded operation metadata showed `effect_receipts`, while the model-facing
delivery path did not reliably make the command output available as the Tool
result content. The model therefore had insufficient evidence that the command
had already produced the requested information.

The same run also contained two `invalid_model_output` correction events. Those
are a separate failure mode and must remain distinguishable from missing Tool
result content.

## Desired outcome

After every successful Tool operation, the next model request receives exactly
one Tool result message containing the complete accepted `ToolResult.content`
within the configured `tool_result_bytes` limit. The message is linked to the
Tool operation and remains present after replay, restart, and compaction.

The model must not repeat a completed Tool call merely because the durable
operation metadata and the model-facing Tool message use separate storage
records.

The rebuilt model context also retains up to the ten most recent successful
compaction checkpoints for the Conversation. Checkpoints are ordered from
oldest to newest, followed by the current retained tail. This gives the model
more than one historical semantic checkpoint without retaining an unbounded
sequence of summaries.

## Existing contracts retained

- `ToolResult.content` and `effect_receipts` remain distinct fields of the
  `agl-core` result type.
- Effect receipts are validated after the Tool handler runs and before the
  result is accepted.
- Oversized Tool results remain rejected as `result_too_large`; arbitrary
  slicing is not introduced.
- Durable events remain structural and do not contain Tool input or output.
- Complete messages, Tool results, operations, and compaction records remain
  queryable in SQLite.
- A Tool result is represented once in the model context.
- At most ten successful compaction checkpoint summaries are included in the
  rebuilt model context, subject to the overflow behavior selected below.
- No legacy result format, migration shim, or compatibility fallback is added.

## Context delivery behavior

For a successful Tool operation, the Store and Agent driver must preserve this
chain:

```text
Model ToolCall
      |
      v
Tool operation result metadata + result_message_id
      |
      v
one model-context Tool message containing ToolResult.content
```

The result message must be committed atomically with the operation transition.
If the link is missing, points to the wrong operation, has the wrong role, or
has different content, context reconstruction must fail with an explicit
integrity error rather than silently omitting the result.

The same rule applies to ordinary conversation continuation, operation replay,
daemon restart, and the context rebuilt after compaction. A rebuilt context
contains the selected checkpoint messages followed by the current retained
tail. Compaction may summarize an old Tool result, but it must not make a
completed Tool operation look pending or absent.

## Observability

The daemon must distinguish these stages in diagnostics:

1. Tool result accepted and stored;
2. Tool result resolved into the model context;
3. Tool result missing or inconsistent during context reconstruction.

The diagnostic must include the operation identity and Tool ID, but must not
move Tool output into durable `AgentEvent` data.

## Implementation scope

Expected changes are localized to:

- `products/services/agl-daemon/src/store/agent.rs`: result-message storage,
  replay, decoding, and context projection;
- `products/services/agl-daemon/src/store/agent/compaction.rs`: preserve the
  Tool-result fact and selected checkpoint summaries when rebuilding compacted
  context;
- `products/services/agl-daemon/src/agent/operation_driver.rs`: only where
  result delivery or validation needs an explicit invariant;
- Store, Agent, compaction, and live-smoke tests;
- `docs/components/tools.md` and `docs/components/events.md` only if the
  externally observable contract changes.

No SQLite migration is expected unless the selected human decision changes the
stored result schema.

## Acceptance tests

1. A `command.exec` result containing unique stdout and stderr is committed and
   reconstructed with identical `ToolResult.content`.
2. A model context contains the sequence `ToolCall -> ToolResult` exactly once.
3. A successful Tool result remains available after daemon restart and replay.
4. A compacted Conversation preserves enough content or summary evidence for
   every ordinary Tool result that remains in the compaction retained tail;
   completed commands are not repeated solely because of context
   reconstruction. Tool results outside that tail may be represented by the
   compaction summary and remain fully queryable in SQLite.
5. A Conversation with more than ten successful compactions restores the ten
   most recent checkpoint summaries in chronological order, followed by the
   retained tail; an older checkpoint is not included in the model context but
   remains queryable in SQLite.
6. A missing or mismatched `result_message_id` fails explicitly and never
   produces a context with a silently omitted Tool result.
7. Tool-result byte limits and `result_too_large` behavior remain unchanged.
8. Durable events still exclude Tool input and output.
9. Diagnostics distinguish stored, delivered, and missing/inconsistent Tool
   results.
10. The existing live smoke executes a command producing a unique marker and
   verifies that the model can use the marker without executing the command a
   second time.
11. Tests for `invalid_model_output` corrections remain separate from Tool
    result-delivery tests.

Tests use deterministic inference fixtures for exact context assertions; they
do not depend on a live model choosing particular prose.

## Non-goals

- Changing the public `ToolResult` type.
- Adding Tool-result content to `AgentEvent`.
- Adding a second model call to interpret or summarize every Tool result.
- Introducing a generic tool-output cache.
- Automatically retrying a Tool call when its result is absent.
- Fixing `invalid_model_output` correction behavior in this increment.
- Adding backward-compatible reads for an obsolete result format.

## Open human decisions

### D1. Canonical storage representation

Selected: **A**.

- **A — keep normalized storage (recommended):** keep content in
  `agent_messages`, keep structural metadata in `result_json`, and make every
  context/replay/compaction path resolve and validate `result_message_id`.
  This avoids duplicating potentially large Tool output.
- **B — duplicate content in `result_json`:** store content both in the
  operation metadata and the Tool message. This simplifies consumers that read
  only operation rows but creates two content copies and requires an explicit
  equality check and conflict rule.

This decision changes the durable representation and recovery behavior.

### D2. Compaction treatment of old Tool output

Selected: **A**.

- **A — retain ordinary Tool messages in the compaction tail (recommended):**
  keep the complete accepted content of each ordinary Tool-result message that
  the existing compaction algorithm places in its retained tail. This does not
  mean retaining every Tool message in the Conversation, and it does not refer
  to the internal compaction message itself. Older Tool results outside the
  tail use the existing compaction summary while remaining fully queryable in
  SQLite.
- **B — retain only a bounded summary:** allow compaction to replace old Tool
  content with a daemon-validated summary, while preserving the exact result in
  SQLite outside the model context.

This decision changes what evidence the model receives after compaction.

### D3. Number of semantic checkpoints

Selected: **A** — restore the ten most recent successful compaction checkpoint
messages in chronological order. The cap is per Conversation; older checkpoint
messages remain durable and queryable but are not included in the next model
context. A checkpoint means the internal assistant message created by a
successful compaction operation, not an ordinary Tool-result message.

### D4. Checkpoint overflow behavior

Selected: **A** — when ten summaries do not fit the model context, retain the
newest checkpoint and add older checkpoints in chronological order only while
the measured request fits. Never silently discard the current retained tail.

The rejected alternative was:

- **B — fail compaction/context rebuild:** keep the ten-checkpoint requirement
  strict and return a typed `context_exhausted` failure when the request does
  not fit.

This decision changes failure behavior under long Conversations.

## Completion gate

The human confirmed on 2026-09-08 that D1, D2, D3, and D4 are complete and
authorized implementation.
