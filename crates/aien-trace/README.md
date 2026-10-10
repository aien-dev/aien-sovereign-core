# aien-trace

Correlation-only execution trace for AIEN (aien-sovereign-core#398).

## What it is

Typed events (`TraceEvent`) that say "this boundary was crossed" and carry the
ids needed to join a swarm, its sequences, model turns, tool decisions, effect
results and Interplane messages into one run. It is observational. It is not
evidence and it is not authorization: the effect receipt ledger, the compose
ledger, the generation record and the independent receipt verifier keep
governing. Events point at those artifacts by digest or relative path.
A trace id is a correlation key, never a secret and never a proof.

## Default

`NullSink` discards everything and is the default. Tracing is off until a
caller installs another sink. The crate starts no thread unless
`BoundedJsonlSink::spawn_flusher` is called.

## Cardinality

One event per boundary crossing: swarm launch, branch fork, sequence
admit/preempt/finish, model turn, tool request, authority decision, effect
execution, result, cancel, timeout. Bounded by sequences times stages. There
are no per-token or per-kernel events, so GPU hot paths are untouched.

## Buffering and backpressure

`BoundedJsonlSink` holds a bounded queue (default 4096 events). `emit` takes a
mutex only for one push and does no I/O. When the queue is full the event is
dropped and counted. The next `flush` appends one `trace_dropped` event whose
note says how many were dropped. `dropped_total()` is the lifetime count.
Security-critical records live in the ledgers, not here, so dropping a trace
event never loses evidence.

## Retention

Append-only JSONL, one object per line, created with mode 0600 on unix. The
operator rotates or deletes files. The crate deletes nothing.

## What is never exported

No prompts, tool arguments, model output, memory items or secrets. Enforced by
type: the only free text is `BoundedNote` (64 bytes, control characters and
`<` `>` removed). Ids are `BoundedId` (64 characters, small alphabet), digests
are hex, paths are relative without `..`. Deserialization applies the same
rules, so edited files cannot widen a field.

## Rebuilding one run

Read the JSONL lines into `Vec<TraceEvent>` and call `reconstruct(&events)`.
It checks one trace id, unique event ids and a valid parent for every child,
then gives `Tree::children(id)` and `Tree::chronological()`. Statuses map to
the AEGIS failure vocabulary plus `ok`; `uncertain` means the request may have
reached the server and is never retried automatically. To verify anything,
follow the digest or path to the independent artifact.
