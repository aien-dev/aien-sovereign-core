# WHOLE-SYSTEM-E2E, harness v1.4: E1 and CTRL-E1 install rows (RUN-4-INSTALL)

Tracking: aien-dev/aien-sovereign-core#421 (owning issue), aien-dev/aien-architecture#190 milestone R8 (engineering
readiness). Contract: `ACCEPTANCE_E2E.md` FROZEN at aien-architecture b2400fe, sha256
`9e3d5f4e23d65ac17be8f2f4ca48355f0aaa486f67ff5a8a8044dcdd646d34e8`. This file is committed before the harness change
that implements it. `ACCEPTANCE-v1.md` to `-v1.3.1-e3m.md` stay as written; RUN-1, RUN-2-GB10 and RUN-3-GB10 stay as recorded.

## Scope statement (read first)

**ENGINEERING READINESS ONLY.** A run whose E1 receipt says `signature_check: dry-run-key` is never a public PASS. The
public-release key ceremony (`docs/release/KEY_CEREMONY.md`) has not happened, `docs/release/allowed_signers` is a
placeholder, and a dry-run artifact is labelled `candidate = "NOT-RELEASABLE"`. The contract allows `dry-run-key` "stated as
a gap" when no offline key exists; this file states it. No outcome of RUN-4-INSTALL is a release claim, a RUN-3
(public-model) claim, or a program PASS. Every note about the run says so.

## Contract rows, as frozen (quoted, not changed)

E1 install: "install offline from the release artifact into a clean home and start". Receipt fields `artifact_sha256`,
`signature_check` (`verified`, `dry-run-key`, or `none`), `signer`, `files_verified` (count), `daemon_sha256`,
`cli_sha256`. Rule: "signature check `verified` or, when no offline key exists, `dry-run-key` stated as a gap; every file
in `release.toml` verified; daemon up". CTRL-E1: "an artifact with one byte changed installs nothing and names `checksum`
or `signature` as the reason".

## What changes, and what does not

New mode: `WSE2E_ARTIFACT=<path to sovereign-<os>-<arch>.tar.gz>`. Next to the archive sit `SHA256SUMS.txt` and
`SHA256SUMS.txt.sig` (the three assets `install.sh` release mode reads). Without the variable the harness behaves exactly
as v1.3.1 (cpu and gb10 modes unchanged, `selftest` still passes).

In artifact mode:

1. **No source build of the system under test.** The harness does not build `aien-cli`. The CLI and the daemon are the
   binaries the installer puts in the run's clean home. (The test-only ALLEN fixture subject is still built from this tree
   with `cargo test`, as in v1.2 and v1.3; that helper file is not part of the installed system.)
2. **E1.** The archive's own `install.sh` (taken out of the archive into the run folder, so no source tree is used) is run
   with `HOME=$RUN/home`, `AIEN_RELEASE_TAG` set, `AIEN_RELEASE_BASE_URL=file://<archive dir>`, and
   `AIEN_DRY_RUN_SIGNERS=<signers file>` (default `<archive dir>/dry-run-signers`, when that file exists), in an otherwise
   empty environment. Offline by construction: the only URL is `file://`, and `http_proxy`/`https_proxy`/`all_proxy` point at
   a closed local port so any network fetch would fail and be recorded. The installer runs before any daemon starts; the E1
   receipt is written at its usual place in the chain (after PRE and CTRL-E3a), so the chain order is unchanged.
3. **From then on** `cli()` and every daemon start use `$RUN/home/.local/bin/aien`. `daemon_sha256` and `cli_sha256` in
   every receipt are the sha256 of that installed file (the contract names one binary, `aien`, for both).
4. **E1 receipt.** The contract fields, from `install-receipt.json`: `artifact_sha256`; `signature_check` (installer class
   `release` is written `verified`, `dry-run-key` stays `dry-run-key`; any other class, including `internal-test-key`, is
   outside the contract set and makes E1 FAIL); `signer` (the signer key fingerprint the installer recorded); `files_verified`;
   `daemon_sha256`; `cli_sha256`. Extra fields: `installer_signature_check` (raw class), `dry_run`, `candidate`, `tag`,
   `release_id`, `install_receipt_sha256`, `installer_sha256`, `install_exit`, `install_log`, `files_recheck` (see rule),
   `offline_method`, `readiness_scope` (`ENGINEERING READINESS only, not a public PASS` whenever `signature_check` is not
   `verified`), `public_pass` (`false` unless `signature_check` is `verified` and `dry_run` is false).
5. **E1 rule (all must hold for PASS).** installer exit 0; `signature_check` is `verified` or `dry-run-key`;
   `files_verified` equals the number of `[files]` entries in the installed `release.toml` and every one of them was
   re-hashed by the harness itself and matches (`files_recheck`: `N of N`); the installed `bin/aien` sha256 equals the
   receipt's `cli_sha256`; the daemon started from it (PRE reached "daemon up"). A failed install aborts the chain: every later
   row is NOT_RUN and E1 is FAIL.
6. **CTRL-E1.** Three scratch copies of the artifact, each installed into its own empty home with the same installer:
   (a) one byte of the archive changed (offset 100, flipped); (b) one byte of `SHA256SUMS.txt` changed (its first
   character); (c) `SHA256SUMS.txt.sig` removed. PASS only if all three: installer exit non-zero, the refusal text names
   `checksum` (a) or `signature` (b, c), and nothing was installed (no `releases/`, no `installed.toml`, no
   `install-receipt.json`, no file in `.local/bin`). Receipt fields: `install_refused` (all three refused), `refusal_reason`
   (part a's refusal line), `nothing_installed`, `mutation` (file, offset, before and after byte for a and b), `parts` (each
   part: name, exit, refusal line, reason word, nothing_installed). The independent verifier reads `install_refused` and
   `refusal_reason`.
7. **Backend.** The backend rule is the mode's rule, unchanged. The aarch64 release build links the native Omega engine
   (its binary carries `OmegaGb10Backend` and no `CPU-reference` string, checked with `grep -a -c` on the installed binary),
   so an install run on that artifact is run with `WSE2E_BACKEND=gb10`: the installed daemon must print the native Omega
   GB10 backend line, the strict start line, every model call needs `gpu_executions >= 1`, and any fallback marks the row
   FAIL (`ACCEPTANCE-v1.3-gb10.md` G2 to G5). The zero-CUDA gate runs on the installed binary. In artifact mode `pins` is
   `null` (the engine pins belong to a source build) and each receipt names the archive instead. RUN-4-INSTALL therefore uses
   the GB10 chip and must not overlap another chip run.
8. Every other row (PRE, CTRL-E3a, E2, CTRL-E2, E3, E3m, E4, CTRL-E4, CTRL-E4b, E6, CTRL-E3b, NET, CTRL-E6, E5, CTRL-E5) is
   unchanged from v1.3.1, and the verifier is not changed by this file.

## Disclosure: the artifact is older than main

The dry-run artifact available when this was written is GitHub Actions run 38105672607 (workflow "Multi-OS Cross-Platform
Release", `workflow_dispatch` with `dry_run`, main 46c4e0d). Main has moved on since: sc#412 (objective identity on grants,
daemon re-reads the objective record after a restart) and sc#418 (start refusal, durable provenance default) are **not** in
that build. Rows that touch those behaviours (E6, CTRL-E6, the objective rows) may differ from RUN-3-GB10, and that would be
a finding about the artifact, not a harness defect. If a newer dry-run artifact exists when RUN-4-INSTALL is declared, it is
used and its run URL and main commit are recorded.

## Predictions for RUN-4-INSTALL (written before the run)

| row | prediction | why |
|---|---|---|
| E1 | PASS, `signature_check: dry-run-key`, `files_verified: 17`, `public_pass: false` | UNVERIFIED for the run (confidence high): the same artifact installed on this Spark with these values before this file was written |
| CTRL-E1 | PASS (three parts refused) | UNVERIFIED (confidence high): `scripts/test-install-release.sh` has the same negative rows and passes |
| PRE, CTRL-E3a, E2, CTRL-E2, E3, E3m, NET, CTRL-E3b, CTRL-E5 | PASS | UNVERIFIED (confidence moderate): PASS in RUN-3-GB10 on a source build; this binary is older (see disclosure) |
| E4 | FAIL | UNVERIFIED (confidence moderate to high): RUN-3-GB10 E4 failed because the daemon refused the model's edit on a `./report.md` path rule; the fix is being made on a separate lane and cannot be in this older artifact. A finding about the artifact build, not about install |
| CTRL-E4b | RECORDED, likely a path-rule refusal as in RUN-3-GB10 | same cause; not judged |
| E6, CTRL-E6, CTRL-E4 | PASS or FAIL as measured | the older build lacks sc#412 and sc#418 behaviour (E6 and the restart refusals are the rows most exposed) |
| CTRL-E4b | RECORDED | not judged |
| E5 | NOT_RUN | R7 |
| G4, G5 | `fallback_detected: false`, `gpu_executions >= 1` on model rows | as RUN-2/3-GB10; the engine in the artifact is whatever its build pinned (UNVERIFIED which) |
| independent verifier | agrees with the harness on E1 and CTRL-E1 | the verifier already reads exactly these fields (`tools/aien-verify` `e1_row`, `ctrl_e1`) |

## Rules of this run

One declared attempt, folder `runs/RUN-4-INSTALL-<utc>/`, run from a clean tree at the merged main commit that contains
v1.4, started detached, never killed, `quietlock check` first, the chip free of other runs, nothing under `runs/` edited
afterwards. The result note says ENGINEERING READINESS only, names the artifact run URL, main commit and sha256, and claims
no program PASS.
