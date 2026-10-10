# Canonical Topology: Source of Truth and Composition Root

## Source of truth

`aien-sovereign-core` is the canonical source for every crate it contains:
runtime, scheduler, KV cache, inference ABI, Cortex, protocols, worlds, CLI.

Satellite repositories (`cortex-rs`, `spark-hive`, `spark-supervisor`,
`spark-dream`, `spark-debugger`, `spark-adapters`, `spark-crumbs`,
`spark-inquisitor`, `aien-harness`, `rad-id-sync`) are archived, read-only
snapshots on GitHub as of 2026-09-23. Each one's final commit ("docs: archival
pointer to canonical home") points back at its crate in this repository.
They are not parallel implementations and nothing mirrors into them; a
satellite that disagrees with the monorepo is stale by definition. The
monorepo wins.

`scripts/sync-standalone-repos.sh --check --all` is the drift check for anyone
who still holds the local `<name>-publish` checkouts: it diffs each crate's
`src/`, `tests/`, `schemas/` and `[package]` manifest fields against the
monorepo and exits non-zero on any difference. `AIEN_MIRROR_WORKSPACE` points
it at the directory holding those checkouts. The sync mode of the same script
is the mirror pipeline should the satellites ever be unarchived (that is a
GitHub-side decision for the project owner, together with relicensing them to
the workspace licence); until then there is no satellite automation to run.

## Canonical process model

Exactly one long lived composition root owns inference plus scheduler plus KV
plus Cortex plus AEGIS plus worlds plus the action executor plus operator
control: the `aien` daemon (`aien start`, `run_daemon_server`).

`aien-local-stack` composes the same pieces for integration proofs. It must
not grow a second, divergent process model. If it needs to serve traffic, it
does so by launching the canonical daemon path, not a parallel one.

## Checkout rules

No crate may assume a sibling checkout layout (`../sibling-repository`). All
cross crate references resolve through the workspace or published versions.
No absolute developer home paths in loaders, manifests, or tests. Model
locations resolve through `AIEN_MODEL_DIR`, `./models`, or `~/models`, in
that order, and every fallback is reported, never silent.
