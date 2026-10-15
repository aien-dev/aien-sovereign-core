# OWN-30B re-qualification on merged main (2026-10-15)

CALC-EXEMPT: every number here is copied from a measured log in this folder or the named pre-merge folder; none is derived.

All results HARDWARE (GB10), one 30 GB model per process, each run in its own `quietlock hold --owner OWN30B --minutes 20`
under `/tmp/aien-gb10.lock`, AIEN_OMEGA_SPIN_US=2000, nothing killed or timed out. Commands: `own30b-gates.sh` (same
commands and flags as the pre-merge RC evidence `own30b-rc-20261014T0153Z` and graph runs `own30b-L2-20261013`).
Exact sc/omega shas, binary sha256, env and command per run: `*.start`. Output `*.raw` / `*.err`, exit `*.rc`.

Build: sc main 9557e0c1 (#510 + #514) plus this branch's native-artifacts.lock lines, omega.lock 62833658 (#439),
pins from `scripts/pins.sh --gpu target/pins` (physics 6d7cf0d4, aienos b84c0a67), AIEN_REQUIRE_NATIVE_LIBS=1.
libomega_gpu.a sha256 6a6cb4df..., byte-identical to the RC archive. Binaries: `binaries.sha256`, `BUILD.txt`.

## Gates vs pre-merge

| run | merged main | pre-merge (RC 20261014T0153Z, graph: L2-20261013) |
|---|---|---|
| check-full (tokens, parity, 10 controls) | rc 0, parity PASS, 10/10 CAUGHT, tokens IDENTICAL x4; decode 38.9 / 35.9 / 37.4 / 37.0 tok/s | rc 0, same verdicts; 39.1 / 36.1 / 37.6 / 37.0 |
| check-wp (whole prompt, device-host) | rc 1: p1 DIVERGES at 0 (cpu top-2 margin 0.4032), p2-p4 IDENTICAL | rc 1, identical divergence (FP8 activation rounding, explained in the RC notes) |
| ttft 512 (OFF / ON dev) | 7.66 / 0.33 s, 7.63 / 0.31 s | 7.65 / 0.31 s, 7.55 / 0.32 s |
| ttft 2048 (OFF / ON dev) | 32.2 / 0.88 s, 32.1 / 0.85 s | 32.1 / 0.87 s, 31.9 / 0.88 s |
| whole-prompt gate 512, 2048 | PASS 0.92 B, 0.98 B; mutants CAUGHT | PASS 0.92 B, 0.98 B; mutants CAUGHT |
| gate-short 1, 2, 20 | PASS 0.01, 0.51, 0.57 B (rc 0; 48 omitted, known preamble assert) | PASS 0.01, 0.51, 0.57 B (rc 134 at 48) |
| graph-off (CTA budget 64) | 38.5 / 35.7 / 36.9 / 36.0 tok/s, PASS | 38.1 / 35.2 / 36.3 / 30.1 (older build sc 8d0eb90) |
| graph-on (+ OWN30B_DECODE_GRAPH=1) | 42.6 / 39.5 / 40.8 / 40.1 tok/s, 15/16 steps one list, PASS | 38.5 / 34.8 / 36.7 / 37.5 (older build) |

No regression. Graph rows compare against an older build, so the speed-up there is not credited to the merge.

## Server smoke (server-smoke/)
The `:18091` command for merged main is `own30b-serve-cmdline-merged.txt`. It has no `--spec on`: merged main has no
guess-ahead flag (RC3 0826a25 is not in main) and parse_args refuses unknown flags. One start, `/v1/models`, then the RC's
three requests: "Paris" 0.92 s (RC 0.96), streamed "Rome" 0.33 s (RC 0.31), long context 1.28 s (RC 1.26). The server was
stopped by SIGTERM after the last response (exit 0, no listener left on 18091).

## native-artifacts.lock (abi-qualify/)
The two lines for omega 62833658 (librx_compose.a e598074e..., libomega_gpu.a 6a6cb4df...) are the digests build.rs reported as
"not recorded", reproduced by an independent omega `make`.
- `scripts/abi-qualify.sh --pins target/pins` host scope: PASS (`host/`).
- `--chip` run 1 (`chip-r1-fail/`): FAIL in chip-inference-linked. The cause is a test added in #514,
  `paged_and_host_pool_caches_export_the_same_bytes_and_round_trip`, which asserted "the stub build has no resident pool".
  The linked native build has a resident pool. The fix in this branch drops the stub-only assertion. The byte comparison
  and round trip are unchanged, and they now also run on the resident chip pool.
- kv_persist tests linked on the chip after the fix: 4/4 ok (`kv-persist-linked-after-fix.log`).
- `--chip` run 2 (`chip-r2/`): PASS, scope host+chip. It ran on the working tree, which is this branch plus the test fix.
- All three evidence.json files say `sovereign_core_dirty: true`: host and chip run 1 ran with the lock lines not yet committed,
  and chip run 2 also had the test fix uncommitted. Both are committed unchanged in this PR.
