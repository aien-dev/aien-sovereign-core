#!/usr/bin/env bash
# OWN-30B cut 3: one sealed chip run of own30b_check under the GB10 lock protocol.
#
#   scripts/own30b-chip-run.sh <evidence-dir> <run-name> <binary> <golden.json> [own30b_check chip args...]
#
# Announces with crumb whisper, takes /tmp/aien-gb10.lock for the whole run, records
# start/end stamps, the binary digest, the exact command, raw output and exit code, then
# refreshes SHA256SUMS in the evidence dir. Never runs two loads at once (the binary itself
# refuses to load with under 70 GB available).
set -uo pipefail
ev=$1; name=$2; bin=$3; golden=$4; shift 4
ckpt=${OWN30B_CHECKPOINT:-$HOME/models/qwen3-30b-a3b-instruct-2507-fp8}
mkdir -p "$ev/$name"
repo=$(cd "$(dirname "$0")/.." && pwd)
{
	echo "start $(date -u +%Y-%m-%dT%H:%M:%SZ)"
	echo "sc commit $(git -C "$repo" rev-parse HEAD) branch $(git -C "$repo" rev-parse --abbrev-ref HEAD)"
	echo "binary $bin sha256 $(sha256sum "$bin" | cut -d' ' -f1)"
	echo "golden $golden sha256 $(sha256sum "$golden" | cut -d' ' -f1)"
	echo "omega lib ${AIEN_OMEGA_GPU_LIB:-pinned omega.lock} ${AIEN_OMEGA_GPU_LIB_SHA256:-}"
	echo "env AIEN_OMEGA_SPIN_US=${AIEN_OMEGA_SPIN_US:-} AIEN_OMEGA_CTA_BUDGET=${AIEN_OMEGA_CTA_BUDGET:-}"
	echo "cmd flock /tmp/aien-gb10.lock $bin chip $ckpt $golden $ev/$name $*"
	free -g | head -2
	uname -r
} > "$ev/$name.start"
crumb whisper odysseus-9e "$repo" - "${OWN30B_WHISPER:-OWN-30B (F): chip run $name, ~10 min}" >/dev/null 2>&1 || true
flock /tmp/aien-gb10.lock "$bin" chip "$ckpt" "$golden" "$ev/$name" "$@" > "$ev/$name.raw" 2> "$ev/$name.err"
rc=$?
echo "$rc" > "$ev/$name.rc"
{ echo "end $(date -u +%Y-%m-%dT%H:%M:%SZ)"; free -g | head -2; } > "$ev/$name.end"
( cd "$ev" && find . -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS )
echo "$name rc=$rc"
grep -E "^(LOAD|PARITY|CONTROL|TOKENS|SUMMARY)" "$ev/$name.raw"
exit "$rc"
