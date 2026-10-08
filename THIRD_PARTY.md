# Third-Party Model Notices

AIEN code is AGPL-3.0-or-later (`LICENSE`, `NOTICE`). The models below are separate
third-party works. Each keeps its publisher's licence; running one on AIEN does not
relicense it. The SmolLM2 attribution detail lives in `ATTRIBUTION.md` and `NOTICE`.

## What the release package contains

Read from `scripts/package-release.sh`: the release tarball holds the seven built
binaries, `CONSTITUTION.md`, `README.md`, `install.sh`, `imprints/en2-trinity/`
(24 KB of text) and a generated `release.toml`. `release.toml` carries a `[model]`
table copied from `release/candidate.toml`; that table is **metadata only** (model id,
snapshot, SHA-256 digests, geometry, licence name). **No model weights are packaged in
any release artifact built by this script.** Weights are fetched or supplied
separately by the operator.

| Model | Licence | Upstream | Weights in a release artifact? | Where referenced |
|---|---|---|---|---|
| SmolLM2-1.7B-Instruct (HuggingFaceTB), rev `31b70e2e869a7173562077fd711b654946d38674` | Apache-2.0 (model card `license: apache-2.0`) | https://huggingface.co/HuggingFaceTB/SmolLM2-1.7B-Instruct | No | `NOTICE`, `ATTRIBUTION.md`, `docs/campaigns/smollm2-qualification/` |
| Llama 3.2 1B Instruct, as `unsloth/Llama-3.2-1B-Instruct`, snapshot `5a8abab4a5d6f164389b1079fb721cfab8d7126c` (CAND-4) | Llama 3.2 Community License (`licence` in `release/candidate.toml`) | https://huggingface.co/unsloth/Llama-3.2-1B-Instruct (re-host of Meta's model) | No. Digests only in `release.toml` | `release/candidate.toml`, `docs/release/ORACLE-FIXTURE-CAND4.md` |
| Qwen3-4B-Instruct-2507 (Qwen), HF revision `cdbee75f` | Apache-2.0 per `docs/campaigns/open-model-qwen3/ACCEPTANCE-v3.md` (upstream card not re-checked, UNVERIFIED) | https://huggingface.co/Qwen/Qwen3-4B-Instruct-2507 | No. Referenced in campaign docs only; not qualified, not distributed, no notice shipped (no Qwen3 entry in `NOTICE`) | `docs/campaigns/open-model-qwen3/` |

## CAND-4 (Llama 3.2 1B): internal only

`release/candidate.toml` records `distribution = "internal-only until an open-licence
model passes"`. CAND-4 is used for internal qualification and is **never distributed**
by this project, in a release archive or otherwise.

If it were ever distributed (weights or a product built on them), the Llama 3.2
Community License would apply and would need at least: a copy of that licence given
to every recipient, and the "Built with Llama" attribution. These requirements are
from general knowledge of the licence and are **UNVERIFIED**: the official page
(`https://www.llama.com/llama3_2/license/`) redirected through developer.meta.com to
dev.meta.ai, which returned HTTP 404 on 2026-10-07, so the text could not be checked.
Re-read the official licence before any distribution. Its text is not reproduced here.

## Open question found while preparing this file

The release tarball does not include `LICENSE` or `NOTICE`. AGPL distribution
generally expects the licence text to travel with the binaries. Adding them changes the
package bytes and its pinned digests, so it was not changed in this PR.
