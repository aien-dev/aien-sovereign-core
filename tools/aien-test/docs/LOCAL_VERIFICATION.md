# Local verification with aien-test

ADR 0033 Slices E and H: the planner, the trusted worker, the status and
verify commands, and the sandbox seam. Everything here runs on the Spark,
offline, with no outside service in the loop. GitHub only receives a commit
status when it can be reached; it is never asked for permission.

## Commands

| Command | What it does | Exit |
|---|---|---|
| `aien-test plan [--base REF] [--repo NAME] [--operator] [--allow-dirty]` | Which gates BASE..HEAD needs, by section, with the graph digest. BASE defaults to `origin/main`, then `main`. | 0 runnable, 2 NOT_RUN (unmapped path or removed gate), 4 BLOCKED_OPERATOR |
| `aien-test test changed [--base REF]` | Prints the plan, then runs the non-blocked selection through the normal runner. | the campaign verdict (PASS 0, FAIL 1, NOT_RUN 2, BLOCKED 3/4, MISSING 5) |
| `aien-test test crate:NAME` | The gates covering a workspace crate (from `cargo metadata`) and every crate that depends on it. | campaign verdict |
| `aien-test status [--state-dir DIR]` | Pools, GPU lock holder, aien-proof slots, quiet flag, worker heartbeat, pending triggers and outbox, newest bundle per repository. Read-only. | 0 |
| `aien-test verify COMMIT [--repo NAME] [--base REF] [--state-dir DIR] [--release]` | The independent check of a bundle, printed as the fixed report block below. | 0 only on `MERGE ELIGIBILITY: READY`, else 1 |
| `aien-test worker [--once REPO SHA] [--poll SECS] [--repos A,B] [--github] [--iterations N] [--state-dir DIR] [--key FILE] [--clones DIR] [--wt-root DIR] [--sandbox none\|user]` | The trusted worker: one head with `--once`, or the loop. | `--once`: the plan verdict; loop: 0 |
| `aien-test run GATE --sandbox user`, `aien-test test ... --sandbox user` | Any run, with every gate program started through the exec helper. | as before |

`--release` (run, test, verify): qualification mode. No cache reuse, the tree
must be clean even with `--allow-dirty`, and in `verify` a cached result of a
gate whose `cache_scope` is `hardware` cannot count.

## The plan

Selection is the set of gates whose watched paths (declared `inputs`, the
D3 implied inputs, the manifest file, the golden file) cover a changed path,
plus the always set, closed over dependents and then dependencies. Sections:

- **Required**: runs on this machine now.
- **Cached**: an evidence receipt already settles it (`plan` only; `verify` takes cached entries from the bundle).
- **Needs Spark**: pool `qemu`, or `arch`/`bare_metal` requirements.
- **Needs GB10**: pool `gb10`.
- **Advisory**: exactness ADVISORY; never required, never moves the verdict.
- **Blocked**: `PIPELINE_CONFIG_CHANGED` or `OPERATOR_STEP_PENDING`.

A changed path no gate covers is listed under `UNMAPPED` and the verdict is
`NOT_RUN UNMAPPED_PATH`. A gate that exists at BASE and is gone at HEAD is
`NOT_RUN GATE_REMOVED`. Both are refusals, not failures: nothing vouches for
that change.

### Trust rules A1 and A4

- **A1**: the always set (`.aien/always`), the unmapped allow-list
  (`.aien/unmapped-ok`) and the trusted keys (`.aien/verify.pub`) are read
  from BASE through git objects, never from the working tree or from HEAD. A
  change under test cannot widen its own allow-list or add its own key.
- **A4**: a change under `.aien/`, `tests/gates/`, `.github/workflows/`,
  `.forgejo/` or `tools/aien-test/` is a pipeline configuration change. The
  whole plan is `BLOCKED_OPERATOR PIPELINE_CONFIG_CHANGED` unless the operator
  passes `--operator`.

`graph_digest` is sha256 over the canonical `[{gate, check_id, depends_on}]`
of the selected gates, sorted by gate. `verify` recomputes it from the
manifests at COMMIT and rejects a bundle whose digest differs.

## Manifests: `run.exec`

`run.exec` is either a repo-relative path or a bare program name:

- A name **with a slash** (`tools/run.sh`) is resolved against the repository
  root, as before. Absolute paths and any `..` component are a manifest
  reject (`BAD_MANIFEST`, exit 1), never NOT_RUN and never a silent run.
- A name **without a slash** (`crumb`, `true`) is a program on the PATH. Under
  `--sandbox none` that is the runner's own PATH; under `--sandbox user` the
  bare name is handed to the exec helper, which resolves it on the trusted
  PATH it sets (`/var/lib/aien-run/cargo/bin:/usr/local/bin:/usr/bin:/bin`).
  The runner also looks the name up on its own PATH to hash the program into
  the receipt (`binary_sha256`); a name found on neither is
  `MISSING_IMPLEMENTATION`.

This is what lets the shared gate template say `exec: crumb` with
`args: [verify, .]`.

## The sandbox seam (C8)

`--sandbox user` rewrites every gate launch into

```
sudo -n /usr/local/sbin/aien-run-exec --cwd CWD --timeout SECS --profile host|qemu|gb10 [--env CARGO_TARGET_DIR=...] -- PROGRAM ARGS
```

Only `CARGO_TARGET_DIR` is passed through. The runner's own timeout and
kill rules are unchanged; a gb10 timeout stays advisory. `--sandbox none`
(the default) leaves the launch exactly as it was. The helper itself belongs
to the INFRA lane; when it is absent the sandbox integration test prints a
skip line and passes.

## The worker

For one head `REPO SHA`:

1. `git fetch --all` in the trusted clone `<clones>/<repo>` (default
   `~/workspace/<repo>`); its checkout is never touched.
2. `git worktree add --detach <wt-root>/<repo>/<sha>` (default
   `/var/lib/aien-run/wt`).
3. `crumb claim` on the worktree when it has a `.crumb` (a missing one is a
   journal note, not an error).
4. Plan against `origin/main` (a head that is `origin/main` is planned against
   its first parent).
5. Run the non-blocked selection with the pools and the sandbox; receipts go
   to `evidence/<repo>/<sha>/`.
6. Build the bundle: one `checks[]` entry per planned gate with status
   `run`, `cached`, `blocked` or `not_run`; `tree_digest` is sha256 of the
   commit's tree id; the executor is the key's fingerprint with class
   `spark`. Sign with `~/.aien/verify.key` and write
   `bundles/<repo>/<sha>/bundle.json` (canonical bytes). The bundle also
   carries a `plan` member (`verdict`, `reason`, `unmapped`, `removed`,
   `blocked`): the bundle algebra says PASS when no required check failed,
   which is vacuously true for an unmapped change with no checks; the plan
   verdict is what `verify` and the GitHub status gate on.
7. Heartbeat (`worker.heartbeat`), journal line (`journal.jsonl`), `crumb
   close`, remove the worktree.

With `--github` the commit status `AIEN Verification (Spark)` is posted:
`success` only when the bundle and the plan both say PASS, `failure` on
FAIL, `error` otherwise; the description names the bundle path. When `gh`
cannot reach GitHub the status is written to `outbox/<repo>.<sha>.json`, the
journal says `publication: pending`, and every later round flushes the
outbox first. GitHub being down never fails a local verification.

The loop (`worker` without `--once`) serves trigger files
`trigger/<repo>.<sha>` oldest first, then, with `--github`, the open pull
request heads and `main` of each `--repos` entry that has no bundle yet,
then sleeps `--poll` seconds. `--iterations N` bounds the rounds (tests).

## The verify report

```
AIEN LOCAL VERIFICATION
Repository: github.com/aien-dev/<repo>
Commit: <40 hex>
Changed files: N
Affected gates: N (names) | none
Executed gates: ...
Cached gates: ...
Failed gates: ...
Blocked gates: ...
Crumb status: CURRENT | STALE
Evidence status: VERIFIED | INVALID
Hardware status: QUALIFIED | NOT_REQUIRED | BLOCKED
Final verdict: PASS | FAIL | NOT_RUN | BLOCKED
Queue time: N ms | n/a
Build time: n/a
Test time: N ms | n/a
Total time: N ms | n/a
MERGE ELIGIBILITY: READY | BLOCKED
```

Lines after the block (`Evidence note`, `Crumb note`, `Verdict reason`,
`Unmapped`, `Problem`, `Bundle`) say why. Checks, in order: the bundle's
shape and signature against the keys at BASE (several keys allowed, one hex
per line, `#` comments; the key the bundle names must be one of them, else
`UNKNOWN_KEY`), repository, commit, executor fingerprint, every required
gate present, the verdict algebra; then the graph digest against a fresh
plan; then every `run` entry's receipt must exist under
`evidence/<repo>/<commit>/` with the same gate, commit and verdict (an
altered receipt no longer matches its digest and is reported missing); then
the plan judge. Crumb status comes from `crumb verify .` in a temporary
detached worktree at COMMIT, removed afterwards. Queue time is the sum of
`pool_wait_ms`, test time the sum of `duration_ms` over the executed
receipts, total time the bundle's `finished_utc - started_utc`; build time
is not measured yet.

`READY` requires: evidence VERIFIED, final verdict PASS, crumb CURRENT, no
unmapped path, no blocked gate, no problem line.

## File layout

```
~/.aien/verify.key                       the worker's signing seed, mode 0600
<repo>/.aien/verify.pub                  trusted public keys, read at BASE
<repo>/.aien/always                      gates that always run, read at BASE
<repo>/.aien/unmapped-ok                 paths no gate needs to cover, read at BASE
~/.local/state/aien-local-ci/
  evidence/<repo>/<sha>/<digest>.json    receipts of that run
  bundles/<repo>/<sha>/bundle.json       the signed bundle
  trigger/<repo>.<sha>                   a run request (removed when served)
  outbox/<repo>.<sha>.json               a commit status waiting for GitHub
  journal.jsonl                          one line per finished head
  worker.heartbeat                       pid, phase, repo, sha, utc
/var/lib/aien-run/wt/<repo>/<sha>        the worker's detached worktree
/usr/local/sbin/aien-run-exec            the exec helper (INFRA)
```

`<repo>` is the last segment of the repository name
(`github.com/aien-dev/aien-sovereign-core` gives `aien-sovereign-core`).

## Tests

`cargo test -p aien-test` covers the planner (selection, sections, A1, A4,
removed gates, verdict algebra), the sandbox rewrite, status parsing, and in
`tests/local_ci.rs` the pipeline end to end on a throwaway repository: a
worker run that `verify` reports READY, then the refusals: an altered
receipt, a bad signature, a bundle presented for another commit, a required
check left out of a re-signed bundle, a hardware-scoped cached result under
`--release`, an executor whose key is not in `verify.pub`, a signing key
another uid cannot read, GitHub down then back (fake `gh`), a trigger file
served by the loop, a bare `exec: true` running and escaping exec paths
refused, and the user sandbox when the helper is installed.
