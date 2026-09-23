# PEARL Cortex path context, Milestone 1

Status: EXPERIMENTAL. NOT ARCHITECTURAL COMMITMENT.

This note freezes one retrieval comparison. It does not add `RelationalPath`, `ContextGraph`, or `PathScore` to the AIEN architecture. A later ADR would be a separate decision after a scientific pass.

## Hypothesis

A bounded deterministic score over existing Cortex claims, added to today's `recall_entities` score, ranks relevant entities higher than `recall_entities` alone.

## Arms

Both arms receive the fixture's query embedding. Neither arm calls the live encoder.

Control: `recall_entities(query, embedding, limit=10)`. The stored `graph` component stays 0. The ranked ids are that call's order.

Treatment: the same baseline score for every non-retracted entity in the case space, plus `0.25 * graph_score`. Order is final score descending, entity id ascending. Take 10.

`graph_score` is 0 for an entity the walk does not reach. An entity can enter the treatment top 10 on baseline score alone.

## Graph

A claim is an undirected edge only when all of these hold:

- `status` is active
- `retracted` is false
- `subject_entity_id` and `object_entity_id` are both non-empty and different
- the claim and both endpoint entities are in the case's `space_slug`

Literal-only, superseded, disputed, invalidated, retracted, and cross-space claims are not edges.

Edge weight is `confidence * tier_weight * evidence_factor * freshness`.

- tier weights: T0 0.4, T1 0.6, T2 0.8, T3 1.0
- `evidence_factor(n) = (1 + n) / (2 + n)`
- freshness is `clamp(0.5 ^ (age_days / 180), 0.25, 1)` against the snapshot timestamp passed into the harness

These fields change weight. They do not decide admission. Predicate text is not a feature.

## Walk

- anchors: top 8 entities by baseline score, ties broken by entity id ascending
- simple undirected paths, maximum 2 hops
- at most 16 neighbors per node, ordered by edge weight descending, then claim id, then neighbor id
- at most 64 scored paths
- at most 32 entities may hold a nonzero graph score
- parallel claims stay as parallel edges
- full entity scan cap: 1,000,000

Path score is the product of edge weights times `0.5 ^ (hops - 1)`.

`graph_score(entity)` is the maximum of `baseline(anchor) * path_score` over scored paths from another anchor. There is no zero-length path.

## Suggestions

If a scored path of two hops connects two anchors and no admitted claim directly joins them, the harness appends a suggestion to the result artifact:

- predicate `path_context.related`
- subject and object ordered by entity id
- score equal to that path contribution

The suggestion is not inserted into Cortex and is not an edge in the same run.

## Fixture

`experiments/fixtures/pearl_cortex_m1_entities.example.json` is an illustration with two cases. It cannot produce a pass or a fail.

A scientific fixture has at least 50 cases and at least 15 relational cases. A case is relational when one relevant entity's `canonical_name` does not contain the query string. Each case has `id`, `spaceSlug`, `query`, `relevantEntityIds`, and a finite `queryEmbedding`. Relevance is binary. Every relevant id must exist in the snapshot. Missing ids, short fixtures, and duplicate ids refuse a verdict.

Editing the fixture or the config changes its hash and invalidates older result files.

## Metrics

Binary nDCG@10 uses discount `1 / log2(rank + 1)` with rank starting at 1. Recall@10 is the fraction of relevant entities in the top 10.

Relative nDCG is `(treatment - baseline) / baseline`. Queries whose baseline nDCG is 0 are omitted from that mean and listed with baseline nDCG, treatment nDCG, and absolute delta. The report also includes the mean absolute nDCG delta over every query.

The paired bootstrap draws 10,000 resamples of the relative values with seed `0x504541524C`. The 95% interval is the 2.5 and 97.5 percentiles. It excludes 0 when the lower bound is positive or the upper bound is negative.

Accounted bytes charge both arms `entities * 256 + 10 * 128`. The treatment also charges `admitted_claims * 96 + paths_scored * 48`. This is a deterministic working-set count, not process RSS. Latency is in-process. Encoder time is outside both clocks. Baseline time is `recall_entities` at limit 10. Treatment time is the full scan, the walk, and the fusion.

## Pass

All of these are required:

1. Mean relative nDCG@10 is at least +5%.
2. The paired-bootstrap 95% interval excludes 0.
3. Mean Recall@10 delta is at least -0.01.
4. Treatment p95 latency is at most 1.5 times baseline p95.
5. Peak accounted bytes per query are at most 2 times the baseline peak.
6. `writes` is 0. The harness compares `memory_candidates`, `promotion_receipts`, and `claims` counts on the snapshot copy, and it refuses to run unless the source file has no `-wal` or `-shm` sibling. The source file hash is checked again after the run.

Strong pass, recorded only when the binding pass is also true: relative nDCG@10 at least +10%, Recall@10 delta greater than 0, and treatment p95 at most 1.25 times baseline. A strong pass does not promote this design into the architecture.

## Runner

`cortex-path-m1 --snapshot PATH --fixture PATH --out RESULT.json --snapshot-time RFC3339`

The process opens a copy of the snapshot. Production recall and its HTTP handler are unchanged. Exit 0 writes a report. Exit 2 refuses a scientific verdict. Exit 1 is an operational error.

Result fields: `dataset_snapshot_sha256`, `eval_fixture_sha256`, `experiment_binary_sha256`, `config_sha256`, per-query rows, `relative_mean_ndcg`, `absolute_mean_ndcg_delta`, `zero_baseline`, `recall_at_10_delta`, latency, accounted bytes, suggestions, `writes`, `pass`, `strong_pass`, `fail`.
