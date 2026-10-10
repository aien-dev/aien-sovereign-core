# REGRESSION sc386: compose memory is recalled only when ALLEN is engaged

Issue: aien-sovereign-core#386. Program: aien-architecture#190, lane L3 (decision by the orchestrator for release v1: memory stays attached to an engaged ALLEN identity through the existing aien-allen-memory path; no new store, no new subsystem).

## Failing behaviour as observed (run folder RUN-1-dry-20261010T134202Z, harness v1.1, CPU reference backend)

1. `artifacts/E4-remember.json`: `aien-cli compose remember --text ...` returned `ok: true` and stored a `constraint` record on a daemon with no ALLEN identity. `artifacts/E4-propose.json` then reports `memory.state = not_engaged`, `items_included = 0`: the item can never be used. Also, even when ALLEN is engaged, `remember` writes only a compose constraint record, which the proposal never reads (the proposal reads ALLEN scoped memory).
2. `artifacts/E4-propose.json`, three attempts: prompt 330 to 373 tokens, cut at 48 generated tokens (`finish_reason max_tokens`), every reply re-emitted the whole report instead of one appended line. The small-edit path uses `AIEN_COMPOSE_MAX_TOKENS`, default 48 (`crates/aien-runtime/src/server.rs`, `model_proposer`).

## Repairs (pre-registered)

1. `compose remember` writes the item into ALLEN scoped memory (context `work` unless `--context` names another) and still writes the constraint record. When ALLEN is not engaged it refuses with the existing NotEngaged refusal text ("ALLEN is not engaged on this machine ...") and stores nothing. `compose recall` adds a `memory` section: `state not_engaged` with that text when not engaged, otherwise the items of the context.
2. Harness `scripts/whole_system_e2e.sh` engages a host-built fixture ALLEN subject for every daemon start from PRE onward (not CTRL-E3a), as `scripts/allen_e2e_demo.sh` step S1 does, and records the identity fingerprint in the PRE and E4 receipts.
3. Append objective: option (b), chosen over (a). Option (a) changes the edit prompt, the proposal parser and the requirement verifier, which is not small. The harness sets `AIEN_COMPOSE_MAX_TOKENS` for the E4 daemon start only and records the value in the E4 receipt.

## Tests

| Test | On current main | After the fix |
|---|---|---|
| `crates/aien-cli/tests/compose_memory_recall_test.rs::remember_refuses_when_allen_not_engaged` (ignored, compose-linked CLI) | FAIL (remember returns ok true) | PASS |
| same file `::remember_then_recall_includes_item_when_engaged` | FAIL (recall has no memory section, item not in ALLEN memory) | PASS |
| append path: `bash scripts/whole_system_e2e.sh selftest` (checks the E4 token budget is a documented value inside the accepted range and is passed to the E4 daemon start only) | FAIL (no such constant) | PASS |
| harness selftest, ALLEN env lines (`allen_env` prints exactly `AIEN_ALLEN_SUBJECT=<path>` and, once, `AIEN_ALLEN_ADOPT=<agent>`) | FAIL (no such function) | PASS |

The full harness `run` mode is not run by this lane (16 minutes on CPU); the orchestrator runs declared runs.

## Must not change

Desk, grants, effect boundary, every refusal other than the new `remember` refusal, anything under `runs/`, the two ACCEPTANCE files and the result notes. CTRL-E3a (no desk key) starts without a subject, unchanged.

## Notes added with the fix

- The harness engages the subject the way demo S1 does: a plain start opens the compose home, the subject is built bound to record 1 of that home (`cargo test -p aien-allen --test e2e_demo_subject -- --ignored`), the daemon is SIGKILLed and restarted with `AIEN_ALLEN_SUBJECT=<path>` and, once, `AIEN_ALLEN_ADOPT=<agent>`. The daemon log must show `ALLEN: engaged` and `aien-cli allen status` must give a fingerprint, or the start counts as failed.
- CTRL-E4 removes the store; the home is then refused at its integrity mark before ALLEN can resolve (observed by hand: no `ALLEN: engaged` line, `allen status` returns the mark refusal), so that one start does not require engagement. The old subject is still passed.
- Append objective: option (b), `AIEN_COMPOSE_MAX_TOKENS=400` on the E4 start only, recorded as `compose_max_tokens` in the E4 receipt. Whether a 1B model then produces exactly one added line is measured by the next declared run, not claimed here.
