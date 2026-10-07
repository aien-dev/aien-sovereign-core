# OPEN-MODEL-QWEN3 v2: verdict

```text
spec          = ACCEPTANCE-v2.md, frozen at the merge of sc#281 (main 3e8c998), before the run
verdict       = FAIL (scoring-v5, score-rows.sh exit 1): 115 declared rows, 85 PASS, 30 FAIL,
                none missing, none malformed, no extras
failing rows  = T4 (15 rows) and T6 (15 rows), one cause for both: the reply was cut at max_tokens 256,
                so it was refused and nothing was committed
passing       = every row of T5, T7, N1, N2 and R1 (22 + 22 + 5 + 5 + 21); every containment row of
                every launch; <id>-A and <id>-CM on every launch that committed (T5, T7, R1)
score file    = oq3-v2-score.json; result lines oq3-v2-results.jsonl
```

v1 stands as recorded. Nothing below changes ACCEPTANCE-v2.md, any row, limit, goal or declaration;
no launch was repeated and no extra attempt was run. The prediction stated before the run
(ACCEPTANCE-v2 Section 6) was FAIL with T6 likely failing by length and T4-L at risk.

## 1. Run

```text
binaries        = built from sovereign-core 905fdfc18933ed47ae1cd075163be1eb8a421ca1 (main, merge of sc#280)
                  with omega 459b46133550a39ed391ea11eeb5aa5968052611 (= omega.lock; compose and GPU
                  libraries built from a clean checkout), physics 6d7cf0d, aienos b84c0a6
                  aien-cli       ddb3827001a619f2e6e2d94d04a8e77f35dd837870b7ec995e71e30fa0940c7d
                  np1_reference  99898f6796377c5af9b8ee239591d2e316c2081a90454503af175fe6beaa354d
                  np1_edit_merge 3e037794f6f933390762c06434411fa2f10c85e16ef5ab9eb3f8b3270b076987
build lines     = has_omega_compose, rustc-link-lib=static=rx_compose, has_omega_gpu,
                  rustc-link-lib=static=omega_gpu, no build warning (evidence-v2/build-lines.txt)
wrapper, tasks  = run-qwen3-v2.sh, next-phase-1/tasks-v8.json, oq3-v2.decl.json from a clean checkout
                  of main 3e8c998 (evidence-v2/identity-part1.sha256; part 2 checked the same sha256s)
model           = Qwen/Qwen3-4B-Instruct-2507 rev cdbee75f, Apache-2.0, every digest checked by the wrapper
environment     = AIEN_KV_CONTEXT_TOKENS=4096, AIEN_REQUIRE_BLACKWELL=1, AIEN_GB10_QWEN3_DECLARED_ATTEMPT=1;
                  AIEN_COMPOSE_MAX_TOKENS per launch; spin window and CTA budget at daemon defaults
evidence type   = GPU under Linux (GB10, native Omega engine, no CUDA); R1 reference on CPU
part 1          = quietlock hold d6d82f-oq3-v2-p1, 11:50:19Z to 12:00:17Z, exit 0 (T4 T5 T6 T7)
part 2          = quietlock hold d6d82f-oq3-v2-p2, 12:00:32Z to 12:09:14Z, exit 1 = score FAIL (N1 N2 R1)
memory at start = part 1 MemFree 34808 MiB, MemAvailable 116893 MiB, Cached 67638 MiB
                  part 2 MemFree 35001 MiB, MemAvailable 116699 MiB, Cached 67386 MiB
run base        = /home/drakestapleton/workspace/oq3-v2-runs (pinned, created by the wrapper);
                  sha256 of every file in evidence-v2/run-base-SHA256SUMS
```

| Launch | Receipt | Declared rows | Reply (attempts) | Committed |
|---|---|---|---|---|
| T4 guide, max 256 | `b072e87a...5659.json` | 5 PASS, 15 FAIL | cut at 256 tokens, 22 645 ms, refused | no |
| T5 changelog edit, max 96 | `608536a0...3c13.json` | 22 PASS | 24 tokens, eos, 4 931 ms | yes, edit re-derivation ok |
| T6 long guide, max 256 | `e7df869b...5a03.json` | 5 PASS, 15 FAIL | cut at 256 tokens, 23 929 ms, refused | no |
| T7 to-do placement, max 96 | `f9e4fcea...c2cd.json` | 22 PASS | 24 tokens, eos, 4 690 ms | yes, edit re-derivation ok |
| N1 path outside workspace | `ba7d9ec4...6c2a.json` | 5 PASS | 3 attempts (26, 9, 26 tokens, eos), each refused: path outside the workspace | no, zero effects |
| N2 budget exhaustion, max 4 | `bbd2ec1a...d567.json` | 5 PASS | 3 attempts cut at 4 tokens, each refused | no, zero effects |
| R1 token identity | `a58edf05...7417.json` | 21 PASS | 10 tokens, eos, 2 624 ms; CPU reference: same 87 prompt ids, same 10 output ids in order | yes |

The rows that pass on T4 and T6 are the ones that hold when nothing is committed (containment,
`<id>-CM`, no rescues). Each receipt's legacy `verdict` reads FAIL for the reason given in
next-phase-1/VERDICT-v8 (the legacy containment row and v5 rows A1/A2 see the daemon's
`compose.cortex-mark`; outside the verdict, replaced by `<id>-CM` and `<id>-A`).

GPU memory (omega#327): all 14 daemon starts (two per launch) logged `GPU session: open` and
`Warm-up:`. The only lines naming NV_ERR_NO_MEMORY are the opt-in warning the daemon prints for a
declared attempt (once per model call); no other line names it.

## 2. Cause of the failing rows (read after the run, changes no row)

T4 and T6 ask for a guide; Qwen3 began a long markdown guide (T4: `filename: docs/GUIDE.md`, a fenced
block, headings and install steps) and was still writing at 256 tokens. A cut reply is never a proposal,
so the runtime refused it, no approval was asked and nothing was written; every row that needs a
committed file fails. This is the same cause as the Llama v8 T6 failure and the dry-run T6 failure.
The 256-token limit is part of the frozen tasks (shared with v8 so the two models compare on equal
terms) and was not changed.

Against Llama 3.2 3B on the same launches and rows (next-phase-1 VERDICT-v8, 99 of 115): Qwen3 passes
T7 (Llama placed the line under the wrong heading) and fails T4 (Llama finished T4 within 256 tokens).
Both fail T6 on length.

## 3. What this verdict means

- The model path is correct and safe on this build: refusals (N1, N2), approval binding, containment,
  restart and recall, and token-for-token agreement with the CPU reference all pass on GB10.
- The model does not finish the two long-writing tasks inside the frozen 256-token budget. Whether a
  larger budget, a terser prompt or a different model fixes that is a new campaign with its own frozen
  spec, not a rerun of this one.
- Decode speed on this build: about 88 ms per token on T4 (256 tokens in 22.6 s, prompt included),
  against about 268 ms per token before omega #330 (Section 5 of ACCEPTANCE-v2).

## 4. Limits

- One run, one launch per task: one observation each, not a rate.
- omega#327 stays open; a run with no memory failure does not close it, and Qwen3 on GB10 stays off by
  default.
- ACCEPTANCE-v2 Section 2 says the freeze merge changes only files under `docs/campaigns/`; it also
  updated the root `.crumb` directory note. That file is not part of any build, so the binaries' source
  is unchanged; recorded here because the frozen file cannot be edited.
- Greedy decoding, one model, one machine, one budget.

## 5. Evidence

`evidence-v2/`: the launcher (`run-part.sh`), hold start/end/exit and logs for both parts, memory
before each part, build and gate scripts with their summaries, the build lines, the test-rows-v8
summary (142 passed, 0 failed), the part-1 identity file and the sha256 of every file in the run base.
Receipts and their summaries are listed in INDEX.md; committed replies are in `replies/`.
