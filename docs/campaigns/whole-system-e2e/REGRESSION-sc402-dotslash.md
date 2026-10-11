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
