# NEXT-PHASE-1 v5: campaign verdict

Verdict: **PASS** (three of three tasks, one launch each, no retries, no tuning after
results, no manual rescue). Spec: `ACCEPTANCE-v5.md` spec_version 5, Section 5 frozen in
commit ac96e9c before the run. This file is written by the reviewer after the receipts;
the receipts are immutable and the rows decide, this file only records what they show
and what they do not cover.

## 1. What the run produced (evidence)

Run: 2026-10-06 10:40:49 to 10:44:10 local (3 min 21 s), inside one `quietlock hold`
(owner np1-v5, 20 min, start and release whispers posted), `~/workspace/.spark-quiet` absent,
load 2.78 before the run, no other GPU job.

Bound inputs (all three receipts): sovereign-core ac96e9cd6c38e2c236d5434194a9e7ba1454be5f;
omega compose 62b6a28 (librx_compose built from the pinned checkout, physics 6d7cf0d);
omega GPU engine 62b6a28 (prebuilt `libomega_gpu.a` sha256 ef80e431…, built by W8 from the
same commit); model unsloth/Llama-3.2-1B-Instruct snapshot 5a8abab, weights sha256
1ff795ff…538f, tokenizer 6b9e4e7f…537b, verified before the build; max_tokens 96; backend
NativeTransformerBackend/OmegaGb10Backend, `gpu_used` true on every step that composes.

| Task | Receipt | Reply (after `filename: <path>`) | File written | Tokens / stop | S3 ms |
|---|---|---|---|---|---|
| T1 NOTES.md | `31dad76f…7217.json` | `# Project Notes` / `This project keeps every change inside its workspace.` | ws/NOTES.md | 16 / eos | 4944 |
| T2 docs/CONTACT.txt | `5e52174e…ca43.json` | `Ada Lovelace: ada@example.org` | ws/docs/CONTACT.txt | 13 / eos | 4337 |
| T3 TODO.md | `c02bf404…fdad.json` | `Write tests` / `Update the changelog` / `Tag the release` | ws/TODO.md | 14 / eos | 4418 |

Reply files: `replies/da485c92…`, `replies/a9da20ce…`, `replies/b7a87b65…` (named by their
sha256; each receipt names its reply). The reviewer read every file on disk: byte-identical
to the reply body and to the digest the receipt binds (`s3 == s4 == s5_disk`).

Rows, every task: S1–S8 PASS; v4 rows (task completion 8/8, content digests equal, recall
byte-identical, approvals exactly 1 bound to the committed effect, rescues 0, identity digest
equal before and after restart, memory prefix digest equal, containment empty, no speculative
effect) PASS; v5 rows Q1 path == requested path, Q2 required phrases present and no echoed
prompt line, Q3 stop reason eos on attempt 1, Q4 no chat markers, A1–A6 PASS.

Observations (report only, never part of the verdict): warm-up 9.8–10.0 s per daemon start,
recorded outside the task window; generation 3.0–3.3 tokens/s wall including prefill;
restart step S7 30–31 s; VmHWM ~17.4 GB before and after restart.

## 2. What this proves, and what it does not

Proven, on the connected production path: a real model (not a stub) ran through Omega on
the GB10 with no CUDA, produced the correct file at the requested path with content that
satisfies the task, the proposal went through inspection, authorization bound to the exact
content digest, execution inside the workspace only, World commit, canonical Cortex
persistence, an explanation citing receipt fields that exist, a daemon restart that kept the
machine identity, and a recall that returned the committed record. Zero manual rescues.

Not proven by this campaign:
- Only three tasks, each a single-file write with short content. Longer content, edits to an
  existing file, multi-file changes, refusals of unsafe goals, and the negative cases of
  ACCEPTANCE-v5 Section 4 (launch failure, budget exhaustion) were not exercised here; the
  v4 FAIL record (`VERDICT-v4.md`) remains the evidence that the rows do catch junk output.
- One run per task, as frozen. This is a correctness demonstration, not a reliability
  figure; no repeat statistics exist.
- Inference fidelity for this model was established by W8 (GPU output token-identical to the
  CPU reference and to the trusted reference on these three prompts); this run did not re-check
  tokens against the reference.
- The compose library build needs an aienos clone at the `aienos.lock` commit
  (`AIEN_AIENOS_LOCK_REPO`, documented in `crates/aien-omega-compose/build.rs`); the first
  build attempt without it failed. This is a build input, not a runtime one, and should be
  listed in the release inputs.

## 3. Conclusion on the frozen model input

Llama-3.2-1B-Instruct, frozen under Section 5, completes the workflow correctly where
TinyLlama (v4) did not. The model is under the Llama 3.2 Community License; its place on
the production path is a licensing decision for the owner, recorded here as open, not as
settled. No tuning happened after the run; the next change to prompt, budget, model or rows
is a new spec version.
