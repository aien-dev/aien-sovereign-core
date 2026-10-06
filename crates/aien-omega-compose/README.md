# aien-omega-compose

Rust binding to omega COMPOSITION-2 through the rxc_host C ABI
(`src/runtime/rxc_host_abi.h`, archive `build/librx_compose.a`, make target
in omega `mk/rx_compose_lib.mk`). NEXT-PHASE-1 cut 1b.

## Building against omega

The library is built from an omega checkout whose HEAD must equal the
expected commit:

| Variable | Meaning |
|----------|---------|
| `AIEN_OMEGA_COMPOSE_DIR` | omega checkout; `make <OUT_DIR>/librx_compose.a` runs in it |
| `AIEN_OMEGA_COMPOSE_SHA` | full 40-hex sha the checkout must be at. **Overrides `omega.lock`.** Pre-merge only: `omega.lock` pins the GPU candidate (CAND-2) and is not edited for this crate. |
| `AIEN_PHYSICS_DIR` | physics checkout at omega's `physics.lock` (default `<omega>/../physics`) |
| `AIEN_AIENOS_LOCK_REPO` | aienos clone holding omega's `aienos.lock` commit (passed as `AIENOS_LOCK_REPO`) |
| `AIEN_OMEGA_COMPOSE_LIB` | link a prebuilt `librx_compose.a` instead (not sha-checked) |
| `AIEN_FORCE_CPU_STUB=1` | stub: every call returns `ComposeError::Unavailable` |

With none of these set the crate builds the stub and prints a warning.
`AIEN_OMEGA_COMPOSE_DIR` is separate from aien-omega-gpu's `AIEN_OMEGA_DIR`, so
the GPU crate's `omega.lock` check is never bypassed by this override.

Until omega `librx_compose.a` is merged and `omega.lock` pins a commit that
has it, build with the branch commit of omega PR "runtime: librx_compose.a +
host ABI facade (NEXT-PHASE-1 cut 1a)":

```text
AIEN_OMEGA_COMPOSE_DIR=<omega checkout at that commit>
AIEN_OMEGA_COMPOSE_SHA=<that commit, 40 hex>
AIEN_PHYSICS_DIR=<physics checkout at omega physics.lock>
AIEN_AIENOS_LOCK_REPO=<aienos clone>
cargo test -p aien-omega-compose
```

The integration test (`tests/compose.rs`) is ignored in a stub build.

## Semantics (from the C header)

- `open` binds the directory to one AienMachineId (`machine.id`); another
  machine is refused. The Cortex journal is `<dir>/cortex.cx`.
- `register_skill` before the first `run`/`recall`/`info`. At most 2 Skills.
  Callbacks run on omega World worker threads.
- `run(task, now_us)`: the Skills compete on staged J-Space branches, AEGIS
  checks each result with the `set_verify` callback, the winner commits.
  The result carries the Cortex record ids, branch count and verdict.
- A reopen appends records; pre-restart records stay an unchanged prefix.
- Torn journal tail: `refuse_torn = true` refuses without touching the file;
  otherwise it is repaired when it lies past the last commit, and refused
  explicitly (stably) when it lies inside the committed record.
