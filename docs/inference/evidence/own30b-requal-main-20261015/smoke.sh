#!/bin/bash
# One smoke of the merged-main 30B server (~/handoffs/own30b-serve-cmdline-merged.txt) on :18091, then SIGTERM by PID once
# idle. Requests are the pre-merge RC smoke's (own30b-rc-20261014T0153Z/server-smoke). Run inside one quietlock hold.
set -u
EV=${EV:?}; RC=$HOME/workspace/sc-requal-main/docs/inference/evidence/own30b-rc-20261014T0153Z/server-smoke
mkdir -p "$EV"; cd $HOME/handoffs/main-bin-9557e0c1 || exit 2
CMD="./aien-serve-http --engine omega --model-dir $HOME/models/qwen3-30b-a3b-instruct-2507-fp8 --port 18091"
{ echo "start $(date -u +%FT%TZ) cwd $(pwd)"; echo "cmd $HOME/handoffs/main-bin-9557e0c1/aien-serve-http --engine omega --model-dir $HOME/models/qwen3-30b-a3b-instruct-2507-fp8 --port 18091"; echo "binary sha256 $(sha256sum aien-serve-http | cut -d' ' -f1)"; free -g | head -2; } > "$EV/smoke.start"
setsid $CMD > "$EV/server.log" 2>&1 < /dev/null &
PID=$!; echo "pid $PID" >> "$EV/smoke.start"
t0=$(date +%s); ready=0
while kill -0 $PID 2>/dev/null; do
  if curl -sf -m 5 http://127.0.0.1:18091/v1/models > "$EV/models.json" 2>/dev/null; then ready=1; break; fi
  sleep 2
done
echo "ready=$ready after $(( $(date +%s) - t0 )) s" >> "$EV/smoke.start"
if [ $ready = 1 ]; then
  for n in 1 3; do
    s=$(date +%s.%N); curl -s -m 300 http://127.0.0.1:18091/v1/chat/completions -H 'content-type: application/json' --data-binary @$RC/req$n.json > "$EV/resp$n.json"; e=$(date +%s.%N)
    echo "wall $(echo "$e - $s" | bc) s" > "$EV/resp$n.time"
  done
  s=$(date +%s.%N); curl -sN -m 300 http://127.0.0.1:18091/v1/chat/completions -H 'content-type: application/json' --data-binary @$RC/req2.sent.json > "$EV/resp2.sse"; e=$(date +%s.%N)
  echo "wall $(echo "$e - $s" | bc) s" > "$EV/resp2.time"
  cp $RC/req1.json $RC/req2.sent.json $RC/req3.json "$EV/"
fi
kill -TERM $PID 2>/dev/null; wait $PID; rc=$?
{ echo "server stopped (SIGTERM after the last response) exit $rc at $(date -u +%FT%TZ)"; ss -ltn | grep -c ':18091 ' | sed 's/^/listeners on 18091 after stop: /'; free -g | head -2; } > "$EV/smoke.end"
