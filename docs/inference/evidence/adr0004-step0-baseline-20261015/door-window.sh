#!/usr/bin/env bash
# ADR 0004 step 0, G30-door baseline: the door's served path (SamplingObserver, full logits read
# back every decode step) on own30b_check's 4 prompts with one fixed seed. Run under one
# quietlock hold. Starts aien-serve-http on PORT (never 18091, never 18092), sends the 4 prompts
# twice (passes 1 and 2), stops it with SIGTERM, starts a second process and sends them once more
# (pass 3), so the receipt shows the tokens repeat inside one process and across processes.
#   door-window.sh <aien-serve-http> <evidence-dir> [port]
# Request: one user message, seed 1, temperature 1.0, top_p 1.0, top_k 0 (sampling-gate.sh
# gate (a) settings: the whole distribution is sampled, so any logits change can move a token),
# max_tokens 64, stream false. Reply digest = sha256 of the exact content string (jq -j).
set -uo pipefail
BIN=$1 EV=$2 PORT=${3:-18103}
case "$PORT" in 18091 | 18092) echo "door-window: port $PORT is reserved" >&2; exit 2 ;; esac
MODEL=$HOME/models/qwen3-30b-a3b-instruct-2507-fp8
MID=qwen3-30b-a3b-instruct-2507
HERE=$(cd "$(dirname "$0")" && pwd)
PROMPTS=$HERE/prompts.json
mkdir -p "$EV"
ts() { date -u +%Y-%m-%dT%H:%M:%SZ; }
log() { echo "$(ts) $*" | tee -a "$EV/window.log"; }
CUR=
cleanup() { [ -n "$CUR" ] && kill -0 "$CUR" 2>/dev/null && kill -TERM "$CUR" && wait "$CUR"; }
trap cleanup EXIT

start() { # session name
	if ss -ltn | grep -q "127.0.0.1:$PORT "; then log "port $PORT busy"; return 1; fi
	log "start $1: $BIN --engine omega --model-dir $MODEL --port $PORT (env: no AIEN_* or OWN30B_* set)"
	env -u AIEN_OMEGA_SPIN_US -u OWN30B_DECODE_GRAPH "$BIN" --engine omega --model-dir "$MODEL" --port "$PORT" \
		> "$EV/$1.server.log" 2>&1 &
	CUR=$!
	local i
	for i in $(seq 1 300); do
		if curl -sf "http://127.0.0.1:$PORT/v1/models" > /dev/null 2>&1; then
			log "start $1: ready, pid $CUR ($(grep -m1 'ready in' "$EV/$1.server.log" | grep -o 'ready in [0-9.]* s'))"
			return 0
		fi
		kill -0 "$CUR" 2>/dev/null || { log "start $1: server exited"; return 1; }
		sleep 1
	done
	log "start $1: not ready in 300 s"
	return 1
}
stop() {
	kill -TERM "$CUR"
	wait "$CUR"
	log "stop: pid $CUR exit $?"
	CUR=
}
pass() { # pass number
	local n i name text code
	n=$1
	for i in 0 1 2 3; do
		name=$(jq -r ".[$i].name" "$PROMPTS")
		text=$(jq -r ".[$i].text" "$PROMPTS")
		code=$(curl -s -o "$EV/pass$n-$name.json" -w '%{http_code}' "http://127.0.0.1:$PORT/v1/chat/completions" \
			-H 'content-type: application/json' \
			-d "$(jq -cn --arg m "$MID" --arg p "$text" \
				'{model: $m, max_tokens: 64, stream: false, temperature: 1.0, top_p: 1.0, top_k: 0, seed: 1,
				  messages: [{role: "user", content: $p}]}')")
		if [ "$code" != 200 ]; then log "pass$n $name HTTP $code"; return 1; fi
		log "pass$n $name sha256 $(jq -j '.choices[0].message.content' "$EV/pass$n-$name.json" | sha256sum | cut -d' ' -f1)" \
			"$(jq -c '{finish: .choices[0].finish_reason, prompt: .usage.prompt_tokens, reply: .usage.completion_tokens,
				seed: .usage.aien_sampling.seed, mode: .usage.aien_sampling.mode, reply_logprob: .usage.aien_sampling.reply_logprob}' \
				"$EV/pass$n-$name.json")"
	done
}

log "door-window: binary $BIN sha256 $(sha256sum "$BIN" | cut -d' ' -f1), port $PORT"
start session-a || exit 1
pass 1 || exit 1
pass 2 || exit 1
stop
start session-b || exit 1
pass 3 || exit 1
stop
log "door-window: done"
