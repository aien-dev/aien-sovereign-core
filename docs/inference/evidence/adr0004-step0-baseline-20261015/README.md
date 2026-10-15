# ADR 0004 step 0: the frozen pre-migration 30B baseline, main 559f0f1e (2026-10-15)

CALC-EXEMPT: every measured number is copied from a log or reply in this folder; the four G30-speed
floors are the only derived numbers and each carries its calc id.

ADR 0004 section 7, step 0: seal on main, before step 1, the receipts every later step is gated
against. **The frozen baseline is `check-full-3/` (G30-tokens, G30-speed) and `door-2/` (G30-door
tokens).** Runs 1 and 2 of each are kept as corroboration and as the record of why run 3 was taken.

## Build
- sc main `559f0f1e4afba37fe66b1dddbe13b87d23bc04ab` (merge of #531, ADR 0004), fresh worktree, no tracked
  change outside this folder (each `*.start` records it). omega `4799f9eb` (omega.lock), physics `6d7cf0d4`,
  aienos `b84c0a67`, `libomega_gpu.a` sha256 `0629ad9cec0addd4ccc461f0523c2c3964964f0bafc0b98b678e5080b0373ae5`
  (the locked archive, same as the pinned `pin-bin-afe59ae1` and `pin-bin-f881710a` builds).
- `scripts/pins.sh --gpu`, then `. target/pins/env.sh` (AIEN_REQUIRE_NATIVE_LIBS=1) and
  `cargo build --release -p aien-serve-http -p aien-inference-abi --bins`.
- `own30b_check` sha256 `728a54c89c153c3381c698b2d5f4b4086aa2ca01ba66023274f81985f8ecaf71`;
  `aien-serve-http` sha256 `cabf718d7ff16a83cd2f00f79ef4ce28a89d442575a24fd58e6760d5fc997c51`.

## Commands
- `step0-runs.sh` (EV, SC, RUN env): one `quietlock hold --owner STEP0 --minutes 20` per run, released
  between runs. Each hold lasted under 4 minutes.
- check-full: `flock /tmp/aien-gb10.lock env AIEN_OMEGA_SPIN_US=2000 own30b_check chip <30B fp8>
  own30b-f-20261013T1745Z/golden.json <out> --prompts 1,2,3,4 --new 16 --layers 0,23,47 --weights fp8
  --attn fused --router device --lm-head gemv --prefill auto --force-memory`: the arguments of
  `own30b-idle-speed-20261015` check-full (the ADR's reference numbers) plus `--force-memory` (see below).
  The exact line is in each `check-full*.start`.
- door: `door-window.sh` starts `aien-serve-http --engine omega --model-dir <30B fp8> --port 18103`
  (no AIEN_* or OWN30B_* env: the served defaults, spin 2000 us), sends own30b_check's 4 prompt texts
  (`prompts.json`, copied byte for byte from `own30b_check.rs` PROMPT_TEXTS) as one user message each with
  seed 1, temperature 1.0, top_p 1.0, top_k 0, max_tokens 64, stream false, twice (passes 1, 2), stops the
  server with SIGTERM, starts a second process and sends them once more (pass 3). Every reply reports
  `mode=sampled seed=1` in `usage.aien_sampling`; the door's `SamplingObserver` reads the full logits every step.

## Machine state
Every `*.start` and `*.end` records load average, MemAvailable, compute apps, GPU state, listeners and the
line count of the :18091 server log. In every run the only other GPU process was the idle exp445 30B server
on :18091 (36424 MiB); its log stayed at 899 lines from the first start to the last end, so it served no
request during any run. Before each check-full run: GPU 0 %, P8. The door runs started right after a chip run
(`door.start` 1 %, P0; `door-2.start` 0 %, P0). Other listeners: cortex :18080, spark-mail :18092, spark-cockpit
:18095 (none is a model server); no :18097.

## G30-tokens and G30-speed (`check-full-3/`, frozen)
`run.json` pass true; `SUMMARY parity=PASS tokens=PASS`; parity PASS on layers 0, 23, 47; 10 of 10
controls CAUGHT; prefill chunk 64 vs 1 PASS; gemm-mutants-off PASS. Tokens IDENTICAL to the golden on all
4 prompts (p2's golden carries the stop token 151645, which the engine consumes; `first_diff` null, as in
every earlier receipt). The chip token lists of all three runs hash to the same value as the
`own30b-idle-speed-20261015` check-full (sha256 of `jq -c '[.prompts[] | .chip]'`: `bb1a0401...b22b63f`).

| decode tok/s | p1 | p2 | p3 | p4 | 1-minute load average, start to end |
|---|---|---|---|---|---|
| run 1 (`check-full/`) | 37.5 | 32.6 | 36.9 | 34.7 | 8.93 to 18.06 |
| run 2 (`check-full-2/`) | 39.0 | 36.1 | 37.6 | 34.0 | 2.58 to 3.51 |
| **run 3 (`check-full-3/`), frozen baseline** | **39.1** | **36.1** | **37.5** | **36.6** | 2.08 to 11.63 |
| ADR reference, `own30b-idle-speed-20261015` | 38.8 | 36.1 | 37.2 | 36.6 | 0.55 at start |

Run 1 shared the CPU with other agents' compiles (several rustc at 100 %); its per-step expert and attention
times rose (p2 experts 16.3 ms against 14.5). Run 2 was clean on p1 to p3 but p4 took 11.2 ms attention per
step against 10.1 to 10.4 in run 3 and the ADR receipt. Run 3 matches the ADR's numbers to within 0.3 tok/s.
G30-speed floors from run 3 (97 percent): p1 37.927 (calc:ca6917a169f4), p2 35.017 (calc:db0cafe07493),
p3 36.375 (calc:0cc180e77663), p4 35.502 (calc:d0e63d84b0e7). Each is at or above the ADR's
"at least" floors 37.6 / 35.0 / 36.1 / 35.5. Like the ADR's, they are taken from the one-decimal values
the tool prints (the same convention a later step's printed numbers are compared with); `run.json` holds
the unrounded rates. Run 3's load rose to 11.63 by its end, so its p4 figure was measured with other
work starting; it still equals the ADR reference.

## G30-door (`door-2/`, frozen)
Reply content sha256 (byte-exact, `jq -j '.choices[0].message.content'`), identical in all 3 passes of
`door-2/` and all 3 passes of `door/` (6 replies per prompt, 24 in all, from 4 server processes):

| prompt | prompt tokens | reply tokens, finish | content sha256 | reply_logprob |
|---|---|---|---|---|
| p1-one-token | 9 | 11, stop | 7183808e3236265d2eb507c321e124b5ee6be06c7b799833adbf8d01cf70a8ed | -2.075414983119233 |
| p2-short | 20 | 7, stop | a1b7eb2ee7a6aded8dda4e6cf30826f5afffb28a5597ee9389e91eb326d4e319 | -0.00006239156406184776 |
| p3-medium | 204 | 64, length | 58634d9dfa7d324e2b4f9ce3a202f3007cb3b7e635892cc7844d98c70c573fce | -9.451717192233671 |
| p4-long | 657 | 64, length | 8e1d40eaf6d97fc40ba53e3be573094443484be2976788187c63b9ad88031d9e | -10.717370043619697 |

Door decode tok/s (server log lines, `door-2/session-*.server.log`), passes 1 / 2 / 3:
p1 35.37 / 35.83 / 34.81; p2 35.35 / 35.53 / 35.00; p3 35.16 / 35.22 / 34.39; p4 34.73 / 34.88 / 34.10
(passes 1 and 2 in `session-a`, pass 3 in `session-b`).
Run 1 (`door/`): 33.33 to 35.71. Every figure is above the G30-door speed floor 32.95, which the ADR sets
from the `own30b-idle-speed-20261015` door receipt (33.97 / 33.99), not from this folder.

## Limits
- The door path reports the reply text, not token ids. The baseline is the byte-exact content digest plus
  the reply token count and `reply_logprob` (the sum of the sampled tokens' log-probabilities); two different
  token lists matching on all three is not excluded but not plausible.
- p1 through the door is the chat-templated "Hello" (9 tokens); own30b_check feeds the raw one-token prompt,
  so the door's p1 reply is not comparable to the golden, only to later door receipts.
- The door prompts differ from the `own30b-idle-speed-20261015` door receipt (750 and 822 token prompts),
  so this folder's door tok/s is reference only; the ADR's floor stands as written.
- The idle :18091 server was left running (owner's instruction); it served nothing during the runs, but the
  ADR's reference receipt had no second model resident. With it resident own30b_check printed 64.4 to
  69.5 GB available before load, below its 70 GB guard, hence `--force-memory` (the guard only refuses;
  it changes no setting).
- Machine load could not be held low: other agents' work does not stop for the quiet flag once started.
  Later steps should take their G30-speed receipts at a comparable load (each `*.start` records it).
- Environment: the `*.start` files do not list environment variables. The runner was started from a fresh
  tool shell, separate from the build shell that sourced `target/pins/env.sh`; such a shell carries no
  AIEN_* or OWN30B_* variable (checked after the runs, not recorded during them: INFERRED for the runs).
  `door-window.sh` additionally unsets AIEN_OMEGA_SPIN_US and OWN30B_DECODE_GRAPH for the server.
- Reuse by a later step: the scripts are kept exactly as run. `step0-runs.sh` hard-codes this folder's name
  in its "tracked changes" filter (the count also includes untracked files) and "main 559f0f1e" in the
  quietlock reasons; change both, and record `env | grep -E '^(AIEN|OWN30B)_'` in each `*.start`.
- Receipts were taken on 559f0f1e; the PR was later rebased onto main e3b36d5, which adds only
  spark-supervisor changes (no inference code).
