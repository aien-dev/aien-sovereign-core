# aien-sovereign-core

The Rust workspace for the AIEN host runtime on Linux (DGX Spark target): the `aien` command line, a single-process runtime daemon, the inference ABI and scheduler, and the supporting services around them. This file lists what exists and who owns what. It makes no claim that any part is complete or released; the README states the release state, and `docs/audits/` in aien-architecture holds the integration audit.

Ownership: this repository has no `CODEOWNERS` file, so per-component owners are not recorded here (UNVERIFIED who owns each crate). Cross-repo plans and decisions are owned by aien-architecture (`PLAN_AUTHORITY.md`, `CURRENT_EXECUTION_PLAN.md`, ADRs). Decisions made in this repo live in `docs/adr/`.

## Language

**Runtime daemon**:
The long-running process started by `aien start` (it runs as `aien --daemon`). It owns the model, the KV memory and the socket the command line talks to. Code: `crates/aien-runtime`, started from `crates/aien-cli/src/commands.rs`.
_Avoid_: server, service

**Approval desk**:
The operator's approval step for effects. It is required by default; a desk key created by `aien compose desk-key --create 1` must exist before the daemon starts. Code: `aien-runtime/src/spine.rs`, `approved_auth.rs`.
_Avoid_: login, auth

**Objective record**:
The operator's written objective, stored as `objectives/<id>.json` under the provenance folder, re-read by the daemon when a grant is bound to it. Code: `aien-runtime/src/objective.rs`, `aien-cli/src/objective.rs`.
_Avoid_: task, ticket

**Compose home**:
The state folder holding the desk key and composition state (`AIEN_COMPOSE_DIR`, else under `AIEN_RUNTIME_STATE_DIR`, else XDG state, else `~/.local/state/aien-runtime/compose`).

**Candidate**:
The release candidate this tree may be released as, named in `release/candidate.toml` with its model digests and source pins. `omega.lock` pins the omega commit that is linked. Changed only with aien-architecture's candidate manifest.

## Components (from crate descriptions)

Runtime and inference
- `aien-runtime`: single-process runtime spine (daemon, compose bridge, objective and desk logic).
- `aien-cli`: the `aien` command line and terminal agent harness.
- `aien-inference-abi`, `aien-abi-core`, `aien-inference-runtime`, `aien-inference-service`: inference ABI, backend trait, weights loader, service protocols.
- `aien-kv-cache`, `aien-scheduler`: paged KV cache and continuous-batching scheduler.
- `aien-omega-gpu`, `aien-omega-compose`: bindings to the omega GPU matmul engine and omega composition (pinned by `omega.lock`).
- `aien-platform`, `aien-platform-linux`: platform ABI and the Linux / GB10 driver cell.

Identity, memory, evidence
- `aien-allen`, `aien-allen-memory`, `aien-allen-profile`: ALLEN identity gate, scoped memory, persona profile.
- `cortex-rs`, `cortex-encoder-rs`, `cortex-path-m1`: Cortex memory engine, embeddings, a read-only retrieval experiment.
- `aien-capability`, `aien-mcp`, `aien-security`: tool and effect descriptors, MCP sessions, security abstractions.
- `aien-proof`, `aien-closure`, `aien-trace`, `aien-dual-observation`, `aien-replay`, `aien-campaign-verifier`, `crumbs`: test board, closure check, correlation trace, observation snapshot, transcript reader, campaign verifier, curriculum substrate.

Services and tools
- `spark-*` crates (cockpit, supervisor, debugger, harness, aegis, hive, dream, mail, discord hub, harvester, inquisitor, adapters, crumbs) and `rad-id-sync`, `aien-switchboard`, `aien-drive`: supporting services and tools; see each crate's Cargo description.
- `tools/aien-verify`: independent verifier of the whole-system end-to-end run folders; standard library only. Tests: `tools/aien-verify/tests/run.sh` (run in CI).

## Where things are

- Release machinery: `install.sh`, `scripts/check-release-candidate.sh`, `scripts/package-release.sh`, `docs/release/`.
- First-run steps for an operator: `docs/FIRST_RUN.md`.
- End-to-end and campaign material: `scripts/whole_system_e2e.sh`, `docs/campaigns/`.
