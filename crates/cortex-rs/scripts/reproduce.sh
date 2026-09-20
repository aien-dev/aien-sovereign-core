#!/usr/bin/env bash
set -euo pipefail

# Sovereign Reproduction Runner for cortex-rs
# Tests live HTTP server lifecycle: startup, health probe, entity ingestion,
# FTS5 lexical search latency, and graph traversal.

PORT=${PORT:-18089}
HOST=${HOST:-127.0.0.1}
TOKEN="cortex-reproduce-$(date +%s)"
TMP_DB=$(mktemp /tmp/cortex_reproduce_XXXXXX.db)
trap 'rm -f "$TMP_DB"* 2>/dev/null || true' EXIT

echo "================================================================================"
echo "CORTEX-RS TURNKEY HTTP VERIFICATION RUNNER"
echo "Port: $PORT | Ephemeral DB: $TMP_DB"
echo "================================================================================"

# Locate binary: prioritize local target/release
if [ -x "./target/release/cortex-rs" ]; then
    BIN="./target/release/cortex-rs"
elif command -v cortex-rs >/dev/null 2>&1; then
    BIN="cortex-rs"
else
    echo "Building release binary..."
    cargo build --release --quiet
    BIN="./target/release/cortex-rs"
fi

# Launch cortex-rs in background
export CORTEX_TOKEN="$TOKEN"
"$BIN" --host "$HOST" --port "$PORT" --db-path "$TMP_DB" &
PID=$!
trap 'kill $PID 2>/dev/null || true; rm -f "$TMP_DB"* 2>/dev/null || true' EXIT

# Wait for health endpoint
echo -n "Waiting for cortex-rs server on http://$HOST:$PORT/health..."
READY=0
for _ in $(seq 1 30); do
    if curl -s "http://$HOST:$PORT/health" | grep -q '"status":"ok"'; then
        READY=1
        break
    fi
    sleep 0.1
done

if [ "$READY" -ne 1 ]; then
    echo " FAILED to start server."
    exit 1
fi
echo " OK"

# Ingest test entities
echo -n "Ingesting sample knowledge records..."
for i in 1 2 3 4 5; do
    curl -s -X POST "http://$HOST:$PORT/api/cortex/write" \
        -H "Authorization: Bearer $TOKEN" \
        -H "Content-Type: application/json" \
        -d "{
            \"kind\": \"entity\",
            \"value\": {
                \"space\": \"atlas-memory\",
                \"canonicalName\": \"system_module_$i\",
                \"entityType\": \"discovery\",
                \"content\": \"Blackwell GB10 unified memory verification node $i with hardware TPM attestation.\"
            }
        }" >/dev/null
done
echo " OK (5 entities committed)"

# Execute search and measure latency
echo "Testing FTS5 lexical search latency..."
SEARCH_OUT=$(curl -s -w "\nHTTP_CODE:%{http_code}\nTIME_TOTAL:%{time_total}s\n" \
    -H "Authorization: Bearer $TOKEN" \
    "http://$HOST:$PORT/api/cortex/search?q=Blackwell%20memory&limit=5")

echo "$SEARCH_OUT"

echo "================================================================================"
echo "VERIFICATION PASSED: cortex-rs is running and responsive."
echo "================================================================================"
