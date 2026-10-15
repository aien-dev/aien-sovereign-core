# ADR 0004 step 1 gate receipts (2026-10-15)

Branch `refactor/adr0004-step1-kv-geometry` (PR #543), rebased on main f87b63b (step 0 baseline merged). Omega 4799f9eb; `libomega_gpu.a` sha256 0629ad9c..., the same archive as the step 0 baseline. Baseline: `../adr0004-step0-baseline-20261015/check-full-3/`.

| Gate | Receipt | Result |
|---|---|---|
| Host tests (`cargo test -p aien-inference-abi -p aien-serve-http --features aien-inference-abi/host-test-engine`, stub build) | `host-tests.txt` | all green, 0 failed |
| New test: `KvGeometry::from(&c)` carries the numbers `for_config` read (30B and tiny shapes; the 8B in the dense tests) | `host-tests.txt`, `native-kv.log` | pass |
| G30-kv: `restored_prefix_continues_exactly_like_the_uninterrupted_engine`, `paged_and_host_pool_caches_export_the_same_bytes_and_round_trip`, serve-http `tests/kv_persist.rs` | `host-tests.txt`; native (resident chip pool) `native-kv.log` | pass |
| Exported KV bytes equal main's: `exported_kv_bytes_match_the_digests_recorded_on_main` (digests printed by the same fill and hash on origin/main 559f0f1e) | `host-tests.txt` (host pool), `native-kv.log` (chip pool) | pass |
| G30-tokens `--tokens-only`: `own30b_check chip`, the baseline's flags plus `--tokens-only`, quiet flag, `/tmp/aien-gb10.lock`, `AIEN_OMEGA_SPIN_US=2000` | `tokens-only/`, `tokens-only.start` | IDENTICAL on all 4 prompts; the chip token lists (name, chip, first_diff, emitted) hash equal to the baseline's run.json |

Decode tok/s (informational, G30-speed is not a step 1 gate): 38.1 / 35.5 / 36.9 / 36.0 against the step 0 floors 37.927 / 35.017 / 36.375 / 35.502; every prompt is above its floor.
