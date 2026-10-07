# OPEN-MODEL-QWEN3 v3: verdict

```text
spec          = ACCEPTANCE-v3.md, frozen at the merge of sc#286 (main cad36d9), before the run
verdict       = FAIL (scoring-v5, score-rows.sh exit 1): 156 declared rows, 153 PASS, 3 FAIL,
                none missing, none malformed, no extras, none NOT_APPLICABLE
failing rows  = G2/Q2 (phrase check), G3/G3-L (line count), G3/G3-RQ (line count, same cause)
                every launch finished by itself; none was cut at a token limit, none timed out
passing       = every row of G1, E1, E2, N1, N2 and R1; every row of G2 and G3 except the three above
score file    = oq3-v3-score.json; result lines oq3-v3-results.jsonl
```

v1 and v2 stand as recorded (VERDICT-v1.md; VERDICT-v2.md = FAIL, 85 of 115 rows). Nothing below changes
ACCEPTANCE-v3.md, any row, limit, goal, task or declaration; no launch was repeated and no extra attempt was run.
The prediction stated before the run (ACCEPTANCE-v3 Section 10) was FAIL (5 to 10 percent for a full pass) with the
failing rows in G2 or G3 on length, not in containment, approval, replay or recovery. That is what happened. The
prediction put G1 at about 40 percent and it passed completely.

Three points to keep apart from this result:

- **v2 failed on the 256-token cut, not on wall-clock time.** T4 and T6 were cut at 256 tokens (22.6 s and 23.9 s) and
  refused. v3 gave documents 1024 tokens and 120 s; no v3 reply came near either limit (longest 436 tokens, 43 s).
- **The v3 failures are different from the v2 failures.** They are about what the model wrote, not about a limit.
- **DIAGNOSTIC-LONG-1 is not qualification evidence.** It is measurement on an unmerged build and scores nothing; it is
  quoted below as context only.

## 1. Run

```text
binaries        = sovereign-core 8f3e8c8b879e10dd83883cee150f16508284d643 (main), omega 01f6a74636b8383b010cdb95597839582c415c27
                  (= omega.lock), physics 6d7cf0d, aienos b84c0a6, frozen build in /home/drakestapleton/workspace/oq3-v3-build
                  aien-cli       689027ea9ac09f52ec130a2d0b0310995b7539113ccd3a63b06f1037ad2cf4de
                  np1_reference  df91eede1223f454b7c6afd47589aac6406c8e69e393018fdbe3377b2103f686
                  np1_edit_merge abc345a47e872728e8b59d050ccf41794c78266e19da882ad6e3d1db681c6377
                  (the wrapper checked all three sha256 and the model digests before launching; sha256 of the files the run used:
                  evidence-v3/identity-part1.sha256, parts 2 and 3 matched)
wrapper, tasks  = run-qwen3-v3.sh, tasks-oq3-v3.json, oq3-v3.decl.json, rows-oq3-v3.jq, v3-rows.sh from a clean checkout of
                  main cad36d9 at /home/drakestapleton/workspace/oq3-v3-wrap (launcher: evidence-v3/run-part.sh)
model           = Qwen/Qwen3-4B-Instruct-2507 rev cdbee75f, Apache-2.0
environment     = AIEN_KV_CONTEXT_TOKENS=4096, AIEN_REQUIRE_BLACKWELL=1, AIEN_GB10_QWEN3_DECLARED_ATTEMPT=1; compose budgets
                  29000 / 120000 ms and caps per launch set by the wrapper; spin window and CTA budget at daemon defaults
evidence type   = GPU under Linux (GB10, native Omega engine, no CUDA); R1 reference on CPU
part 1          = quietlock hold 031756-oq3-v3-p1, 19:28:53Z to 19:37:35Z, exit 0 (G1 G2 G3)
part 2          = quietlock hold 031756-oq3-v3-p2, 19:37:41Z to 19:47:32Z, exit 0 (E1 E2 N1 N2)
part 3          = quietlock hold 031756-oq3-v3-p3, 19:47:41Z to 19:52:04Z, exit 1 = score FAIL (R1 and the score)
memory at start = part 1 MemFree 10225 MiB, MemAvailable 118150 MiB, Cached 93056 MiB
                  part 2 MemFree 37986 MiB, MemAvailable 117395 MiB, Cached 64826 MiB
                  part 3 MemFree 38044 MiB, MemAvailable 117460 MiB, Cached 64833 MiB
run base        = /home/drakestapleton/workspace/oq3-v3-runs (pinned, created by the wrapper);
                  sha256 of every file in evidence-v3/run-base-SHA256SUMS
```

The quiet flag was clear and no other hold or GPU job was live before each part. No retry, no tuning, no cache drop.
Part 1 started with only about 10 GiB MemFree (page cache 93 GiB), the crowded-memory condition omega#327 describes.

Per launch (attempts and tokens are from each launch's s3-report; "recognized" is `requirements_recognized`, information only;
"met" is the outcome of the RQ row, computed on the saved file):

| Launch | Receipt | Declared rows | Attempts | Tokens, finish, ms | Committed | Requirements recognized vs met |
|---|---|---|---|---|---|---|
| G1 doc, cap 1024 | `30b4c7ba...14fd2310.json` | 24 PASS | 1 | 210, eos, 20 148 | yes, 20 lines | recognized: at least 18 non-empty lines; met (rows L, RQ, F, SB PASS) |
| G2 doc, cap 1024 | `10b9db39...bf923592.json` | 23 PASS, 1 FAIL | 1 | 432, eos, 39 418 | yes, 26 lines | recognized: at least 25 non-empty lines; met (26); G2/Q2 FAIL, phrases missing |
| G3 doc, cap 1024 | `35f00c9f...4d4ee3d4.json` | 22 PASS, 2 FAIL | 1 | 436, eos, 43 064 | yes, 24 lines (13 non-empty) | recognized: none; not met: 13 of 35 lines, 6 of 6 headings |
| E1 edit, cap 96 | `b8596e86...332f054f.json` | 24 PASS | 1 | 27, eos, 5 343 | yes, edit re-derivation ok | none recognized (edit); n/a |
| E2 edit, cap 96 | `4de96580...5fc7e439.json` | 24 PASS | 1 | 38, eos, 6 449 | yes, edit re-derivation ok | none recognized (edit); n/a |
| N1 path outside workspace | `1b34519e...ccf11ec7.json` | 6 PASS | 3 | 12, eos, 3 094 / 3 386 / 3 636, each refused: path outside the workspace | no, zero effects | n/a |
| N2 cap 16 | `2827982b...ef37d02c.json` | 7 PASS | 3 | 16, max_tokens, 3 405 / 4 106 / 3 682, each refused: reply cut at the token limit | no, zero effects | n/a |
| R1 identity, cap 96 | `685d895f...13721324.json` | 23 PASS | 1 | 29, eos, 3 774; CPU reference: same 29 output ids | yes | none recognized; n/a |

(Receipt names are abbreviated; the full names are in INDEX.md. Every reply was inside the shared
deadline of its task: row `<id>-D` PASS on all eight launches, so no launch exceeded 29 s for edits or 120 s for documents.)

Every new v3 row passed except G3-L and G3-RQ (one cause, Section 2): saved bytes equal approved bytes (`<id>-SB`) on all six launches that committed, complete
document and no truncation at embedded fences (`<id>-F`) on G1, G2 and G3, one deadline for all attempts (`<id>-D`) on all eight,
and N2-R (a token-limit cut is not a timeout). G1 is the old DIAGNOSTIC-LONG-1 D5 case (three fenced examples, text after
them): saved whole, 20 lines, ending with the "## Summary" section.

The legacy receipt `verdict` field reads FAIL for the reason given in next-phase-1/VERDICT-v8 (the legacy containment rows
see the daemon's `compose.cortex-mark`) and is outside the verdict, as in v2.

GPU memory (omega#327): all 16 daemon starts (two per launch) logged `GPU session: open` and `Warm-up:` exactly once each.
No line names NV_ERR_NO_MEMORY or any other NV_ERR except the opt-in warning the daemon prints for a declared attempt
(`OMEGA_BACKEND WARNING: AIEN_GB10_QWEN3_DECLARED_ATTEMPT=1 ... omega#327 (NV_ERR_NO_MEMORY on low MemFree) is open`, once per
model call). Per-log counts: evidence-v3/gpu-memory-lines.txt. Expected none; none found.

## 2. Cause of the failing rows (read after the run, changes no row)

- **G2/Q2.** The goal asked for a release checklist "that covers preparing, testing, publishing and announcing a release". The
  inherited phrase check looks for the four words, case-insensitive. The reply has 26 lines (25 required, so G2-L and G2-RQ pass) and
  covers testing, but it does not contain the words "preparing", "publishing" or "announcing" ("Prepare a release announcement email",
  "Schedule a public announcement" use other forms). The model did what the goal asked in substance and missed the exact words
  the row requires. The row is the v8 Q2 row and was not changed.
- **G3/G3-L and G3/G3-RQ (one cause).** The reply has all six requested sections as headings (RQ recount: 6 of 6 headings found) and
  24 lines, of which 13 are non-empty: one paragraph under each heading, separated by blank lines. The goal asked for at least 35
  lines. The runtime's requirement validation does not recognize the G3 sentence ("at least 35 lines with at least 6 sections
  titled ...", ACCEPTANCE-v3 Section 12, finding 1), so `requirements_recognized` was empty, the 13-line reply was accepted on its first
  attempt (436 tokens, 43 s) and no retry took place. The length shortfall is therefore both the model's (it wrote short
  paragraphs) and a coverage gap in the validator that the spec already predicted. The outcome-based RQ row is what exposes it.
  It is a real result, not a scoring artifact: the saved file does not meet the stated requirement.

## 3. Comparison, as context only

- **v2 (VERDICT-v2.md, 85 of 115):** T4 and T6 failed because the reply was cut at 256 tokens and refused, so nothing was committed.
  In v3 all three documents finished by themselves, 210 to 436 tokens in 20 to 43 s, and committed. Wall-clock was not the v2 limit and
  was not reached in v3 either. v2's other launches (edits, negatives, replay) passed and v3's equivalents pass again.
  The two campaigns have different tasks, limits and row counts; 85/115 and 153/156 are not a like-for-like score.
- **DIAGNOSTIC-LONG-1 (sc#283, not qualification evidence):** five document goals on an unmerged build, 313 to 649 tokens,
  28.6 to 61.9 s, all finished by themselves; v3 documents (210, 432, 436 tokens; 20 to 43 s) sit in the same range. The diagnostic's
  short-lines case (D3, 13 lines when 20 were asked) resembles G3 here (13 non-empty lines when 35 were asked); its parser bug (D5) did not recur in G1.

## 4. What this verdict means

- The fixed limits and repairs work as designed on this build: no token-limit cut on any document, no timeout, a document with
  inner code fences saved whole (G1), saved bytes equal the approved bytes on every committed launch, token-for-token agreement with
  the CPU reference (R1), refusals for the outside path (N1) and the token-limit cut (N2) separate, containment, restart and recall pass.
- Qwen3-4B does not reliably meet every stated requirement of a long document in one answer: G2 missed required words and G3 wrote
  too few lines. Whether stronger requirement checking (so the runtime refuses and retries a short G3 reply), a different prompt,
  or a bigger model fixes it is a new campaign with its own frozen spec, not a rerun of this one.

## 5. Limits

- One run, one launch per task: one observation each, not a rate. Greedy decoding, one model, one machine.
- The tasks are fresh to Qwen3 but were chosen by the spec writer after seeing v2 and the diagnostic (ACCEPTANCE-v3 Section 11).
- The GPU dry run of the wrapper (ACCEPTANCE-v3 Section 9, item 8) did not happen; the real run was the first GPU run of this wrapper. It worked.
- The failing rows reflect the exact words and line counts the frozen goals and rows demanded; they say nothing about answer quality otherwise.
- omega#327 stays open: a run with no memory failure does not close it (this one started with low MemFree and still had none),
  and Qwen3 on GB10 stays off by default.
- ACCEPTANCE-v3 Section 7 says the freeze merge changes only files under `docs/campaigns/`; the binaries' source was
  checked by sha256 by the wrapper before each part.

## 6. Evidence

`evidence-v3/`: the launcher (`run-part.sh`, `hold.sh`), hold start, end and exit and logs for the three parts, memory before each
part, the identity sha256 files, the sha256 of every file in the run base (`run-base-SHA256SUMS`), per-daemon GPU memory line counts
(`gpu-memory-lines.txt`), and the earlier freeze files (build and gate scripts and summaries, build lines, test-rows-v8 summary,
scorer dry run). Receipts, summaries and `*.v3rows.json` side files (named by their own sha256), result lines `oq3-v3-results.jsonl`
and score `oq3-v3-score.json` are in this folder and listed in INDEX.md; committed replies are in `replies/`.
Added at review (e3d035): the three hold logs `hold-p1.log`, `hold-p2.log`, `hold-p3.log`, copied unchanged from the run records. The
run base on disk holds one file not listed in `run-base-SHA256SUMS`: `stage/tasks-v8.json`, a regular file where the rest of `stage/`
is symlinks. It is listed here so the 329 files on disk against 328 listed is explained; no row reads it.
