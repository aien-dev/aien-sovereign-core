# ALLEN end-to-end demo v4-gb10: result

Spec: DEMO-v4-gb10.md = DEMO-v2.md with M-A = Qwen3-4B-Instruct-2507 (cdbee75) on the GB10 and M-B = SmolLM2-1.7B on the
CPU reference. Frozen declaration: aien-architecture#159 issuecomment-6060242596, pre-run note issuecomment-6060283301.
Driver: scripts/allen_e2e_demo.sh with DEMO_VERSION=v4-gb10, run from a detached tree at 6e24bbc (scripts/ and docs/
byte-identical to PR head fe778ec; driver sha256 24e14919234c8660ba01072610efd497693cccd8c5a8f70299c68149ba64c6e4).
Receipts: receipts-v4-gb10.jsonl (one line per step, 10 steps plus the GB10-lives line). Raw outputs: artifacts-v4-gb10/.
GB10 daemon: the sealed sc#277 attempt-3 candidate binary, sha256 152c0aecce662f618bf683c8854d6de56a7075e0461c2433570f4c15b68571a5,
built from sovereign-core 6bbe2ec2 (hash checked before the hold and by the driver before every GB10 life).
CPU leg daemon/CLI: built from 6e24bbc, sha256 d133c38a0062... (in every receipt).
Label: S0-S6 real-GB10 (Linux-hosted GB10, native Omega engine, no CUDA). S7-S8 real-CPU. Native AIENOS: NOT_RUN.
Not a physical-Spark AIENOS result. mock: not used.
Run: 2026-10-08, 12:59:45 to 13:03:31 UTC, inside one quietlock hold (owner f68569-allen-gb10, 20 minutes booked,
3 min 46 s used), slot from coordinator 8985c8.

v1 (FAIL), v2 (FAIL) and v3 (PASS, real-CPU) stay recorded as they were. This run does not change them.

## Verdict: PASS (10 of 10 steps, plus every GB10 life clean)

## Step table

| Step | Result | Evidence |
|------|--------|----------|
| S0 | PASS | Daemon up on M-A on the GB10 (Backend: OmegaGb10Backend, native Omega engine, no CUDA, sm_121). "Authorize MAC: on". allen status not_engaged, no ALLEN state before S1. |
| S1 | PASS | One identity, ID0 = 9ff5d749bf9a56d3dad935eed21e3129debf24425322e72a90c8928ad5970a3c. AgentRoot 24293fdde359... |
| S2 | PASS | Profile revision 1 with plain-language on, under ID0. Memory inspect shows exactly N-work, N-pers, G1. Goals list shows exactly G1. |
| S3 | PASS | Commit DONE, inference on the GB10. MemoryReport context work, items_included 2. C1-C5 hold. 54 words. garden.md sha256 280270d87bc25cbd20068ee1f69d7ac23e3ef6ba8abce3f43a986698ab9b6e04. |
| S3-red | PASS | Checker on the hand-written document reports exactly C3 C5. The good control reports none. |
| S4 | PASS | Ledger links grant id, proposal sha256, content sha256 (equal to garden.md on disk), the M-A model digest and ID0. |
| S5 | PASS | Graceful stop (exit 0), restart on M-A on the GB10. ID0, profile, notes, G1 identical. S3 intent still DONE, ledger and record count unchanged (not re-executed). garden.md same sha256, inode and mtime. Replay reconcile: 0 claims. |
| S6 | PASS | Forgot N-pers. Inspect shows no text (row state forgotten). Recall in personal returns 0 items. 0 files under daemon state still hold the canary. |
| S7 | PASS | Graceful stop of the GB10 daemon (exit 0), start on M-B on the CPU. Model line, Backend line and model digest changed (d6c42883a895 to f55217be716b). ID0, profile, N-work, G1 unchanged. N-pers still absent. |
| S8 | PASS | Commit DONE on M-B (CPU). MemoryReport context work, items_included 2. C1-C5 hold. 107 words. garden-b.md sha256 092c4c5ca2383308a27f325e8b8959e6b82aa31bd449feada263dd85385b9189. Provenance links all present, including the M-B digest and ID0. |

## GB10 lives (3, as declared)

Each of daemon-1, daemon-2 and daemon-3: OmegaGb10Backend line; GB10_SERVING_RESERVATION line (194170880 bytes, context
4096, 1536 matmul kernel calls prepared in 21-29 ms, then sealed); 0 allocation failures, 0 RM failures, 0 fallback lines,
0 seat-kill lines; stop by `aien-cli compose shutdown` (1 request each, acknowledged), daemon exit 0, socket removed; kernel
log NVRM count 1533 before and 1533 after every life (delta 0). No process holding a GB10 channel was signalled.

## Contamination check

A 1 s camera (pgrep cargo|rustc|make|qemu, 222 samples, 12:59:45 to 13:03:30) saw no foreign build or QEMU process. The
only build activity was the driver's own S1 fixture build (cargo test of e2e_demo_subject into this job's target dir),
which is part of the declared steps, as in v3. The driver itself has no per-life quiet gate; the camera replaces it as
evidence. No timing is claimed, so this does not affect any criterion.

## Limits

- Linux-hosted GB10 only. Nothing here is a native AIENOS result or a physical-Spark AIENOS result.
- Hard-crash recovery on the GB10: NOT TESTED (chip safety rule: no kill of a process holding a GB10 channel). kill -9
  recovery evidence is from CPU legs only (v3 S5/S7, recovery matrix sc#307, concurrency test sc#325).
- One request per GB10 life; no endurance, no throughput or latency claim.
- v3's model order is swapped (Qwen3 first here), so the S3 and S8 texts are not comparable to v3.
- The models are the fixed demo checkpoints; this says nothing about model quality beyond C1-C5.
