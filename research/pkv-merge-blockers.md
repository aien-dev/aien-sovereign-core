# What blocks the persistent-KV PRs (#487 to #493) from merging

Ticket: aien-spark/aien-sovereign-core#495 (T1 of map #494). Researched 2026-10-13/14 in a fresh clone.
Tags: VERIFIED = seen directly (source named). INFERRED = reasoned from stated facts. UNKNOWN = not established.

## Short answer

1. VERIFIED: the only required check on `main` is `Workspace Verification & Invariant Audit`
   (`gh api repos/aien-spark/aien-sovereign-core/branches/main/protection`: one context, `strict:false`,
   0 required approvals, `enforce_admins:true`, no rulesets: `gh api .../rulesets` returned `[]`).
2. VERIFIED: that check PASSES on #487 and #488 (the two PRs that target `main`). No required check
   is failing anywhere. (`gh pr view <n> --json statusCheckRollup`)
3. VERIFIED: that check never RAN on #489 to #493. Their base is another PR branch, and `ci.yml` only triggers
   on `pull_request: branches: [main]` (`.github/workflows/ci.yml` lines 4-7). So once they are retargeted to
   `main`, the required check has to run and pass: its result for them is UNKNOWN today.
4. VERIFIED: the one check that fails on all seven PRs is `AIEN Verification`, step "Crumbs must be exactly what
   the compiler produces now": `crumb verify .` reports stale Crumbs. It is NOT required today
   (header of `.github/workflows/aien-verification.yml`: "NOT a required check yet"; protection lists only one context).
   It still shows red, and phase 3 will make it required.
5. VERIFIED: main itself is clean (`crumb verify` OK, 17 crumbs current; AIEN Verification green on main run 38498550909).
   So the staleness comes from the PR contents, and it is fixed by running `crumb compile` and committing the result.

## Base / stack chain (VERIFIED: baseRefName / headRefName from `gh pr view`)

```
main
 +-- #487  fix/preempt-resume-reemit            (stands alone)
 +-- #488  pkv/484-persistent-kv
      +-- #489  pkv/c4-host-fs                  base = #488 branch
           +-- #490  pkv/pr3-worker-review      base = #489 branch
                +-- #491  pkv/pr4-runtime       base = #490 branch
                     +-- #492  pkv/pr5-restart  base = #491 branch
                          +-- #493  pkv/pr6-concurrency-bench  base = #492 branch
```

All seven are OPEN, not draft, `mergeable: MERGEABLE`, `mergeStateStatus: UNSTABLE` (UNSTABLE = a non-required check is
failing). Commits are strictly linear: each PR contains its base's full history (`git rev-list` ahead/behind: 0 behind
its base for 489 to 493). All seven were branched from main commit `f560be4f`; main has since moved one commit
(`b9b52f9c`, docs link rewrite, 39 files).

## Per-PR table

| PR | Base | Required check (`Workspace Verification & Invariant Audit`) | AIEN Verification (not required) | Stale Crumbs (reproduced locally) | Other |
|----|------|------|------|------|------|
| 487 | main | PASS | FAIL | root `.` | Standalone scheduler fix |
| 488 | main | PASS | FAIL | root `.` | Bottom of stack |
| 489 | #488 branch | NOT RUN (base is not main) | FAIL | root `.` | |
| 490 | #489 branch | NOT RUN | FAIL | root `.` | |
| 491 | #490 branch | NOT RUN | FAIL | root `.` | |
| 492 | #491 branch | NOT RUN | FAIL | `.`, `crates/aien-cli`, `crates/aien-cli/src` | |
| 493 | #492 branch | NOT RUN | FAIL | `.`, `crates/aien-cli`, `crates/aien-cli/src` | Carries its own copy of the #487 fix |

Also "Release gate dry run" passes on all seven. "Linked composition tests" passes on 487/488 and did not run on 489-493 (same base rule).
Evidence: CI log lines `STALE .` / `crumb verify: FAIL: N of 17 crumbs stale; run: crumb compile` in runs
38498342744 (487), 38498347334 (488), 38498351194 (489), 38498356700 (490), 38498362472 (491), 38498368504 (492),
38498373832 (493), step "Crumbs must be exactly what the compiler produces now".

Local reproduction (VERIFIED): built the pinned crumb tool (aien-architecture @ 94fffbcf, same pin as CI), ran
`crumb verify .` on each PR head and on each PR head merged with current main: same stale list as CI in every case.
Running `crumb compile .` on #487, #488 and #492 modified only `.crumb` files (root only for 487/488; plus the two
`aien-cli` crumbs for 492) and `crumb verify .` then reported 17 crumbs current. INFERRED for 489-491 and 493 (same
mechanism, same stale lists as CI; compile not run on those).

Why every PR goes stale: INFERRED. The root `.crumb` holds a digest over the whole tree (compile is "bottom-up
(Merkle digests)" per the tool's own usage text), so any PR that changes any file leaves the root digest outdated unless
it also recompiles. The PRs changed code but did not commit a recompile.

## The #487 overlap (VERIFIED, merge tests run locally)

- #487 is NOT inside the #488 to #493 stack (`git merge-base --is-ancestor pr/487 pr/493` false).
- #493 contains its own commit of the same fix (`a71572da` vs #487's `2512afda`): the `prefill_input` change in
  `crates/aien-scheduler/src/sequence.rs` is textually the same. Different commit fingerprints, but no conflict:
  merging #487 into each of #488 to #493 gave 0 conflicts, and rebasing the whole #493 history onto #487 gave 0 conflicts.
- So merge order between #487 and the stack does not matter for conflicts. Merge #487 first anyway (smaller, standalone).

## Repo merge settings (VERIFIED, `gh api repos/aien-spark/aien-sovereign-core`)

Squash, merge commit and rebase merge are all allowed; "delete branch on merge" is ON; auto-merge is OFF.
INFERRED from GitHub's documented behaviour (not tested here): when a PR's branch is deleted on merge, GitHub
retargets PRs that were based on it to the base of the merged PR. That only works cleanly if the merged PR's commits stay
in history (merge commit or rebase merge). With squash, the next PR still lists the old commits and needs a manual
rebase onto the new main.

## Exact fix list (do in this order; nothing here has been done)

Pre-step. For every PR below, after rebasing or retargeting, run the pinned crumb tool `crumb compile .` at the repo
root and commit the changed `.crumb` files. (Pinned tool: aien-architecture @ 94fffbcf88492c5a1c54211b5126c8a6c370a98d,
`make -C tools/crumb crumb`.) Because the root digest covers the whole tree, the recompile must be the LAST commit
on each branch, redone after any rebase or after main moves.

1. #487: `crumb compile .`, commit `.crumb`, push. Required check already passes. Merge into main.
2. #488: rebase/merge latest main (one new docs commit, no conflicts: VERIFIED merge test), `crumb compile .`, commit `.crumb`, push.
   Required check already passes. Merge into main with a merge commit or rebase merge (not squash), so its commits stay in main.
3. #489: retarget base to `main` (or let GitHub do it after #488's branch is deleted). Rebase onto main, `crumb compile .`, commit, push.
   Wait for `Workspace Verification & Invariant Audit` to run and pass (UNKNOWN today). Merge.
4. #490: same as step 3 (retarget to main, rebase, compile, wait for required check, merge).
5. #491: same.
6. #492: same, and the compile also rewrites `crates/aien-cli/.crumb` and `crates/aien-cli/src/.crumb`; commit all three.
7. #493: same as 492 (three crumbs). Its duplicate copy of the #487 fix rebases cleanly (VERIFIED), so no manual conflict work.

Shortcut if Drake prefers fewer rounds: retarget #489 to #493 all to `main` now. Their diffs then show the whole stack
so far, the required check runs on each, and each only needs a final rebase plus `crumb compile` as its turn comes.
(INFERRED: retargeting does not alter commits; the cost is that each later PR's page shows earlier PRs' changes until they merge.)

## UNKNOWN

- Whether `Workspace Verification & Invariant Audit` (build, tests, clippy etc.) passes for #489 to #493. It has never run for them
  because of the base-branch rule; no heavy build was run here.
- Whether phase 3 (making AIEN Verification required) is scheduled; the workflow header says it needs Drake's agreement.
- AIEN Verification steps after the crumb step were skipped in the failing runs; unknown if they would pass (the repo may
  not even be enrolled: the log was not checked for `.aien/verify.pub` on base).
