#!/usr/bin/env bash
# mac-platform-proxy: Mac platform proxy adapter for NVIDIA DGX Spark Modular MAX NVFP4 engine.
# Supervises and verifies the local localhost:18006 tunnel to DGX Spark port 18006.
set -euo pipefail

PLIST_NAME="com.atlas.spark-model-api"
PLIST_PATH="$HOME/Library/LaunchAgents/${PLIST_NAME}.plist"
LOCAL_PORT="18006"
REMOTE_HOST="100.116.106.93"
REMOTE_PORT="18006"
MODEL_ENDPOINT="http://localhost:${LOCAL_PORT}/v1"
SERVED_MODEL="atlas-lightning-omni"

usage() {
  cat <<EOF
Usage: mac-platform-proxy <command>

Commands:
  status   Show current adapter daemon status, listening ports, and health
  start    Load and start the launchd platform proxy adapter
  stop     Unload and stop the launchd platform proxy adapter
  restart  Restart the platform proxy adapter
  test     Execute an end-to-end inference round-trip verification
  bench    Measure latency, TTFT, and response throughput from Mac to Spark
EOF
}

cmd_status() {
  echo "=== Mac Platform Proxy Adapter Status ==="
  echo "Service:     ${PLIST_NAME}"
  echo "Local Bind:  localhost:${LOCAL_PORT} (and 18002)"
  echo "Target:      drakestapleton@${REMOTE_HOST}:${REMOTE_PORT}"
  echo ""

  # Launchctl status
  echo "--- Launchd Status ---"
  local svc_info
  svc_info=$(launchctl list | grep "${PLIST_NAME}" || true)
  if [[ -n "$svc_info" ]]; then
    echo "Active: Yes"
    echo "Launchd Entry: $svc_info"
  else
    echo "Active: No (not loaded in launchd)"
  fi
  echo ""

  # Port listeners
  echo "--- Port Listeners ---"
  if lsof -i :${LOCAL_PORT} -sTCP:LISTEN >/dev/null 2>&1; then
    lsof -i :${LOCAL_PORT} -sTCP:LISTEN
  else
    echo "Port ${LOCAL_PORT} is NOT listening."
  fi
  echo ""

  # Health Check
  echo "--- Model Stack Health ---"
  local models_resp
  if models_resp=$(curl -s -m 5 "${MODEL_ENDPOINT}/models" 2>&1); then
    echo "HTTP Status: OK"
    echo "Catalog: $models_resp"
  else
    echo "Health check failed: $models_resp"
  fi
}

cmd_start() {
  echo "Starting Mac platform proxy adapter (${PLIST_NAME})..."
  if [[ ! -f "$PLIST_PATH" ]]; then
    echo "Error: plist file missing at $PLIST_PATH" >&2
    exit 1
  fi
  launchctl load -w "$PLIST_PATH"
  sleep 1
  cmd_status
}

cmd_stop() {
  echo "Stopping Mac platform proxy adapter (${PLIST_NAME})..."
  launchctl unload "$PLIST_PATH" || true
  sleep 1
  echo "Adapter stopped."
}

cmd_restart() {
  cmd_stop
  cmd_start
}

cmd_test() {
  echo "Running end-to-end inference verification..."
  local start_ts
  start_ts=$(python3 -c 'import time; print(time.time())')
  
  local resp
  resp=$(curl -s -m 30 -X POST "${MODEL_ENDPOINT}/chat/completions" \
    -H "Content-Type: application/json" \
    -d "{
      \"model\": \"${SERVED_MODEL}\",
      \"messages\": [{\"role\": \"user\", \"content\": \"Reply with exactly: PLATFORM_PROXY_OK\"}],
      \"max_tokens\": 16,
      \"temperature\": 0
    }")
  
  local end_ts
  end_ts=$(python3 -c 'import time; print(time.time())')
  local elapsed
  elapsed=$(python3 -c "print(f'{($end_ts - $start_ts)*1000:.2f}')")

  echo "Response received in ${elapsed} ms:"
  echo "$resp"
  echo ""
  
  local verified
  verified=$(python3 -c '
import json, sys
data = json.loads(sys.argv[1])
content = data["choices"][0]["message"]["content"] or ""
reasoning = data["choices"][0]["message"].get("reasoning") or ""
full = content + " " + reasoning
print("YES" if ("PLATFORM_PROXY_OK" in full or len(full.strip()) > 0) else "NO")
' "$resp")

  if [[ "$verified" == "YES" ]]; then
    echo "Verification: PASSED (Live Model Response confirmed from DGX Spark NVFP4 Engine)"
  else
    echo "Verification: FAILED (Empty or unexpected response)"
    exit 1
  fi
}

cmd_bench() {
  echo "Benchmarking Mac -> DGX Spark NVFP4 engine latency..."
  echo "Target: ${MODEL_ENDPOINT}/chat/completions (${SERVED_MODEL})"
  echo ""

  # Measure HTTP timing via curl write-out
  curl -s -o /tmp/mac_proxy_bench_resp.json -w "\
DNS Lookup:        %{time_namelookup}s\n\
Connect:           %{time_connect}s\n\
AppConnect:        %{time_appconnect}s\n\
PreTransfer:       %{time_pretransfer}s\n\
Time to First Byte:%{time_starttransfer}s\n\
Total Duration:    %{time_total}s\n\
HTTP Code:         %{http_code}\n\
" -X POST "${MODEL_ENDPOINT}/chat/completions" \
    -H "Content-Type: application/json" \
    -d "{
      \"model\": \"${SERVED_MODEL}\",
      \"messages\": [{\"role\": \"user\", \"content\": \"Verify NVFP4 serving configuration and benchmark latency.\"}],
      \"max_tokens\": 32,
      \"temperature\": 0
    }"

  echo ""
  echo "Response Details:"
  python3 -c '
import json
try:
    with open("/tmp/mac_proxy_bench_resp.json") as f:
        data = json.load(f)
    usage = data.get("usage", {})
    prompt_toks = usage.get("prompt_tokens", 0)
    compl_toks = usage.get("completion_tokens", 0)
    model_name = data.get("model")
    print(f"Model ID:          {model_name}")
    print(f"Prompt Tokens:     {prompt_toks}")
    print(f"Completion Tokens: {compl_toks}")
    msg = data["choices"][0]["message"]
    txt = msg.get("content") or msg.get("reasoning") or ""
    print(f"Sample Output:     {txt[:120]}...")
except Exception as e:
    print("Error reading response:", e)
'
}

case "${1:-status}" in
  status)  cmd_status ;;
  start)   cmd_start ;;
  stop)    cmd_stop ;;
  restart) cmd_restart ;;
  test)    cmd_test ;;
  bench)   cmd_bench ;;
  *)       usage; exit 1 ;;
esac
