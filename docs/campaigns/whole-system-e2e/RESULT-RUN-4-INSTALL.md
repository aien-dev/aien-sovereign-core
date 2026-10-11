# WHOLE-SYSTEM-E2E, RUN-4-INSTALL (declared, installed from a dry-run artifact): result

**ENGINEERING READINESS ONLY. Never a public PASS.** The artifact was signed with a throwaway dry-run key
(`signature_check: dry-run-key`, `candidate = "NOT-RELEASABLE"`). The public-release key ceremony has not happened.
No program PASS, no release claim, no RUN-3 (public model) claim. Owning issue: aien-sovereign-core#421; harness PR #422.

Rows: `ACCEPTANCE-v1.4-e1.md` over v1.3.1, v1.3-gb10, v1.3-ctrl-e5, v1.2. Contract FROZEN at aien-architecture b2400fe,
sha256 `9e3d5f4e23d65ac17be8f2f4ca48355f0aaa486f67ff5a8a8044dcdd646d34e8`.

## What ran

- Harness v1.4 at merged main `cb15198f973f336ba561f62cef79d6f05dad9c52` (includes sc#412, #418, #425, #422), `WSE2E_BACKEND=gb10`,
  started detached 2026-10-11 06:20:29 UTC, rows 06:21:37 to 06:24:50 UTC, exit 0, never killed. Host `spark-b87b`.
  Run folder `runs/RUN-4-INSTALL-20261011T062029Z/`.
- Artifact: GitHub Actions run 38117352791 (`release.yml`, `workflow_dispatch`, `dry_run=true`, built from main
  `cb15198f973f336ba561f62cef79d6f05dad9c52`; publish job skipped). Archive `sovereign-linux-aarch64.tar.gz` sha256
  `a6409e919d81e615dd491951f40b7ed295244c6d09dee2d13a7758513fe61ef2`. Installed offline (file://, dead proxies) with the
  archive's own `install.sh` into the run's clean HOME. **Every row after E1 ran on the installed `aien`** (daemon and CLI);
  only the test-only ALLEN fixture subject is built from this tree.
- GB10: the installed daemon printed the native Omega GB10 backend line on all 8 starts; `fallback_detected: false`; zero-CUDA
  gate exit 0 on the installed binary. Predecessor artifact (run 38105672607, main 46c4e0d) was superseded before the run.

## Rows as measured (OBSERVED)

| row | harness | verifier | note |
|---|---|---|---|
| PRE, CTRL-E3a | PASS | PASS (supp.) | NoDesk refusal before serving |
| E1 | PASS | PASS | `dry-run-key`, files_verified 17 (harness re-hash 17 of 17), installed bin sha equals receipt cli/daemon sha |
| E2, CTRL-E2 | PASS | PASS | |
| E3 | PASS | PASS | gpu_executions 82014, fallback 0 |
| E3m | PASS | PASS (supp.) | |
| E4 | **FAIL** | FAIL | after SIGKILL and restart the second objective did not commit: "edit reply changes nothing in the file" (model edit refused by the daemon) |
| E6 | PASS | PASS | |
| CTRL-E3b, NET | PASS | PASS (supp.) | |
| CTRL-E4b | RECORDED | n/a | same refusal as E4; not judged |
| CTRL-E6 | PASS | PASS | `E_MARK_TRUNCATED` |
| CTRL-E4 | PASS (harness) | NOT_RUN | verifier: E4 showed no recall, so the control proves store loss only; recorded `status_disagreement`, as in RUN-3-GB10 |
| E5 | NOT_RUN | NOT_RUN | R7 |
| CTRL-E1 | PASS | PASS | three copies refused, nothing installed: archive byte (names `checksum`), SHA256SUMS byte (`signature`), signature removed (`signature`) |
| CTRL-E5 | PASS | PASS | |

Verdict id (verifier, final): `f09e5f05d68f62337913ca3f09ed3c1815ce86307bf8e866dda778d0ced9f4e4`. Run verdict **FAIL** (E4).

## What this shows, and what it does not

- E1 and CTRL-E1 are measured end to end: an offline install of a release artifact into a clean home works, refuses the
  three tampered forms, and the whole chain runs on the installed binaries. That is the R8 engineering-readiness question.
- The path-rule fix (#425) was in this build, so the RUN-3-GB10 `./report.md` refusal is not the E4 cause here; the refusal text
  is "edit reply changes nothing in the file" (the 1B model re-emitting the file unchanged). Cause beyond that text: UNVERIFIED.
- Not claimed: a public release, a verified (non dry-run) signature, the public model (RUN-3), cross-machine agreement (E5/R7),
  or any program PASS.

`sha256sum -c chain/SHA256SUMS` from the committed tree: checked at commit time (see PR).
