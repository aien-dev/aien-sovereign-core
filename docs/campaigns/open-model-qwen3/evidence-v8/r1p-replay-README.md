# v8 step 1: R1-P CPU replay repaired (exact token identity)

`r1p-replay-cpu.json`: `np1_reference` built from this branch, `NP1_REFERENCE_ALLEN=default`, Qwen3-8B
(`~/models/qwen3-8b`, model_sha256 17bc3688...), goal of task R1, mode `new`, 96 tokens, CPU only, no chip hold,
2026-10-12T01:35:10Z to 01:38:52Z, exit 0.

| | v7 GB10 run (s3-report-R1.json, attempt 1) | v7 CPU replay (R1.reference.json) | this replay |
|---|---|---|---|
| prompt tokens | 164 | 99 | 164 |
| prompt_ids_sha256 | eee4e094...adfe6 | 335c249b...27ff | eee4e094...adfe6 |
| output tokens | 26 | 26 | 26 |
| reply sha256 | bf760bf2...a812 | bf760bf2...a812 | bf760bf2...a812 |

The model-visible token sequence now equals the GB10 prompt exactly, ALLEN context included. The v7 record and its
R1-P FAIL are unchanged (RESULT-v7.md); this is the repaired tool a later campaign uses. Unit check:
`AIEN_QWEN3_MODEL_DIR=<qwen3-8b> cargo test -p aien-runtime --example np1_reference v7_r1` (tokenizer only, no model run).
