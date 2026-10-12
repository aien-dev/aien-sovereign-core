# Draft-reuse speculative verification, feasibility leg (2026-10-12, sc#384 speed lane)

Development measurement, not a campaign run. Nothing is promoted by it. Preregistration and the
full record: `~/workspace/investigations/2026-10-12-qwen3-8b-decode-speed/plan.md` (decision 14:30Z).

Question: in the corrective second attempt of a document task, can the rejected first draft be
reused as a free drafter (propose short stretches of it, verify them with the unchanged Qwen3-8B
in one row-batched pass, `NativeTransformerBackend::verify_block`) without changing the output,
and how much time does it save?

Setting: real corrective D1 prompt (877 tokens: system, goal, refusal reason, the v7 refused
reply quoted), draft = the v7 refused reply's 655 token ids, greedy, cap 1024, GB10 at the daemon
defaults (CTA 1024, spin 2000 us), engine pinned by `omega.lock`.

| run | tokens | ms | vs plain | tokens identical to plain | verify calls (mean ms) | draft accepted |
|---|---|---|---|---|---|---|
| plain greedy (M1) | 1013 (eos) | 89731 | 1.00 | - | - | - |
| B=2 (3 rows) | 1013 | 57358 | 0.64 | yes | 246 (88.9) | 449/492 |
| B=4 (5 rows) | 1024 (cap) | 53111 | 0.59 | no, from index 764 | 161 (102.5) | 530/641 |
| B=8 (9 rows) | 1024 (cap) | 49637 | 0.55 | no, from index 764 (same as B=4) | 104 (123.3) | 587/824 |

Primitive cost at context 1873 (M3, three calls each): plain step 78-81 ms; verify rows 2: 85,
4: 95, 8: 121, 16: 168 ms. VmHWM 29.7 GB after load, 31.8 GB at the end. Fallbacks: 0.

Tie check (`tie-763.json`): at output index 764 the two candidates (323, 382) are 0.008 apart on a
scale of 29 by the plain path (bitwise equal to verify at 1 and 3 rows) and 0.002 apart at 5 and
9 rows (a different summation path above 4 rows). The baseline run chose 382, the plain re-read
of the same context chooses 323: a coin flip that moves with any change of numerical path, the
production prefill-vs-decode mix included. The B=4/B=8 divergence is that coin flip, not a fault.
`tie-764.json` is the same check one position late (all paths agree there), kept as a record.

Verdict against the preregistered rule (identical ids for every B and best-B at or under 0.6x):
NOT MET, narrowly on both axes. Against the directive's gate (verification not too expensive,
enough draft tokens accepted): met. Limits: one run per setting, one task, development harness
rather than the serving spine; the whole two-attempt task was not measured as one unit.
