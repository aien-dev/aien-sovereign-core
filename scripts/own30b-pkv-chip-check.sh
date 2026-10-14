#!/usr/bin/env bash
# Persistent KV acceptance check on the chip (sc#505, locked by #499). CHIP-ONLY: never run by
# CI or cargo test. It starts its OWN servers on 127.0.0.1:18097 (PKV_CHECK_PORT; never touches :18091, the live
# server, or its PID/log files) and loads the 30B checkpoint each time (~30.5 GB resident, about
# 18 s load plus the checkpoint digest when persistence is on).
#
#   scripts/own30b-pkv-chip-check.sh <evidence-dir> <aien-serve-http binary>
#
# The binary must be a native release build of this branch from a clean checkout (dirty or
# unknown builds refuse persistence by design), e.g.
#   AIEN_OMEGA_DIR=<omega checkout at the omega.lock pin> cargo build --release -p aien-serve-http
#
# Key: by default the store key comes from atlas-vault (`atlas-vault run AIEN_KV_STORE_KEY --`);
# PKV_CHECK_EPHEMERAL_KEY=1 instead makes a throwaway key that lives only in this script's
# memory and the child's environment (never printed or saved). The store is a private temp dir.
#
# Refuses to start while :18091 answers, because two omega processes sharing the chip at once is
# UNKNOWN territory: pause the live server first, or set PKV_CHECK_WITH_LIVE=1 to accept the risk.
#
# Phases (same greedy two-turn conversation, temperature 0, seed 1):
#   off    persistence off, one server kept running          -> turn2 = T_off (uninterrupted)
#   on     persistence on (fresh store), one server running   -> turn2 = T_on
#   park   persistence on (fresh store S): turn 1, wait for the parks, stop the server
#   restore restart on S: turn 2 must restore (restores == 1)  -> turn2 = T_restored
#   neg-switch restart on S with --prefill-device-host off: restores == 0 (another identity)
#   neg-corrupt flip one byte in every stored object of S, restart: restores == 0, still answers
# PASS when T_on == T_off and T_restored == T_off and both negative controls restore nothing.
# Rough duration: 6 server starts, 5 to 8 minutes.
set -euo pipefail
dir=$1 BIN=$2
MODEL=${OWN30B_MODEL_DIR:-$HOME/models/qwen3-30b-a3b-instruct-2507-fp8}
PORT=${PKV_CHECK_PORT:-18097}
base="http://127.0.0.1:$PORT"
mkdir -p "$dir"
work=$(mktemp -d /tmp/pkv505-chip.XXXXXX)
chmod 700 "$work"
PIDF="$work/server.pid"
trap 'stop_server; rm -rf "$work"' EXIT

if curl -s -m 2 http://127.0.0.1:18091/health >/dev/null 2>&1 && [ "${PKV_CHECK_WITH_LIVE:-0}" != 1 ]; then
	echo "refusing: the live server on :18091 is up (pause it first, or PKV_CHECK_WITH_LIVE=1)" >&2
	exit 2
fi
if ss -Hltn "sport = :$PORT" | grep -q . || curl -s -m 2 "$base/health" >/dev/null 2>&1; then
	echo "refusing: something already listens on :$PORT (18095 is spark-cockpit; pick a free PKV_CHECK_PORT)" >&2
	exit 2
fi
if [ "${PKV_CHECK_EPHEMERAL_KEY:-0}" = 1 ]; then
	KEY=$(head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n')
fi

stop_server() {
	if [ -f "$PIDF" ]; then
		local p
		p=$(cat "$PIDF")
		if [ -n "$p" ] && [ "$(ps -o comm= -p "$p" 2>/dev/null || true)" = aien-serve-http ]; then
			kill "$p"
			for _ in $(seq 1 100); do kill -0 "$p" 2>/dev/null || break; sleep 0.1; done
		fi
		rm -f "$PIDF"
	fi
}

# start <log-name> <store-dir or -> [extra server flags...]
start_server() {
	local name=$1 store=$2
	shift 2
	local log="$dir/$name.log"
	local cmd="echo \$\$ > '$PIDF'; exec '$BIN' --engine omega --model-dir '$MODEL' --port $PORT $*"
	if [ "$store" = - ]; then
		setsid env -u AIEN_KV_STORE_DIR -u AIEN_KV_STORE_KEY sh -c "$cmd" >"$log" 2>&1 </dev/null &
	elif [ -n "${KEY:-}" ]; then
		setsid env AIEN_KV_STORE_DIR="$store" AIEN_KV_STORE_KEY="$KEY" sh -c "$cmd" >"$log" 2>&1 </dev/null &
	else
		setsid atlas-vault run AIEN_KV_STORE_KEY -- env AIEN_KV_STORE_DIR="$store" sh -c "$cmd" >"$log" 2>&1 </dev/null &
	fi
	for _ in $(seq 1 300); do
		grep -q "serving \|load failed\|bind failed" "$log" 2>/dev/null && break
		sleep 1
	done
	grep -q "serving " "$log" || { echo "server $name did not start; see $log" >&2; exit 1; }
	grep "persistent KV" "$log" || true
}

SYS="You are the OWN-30B assistant on the AIEN Spark. Answer briefly."
for i in $(seq 1 40); do SYS="$SYS Rule $i: keep answers short, never invent facts, say UNKNOWN when unsure."; done
Q1="Name three uses of a torque wrench."
Q2="Which of those matters most on a bicycle, and why?"

chat() { # <out-name> <messages-json>
	jq -n --argjson m "$2" '{model:"x", messages:$m, max_tokens:64, temperature:0, seed:1, stream:false}' |
		curl -s -m 600 "$base/v1/chat/completions" -H 'content-type: application/json' -d @- >"$dir/$1.json"
	jq -r '.choices[0].message.content' "$dir/$1.json"
}
m1=$(jq -n --arg s "$SYS" --arg q "$Q1" '[{role:"system",content:$s},{role:"user",content:$q}]')
turn2_msgs() { jq -n --argjson m "$m1" --arg a "$1" --arg q "$Q2" '$m + [{role:"assistant",content:$a},{role:"user",content:$q}]'; }
health() { curl -s -m 10 "$base/health" >"$dir/$1-health.json"; cat "$dir/$1-health.json"; }
pk() { jq -r ".persistent_kv.$2" "$dir/$1-health.json"; }

wait_parks() { # until parked_chunks > 0 and unchanged for 3 s
	local last=-1 now
	for _ in $(seq 1 60); do
		now=$(curl -s -m 5 "$base/health" | jq -r '.persistent_kv.parked_chunks')
		[ "$now" != 0 ] && [ "$now" = "$last" ] && return 0
		last=$now
		sleep 3
	done
	echo "parks did not settle" >&2
	exit 1
}

echo "== off: persistence off, uninterrupted"
start_server off -
a1_off=$(chat off-turn1 "$m1")
t_off=$(chat off-turn2 "$(turn2_msgs "$a1_off")")
health off >/dev/null
stop_server

echo "== on: persistence on, uninterrupted"
start_server on "$work/store-on"
a1_on=$(chat on-turn1 "$m1")
t_on=$(chat on-turn2 "$(turn2_msgs "$a1_on")")
health on >/dev/null
stop_server

echo "== park: turn 1, wait for the parks, stop"
S="$work/store"
start_server park "$S"
a1=$(chat park-turn1 "$m1")
wait_parks
health park >/dev/null
stop_server

echo "== restore: restart on the same store, turn 2"
start_server restore "$S"
t_restored=$(chat restore-turn2 "$(turn2_msgs "$a1")")
health restore >/dev/null
stop_server

echo "== neg-switch: restart with another prefill switch (another identity)"
start_server neg-switch "$S" --prefill-device-host off
chat neg-switch-turn2 "$(turn2_msgs "$a1")" >/dev/null
health neg-switch >/dev/null
stop_server

echo "== neg-corrupt: flip one byte in every stored object, restart"
find "$S/objects" -type f | while read -r f; do
	sz=$(stat -c %s "$f")
	off=$((sz / 2))
	b=$(dd if="$f" bs=1 skip=$off count=1 2>/dev/null | od -An -tu1 | tr -d ' ')
	printf "$(printf '\\%03o' $(((b ^ 64) & 255)))" | dd of="$f" bs=1 seek=$off count=1 conv=notrunc 2>/dev/null
done
start_server neg-corrupt "$S"
chat neg-corrupt-turn2 "$(turn2_msgs "$a1")" >/dev/null
health neg-corrupt >/dev/null
stop_server

fail=0
check() { if [ "$2" = "$3" ]; then echo "PASS $1"; else echo "FAIL $1 ($2 != $3)"; fail=1; fi; }
check "turn 1 identical off/on/park" "$a1_off|$a1_on" "$a1|$a1"
check "T_on == T_off (persistence changes no token)" "$t_on" "$t_off"
check "T_restored == T_off (restored = uninterrupted)" "$t_restored" "$t_off"
check "restore served once" "$(pk restore restores)" 1
check "neg-switch restores nothing" "$(pk neg-switch restores)" 0
check "neg-corrupt restores nothing" "$(pk neg-corrupt restores)" 0
check "off health has no persistent_kv" "$(jq -r 'has("persistent_kv")' "$dir/off-health.json")" false
(cd "$dir" && sha256sum ./*.json ./*.log >SHA256SUMS)
[ $fail = 0 ] && echo "PKV CHIP CHECK: PASS" || echo "PKV CHIP CHECK: FAIL"
exit $fail
