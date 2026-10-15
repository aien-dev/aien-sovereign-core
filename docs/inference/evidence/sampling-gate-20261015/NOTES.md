# OWN-8B stage S: sampling gate on the 30B, 2026-10-15

Design: `~/handoffs/2026-10-15-own8b-design.md` sections 4 (row S) and 4.1.
Question: the pinned 30B server on :18091 gave byte-identical replies for the three Glyph
repro requests (seeds 446001, 447001, 448001; temperature 0.7, top_p 0.8). Does the engine
ignore the seed, or is the model confident?

**Answer: the model is confident; the seed is honoured.** Every number below is copied from
the files named next to it.

## What the record measures

Each reply carries `usage.aien_sampling` and the server log line ends with the same record:
mode, seed used, temperature, top_p, top_k, one kept-set size per sampled draw (and
min/median/mean/max), the top token's probability (min/mean), `reply_logprob` and
`p_repeat = exp(reply_logprob)`: the chance that an honest draw with another seed repeats this
reply's sampled tokens exactly. The kept set at top_p 1.0 is the whole vocabulary (151936), so
it says nothing about confidence there; `p_repeat` does.

## Repro requests (gate d), release build, window 2 (`chip2/release/gate.txt`)

| request | content_sha256 | kept (min/med/mean/max) | kept1 | top_prob min/mean | p_repeat |
|---|---|---|---|---|---|
| seed 446001 | 117f6000b6491ea5 | 1/1/1.0208/2 | 47 of 48 | 0.76888/0.99030 | 0.76888 |
| seed 447001 | 117f6000b6491ea5 | 1/1/1.0208/2 | 47 of 48 | 0.76888/0.99030 | 0.76888 |
| seed 448001 | 117f6000b6491ea5 | 1/1/1.0208/2 | 47 of 48 | 0.76888/0.99030 | 0.76888 |

Server log (`chip2/release.server.log`): `sampling mode=sampled seed=446001 temp=0.7 top_p=0.8
top_k=20 greedy=0 sampled=48 kept(min/med/mean/max)=1/1/1.02/2 kept1=47/48
top_prob(min/mean)=0.7689/0.9903 p_repeat=7.689e-1`. The request sets no top_k; the server
default top_k=20 applied.

Reading: in 47 of 48 steps top_p 0.8 keeps a single token (the top token alone holds more than
80%). In the one remaining step two tokens survive and the top one has probability 0.76888.
Three honest seeds repeat that reply with chance 0.76888^2 = 0.59117 (gate line:
`p_repeat^2=0.59117 >= 0.01`, `steps_with_choice=3/144`). Identical replies are the expected
outcome, not a fault.

## Gates, window 2 (2026-10-15 11:30:23 to 11:33:53 UTC, `chip2/window.log`)

Builds from commit in `host/built-from-commit.txt`; sha256 in `host/binaries.sha256`
(release 4f6df9c0..., debug-assertion build 6678d78b...). Neither links a CUDA library.

| server | a replay | b spread | c greedy | d repro | verdict |
|---|---|---|---|---|---|
| release | 4/4 PASS | 4/4 PASS | 4/4 PASS (p1 equal to the pinned golden) | PASS (confidence, 0.59117) | PASS |
| mutant ignore-seed | 4/4 PASS | 0/4 FAIL | 4/4 PASS | FAIL (seed not its own) | FAIL |
| mutant force-greedy | 4/4 PASS | 0/4 FAIL | 4/4 PASS | FAIL (not sampled) | FAIL |

Gate (b) per prompt, release (`chip2/release/gate.txt`), seeds 1, 2, 3 at temperature 1.0,
top_p 1.0: p1 distinct 3/3; p2 distinct 2/3; p3 "Name three colours." distinct 1/3 with
p_repeat 0.98326 (p_repeat^2 = 0.966806, confident, PASS); p4 distinct 3/3.
Mutant ignore-seed: distinct 1/3 on all four prompts, record seed=0 (for example
`chip2/mutant-ignore-seed.server.log`: `seed=0 ... p_repeat=2.898e-10 MUTANT=ignore-seed`), so
p1 and p4 fail on distinctness alone, not only on the self-report.
Mutant force-greedy: mode=greedy, sampled=0.

Baseline greedy reply of the pinned server to prompt 1 (`chip2/baseline-pinned-nonstream.json`):
"A Grace Blackwell GB10 is a high-performance, limited-edition electric motorcycle designed by
Grace Blackwell, ...". It differs from the 2026-10-13 smoke ("electric sports car"), so gate (c)
uses the pinned server's own reply of today as the golden. The release build matched it.

Release server ready in 54.1 s (cold), mutants 15.4 s and 15.3 s. The pinned server was
restarted from `~/handoffs/own30b-serve-cmdline-merged.txt` (pid 2420493, own session),
`/v1/models` answered at 11:33:52Z. Quiet hold about 4 minutes.

## Window 1 (11:09:34 to 11:12:26 UTC, `chip/`), earlier rule

The first window ran an earlier gate: (b) needed 2 distinct replies of 3 for every prompt, and
(d) used kept_mean <= 1.5. Release: a 4/4, b 3/4 FAIL (p3 "Name three colours." 1/3 distinct),
c 4/4, d PASS (kept_mean=1.0208, steps_with_choice=3/144). Both mutants also passed (d) under
that rule, so kept_mean could not tell a confident model from a seed-ignoring engine. That,
and the review, led to the confidence record (`p_repeat`) and the rule above: a prompt with
identical replies passes only when an honest sampler would repeat them with chance at least
1%, and every reply must report mode=sampled with its own seed. Window 1 content hashes were
taken through shell command substitution; window 2 hashes the exact bytes, so the two windows'
hashes are not comparable. Window 1 restore left its launching shell waiting (it inherited the
exit trap); fixed in `chip-window.sh` (trap cleared in the child, `setsid -f`), and window 2
exited cleanly.

## Host (`host/`)

- Red: `red-loopback.txt`, the loopback gate tests before the record existed: 3 failed (the
  record tests), 3 passed (the seed spread, the two mutant engines). The chain honoured the seed
  before any change.
- Green: `green-serve-http.txt` (all aien-serve-http tests pass), `green-abi.txt` (168 passed).
- `release-refuses-mutant.txt`: a release build exits with code 2 when AIEN_SAMPLING_MUTANT is set.
  `dbgassert-accepts-mutant.txt`: the debug-assertion build prints the MUTANT ACTIVE banner.

## Notes

- Crumbs: the brief named crumb tool d0f0b95d; CI verifies with aien-architecture tool 94fffbcf,
  which calls d0f0b95d output stale. Crumbs were compiled with 94fffbcf. Running d0f0b95d in a
  worktree also rewrote `.crumb` files inside `target/pins` and a build cache; those were restored
  from git and the cache deleted before the window 2 builds.
- Repro request texts belong to another experiment: only usage, finish reason and content sha256
  are kept (`REDACT_REPRO=1`).
