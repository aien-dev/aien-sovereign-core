# Regression evidence, sc#402 (a leading `./` names the same workspace file), Lane A2, 2026-10-11

Pre-registered before the code change. Results are filled in below the plan after the checks; the plan is not edited.

## The failing scenario (RUN-3-GB10-20261011T045450Z, row E4)

After `remember` succeeded, the daemon was killed (SIGKILL) and restarted, and the second objective asked the model to
edit `report.md`. Attempts 2 and 3 answered `filename: ./report.md` and were refused by the daemon's proposal reader:
`path "./report.md" is not a plain relative path` (`RESULT-RUN-3-GB10.md`, E4 and CTRL-E4b rows). `./report.md` and
`report.md` name the same file in the workspace, so this is a product defect, not a safety refusal.

## The rule (coordinator ruling, 2026-10-11)

One shared function, `destination::strip_leading_curdir`, removes leading `./` segments, and nothing else, where a path
enters the daemon from text:

1. the model's proposal (`spine::check_file_proposal`, the `filename:` line, which every reader of a reply uses: the
   skill, the AEGIS contract, the authorization and the write), and
2. the words of a goal (`destination` word cleaning, which feeds the named destination and named inputs that
   `classify_named` and the input collector then check).

The existing strict checks then run unchanged on the stripped path: `spine::check_relative_path` (no absolute path,
no `~`, no `..`, no empty or `.` segment, letters, digits and `._-/` only, at most 255 bytes) and, for grants,
`effects::confine_shape`. So absolute paths, any `..` segment (also `./../x`), empty segments, `~`, backslashes, NUL
and anything that escapes the workspace stay refused exactly as before.

Decisions recorded with the rule:

- Inner `./` segments are NOT normalised: `a/./b` stays refused by the spine. `.` and `./` (empty after stripping) stay
  refused. `.//x` strips to `/x`, which is absolute and stays refused.
- The grant and effect layer (`effects::confine_shape`, `confine_target`, `open_confined`) is left strict: it is a
  backstop that accepts only the one canonical spelling, so an authorization record never holds two spellings of one
  file. Its existing test that refuses a grant for `./N.md` stays as it is. Grants are built from the proposal's path,
  which is now already canonical, so the backstop is never reached with `./` from a model reply.

## Plan

| check | how | expected |
|---|---|---|
| new tests red on main | the new unit tests in `spine.rs` and `destination.rs`, built against main 8c09217 with the test commit only | `./report.md` proposal refused (test fails) |
| new tests green | `cargo test -p aien-runtime -p aien-cli` (linked compose archive) | all pass, including: `./report.md` -> `report.md`; `././a/b.md` -> `a/b.md`; refused: `../x`, `./../x`, `/x`, `a/../b`, `a/./b`, `.`, `./`, empty, `~/x`, `.//x`; an edit reply naming `./report.md` merges into `report.md`; a goal naming `./report.md` names `report.md` |
| grant backstop unchanged | existing `grants_are_confined_and_the_approved_kind_is_reserved` | passes unchanged (`./N.md` grant refused) |
| lint | `cargo clippy -p aien-runtime -p aien-cli --all-targets -- -D warnings`, `cargo fmt --check` | clean |
| RUN-1 verdict unchanged | `tools/aien-verify` on a scratch copy of `runs/RUN-1-20261010T154724Z` | `verdict_id` as recorded in its `VERIFIER_RECEIPT.json` |

## Results (2026-10-11, branch e2e/r6-run5)

| check | result |
|---|---|
| new tests red on main | main 8c09217 plus the test commit only: does not compile (`strip_leading_curdir` missing, 10 errors). With an identity stand-in for that function (the behaviour of main): 4 of the 5 new tests FAIL (`leading_dot_slash_names_the_bare_path`, `an_edit_reply_naming_dot_slash_merges_into_the_named_file`, `strip_leading_curdir_removes_only_leading_dot_slash`, `a_goal_naming_dot_slash_names_the_bare_path`); `every_other_form_is_refused_as_before` passes, as it must on main |
| new tests green | the 5 new tests pass |
| `cargo test -p aien-runtime -p aien-cli -- --test-threads=1` (linked compose archive sha256 3eb20f5e...) | 617 passed, 0 failed, 23 ignored |
| grant backstop unchanged | `grants_are_confined_and_the_approved_kind_is_reserved` passes unchanged (`./N.md` grant still refused) |
| lint | `cargo clippy -p aien-runtime -p aien-cli --all-targets -- -D warnings` clean; `cargo fmt --check` clean |
| RUN-1 verdict unchanged | `tools/aien-verify` on a scratch copy of `runs/RUN-1-20261010T154724Z`: `verdict_id a50fccb63363ef9224a19bd8bf4f249754abf5d88667ba0fd6ce93dfb8d63d80`, chain_unbroken true (as recorded) |

Limits: unit level only; the chip path is exercised by the next declared attempt (RUN-5-GB10). The model may still answer
in other forms that stay refused (for example an absolute path); this repair covers only a leading `./`. Attempt 1 of
RUN-3 E4 was refused for another reason ("edit reply changes nothing in the file"), which this change does not touch.
`runs/` was not edited.
