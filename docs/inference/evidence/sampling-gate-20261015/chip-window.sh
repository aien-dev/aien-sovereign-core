#!/usr/bin/env bash
# OWN-8B stage S chip window on :18091 (run under quietlock hold). Swaps the pinned 30B server for
# the stage S build, runs scripts/sampling-gate.sh (release build, then the two debug-assertion
# mutants), then restarts the ORIGINAL pinned server from its saved command line (setsid).
#   chip-window.sh <worktree> <evidence-dir>
set -uo pipefail
WT=$1 EV=$2
PORT=18091
MODEL=$HOME/models/qwen3-30b-a3b-instruct-2507-fp8
MID=qwen3-30b-a3b-instruct-2507
REL=$WT/target/gpu-release/release/aien-serve-http
DBG=$WT/target/gpu-dbgassert/release/aien-serve-http
GATE=$WT/scripts/sampling-gate.sh
REPRO=(/tmp/gtrial/out-3att-aborted/gen/a-glyph-1/dev01/r1.req.json
	/tmp/gtrial/out-3att-aborted/gen/a-glyph-2/dev01/r1.req.json
	/tmp/gtrial/out-3att-aborted/gen/a-glyph-3/dev01/r1.req.json)
CMDFILE=$HOME/handoffs/own30b-serve-cmdline-merged.txt
RUNLOG=$HOME/workspace/exp445-runlogs/own30b-18091.log
mkdir -p "$EV"
ts() { date -u +%Y-%m-%dT%H:%M:%SZ; }
log() { echo "$(ts) $*" | tee -a "$EV/window.log"; }

wait_models() { # seconds -> 0 when /v1/models answers
	local i
	for i in $(seq 1 "$1"); do
		curl -sf "http://127.0.0.1:$PORT/v1/models" > /dev/null 2>&1 && return 0
		sleep 1
	done
	return 1
}
wait_port_free() {
	local i
	for i in $(seq 1 120); do
		ss -ltn | grep -q "127.0.0.1:$PORT " || return 0
		sleep 1
	done
	return 1
}
stop_pid() { # pid
	kill -TERM "$1" 2>/dev/null
	local i
	for i in $(seq 1 120); do kill -0 "$1" 2>/dev/null || return 0; sleep 1; done
	log "pid $1 did not exit after SIGTERM in 120 s"
	return 1
}

restore() {
	# A caught (not ignored) TERM: restore runs to the end, and the child resets it to default.
	trap - EXIT
	trap 'log "TERM during restore: ignored until the pinned server is back"' TERM
	[ -n "${RESTORED:-}" ] && return 0
	RESTORED=1
	if [ -n "${CUR:-}" ] && kill -0 "$CUR" 2>/dev/null; then stop_pid "$CUR"; fi
	wait_port_free || log "port $PORT still busy before restore"
	local l1 wd cmd
	l1=$(head -1 "$CMDFILE"); wd=${l1##* }; cmd=${l1% *}
	[ -f "$RUNLOG" ] && cp "$RUNLOG" "$RUNLOG.before-stage-s-$(date -u +%Y%m%dT%H%M%SZ)"
	log "restore: $cmd (wd $wd), log $RUNLOG"
	# shellcheck disable=SC2086
	(trap - EXIT TERM; cd "$wd" && exec setsid -f nohup $cmd > "$RUNLOG" 2>&1 < /dev/null)
	if wait_models 300; then
		log "restore: /v1/models answers: $(curl -s "http://127.0.0.1:$PORT/v1/models" | head -c 200)"
		log "restore: pid $(pgrep -f "aien-serve-http --engine omega.*--port $PORT" | head -1)"
	else
		log "restore: FAILED, /v1/models silent after 300 s"
	fi
}
trap restore EXIT
trap 'log "TERM: aborting (see ABORT)"; exit 9' TERM

# Start a server (name, binary, env...) in this group; sets CUR. Fails if it does not answer.
start() {
	local name=$1 bin=$2
	shift 2
	wait_port_free || { log "port $PORT busy"; return 1; }
	log "start $name: $* $bin --engine omega --model-dir $MODEL --port $PORT"
	env "$@" "$bin" --engine omega --model-dir "$MODEL" --port "$PORT" > "$EV/$name.server.log" 2>&1 &
	CUR=$!
	echo "$CUR" > "$EV/cur.pid"
	if wait_models 300; then
		log "start $name: ready ($(grep -m1 'ready in' "$EV/$name.server.log" | sed 's/.*ready in \([0-9.]* s\).*/\1/'))"
	else
		log "start $name: not ready in 300 s"
		return 1
	fi
}
gate() { # name, extra env...
	local name=$1
	shift
	env "$@" "$GATE" "http://127.0.0.1:$PORT" "$MID" "$EV/$name" "${REPRO[@]}" > "$EV/$name.gate.out" 2>&1
	local rc=$?
	log "gate $name rc=$rc: $(grep -E '^gate |^VERDICT' "$EV/$name.gate.out" | tr '\n' ';')"
}

# 0. Preconditions and the baseline greedy reply from the pinned server (warm smoke request).
if pgrep -f '^bash tools/exp445/[r]un.sh --config A --tasks /tmp/gtrial' > /dev/null; then log "a Glyph trial is running; abort"; trap - EXIT; exit 3; fi
OLD=$(pgrep -f "pin-bin-afe59ae1/aien-serve-http --engine omega.*--port $PORT" | head -1)
[ -n "$OLD" ] || { log "pinned server not found on :$PORT; abort"; trap - EXIT; exit 3; }
log "pinned server pid $OLD: $(tr '\0' ' ' < "/proc/$OLD/cmdline")"
curl -s "http://127.0.0.1:$PORT/v1/chat/completions" -H 'content-type: application/json' \
	-d '{"model":"qwen3-30b-a3b-instruct-2507","temperature":0,"max_tokens":48,"messages":[{"role":"user","content":"In one sentence, what is a Grace Blackwell GB10?"}]}' \
	-o "$EV/baseline-pinned-nonstream.json"
log "baseline pinned greedy: $(jq -c '{content: .choices[0].message.content, usage}' "$EV/baseline-pinned-nonstream.json")"

# Watchdog: a Glyph trial does not honour the quiet flag. If one starts, stop the test server
# at once (the trial sees connection refused, never a stage S or mutant reply) and abort.
MAIN=$$
(
	while kill -0 "$MAIN" 2>/dev/null; do
		if pgrep -f '^bash tools/exp445/[r]un.sh' > /dev/null; then
			echo "$(ts) WATCHDOG: trial started; stopping the test server and aborting" | tee -a "$EV/window.log" "$EV/ABORT"
			[ -f "$EV/cur.pid" ] && kill -TERM "$(cat "$EV/cur.pid")" 2>/dev/null
			kill -TERM "$MAIN"
			exit 0
		fi
		sleep 2
	done
) &

# 1. Swap: stop the pinned server.
stop_pid "$OLD" || exit 4
log "pinned server stopped"

# 2. Stage S release build: gates (a)-(d).
start release "$REL" || exit 5
gate release GREEDY_GOLDEN="$EV/baseline-pinned-nonstream.json"
stop_pid "$CUR"; CUR=

# 3. Negative controls: debug-assertion build with each mutant (must FAIL (b); force-greedy passes (c)).
for m in ignore-seed force-greedy; do
	start "mutant-$m" "$DBG" AIEN_SAMPLING_MUTANT=$m || exit 6
	gate "mutant-$m" GREEDY_GOLDEN="$EV/baseline-pinned-nonstream.json"
	stop_pid "$CUR"; CUR=
done
log "window done; restoring the pinned server"
