# Canonical Topology: Source of Truth and Composition Root

## Source of truth

`aien-sovereign-core` is the canonical source for every crate it contains:
runtime, scheduler, KV cache, inference ABI, Cortex, protocols, worlds, CLI.

Satellite repositories (`cortex-rs`, `spark-hive`, `spark-supervisor`,
`spark-debugger`, `spark-adapters`, `spark-crumbs`, `spark-inquisitor`,
`rad-id-sync`) are published mirrors, not parallel implementations. Changes
land in the monorepo first, then mirror out by automation. Manual
synchronization between the monorepo and a satellite is a bug, not a workflow.

Until the mirror pipeline exists, any satellite that disagrees with the
monorepo is stale by definition. The monorepo wins.

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
