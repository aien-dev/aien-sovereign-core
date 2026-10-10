# Inventory for #398: ids and recording sites a correlation trace can join on

Read from main at the branch point. Line numbers are from that checkout. Roles:
AUTH = authorization (decides whether something may run), EVID = evidence
(records what happened), NEITHER = bookkeeping or an observation tap.
A trace event is NEITHER by design: it points at AUTH and EVID, never replaces them.

## Scheduler (`crates/aien-scheduler`)
| Site | What | Role |
|---|---|---|
| `src/sequence.rs:11` `SequenceId {slot: u32, generation: u32}`; packed u64 via `from_u64` / `Into<u64>` (`:50`, `:56`) | Per-sequence id, slot plus generation. Layout in doc comment at `:8`. | NEITHER |
| `src/lib.rs:82` `step_id` field, set `:99`, stamped into the batch `:399`, incremented `:433` and `:961` | Scheduler step counter, local to one scheduler instance. Not unique across restarts. | NEITHER |
| `src/lib.rs:61` `SchedulerMetrics` (counters only; `admitted_requests` bumped `:229`, `:256`; `preempted_requests` `:499`, `:1104`; `finished_requests` `:1108`, `:1154`; `total_steps` `:1181`) | Aggregates. No per-sequence or per-swarm key, so it cannot be joined on. | NEITHER |
| `src/lib.rs:157` `submit_request` returns `SequenceId`; `:261` `fork_sequence`; `:286` `fork_subagent` | Admission entry points. Natural place for `sequence_admitted`. | NEITHER |
| `src/lib.rs:108` `set_decision_observer`; call site `:424` | Read-only tap, once per `build_scheduled_batch`, `None` by default. | NEITHER |
| `src/dual_observer.rs:239` `DecisionObserver::observe_admit_preempt(&AdmitPreemptRecord)`; record `:215` carries `step_id` and, via `AdmissionCandidate.seq` (`:150`), the `SequenceId` of admission candidates | Decision record of the DUAL experiment. Observation only; return type is `()`. | EVID for scheduler choices (DUAL), not for tool authority |

## Swarm (`crates/aien-runtime/src/swarm.rs`)
| Site | What | Role |
|---|---|---|
| `:80` `swarm_id` from `next_swarm_id` (starts 1, `:65`), returned by `launch_swarm` (`:71`) | Per-process counter. Resets on daemon restart (UNVERIFIED whether it is ever persisted; no persistence seen in this file). | NEITHER |
| `:31` `SwarmRecord {id, root_sequence_id, branch_sequences, branch_worlds, finished_branches, state}`; `SwarmState` `:11` | Swarm to sequence to world mapping. | NEITHER |
| `:84`, `:116`, `:117` root world id, `fork_world`, `arena.fork` | World ids and branch `SequenceId`s are created together at fork. Branch KV is forked later by `fork_branches_from_ready_root` (`:172`). | NEITHER |
| `:221` `cancel_swarm`; `:274` `note_sequence_finished` | Cancel and natural completion. Both drop branch worlds. No event is recorded today. | NEITHER |
| `src/spine.rs:309` `AienRuntimeSpine::launch_swarm`; `:193`-`:210` finish path: `SequenceId::from_u64(request_id)` then `note_sequence_finished` | The `request_id` of a finished stream IS the packed sequence id here. | NEITHER |
| `src/client.rs:108` `cancel_swarm` sends `ControlCommand::CancelSwarm(swarm_id)` | Control entry for cancel. | NEITHER |

## Generation record (`crates/aien-runtime/src/generation.rs`)
| Site | What | Role |
|---|---|---|
| `:12` `GENERATION` marker; `:125` `build_record` | Daemon-authored note in the compose ledger: model and tokenizer sha256, prompt/output token-id hashes, finish reason, daemon pid/start, `request_id` and `operation_id`. Doc (`:1`-`:8`): "EVIDENCE ONLY", no grant, intent or effect decision reads it. | EVID |
| `:41` `request_id`, `:42` `operation_id` | "Caller-asserted: the client chooses the envelope ids; recorded, not trusted" (`:139`). Do not treat as proof of identity. | EVID (untrusted field) |
| `:168` `PROVENANCE`, `:180` `Provenance {generation_record: Option<u64>, allen_agent, stale_generation, note}` | Link from a commit/grant/intent/ack to the generation record id (a compose ledger record id). Reserved key; no ledger reader parses it for decisions (`:171`). | EVID |
| `src/spine.rs:2651` `write_task_generation`, used `:2579`; `Provenance::new` `:2580` | Where the generation record id is minted for a run. | EVID |

## MCP broker (`crates/aien-mcp`)
| Site | What | Role |
|---|---|---|
| `src/wire.rs:8` `CallOutcome {Finished(Value), Rejected(String), Uncertain}` | Provider answer. `Uncertain` means the request may have reached the server and is never resent. | EVID (provider side) |
| `src/effect.rs:9` `EffectIntent {provider, tool_name, arguments, capability_digest}` | What is to be run. Carries arguments, so a trace must not copy it. | NEITHER |
| `src/authority.rs:149` `intent_digest(&EffectIntent) -> Digest32` | Digest naming one intent. Join key to approval and ledger. | NEITHER (name only) |
| `src/authority.rs:25` `AuthorityDecision {Allow, AllowRestricted, RequireApproval, Deny, Contain}`; `:133` `AuthorityOutcome {Pending, Approval, Denied, Contained, Execution}` | Policy verdict and the reasons nothing ran. | AUTH |
| `src/broker.rs:293` `EffectLane::authorize`; `:308` `authorize_approved`; `:344` policy digest computed | Mints `AuthorizedEffect`; the only production mint. | AUTH |
| `src/effect.rs:64` `AuthorizedEffect<T>` (private fields; `approval_id()` `:76`, `world_id()` `:99`, `winning_jnode()` `:103`, `policy_digest()` `:107`, `idempotency_key()` `:115`) | Capability token in the type system. Cannot be built from a trace. | AUTH |
| `src/approval.rs:105` `ApprovalDesk::issue`; grant id = `Digest32::of(counter ++ intent_digest ++ idempotency_key)` (`:112`-`:118`); `ApprovalGrant::id()` `:86` is crate-private | Approval grant id (32 bytes). Counter is per broker. | AUTH |
| `src/broker.rs:32` `LedgerEntry {InFlight, Completed(EffectReceipt), Uncertain}`; row keyed by `EffectId`, bound to `EffectIdentity {intent digest, world_id, winning_jnode}` (`:43`) | In-memory idempotency ledger. | EVID (in memory only, UNVERIFIED whether persisted elsewhere) |
| `src/broker.rs:485` execute; receipt built `:544` | `EffectReceipt {effect_id, world_id, winning_jnode, policy_digest, provider, tool_name, capability_digest, output}` (`effect.rs:25`). `Finished` stores Completed, `Uncertain` or transport error stores Uncertain and returns `ReconciliationRequired`, `Rejected` removes the row. | EVID |
| No `EffectReceipt` digest function seen in `effect.rs` or `broker.rs`. Receipt digest for a trace is UNVERIFIED: must be defined by the instrumenter (for example over a canonical encoding) or taken from the Interplane receipt verifier. | | |

## Compose daemon (`crates/aien-runtime/src/spine.rs`, `effects.rs`)
All ids below are compose ledger record ids (u64, one append-only journal), not hashes.
| Stage | Site | Role |
|---|---|---|
| Proposal and commit | `spine.rs:2438` `run_task_inner`; model run; commit record `effects.rs:1277` `write_compose_commit`, called `spine.rs:2581`, stamped with `Provenance` | EVID (commit claim) |
| Mint grant | `effects.rs:1335` `authorize` (`ComposeAuthorize`), `:1326` `mint_grant`; also `write_approved_grant` `:1182` for approved compose | AUTH |
| Open intent | `effects.rs:1592` `open_intent`; text has `phase: "intent"`, `authorization: <grant id>` (`:1626`) | AUTH check, then EVID of the intent record |
| Execute and ack | `effects.rs:1641` `ack`; phase `ack` (`:14`); links `[intent, authorization]` | EVID (executor report plus world state) |
| Record mark | `spine.rs:2058` `advance_mark`; called after commit `:2598`, and on `:2364`, `:2546`, `:2714`, `:2972`; types in `cortex_mark.rs:123` `MarkRefusal` | EVID (tamper evidence for the journal) |
| Reconcile | `effects.rs:1673` `reconcile`, phase `reconcile` (`:15`), `:1858` `reconcile_at_start`; gate `:705` `reconcile_gate`; `spine.rs:2385` `set_reconcile_failed` | EVID, and a gate that refuses effect commands |
| Effect states | `effects.rs:52` `EffectState {Open, Done, NotDone, Unresolved}` | EVID |

Refusal names (`effects.rs:85`, text `EFFECT_REFUSED <name>: detail`), verified by grep of
`Refusal::new` call sites: Stopped, NotAuthorized, AlreadySpent, ReconciliationRequired,
Revoked, Stale, ReconcileFailed, NotRegular, MissingParent, CorruptLedger, Unreadable,
OutsideWorkspace, DeskMacRequired, Replayed, AlreadyAuthorized. Mark errors (`spine.rs`):
E_MARK, E_MARK_TRUNCATED, E_MARK_DIGEST, E_MARK_WRITE; also E_REPLAY (`spine.rs:2334`).
All are AUTH outcomes (a refusal), mapped by a trace to a status, never reinterpreted.

## Join keys
| Link | Key | Notes |
|---|---|---|
| swarm to branch/root | `swarm_id` (u64), `SequenceId` as u64 | `SwarmRecord` `swarm.rs:31` |
| sequence to world | `world_id` (u64) | fork time, `swarm.rs:116` |
| sequence to scheduler step | `step_id` (u64) | `AdmitPreemptRecord.step_id`; per scheduler instance |
| sequence to stream/generation | `request_id` equals packed `SequenceId` on the finish path (`spine.rs:193`) | generation `request_id` is caller-asserted |
| model turn to commit/grant/intent/ack | `generation_record` (ledger id) via `Provenance` | evidence only |
| proposal to grant to intent to ack | ledger record ids: `authorization` field, `links` | `effects.rs:1626` |
| tool request to approval to receipt | `intent_digest`, approval grant id (`approval_id()`), `EffectId` idempotency key, `world_id`, `winning_jnode` | `broker.rs:43` |
| AIEN to Interplane | UNVERIFIED: no Interplane id exists in this repo yet; coordinate with interplane#97 | trace id is minted here and handed over |
