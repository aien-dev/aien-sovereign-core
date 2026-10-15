# OWN-30B on omega 4799f9eb (pin bump, 2026-10-15)

CALC-EXEMPT: every number here is copied from a measured log in this folder (the `*.log` files are force-added past
.gitignore so SHA256SUMS checks on a fresh clone) or in #524's and #514's folders
(`own30b-requal-main-20261015`, `pkv514-chip-20261014`); none is derived.

## What changed
`omega.lock` 62833658 -> 4799f9eb (omega main after #459 GB10 stale-L2 invalidate + cmdlist prefix reserve,
#460 wide bf16 matmul layout fix and platform ABI 1.1 -> 1.2, #461 test only, #463 evidence).
omega's physics.lock 6d7cf0d4 and aienos.lock b84c0a67 are the same at both omega commits.

ABI 1.2 adds `OmegaGpuPlatformCaps.submit_prefix_words` and `omega_gpu_session_submit_prefix_words()`.
sovereign-core mirrors no platform struct (no `OmegaGpuPlatformCaps`, no platform install, no direct ring
submission: grep of `crates/` finds none), and the `omega_gpu.manifest` structs are unchanged, so no Rust
change is needed. The build-time manifest check passed (abi-qualify `conformance`, `gpu-host`).

`native-artifacts.lock`: libomega_gpu.a `0629ad9c...` and librx_compose.a `e598074e...` (same bytes as at
62833658) for omega 4799f9eb. build.rs reported both as "not recorded"; an independent omega `make` in a
separate checkout produced the same two digests (`omega-independent-make.sha256`).

Build: sc afe59ae1 (main bbca84b1 + the two lock changes), `scripts/pins.sh --gpu target/pins`,
AIEN_REQUIRE_NATIVE_LIBS=1, no "not recorded" warning. Binaries `binaries.sha256`, `BUILD.txt`.

## Machine state (read this before comparing speed)
The GB10 was shared with a long MAX Qwen3-8B experiment on :18097 (about 36 GB, GPU busy ~90%) for every run
here. #524 ran on an idle GPU (MemAvailable 106 GB). The 30B tools refuse to load under 70 GB available
(a guard against a second 30B model), so every run here adds `--force-memory` (MemAvailable ~68 GB, no other
30B process; `attempt1-refused-70gb/` is the refused first attempt). Speeds here therefore measure GPU
sharing, not the pin. `baseline-ab/` isolates the pin.

## Baseline A/B, same machine state (baseline-ab/)
check-full, same command, alternated #524 binary (omega 62833658) and this binary (omega 4799f9eb):

| run | decode p1 / p2 / p3 / p4 tok/s | verdict |
|---|---|---|
| old-1 (62833658) | 2.5 / 2.5 / 2.5 / 2.4 | parity PASS, tokens PASS |
| new-1 (4799f9eb) | 2.5 / 2.4 / 2.6 / 2.5 | parity PASS, tokens PASS |
| old-2 (62833658) | 2.4 / 2.3 / 3.2 / 2.6 | parity PASS, tokens PASS |
| new-2 (4799f9eb) | 2.6 / 3.3 / 3.2 / 2.4 | parity PASS, tokens PASS |

The old and new engines run at the same speed under the same load (2/2 each). The drop from ~38 tok/s comes
from sharing the GPU, not from the pin.

## Gates (same commands as #524, `own30b-gates.sh`)

| run | omega 4799f9eb, GPU shared with MAX | #524, idle GPU |
|---|---|---|
| check-full | rc 0, parity PASS, 10/10 CAUGHT, tokens IDENTICAL x4; 2.4 / 2.4 / 2.6 / 2.5 tok/s | rc 0, same verdicts; 38.9 / 35.9 / 37.4 / 37.0 |
| check-wp | rc 1: p1 DIVERGES at 0 (cpu top-2 margin 0.4032), p2-p4 IDENTICAL | identical verdict |
| ttft 512 (OFF / ON dev) | 48.0 / 1.43 s, 46.7 / 1.60 s | 7.66 / 0.33 s, 7.63 / 0.31 s |
| ttft 2048 (OFF / ON dev) | 204.4 / 2.52 s, 202.6 / 2.52 s | 32.2 / 0.88 s, 32.1 / 0.85 s |
| ttft summaries | tokens DIFFER (OFF vs ON, as #524); devhost vs host BIT-IDENTICAL | same |
| whole-prompt gate 512, 2048 | PASS 0.92 B, 0.98 B; mutants CAUGHT | PASS 0.92 B, 0.98 B |
| gate-short 1, 2, 20 | PASS 0.01, 0.51, 0.57 B (rc 0) | PASS 0.01, 0.51, 0.57 B |
| graph-off (CTA budget 64) | 2.6 / 2.4 / 2.5 / 2.5 tok/s, PASS | 38.5 / 35.7 / 36.9 / 36.0 |
| graph-on (+ OWN30B_DECODE_GRAPH=1) | 15.2 / 13.9 / 14.3 / 14.4 tok/s, 15/16 steps one list, PASS | 42.6 / 39.5 / 40.8 / 40.1 |

Every correctness verdict and every gate ratio is the same as #524. The speed columns are not comparable
(different GPU load); the A/B above is the speed comparison. Under GPU sharing the one-list decode (one
submission per token) loses far less than the per-launch path, which fits a per-submission cost of sharing
(INFERRED, not measured separately).

## ABI qualification (abi-qualify/)
- `scripts/abi-qualify.sh --pins target/pins` host scope: PASS (`host/`).
- `--chip`: PASS, scope host+chip, including chip-gpu, chip-inference and chip-inference-linked (`chip/`).
- Both say `sovereign_core_dirty: true`: the only change on the tree was this untracked evidence folder
  (HEAD afe59ae1).

## Persistent KV on the chip (pkv-chip/, runner `pkv-run.sh`)
The #514 items rerun because #459 changes how cached buffers are synced. Item 5 (two 30B processes,
~71 GB) was not run: it would not fit beside the MAX experiment.

| item | result here (GPU shared) | #514 run3 (idle GPU, omega d0889a94) |
|---|---|---|
| 1 fused-cache round trip | PASS: 2079 positions, 204,472,320 bytes, 9 chunks parked, 0 refused, 0 failed; round trip identical 5/5; neg flip found at its offset, truncated payload refused | PASS, same bytes and verdicts |
| 3 export cost (median) | 216.7 ms | 65.3 ms |
| 4 fetch warm / cold (median), import | 734.5 / 925.7 ms (cold block reads >= 204.5 MB), import 103.9 ms; corrupt store refused | 453.6 / 693.5 ms, import 22.4 ms |
| 2a restored turn 2 == uninterrupted (in process) | PASS | PASS |
| 2b server chip check | PASS, 28/28 checks (torn write, late load, late park, neg-switch, neg-corrupt; restore covered the 750-token turn-1 prompt) | PASS, 28/28 |

Every correctness check matches #514. The timings are slower under GPU and CPU sharing (the MAX process
runs at 100% of one core) and are not a pin comparison.

## Server smoke (server-smoke/)
The `:18091` command is `own30b-serve-cmdline-merged.txt` (also updated in `~/handoffs`): same flags as
#524, new binary. One start (ready in 71 s), `/v1/models`, then the RC's three requests: "Paris" 5.6 s,
streamed "Rome" 4.2 s, long context 9.0 s, same answers as #524 (0.92 / 0.33 / 1.28 s on an idle GPU).
Stopped by SIGTERM after the last response (exit 0, no listener left on 18091).

## Limits
- Speed was measured only under GPU sharing; no idle-GPU number exists for 4799f9eb. The A/B shows no
  pin effect at this load (2 runs each); it does not exclude a small difference on an idle GPU.
- The L2 invalidate fires only after a host write to a GPU-cached buffer; whether any OWN-30B path does that
  per token is not instrumented here (UNKNOWN). Equal A/B speed says it costs nothing visible at this load.
