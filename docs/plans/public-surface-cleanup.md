# Public Surface Cleanup: Master Plan

Status: draft for analysis. No implementation in this document.
Decision set: Q1 through Q14, frozen with the operator. One wording refinement
on Q8/Q14 applies: withdrawn figures return when they meet the **applicable**
C1-grade provenance requirements, not every requirement of the C1
branch-correctness campaign.

## Meta-rule

This project does not need to appear smaller. It needs to distinguish clearly
between what exists, what has been measured, what is under active development,
and what is an architectural ambition.

## Target public architecture

Product identity: **AIEN Neural Runtime**, described as **a sovereign agent
and inference runtime**. No public implication that AIEN replaces Linux or
macOS or is literally an operating system. Research documents may state that
AIEN explores operating-system abstractions for persistent agent state,
inference scheduling, physical context ownership, and execution authority.

Three separated layers:

1. README: engineering. What it is, what works, what hardware, how to run it,
   what has been measured, what does not work yet.
2. Research and docs: architecture and research philosophy.
3. CONSTITUTION and manifesto: values and long-term ambition.

README rule: describe the smallest claim supported by current evidence.
Research docs may describe the larger hypothesis. The manifesto may describe
the larger ambition.

## Decisions Q1-Q14 (frozen)

- Q1/Q7 license: code under plain Apache-2.0. Model weights and generated
  artifacts carry separate licenses where necessary. Anti-enclosure becomes a
  clearly nonbinding `COVENANT.md` with the public ask "Keep foundational
  advances open". Remove language implying the covenant modifies Apache
  rights, retroactively changes historical licenses, overrides third-party
  ToS, or establishes automatic statutory damages. No Git history rewrite.
- Q2 fallback: replace the `zero fallback` interpretation in README and
  `docs/AIEN_RUNTIME_ARCHITECTURE.md` with: "The recorded fallback counter
  was zero for the GPU operations instrumented by this benchmark. The current
  counter does not observe all CPU-executed operations; C1 is adding
  per-operation device provenance." Verified in code:
  `crates/aien-inference-abi/src/blackwell_backend.rs` increments the counter
  only on matmul, batch matmul, BF16 paged attention, and logits paths, while
  `rmsnorm`, `apply_rope`, `swiglu`, and `gqa_attention` run on CPU with no
  increment. First action, before any benchmark is amplified further.
- Q3 naming: per target architecture above.
- Q4/Q8 benchmarks: formal publication rule. Every headline performance
  number must resolve to a reproducible command and evidence artifact in
  `benchmarks/`. Primitives are reported as primitives with artifact links,
  never translated into system-level conclusions. Retire the "Verified
  Performance Benchmarks" heading in favor of "Measured Results" or
  "Reproducible Benchmarks". Comparison rows must state exact
  software/version, hardware, commit, command, workload, concurrency,
  measurement boundary, and artifact.
- Q5 installer: keep the one-line installer as a labeled convenience path,
  not the high-assurance mechanism. Track signed releases, immutable
  versions, checksums/signatures, and pinned manifests as follow-up work.
- Q6 voice: engineering terms in code-facing text. "Nesting Ritual" becomes
  hardware/runtime initialization or initiation sequence. "cognitive soul"
  becomes seat profile or agent profile. "Sovereign Coordination" becomes
  Contact. Landing order: identity, implemented scope, hardware, diagram,
  install, run example, tests, measurements, limitations, deeper links.
- Q9/Q11 repos: every public repo must have exactly one authoritative
  responsibility that cannot be served better as a directory or crate in
  another repo. Canonical set: `aien-sovereign-core`, `aien-protocols`
  (contingent on real versioning below), `benchmarks` (immutable evidence),
  `aegis-runtime` (confirmed independent: own tags, release pipeline,
  `:18096` deploy target, distinct codebase). Archive the rest with pointer
  READMEs. Archive preserves links and provenance; nothing is deleted.
- Q10 tiers and limitations: per sections below.
- Q12 rename: core `aien-inference-protocol` becomes `aien-inference-service`
  (request/event/service execution interface). The protocols-repo crate keeps
  the `aien-inference-protocol` name (shared cross-repo state, context,
  branch, lease types).
- Q13: this document is the single canonical plan. Execution is sequenced
  PRs, one purpose per PR.
- Q14: benchmark engineering proceeds independently. Republication gates on
  the applicable C1-grade provenance requirements. C1 owns the evidence
  standard, not generation.

## Repo dispositions

Keep: `aien-sovereign-core`, `aien-protocols` (after versioning),
`benchmarks`, `aegis-runtime`, the two sites, org profile.
Versioning requirements for `aien-protocols` before it may be called stable:
semantic version tags from `v0.1.0`, `CHANGELOG.md`, compatibility policy,
explicit deprecation policy, each published consumer records the protocol
version it targets. Until then it is described as an independently consumed
spec repo, not a stable versioned standard. Known path consumers today:
`aegis-runtime`, `aien-local-stack`, `spark-inquisitor`, `spark-rsi`.
Archive with pointer README (canonical replacement named, no longer
authoritative stated): `spark-inquisitor`, `harvester`, `spark-adapters`,
`spark-debugger`, `spark-supervisor`, `spark-hive`, `aien-harness`,
`spark-dream`, `cortex-rs`, `rad-id-sync`, `spark-crumbs`, `crumb-spec`
(spec content folds into protocols or core docs), `aien-local-stack`
(composition example folds into core). Remove the standalone `spark-aegis/`
export copy (non-git duplicate of the core crate); the core crate stays.

## Rename map

- `aien-sovereign-core/crates/aien-inference-protocol` to
  `aien-inference-service`. Update dependents (known: `aien-inference-runtime`
  and any path consumers inside core).
- No change to the `aien-protocols` crate of the same name.

## License migration

1. `LICENSE`: remove Sections 10 through 17 (the SRCL rider). Plain
   Apache-2.0 governs from the migration commit forward. History is not
   rewritten; prior commits remain historical facts.
2. All 31 workspace crates: `license.workspace` from `SRCL-1.0` to
   `Apache-2.0`.
3. New `COVENANT.md`: nonbinding values statement with explicit language
   that it grants and restricts no legal rights.
4. Update references: `AGENT_CODE_OF_CONDUCT.md`, README license section
   (including removal of the "essentially Apache" characterization and the
   Sections 11-13 covenant summaries), badges.
5. `CONTRIBUTING.md` already states Apache-2.0 for code and CC0-1.0 for
   benchmark data; reconcile any remaining SRCL mentions with the migration.
6. Legal review of the final shape is tracked work, not AI-generated closure.

## README and content rules

Landing order: identity plus one sentence, implemented scope, hardware,
diagram, install, run example, tests, measurements, limitations, deeper
links. Cosmology, mission narrative, and partnership framing move below the
engineering front door into linked research and values documents.
Terminology substitutions per Q6. Subsystem list headlines Tier 1 only:
Cortex, CLI and runtime composition, KV and COW manager, SequenceArena and
scheduler, inference ABI, Qwen FP8 MoE work, C1 provenance infrastructure,
each with evidence links. Tier 2 (AEGIS boundary, MCP integration,
supervisor, debugger, cockpit, distillation pipeline, Harvester) may appear
only in an explicitly labeled experimental or development section with
reduced visual status. Tier 3 (RSI, Dream, Hive higher-level abstractions,
self-improvement claims, Open Humanity concepts) stays out of the README.
Platform section follows `docs/PLATFORM_MATRIX.md`: GB10 primary reference,
Apple Silicon and x86_64 per measured status, AMD ROCm marked experimental
and architected (runtime qualification pending).

## Known Limitations (ships in the cleanup)

Minimum contents: fallback-counter scope and hybrid CPU/GPU execution;
platform targets not yet qualified; license transition in flight; installer
not yet signature-verifying; withdrawn benchmark figures and their
regeneration status; Qwen full-runtime qualification incomplete; C1 evidence
campaign in progress.

## Benchmark withdrawal and republication rules

Withdrawn immediately if provenance does not meet the bar: fork 2.06 us, COW
mutation 13.30 us, the 500.0x ratio and associated memory figures, and the
BENCH comparison table including the FastAPI baseline. Findings on record:
the cited `benchmarks/data/benchmarks_latest.json` does not exist in the
repo, the `gb10_canonical` artifact directory exists only in the external
benchmarks repo, the microbenchmark literals appear only in README text, and
no in-repo command produces the FastAPI baseline. Replacement content gives
the exact regeneration commands (`bench_inference_stack`,
`cow_branching_tests`, `bench_apples_to_apples` with parameters) and states
that figures are being regenerated under the evidence standard.
Applicable C1-grade provenance for any republished figure: commit identity,
hardware and environment record, exact command, raw samples, SHA-256
digests, measurement definition, and reproducibility steps. A primitive
allocator benchmark does not need branch-equivalence testing; it does need
every item in the previous sentence.

## PR sequence and acceptance criteria

1. Fallback wording correction. Accept: both files carry the frozen sentence,
   no remaining unqualified `fallback_count == 0` claim in public text.
2. Benchmark withdrawal plus regeneration instructions. Accept: listed figures
   gone from README, commands present, no new figures without artifacts.
3. Crate rename with dependent updates. Accept: workspace builds, tests pass,
   no stale crate name in manifests or imports.
4. License transition plus `COVENANT.md`. Accept: rider gone, fields flipped,
   references reconciled, preflight clean.
5. README and voice restructuring plus Known Limitations. Accept: landing
   order, tier gating, terminology substitutions, limitations present.
6. `aien-protocols` versioning. Accept: tags, changelog, compat and
   deprecation policies, consumer version pins recorded.
7. Repo archival with pointer READMEs. Accept: each archived repo points at
   its canonical home and states non-authoritative status.
8. Installer trust-path follow-up. Accept: convenience labeling in place,
   signing work tracked with acceptance defined.
9. Republication of regenerated figures. Accept per figure: artifact bundle
   satisfies the applicable provenance list and relevant verifier checks.

General gate per PR: branch discipline, `bash scripts/agent-preflight.sh`
exit 0, evidence in the PR description, no unrelated scope.
