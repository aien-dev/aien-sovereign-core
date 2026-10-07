# AIEN Artifact Licensing Matrix

This document defines the licensing structure governing software, specifications, benchmark data, and model artifacts across the AIEN ecosystem.

AIEN software is licensed independently from AIEN model artifacts. Possession, compilation, execution, or deployment of AIEN software creates zero claims, obligations, or covenants concerning artificial intelligence models trained with independent datasets, weights, services, or infrastructure, unless a specific model or dataset expressly carries separate terms.

---

## Artifact Classification Matrix

| Layer | Artifact Description | Canonical License | Identifier |
| :--- | :--- | :--- | :--- |
| **Core Software Runtime** | Rust crates (`aien-scheduler`, `aien-kv-cache`, `cortex-rs`, `spark-rsi`, `aien-harness`, CLI, web gateways) | GNU Affero General Public License v3.0 or later | `AGPL-3.0-or-later` |
| **Compiler & Dynamic C-ABI** | Shared library bindings (`spark-max-cabi`, `libspark_max.so`) | GNU Affero General Public License v3.0 or later | `AGPL-3.0-or-later` |
| **Protocols & Specifications** | Inference ABI specifications, Crumb protocol definitions, schemas, and formats | GNU Affero General Public License v3.0 or later | `AGPL-3.0-or-later` |
| **Benchmark Harness Code** | Measurement driver, CLI, test generators, and harness binaries in `benchmarks/` | GNU Affero General Public License v3.0 or later | `AGPL-3.0-or-later` |
| **Benchmark Datasets & Metrics** | Raw telemetry JSON, hardware timing records, power logs, and rendered SVG comparison charts | Creative Commons Zero 1.0 Universal | `CC0-1.0` |
| **AI Foundation Model Weights** | Neural network checkpoints, tensors, and tokenizer parameters released by AIEN | Specific Model License | Refer to accompanying `MODEL_LICENSE` |
| **Training Code & Distillation** | Model training recipes, data processing pipelines, and distillation scripts | GNU Affero General Public License v3.0 or later | `AGPL-3.0-or-later` |
| **Training Datasets** | Curated datasets, imprints, or knowledge corpora | Dedicated Data License | Refer to accompanying `DATA_LICENSE` |
| **Constitutional Governance** | Mission, principles, engineering standards, and developer oaths | Upstream Project Charter | Non-binding on downstream software users |
| **Voluntary Reciprocity** | Shared research freedoms, open weight releases, and reciprocal distillation program | Bilateral Research Covenant | Refer to `OPEN_COOPERATION_COVENANT.md` |

---

## Model Release Terminology

To maintain technical and legal precision:

1. **Open Source AI**: This designation is reserved exclusively for artificial intelligence releases that fully satisfy the Open Source AI Definition (OSAID 1.0) established by the Open Source Initiative (OSI), providing model parameters, training and inference code, and comprehensive training data documentation.
2. **Open Weights**: Artificial intelligence models published with public parameter weights, inference code, and architecture specifications, but which do not bundle the exhaustive training corpus necessary to satisfy OSAID 1.0, are precisely designated as Open Weights.

---

## Downstream Implementation Autonomy

AIEN software is licensed under the GNU Affero General Public License v3.0 or later (`LICENSE`, `NOTICE`). Its terms, not this document, decide what a downstream user may do: they cover copying, modifying, linking, distributing and offering the software over a network. Model weights are separate works: a third-party model keeps its publisher's license (for example SmolLM2-1.7B-Instruct, Apache-2.0, see `NOTICE` and `ATTRIBUTION.md`), and running a model on AIEN does not relicense it.
