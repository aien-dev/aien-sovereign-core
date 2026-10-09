# OPEN-MODEL-QWEN3 v5: verdict

```text
spec          = ACCEPTANCE-v5.md, FROZEN (frozen-v5.json, pins at sovereign-core 30258fc), before the run
verdict       = FAIL (scoring-v5, oq3-v5-score.json): 334 declared rows, 262 PASS, 72 FAIL,
                none malformed, none NOT_APPLICABLE, each row present once (rep 1)
failing rows  = 72, all in three tasks: D1 (23), D2 (25), D6 (24)
passing       = every declared row of D3, D4, D5, E1, E2, N1, N2 and R1 (8 of 11 tasks)
score file    = oq3-v5-score.json; result lines oq3-v5-results.jsonl
```

Nothing below changes ACCEPTANCE-v5.md, any row, limit, goal, task or declaration; no launch was repeated.
v1 to v4 stand as recorded. All numbers are from the files named in Section 6.

## 1. Result per task

| Task | Declared rows | PASS | FAIL | Proposal attempts (`s3-report.json`) | Committed |
|---|---|---|---|---|---|
| D1 | 34 | 11 | 23 | 2, both refused | no |
| D2 | 36 | 11 | 25 | 2, both refused | no |
| D3 | 36 | 36 | 0 | 2: refused, then parsed | yes |
| D4 | 35 | 35 | 0 | 2: refused, then parsed | yes |
| D5 | 36 | 36 | 0 | 2: refused, then parsed | yes |
| D6 | 33 | 9 | 24 | 2: refused, then timeout | no |
| E1 | 33 | 33 | 0 | 1, parsed | yes |
| E2 | 34 | 34 | 0 | 1, parsed | yes |
| N1 | 11 | 11 | 0 | none (refused for its destination before any model call) | no, as required |
| N2 | 14 | 14 | 0 | 3, each refused: reply cut at the token limit (16 tokens) | no, as required |
| R1 | 32 | 32 | 0 | 1, parsed | yes |
| Total | 334 | 262 | 72 | | |

D3, D4 and D5 also had a first reply refused by a requirement check (D3 "at least 18 non-empty lines, found 9", D4
"at least 13 non-empty lines, found 5", D5 headings "Documents", "Clothes", "Electronics" missing); the second attempt of each
was accepted and committed, so their rows pass.

## 2. Why D1, D2 and D6 failed (verbatim from `evidence-v5/s3-report-<task>.json`)

- **D1**, attempt 1 (541 tokens, 50 789 ms) and attempt 2 (531 tokens, 53 470 ms), both refused with:
  `unmet requirement: at least 22 non-empty lines, found 8`
- **D2**, attempt 1 (442 tokens, 45 179 ms) and attempt 2 (454 tokens, 46 901 ms), both refused with:
  `unmet requirement: at least 12 words of plain text in the section "Wrap Up", the section was not found; the headings "Example One: Date Prefix", "Example Two: Lowercase Names", "Example Three: Number the Files", "Wrap Up", missing "Wrap Up" (each must be a markdown heading line)`
- **D6**, attempt 1 (1011 tokens, 103 819 ms) refused with:
  `unmet requirement: the headings "Bringing Her Home", missing "Bringing Her Home" (each must be a markdown heading line)`
  attempt 2 timed out: `model proposal exceeded 66180 ms` (66 219 ms, no tokens recorded).

In all three the daemon had the model answer (rows `<id>-PRE`, `<id>-OPR`, `<id>-PIN` and `<id>-BE` PASS on all three, so the
requirement was read and the GB10 path was used with no fallback) and the product refused the reply before any write: S4
(authorize), S5 (execute) and S6 are not ok in `run.json` for D1, D2 and D6, nothing was committed, the generation record
is null. Per ACCEPTANCE-v5 Section 5 the first failing row of each is a model row (`<id>-L`/`-M`/`-RQ`-type, not
`PRE`, `OPR`, `PIN`, `GR` or `N1-NR`). D6-DEC and D6-MEAS also FAIL because the timed-out attempt has no `decoding` and no token ids;
that follows from the timeout, not from a separate fault. D1-GR, D2-GR and D6-GR FAIL because no generation record exists without a commit.

## 3. Run

```text
binaries        = sovereign-core 30258fce8d373bbd01ede59076dddce6e8a64901 (build ~/workspace/oq3-v5-build-30258fc),
                  omega 6c6180cf378075b61291f4565d226eba38b4decd, physics 6d7cf0d4d8eb2cda7b512100ff6058e25dbb3ddf,
                  aienos b84c0a67590a934f3f3e001b12ec85ebc086a9eb (pins-part1..4.json equal frozen-v5.json pins)
                  aien-cli       62d98e0b92372862c4a1f7d762a92ec8cddcf398dced8048f75e086b2d66bc55
                  np1_reference  a684b55af6efb1cea91ee119352552e7462c50026f2c91d38010b40c4e3fb7b9
                  np1_edit_merge 66be8da4d3c5c7c32089e851acdbed9c050afa38f2d7db9e8fbced3811949e80
                  Cargo.lock     49d97bf30113b1727fcfc0e33be79d9446ae13651a08afc32bba889b77fca265
wrapper, tasks  = run-qwen3-v5.sh and the v5 files from a clean clone of sovereign-core main 5533a49 (launcher: evidence-v5/run-part.sh, run-all.sh)
model           = Qwen/Qwen3-4B-Instruct-2507 rev cdbee75f, Apache-2.0; model_sha256 17a78fbba447a4e66a3d886c0998fbcf2f9201d46e5e0c5bfec2d57c975976b7 (index+shards)
environment     = AIEN_KV_CONTEXT_TOKENS=4096, AIEN_REQUIRE_BLACKWELL=1, AIEN_REQUIRE_CHECKPOINT=1, AIEN_GB10_QWEN3_DECLARED_ATTEMPT=1
evidence type   = GPU under Linux (GB10, native Omega engine, no CUDA); R1 reference on CPU
part 1          = quietlock hold 166657-oq3-v5-p1, 13:30:28 to 13:37:42 UTC 2026-10-09, exit 0 (D1 D2 D3)
part 2          = hold 166657-oq3-v5-p2, to 13:45:41, exit 0 (D4 D5 D6)
part 3          = hold 166657-oq3-v5-p3, to 13:48:58, exit 0 (E1 E2 N1)
part 4          = hold 166657-oq3-v5-p4, to 13:52:59, exit 1 = the scorer's FAIL exit, not a crash (N2 R1 and the score)
memory at start = MemFree 54009308, 55133776, 55158176, 55140836 kB (parts 1 to 4); MemAvailable 110310924, 110421232, 110452344, 110438848 kB
NVRM kernel log = line count 1533 before and after every part (evidence-v5/nvrm-before-p*.txt, nvrm-after-p*.txt)
```

Launch wall times (ms, `<id>.launch-v5.json`): D1 160380, D2 147690, D3 120410, D4 156100, D5 92100, D6 226260,
E1 66900, E2 68970, N1 55550, N2 71750, R1 59140. The run stopped on part 4's exit code 1 after the last part (`evidence-v5/stopped`).

GPU memory (omega#327): all 22 daemon logs show `GPU session: open` twice and `Warm-up:` once per start, and no `NV_ERR` line
other than the opt-in warning for a declared attempt (counts per log: `evidence-v5/gpu-memory-lines.txt`). Expected none; none found.

**R1, GPU against CPU identity:** R1-X, R1-P and R1-T PASS. The GB10 run exposed 20 token ids for a 111-token prompt;
the CPU reference (`ReferenceCpuBackend`) had the same prompt id digest (`aa8b2748...b7793e2`) and the same 20 output ids
(`gpu_ids` equal to `cpu_ids` in the receipt `02efdfe4...b665996.json`).

## 4. What this verdict means

- The qualification fails on three long or strictly structured documents. In each case the model wrote a reply that did not meet a
  stated requirement (22 non-empty lines; a section titled "Wrap Up"; a heading line "Bringing Her Home") and, on D6's second try, did
  not finish inside its time budget. This is model output quality on long and structured documents, not a product fault.
- The refusal path worked: the product refused each reply before any write, committed nothing wrong, and left S4 to S6 not run.
  Containment, approval, recall, identity and memory rows pass on the five committed documents and edits (D3, D4, D5, E1, E2) and on R1.
- The fixes of ACCEPTANCE-v5 Section 1 hold on this run: no goal was refused by the requirement reader before the model (`<id>-PRE` PASS on
  every launch that calls the model), the GB10 path ran with no fallback (`<id>-OPR`), N1 was refused for its destination, N2 for the token cut.
- Whether a bigger model, a different prompt or a tighter retry would pass D1, D2 and D6 is a new campaign with its own frozen spec.

## 5. Limits

- One run, one launch per task: one observation each, not a rate. Greedy decoding, one model, one machine, GB10 at the frozen pins.
- Three launches (D3, D4, D5) passed only on their second attempt; the first replies were refused. This is one observation each.
- The score file lists three extra non-declared result lines for N1 (`N1/N1-C`, `N1/N1-B`, `N1/N1-H`, FAIL, listed under `extras` in
  `oq3-v5-score.json`). They are inherited v8 rows that ACCEPTANCE-v5 Section 3.3 replaces for N1 by `N1-NR`, `N1-NM`, `N1-RC` and `N1-RH`, which
  all PASS; the verdict counts declared rows only (334). This section records them and does not rescore them. (The spec text says
  the replaced rows are not declared; whether the scorer should have dropped them is a tooling finding, not a change to this verdict.)
- `oq3-v5-results.jsonl` therefore has 337 lines (334 declared plus those 3).
- The legacy receipt `verdict` field reads FAIL on every receipt and is outside the verdict, as in v2 to v4.
- omega#327, omega#277 and sc#277 stay open and separate; Qwen3 on GB10 stays opt-in. Release readiness is not claimed.

## 6. Evidence

Files in this folder: receipts `<sha>.json` and `<sha>.summary.txt` and `*.v5rows.json` side files (named by their own sha256), result lines
`oq3-v5-results.jsonl`, score `oq3-v5-score.json`, committed replies in `replies/`, receipt list in INDEX.md.
`evidence-v5/` (added for this verdict): `hold-p1..p4.{start,end,rc,log}`, `mem-p*.txt`, `nvrm-before-p*.txt`, `nvrm-after-p*.txt`,
`identity-part*.sha256`, `pins-part*.json`, `meminfo-part*.json`, `run-part.sh`, `run-all.sh`, `all.end`, `stopped`,
`s3-report-<task>.json` for the eleven launches, `run-base-SHA256SUMS` (every file of the run base `~/workspace/oq3-v5-runs`, 486 files)
and `gpu-memory-lines.txt`. Raw daemon logs and run directories stay in the run base, listed by sha256 in `run-base-SHA256SUMS`.
