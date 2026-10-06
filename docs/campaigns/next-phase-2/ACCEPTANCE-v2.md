# NEXT-PHASE-2: acceptance amendment v2 (frozen before any code of this cut)

```text
campaign_id  = "next-phase-2"
spec_version = 2
amends       = ACCEPTANCE.md spec_version 1 (cf34162, unchanged; this file supersedes it
               where it says so, row by row)
status       = FROZEN at the commit that adds this file. It is committed alone, before any
               code of the cut. Later changes need ACCEPTANCE-v3.md.
base         = sovereign-core main 96d97af (after #223); omega.lock 62b6a28
```

Every claim below has a source (file:line at base 96d97af unless another repo is named) or
is marked UNVERIFIED. Omega paths are at 62b6a28.

## 1. Assumptions in v1, checked against the code

| # | v1 assumption | Finding | Evidence |
|---|---------------|---------|----------|
| A1 | The live S4/S5 effect path may not run through the aien-mcp broker (v1 6, 70%) | CONFIRMED: it does not. `aien-mcp` is a dependency of no other crate; the S5 write is a plain `std::fs::write` in the CLI process, not in the daemon | `grep -rn aien-mcp crates/*/Cargo.toml` hits only `crates/aien-mcp/Cargo.toml:2`; `crates/aien-cli/src/compose.rs:294` |
| A2 | The broker ledger is in memory only (v1 6, 75%) | CONFIRMED, and irrelevant to the live path (A1). `LedgerEntry::Uncertain` and `Error::ReconciliationRequired` exist only in the broker; there is no reconcile function | `crates/aien-mcp/src/broker.rs:35,55,203,415,469`; `grep -rn "fn reconcile" crates` empty |
| A3 | Effect state survives a daemon restart | PARTLY. The S5 effect record is a Cortex host note (`kind effect`), durable (Cortex opens with `CX_OPEN_SYNC`, one `fdatasync` per append), but it is written AFTER the write. No intent exists before the write, so a crash between the write and the note leaves no trace of the effect | `compose.rs:294` (write), `:302` (receipt), `:306` (note); omega `src/runtime/rx_compose.c:871`, `src/runtime/rx_cortex.c:198` |
| A4 | Reconciliation (`Uncertain` -> `ReconciliationRequired`) is reached on the live path | NOT PRESENT on the live path (A1, A2). A second `aien compose execute` with the same authorization writes again and appends a second effect note: nothing marks an authorization as spent | `compose.rs:262-311` (no spent check) |
| A5 | `processed_operations.json` gives idempotency on the effect path | NOT CONSULTED. Compose commands are answered in `handle_connection` before `handle_control_command`, the only caller of `is_operation_processed`. The client mints a fresh `operation_id` from the clock per call, so a retried logical step never repeats an id anyway. A corrupt file is silently discarded on load (idempotency reset without a word) | `crates/aien-runtime/src/server.rs:547-585`; `spine.rs:343-351`; `crates/aien-runtime/src/client.rs:65`; `control.rs:165-175` |
| A6 | Where AienMachineId and canonical Cortex memory live | Machine root: 32 random bytes in `<compose dir>.machine-root` (created once, 0600, refused if not 32 bytes). `machine.id`, `cortex.cx`, `jspace/` inside the compose dir, created and identity-checked by omega `rxc_host_open`; the daemon opens the home lazily on the first compose command | `spine.rs:552-585`, `:875-922`, `:976`, `:1038`; omega `src/runtime/rxc_host_abi.h:86,220-224` |
| A7 | No resume command exists (v1 F4, 70%) | CONFIRMED. `ControlCommand` has `Shutdown` and no resume or stop record; nothing durable says "stopped" | `control.rs:48-91` |
| A8 | Torn tail and anchor checks protect Cortex | CONFIRMED for a cut inside a record (`E_TORN`, nothing cut) and for a boundary cut inside the J-Space-anchored prefix (`E_REPLAY`). UNVERIFIED (60% that it is NOT caught): a boundary cut that drops only host notes appended after the last anchored run, because host notes are written "between runs" and the header does not say they move the anchor | omega `rxc_host_abi.h:42-56,58-62`; `spine.rs:596-611` |
| A9 | `AIEN_GPU_BACKEND=omega` / `AIEN_REQUIRE_BLACKWELL` refuse without the GPU engine | CONFIRMED for a build without the engine linked (fatal, exit 1). A GPU-linked build that loses the device at run time goes through `strict.rs`, fatal in a production build; not exercised here | `crates/aien-cli/src/commands.rs:1286-1317,1340-1346`; `crates/aien-inference-abi/src/omega_backend.rs:9-12` |
| A10 | `AIEN_FORCE_CPU_STUB` gives a CPU-only daemon | WRONG as stated: it stubs BOTH omega crates, so the compose home is unusable (`ComposeError::Unavailable`). A daemon that never touches the GPU but keeps the real compose library is built with the compose archive linked and no GPU archive (`AIEN_OMEGA_COMPOSE_DIR` set, `AIEN_OMEGA_DIR` and `AIEN_OMEGA_GPU_LIB` unset) | `crates/aien-omega-compose/build.rs:1-17,56`; `crates/aien-omega-gpu/build.rs:1-9,37` |
| A11 | Omega caproot `RX_OP_REVOKE` is reachable on the live effect path (v1 F5b) | NOT REACHABLE. No sovereign-core code names caproot or `CapabilityRef`; the operator effect is gated by Cortex authorization records only | `grep -rn "caproot\|CapabilityRef" crates` empty |
| A12 | Omega composition fault hooks for the in-settle variant (v1 F2) | Present only in `AIEN_TEST_BUILD` omega builds; `librx_compose.a` is built without them | omega `rxc_host_abi.h:64-65` |

## 2. What this cut adds (the production behaviour the rows test)

Named here so the rows below have something exact to check. Design rules, not code.

1. **Effect intents in Cortex.** Every `write_file` effect goes through three host records, all
   `kind effect`, text JSON with a `phase` field: `intent` (durable before the write; cites the
   authorization; names target, content sha256, the target's sha256 at authorization time
   `prior_sha256`, and the executor's pid and start time), then `ack` (after the write, state
   decided by the daemon from the world, not from the executor's claim), or `reconcile` (state
   decided later). The intent is appended by the daemon in one step under the compose-home lock
   together with every check in 2.3, so two executors cannot both open an intent for one
   authorization.
2. **Effect states.** `OPEN` (intent, no ack yet), `DONE`, `NOT_DONE`, `UNRESOLVED`. The world
   check: target sha256 = content sha256 -> `DONE`; target sha256 = `prior_sha256` (absent =
   absent) -> `NOT_DONE`; anything else -> `UNRESOLVED`. An `OPEN` intent whose executor process is
   still alive (same pid and same start time in `/proc`) is never reconciled.
3. **Checks at the effect boundary** (refusal names, printed as `EFFECT_REFUSED <Name>`):
   `Stopped` (an operator stop record is the newest of stop/resume), `NotAuthorized` (the cited
   record is not a verified authorization for this exact proposal, path and content), `Revoked`
   (a revoke record names it), `Stale` (a stop record is newer than the authorization, or the
   target's current sha256 differs from the authorization's `prior_sha256`, or the authorization
   predates this format and has no `prior_sha256`), `AlreadySpent` (an intent already cites this
   authorization and is `DONE` or `NOT_DONE`), `ReconciliationRequired` (an intent already cites
   it and is `OPEN` or `UNRESOLVED`), `CorruptLedger` (a host record fails its digest or an
   effect/control record does not parse). One authorization opens at most one intent, ever.
   A retry after `NOT_DONE` needs a new authorization.
4. **Reconcile.** Runs once at daemon start when the compose home already exists (result printed
   as one `Reconcile:` line in the daemon log), and on operator request
   (`aien compose reconcile`). It records `DONE` / `NOT_DONE`, or records `UNRESOLVED` once per
   distinct target digest and keeps the effect open. Only an operator declaration
   (`--declare done|not_done --approver NAME`) closes an `UNRESOLVED` effect that the world
   cannot decide. A refused home is reported, never repaired, by this step.
5. **Operator stop and resume.** `aien compose stop --approver NAME` and
   `aien compose resume --approver NAME` append control records (`kind authorization`, text with
   `control`). The stop is durable across daemon restarts. Grants issued before a stop are stale
   forever; resume needs a new authorization for any effect.
6. **Revoke.** `aien compose revoke --authorization ID --approver NAME` appends a revoke record
   unless the authorization is already spent, in which case nothing is recorded and the answer is
   `revoked: false` (same contract as `ApprovalDesk::revoke`, `crates/aien-mcp/src/approval.rs:119-128`).
7. **Reserved records.** `ComposeNote` refuses an `effect` text with a `phase` field and an
   `authorization` text with a `control` field, so the gated records can only come from the gated
   commands.
8. **Corrupt idempotency state.** A `processed_operations.json` that exists but does not parse
   makes the daemon refuse to start with a named error, instead of the silent reset at
   `control.rs:165-175`.
9. **Fault holds** (test build only). The CLI built with the cargo feature `fault-hold` stops at
   a named point (`before_intent`, `after_intent`, `after_write`) when `AIEN_FAULT_HOLD` names it,
   creates `AIEN_FAULT_HOLD_FILE` with its pid, and continues when the harness deletes that file.
   The default build contains no fault code (checked: the string `AIEN_FAULT_HOLD` is absent from
   the default release binary).

Out of scope for this cut and therefore not claimed: moving the effect into the daemon or the
aien-mcp broker, caproot capabilities on the effect path (A11), omega in-settle crash hooks (A12).

## 3. Rules for every run (supersede v1 section 3)

- **R1 no unauthorized effect.** Every `intent` record cites an earlier verified authorization
  record whose text matches the intent; the workspace changes only at the proposal path; the
  outside sentinel is unchanged.
- **R2 no silent duplicate.** At most one `intent` per authorization; at most one `write_file`
  receipt with `success: true` per authorization; the target's `stat` (inode, size, mtime ns,
  ctime ns) is identical before and after every refused attempt.
- **R3 no false success.** A CLI line with `"ok": true` for `execute` only when its state is
  `DONE`; a `write_file` receipt with `success: true` only when the daemon recorded the ack as
  `DONE`; no receipt or record says `DONE` while the target digest differs from the content
  digest.
- **R4 identity and memory.** `machine_id` reported before the fault equals the one after;
  `machine.id` file sha256 unchanged; the prefix digest over records `1..N` (N = record count
  before the fault) is equal before and after recovery. In case C6 R4 is replaced by the
  detection rule of that case.
- **R5 unresolved stays unresolved.** An effect without a terminal record is listed `OPEN` or
  `UNRESOLVED` by `aien compose effects`, and any execute citing its authorization is refused with
  `ReconciliationRequired`, until a reconcile or operator declaration records a terminal state.
- Words not used: no receipt or document text claims exactly-once. The claim is at most one
  application, or an explicit `UNRESOLVED`.

## 4. Fixture, builds and controls (supersede v1 sections 1 and 3, last bullet)

- **Builds** (same source commit, recorded in the receipt with each binary's sha256):
  `gpu` = release build with both omega archives linked (as NEXT-PHASE-1);
  `cpu-fault` = release build with the compose archive linked, no GPU archive, cargo feature
  `fault-hold`. Every case below runs its daemon and CLI from `cpu-fault` and never touches the
  GPU. Only the fixture run uses `gpu`.
- **Fixture F0.** One NEXT-PHASE-1 style run on the `gpu` build (TinyLlama on GB10, one daemon,
  S0 provision, S1 remember, S2 inspect, S3 propose, then Shutdown), inside one quietlock hold of
  at most 20 minutes with announce and release whispers. F0 is valid only if S3 committed a
  proposal. Model quality is not scored. Each run of each case starts from a fresh byte copy of
  F0 (compose dir, machine root, workspace, S3 report) in its own run root, with its own
  `AIEN_PROVENANCE_DIR` and `AIEN_RUNTIME_STATE_DIR`. The sha256 of F0's file list is recorded.
  If no GPU window is available, F0 is NOT_RUN and so is every case that needs it.
- **Uninjected control per case.** Same copy of F0, same commands, same build, no hold armed and
  no injection: S4 authorize, S5 execute (expected `DONE`, one write), a second execute with the
  same authorization (expected `AlreadySpent`, stat unchanged), daemon Shutdown and restart
  (expected `Reconcile:` line with nothing open), recall (R4). A case is scored only if its
  control passes R1..R5 and reaches `DONE`.
- **Repetitions.** Each injected variant runs 3 times; one rule violation in any repetition fails
  the row. Controls run once per case.

## 5. The cases (supersede v1 section 4)

| Row | Injection (exact) | PASS iff (every rule in section 3 also holds) |
|-----|-------------------|-----------------------------------------------|
| C1a ack lost, world decides | Hold at `after_write`; SIGKILL the daemon; release the hold, so the executor's ack call fails; restart the daemon | executor prints `ok:false`, state `UNRESOLVED`, exit 3; its `write_file` receipt has `success:false`; the intent has no ack; the restart's `Reconcile:` records `DONE` with target digest = content digest; a retry with the same authorization is refused `AlreadySpent`; target written once (stat equal from the fault to the end) |
| C1b ack lost, world ambiguous | As C1a, and before the restart the harness overwrites the target with other bytes | restart records `UNRESOLVED` and keeps it open; retry refused `ReconciliationRequired`, stat unchanged; `aien compose reconcile --declare not_done --approver drake` records the terminal state; a retry afterwards is refused `AlreadySpent`; the harness bytes are never overwritten |
| C2a kill -9 before the durable commit | Hold at `before_intent`; SIGKILL the daemon; release | executor refuses (`ok:false`), no write, no intent record, record count unchanged; after restart the same authorization executes once to `DONE` |
| C2b kill -9 after the durable commit, executor dies | Hold at `after_intent`; SIGKILL the executor (daemon keeps running) | no write; retry refused `ReconciliationRequired`; `aien compose reconcile` records `NOT_DONE`; retry refused `AlreadySpent`; a new authorization executes once to `DONE` (approvals = 2, both counted) |
| C2c kill -9 after the durable commit, whole host | Hold at `after_intent`; SIGKILL the daemon, then the executor; restart | restart records `NOT_DONE`; no write; recall shows the intent and the reconcile record |
| C2d kill -9 after the write, executor dies | Hold at `after_write`; SIGKILL the executor | `aien compose reconcile` records `DONE`; retry refused `AlreadySpent`; written once |
| C3a GPU required, engine absent | Start the `cpu-fault` daemon with `AIEN_REQUIRE_BLACKWELL=1` on the F0 copy | daemon exits non-zero with the error naming the GB10 requirement; no socket; F0 copy byte-identical afterwards |
| C3b GPU absent, explicit CPU | Start the `cpu-fault` daemon without the requirement; run S3 propose on the F0 copy | daemon log `Backend:` names `CPU-reference`; S3 either commits or reports `committed:false` with a named reason; no output says GPU; no effect without authorization |
| C3 control | The F0 run itself | `gpu` daemon with `AIEN_REQUIRE_BLACKWELL=1` starts and names `OmegaGb10Backend` |
| C4 stop then resume | Authorize A1; `stop`; execute A1; Shutdown and restart; execute A1; `resume`; execute A1; authorize A2; execute A2 | the three A1 executes are refused `Stopped`, `Stopped`, `Stale`; zero writes until A2 (stat unchanged); A2 reaches `DONE`, written once |
| C5a revoked | Authorize A1; `revoke` A1 (`revoked:true`); execute A1 | refused `Revoked`; no write |
| C5b revoke after spend | Execute A1 to `DONE`; `revoke` A1 | `revoked:false`; no record appended (record count unchanged) |
| C5c stale world | Authorize A1; the harness writes other bytes to the target; execute A1 | refused `Stale`; harness bytes unchanged |
| C6 (each variant) damaged durable state | After a control run plus one extra authorization A2 of the same proposal (unspent), daemon stopped. Then one damage: (a) truncate `cortex.cx` inside its last record; (b) flip one byte inside the last record; (c) cut `cortex.cx` at the boundary before the last record; (d) truncate `jspace/jspace.data` by 1 byte; (e) flip one byte in `jspace/jspace.meta`; (f) truncate the machine root to 16 bytes; (g) flip one byte of `machine.id`; (h) write `{` into `processed_operations.json`. Start the daemon; run recall; run execute A2 | daemon refuses to start (h), or the start `Reconcile:` line and `recall` report a named refusal (`E_TORN`, `E_DIGEST`, `E_REPLAY`, `E_IDENTITY`, machine root length) and execute A2 is refused with no write; the damaged files' sha256 are unchanged by the daemon (no silent repair or reset); or the home opens only after an explicit `aien compose recover`, with a repair record. A variant that opens and recalls as if intact, or executes A2, FAILS |

C6 note on (c): A8 says this may FAIL on the pinned omega; the row is scored as written.

## 6. Verdicts (supersede v1 section 5)

- Row verdict: PASS, FAIL, or NOT_RUN (could not be injected or its control failed). NOT_RUN is
  never PASS.
- Declared NOT_PROVED (not rows, not counted, listed in the receipt): runtime loss of the GPU in a
  GPU-linked build; omega in-settle crash hooks (A12); caproot revoke on the effect path (A11);
  Cortex and J-Space rolled back together (omega known limit); torn writes below the file level.
- Campaign PASS = every row in section 5 PASS (C3 control included), F0 valid. Report only: wall
  ms per step, daemon start ms.
- Receipt: `gate "NEXT_PHASE_2"`, fields `commit`, `omega_compose_commit`, `builds[] {name,
  sha256}`, `fixture {sha256, s3_committed, machine_id}`, `rows[] {row, rep, injected_at,
  outcome, evidence_digests[], verdict}`, `not_proved[]`, `verdict`; named by the sha256 of its
  content, never edited, indexed in `INDEX.md`, with a `.summary.txt` beside it (NEXT-PHASE-1
  `make-receipt.sh` style).

## 7. Resolution of every v1 PROPOSED item

| v1 item | v2 resolution |
|---------|---------------|
| Clean S1..S8 baseline PASS as precondition | Replaced by the per-case control of section 4 on the effect path. S1..S8 model quality is out of scope (NEXT-PHASE-1 VERDICT-v4 already FAIL on output quality) |
| F4: 2 authorize receipts allowed only for stop-between-S4-S5 | Kept and generalised: every case that needs a second grant (C2b, C4) states it; all grants are counted |
| Campaign PASS = 6/6, 0 NOT_RUN | Section 6: all section 5 rows PASS; the NOT_PROVED list is fixed here and cannot grow without v3 |
| 3 repetitions per row | Kept for injected variants; controls once per case |
| F1 "fake sink withholds its ack" | Replaced by the real executor and a real daemon kill (C1a, C1b). No fake sink |
| F2 in-settle `RXC_FP_CORTEX` crash | NOT_PROVED (A12) |
| F3 `AIEN_FORCE_CPU_STUB=1` run | Replaced by the `cpu-fault` build (A10) |
| F4 stop during S3 | Dropped: S3 makes no external effect and needs the GPU; stop is tested at the effect boundary (C4) |
| F5b caproot revoke | NOT_PROVED (A11) |
