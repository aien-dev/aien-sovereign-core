# NEXT-PHASE-2: acceptance amendment v3 (frozen before any code of this cut)

```text
campaign_id  = "next-phase-2"
spec_version = 3
amends       = ACCEPTANCE-v2.md spec_version 2 (unchanged; this file supersedes it row by
               row where it says so; every v2 row not named here stays as written)
status       = FROZEN at the commit that adds this file. It is committed alone, before any
               code of the cut. Later changes need ACCEPTANCE-v4.md.
base         = sovereign-core main aec763c (contains ee07aa3 = #227); omega.lock 62b6a28
reason       = VERDICT-v2.md: C6c FAIL, C6d NOT_RUN, C2a UNVERIFIED, reconcile-failed gap
```

Every claim has a source: sovereign-core file:line at aec763c, omega file:line at 62b6a28
(`om/` = omega `src/runtime/`), or a command run before this file was written. Anything else
is marked UNVERIFIED.

## 1. Findings this amendment is built on

| # | Finding | Evidence |
|---|---------|----------|
| G1 | **The 5 records every daemon start appends** (VERDICT-v2 section 3, counts 23 -> 28 -> 33) are the five `CX_K_ENTITY_CREATED` records (class `CX_ENTITY` = 1, kind 0x10002 = 65538), one per resident World object of the composition, subjects 1..5 = goal, candidate 0, candidate 1, verdict, state. Chain: an existing home makes the daemon open it at start (`reconcile_at_start`, `crates/aien-runtime/src/effects.rs:767-770`, called from `server.rs:183`) -> `with_home` -> `open_home` -> `compose.info()` (`spine.rs:928`) -> `rxc_host_info` seals (`om/rxc_host_abi.c:429-431`) -> `host_seal` -> `rx_compose_open` (`om/rxc_host_abi.c:216`) -> `build_world` -> `build_in_world` creates the 5 objects (`om/rx_compose.c:733-735`, `rx_world_create` emits one `RX_CRUMB_CREATE` crumb each, `om/rx_world.c:2210-2221`) -> `rx_cortex_attach` (`om/rx_compose.c:819`) -> `link_add` journals every crumb already in the World (`om/rx_cortex_record.c:205`) -> `record_link` maps CREATE to `CX_ENTITY`/`CX_K_ENTITY_CREATED` (`om/rx_cortex_record.c:45`), subject = object id + 1 (`om/rx_cortex_record.c:57-58`) | Command (base aec763c, cpu build, copy of the v2 fixture F0): three daemon starts; recall counts 22, 27, 32; `recall --ids 28..32` after the third start returned exactly `{cls 1, kind 65538, subject 1..5, tag 1, links 0, note null}`. `om/rx_cortex.h:89-93` (CX_K_BASE 0x10000, ENTITY_CREATED = base + 2) |
| G2 | A record-boundary cut that drops only host notes written after the last composition commit is not detected (C6c FAIL): the journal header carries no count (`om/rx_compose.c:885-891`), the J-Space anchor moves only at `commit_js` (`om/rx_compose.c:508-516`) | VERDICT-v2 section 1; receipt `dc0c88a8…` C6c rows |
| G3 | `Compose::open` (`rxc_host_open`) appends nothing: it checks identity and probes the journal read-only, `info.records = probe.n` (`om/rxc_host_abi.c:251-281`). Every later call (`info`, `record`, `recall`, `note`, `run`) seals the home first, which appends the 5 records of G1 (`om/rxc_host_abi.c:341,388,408,419,431,486`) | source |
| G4 | **Reconcile-failed gap.** `server.rs:181-187`: the start-up reconcile runs in `spawn_blocking`; a panic becomes `Reconcile: failed: …` via `unwrap_or_else`; an error becomes `Reconcile: refused: …` (`effects.rs:791`); in both cases the daemon goes on to the accept loop (`server.rs:189`) and `open_intent` (`effects.rs:560-575`) does not consult the start-up result | source |
| G5 | `jspace.data` is never written at omega 62b6a28 through the composition: bytes land there only via `spill_u` (`om/rx_jspace.c:567`), reached from `js_real_spill` (`:1325`) and `js_forge_enforce` (`:1344`), and from EDIT patches (`js_branch_edit`, `:1333`); no omega source outside `rx_jspace.c` and tests calls any of the three (grep). `spill_end` is the u64 at offset 48 of `jspace.meta` (`om/rx_jspace.c:1635,1714`); a data file shorter than it is refused (`:1721`). F0v2: `jspace.data` 0 bytes, `spill_end` 0 | source; `od -An -tu8 -j48 -N8` of F0v2 `jspace.meta` = 0 |
| G6 | A deleted `jspace.meta` opens as "no checkpoint yet" and the data file is truncated to 0 (`om/rx_jspace.c:1914-1919`); whether the composition then refuses (state records name branches that no longer exist, `om/rx_compose.c` `recover`) is UNVERIFIED | source |
| G7 | `rxc_host_recover` on a whole, anchored journal changes nothing and sets `records_kept` (`om/rxc_host_abi.c`, `if (!out->tail_torn && A <= kept) goto done`); when it cuts, it appends one `RXC_HOST_TAG_REPAIR_TAIL` record and runs a trial open (which appends G1's 5 records) | source |

## 2. What this cut adds (the production behaviour the rows test)

1. **Cortex record mark.** A file `<compose dir>.cortex-mark` (beside `.machine-root`, mode
   0600, never inside the compose dir), 128 bytes: `"AIENCXM1"` | u32 version 1 | u32 0 |
   machine id (32) | u64 seq | u64 records M | digest of record #M (32, zeros if M = 0) |
   sha256 of the first 96 bytes. Written only after an append returned (the append is already
   `fdatasync`ed, `om/rx_cortex.c` `CX_OPEN_SYNC`): write `.tmp`, `sync_all`, rename, sync the
   parent dir. It advances after every daemon operation that may append (end of the home open,
   after every command run on the open home, after every composition run).
2. **Load check** (home open; count before anything is appended, G3):
   | State | Action | Daemon text |
   |---|---|---|
   | mark unreadable, wrong length/magic/version/sha, or another machine id | refuse, touch nothing | `record mark damaged (E_MARK): <why>` |
   | journal records F < mark M | refuse, touch nothing | `Cortex journal holds F records, its record mark says M: M-F record(s) lost at a record boundary (E_MARK_TRUNCATED, mark seq S). Run RecoverComposeHome (aien compose recover)` |
   | F >= M, record #M digest differs | refuse | `Cortex record #M differs from its record mark (E_MARK_DIGEST, mark seq S)` |
   | F = M, digest equal | open | none |
   | F > M, digest equal | open, advance | `Cortex mark: record mark advanced M->F (records written before an interrupted mark update; not corruption)` |
   | no mark, journal absent or empty | open, create | `Cortex mark: mark created for a new home (seq S, records R)` |
   | no mark, journal present (old install, or a deleted mark) | open, adopt, never refuse | `Cortex mark: mark adopted (seq S, records R): this home had no record mark; a loss before this start cannot be ruled out` |
   Refusals use the existing form `compose home <dir> refused: …` (`spine.rs:605-617`), so they
   appear in the `Reconcile: refused:` start line, in `recall` and in every effect command. The
   digest check needs the open home, so it runs after the 5 start-up records of G1 are
   appended (G3); the count check runs before. A refusal never rewrites the mark.
3. **Recover keeps the old mark.** `aien compose recover`: after omega's own repair, if the mark
   is damaged, ahead of the records kept, or names a different record #M, it is renamed to
   `<compose dir>.cortex-mark.lost-<seq>` (`.lost-damaged` when the seq cannot be read; never
   overwritten, never deleted), the home is opened, a fresh mark is written (seq = old seq + 1),
   and one host `constraint` record is appended with text
   `{"repair":"cortex-mark","mark_records":M,"journal_records":F,"lost":L,"old_seq":S,"old_digest":…,"kept_as":…}`.
   The recover answer gains `mark_lost`, `mark_kept_as`, `mark_repair_record`. Never automatic.
4. **Reserved repair key** (DECIDED 2): `ComposeNote` refuses a `constraint` text with a `repair`
   field (beside v2 2.7). Omega stays at 62b6a28; no new omega note kind.
5. **Reconcile-failed refusal (new rule, G4).** If the start-up reconcile panics or returns an
   error (`Reconcile: failed:` or `Reconcile: refused:` or `Reconcile: unexpected`), the daemon
   keeps serving but every **effect command** (the intent and the ack of `aien compose execute`)
   is refused `EFFECT_REFUSED ReconcileFailed`, until an operator `aien compose reconcile`
   (without `--declare`) succeeds in this daemon. Authorize, stop, resume, revoke, recall,
   effects, recover and reconcile stay available. The start line says so:
   `…; effect commands refuse until a successful reconcile (aien compose reconcile)`.
6. **Fault hook** (test build only, feature `fault-hold`, now forwarded to aien-runtime):
   `AIEN_FAULT_HOLD=reconcile_panic` in the daemon's environment makes the start-up reconcile
   panic inside its `spawn_blocking` task, i.e. exactly the `unwrap_or_else` path of G4. The
   default build has no fault code (checked: `reconcile_panic` absent from the release binary).

Not claimed: tamper resistance. The mark's sha256 is unkeyed; rewriting journal and mark
together, deleting the mark (adopted and reported, not refused), or rolling back compose dir and
mark together are not detected (declared NOT_PROVED, section 6).

## 3. Rules for every run

R1..R5 of ACCEPTANCE-v2 section 3 unchanged. New:

- **R6 no false corruption report.** In every control and every row C1..C5, C8, no daemon start
  line, recall or execute answer contains `E_MARK`.

## 4. Fixture, builds, controls (supersede v2 section 4 where named)

- **Builds** as v2 (`gpu`, `cpu-fault`), same source commit, both recorded with sha256 and the
  build environment: `AIEN_OMEGA_COMPOSE_DIR` (omega checkout at 62b6a28), `AIEN_PHYSICS_DIR`
  (physics at omega's `physics.lock`), `AIEN_AIENOS_LOCK_REPO=/home/drakestapleton/workspace/aienos-repo`
  (holds omega's `aienos.lock`), `gpu` also `AIEN_OMEGA_GPU_LIB` (prebuilt `libomega_gpu.a`
  from omega 62b6a28). The receipt records proof that the real compose library is linked in
  both builds (marker strings of `librx_compose.a` in the binary) and the GPU engine in `gpu`.
- **Fixture F0v3**: v2's F0 procedure on the new `gpu` build with the NEXT-PHASE-1 v5 frozen model
  (Llama-3.2-1B-Instruct snapshot 5a8abab, `AIEN_COMPOSE_MAX_TOKENS=96`), inside one quietlock
  hold of at most 20 minutes with start and release whispers, only when
  `~/workspace/.spark-quiet` is absent. F0v3 is valid only if S3 committed.
  **Fallback (pre-registered):** if F0v3 cannot run or is not valid, the cases run on F0v2
  (`np2-fix`, files sha256 `3e4e26ef…eaf8` re-checked before use), the C3 control row is NOT_RUN,
  and each case's first start adopts the mark (expected, reported, scored by R6 only for
  `E_MARK`).
- **F0v2 is also the old-install input of C8a** (written by sovereign-core b1cf3be, which has no
  mark code; files sha re-checked before use).
- Every run copies `compose.cortex-mark` with the compose dir when it exists, and includes it in
  the home hashes (C3a) and in the pre-damage copy (C6).
- Controls: v2 control (once per case family) for C1, C2, C4, C5, C6, C7, C8.
- **Repetitions:** 3 per injected row, except C6i: exactly once (DECIDED 4).

## 5. The rows (supersede v2 section 5 for the rows named; others unchanged)

| Row | Injection (exact) | PASS iff (rules R1..R6 also hold) |
|-----|-------------------|-----------------------------------|
| C2a (rule frozen, G1) | as v2 | executor refuses, no write, no intent; host-record count unchanged across the restart; `records_total` after the restart = before the kill + 5, and records before+1 .. before+5 are exactly class 1, kind 65538, subjects {1,2,3,4,5} once each (G1); after restart the same authorization executes once to DONE |
| C6c | as v2 (cut `cortex.cx` at the boundary before the last record, which is grant A2) | start line `Reconcile: refused` and `recall` refused, both naming `E_MARK_TRUNCATED`; execute A2 refused; no write; `cortex.cx` and `compose.cortex-mark` byte-identical to their damaged-state copies after the case |
| C6c-ctl (control, crash between append and mark update) | copy `compose.cortex-mark` just before `authorize A2`; after the daemon stops, put that copy back (the on-disk state of a process that died after the A2 append and before its mark update; simulated, not a real kill) | NOT reported as corruption: the home opens, the start log has `record mark advanced M->M+1`, recall ok, no `E_MARK` anywhere, execute A2 reaches DONE once |
| C6d (guarded, DECIDED 3) | if `jspace.data` is non-empty or `spill_end` (meta offset 48) > 0: truncate `jspace.data` to `spill_end - 1` bytes (or by 1 byte if `spill_end` is 0) and score by the v2 C6 rule. Otherwise run the same case uninjected and record NOT_APPLICABLE; at the end of the case the harness asserts `jspace.data` is still 0 bytes, and if it is not, it stops the daemon, applies the truncation, restarts and scores the C6 rule on a fresh grant A3 | NOT_APPLICABLE is a row result, never PASS, listed as such in the verdict. Injected: the v2 C6 rule |
| C6i (once, DECIDED 4) | delete `jspace/jspace.meta` (daemon stopped) | the v2 C6 rule, and after the case `jspace.meta` is still absent (no silent re-creation) and `cortex.cx` is byte-identical to its pre-damage copy apart from appended records |
| C6a, C6b, C6e, C6f, C6g, C6h | as v2 | as v2; plus `compose.cortex-mark` unchanged by a refused start |
| C7a reconcile forced to fail | daemon started with `AIEN_FAULT_HOLD=reconcile_panic` on a home with history; then: authorize A1, execute A1, `aien compose reconcile`, execute A1, shutdown, restart without the hook | start line `Reconcile: failed:` naming the refusal of effect commands; daemon serves (socket up); authorize answers; first execute refused `ReconcileFailed`, no intent, no write; the reconcile answers ok; second execute DONE once; restart line normal (`checked 0`) |
| C7b reconcile refused, then recover | C6c's injection; start (expect `Reconcile: refused … E_MARK_TRUNCATED`); `aien compose recover`; authorize A3; execute A3; `aien compose reconcile`; execute A3 | recover answers `mark_lost` = 1 and `mark_kept_as` = an existing `<compose dir>.cortex-mark.lost-<seq>` file byte-identical to the damaged-state mark; a host constraint record with `"repair":"cortex-mark"` and `"lost":1` exists; first execute A3 refused `ReconcileFailed` with no write; reconcile ok; second execute A3 DONE once |
| C8a mark adoption, old install | the case runs on a copy of F0v2 (no mark file): start, recall, shutdown, start, recall | first start log has exactly one `mark adopted (seq 1, records R)` line; a 128-byte `compose.cortex-mark` then exists; the home opens and recall is ok; the second start has no adoption or advance line; no `E_MARK` |
| C8b mark deleted on a v3 home | after the control steps (A1 DONE), stop, delete `compose.cortex-mark`, start, recall, execute A1 | the start log has exactly one `mark adopted` line; recall ok; the re-execute is refused `AlreadySpent` (ledger intact); no `E_MARK` |

C6c note: the v2 C6c row was FAIL; the injection is unchanged, only the expected outcome is new.

## 6. Verdicts (supersede v2 section 6)

- Row verdict: PASS, FAIL, NOT_RUN (could not be injected, or its control failed),
  NOT_APPLICABLE (only C6d, only through its guard), UNVERIFIED (a row whose reading would need a
  rule written after the run). None of NOT_RUN, NOT_APPLICABLE, UNVERIFIED is PASS.
- Campaign PASS = every row of v2 section 5 that this file does not replace, every row of
  section 5 here, and the C3 control, PASS; C6d PASS or NOT_APPLICABLE (named in the verdict and
  not counted as a pass); F0 valid.
- Declared NOT_PROVED (not rows): the five of v2, plus: tamper resistance of the record mark
  (journal and mark rewritten together, mark deleted then journal cut, compose dir and mark
  rolled back together); a real process kill between an append and its mark update (C6c-ctl
  reproduces the on-disk state only); J-Space spill damage while compose never spills (C6d).
- Receipt: v2 format, `acceptance_spec` = this file, plus `build_env` (the variables above) and
  `link_proof`; named by its sha256, never edited, indexed, `.summary.txt` beside it, and a
  `VERDICT-v3.md` written after the run.
