# WHOLE-SYSTEM-E2E, RUN-1: second-machine verdict (R7)

Milestone R7 of aien-architecture#190: an independent machine rebuilds the verifier from source and judges the same run
folder; the two machines must produce the same `verdict_id`. Run folder: `runs/RUN-1-20261010T154724Z/` as merged in
sc#395 (main `b097a56e0434755c5d51d091bbd3307e734e03ee`), untouched. Contract digest
`9e3d5f4e23d65ac17be8f2f4ca48355f0aaa486f67ff5a8a8044dcdd646d34e8`. First-machine result: `RESULT-RUN-1.md`.
Raw outputs: `r7/`. 2026-10-10, about 16:45 to 17:00 UTC. **No PASS claim for the program and no claim that AIEN is
complete.**

## What was done (OBSERVED)

1. **Second machine.** The MacBook (`Macbook.local`, arm64, macOS 26.6.2) ran `r7-mac.sh` over ssh: fresh clone of
   aien-sovereign-core at `b097a56e`, verifier built with `rustc 1.98.1` alone from `tools/aien-verify/src/main.rs`
   (source sha256 `2ec847c3...`, binary sha256 `9fa81199...`), self-test ok, `shasum -a 256 -c chain/SHA256SUMS` 0 bad
   lines. Nothing on the Spark was read. Verifier run with the Spark's `base_verdict_id`
   `a50fccb63363ef9224a19bd8bf4f249754abf5d88667ba0fd6ce93dfb8d63d80` as `--peer-verdict-id`.
   Output `r7/RUN-1-20261010T154724Z.mac.verdict.txt`.
2. **First machine, peer round.** The Spark rebuilt the verifier the same way from the same merged commit (same source
   sha256 `2ec847c3...`, binary sha256 `136c911d...`, a different binary because a different toolchain and platform) and
   reran with the Mac's `base_verdict_id` as `--peer-verdict-id`. Output `r7/RUN-1-20261010T154724Z.spark-peer.verdict.txt`.
3. **CTRL-E5 (one byte changed).** On the Mac, the run folder was copied to a scratch path and one byte of
   `artifacts/E3-report.md` was overwritten (`before_sha256 15bdec51...`, `after_sha256 ce0452d7...`, `cmp -l` reports
   1 differing byte). The manifest check reported 1 bad line (`./artifacts/E3-report.md: FAILED`) and the verifier was
   rerun over the copy. The same mutation was repeated on a Spark copy as a cross-check. Outputs
   `r7/CTRL-E5.mac.mutated-copy.txt` and `r7/CTRL-E5.spark.mutated-copy.txt`. The committed run folder was not touched
   (`git status --short --ignored` on `runs/` is empty).

## Results (OBSERVED from the verifier outputs)

| item | Mac | Spark (peer round) |
|---|---|---|
| `base_verdict_id` | `a50fccb6...` | `a50fccb6...` |
| `verdict_id` | `437467db4149b013738578c10ef68b7168dfb95feae8dea882de17a6118fc5fd` | `437467db4149b013738578c10ef68b7168dfb95feae8dea882de17a6118fc5fd` |
| `chain_unbroken` | true | true |
| E2 E3 E4 E6, CTRL-E2 CTRL-E3 CTRL-E4 CTRL-E6, PRE, E3m | PASS | PASS |
| E5 | PASS: "chain unbroken and the peer machine's base verdict id equals this machine's" | PASS, same reason |
| E1, CTRL-E1 | NOT_RUN (R8) | NOT_RUN |
| CTRL-E5 (verifier row) | NOT_RUN: "receipt lacks the facts this control needs" | NOT_RUN |

Every row and every id line is identical between the two machines (`diff` of the row lines is empty).

Mutated copy, both machines: E3 FAIL `DIGEST_MISMATCH: artifacts/E3-report.md hashes to ce0452d7... but the receipt
content_sha256 is 15bdec51...`; `chain_unbroken false`; E5 FAIL "chain is not unbroken"; `verdict_id` changed to
`ee7d7e8d...`. Rows and ids of the mutated copy are also identical between the machines.

## What follows (INFERRED, by the stated rules)

1. E5 of the frozen contract ("verifier reports every row and the chain unbroken; on a second machine, same
   `verdict_id`") is met for RUN-1: two machines, two independently built verifiers from the same source, same rows,
   same `verdict_id`.
2. The CTRL-E5 behaviour the contract asks for ("one output file with one byte changed is rejected by the verifier,
   `result_digest` mismatch") is shown on the second machine: the mutation is rejected on a digest mismatch, the chain
   breaks and the verdict changes.

## Not claimed

- The verifier's own `CTRL-E5` row stays NOT_RUN. The harness writes no CTRL-E5 receipt (`mutated_output_rejected`,
  `verifier_reason`) into the chain; the control above was performed by hand on a copy of the run folder after the chain
  closed, and its evidence is the raw outputs under `r7/`, not a chain receipt. A later harness version can add that
  receipt so the verifier judges the row itself; this note does not change `ACCEPTANCE-v1.2.md` or the run folder.
- Nothing about an installed artifact (E1, CTRL-E1), the public model, the GB10 path (R6), native AIENOS, or program
  qualification.

## Limits

- The second machine ran the verifier only; it did not rerun the objective (the contract's E5 is a verifier step).
- Both machines built from the same source file at the same commit; independence is of machine, toolchain and build,
  not of verifier authorship.
- The ssh transport carried the commands and the printed outputs; the Mac fetched the repository from GitHub itself.
- The Mac's CTRL-E5 script stopped after the verifier returned exit 1 (the script used `set -e`); the verifier output had
  already been written and was read back in a second ssh call. The two header lines (`verifier_exit`) are therefore
  absent from the Mac file and present only as this note.

## Next

R6 (GB10) waits on the accelerator backend lane; R8 (signed installed release with a qualified open-license model)
waits on the key ceremony (sc#385) and the Qwen3 qualification (sc#384), both Drake-controlled.
