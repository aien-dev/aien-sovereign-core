#!/usr/bin/env bash
export AIEN_BIN=/home/drakestapleton/workspace/oq3-v6-build-de9c626/target/release/aien-cli AIEN_MODEL_PATH=/home/drakestapleton/models/qwen3-4b-instruct-2507-cdbee75/model.safetensors.index.json AIEN_TOKENIZER_PATH=/home/drakestapleton/models/qwen3-4b-instruct-2507-cdbee75/tokenizer.json AIEN_KV_CONTEXT_TOKENS=1024 AIEN_FORCE_CPU_STUB=1
export AIEN_COMPOSE_DOC_MAX_TOKENS=24 AIEN_COMPOSE_DOC_BUDGET_MS=590000 NP1_DIR=~/workspace/.worktrees/d1-qwen3-8b/docs/campaigns/next-phase-1 NP1_ALLEN_BUILDER=/home/drakestapleton/workspace/.worktrees/d1-qwen3-8b/docs/campaigns/open-model-qwen3/build-allen-subject.sh OQ3_SC_DIR=~/workspace/.worktrees/d1-qwen3-8b OQ3_ALLEN_TARGET=~/workspace/d1-proof/allen-target
bash /home/drakestapleton/workspace/.worktrees/d1-qwen3-8b/docs/campaigns/open-model-qwen3/run-campaign-allen.sh ~/workspace/d1-proof/run4b
echo "driver exit $?"
