# REGRESSION sc#394: append goals place new lines next to the heading, not at the end

Pre-registered before any test or fix. Program: aien-architecture#190 (lane L1/L6).

## Failing behaviour as observed

Evidence is in branch `e2e/harness-v1.2`, folder
`docs/campaigns/whole-system-e2e/runs/RUN-1-shakedown-20261010T150104Z/artifacts/`
(not edited here): `E4-report.md`, `E3m-report.md`, `E4-propose.json`.

Row E4, objective `Append one line to report.md naming the topic you reported on first.`
Memory was recalled (`memory.items_included 1`) and the model named the right topic.
`merge_edit_reply` (crates/aien-runtime/src/spine.rs) inserted the new line right after the
anchor above it, the heading. `E3m-report.md` (before) has the heading at line 1; `E4-report.md`
(after) has the new line at line 3 and 8 lines against 7 before (`diff` says `2a3`). The harness
row and the independent verifier both scored placement FAIL.

## Tests to add (unit tests in spine.rs)

| Test | On current main | After the fix |
|---|---|---|
| `append_goal_places_new_lines_at_end` | FAIL (new line lands after the heading) | PASS |
| `append_goal_with_several_new_lines_keeps_reply_order` | FAIL | PASS |
| `non_append_goal_keeps_anchor_placement` (negative control) | PASS | PASS |
| `append_goal_still_needs_a_shared_line` | PASS | PASS |
| `append_word_detection` | FAIL (to compile-fail or fail: the detector does not exist yet) | PASS |

Tests 3 and 4 pass on main by design: they pin behaviour the fix must not change.
Tests 1, 2 and 5 call the new placement entry points, so on main they do not compile; the
pre-fix run is made against a stub of the entry points that keeps today's placement, so the
failures are assertion failures and not build errors.

## Rule chosen for "append goal"

The goal contains the word `append` as a whole word, case-insensitive: split the goal on every
character that is not an ASCII letter or digit and compare each piece to `append`. So
`Append one line`, `append` and `APPEND` match; `appendix` and `appended` do NOT match
(they are different words). No environment variable, no configuration.

## What the fix changes

In an append goal every non-anchor reply line goes after the last non-blank prior line, in
reply order. The goal reaches the merge through a small `EditPlacement { NextToAnchor, AtEnd }`
value computed once from the goal where the edit target is built.

## What the fix must not change

- No prior line is removed or rewritten; prior bytes are kept.
- The reply must still share a non-blank line with the prior (Err otherwise).
- The cell limit (`COMPOSE_EDIT_MERGE_MAX_CELLS`) and the "changes nothing" Err stay.
- The result ends with exactly one newline.
- Non-append goals: output byte for byte as today. Existing tests of `merge_edit_reply`,
  `edit_proposal`, `propose_task_checked` keep their signatures and results.
