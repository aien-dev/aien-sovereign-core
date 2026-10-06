# Scoring contract v5 (shared by campaigns)

```text
contract      = "scoring-v5"
status        = FROZEN at the commit that adds this file
scorer        = docs/campaigns/scoring/score-rows.sh (shell + jq)
tests         = docs/campaigns/scoring/test-scoring-v5.sh
reason        = NEXT-PHASE-2 v4 review: next-phase-2/make-receipt.sh scores a row PASS when
                every repetition it finds is PASS, whatever their number. A row with 2 of
                3 required repetitions, or 1 of 3, scores PASS.
applies_to    = receipts of NEXT-PHASE-2 v5 and later; NEXT-PHASE-1 v6 may adopt it
```

## 1. What is not changed

`next-phase-2/make-receipt.sh`, receipt `e401fffe...`, `VERDICT-v4.md` and `ACCEPTANCE-v4.md`
are untouched. The v4 receipt stays reproducible because its scorer is the same bytes. v4 is a
v4 result under the v4 rule; scored under this contract it also PASSes (section 6).
`next-phase-1/` is not touched.

## 2. Declaration

The case set is read from a frozen declaration file (JSON), never inferred from the results.
It is transcribed from the frozen acceptance file and names that file (`frozen_by`).

```text
{ contract: "scoring-v5", campaign: <name>, frozen_by: <acceptance file>,
  rows: [ { row, role: "case"|"control", reps: <integer >= 1>, expected: "PASS",
            control: <row id> | "F0" | null, not_applicable: <bool> } ] }
```

`expected` must be `"PASS"`. A bad declaration (missing field, reps < 1, duplicate row id,
a control that names no declared row) gives verdict ERROR. `"F0"` is an external control: its
value is passed in a third file (`{"F0": true}`); absent means false.

## 3. Results

One JSON object per line, one line per run: `row`, `rep` (integer >= 1), `verdict` (PASS,
FAIL, NOT_RUN, NOT_APPLICABLE or UNVERIFIED). A NOT_APPLICABLE run also needs a non-empty
`outcome`, which is its stated reason. A run id is `(row, rep)`.

## 4. Row rules (first matching rule wins)

1. Two records with the same run id: ERROR. Never counted twice.
2. Any counted repetition FAIL: FAIL.
3. NOT_APPLICABLE on a row whose declaration does not list `not_applicable`: FAIL.
4. A record of the row that is not well formed (no integer `rep`, no valid `verdict`, a
   NOT_APPLICABLE without reason), or any UNVERIFIED run: INCOMPLETE.
5. Fewer than `reps` distinct required repetitions (1..reps): INCOMPLETE; none at all: NOT_RUN.
6. Any NOT_RUN repetition: NOT_RUN.
7. NOT_APPLICABLE runs: NOT_APPLICABLE only if every repetition is NOT_APPLICABLE; mixed with
   PASS it is INCOMPLETE. NOT_APPLICABLE never counts as PASS and never satisfies a required
   repetition.
8. Otherwise (exactly `reps` repetitions, all PASS): PASS.

Control: a row with a `control` whose rule 8 result is PASS or NOT_APPLICABLE is NOT_RUN
unless its control row scored PASS (a control row must have its declared expected result,
PASS) or its external control is true. A control row that does not score PASS is itself FAIL,
INCOMPLETE, ERROR or NOT_RUN by the rules above.

Extra runs: a run with `rep` above `reps`, or for a row not in the declaration, is listed
under `extras` and never counted. It cannot raise or lower a verdict.

## 5. Campaign verdict

PASS only if there is no malformed line (a line that is not a JSON object with a string
`row`) and every row is PASS, or NOT_APPLICABLE where declared (named in `not_applicable`,
not counted as a PASS). Otherwise the first of: ERROR, FAIL, INCOMPLETE, NOT_RUN. The scorer
exits 0 only for PASS, 1 for any other verdict, 2 if it cannot score (fail closed).

## 6. Check against v4

`declarations/np2-v4.decl.json` is the v4 case set (27 cases, 7 control rows; 3 repetitions
per injected row, C6i once, controls once; C6d may be NOT_APPLICABLE). The v4 receipt's
`controls` and `rows` scored with it and `F0 = true`: PASS, C6d NOT_APPLICABLE, 33 rows PASS.
The same runs with one C7c repetition removed: INCOMPLETE. The C3 control row (fixture
backend) is a receipt-level check and stays outside this scorer.
