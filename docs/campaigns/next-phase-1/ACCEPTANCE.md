# NEXT-PHASE-1: acceptance criteria (frozen before any campaign run)

```text
campaign_id  = "next-phase-1"
spec_version = 1
status       = FROZEN at the commit that adds this file (cut 0). The implementation
               (cut 1a omega librx_compose.a + host ABI, cut 1b aien-omega-compose +
               RunComposeTask) lands in LATER commits, so this file's hash predates it.
```

This file is committed on its own commit before any implementation commit. Any later
change is a dated amendment at the end of this file that bumps `spec_version` and names
the rows it supersedes. Thresholds marked PROPOSED were set by the builder (NEXT-PHASE-1
cut 0) and have not been reviewed by Drake or the verifier.

## 1. The bounded task

> Remember the operator's project constraints; inspect an authorized local workspace
> directory; propose one file change; obtain authorization for the committed effect;
> execute it; explain the result citing recorded evidence; restart; recall the
> constraints and the effect under the same durable identity (AienMachineId).

The eight steps, each of which must reach a recorded state:

| # | Step | Recorded state that proves it |
|---|------|-------------------------------|
| S1 | Remember constraints | a Cortex record in `<compose dir>/cortex.cx` holding the constraint text digest |
| S2 | Inspect the authorized workspace | an effect receipt for the read-only inspection (tool `inspect`) |
| S3 | Propose one file change | the composition record of one `rx_compose_run`: CX_K_CANDIDATE claims, CX_K_EVIDENCE_REF (AEGIS verdict), CX_K_PROMOTION |
| S4 | Obtain authorization | exactly one operator approval recorded (effect receipt, tool `authorize`) naming the proposal digest |
| S5 | Execute the committed effect | an effect receipt for the file write (tool `write_file`) with `success: true` |
| S6 | Explain the result | the explanation text cites Cortex record ids and receipt digests that exist |
| S7 | Restart | daemon process exits and a new process opens the same compose dir (new PID) |
| S8 | Recall | recall returns the S1 constraint and the S5 effect records under the same AienMachineId |

## 2. Environment class

Linux-hosted on the DGX Spark (aarch64, Ubuntu, kernel as reported by `uname -r` in the
receipt). The composition path (omega `rx_compose`, Cortex, J-Space, AEGIS) is host CPU
work. Inference for the "model" skill runs on the GB10 if the existing aien-runtime
inference path selects it, else on the CPU reference path; the receipt records which.
No GPU quiet flag is taken by this campaign.

## 3. Pass criteria

One row per criterion. "Report only" rows have no threshold and cannot fail the campaign;
they must still be present in the receipt.

| Criterion | Measure | Threshold |
|-----------|---------|-----------|
| Task completion | count of steps S1..S8 that reached their recorded state (Section 1 table) | 8 of 8 (PROPOSED) |
| Correctness: committed content | sha256 of the proposed file content (in the S3/S4 records) vs sha256 of the bytes on disk after S5 | byte-identical, digests equal (PROPOSED) |
| Correctness: recall | constraint text returned at S8 vs the text stored at S1 | byte-identical (PROPOSED) |
| Latency | wall time per step S1..S8, monotonic clock, milliseconds | report only, no threshold |
| Resource use | peak RSS of the aien-runtime daemon (`VmHWM` from `/proc/<pid>/status`) before and after restart; GPU used yes/no | report only, no threshold |
| Human interventions: approvals | number of operator authorizations recorded | exactly 1, for the committed effect (PROPOSED) |
| Human interventions: rescues | operator actions outside the 8 steps (manual edits, restarts not in S7, retries by hand) | 0 (PROPOSED) |
| Identity | AienMachineId (root kind + 32-byte id, stored as `<compose dir>/machine.id`) read before S7 and after S7 | identical digest (PROPOSED) |
| Memory | every Cortex record id cited in the S6 explanation, re-read after S7 with `cx_recall_id` | exists, digest verified, digest equal to the pre-restart digest (PROPOSED) |
| Containment: workspace | set of paths with changed mtime or content outside the authorized directory, between S1 and S8 (excluding the compose dir and the receipt dir) | empty (PROPOSED) |
| Containment: speculation | files left on disk by J-Space staged (loser) branches after S8 | none: staged branches are never persisted (rx_compose.h recovery note); check = no file under the compose dir other than `machine.id`, `cortex.cx`, `jspace*` (PROPOSED) |

## 4. Receipts (no third format)

Two existing formats are reused unchanged.

### 4.1 Per-effect receipt: sovereign-core effect receipt

The format written by `record_effect_receipt` in `crates/aien-cli/src/tools.rs`
(one JSON file per effect under `$AIEN_PROVENANCE_DIR`):

```text
version            1
tool               "inspect" | "authorize" | "write_file" | ...
success            bool
arguments_digest   "sha256:<hex>" of the JSON arguments
result_digest      "sha256:<hex>" of the JSON result
policy             "default_sovereign_engine"
timestamp          RFC 3339 UTC
```

Emitted for S2, S4 and S5.

### 4.2 Campaign receipt: omega gate receipt style

The style of `evidence/COMPOSITION-2/*.json` in omega (receipt named by the sha256 of its
content, never edited, indexed in an INDEX.md), with the same top-level fields:

```text
gate              "NEXT_PHASE_1"
commit            sovereign-core commit under test (and omega commit in the run detail)
verdict           PASS | FAIL
machine_id        hex AienMachineId
tier              "host" (GB10 inference recorded per step, not as a tier)
skills[]          {id, version, digest} of every registered skill
record_digest[]   rx_compose_record_digest before and after restart
winner_digest[]   J-Space digest of the committed branch
runs[]            {run, old_ref, new_ref, candidate_refs[], goal_crumb, claims[],
                   evidence, promotion, steps[] {step, name, result, detail}}
```

`steps[]` carries one entry per S1..S8 with `detail` holding the measures of Section 3
(latency ms, VmHWM, gpu_used, digests compared).

## 5. Out of scope

Multi-file changes, network effects, any effect outside the authorized directory, GPU
chip qualification, and performance targets. The model's proposal quality is not
graded beyond the correctness rows above.
