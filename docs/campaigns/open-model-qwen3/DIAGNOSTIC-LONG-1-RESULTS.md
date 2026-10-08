# DIAGNOSTIC-LONG-1 results (measurement, no verdict)

Run by session 031756, 2026-10-07, following DIAGNOSTIC-LONG-1.md (3a696da) unchanged: same five tasks, budgets
and prediction. Labelled measurement on an unmerged build. Nothing here is a pass, a fail or a score.

## Build and conditions (all OBSERVED)

- sovereign-core 1297f83 (PR #284) with one local, uncommitted change: `omega.lock` set to 7cca7af, because the
  build refuses an omega checkout that differs from the lock. omega 7cca7af (PR #331), physics 6d7cf0d.
- Build logs show `has_omega_compose`, `has_omega_gpu` and `has_omega_wait_ms`, and link `rx_compose` and `omega_gpu`.
- sha256: aien-cli be9f2ec812f207eacca80fb188f38c8a2a2bf7033bc4116b91df651273b3ca10; np1_reference 99898f67...354d;
  np1_edit_merge 3f39f604...71f; tasks file ca4ba4d9ad1f9034c230f2122f0e22b6299adf1ec58627e82b9ec4d814febb1f.
- CPU tests with the omega libraries linked (`cargo test -p aien-omega-compose -p aien-runtime -p aien-cli`, no model):
  376 passed, 0 failed, 56 ignored (end-to-end tests needing a model or checkpoint). Includes
  `compose_budget_setting_parses_and_refuses` and `measured_attempt_budget_admits_a_second_attempt`.
- Environment as v2 (`AIEN_KV_CONTEXT_TOKENS=4096`, `AIEN_REQUIRE_BLACKWELL=1`, declared attempt) plus
  `AIEN_COMPOSE_BUDGET_MS=120000` and `AIEN_COMPOSE_MAX_TOKENS=1024`. Model Qwen3-4B-Instruct-2507 cdbee75, the
  v2 digests (not re-checked by a v2 wrapper; unchanged files).
- Driver: next-phase-1 `run-campaign.sh` from 1297f83, unchanged, via a small bash wrapper (`run-part.sh`). The
  receipt step used a scratch copy of the next-phase-1 folder whose `tasks-v8.json` was replaced by the diagnostic
  tasks file, so the receipt tool could find D1 to D5. No task text, budget or prediction was changed.
- Two quietlock holds named 031756-diag-long1 (owner id), no other hold or GPU job was live: part 1 (D1 D2 D3)
  16:35:54Z to 16:44:19Z, exit 0; part 2 (D4 D5) 16:44:32Z to 16:50:45Z, exit 0. MemFree 35.8 GB, Cached 69.8 GB
  before part 1 (35.5 GB / 69.5 GB before part 2). One launch per task, no retry, no rerun.

## Per task (OBSERVED)

| Task | tokens | finish | attempt ms | ms/token | parsed | committed | rows L / M / A / CM | non-empty lines in committed file |
|---|---|---|---|---|---|---|---|---|
| D1 | 313 | eos | 28625 | 91.5 | yes | yes | PASS PASS PASS PASS | 23 (min 12) |
| D2 | 370 | eos | 33026 | 89.3 | yes | yes | PASS PASS PASS PASS | 23 (min 12) |
| D3 | 435 | eos | 37798 | 86.9 | yes | yes | FAIL PASS PASS PASS | 13 (min 20) |
| D4 | 473 | eos | 40906 | 86.5 | yes | yes | PASS PASS PASS PASS | 30 (min 15) |
| D5 | 649 | eos | 61943 | 95.4 | yes | yes | FAIL PASS PASS PASS | 3 (min 20) |

Every task used one attempt (no second attempt). Prefill is not logged separately per attempt; "attempt ms"
includes it. Daemon start warm-up was about 27 s per start. Rows are a correctness reading only, as the plan says.
Launch wall time (two daemon starts plus all steps): 163 s, 169 s, 173 s, 176 s, 197 s.

## Readings against the plan

- No task was cut at 1024. All five finished by eos at 313 to 649 tokens, so the model stops on its own on these
  long documents; the v2 cut at 256 hid a complete answer for D1 and D2 (they finished at 313 and 370 tokens).
- D1 and D2 (the v8 T4 and T6 goals): both parse, commit and pass the line count (D1 23 non-empty lines against 12).
  Under the plan's reading this is "eos and rows pass: truncation was hiding a good answer", for these two goals.
- D3 stopped by eos but wrote 13 non-empty lines (paragraph prose) against a 20 line minimum: the answer was
  complete but short on line count, a task-wording matter, not a budget one.
- D5 (FAQ): the reply was 649 tokens and 66 lines with 18 code fence lines (nested fenced blocks). The committed file has
  4 lines: the proposal parser ended the content at the first inner closing fence. INFERRED from the reply text and
  the committed file (not tested further): a nested fence in a reply truncates the committed content. Product issue,
  separate from the token budget.
- Prediction check (stated in the plan before the run): D1 and D2 predicted to stop between 400 and 800 tokens;
  they stopped earlier (313, 370). Predicted at least one of five cut at 1024 (about 60 percent): none was.
  D3 predicted most likely to be cut: it stopped at 435. D5 predicted most likely to stop by eos: it did (the longest).
- omega#327 GPU memory error lines (NV_ERR_NO_MEMORY): none in any daemon log. The only matching line is the
  declared-attempt WARNING printed at each daemon start.

## Limits

Five launches, greedy, one observation each: not a rate. D3 to D5 were never run before, D1 and D2 were seen in v2.
Unmerged build with a local `omega.lock` change. Per-launch timing is wall clock on a loaded machine (about 70 GB
cache). Raw logs, receipts and sha256 list: `~/workspace/oq3-diag-long1-runs` and
`~/workspace/oq3-diag-long1-records` (`raw-logs.sha256`, `identity.sha256`).
