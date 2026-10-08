# Release build recipe and checks

One recipe builds the release and one script checks it. The workflow, a person reproducing a digest and the
checks all use the same commands.

## Build

`scripts/release-build.sh` (needs the pinned toolchain from `rust-toolchain.toml`, `cargo fetch --locked` into the
CARGO_HOME in use, and for the native engine `AIEN_OMEGA_DIR` at `omega.lock` and `AIEN_PHYSICS_DIR` at omega's
`physics.lock`; `AIEN_DEV_FALLBACK` and `AIEN_FORCE_CPU_STUB` unset, no mojo on PATH):

1. `scripts/repro-build.sh -p spark-cockpit-rs -p spark-inquisitor -p cortex-encoder-rs -p cortex-rs -p spark-supervisor -p spark-debugger`
2. `scripts/repro-build.sh -p aien-cli`

`repro-build.sh` is `cargo build --release --locked` with `--remap-path-prefix` for CARGO_HOME and the repo root.
Cargo unifies features across all `-p` packages in one command, so a binary's digest depends on its group
(`F1-binary-digest-difference.md`: one seven-package command gives a different aien-cli). Packages are therefore built
in fixed groups and `aien-cli` alone and last. That aien-cli digest equals CAND-3's `aien-cli-native-release`
(F1, build A). The CAND-3 manifest `[build]` line lists `-p aien-cli -p aien-proof -p aien-test` in one command;
whether that group gives the same aien-cli digest as `-p aien-cli` alone is UNVERIFIED here, and the release does not
package aien-proof or aien-test.

## Package and check

- `scripts/package-release.sh <out.tar.gz>` runs the release gate itself and writes the candidate id and `[model]`
  table from `release/candidate.toml` into `release.toml`. A failing gate means no package (`AIEN_DRY_RUN=1` only
  labels it `NOT-RELEASABLE`, which the checks reject).
- `scripts/check-release-candidate.sh` (tree): candidate named, `omega.lock` equals `omega-commit`, `Cargo.lock`
  holds exactly the `[pins]` revisions (aien-protocols, crumb-spec, spark-crumbs), `[model]` complete.
- `scripts/check-release-candidate.sh --package A.tar.gz --native`: candidate id, `[model]` table (model id,
  safetensors, tokenizer, config, oracle fixture digests), `aien-cli-sha256` equals the digest of `bin/aien`, every
  listed file, no unlisted file; `--native` is required (linux aarch64, omega linked): the digest must equal
  `[executables] aien-cli-native-release`, and each helper binary must equal its `sc-<name>` entry. A package without `--native` is refused (no candidate digest for that kind); the release matrix builds only the native target.
- `scripts/verify-release-assets.sh <dir> <allowed_signers>`: signature on `SHA256SUMS.txt` valid for the pinned
  signer, every listed file matches, every `sovereign-*` archive is covered.
- `scripts/test-release-checks.sh` (also in CI) proves each of these rejects: wrong omega.lock or Cargo.lock pins,
  executable digest mismatch, model input mismatch, missing, corrupt or foreign signature, tampered files.

## Release workflow modes

- Tag push `v*`: build, check, sign with the `AIEN_RELEASE_SIGNING_KEY` secret (job fails if absent), verify against
  `docs/release/allowed_signers`, publish. Until the public-release key steps in `RELEASE_SIGNING.md` are done, what this
  publishes is an internal test release signed with the old key, never a public release.
- Manual run (`workflow_dispatch`): input `dry_run`, default true. Builds, packages, checks, signs with a throwaway key
  generated in the job (labelled in `DRY-RUN-NOT-A-RELEASE.txt`), verifies, uploads workflow artifacts only. Gate or
  package-check failures are recorded in `DRY-RUN-STATUS-<target>.txt` instead of stopping the run. It never creates a
  release. With `dry_run` set to false a manual run publishes like a tag push.
- Pull requests (`release-dry-run.yml`): the publish guard, `scripts/test-release-gate-wiring.sh` and the refusal suite
  (`scripts/test-release-real-tree.sh`, run on a fixture tree pinned to the candidate) block. The tree gate on the PR's
  own revision only reports "IS / is NOT releasable as <candidate>" in the step summary: main may move ahead of the
  candidate's omega-commit. A release must be cut from a commit whose tree passes the gate; the tag-push path enforces
  it, and `test-release-gate-wiring.sh` fails if that step ever becomes non-blocking (only the manual dry run may
  continue past it).
