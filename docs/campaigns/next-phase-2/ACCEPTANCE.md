# NEXT-PHASE-2: acceptance criteria for continuity under failure (frozen before any run)

```text
campaign_id  = "next-phase-2"
spec_version = 1
status       = FROZEN at the commit that adds this file (cut 0). No harness, hook or
               reconcile code exists yet; later commits cannot change this file except by
               a dated amendment that bumps spec_version and names the rows it supersedes.
```

Thresholds marked PROPOSED were set by the builder and are not yet reviewed by Drake or the
verifier. Anything without a direct source is marked UNVERIFIED with a confidence.

## 1. Baseline this campaign extends

The NEXT-PHASE-1 workflow S1..S8 (remember, inspect, propose, authorize, execute, explain,
restart, recall) exactly as frozen in `docs/campaigns/next-phase-1/ACCEPTANCE.md` on branch
`next-phase/compose-bridge` (not yet on main; UNVERIFIED that it merges unchanged, 80%).
Pins: omega `62b6a28` (composition, Cortex, J-Space, caproot); sovereign-core = the branch head
sha recorded in each receipt at run time. NEXT-PHASE-1 receipts there are verdict FAIL at
`0503ce4` (S3 propose failed), so a clean baseline PASS is a precondition: a failure row is
only scored on a run whose uninjected control passes S1..S8 (PROPOSED).

## 2. Vocabulary reused (no third format)

| Word | Source |
|------|--------|
| step S1..S8, `steps[]`, `verdict PASS|FAIL`, `record_digest[]`, `winner_digest[]`, `machine_id` | NEXT-PHASE-1 ACCEPTANCE.md sections 1 and 4.2 |
| effect receipt: `version, tool, success, arguments_digest, result_digest, policy, timestamp` | sovereign-core `crates/aien-cli/src/tools.rs` `record_effect_receipt` |
| `processed_operations` (operation ids persisted so idempotency survives restarts), `ControlCommand::Shutdown`, `ShutdownAck` | `crates/aien-runtime/src/control.rs` |
| `LedgerEntry::{Completed, Uncertain, InFlight}`, `Error::ReconciliationRequired`, `EffectInFlight`, `IdempotencyConflict`, `CallOutcome::Uncertain` | `crates/aien-mcp/src/broker.rs`, `error.rs`, `wire.rs` |
| `ApprovalError::{Revoked, Consumed}`, `ApprovalDesk::revoke` (false once spent) | `crates/aien-mcp/src/approval.rs` |
| OLD-or-NEW recovery, `CX_K_ADMISSION` tag `RXC_ADMIT_ROLLBACK`, fault points `RXC_FP_*`, `RXC_CRASH_EXIT 77`, test hooks need `-DAIEN_TEST_BUILD=1` | omega `src/runtime/rx_compose.h` |
| `CX_ERR_TORN`, `CX_ERR_DIGEST`, `CX_ERR_FORMAT`, `CX_OPEN_REPAIR_TAIL`, `cx_verify_chain`; record-boundary cut caught by the count and head digest in the J-Space anchor | omega `src/runtime/rx_cortex.h` |
| `RX_OP_REVOKE`, generation-checked `CapabilityRef {cap_id, generation}`, stale or revoked ref fails closed | omega `src/runtime/rx_caproot.h` |
| `AIEN_GPU_BACKEND=omega` refuses to start rather than fall back; `AIEN_FORCE_CPU_STUB` (build switch) | `crates/aien-inference-abi/src/omega_backend.rs`, `crates/aien-omega-gpu/build.rs` |
| receipt named by sha256 of content, never edited, indexed | omega `evidence/COMPOSITION-2/INDEX.md` |

The word "unresolved" is not defined in aien-architecture `doctrine/ARCHITECTURE.md` (grep for
unresolved, exactly-once, idempot, duplicate found only an evidence-schema row). This campaign
maps "unresolved" onto the existing `LedgerEntry::Uncertain` / `ReconciliationRequired`; it
introduces no new state name.

## 3. Rules that hold for every row

- No unauthorized effect: count of effect receipts (tool `write_file`) lacking a matching
  `authorize` receipt for the same arguments_digest = 0.
- No silent duplicate effect: bytes written on disk and sink-observed writes for one
  idempotency key = at most 1; a second attempt returns the ledger outcome, not a second write.
- No false success: no receipt or explanation says `success: true` for an effect the sink did
  not apply, and none says applied for an `Uncertain` one.
- Identity and memory: `machine.id` digest and every Cortex record digest cited before the
  fault equal those after recovery (NEXT-PHASE-1 Identity and Memory rows).
- No universal exactly-once claim anywhere in receipts or text. At-most-once-or-unresolved only.
- Every injection runs against a fresh compose dir and a fresh `AIEN_PROVENANCE_DIR`.

## 4. The six failure cases

| # | Injection (mechanism) | Required outcome | Evidence to produce | PASS iff |
|---|-----------------------|------------------|---------------------|----------|
| F1 | Effect applied, ack lost. A campaign fake effect sink applies the S5 write then withholds its ack (modelled as `CallOutcome::Uncertain`), and the harness SIGKILLs the daemon right after the sink applies. Restart. | Ledger row is `Uncertain`; retry returns `ReconciliationRequired`, never a second write; no `success: true` receipt; a reconcile step later compares disk sha256 to the proposal digest and sets the effect to Completed or records it as not applied | sink log (apply count), `write_file` receipt (absent or `success:false`), ledger row before and after reconcile, disk sha256 vs S3/S4 digest, retry error name | apply count = 1, retry refused with `ReconciliationRequired`, reconcile ends in a recorded terminal state, rules in section 3 hold |
| F2 | Daemon SIGKILL at a named step boundary: after S3 (CX_K_PROMOTION written) and again after S5 (before S6). Test build with `RXC_FP_CORTEX` crash (exit 77) for the in-settle variant. | OLD-or-NEW: state is the last durable record; no half state; restart does not redo S5 | step name killed at, Cortex record count and head digest before and after, `CX_K_ADMISSION` rollback record if the newest record's branch was not sealed, receipts count | after restart exactly one of OLD or NEW, committed identity and memory preserved, S5 write count <= 1 |
| F3 | GPU unavailable. Run with the GPU library absent (`AIEN_OMEGA_GPU_LIB` unset or unusable) and `AIEN_GPU_BACKEND=omega`, and separately a build with `AIEN_FORCE_CPU_STUB=1`. | Omega-demanded run refuses to start (named error), no silent fallback; the explicit CPU run completes or fails on its own merits with `gpu_used:false` and the backend name recorded | process exit and error text, receipt `backend`, `gpu_used`, S1..S8 results | no run reports `gpu_used:true` without the GPU; refusal or CPU path is explicit; no effect without authorization |
| F4 | Operator stop and resume. Send `ControlCommand::Shutdown` (expect `ShutdownAck`) during S3 and again between S4 and S5, restart, then resume. There is no resume command in `control.rs` (UNVERIFIED that none exists elsewhere, 70%), so resume means a fresh operator authorization of the same proposal digest. | Stopped work does not apply an effect while stopped; after restart the old grant is not silently reused; the effect applies only after a new authorization; operation ids in `processed_operations` are not replayed | `ShutdownAck`, processed_operations before and after, count of `authorize` receipts (1 per approved effect, PROPOSED: 2 allowed only for the stop-between-S4-S5 variant), disk state during the stop | zero writes while stopped, resumed write authorized, no duplicate |
| F5 | Capability revoked mid-flight. (a) `ApprovalDesk::revoke` on the S4 grant before S5 spends it; (b) caproot `RX_OP_REVOKE` (generation bump) of a candidate or world capability between propose and commit. | (a) spend refused with `ApprovalError::Revoked`, no write; (b) stale `CapabilityRef` fails closed, commit rejected, branch released; a grant already spent is not revoked (`revoke` returns false) and the case is then F1, not F5 | error name, revoke return value, caproot generation before and after, effect receipt (`success:false` or none), disk unchanged | no write after revoke, error names match, loser or rejected branch left no file (NEXT-PHASE-1 speculation row) |
| F6 | Durable store damage, applied to a copy while the daemon is stopped: (a) truncate `cortex.cx` mid-record; (b) flip one byte inside a record; (c) cut `cortex.cx` exactly at a record boundary; (d) truncate and (e) flip a byte in `jspace/jspace.data` or `jspace.meta`. | (a) open reports `CX_ERR_TORN`, repair only with explicit `CX_OPEN_REPAIR_TAIL`; (b) `CX_ERR_DIGEST` or `CX_ERR_FORMAT`; (c) caught by the anchor count and head digest cross-check; (d,e) refuse or recover the previous sealed state with a `RXC_ADMIT_ROLLBACK` record. Never open as if intact and never report recall success on damaged data. Both stores rolled back together is a documented KNOWN LIMIT of omega, scored as NOT_PROVED, not PASS. | sha256 of each file before and after damage, open error code, post-recovery record count and head digest, recall result | each of a..e detected or recovered to a verified prior state; recall returns only verified records |

## 5. Per-row verdict and campaign verdict

- A row is PASS only if its PASS clause and every rule in section 3 hold, on the run whose
  uninjected control passed. Otherwise FAIL. A row that cannot be injected is NOT_RUN, never PASS.
- Campaign PASS = 6 of 6 rows PASS, 0 NOT_RUN (PROPOSED). Report-only: wall time per step and per
  recovery (ms), `VmHWM` before and after restart.
- Each row is repeated 3 times with the fault at the same named boundary (PROPOSED); any single
  run violating a section 3 rule fails the row.
- Receipt: omega gate receipt style, `gate "NEXT_PHASE_2"`, with `rows[] {row, injected_at,
  outcome, evidence_digests[], verdict}` added to the NEXT-PHASE-1 fields; named by its sha256.

## 6. Known gaps found while writing this (they make FAIL likely, honestly)

- `aien-mcp` ledger is an in-memory `HashMap` (`broker.rs`); persistence across a daemon restart
  and any reconcile API are not present in the code read (grep `reconcile` finds only the error
  name and one test). UNVERIFIED that no persistent store exists elsewhere, 75%. F1 and F4 need
  cut 1 to add them; this file does not.
- `aien-mcp` is not a dependency of `aien-runtime` or `aien-cli` Cargo.toml (grep empty), so
  the S4/S5 path in NEXT-PHASE-1 may not run through the broker at all. UNVERIFIED, 70%.
- Omega composition fault hooks exist only in `AIEN_TEST_BUILD` builds (rx_compose.h), so F2's
  in-settle variant cannot run on the production program.

## 7. What this campaign does not prove

- Not exactly-once effects. At most one apply or an explicit unresolved state, nothing more.
- Not tolerance of failures outside the six rows: power loss with torn disk writes below the
  file level, disk full, clock jumps, network partitions, multiple simultaneous faults.
- Not recovery when Cortex and the J-Space checkpoint roll back together (omega KNOWN LIMIT).
- Not security against a malicious operator or an attacker with write access to the stores.
- Not GPU hardware qualification, not performance, not model answer quality.
- Not external-sink correctness: the sink is a fake, real tools may fail differently.
