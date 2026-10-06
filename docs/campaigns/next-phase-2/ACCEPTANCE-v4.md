# NEXT-PHASE-2: acceptance amendment v4 (frozen before any code of this cut)

```text
campaign_id  = "next-phase-2"
spec_version = 4
amends       = ACCEPTANCE-v3.md spec_version 3 (unchanged; this file supersedes it row by
               row where it says so; every v3 row, rule and build step not named here stays
               as written, including the v2 rows v3 kept)
status       = FROZEN at the commit that adds this file. It is committed alone, before any
               code of the cut. Later changes need ACCEPTANCE-v5.md.
base         = sovereign-core main fef16ad (contains #228); omega.lock 62b6a28
reason       = VERDICT-v3.md: one FAIL row, C7a (the forced panic aborts the daemon)
```

Every claim has a source: sovereign-core file:line at fef16ad, or a command run before this
file was written. Anything else is marked UNVERIFIED.

## 1. Findings this amendment is built on

| # | Finding | Evidence |
|---|---------|----------|
| H1 | Release builds abort on panic: `Cargo.toml:58` `panic = "abort"` in `[profile.release]`. A panic inside the start-up `spawn_blocking` task ends the process, so the `Err(e)` arm at `crates/aien-runtime/src/server.rs:192-198` (`Reconcile: failed:`; VERDICT-v3 section 3 cites it as 190-197; the code is unchanged since its run, the arm spans 192-198) cannot run in a release build. Both campaign builds are release builds (ACCEPTANCE-v2 section 4) | source; VERDICT-v3 section 3 (3 of 3 C7a runs aborted) |
| H2 | The error arm that release builds do take: `reconcile_at_start` maps a `ControlResponse::Error` from the start-up reconcile to `set_reconcile_failed("refused: …")` and the line `Reconcile: refused: <e>; effect commands refuse until a successful reconcile (aien compose reconcile)` (`crates/aien-runtime/src/effects.rs:831-834`). `open_intent` then refuses through `reconcile_gate` (`effects.rs:486-495`, `EFFECT_REFUSED ReconcileFailed`) | source; the same arm is proven by VERDICT-v3 C6a..c, C6e..g, C6i, C7b |
| H3 | The socket is bound before the start-up reconcile runs (`server.rs:97` bind, reconcile spawned at `server.rs:188`), so a start-up abort leaves a socket file with no process behind it. The next daemon start removes a socket file that refuses connections before it binds (`server.rs:80-83`) | source |
| H4 | **Real panic, observed.** The v3 `cpu-fault` binary (sha256 `7aa59474…bf846b`) started with `AIEN_FAULT_HOLD=reconcile_panic` on a byte copy of F0v3: exit status 134 (SIGABRT, `Aborted (core dumped)`); log `panicked at crates/aien-runtime/src/effects.rs:807:9: fault hold reconcile_panic: …`; the home (compose dir, machine root, record mark) hashed identical before and after; a socket file `aien.sock` was left; `aien compose recall` answered `{"ok":false,"error":"Failed to connect to AIEN runtime socket at …: Connection refused (os error 111)"}`; provenance and state dirs stayed empty | command, run once before this file was written, on a scratch copy (`np2v4-probe`), not part of any receipt |

## 2. What this cut adds

1. **Error fault hook** (test build only, feature `fault-hold`): `AIEN_FAULT_HOLD=reconcile_error`
   in the daemon's environment makes `reconcile_at_start`, on a home that exists (after its
   `cortex.cx` check), take the reconcile result to be
   `ControlResponse::Error("fault hold reconcile_error: forced start-up reconcile error (test build)")`
   instead of calling `reconcile`. The result then goes through the unchanged match of H2, so the
   production error arm (gate set, `Reconcile: refused:` line) is what runs.
2. **Panic hook kept unchanged**: `AIEN_FAULT_HOLD=reconcile_panic` still panics at the top of
   `reconcile_at_start` (v3 2.6). It is now tested for what release builds do (row C7c).
3. The default build has no fault code: neither `reconcile_error` nor `reconcile_panic` appears in
   the `gpu` binary; both appear in `cpu-fault`. Recorded in `link_proof`.

No production behaviour changes in this cut.

## 3. Rules for every run

R1..R6 of ACCEPTANCE-v3 unchanged. R6 (no `E_MARK`) also covers C7c.

## 4. Fixture, builds, controls (supersede v3 section 4 where named)

- **Builds** as v3 (`gpu`, `cpu-fault`, same source commit, same build environment variables,
  `link_proof` in the receipt), from the code commit of this cut.
- **Fixture F0v4**: v3's F0 procedure on the new `gpu` build (Llama-3.2-1B-Instruct snapshot
  5a8abab, `AIEN_COMPOSE_MAX_TOKENS=96`), inside one quietlock hold of at most 20 minutes with
  start and release whispers, only when `~/workspace/.spark-quiet` is absent. Valid only if S3
  committed. **Fallback (pre-registered):** if F0v4 cannot run or is not valid, the cases run on
  F0v3 (`np2v3-fix`, `fixture.json` `files_sha256` `f09af742…9a`, re-checked before use) and the
  C3 control row is NOT_RUN.
- C8a input: F0v2 as in v3 (files sha re-checked before use).
- Controls and repetitions as v3: 3 per injected row, C6i exactly once.

## 5. The rows (supersede v3 section 5 for the rows named; others unchanged)

| Row | Injection (exact) | PASS iff (rules R1..R6 also hold) |
|-----|-------------------|-----------------------------------|
| C7a reconcile returns an error | daemon started with `AIEN_FAULT_HOLD=reconcile_error` on a home with history; then: authorize A1, execute A1, `aien compose reconcile`, execute A1, shutdown, restart without the hook | the start line starts `Reconcile: refused: fault hold reconcile_error` and ends `effect commands refuse until a successful reconcile (aien compose reconcile)`; the daemon serves (socket up); authorize answers; the first execute is refused `ReconcileFailed`, no intent, no write; the reconcile answers ok; the second execute is DONE, the target written once; restart line normal (`checked 0`) |
| C7c real panic at start (new) | daemon started with `AIEN_FAULT_HOLD=reconcile_panic` on a home with history; after it exits, restart without the hook and **without removing the left socket file**; recall; effects; shutdown | **daemon exits, no socket, no write**, read as: the process exits by itself with a non-zero status within the start timeout; its log names `fault hold reconcile_panic`; no process serves the socket (an `aien compose recall` sent while the hook daemon is gone fails to connect, `ok` false); the home (compose dir, machine root, record mark) hashes identical before the start and after the exit; the target file unchanged; provenance dir empty. Then the restart removes the left socket file itself and comes up with a normal reconcile line (`checked 0`); recall ok; the effect ledger holds no intent. Whether a socket file is left after the abort is recorded (a check that always holds, value = present or absent), not scored (H3) |
| C6d | as v3 (guarded; NOT_APPLICABLE is a row result, never PASS) | as v3 |
| C6i | as v3 (exactly once) | as v3 |

## 6. Verdicts (supersede v3 section 6 where named)

- Row verdicts as v3: PASS, FAIL, NOT_RUN, NOT_APPLICABLE (only C6d, only through its guard),
  UNVERIFIED. None of NOT_RUN, NOT_APPLICABLE, UNVERIFIED is PASS.
- Campaign PASS = every row of v2 and v3 section 5 not replaced, every row of section 5 here
  (C7a, C7c), and the C3 control, PASS; C6d PASS or NOT_APPLICABLE (named in the verdict, not
  counted as a pass); F0 valid.
- Declared NOT_PROVED (not rows): the v3 list, plus: the `Err(e)` arm of `server.rs:192-198`
  (unreachable in release builds, H1; no row exercises it); the cause of a real start-up panic
  (C7c proves only that the daemon stops before serving and writes nothing).
- Receipt: v3 format, `acceptance_spec` = this file; named by its sha256, never edited, indexed,
  `.summary.txt` beside it, and a `VERDICT-v4.md` written after the run.
