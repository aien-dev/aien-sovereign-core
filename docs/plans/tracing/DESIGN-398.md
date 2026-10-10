# Unified execution tracing (aien-sovereign-core#398): design and limits

Owner: campaign session 1ea888, 2026-10-10. Branch `feat/unified-execution-tracing-398`. Inventory of the
identifiers this joins on: `INVENTORY-398.md` (same folder). Crate: `crates/aien-trace` (serde and serde_json only).

## What a trace is, and is not

A trace is a correlation record: one event per boundary crossing of one run, each with a parent, joined by
stable ids that already exist in the runtime. It is observational. It never authorizes anything (the
`AuthorizedEffect` mint and the compose grant stay where they are), and it is never proof: proof is the
compose ledger with its record marks, the effect receipts, the generation record and the independent
verifier (`tools/aien-verify`). Every event points at proof by digest or relative path and carries no
prompt, tool argument, model output, file path or memory item; the type system enforces that (the only free
text is a 64 byte note with `<`, `>` and control characters removed, and the runtime writes only fixed labels
and refusal names into it).

## Field mapping (contracts C1 and C2 in `~/workspace/six-issues/COORDINATION.md`)

| Event field | Source | Note |
|---|---|---|
| `trace_id` | swarm: `TraceId::derive(root_world_id, swarm_id, 0)` per swarm; compose daemon: one per `ComposeBridge` (derived from start time and pid); when a request came through Interplane the Interplane `trace_id` (32 lowercase hex) is copied into `ids.interplane_trace_id` | C1: Interplane's id is the cross-boundary join key; AIEN never re-mints it |
| `ids.interplane_message_id`, `ids.interplane_request_id` | Interplane envelope and CapabilityRequest ids | C1, bounded to 64 chars |
| `status` | one to one with the AEGIS `FailureClass` plus `ok` | C2: `rejected` also covers Interplane denied, not_found, invalid; `approval_pending` is terminal for the attempt and not a failure |
| `effect_certainty` | `no_effect`, `uncertain`, `effect_occurred` | C2 `EffectCertainty`; `uncertain` is aien-mcp `CallOutcome::Uncertain` or a compose intent settled UNRESOLVED, and is never retried by anything that reads a trace |
| `refs.intent_digest` | `aien_mcp::authority::intent_digest` | names the intent, never its arguments |
| `refs.effect_receipt_digest` | `aien_mcp::receipt_digest` (canonical encoding with sorted keys, sha256) | the receipt itself stays in the ledger |
| `ids.generation_record` | compose ledger record id from `write_task_generation` | evidence pointer |
| `ids.decision_id` | compose: the cx promotion (authorize) or the commit record id (model turn) | what was decided about |
| `ids.grant_id` | compose grant record id, or the broker approval id hex | authorization pointer, not authority |
| `ids.tool_request_id` | compose effect intent record id, or the broker tool request id | one effect attempt |

## Where events are emitted

| Stage | Site | Event, status |
|---|---|---|
| swarm launch | `SwarmManager::launch_swarm` | `swarm_launched` ok (root event of the swarm trace) |
| branch fork | `SwarmManager` where branch worlds and sequences are created | `branch_forked` ok, parent root |
| admission, preemption | `TraceDecisionObserver` (aien-runtime `trace_observer.rs`) on the scheduler's existing `DecisionObserver` tap; admitted from `admission_examined` verdicts `AdmittedPrefill`/`AdmittedDecode`, preempted from `choice.preempted` | `sequence_admitted`, `sequence_preempted` |
| branch finished | `note_sequence_finished` | `sequence_finished` ok |
| cancel | `cancel_swarm` | `swarm_cancelled` cancelled |
| broker authority | `EffectLane::authorize` (aien-mcp) | `authority_decided` ok / approval_pending / rejected, note = reason label (`provider_not_admitted`, `refused`, ...) |
| broker effect | `EffectLane::execute` (Finished, Rejected, Uncertain, ledger replay, in-flight duplicate) | `effect_executed` ok+effect_occurred / rejected+no_effect / uncertain / `in_flight`; child of the lane's parent event (an `AuthorizedEffect` carries no event id) |
| compose model turn | `ComposeBridge::run_task_inner` after the commit record is written | `model_turn` ok with `generation_record` and `decision_id` = commit id, note `compose_commit` or `uncommitted`; proposer error: `model_turn` failed, note `proposer_error` |
| compose authority | `effects::authorize` | `authority_decided` ok (`grant_id` = grant record) / rejected+no_effect (note = refusal name) / failed |
| compose intent | `effects::open_intent` | `tool_requested` ok (`tool_request_id` = intent record, `grant_id`) / rejected (refusal name) / failed |
| compose ack | `effects::ack` | `effect_executed` ok+effect_occurred (DONE) / failed+no_effect (NOT_DONE) / uncertain+uncertain (UNRESOLVED) / rejected / failed |
| compose reconcile | `effects::reconcile` (also `reconcile_at_start`) | `result_received` ok (note = caller label) or failed (`reconcile_failed`), one per call, `tool_request_id` when one intent was declared |

The compose bridge's first event is the root of its trace and every later event is its child (the bridge
has no per-task context today; the ids join the rows). The scheduler crate is not modified. No event is
emitted from inside a kernel launch, a KV block operation or a decode step.

## Exporter, cardinality, buffering, retention

- Default sink is `NullSink` (disabled): `enabled()` is false, so no event is built, no clock is read, nothing
  is allocated. The daemon installs `BoundedJsonlSink` only when `AIEN_TRACE_JSONL` names a file
  (`server.rs: trace_sink_from_env`; file created 0o600, append only, one JSON object per line); a flusher
  thread writes every 250 ms and once more when the handle drops at the end of `run`, after the compose home
  closed.
- Cardinality: at most one event per boundary crossing, so per run about `2 + 2 x branches + 3 x tool
  calls + reconcile rows`. There is no per-token or per-step event.
- Buffering and backpressure: a bounded in-memory queue (default 4096 events) under a mutex held only for
  the push; `emit` never blocks and never does I/O. When the queue is full the event is dropped and counted;
  the next flush appends one `trace_dropped` event with the count. `dropped_total()` is the lifetime count.
- Retention: the file is append only; the crate never deletes or rotates; the operator rotates by moving the
  file between daemon runs.
- Failure independence: if the sink is disabled, full, or the file is unwritable, inference and the compose
  path are unaffected; security-critical evidence is written by the ledger, not by the trace.

## Tamper statement

Editing, deleting or forging exported events changes nothing the runtime decides: `execute` needs an
`AuthorizedEffect`, which only `EffectLane::authorize` mints; the compose daemon executes only against a
grant in its ledger under a record mark; `tools/aien-verify` reads the ledger and receipts, not traces. A
forged `authority_decided ok` event is just a line in a file. The provenance guard test
(`provenance_is_read_by_no_decision_code`) still holds: the trace wrappers read the response, never the
provenance fields.

## Overhead (measured; numbers in the PR body)

`crates/aien-runtime/tests/trace_overhead_benchmark.rs` runs the same 64-branch swarm workload on the
reference CPU backend with `NullSink` and with `MemorySink` and prints elapsed milliseconds
(`TRACE_OVERHEAD kind=... elapsed_ms=...`). The PR body records release-build numbers with host, commit and
load at the time. The LINEAR and BRANCH model-serving cases of the issue are not measured here: the swarm
workload has no model forward pass, so the measured overhead is that of the scheduling and event path only.

## Limits

- Interplane ids arrive only when interplane#97 lands its adapter; until then `interplane_*` are empty.
- The scheduler observer emits only what `AdmitPreemptRecord` carries.
- Trace ids are correlation keys derived from counters and start times, not secrets and not unique across
  machines.
- The compose bridge has one root per daemon start, not one per task; join tasks by `generation_record`,
  `decision_id`, `grant_id` and `tool_request_id`.
- Broker `effect_executed` events are children of the lane's parent, not of the matching
  `authority_decided` (the `AuthorizedEffect` type is frozen and carries no event id).
- Tie order of events in the same millisecond is by `event_id`; not stressed with many concurrent emitters.
- OTEL export is out of scope (a separate concern per the issue).
