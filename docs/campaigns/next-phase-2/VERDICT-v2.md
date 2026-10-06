# NEXT-PHASE-2 (ACCEPTANCE-v2): campaign verdict

```text
receipt  = dc0c88a84f410ca5547b20626971b9b93eed03ccae511fe62029efc5eaad4206.json (never edited)
VERDICT  = FAIL
C6c      = FAIL
C6d      = NOT_RUN
C2a      = UNVERIFIED for the frozen rule (receipt row reads PASS)
```

This note sits beside the receipt. The receipt is not edited. Where this note and the receipt
disagree, this note is the campaign verdict.

## 1. C6c FAIL

As ACCEPTANCE-v2 item A8 predicted: a cut of the memory file exactly at the record boundary that
drops only the last grant is not detected on load. The lost grant is then refused `NotAuthorized`,
so nothing is written, but the loss itself is silent. Evidence: receipt `.summary.txt` note line.

## 2. C6d NOT_RUN

The fixture's `jspace.data` is 0 bytes, so "trim 1 byte" changes nothing; the harness detects the
no-op and records NOT_RUN. A fixture with non-empty J-Space data is needed.

## 3. C2a reclassified UNVERIFIED

The frozen rule (ACCEPTANCE-v2.md line 137, row C2a) requires "record count unchanged". The run
measured, in each of the three C2a runs (receipt check `record_count_unchanged_except_open_anchors`):

| before kill | after restart | after a clean restart |
|-------------|---------------|-----------------------|
| 23          | 28            | 33                    |

The count rose by 5 on every daemon start. The row was scored PASS by comparing against the growth
of a clean restart (host notes unchanged, `host_record_count_unchanged` = [3,3]). That reading was
written after the run; post-run reinterpretation is not allowed. The identity of the 5 records each
daemon start appends is UNVERIFIED (they are not host notes). C2a is therefore UNVERIFIED for the
rule as frozen. The next acceptance version must freeze the rule (and identify those 5 records)
before any rerun.

## 4. Known gap carried into the next cut

If the start-up reconcile panics, the daemon prints `Reconcile: failed: ...` and keeps serving
(`crates/aien-runtime/src/server.rs` lines 182-186: `spawn_blocking(reconcile_at_start)` result
mapped by `unwrap_or_else`). Whether effect commands still refuse in that state is untested. Next
cut: add a case that forces the reconcile to fail and checks that every effect command refuses
until a successful reconcile.

## 5. Next cut (case list)

- C6c: record high-water mark kept outside the memory file; C6c must flip to PASS, plus a control
  where a crash between append and mark update is not reported as corruption.
- C6d: non-empty `jspace.data` fixture produced through the real code path.
- C2a: rule frozen with the 5 start-up records identified, then rerun.
- Reconcile-failed state: effect commands refuse (section 4).
