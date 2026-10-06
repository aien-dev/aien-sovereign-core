# Receipt v2: atomic upgrade, one-version rollback, reproducible packages, 2026-10-06 (NEXT-PHASE-4 cut 2)

Supersedes the v1 receipt (`2026-10-06-install-upgrade-rollback.md`, kept unchanged as the record of the gaps this cut closes).
Machine: NVIDIA DGX Spark (aarch64), host CPU only. No GPU or chip work was run. Everything ran in a scratch prefix,
`env -i`, empty HOME, `AIEN_INSTALL_NO_PROFILE=1`, no sudo. Evidence: `np4-v2-evidence/demo.sh` and `np4-v2-evidence/demo.log`
(scratch path shortened to `<scratch>` in the log).

## How the four releases in the demo were made (honest labelling)

- relA: real build of CAND-3 pins, `aien-cli` = 3a17ee79... (the CAND-3 `aien-cli-native-release` digest), candidate CAND-3.
- relB: real rebuild after the one-line `--version` change (`+np4-upgrade`), `aien-cli` = e789c04d..., candidate CAND-3 (log build-v2 and
  the sha256 of the build tree binary match e789c04d, checked in this job).
- relC: the relB binary with `+np4-upgrade` byte-replaced by `+np4-third-r` (same length), repackaged with `scripts/package-release.sh`,
  `aien-cli` = 01cac51b... It is a test fixture, not a build of any source. Candidate id CAND-3.
- relOld: the relA binary repackaged with candidate id CAND-2, to exercise the downgrade guard.
- All signed with a throwaway key (not the pinned release key): NOT_RUN for a release signed by the pinned key.

## Steps (demo.log)

| Step | Result | Evidence |
|---|---|---|
| 1 install relA | PASS | "Verifying every package file against release.toml", "Verifying the staged copy", live release 3a17ee79a238; seven symlinks in bin point at `releases/current/bin/...`; `installed.toml` has current=3a17ee79a238, previous empty |
| 2 execute | PASS | `aien --version` from empty dir and empty PATH works; `zero-cuda-gate.sh` ZERO_CUDA PASS. `aien doctor` and any GPU probe: NOT_RUN |
| 3 upgrade to relB | PASS | live e789c04d55b1, previous 3a17ee79a238, `--version` shows `+np4-upgrade`; user edit in operator.toml survived (sha256 check OK) |
| 3b downgrade guard (relOld, CAND-2) | PASS | "not known to be newer than installed 'CAND-3' ... Nothing was installed."; live unchanged |
| 4 rollback | PASS | `--rollback` gave live 3a17ee79a238 (was e789c04d55b1), binary sha256 equals step 1, `last-action = "rollback"`, operator.toml unchanged |
| 5 interrupted upgrade to relC, kill -9 at mid-copy, after-copy, before-swap | PASS | See below |
| 6 pinned-key refusal (no signer override) | PASS | "SHA256SUMS.txt signature is NOT valid. Nothing was installed." |
| `scripts/test-install-release.sh` | PASS | re-run in this job; includes its own kill -9 points |

Step 5 detail. This replaces the first demo (demo-v2), whose interrupt section was invalid: the second release was already
staged so the installer never reached the pause point ("NEVER PAUSED") and the kills hit nothing. The fixed run uses a never-before-installed
release (relC), and each kill landed (the "paused at <point>" line was seen before the kill). After each kill the live release stayed
3a17ee79a238, the installed `aien` stayed 3a17ee79a238 and ran. Leftovers: a `.stage.01cac51b2bc9.<pid>` directory after mid-copy and
after-copy (not live, not a release name); after before-swap a complete digest-named directory `01cac51b2bc9` that is staged but not
live. Rerunning the install of relC made 01cac51b2bc9 live, `aien --version` shows `+np4-third-r`, and the release list was
01cac51b2bc9, 3a17ee79a238, current (the old e789 was rotated out as designed: one previous release kept). Not tested:
a kill during the single rename itself (kill after-swap is a pause point in install.sh but was not in this demo).

## F1: the two `aien-cli` digests (from v1 receipt and build logs, nothing new measured)

- `3a17ee79a238...` is `repro-build.sh -p aien-cli` built on its own. It equals the CAND-3 manifest digest `aien-cli-native-release`.
  `package-release.sh` now packages this one, and its build recipe in release.yml builds per package, last, `aien-cli` alone.
- `ccecf9a13ef2c7a1...` is `aien-cli` taken from a single cargo command that builds all seven packaged crates together
  (np4-logs/build1.log line 876). Same size, 6,399,222 differing bytes, not the CAND-3 digest. Cause UNVERIFIED (v1 guessed
  dependency feature unification; not investigated).

## UNVERIFIED / NOT_RUN

- `.github/workflows/release.yml`: YAML parses (PyYAML safe_load run from a scratch interpreter outside the repo, nothing added to the repo: jobs build-release and
  publish-release, triggers push and workflow_dispatch). The workflow itself was NEVER RUN, so the CI build digest equals CAND-3: UNVERIFIED.
  actionlint not available: NOT_RUN.
- macOS: `mv -fh` path in install.sh and GNU-tar handling (`gtar`) in package-release.sh: UNVERIFIED, run on Linux only.
- Reproducible packaging: two packagings were equal (sha256 b0aa34a6...) per the previous worker; not re-run in this job.
- Release signed by the pinned AIEN release key: NOT_RUN (key unavailable).
- Kill during the final rename (after-swap): NOT_RUN. Power loss or filesystem-level failure: NOT_RUN.
- Published linux-aarch64 asset on the Spark: NOT_RUN (inherited from the 2026-10-04 receipt).
- Other six binaries were verified only through `release.toml` file hashes, not against CAND-3 digests (only `aien-cli` has one).
