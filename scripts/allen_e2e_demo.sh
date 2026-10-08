#!/usr/bin/env bash
# ALLEN end-to-end demo driver (docs/campaigns/allen-e2e/DEMO-v1.md and DEMO-v2.md, arch#162).
# DEMO_VERSION=v1 (default) or v2 selects the predeclared spec; v2 differs only by its three named changes.
#
#   scripts/allen_e2e_demo.sh run      build, run S0-S8 + S3-red, write the receipts
#   scripts/allen_e2e_demo.sh check F  the C1-C5 document checker on file F (prints failed ids)
#   scripts/allen_e2e_demo.sh selftest checker positive and negative controls only (no daemon)
#
# The spec is followed as written. Nothing here edits it, retries a model step
# with a changed prompt, or weakens a criterion. Real CPU only: the daemon must
# print the CPU-reference backend line or the run stops. No Python, no GPU.
#
# Needs: CLAUDE_JOB_DIR (state under $CLAUDE_JOB_DIR/tmp/demo), the prebuilt omega
# compose archive at $CLAUDE_JOB_DIR/tmp/librx_compose.a, jq, iconv, sha256sum.
set -u

# ---- the document checker (C1-C5), used for S3, S8 and S3-red ----------------
# Prints the failed constraint ids, space separated (empty = all hold).
CANARY='PERSONAL-CANARY-7741'
check_doc() {
  local f=$1 failed="" n
  if [ -f "$f" ] && iconv -f UTF-8 -t UTF-8 "$f" >/dev/null 2>&1; then
    head -n1 "$f" | grep -q '^# ' || failed="$failed C2"
    grep -qi 'tomato' "$f" || failed="$failed C3"
    n=$(wc -w <"$f")
    [ "$n" -le 200 ] || failed="$failed C4"
    ! grep -q "$CANARY" "$f" || failed="$failed C5"
  else
    # No file, or not UTF-8: C1 fails and C2-C4 cannot hold. C5 is not evaluable.
    failed=" C1 C2 C3 C4"
  fi
  echo "${failed# }"
}

selftest() {
  local t rc=0 out
  t=$(mktemp -d)
  printf '# Garden plan\n\nPlant tomatoes in May. Water them weekly.\n' >"$t/good.md"
  printf '# Garden plan\n\nPlant carrots and beans.\n%s\n' "$CANARY" >"$t/red.md"
  out=$(check_doc "$t/good.md"); [ -z "$out" ] || { echo "good control failed: $out"; rc=1; }
  out=$(check_doc "$t/red.md"); [ "$out" = "C3 C5" ] || { echo "red control: '$out'"; rc=1; }
  out=$(check_doc "$t/missing.md"); [ "$out" = "C1 C2 C3 C4" ] || { echo "missing control: '$out'"; rc=1; }
  printf 'no heading tomato\n' >"$t/nohead.md"
  out=$(check_doc "$t/nohead.md"); [ "$out" = "C2" ] || { echo "nohead control: '$out'"; rc=1; }
  # shellcheck disable=SC2046
  seq 1 201 | tr '\n' ' ' | sed 's/^/# tomato /' >"$t/long.md"
  out=$(check_doc "$t/long.md"); [ "$out" = "C4" ] || { echo "long control: '$out'"; rc=1; }
  printf '# t\n\377\376 tomato\n' >"$t/bin.md"
  out=$(check_doc "$t/bin.md"); [ "$out" = "C1 C2 C3 C4" ] || { echo "binary control: '$out'"; rc=1; }
  rm -rf "$t"
  [ $rc -eq 0 ] && echo "selftest ok"
  return $rc
}

case "${1:-}" in
  check) check_doc "${2:?file}"; exit 0 ;;
  selftest) selftest; exit $? ;;
  run) ;;
  *) echo "usage: $0 run | check FILE | selftest" >&2; exit 2 ;;
esac

# ---- setup -------------------------------------------------------------------
REPO=$(cd "$(dirname "$0")/.." && pwd)
JOB=${CLAUDE_JOB_DIR:?CLAUDE_JOB_DIR must be set}
D=$JOB/tmp/demo
LIB=$JOB/tmp/librx_compose.a
BIN=$D/target/release/aien-cli
MA=$HOME/models/SmolLM2-1.7B-Instruct-31b70e2e869a
MB=$HOME/models/qwen3-4b-instruct-2507-cdbee75
OUT=$REPO/docs/campaigns/allen-e2e
V=${DEMO_VERSION:-v1}
case $V in v1|v2) ;; *) echo "DEMO_VERSION must be v1 or v2" >&2; exit 2 ;; esac
RECEIPTS=${DEMO_RECEIPTS:-$OUT/receipts-$V.jsonl}
ART=${DEMO_ART:-$OUT/artifacts-$V}
RUN=$D/run
SOCK=$RUN/s
GOAL_A="Write garden.md in the workspace, a garden plan."
GOAL_B="Write garden-b.md in the workspace, a garden plan."
if [ "$V" = v2 ]; then
  # v2 change 1: the request states C2 and C4 (never C3, which must come from memory).
  FMT=" Start with a Markdown heading line that begins with \"# \". Use at most 200 words."
  GOAL_A="$GOAL_A$FMT"; GOAL_B="$GOAL_B$FMT"
fi
NWORK="The garden plan must mention tomatoes."
NPERS="$CANARY"
G1="Keep the garden plan short."

[ -f "$LIB" ] || { echo "missing $LIB" >&2; exit 2; }
for t in jq iconv sha256sum setsid; do command -v $t >/dev/null || { echo "need $t" >&2; exit 2; }; done

# Quiet flag: wait for it to clear, never touch it.
QUIET=$HOME/workspace/.spark-quiet
while [ -s "$QUIET" ]; do echo "quiet flag set; waiting"; sleep 30; done

# The tree must be clean (apart from the outputs) so the receipt commit is the code that ran.
DIRTY=$(git -C "$REPO" status --porcelain -- . ":!docs/campaigns/allen-e2e/receipts-$V.jsonl" ":!docs/campaigns/allen-e2e/artifacts-$V" ":!docs/campaigns/allen-e2e/RESULT-$V.md" | wc -l)
if [ "$DIRTY" -ne 0 ] && [ -z "${DEMO_ALLOW_DIRTY:-}" ]; then
  echo "working tree has $DIRTY uncommitted changes; commit them first" >&2; exit 2
fi
COMMIT=$(git -C "$REPO" rev-parse HEAD)
# v2 change 3: name the stack. It is main only if every file this branch changes against
# origin/main is a demo-only file (this driver, the demo docs, the test-only subject fixture, crumbs).
git -C "$REPO" fetch -q origin main 2>/dev/null
MAIN=$(git -C "$REPO" rev-parse origin/main)
NONDEMO=$(git -C "$REPO" diff --name-only "$MAIN" HEAD | grep -vE "^(scripts/allen_e2e_demo\.sh|docs/campaigns/allen-e2e/|crates/aien-allen/tests/e2e_demo_subject\.rs$|(.*/)?\.crumb$)" | wc -l)
if git -C "$REPO" merge-base --is-ancestor "$MAIN" HEAD && [ "$NONDEMO" -eq 0 ]; then
  STACK="main at ${MAIN:0:12} plus demo-only files"
else
  STACK="candidate stack at ${COMMIT:0:12}, not main"
fi

# Build with the real compose library (no omega dir, no GPU lib).
(
  cd "$REPO" && env -u AIEN_OMEGA_DIR -u AIEN_OMEGA_COMPOSE_DIR -u AIEN_OMEGA_GPU_LIB -u AIEN_FORCE_CPU_STUB \
    AIEN_OMEGA_COMPOSE_LIB="$LIB" CARGO_TARGET_DIR="$D/target" cargo build --release -p aien-cli
) >"$D/build-run.log" 2>&1 || { echo "build failed, see $D/build-run.log" >&2; exit 2; }
grep -q "building the stub" "$D/build-run.log" && { echo "stub compose build; refusing" >&2; exit 2; }
BIN_SHA=$(sha256sum "$BIN" | cut -d' ' -f1)

# Fresh state. An older run directory is moved aside, never deleted.
[ -e "$RUN" ] && mv "$RUN" "$RUN.old-$(date +%s)"
mkdir -p "$RUN"/{state,compose,prov,ws,home,logs} "$ART"
: >"$RECEIPTS"
LOGS=$RUN/logs
WS=$RUN/ws

E=(env -u AIEN_OMEGA_DIR -u AIEN_OMEGA_COMPOSE_DIR -u AIEN_OMEGA_GPU_LIB -u AIEN_REQUIRE_BLACKWELL
   -u AIEN_GPU_BACKEND -u AIEN_MODEL_DIR -u AIEN_MODEL_PATH -u AIEN_TOKENIZER_PATH -u AIEN_FORCE_CPU_STUB
   -u AIEN_FAULT_HOLD -u AIEN_FAULT_HOLD_FILE -u AIEN_ALLEN_SUBJECT -u AIEN_ALLEN_ADOPT
   -u AIEN_COMPOSE_MAX_TOKENS -u AIEN_COMPOSE_DOC_MAX_TOKENS
   HOME="$RUN/home" NO_COLOR=1 AIEN_RUNTIME_SOCK="$SOCK" AIEN_RUNTIME_STATE_DIR="$RUN/state"
   AIEN_COMPOSE_DIR="$RUN/compose" AIEN_PROVENANCE_DIR="$RUN/prov" AIEN_REQUIRE_CHECKPOINT=1
   AIEN_COMPOSE_AUTHORIZE_REQUIRES_DESK=1 AIEN_COMPOSE_EDIT_BUDGET_MS=599000 AIEN_COMPOSE_DOC_BUDGET_MS=599000)
cli() { "${E[@]}" "$BIN" "$@"; }

# Weights digests. Single file: its sha256. Sharded: sha256 of the sorted
# "<sha256>  <name>" listing of every shard (the daemon itself digests only the
# shard file it is pointed at).
weights_digest() {
  local d=$1
  if [ -f "$d/model.safetensors" ]; then sha256sum "$d/model.safetensors" | cut -d' ' -f1
  else (cd "$d" && sha256sum -- *.safetensors | LC_ALL=C sort -k2 | sha256sum | cut -d' ' -f1); fi
}
WD_A=$(weights_digest "$MA"); WD_B=$(weights_digest "$MB")

# ---- receipts ----------------------------------------------------------------
CUR_DIR=none; CUR_WD=none; CUR_BACKEND="no daemon started"; CUR_MODELLINE=none
CUR_DSHA=none; CUR_TSHA=none; CUR_MAC="no daemon started"; ID0=none
rec() { # step status evidence [extra json]
  local extra=${4:-'{}'}
  jq -nc --arg step "$1" --arg status "$2" --arg ev "$3" --argjson extra "$extra" \
    --arg commit "$COMMIT" --arg bin "$BIN_SHA" --arg backend "$CUR_BACKEND" --arg modelline "$CUR_MODELLINE" \
    --arg mdir "$CUR_DIR" --arg wd "$CUR_WD" --arg dsha "$CUR_DSHA" --arg tsha "$CUR_TSHA" \
    --arg mac "$CUR_MAC" --arg id0 "$ID0" --arg stack "$STACK" --arg ver "$V" --arg utc "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    '{step:$step,status:$status,label:"real-CPU",stack:$stack,spec:("DEMO-"+$ver),evidence:$ev,
      repo_commit:$commit,daemon_sha256:$bin,cli_sha256:$bin,
      backend_line:$backend,model_line:$modelline,model_dir:$mdir,
      model_weights_digest:$wd,daemon_model_sha256:$dsha,tokenizer_sha256:$tsha,
      authorize_mac_line:$mac,allen_logical_agent_id:$id0,utc:$utc} + $extra' >>"$RECEIPTS"
  echo "[$(date -u +%H:%M:%S)] $1 $2: $3" | cut -c1-300
}

# ---- daemon control ----------------------------------------------------------
DN=0; DPID=""; ABORT=""
start_daemon() { # model_dir [VAR=val ...]; sets CUR_* from the daemon log
  local d=$1; shift
  local mp="$d/model.safetensors"; [ -f "$mp" ] || mp="$d/model-00001-of-00003.safetensors"
  DN=$((DN + 1)); DLOG=$LOGS/daemon-$DN.log; rm -f "$SOCK"
  "${E[@]}" AIEN_MODEL_PATH="$mp" AIEN_TOKENIZER_PATH="$d/tokenizer.json" "$@" "$BIN" daemon >"$DLOG" 2>&1 &
  DPID=$!
  local i
  for i in $(seq 1 600); do
    sleep 2
    kill -0 "$DPID" 2>/dev/null || { DPID=""; echo "daemon exited early: $(tail -c 400 "$DLOG")"; return 1; }
    if [ -S "$SOCK" ] && grep -q "Replay reconcile:" "$DLOG"; then break; fi
  done
  [ -S "$SOCK" ] || { echo "daemon not serving after 20 min"; return 1; }
  CUR_DIR=$d
  if [ "$d" = "$MA" ]; then CUR_WD=$WD_A; else CUR_WD=$WD_B; fi
  CUR_BACKEND=$(grep -m1 '^  Backend:' "$DLOG" | sed 's/^ *//')
  CUR_MODELLINE=$(grep -m1 '^  Model:' "$DLOG" | sed 's/^ *//')
  CUR_MAC=$(grep -m1 '^Authorize MAC:' "$DLOG")
  CUR_DSHA=$(sed -n 's/.*model_sha256=\([0-9a-f]\{64\}\).*/\1/p' "$DLOG" | head -1)
  CUR_TSHA=$(sed -n 's/.*tokenizer_sha256=\([0-9a-f]\{64\}\).*/\1/p' "$DLOG" | head -1)
  : "${CUR_DSHA:=none}" "${CUR_TSHA:=none}"
  case $CUR_BACKEND in *CPU-reference*) ;; *) echo "not the CPU reference backend: $CUR_BACKEND"; kill -9 "$DPID" 2>/dev/null; wait "$DPID" 2>/dev/null; DPID=""; return 1 ;; esac
  return 0
}
KILL_RC=""
kill9() {
  [ -n "$DPID" ] || { KILL_RC="no daemon"; return 1; }
  kill -9 "$DPID" 2>/dev/null; wait "$DPID" 2>/dev/null; KILL_RC=$?; DPID=""; rm -f "$SOCK"
  [ "$KILL_RC" -eq 137 ]
}
trap '[ -n "$DPID" ] && kill -9 "$DPID" 2>/dev/null' EXIT

# ---- snapshots used by S5-S7 -------------------------------------------------
snap() { # prefix: profile, memory inspect, goals, effects ledger, document
  local p=$1
  cli allen show | jq -S .result >"$ART/$p-profile.json"
  cli allen memory inspect --owner 1 | jq -S .result.result >"$ART/$p-memory.json"
  cli allen goals list --owner 1 | jq -S .result.result >"$ART/$p-goals.json"
  cli compose effects | jq -S .ledger >"$ART/$p-ledger.json"
  # Every daemon open adds 5 bookkeeping records of kind 65538 (same kind as record 1, no effect, no
  # authorization). So compare the host (effect-class) records, not the raw total.
  cli compose recall | jq -S ".recall.host|length" >"$ART/$p-records.json"
  cli compose recall | jq -S .recall.records_total >"$ART/$p-records-total.json"
}
same() { cmp -s "$ART/$1-$3.json" "$ART/$2-$3.json"; }

sha_of() { sha256sum "$1" | cut -d' ' -f1; }
skip() { [ -n "$ABORT" ] && { rec "$1" NOT_RUN "$ABORT"; return 0; }; return 1; }

# One model task: propose -> authorize (desk MAC) -> execute -> checker. $1 tag, $2 file, $3 goal
model_task() {
  local tag=$1 file=$2 goal=$3
  TASK_STATUS=FAIL; TASK_EV=""; TASK_COMMIT=0; TASK_DONE=0; TASK_JSON='{}'
  cli compose propose --goal "$goal" --workspace "$WS" --context work >"$ART/$tag-propose.json" 2>"$ART/$tag-propose.err"
  local rc=$?
  if [ "$(jq -r '.report.committed // false' "$ART/$tag-propose.json" 2>/dev/null)" != true ]; then
    TASK_EV="propose did not commit (exit $rc): $(jq -r '.report.proposer_error // .error // "no proposal"' "$ART/$tag-propose.json" 2>/dev/null | cut -c1-200); model attempts recorded verbatim in $tag-propose.json. No retry (spec)."
    return
  fi
  TASK_COMMIT=1
  local mem_ctx mem_n
  mem_ctx=$(jq -r '.report.memory.context' "$ART/$tag-propose.json"); mem_n=$(jq -r '.report.memory.items_included' "$ART/$tag-propose.json")
  cli compose authorize --report "$ART/$tag-propose.json" --workspace "$WS" --approver drake --desk 1 >"$ART/$tag-authorize.json" 2>"$ART/$tag-authorize.err"
  if [ "$(jq -r '.ok' "$ART/$tag-authorize.json" 2>/dev/null)" != true ]; then
    TASK_EV="authorize refused: $(jq -c . "$ART/$tag-authorize.json" | cut -c1-250)"; return
  fi
  local grant; grant=$(jq -r '.authorization.id' "$ART/$tag-authorize.json")
  cli compose execute --report "$ART/$tag-propose.json" --workspace "$WS" --authorization "$grant" >"$ART/$tag-execute.json" 2>"$ART/$tag-execute.err"
  local st; st=$(jq -r '.state // "none"' "$ART/$tag-execute.json")
  cli compose effects | jq -S . >"$ART/$tag-effects.json"
  cli compose recall >"$ART/$tag-recall.json"
  local failed; failed=$(check_doc "$WS/$file")
  [ -f "$WS/$file" ] && cp "$WS/$file" "$ART/$file"
  TASK_JSON=$(jq -nc --arg f "$file" --arg failed "$failed" --arg st "$st" --arg mc "$mem_ctx" --argjson mn "${mem_n:-0}" --argjson grant "$grant" \
    '{file:$f,constraints_failed:$failed,effect_state:$st,memory_context:$mc,items_included:$mn,grant_id:$grant}')
  [ "$st" = DONE ] && TASK_DONE=1
  if [ "$st" != DONE ]; then
    case $st in UNRESOLVED|UNCERTAIN) TASK_STATUS=UNRESOLVED ;; esac
    TASK_EV="effect state $st (not DONE); constraints failed: [${failed:-none}]"; return
  fi
  if [ "$mem_ctx" != work ] || [ "${mem_n:-0}" -lt 1 ]; then
    TASK_EV="MemoryReport context=$mem_ctx items_included=$mem_n (need work, >=1); constraints failed: [${failed:-none}]"; return
  fi
  if [ -n "$failed" ]; then
    TASK_EV="commit DONE, MemoryReport work/$mem_n, but document fails: $failed (document in artifacts-$V/$file, recorded verbatim)"; return
  fi
  TASK_STATUS=PASS
  TASK_EV="commit DONE, MemoryReport context=work items_included=$mem_n, C1-C5 hold, $(wc -w <"$WS/$file") words, sha256 $(sha_of "$WS/$file")"
}

# The link chain itself (v2): the commit, grant, intent and ack records of this task each name
# the same generation record and ALLEN agent, and that generation record's model_sha256 is the
# digest the daemon loaded. Read through recall, digest-verified. Sets LINK_MODEL, LINK_ID (0/1)
# and LINK_JSON. Stricter than the v1 text search, which only asked whether the values appear.
record_text() { cli compose recall --ids "$1" | jq -c 'if .recall.cited[0].verified == true then (.recall.cited[0].text|fromjson) else {unverified:true} end' 2>/dev/null || echo '{"unreadable":true}'; }
follow_link() { # tag file msha
  local tag=$1 file=$2 msha=$3 gen commit grant intent ack id r ok_m=1 ok_i=1 rows="[]"
  gen=$(jq -r '.report.generation_record // "none"' "$ART/$tag-propose.json")
  commit=$(jq -r '.report.compose_commit // "none"' "$ART/$tag-propose.json")
  grant=$(jq -r '.authorization.id // "none"' "$ART/$tag-authorize.json")
  intent=$(jq -r --arg f "$file" '[.ledger.intents[]|select(.path==$f)][0].intent // "none"' "$ART/$tag-effects.json")
  ack=$(jq -r --arg f "$file" '[.ledger.intents[]|select(.path==$f)][0].state_record // "none"' "$ART/$tag-effects.json")
  for id in "$commit" "$grant" "$intent" "$ack"; do
    case $id in ''|none|null) ok_m=0; ok_i=0; continue ;; esac
    r=$(record_text "$id")
    [ "$(printf '%s' "$r" | jq -r '.provenance.generation_record|tostring')" = "$gen" ] || ok_m=0
    [ "$(printf '%s' "$r" | jq -r '.provenance.allen_agent // "absent"')" = "$ID0" ] || ok_i=0
    rows=$(printf '%s' "$rows" | jq -c --argjson id "$id" --argjson r "$r" '. + [{record:$id,provenance:($r.provenance // null)}]')
  done
  local g gm="none"
  case $gen in ''|none|null) ok_m=0 ;; *) g=$(record_text "$gen"); gm=$(printf '%s' "$g" | jq -r '.model_sha256 // "none"'); [ "$gm" = "$msha" ] || ok_m=0 ;; esac
  LINK_MODEL=$ok_m; LINK_ID=$ok_i
  LINK_JSON=$(jq -nc --arg gen "$gen" --arg gm "$gm" --argjson rows "$rows" --arg c "$commit" --arg g "$grant" --arg i "$intent" --arg a "$ack" \
    '{generation_record:$gen,generation_record_model_sha256:$gm,chain:{compose_commit:$c,grant:$g,intent:$i,ack:$a},records:$rows}')
}

# Provenance links for one commit: which of the five items appear in what the daemon recorded.
# Sources: the effect ledger, every compose record text (recall), the task report, the authorize
# and execute answers, and the provenance receipts. NOT the daemon boot log (it is not a ledger).
provenance() { # tag file model_sha_expected
  local tag=$1 file=$2 msha=$3 ledger recall psha csha auth pool
  recall=$ART/$tag-recall.json; ledger=$ART/$tag-effects.json
  psha=$(jq -r '.report.proposal_sha256' "$ART/$tag-propose.json")
  csha=$(sha_of "$WS/$file")
  auth=$(jq -r '.authorization.id' "$ART/$tag-authorize.json")
  local l_grant l_prop l_content l_model l_id
  l_grant=$(jq -r --arg f "$file" --argjson a "$auth" '[.ledger.intents[]|select(.path==$f and .authorization==$a)]|length' "$ledger")
  l_content=$(jq -r --arg f "$file" --arg c "$csha" '[.ledger.intents[]|select(.path==$f and .content_sha256==$c)]|length' "$ledger")
  pool=$(cat "$ledger" "$recall" "$ART/$tag-propose.json" "$ART/$tag-authorize.json" "$ART/$tag-execute.json" "$RUN"/prov/*.json 2>/dev/null)
  l_prop=$(printf '%s' "$pool" | grep -c "$psha")
  l_model=$(printf '%s' "$pool" | grep -c "$msha")
  l_id=$(printf '%s' "$pool" | grep -c "$ID0")
  LINK_JSON=null
  if [ "$V" = v2 ]; then
    follow_link "$tag" "$file" "$msha"
    l_model=$LINK_MODEL; l_id=$LINK_ID
  fi
  PROV_JSON=$(jq -nc --argjson g "${l_grant:-0}" --argjson p "$l_prop" --argjson c "${l_content:-0}" --argjson m "$l_model" --argjson i "$l_id" \
    --arg auth "$auth" --arg psha "$psha" --arg csha "$csha" --arg msha "$msha" --argjson link "$LINK_JSON" \
    '{grant_id:$auth,proposal_sha256:$psha,content_sha256:$csha,model_digest_expected:$msha,link_chain:$link,
      ledger_links:{grant_id:($g>0),proposal_sha256:($p>0),content_sha256_equals_disk:($c>0),model_digest:($m>0),logical_agent_id:($i>0)}}')
  [ "${l_grant:-0}" -gt 0 ] && [ "$l_prop" -gt 0 ] && [ "${l_content:-0}" -gt 0 ] && [ "$l_model" -gt 0 ] && [ "$l_id" -gt 0 ]
}

# ============================== S0 ============================================
cli compose desk-key --create 1 >"$ART/S0-deskkey.json" 2>&1   # prints id and path, never the key
if start_daemon "$MA"; then
  ST_ALLEN=$(cli allen status | jq -r .result.identity)
  ALLEN_DIRS=$(ls -d "$RUN"/compose.allen-* "$RUN"/compose/*allen* 2>/dev/null | wc -l)
  if [[ $CUR_MAC == "Authorize MAC: on"* ]] && [ "$ST_ALLEN" = not_engaged ] && [ "$ALLEN_DIRS" -eq 0 ]; then
    rec S0 PASS "daemon up on M-A (CPU-reference), log: '$CUR_MAC', allen status identity=$ST_ALLEN, 0 ALLEN state directories before S1"
  else
    rec S0 FAIL "MAC line='$CUR_MAC' identity=$ST_ALLEN allen_dirs=$ALLEN_DIRS"
    ABORT="S0 failed, nothing further can be trusted"
  fi
else
  rec S0 FAIL "daemon did not start on M-A: $(tail -c 300 "$LOGS/daemon-$DN.log" 2>/dev/null | tr '\n' ' ')"
  ABORT="S0 failed: daemon did not start on M-A"
fi

# ============================== S1 ============================================
if ! skip S1; then
  # The compose home exists once the daemon opened it; its record 1 is the lineage a subject must bind.
  LINEAGE=$(cli compose recall --ids 1 | jq -r '.recall.cited[0].digest')
  kill9 || true
  SUBJ=$RUN/subject.bin
  FX=$(cd "$REPO" && env -u AIEN_OMEGA_DIR -u AIEN_OMEGA_COMPOSE_DIR AIEN_OMEGA_COMPOSE_LIB="$LIB" CARGO_TARGET_DIR="$D/target" \
        DEMO_SUBJECT_OUT="$SUBJ" DEMO_LINEAGE_HEX="$LINEAGE" DEMO_SUBJECT_NAME="allen-e2e-demo-$V" \
        cargo test --release -p aien-allen --test e2e_demo_subject -- --ignored --nocapture 2>&1)
  AGENT=$(printf '%s\n' "$FX" | sed -n 's/^AGENT=//p'); AROOT=$(printf '%s\n' "$FX" | sed -n 's/^ROOT=//p')
  if [ -z "$AGENT" ] || [ ! -s "$SUBJ" ]; then
    rec S1 FAIL "could not build the subject fixture: $(printf '%s' "$FX" | tail -c 300)"
    ABORT="S1 failed: no identity"
  elif start_daemon "$MA" AIEN_ALLEN_SUBJECT="$SUBJ" AIEN_ALLEN_ADOPT="$AGENT"; then
    ALOG=$(grep '^ALLEN: engaged' "$LOGS/daemon-$DN.log")
    STJ=$(cli allen status | jq -c .)
    FP=$(printf '%s' "$STJ" | jq -r .result.fingerprint)
    LOGID=$(printf '%s' "$ALOG" | sed -n 's/.*agent=\([0-9a-f]\{64\}\).*/\1/p')
    NLINES=$(printf '%s\n' "$ALOG" | grep -c .)
    if [ "$(printf '%s' "$STJ" | jq -r .result.identity)" = engaged ] && [ "$LOGID" = "$AGENT" ] && [ "$FP" = "${AGENT:0:8}" ] && [ "$NLINES" -eq 1 ]; then
      ID0=$AGENT
      rec S1 PASS "ID0=$ID0 (daemon log 'ALLEN: engaged agent=...', allen status fingerprint $FP); AgentRoot $AROOT; one identity" \
        "$(jq -nc --arg root "$AROOT" --arg lineage "$LINEAGE" '{agent_root:$root,cortex_lineage_record1:$lineage,subject_kind:"host-built fixture (test-only encoder), adopted once by AIEN_ALLEN_ADOPT; real subjects come only from AIENOS cs_provision (NOT_RUN)"}')"
    else
      rec S1 FAIL "status/log disagree: identity=$(printf '%s' "$STJ" | jq -r .result.identity) fp=$FP log=$LOGID lines=$NLINES"
      ABORT="S1 failed: identity not engaged"
    fi
  else
    rec S1 FAIL "daemon refused or did not start with the subject: $(tail -c 300 "$LOGS/daemon-$DN.log" | tr '\n' ' ')"
    ABORT="S1 failed: identity not engaged"
  fi
fi

# ============================== S2 ============================================
if ! skip S2; then
  cli allen set --expect 0 --plain-language 1 >"$ART/S2-set.json"
  cli allen memory put --context work --text "$NWORK" >"$ART/S2-put-work.json"
  cli allen memory put --context personal --text "$NPERS" >"$ART/S2-put-pers.json"
  cli allen goals add --context work --text "$G1" >"$ART/S2-goal.json"
  snap S2
  OK_PROF=$(jq -r --arg id "$ID0" '.profile.persona.plain_language==true and .profile.agent==$id and .revision==1' "$ART/S2-profile.json")
  OK_MEM=$(jq -r --arg a "$NWORK" --arg b "$NPERS" --arg g "$G1" \
    '(map(select(.state=="live"))|length)==3 and (map(select(.scope=="work" and .kind=="fact" and .text==$a))|length)==1 and (map(select(.scope=="personal" and .kind=="fact" and .text==$b))|length)==1 and (map(select(.scope=="work" and .kind=="goal" and .text==$g))|length)==1' "$ART/S2-memory.json")
  OK_GOALS=$(jq -r --arg g "$G1" '(.goals|length)==1 and .goals[0].text==$g and .goals[0].scope=="work" and .goals[0].state=="open"' "$ART/S2-goals.json")
  if [ "$OK_PROF" = true ] && [ "$OK_MEM" = true ] && [ "$OK_GOALS" = true ]; then
    rec S2 PASS "profile revision 1 plain-language=1 under ID0; memory inspect shows exactly N-work (work), N-pers (personal), G1 (work goal); goals list shows exactly G1"
  else
    rec S2 FAIL "profile_ok=$OK_PROF memory_ok=$OK_MEM goals_ok=$OK_GOALS (see artifacts S2-*.json)"
  fi
  NPERS_ITEM=$(jq -r --arg b "$NPERS" '.[]|select(.text==$b)|.item' "$ART/S2-memory.json")
fi

# ============================== S3 ============================================
S3_OK=0
if ! skip S3; then
  model_task S3 garden.md "$GOAL_A"
  rec S3 "$TASK_STATUS" "$TASK_EV" "$TASK_JSON"
  S3_OK=$TASK_DONE
fi

# ============================== S3-red ========================================
if ! skip S3-red; then
  printf '# Garden plan\n\nPlant carrots and beans along the south fence.\nNote: %s\n' "$CANARY" >"$ART/red-garden.md"
  RED=$(check_doc "$ART/red-garden.md")
  printf "# Garden plan\n\nPlant tomatoes in May.\n" >"$ART/good-control.md"; GOOD=$(check_doc "$ART/good-control.md")
  if [ "$RED" = "C3 C5" ]; then
    rec S3-red PASS "checker on the hand-written document (breaks C3 and C5) reports exactly: $RED; positive control (good document) reports: [${GOOD:-none}]" "$(jq -nc --arg r "$RED" '{checker_failed:$r}')"
  else
    rec S3-red FAIL "checker reported '$RED', expected exactly 'C3 C5'" "$(jq -nc --arg r "$RED" '{checker_failed:$r}')"
  fi
fi

# ============================== S4 ============================================
if ! skip S4; then
  if [ "$S3_OK" != 1 ]; then
    rec S4 NOT_RUN "S3 produced no commit, so there is no commit to trace"
  elif provenance S3 garden.md "$CUR_DSHA"; then
    rec S4 PASS "ledger links grant id, proposal sha256, content sha256 (= garden.md on disk), M-A model digest and ID0" "$PROV_JSON"
  else
    MISS=$(printf '%s' "$PROV_JSON" | jq -r '.ledger_links|to_entries|map(select(.value==false).key)|join(",")')
    rec S4 FAIL "missing links in what the daemon recorded for the S3 commit: $MISS. Present: the rest$([ "$V" = v1 ] && echo ". The model digest and ID0 appear in the daemon boot log and the ALLEN profile/memory stores, not in the compose or effect ledger records of the commit")" "$PROV_JSON"
  fi
fi

# ============================== S5 ============================================
if ! skip S5; then
  snap S5a
  [ -f "$WS/garden.md" ] && { G_SHA_BEFORE=$(sha_of "$WS/garden.md"); G_STAT_BEFORE=$(stat -c '%i %Y.%y' "$WS/garden.md"); } || { G_SHA_BEFORE=none; G_STAT_BEFORE=none; }
  if kill9; then KILLED="kill -9 (exit 137)"; else KILLED="kill rc=$KILL_RC"; fi
  if start_daemon "$MA" AIEN_ALLEN_SUBJECT="$RUN/subject.bin"; then
    snap S5b
    G_SHA_AFTER=none; G_STAT_AFTER=none
    [ -f "$WS/garden.md" ] && { G_SHA_AFTER=$(sha_of "$WS/garden.md"); G_STAT_AFTER=$(stat -c '%i %Y.%y' "$WS/garden.md"); }
    REPL=$(grep -m1 '^Replay reconcile:' "$LOGS/daemon-$DN.log")
    FPA=$(cli allen status | jq -r .result.fingerprint)
    ALOG2=$(grep -c "^ALLEN: engaged agent=$ID0" "$LOGS/daemon-$DN.log")
    CHK=""
    [ "$FPA" = "${ID0:0:8}" ] && [ "$ALOG2" = 1 ] || CHK="$CHK identity"
    same S5a S5b profile || CHK="$CHK profile"
    same S5a S5b memory || CHK="$CHK notes"
    same S5a S5b goals || CHK="$CHK goals"
    same S5a S5b ledger || CHK="$CHK ledger-changed(re-executed?)"
    same S5a S5b records || CHK="$CHK host-record-count"
    [ "$S3_OK" = 1 ] || CHK="$CHK no-S3-commit"
    [ "$(jq -r '[.intents[]|select(.path=="garden.md" and .state=="DONE")]|length' "$ART/S5b-ledger.json")" = 1 ] || CHK="$CHK S3-not-DONE"
    [ "$G_SHA_BEFORE" != none ] && [ "$G_SHA_BEFORE" = "$G_SHA_AFTER" ] && [ "$G_STAT_BEFORE" = "$G_STAT_AFTER" ] || CHK="$CHK garden.md"
    if [ -z "$CHK" ]; then
      rec S5 PASS "$KILLED then restart on M-A: ID0 same, profile/notes/G1 identical, S3 intent still DONE and ledger+record count unchanged (not re-executed), garden.md same sha256, inode and mtime; $REPL"
    elif [ "$S3_OK" != 1 ]; then
      rec S5 NOT_RUN "no S3 commit existed to check; restart itself: identity/profile/notes/goals problems=[${CHK# }]"
    else
      rec S5 FAIL "differences after restart:${CHK}; $REPL"
    fi
  else
    rec S5 FAIL "$KILLED; daemon did not restart on M-A: $(tail -c 300 "$LOGS/daemon-$DN.log" | tr '\n' ' ')"
    ABORT="S5: daemon would not restart"
  fi
fi

# ============================== S6 ============================================
if ! skip S6; then
  cli allen memory forget --context personal --item "${NPERS_ITEM:-none}" >"$ART/S6-forget.json"
  snap S6
  cli allen memory recall --context personal >"$ART/S6-recall-personal.json"
  LIVE=$(jq -r --arg b "$NPERS" '[.[]|select(.text==$b or .state=="live" and .scope=="personal")]|length' "$ART/S6-memory.json")
  SHOWN=$(jq -r --arg i "${NPERS_ITEM:-none}" '[.[]|select(.item==$i)|.state]|join(",")' "$ART/S6-memory.json")
  RITEMS=$(jq -r '.result.result.items|length' "$ART/S6-recall-personal.json")
  DISK=$(grep -rl "$NPERS" "$RUN/compose" "$RUN/compose.allen-memory" "$RUN/compose.allen-profile" "$RUN/state" "$RUN/prov" "$LOGS" 2>/dev/null | wc -l)
  if [ "$LIVE" -eq 0 ] && [ "$RITEMS" -eq 0 ] && [ "$(jq -r '.ok' "$ART/S6-forget.json")" = true ]; then
    rec S6 PASS "forgot N-pers: inspect no longer shows its text (its row remains as state '${SHOWN:-absent}' with no text), recall --context personal returns 0 items; files under daemon state still containing the canary text: $DISK" \
      "$(jq -nc --argjson d "$DISK" '{canary_files_remaining_in_daemon_state:$d}')"
  else
    rec S6 FAIL "after forget: live/visible=$LIVE recall_items=$RITEMS forget_ok=$(jq -r '.ok' "$ART/S6-forget.json")"
  fi
fi

# ============================== S7 ============================================
if ! skip S7; then
  OLD_BACKEND=$CUR_BACKEND; OLD_MODELLINE=$CUR_MODELLINE; OLD_DSHA=$CUR_DSHA; OLD_TSHA=$CUR_TSHA
  if kill9; then KILLED="kill -9 (exit 137)"; else KILLED="kill rc=$KILL_RC"; fi
  if start_daemon "$MB" AIEN_ALLEN_SUBJECT="$RUN/subject.bin"; then
    snap S7
    FPB=$(cli allen status | jq -r .result.fingerprint)
    CHK=""
    if [ "$V" = v2 ]; then
      # v2 change 2: the Model: line must change; the Backend: line is recorded, not required to change.
      [ "$CUR_MODELLINE" != "$OLD_MODELLINE" ] && [ "$CUR_MODELLINE" != none ] || CHK="$CHK model-line-identical"
    else
      [ "$CUR_BACKEND" != "$OLD_BACKEND" ] || CHK="$CHK backend-line-identical"
    fi
    [ "$CUR_DSHA" != "$OLD_DSHA" ] && [ "$CUR_DSHA" != none ] || CHK="$CHK model-digest-not-changed"
    [ "$FPB" = "${ID0:0:8}" ] || CHK="$CHK identity"
    same S6 S7 profile || CHK="$CHK profile"
    same S6 S7 memory || CHK="$CHK notes"
    same S6 S7 goals || CHK="$CHK goals"
    cli allen memory recall --context personal >"$ART/S7-recall-personal.json"
    [ "$(jq -r '.result.result.items|length' "$ART/S7-recall-personal.json")" = 0 ] || CHK="$CHK N-pers-visible"
    EXTRA=$(jq -nc --arg ob "$OLD_BACKEND" --arg nb "$CUR_BACKEND" --arg om "$OLD_MODELLINE" --arg nm "$CUR_MODELLINE" --arg od "$OLD_DSHA" --arg nd "$CUR_DSHA" \
      '{before:{backend_line:$ob,model_line:$om,daemon_model_sha256:$od},after:{backend_line:$nb,model_line:$nm,daemon_model_sha256:$nd}}')
    if [ -z "$CHK" ]; then
      rec S7 PASS "$KILLED then start on M-B: $([ "$V" = v2 ] && echo "Model: line" || echo "backend line") and model digest changed (${OLD_DSHA:0:12} -> ${CUR_DSHA:0:12}); ID0 same; profile, N-work, G1 unchanged; N-pers still absent" "$EXTRA"
    else
      rec S7 FAIL "criteria not met:${CHK}. Backend line before: '$OLD_BACKEND'; after: '$CUR_BACKEND'. Model line and digest changed: $([ "$CUR_DSHA" != "$OLD_DSHA" ] && echo yes || echo no)" "$EXTRA"
    fi
  else
    rec S7 FAIL "$KILLED; daemon did not start on M-B: $(tail -c 400 "$LOGS/daemon-$DN.log" | tr '\n' ' ')"
    ABORT="S7: M-B daemon did not start"
  fi
fi

# ============================== S8 ============================================
if ! skip S8; then
  model_task S8 garden-b.md "$GOAL_B"
  S8_STATUS=$TASK_STATUS; S8_EV=$TASK_EV
  if [ "$TASK_COMMIT" = 1 ] && [ -f "$WS/garden-b.md" ]; then
    if provenance S8 garden-b.md "$CUR_DSHA"; then
      PROV_NOTE="provenance links all present (incl. M-B digest and ID0)"
    else
      MISS=$(printf '%s' "$PROV_JSON" | jq -r '.ledger_links|to_entries|map(select(.value==false).key)|join(",")')
      PROV_NOTE="provenance links missing: $MISS (M-B digest expected in the provenance)"
      [ "$S8_STATUS" = PASS ] && S8_STATUS=FAIL
    fi
    S8_EV="$S8_EV; $PROV_NOTE"
    TASK_JSON=$(printf '%s' "$TASK_JSON" | jq -c --argjson p "$PROV_JSON" '. + {provenance:$p}')
  fi
  rec S8 "$S8_STATUS" "$S8_EV" "$TASK_JSON"
fi

[ -n "$DPID" ] && kill9
cp "$LOGS"/daemon-*.log "$ART/" 2>/dev/null
echo "receipts: $RECEIPTS"
