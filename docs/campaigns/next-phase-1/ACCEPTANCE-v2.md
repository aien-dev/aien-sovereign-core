# NEXT-PHASE-1 campaign v2: acceptance criteria (frozen before any v2 code or run)

```text
campaign_id  = "next-phase-1"
spec_version = 2
status       = FROZEN at the commit that adds this file. The v2 code (fixed
               proposal template, bounded automatic retry, omega.lock bump)
               lands in LATER commits, so this file's hash predates it.
supersedes   = nothing in Section 3 of ACCEPTANCE.md (spec_version 1).
```

v1 stands as recorded: receipts `92acb566...` (attempt 4) and the three attempts before it
are never edited. v1 verdict FAIL on "Human interventions: rescues" = 3.

## 1. Task, steps, receipts: unchanged

The bounded task, the eight steps S1..S8 and their recorded states (ACCEPTANCE.md Section 1),
the environment class (Section 2) and both receipt formats (Section 4) apply unchanged.

## 2. Pass criteria: v1 thresholds, unchanged

| Criterion | Threshold (as v1) |
|-----------|-------------------|
| Task completion | 8 of 8 |
| Correctness: committed content | byte-identical, digests equal |
| Correctness: recall | byte-identical |
| Latency | report only |
| Resource use | report only (VmHWM before and after restart, GPU yes/no) |
| Human interventions: approvals | exactly 1, for the committed effect |
| Human interventions: rescues | 0 |
| Identity | identical digest |
| Memory | exists, digest verified, equal to pre-restart digest |
| Containment: workspace | empty |
| Containment: speculation | none |

**What counts as a rescue (made explicit, not changed):** any human action after the campaign
driver is launched (editing, re-running a step, changing a prompt, goal or setting, restarting
outside S7). An automatic retry made by the Skill itself, inside one `RunComposeTask`, is NOT a
rescue. The driver is launched once, with one goal, one task, two daemon starts (one restart).

## 3. Declared production-path changes (v2)

(a) **Fixed proposal template.** The "model" Skill of `RunComposeTask` asks the model for
exactly: first line `filename: <relative path>`, then the complete file content. The template is
part of the production `RunComposeTask` path (aien-runtime), the same for every goal; it is not
a per-run prompt edit. The parser accepts only that shape: the first nonempty line must be
`filename: <path>`, the path relative, without `..`, not absolute, not `~`, and the content
nonempty. A reply that fails the parser is refused by the Skill before AEGIS sees it.

(b) **Bounded automatic retry inside the Skill.** At most 3 proposals per task. Attempt k > 1
uses the same template plus one fixed correction line that names the previous refusal reason;
decoding stays greedy (temperature 0). Every attempt is recorded in the task report and in the
campaign receipt as `proposal_attempts[]`: attempt number, wall ms, generated token count, outcome
(`parsed` / `refused` / `timeout` / `error`), refusal reason, sha256 of the text, the text, and for
the attempt handed to AEGIS the AEGIS outcome (pass / fail).

## 4. Token and time budget (measured input, declared)

- `max_tokens` per attempt = 48 (`AIEN_COMPOSE_MAX_TOKENS` default for the compose Skill).
- Measured rate on this path (v1, GB10, TinyLlama, same daemon): S3 = 13.6 s at 32 tokens
  and 16.5 s at 48 tokens, i.e. about 5.7 tokens/s marginal with about 8 s fixed per attempt
  (INFERRED from two points; v2 reports the measured per-attempt rate).
- Wall budget for all attempts of one task = 25 s, unchanged, below `rx_compose_run`'s 30 s
  quiescence wait, which is NOT raised. Attempt 1 always starts; attempt k > 1 starts only if the
  remaining budget is at least the measured duration of attempt k-1; each attempt's limit is the
  remaining budget. At the measured rate a full 48-token attempt takes about 16.5 s, so a second
  attempt fits only when the first ends early (end of sequence); this is expected and recorded.

## 5. Binding and run conditions

- omega pinned by `omega.lock` = 62b6a2899fa230e57fcf7855ea7e2c26404514b6 (omega main after
  omega#311). aien-omega-compose builds librx_compose from the lock by default. The GPU engine
  (aien-omega-gpu) reads the same lock, so v2 also moves the GPU engine pin from CAND-2
  79a805d to 62b6a28; the receipt records both.
- GB10 hold through omega `tools/quietlock` (`quietlock hold --minutes <= 20`), with start and
  release whispers. One run of the driver. The verdict is whatever the rows give.
- Performance is an observation, not a criterion: the summary reports measured tokens/s.
