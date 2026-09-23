#!/usr/bin/env bash
set -euo pipefail
# Home-relative paths: derived from $HOME, overridable via env vars.
# No hardcoded developer home directory.
AIEN_HOME="${AIEN_HOME:-$HOME}"
if [[ -f "$AIEN_HOME/.cache/huggingface/token" ]]; then
  export HF_TOKEN="$(cat "$AIEN_HOME/.cache/huggingface/token")"
fi
sync; sudo -n sysctl -w vm.drop_caches=3 2>/dev/null || true
export CUDA_VISIBLE_DEVICES=0
export MAX_SERVE_HOST=127.0.0.1
export MAX_SERVE_METRICS_ENDPOINT_PORT=8011

SNAPSHOT_DIR="${AIEN_SNAPSHOT_DIR:-$AIEN_HOME/.cache/huggingface/hub/models--nvidia--NVIDIA-Nemotron-3.5-Lightning-30B-A3B-BF16/snapshots/a9904d24bcc1d289a1950fa9d2b978c47cf903b9}"
WEIGHT_ARGS=()
for shard in $(ls -1 "$SNAPSHOT_DIR"/model-*.safetensors | sort); do
  WEIGHT_ARGS+=(--weight-path "$shard")
done

exec "${AIEN_MAX_BIN:-$AIEN_HOME/max-env/bin/max}" serve \
  --model nvidia/NVIDIA-Nemotron-3.5-Lightning-30B-A3B-BF16 \
  "${WEIGHT_ARGS[@]}" \
  --served-model-name atlas-lightning-omni \
  --custom-architectures "${AIEN_CUSTOM_ARCHES:-$AIEN_HOME/nemotron_h_kvexp}" \
  --port 18006 \
  --max-batch-size 8 \
  --max-length 32768 \
  --device-memory-utilization 0.85 \
  --quantization-encoding float4_e2m1fnx2 \
  --enable-structured-output \
  --enable-prefix-caching \
  --kv-cache-format bfloat16 \
  --fold-sampler-into-graph \
  --no-device-graph-capture
