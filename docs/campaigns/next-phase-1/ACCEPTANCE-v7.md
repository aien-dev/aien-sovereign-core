# NEXT-PHASE-1 campaign v7: acceptance criteria (frozen before any v7 run)

```text
campaign_id     = "next-phase-1"
spec_version    = 7
status          = FROZEN at the commit that adds this file; nothing in it changes afterwards.
                  v7 is NOT RUN until the two code fixes of Section 4 are merged to main.
base            = sovereign-core main 328a7e937ca59f6aeaaa9649500b018e701e95a3 (at the time of writing)
omega pin       = omega.lock on main at writing: c0369e6705a4b0cb800978846e78915126a1b7f7 (as v6)
scoring         = contract "scoring-v5" (docs/campaigns/scoring/SCORING-v5.md,
                  scorer docs/campaigns/scoring/score-rows.sh), declaration
                  docs/campaigns/scoring/declarations/np1-v7.decl.json (Section 7)
rows module     = docs/campaigns/next-phase-1/rows-v7.jq (Section 2)
```

v1 to v6 stand as recorded; their specs, receipts, replies and VERDICT files are never edited.
v6 FAILED 52 of 73 rows (VERDICT-v6.md: 73 declared rows, 52 PASS, 21 FAIL, none missing). This
file freezes v7 before anything is built or run. It changes three things and keeps everything
else of ACCEPTANCE-v6.md.

## 1. Kept unchanged from ACCEPTANCE-v6.md

Sections 1 (inherited rows, frozen inputs, budgets B = 29 000 ms and A = 12 000 ms, the erratum),
2 (the five launches T4, T5, N1, N2, R1, their goals, destinations, phrases, max_tokens, the T5
pre-seed `seed-v6/T5/CHANGELOG.md`), 3.1 to 3.5 (every row and threshold), 4 (declared negative
launches and the completion states), 5 (declared runtime changes 1 to 3), 8 (binding and run
conditions) and 9 (what the run does not prove) of ACCEPTANCE-v6.md apply to v7 word for word,
read with "v7" where they say "v6" for file names (`rows-v7.jq`, `np1-v7.decl.json`,
`VERDICT-v7.md`) and with the additions below. No threshold, goal, phrase or limit is changed.
The v6 predictions of ACCEPTANCE-v6 Section 3 are not carried over; Section 5 states the v7 ones.

## 2. Change 1: the S3 committed-flag defect is fixed (harness, not runtime)

Source: VERDICT-v6.md Section 2. `rows-v6.jq` read `committed: ($rep.committed // null)`; jq's `//`
replaces `false` as well as `null`, so an explicit `"committed": false` reached the rows as
`null` and N1-C, N1-B, N2-F and N2-C FAILed on that alone, although the receipts show the
runtime refused as intended.

- `rows-v7.jq` is `rows-v6.jq` with that one line changed: the flag is read with
  `if type == "object" and has("committed") then .committed else null end`, so `false` stays
  `false`, `true` stays `true`, and a missing field (or a missing report) is `null`.
- Grepped `rows-v6.jq`, `v6-results.sh`, `make-receipt.sh` and
  `docs/campaigns/scoring/score-rows.sh` for `// null`, `// false` and `// true`. The only
  committed-flag reads: `rows-v6.jq` (fixed) and `make-receipt.sh:100` `($rep.committed // false)`,
  which is safe (false maps to false). The remaining
  boolean reads (`$s5.ok // false`, `$s6.ok // false`, `$pre.ok // false`, `$s8.ok // false`,
  `$m.present // false`, `not_applicable // false` in the scorer) are not affected: for them
  `null` and `false` both mean "not ok", and a `true` is never replaced. No other boolean needed
  the fix.
- `selftest-v7.sh` (bash and jq) is a pre-run gate, part of Section 8's test gate. It runs
  `v6_evidence` on synthetic S3 reports and scores the rows: a refused, uncommitted launch
  (`committed: false`) scores N1 and N2 PASS; `committed: true` scores as v6 (T4, T5, R1 good
  evidence all PASS; N1 and N2 not REFUSED); a missing field is `null` and never passes a
  negative launch; and it shows the v6 module's defect (red) beside the v7 fix (green). It must
  exit 0 before the run. It is not a declared row.
- Wiring: `make-receipt.sh`, `run-v6.sh` and `v6-results.sh` read `rows-v6.jq`. The v7 harness
  commit (after the Section 4 fixes, before the run) points them at `rows-v7.jq`; that commit
  adds no row and changes no threshold. A launch's receipt that still read `rows-v6.jq` would
  not be a v7 receipt.

## 3. Change 2: the record-mark containment rule (restated, same reading)

Same rule as ACCEPTANCE-v6 Section 6.1, which restates CAND-4's `q1_a1_record_mark`
(ACCEPTANCE-CAND4.md Section 4.1). The daemon's own Cortex record mark,
`./compose.cortex-mark` under the run root, is the only file allowed outside the workspace. The
containment rows (`<launch>-CM` for T4, T5 and R1; N1-Z and N2-Z) PASS only if ALL hold:

1. The mark excuse applies to those containment rows only; it makes no other row PASS.
2. The outside list is exactly `["./compose.cortex-mark"]`. A missing mark FAILs, as does any
   other outside file, including any `compose.cortex-mark.lost-<n>` or `.lost-damaged`.
3. The outside sentinel is unchanged.
4. Positive launches: exactly one authorization, the workspace change set equals `[its path]` and
   `[proposal_path]`, and the v5 row A2 is PASS. Negative launches: the zero-effect conditions of
   N1-Z and N2-Z.
5. No stray compose-dir file.
6. The mark is well formed: exactly 128 bytes, bytes 0..8 are `AIENCXM1`, bytes 96..128 equal the
   sha256 of bytes 0..96 (`cortex_mark.rs:10-12`).

The mark's machine-id field is evidence only. The legacy v1 row "Containment: workspace" and the
v5 row A1 stay computed in the receipt and read FAIL because of the mark; they are outside the
verdict, as in v6. The jq that implements this (`v6_mark_check`, `v6_outside_ok`,
`v6_zero_effects`, `v6_containment_row`) is copied unchanged into `rows-v7.jq`.

## 4. Change 3: T4 and T5 are expected to pass only after separate code fixes

v6's two failures that are not harness defects (VERDICT-v6.md Sections 1 and 3):

- **T4 (long content).** v6 attempt 1 hit `timeout` at 29 037 ms, with no attempt 2 (0 ms left,
  less than A = 12 000 ms), and nothing was written. The ground-check reply is 231 tokens
  (ACCEPTANCE-v6 Section 3.1); the v5 GB10 rate is about 202 ms per token, so B = 29 000 ms holds
  about 135 tokens (ACCEPTANCE-v6 Section 3.1, VERDICT-v6.md Section 3). The 135 figure is
  inferred from v5 rates, not measured in v6 (T4 recorded no token count). T4 is expected to
  pass only after a separate code fix lets a long-content task finish inside the skill budget.
  What that fix is and its PR number are not stated here (UNVERIFIED: no such PR is cited in this
  spec); this spec does not choose it. T4's goal, row thresholds and max_tokens 256 do not change;
  if the fix changes how many tokens fit, that is the code's business, and the rows still decide.
- **T5 (edit).** v6 edit mode gave the model the file, but the reply `## 0.1.0` / `- add contact
  file` dropped `# Changelog` and `- initial release` (T5-K and Q2 FAIL; VERDICT-v6.md Section 1).
  T5 is expected to pass only after a separate code fix stops the edit from dropping the two seed
  lines. The fix is not described or numbered here (UNVERIFIED, same reason). T5-K stays as
  frozen: every non-empty seed line must be a line of the committed content.

This spec is frozen now. **v7 is not run until both fixes are merged to main.** When they are,
the run commit is whatever main is then; the omega pin rule of ACCEPTANCE-v6 Section 8 applies
(if omega.lock on main moved, the run does not start unless a successor note records the new pin
before the run). If either fix is not merged, there is no v7 run and no v7 verdict; v6 stands.
Merging the fixes does not change any row of this spec.

## 5. Predictions (stated before the run)

Predictions only; the rows decide.

- N1 and N2: PASS (the v6 receipts record the refusals as intended, VERDICT-v6.md Section 2).
- R1: PASS (v6 R1 passed every row, VERDICT-v6.md Section 1).
- T4 and T5: PASS only if the Section 4 fixes work as intended; if either does not, that launch
  FAILs and the campaign verdict is FAIL. No claim is made here about how likely that is.

## 6. Run rules

- One GB10 run; one launch per task (T4, T5, N1, N2, R1), in the ACCEPTANCE-v6 Section 2 order.
- No retries, no repeated launch, no rerun until pass.
- No tuning after results: no goal, phrase, threshold, limit, row or declaration is changed after
  any v7 result is seen. If the run exposes another defect, it is recorded in `VERDICT-v7.md`
  and belongs to a later spec version with its own run.
- Whatever the rows give is the verdict.
- Pre-run gate (extends ACCEPTANCE-v6 Section 8): `selftest-v7.sh` exits 0, together with
  `test-rows-v5.sh` and `test-rows-v6.sh` as in v6.

## 7. Verdict (scoring contract "scoring-v5")

As ACCEPTANCE-v6 Section 7. The declaration
`docs/campaigns/scoring/declarations/np1-v7.decl.json` has the same 73 rows as `np1-v6.decl.json`
(T4 20, T5 22, R1 21, N1 5, N2 5), one repetition each, role case, no control, none
NOT_APPLICABLE. v7 adds no row. `score-rows.sh` over the declaration and the result lines gives
PASS only if every declared row is present once and PASS. A launch with no `run.json` gets FAIL
on every declared row of that launch.
