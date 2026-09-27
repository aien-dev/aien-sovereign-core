# Crumb v1 Specification

**Status:** ACCEPTED: Crumb v1 Architecture; implementation qualification pending.
**Date:** 27 September 2026 (drafted); accepted with four amendments the same day.
**Purpose:** Define the permanent curriculum substrate for AIEN: the problem instances AIEN learns from, the sealed machinery behind them, and the evidence pipeline that turns searches into training data.

Nothing in this spec may be quoted as a result. A mechanism is a proposal until it is built, measured and ratified. Section 18 lists what v1 implements today and where; section 19 lists what is deliberately deferred.

## The Crumbline

The curriculum's public name is the **Crumbline**: the unbroken lineage of AIEN's training records. Every crumb AIEN solves and every trace it learns from is a record of something the machine actually did, verified against sealed held-outs.

The guarantee: **LLMs built the structure. They never touched the training data or the training process.** Language models built the laboratory: the generators, the harness, the curriculum machinery. No traditional LLM touches AIEN's training data or its training process.

That claim is encoded as a mechanically checkable chain, not doctrine. Every Crumbline record binds the generator instance digest, the sealed record digest, the verifier digest, the learner-visible digest, the learner build digest, the evaluation receipts, the trace stream digest and the previous Crumbline root (section 4.4).

---

## 1. What a crumb is

A crumb is one learning problem presented to AIEN as **observations only**: input/output examples with no title, explanation, difficulty or purpose. One crumb may be a toy invented a minute ago, another a bounded shadow of a deep open problem, another deliberately meaningless. To AIEN they are all observations.

A crumb is not a benchmark item, a test question or a fact to memorize. It is the substrate AIEN grows up inside.

## 2. The non-negotiable rule

**AIEN is never told why a crumb matters.**

No titles, no topic words, no equations, no explanations, no difficulty labels, no signal that one problem matters more than another. The task boundary comes from the runtime (produce the outputs), never from descriptive text.

This is an information-segregation rule enforced by process boundaries, types and tests (section 10), not a UI choice. It is what makes later rediscovery checkable.

## 3. The old crumb standard

`crumb-spec` and `spark-crumbs` mean something else: filesystem agent scent trails for coordinating LLM coding agents. The curriculum takes zero dependency on them. The two systems share no code, schemas or identity spaces.

## 4. The Crumb v1 contract

### 4.1 Visible side (AIEN sees this)

An ordered list of examples, each `INPUT_BYTES -> OUTPUT_BYTES`, plus the minimum structure needed to parse them. Canonical bytes (`crumbs::visible`):

```text
"CRB1" | schema_version u16 (=1) | encoding u8 | flags u8
in_arity u8 | out_arity u8 | in_lane_bytes u8 | out_lane_bytes u8
n u32 | n x (u32 len, input bytes, u32 len, output bytes)
[optional budget: max_candidates, max_depth, max_oracle_queries, max_program_ops, 4 x u32]
```

Encoding 1 is canonical decimal UTF-8 lanes separated by one space (v1 default). Encoding 2 is raw little-endian lanes (reserved move to raw bytes). Decoding is strict: bytes decode only if re-encoding reproduces them exactly.

### 4.2 Sealed side (AIEN never sees this)

`crumbs::sealed::SealedCrumb`: crumb digest, family id and key, mechanism, generator version, generator code digest, seed, held-out seed, canonical generation parameters, generator instance digest, verification method, difficulty, capability rung, required and provided capabilities (opaque BLAKE3 ids), provenance and contamination class, source family, truth status (verified, conjectural, none), decoy status (clean, rabbit hole with class A to G, no structure), known-solution status, rival explanations, noisy visible indices, expected domain, internal topic, and three held-out tiers (interpolation, extrapolation, adversarial). Its `Debug` output is redacted.

Occurrence-level facts (population tag, draw reason, time) live in the ledger, not in the sealed record, so the sealed record is a pure function of its generator inputs.

### 4.3 Identity (amendment 4)

```text
CrumbOccurrenceId        UUID v7: this presentation in this run
CrumbDigest              BLAKE3(canonical learner-visible Crumb v1 bytes)   the observable problem
GeneratorInstanceDigest  BLAKE3(family_id || generator_version || seed || canonical parameters)
SealedRecordDigest       BLAKE3(canonical sealed record); never enters learner space
```

Every digest uses its own BLAKE3 `derive_key` context. Two independently generated crumbs that expose the same phenomenon get the same CrumbDigest, and the system can see that without revealing why.

### 4.4 The Crumbline ledger

`crumbs::ledger`: each record binds seq, lane, occurrence, condition, population, CrumbDigest, GeneratorInstanceDigest, SealedRecordDigest, verifier digest, learner digest, evaluation digests, trace stream digest, promotion digests and the previous root. `Ledger::verify` recomputes the chain from genesis. Lanes (main, clean room) are separate chains.

## 5. Generator families (amendment 1)

The v1 target is **100 registered family ids** over **at least 24 genuinely distinct mechanisms** covering every rung. Families are parameterizations of reusable mechanisms and combinators, not copies. Acceptance is measured acquisition and reuse, not a file count.

Rungs (builder vocabulary, never sent to AIEN): single transformation, two transformations, repeated transformation, conditional transformation, multi-input relation, recurrence, state, composition, symmetry, modularity, invariance, multi-step algorithms, latent variables, noisy observation, partial observation, counterexample search, theory formation.

Interface: `gen::generate(family, seed) -> (VisibleCrumb, SealedCrumb)`, deterministic in (family id, generator version, seed, parameters). Held-out inputs are disjoint from visible inputs and from each other. A golden digest over every family at fixed seeds pins behaviour; changing a generator without bumping `GENERATOR_VERSION` fails the test suite.

## 6. Composition: the breadcrumb trail

Every operation AIEN discovers and verifies can become a building block for later crumbs. Capability families teach hidden capabilities; composition families quietly require two or three of them. The Omega Library is AIEN's accumulated collection of verified transformations, each with a provenance chain back to the crumbs that demanded it.

## 7. Adversarial generators

Rabbit holes deceive; they never lie. No wrong answer is ever labeled correct. Classes: A simple wrong rule, B several programs fit, C spurious dimension, D prefix imitates then diverges, E arbitrary table with an elegant prefix, F insufficient evidence, G noisy observations; plus no-structure crumbs. **Pattern observed is not law established.** The learner may, and should, retain uncertainty.

## 8. Secret populations and the mixer

Initial schedule, loaded from `config/mixer.v1.json`, not hard-coded: 55 fundamental, 20 composition, 10 ambiguous, 10 rabbit hole, 5 frontier. The population tag is sealed. Consecutive crumbs never share a family or a capability. The mix adapts to evidence: as capabilities are demonstrated, weight moves from acquisition to composition. A population with nothing eligible (v1 has no frontier sources) is redistributed and the redistribution is recorded.

## 9. Frontier feeders

For unresolved mathematics: open structure, finite instance, exact computation or proof, byte examples. The conjecture is never shown, and a relationship AIEN extrapolates beyond the verified region enters a conjecture pipeline, never learned truth. **Deferred past v1** (section 19).

## 10. Verification protocol and the information boundary

The learner is a separate operating-system process with an empty environment. The only channel is the learner protocol (`crumbs::protocol`), whose frames carry exactly: HELLO (search limits), CRUMB (visible bytes), VERDICT (one byte), ADMIT (an operation, its reference and scope), SHUTDOWN; and from the learner READY, BANK, SUBMIT, EVENTS, DONE. No frame can carry a sealed field. Any unknown frame aborts the session with no reply. The oracle answers at most `max_oracle_queries` submissions per crumb; after that it answers "budget exhausted" without evaluating.

The verifier contract (amendment 3) is generic: candidates arrive as learner-neutral **CPG1** programs (`crumbs::program`), and the sealed verifier executes them with its own interpreter against visible examples and all three held-out tiers. A candidate that interpolates the visible samples but fails a held-out is `FalsifiedHidden`: recorded as a useful failure, never discarded.

## 11. Clean-room lane (amendment 2)

Built in v1: contamination classes (SyntheticNovel, AdversarialSynthetic, HumanTheoryDerived, FrontierDerived, RealWorldMeasured), human-constant and human-ontology flags on every family, the forbidden-source policy `provenance::clean_room_check`, a clean-room ledger partition that refuses forbidden records, and a clean-room mixer mode. Actual Physics Zero worlds are **not** built in v1.

## 12. Trace capture

The learner reports bare search events (operation applied, node ids, prune reason, verification outcome). The sealed side reconstructs every candidate program from those events and the declared operation bank and derives state digests, remaining-mismatch vectors, improvement flags, result classes (pruned, failed, partial, improved, verified, rejected by held-outs, rejected robust guess) and contribution to the verified solution. Held-out results enter a record only on SUBMIT events, as the verdict the learner received. Records (`CTR1`, 247 bytes) and training samples (`CTS1`, 268 bytes, one per decision state on a verified path, equivalence-aware positives) are fixed-size, numbers only and BLAKE3 hash-chained. No record can hold reasoning text.

## 13. Evidence-based promotion

```text
candidate operation     sub-chain of an accepted solution
held-out verified       part of a solution the sealed verifier accepted
corroborated            supports >= 2 distinct crumbs
survived adversarial    every support passed a non-empty adversarial tier; exposures bounded
generalized             every support passed a non-empty extrapolation tier
admitted                sent to the learner's library with an explicit scope
```

Scope is recorded, not assumed: `scope_bits` is the widest input magnitude over which a supporting solution was verified, and the learner refuses to offer an operation on crumbs outside it.

## 14. Endgame (out of v1 scope)

Self-generated curricula: AIEN constructs tests of what it believes, Omega realizes them, the verifier evaluates, Cortex records evidence. v1 must not preclude this and does not attempt it.

## 15. Milestone 1 scope

In scope: the contract (4), 100 registered families over >= 24 mechanisms (5), adversarial families (7), the mixer (8), the Omega adapter with architectural sealed-side blindness and the held-out verifier (10), trace capture (12), the promotion ladder (13), the clean-room boundary (11).

Out of scope: frontier feeders (9), actual clean-room physics worlds (11), self-generated curriculum (14).

**Acceptance:** measurable acquisition and reuse of progressively deeper operations, with the reuse recorded in provenance chains. If composition crumbs are solved without reusing earlier discoveries, the trail is not a trail.

## 16. Ownership (amendment 3)

- `crumbs` (this crate, aien-sovereign-core `crates/crumbs`): contracts, identities, generators, mixer, verifier contract, protocol, trace schema, promotion evidence, ledger. It knows nothing of Omega internals and depends only on BLAKE3, UUID and serde.
- `omega` (aien-dev/omega): the consumer adapter. `build/crumbline-learner` reads Crumb v1 bytes, runs Omega synthesis, submits CPG1 candidates, stores admitted operations in the OmegaLibrary.
- `crumb-spec`, `spark-crumbs`: untouched.

## 17. Open questions resolved at acceptance

1. 100 registered family ids is the target; >= 24 distinct mechanisms is the floor; acceptance is acquisition and reuse.
2. The clean-room boundary is built now; physics worlds wait.
3. The Omega adapter lives in Omega; the protocol and generic verifier contract live in `crumbs`.

## 18. Implementation map (v1)

| Mechanism | Code | Test or gate |
|---|---|---|
| Visible contract, strict decode, CrumbDigest | `src/visible.rs`, `src/digest.rs` | `tests/boundary.rs` crumb_v1_format_pass |
| Sealed record, redacted Debug, digests | `src/sealed.rs` | boundary: sealed_boundary_pass_* |
| 122 registered families, 38 mechanisms, capability pool | `src/gen/` | `tests/generators.rs` |
| Determinism and golden pin | `src/gen/mod.rs` | generators crumb_determinism_pass, boundary crumb_determinism_golden_pass |
| Verifier contract, CPG1 | `src/verify.rs`, `src/program.rs` | generators, gates CRUMB_HIDDEN_VERIFY_PASS |
| Process boundary, protocol, oracle budget | `src/protocol.rs`, `src/bin/crumbs-probe-learner.rs` | boundary no_hidden_access_* , gates CRUMB_NO_HIDDEN_ACCESS_PASS |
| Trace corpus and samples | `src/trace.rs` | curriculum crumb_trace_and_sample_pass, gates CRUMB_TRACE_PASS / CRUMB_TRAINING_SAMPLE_PASS / NO_COT_STORAGE_PASS |
| Promotion ladder and scope | `src/promotion.rs` | curriculum crumb_operation_promotion_pass, gates CRUMB_OPERATION_PROMOTION_PASS / CRUMB_LIBRARY_REUSE_PASS / CRUMB_SCOPE_ENFORCEMENT_PASS |
| Mixer | `src/mixer.rs`, `config/mixer.v1.json` | curriculum crumb_curriculum_pass / crumb_no_category_leak_pass |
| Clean room | `src/provenance.rs`, `src/ledger.rs` | curriculum clean_room_provenance_pass |
| Ledger | `src/ledger.rs`, `src/session.rs` | curriculum ledger_chain_detects_tampering |
| Acquisition and reuse experiment | `src/experiment.rs` | `crumbs experiment --learner PATH [--replicates K]` |

## 19. Deferred (not implemented in v1)

- Frontier feeders and the conjecture pipeline (section 9).
- Clean-room physics worlds and the hidden-law world interface.
- Cortex ingress of candidate relations and evidence. Cortex (`crates/cortex-rs`) already has the candidate, evidence and promotion model this should use; v1 writes promotion records and evaluation receipts to files only.
- AIEN_0 next-action guidance: the trace corpus is its training input; no trained policy exists yet.
- Multi-lane CPG1 programs. v1 candidates map lane 0 to lane 0, so multi-input families are valid crumbs that the Omega adapter reports as unsupported.
