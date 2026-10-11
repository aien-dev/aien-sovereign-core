# aien-verify v1.1: E3m topic words follow the rule the run was judged under (sc#423, R6 #402)

Committed before the verifier change that implements it. `VERIFIER-v1.md` stays as written; this file adds to it.
Contract: `ACCEPTANCE_E2E.md` FROZEN at aien-architecture b2400fe, sha256 `9e3d5f4e...`, unchanged.

## Why

`ACCEPTANCE-v1.3.1-e3m.md` (sc#416) changed the harness's check D4 of the supplementary row E3m to case-insensitive
whole-word matching of the topic words and left the verifier on exact matching, so RUN-3-GB10 recorded
`status_disagreement: E3m receipt says PASS but the facts derive FAIL` (its report names `Windmill` only in the heading).
The verifier should judge each folder by the rule that folder was judged under, so recorded folders keep their results.

## E3m D4 rule (new)

The harness writes the rule it used into the E3m receipt as `d4_rule` (since v1.3.1; inside the hash chain, so it cannot
be changed without breaking a link). The verifier reads that fact:

| E3m receipt `d4_rule` | D4 topic-word rule the verifier applies |
|---|---|
| absent (RUN-1, RUN-2-GB10 and every folder before v1.3.1) | exact, case-sensitive substring, as v1 |
| exactly `case-insensitive whole word: LC_ALL=C grep -q -i -w (ACCEPTANCE-v1.3.1-e3m.md)` | case-insensitive whole word, the v1.3.1 rule: a topic word counts when it occurs in any ASCII letter case with no ASCII letter, digit or underscore directly before or after it (GNU `grep -w` under `LC_ALL=C`) |
| any other value | E3m NOT_RUN, naming the unknown rule (never a guess) |

The `ACCEPTANCE-v1.3.1-e3m.as-run.md` copy in a run folder is NOT the key: it is not inside the chain and says what was
in force, not what the receipt was judged by. Check D4 of E3 (file names) is unchanged: exact, case-sensitive.

E3m is a supplementary row. It is not one of the twelve rows that make the `verdict_id`, so this change cannot move any
recorded verdict id; it changes only the E3m line and the status disagreements in `VERIFIER_VERDICT.txt`.

## CTRL-E4: the disagreement with the harness stays, and why

The harness (ACCEPTANCE-v1.2.md CTRL-E4) passes CTRL-E4 on a refusal by name with the report unchanged. The verifier
(VERIFIER-v1.md, pre-registered) also requires E4 itself to have shown a recall; otherwise CTRL-E4 is NOT_RUN. That stays:

1. The frozen contract pairs CTRL-E4 with E4 ("with the store's files removed ... the second objective fails with a
   named refusal"). A negative control shows that the success depends on the thing removed. Without a success there is
   nothing to contrast: in RUN-3-GB10 the second objective was refused with the store intact (a path-form refusal), so a
   refusal with the store removed shows only that the daemon refuses, not that memory was what it needed.
2. NOT_RUN is never a PASS, and the case cannot change a program outcome: whenever the two disagree, E4 is not PASS, so
   the run already fails the contract. When E4 recalls, the two rules give the same answer.
3. Changing it would move recorded verdict ids (RUN-3-GB10 CTRL-E4 NOT_RUN is inside its id), which this lane must not do.

## Regression (filled in after the change, on scratch copies)

| folder | recorded `verdict_id` (its `VERIFIER_RECEIPT.json`) | expected after |
|---|---|---|
| RUN-1-20261010T154724Z | base `a50fccb6...`; with that base as peer `437467db...` | same, E3m PASS as before |
| RUN-2-GB10-20261011T030554Z | `af9e744c...` | same, E3m FAIL as before (no `d4_rule`, exact rule) |
| RUN-3-GB10-20261011T045450Z | `9ca9b7af...` | same; E3m PASS under its `d4_rule`, its E3m disagreement gone, CTRL-E4 disagreement stays |

## Results (2026-10-11, branch e2e/r6-verify, verifier built with `tools/aien-verify/build.sh`, rustc only, 0 warnings)

| check | result |
|---|---|
| `tools/aien-verify/tests/run.sh` | passed 51, failed 0 (was 43; eight new checks below plus one new unit test) |
| the same tests against the main verifier (8c09217) | 3 FAIL: RUN-3 E3m PASS under its `d4_rule`, RUN-3 E3m disagreement gone, unknown `d4_rule` NOT_RUN (red before green) |
| `rustc --test` unit tests | 3 passed (new: `whole_word_ci_is_grep_i_w`) |
| `scripts/whole_system_e2e.sh selftest` | PASS |
| RUN-1-20261010T154724Z, scratch copy | `verdict_id a50fccb63363ef9224a19bd8bf4f249754abf5d88667ba0fd6ce93dfb8d63d80`; with it as peer `verdict_id 437467db4149b013738578c10ef68b7168dfb95feae8dea882de17a6118fc5fd`; E3m PASS (unchanged) |
| RUN-2-GB10-20261011T030554Z, scratch copy | `verdict_id af9e744cc3187400e00599159999f357f53c1df867556f62c8f71a552662800c` (as recorded); E3m FAIL `["windmill"]` (unchanged) |
| RUN-3-GB10-20261011T045450Z, scratch copy | `verdict_id 9ca9b7af6f9255fc7247acae33492768252d5f2017b40c29c159e04fbca833d5` (as recorded); E3m PASS under its `d4_rule`; the E3m disagreement is gone; the CTRL-E4 disagreement stays |
| RUN-3 copy with `d4_rule` removed, links and manifest repaired | chain unbroken, E3m FAIL `windmill` (exact rule) |
| RUN-3 copy with an unknown `d4_rule`, repaired | E3m NOT_RUN naming the unknown rule |

`runs/` was not modified (the test's tree digest before and after is equal). The recorded `VERIFIER_VERDICT.txt` files keep
the text the older verifier wrote; they are evidence of that run and are not regenerated.
