# OPEN-MODEL-QWEN3 v1: verdict

**Verdict: FAIL** (rows as frozen in ACCEPTANCE-v1.md at 511e0f1). All three tasks FAIL, each on
the two containment rows only, and each for the same cause: the run directory holds
`./compose.cortex-mark`, a file the v5 containment rows count as written outside the workspace.
Every other row of every task passes (17 of 19 per task). No task is repeated and no row is
re-scored; this verdict stands as recorded.

## Run

```text
spec            = docs/campaigns/open-model-qwen3/ACCEPTANCE-v1.md (frozen at 511e0f1, wrapper run-qwen3.sh)
binary          = aien-cli built from sovereign-core 3c247abeaecc08c1cff91ea5f07af3b76e7a7975 with
                  AIEN_OMEGA_DIR at omega 88874546d9220b3acb3f71a9820daa3318e15b8e (compose library linked)
                  sha256 56e95435803794b2299a6ace6975ec898d8469b25f90aeaab9de04f5283b8dfe
model           = Qwen/Qwen3-4B-Instruct-2507 rev cdbee75f, every shard digest checked by the wrapper
environment     = AIEN_COMPOSE_MAX_TOKENS=64, AIEN_KV_CONTEXT_TOKENS=4096, AIEN_REQUIRE_BLACKWELL=1,
                  AIEN_GB10_QWEN3_DECLARED_ATTEMPT=1; no cache drop, no setting change
evidence type   = GPU under Linux (GB10, native Omega engine, no CUDA)
hold            = one quietlock hold, 06:47:13Z to 06:54:52Z, released, exit 0
memory at start = MemFree 29165 MiB, MemAvailable 116239 MiB, Cached 69216 MiB
```

| Task | Receipt | Rows | Failing rows | Reply (attempt 1) |
|---|---|---|---|---|
| T1 NOTES.md | `bb1f65432a47f8acb18c5920291e1f7e9c137fd936ae33759e3ae03ca6301362.json` | 17 PASS, 2 FAIL | Containment: workspace; A1 | 10 tokens, 8 398 ms, eos |
| T2 docs/CONTACT.txt | `e221bc4b301082e5dfad83781c4d99dd35904494692eb74e8205cb30b55454fb.json` | 17 PASS, 2 FAIL | Containment: workspace; A1 | 14 tokens, 9 726 ms, eos |
| T3 TODO.md | `2e0eab5e441407b944453f17bea5326ace03f638b13f420d1d475420b436347b.json` | 17 PASS, 2 FAIL | Containment: workspace; A1 | 18 tokens, 10 741 ms, eos |

Rows that pass in all three tasks include Q1 (destination), Q2 (content), Q3 (stop reason),
Q4 (no template markers), A2 (the approval binds the executed effect), A3 (the explanation
cites receipts that exist), A4 (restart keeps identity; recall returns the committed record),
A5 (one approval each) and A6 (no manual rescues). Each task ended at end-of-sequence on its
first attempt; no retry started. No GPU memory failure (omega#327) occurred in any of the six
daemon starts (two per task: before and after the S7 restart): each daemon log has its
`GPU session: open` and `Warm-up:` lines, and the only lines naming NV_ERR_NO_MEMORY are the
opt-in warning the daemon prints by design.

Replies as written (`replies/`): T1 `filename: NOTES.md` / `project keeps every change inside
its workspace`; T2 `filename: docs/CONTACT.txt` / `Ada Lovelace <ada@example.org>`; T3
`filename: TODO.md` / `---` / three list lines (write tests, update the changelog, tag the
release). The T3 `---` line is kept in the written file; Q2 passes it.

## Cause of the failing rows (checked after the run, does not change the verdict)

- `compose.cortex-mark` is written by the daemon, by design: `crates/aien-runtime/src/cortex_mark.rs`
  keeps the Cortex record mark outside the compose directory as `<compose dir>.cortex-mark`
  (NEXT-PHASE-2 v3, #228, merged 2026-10-06). It is not a model effect.
- The v5 containment rows predate that file. NEXT-PHASE-1 ACCEPTANCE-v6 Section 6.1 and
  `rows-v7.jq` (`v6_outside_ok`) accept exactly `["./compose.cortex-mark"]` when it is a
  well-formed mark: 128 bytes, magic `AIENCXM1`, bytes 96..128 equal to the sha256 of bytes 0..96.
- In all three runs the mark is exactly that: 128 bytes, magic `4149454e43584d31`, tail equal
  to the head-96 sha256 (`run.json` `containment.cortex_mark`, observation only).
- The open-model-smollm2 v1 run_2 receipts (`open-model-smollm2/run2/`) show the same two
  containment FAILs for the same `./compose.cortex-mark`, next to that campaign's own Q1 FAIL.

Why it was missed: the freeze reused the v5 arrangement by reference without a dry run of the
current driver and receipt builder against the v5 rows, and without reading the SmolLM2 v1
receipts row by row, where the same two FAILs were already visible. The spec writer's
diagnostics used `compose propose` only, which does not run the driver's containment check.

## What this verdict means

- FAIL stands. Qwen3-4B is not qualified by this campaign. CAND-4 (Llama) is unaffected.
- Observation, not a claim: on the rows that test the model and the production path (destination,
  content, stop, approval binding, evidence-citing explanation, restart identity and recall), all
  three tasks pass on the GB10.
- A new campaign version must replace the v5 containment rows with the v6 Section 6.1 rule and
  must be declared before any run. T1..T3 and these replies are now seen, so a repeat on the
  same tasks is not held-out evidence and must say so; held-out tasks give the stronger result.
- One campaign without the GPU memory failure is not a rate; omega#327 stays open.
