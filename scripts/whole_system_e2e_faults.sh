#!/usr/bin/env bash
# WHOLE-SYSTEM-E2E held-fault rows (aien-sovereign-core#379, lane L2 of aien-architecture#190).
# Rows and predictions: docs/campaigns/whole-system-e2e/FAULTS-v1.md (committed before this script; never edited by it).
#
#   scripts/whole_system_e2e_faults.sh run        build a fault-hold CLI, run the rows, write one run folder
#
# Shell plus jq. No Python, no systemd, no accelerator: every daemon must print the CPU-reference backend line.
# Needs CLAUDE_JOB_DIR (build target and state under $CLAUDE_JOB_DIR/tmp/e2e-faults), the prebuilt compose archive
# at $CLAUDE_JOB_DIR/tmp/librx_compose.a, jq, sha256sum, hostname. Model: WSE2E_MODEL_DIR (default the CAND-4 snapshot).
# Idempotent: it rebuilds its own job dir from scratch each run (rows/, fixture/ are removed first), and it only
# ever signals the daemon and executor processes it started itself (pids kept in DPID and EPID).
set -u
[ "${1:-}" = run ] || { echo "usage: $0 run" >&2; exit 2; }

REPO=$(cd "$(dirname "$0")/.." && pwd)
JOB=${CLAUDE_JOB_DIR:?CLAUDE_JOB_DIR must be set}
LIB=$JOB/tmp/librx_compose.a
D=$JOB/tmp/e2e-faults
BIN=$D/target/release/aien-cli
MDIR=${WSE2E_MODEL_DIR:-$(ls -d "$HOME"/.cache/huggingface/hub/models--unsloth--Llama-3.2-1B-Instruct/snapshots/*/ 2>/dev/null | head -1)}
UTC0=$(date -u +%Y%m%dT%H%M%SZ)
OUT=${WSE2E_OUT:-$REPO/docs/campaigns/whole-system-e2e/runs/FAULTS-$UTC0}
for t in jq sha256sum hostname; do command -v $t >/dev/null || { echo "need $t" >&2; exit 2; }; done
[ -f "$LIB" ] || { echo "missing $LIB" >&2; exit 2; }
[ -n "$MDIR" ] && [ -f "$MDIR/model.safetensors" ] && [ -f "$MDIR/tokenizer.json" ] || { echo "model dir '$MDIR' lacks model.safetensors or tokenizer.json" >&2; exit 2; }
QUIET=$HOME/workspace/.spark-quiet
while [ -s "$QUIET" ]; do echo "quiet flag set; waiting"; sleep 30; done
DIRTY=$(git -C "$REPO" status --porcelain -- . ":!docs/campaigns/whole-system-e2e/runs" | wc -l)
[ "$DIRTY" -eq 0 ] || [ -n "${WSE2E_ALLOW_DIRTY:-}" ] || { echo "working tree has $DIRTY uncommitted changes; commit them first" >&2; exit 2; }
COMMIT=$(git -C "$REPO" rev-parse HEAD)
mkdir -p "$D"
( cd "$REPO" && env -u AIEN_OMEGA_DIR -u AIEN_OMEGA_COMPOSE_DIR -u AIEN_OMEGA_GPU_LIB -u AIEN_FORCE_CPU_STUB \
    AIEN_OMEGA_COMPOSE_LIB="$LIB" CARGO_TARGET_DIR="$D/target" cargo build --release -p aien-cli --features aien-cli/fault-hold ) >"$D/build.log" 2>&1 \
  || { echo "build failed, see $D/build.log" >&2; exit 2; }
grep -q "building the stub" "$D/build.log" && { echo "stub compose build; refusing" >&2; exit 2; }
BIN_SHA=$(sha256sum "$BIN" | cut -d' ' -f1)
for p in AIEN_FAULT_HOLD reconcile_panic reconcile_error; do grep -aqF "$p" "$BIN" || { echo "binary lacks the $p hold; refusing" >&2; exit 2; }; done
W_SHA=$(sha256sum "$(readlink -f "$MDIR/model.safetensors")" | cut -d' ' -f1)
T_SHA=$(sha256sum "$(readlink -f "$MDIR/tokenizer.json")" | cut -d' ' -f1)
C_W=$(sed -n 's/^model-safetensors-sha256 = "\(.*\)"/\1/p' "$REPO/release/candidate.toml")
C_T=$(sed -n 's/^tokenizer-json-sha256 = "\(.*\)"/\1/p' "$REPO/release/candidate.toml")
C_ID=$(sed -n 's/^model-id = "\(.*\)"/\1/p' "$REPO/release/candidate.toml")
HOST=$(hostname)

rm -rf "$D/rows" "$D/fixture"; mkdir -p "$D/rows" "$D/fixture" "$OUT/chain" "$OUT/artifacts"
cp "$REPO/docs/campaigns/whole-system-e2e/FAULTS-v1.md" "$OUT/FAULTS-v1.as-run.md"

sha_of() { sha256sum "$1" | cut -d' ' -f1; }
tstat() { stat -c '%i %s %.9Y' "$1" 2>/dev/null || echo absent; }
utc() { date -u +%Y-%m-%dT%H:%M:%SZ; }

OBJ6='Write a file named done.md in the outbox folder containing one line: finished.'
OBJ_ID=$(printf 'AIEN_E2E_FAULTS_OBJECTIVE_V1\n%s' "$OBJ6" | sha256sum | cut -d' ' -f1)
RUN_ID=$(printf '%s\n%s\n%s\n%s' "$OBJ_ID" "$BIN_SHA" "$W_SHA" "$HOST" | sha256sum | cut -d' ' -f1)

# ---- the receipt chain (same format as scripts/whole_system_e2e.sh) -----------------------------------------
N=0; PREV=$(printf '0%.0s' $(seq 1 64)); CHAIN=$OUT/chain; ROWS=""; CUR_BACKEND=""
rcpt() { # step status evidence [fields json]
  N=$((N + 1)); local f; f=$(printf '%s/%03d-%s.json' "$CHAIN" "$N" "$1")
  jq -n --arg oid "$OBJ_ID" --arg rid "$RUN_ID" --arg step "$1" --arg status "$2" --arg ev "$3" --argjson fields "${4:-{\}}" \
    --arg utc "$(utc)" --arg prev "$PREV" --arg commit "$COMMIT" --arg bin "$BIN_SHA" --arg w "$W_SHA" --arg t "$T_SHA" \
    --arg backend "${CUR_BACKEND:-no daemon started}" --arg host "$HOST" \
    '{objective_id:$oid, run_id:$rid, step:$step, status:$status, utc:$utc, prev_receipt_sha256:$prev, evidence:$ev,
      repo_commit:$commit, daemon_sha256:$bin, cli_sha256:$bin, model_weights_digest:$w, tokenizer_sha256:$t,
      backend_line:$backend, host:$host} + $fields' >"$f"
  PREV=$(sha_of "$f")
  case $1 in CTRL-*|E[0-9]*|FAULT-*) ROWS="$ROWS$1 $2"$'\n' ;; esac
  echo "[$(date -u +%H:%M:%S)] $1 $2: $3" | cut -c1-400 | tee -a "$OUT/console.log"
}

# ---- per-row environment (same variables as the E2E harness) -------------------------------------------------
R=""; WS=""; SOCK=""; ART=""; E=(); REPORT=""
mkenv() { # row dir R already exists
  SOCK=$R/s; WS=$R/ws
  E=(env -u AIEN_OMEGA_DIR -u AIEN_OMEGA_COMPOSE_DIR -u AIEN_OMEGA_GPU_LIB -u AIEN_REQUIRE_BLACKWELL
     -u AIEN_GPU_BACKEND -u AIEN_MODEL_DIR -u AIEN_MODEL_PATH -u AIEN_TOKENIZER_PATH -u AIEN_FORCE_CPU_STUB
     -u AIEN_FAULT_HOLD -u AIEN_FAULT_HOLD_FILE -u AIEN_ALLEN_SUBJECT -u AIEN_ALLEN_ADOPT
     -u AIEN_COMPOSE_MAX_TOKENS -u AIEN_COMPOSE_DOC_MAX_TOKENS
     HOME="$R/home" NO_COLOR=1 AIEN_RUNTIME_SOCK="$SOCK" AIEN_RUNTIME_STATE_DIR="$R/state"
     AIEN_COMPOSE_DIR="$R/compose" AIEN_PROVENANCE_DIR="$R/prov" AIEN_REQUIRE_CHECKPOINT=1
     AIEN_COMPOSE_AUTHORIZE_REQUIRES_DESK=1 AIEN_COMPOSE_EDIT_BUDGET_MS=599000 AIEN_COMPOSE_DOC_BUDGET_MS=599000)
}
cli() { "${E[@]}" "$BIN" "$@"; }

# ---- daemon and executor control (only our own pids) ---------------------------------------------------------
DPID=""; EPID=""; DRC=""; DLOG=/dev/null; DN=0; KILL_RC=""; EKILL_RC=""; ERC=""
start_daemon() { # [VAR=val ...]; sets DPID, CUR_BACKEND; on early exit sets DRC
  DN=$((DN + 1)); DLOG=$R/logs/daemon-$DN.log; DRC=""; rm -f "$SOCK"
  "${E[@]}" AIEN_MODEL_PATH="$MDIR/model.safetensors" AIEN_TOKENIZER_PATH="$MDIR/tokenizer.json" "$@" "$BIN" daemon >"$DLOG" 2>&1 &
  DPID=$!
  local i
  for i in $(seq 1 600); do
    sleep 2
    if [ ! -d "/proc/$DPID" ]; then wait "$DPID" 2>/dev/null; DRC=$?; DPID=""; echo "daemon $DN exited early (rc $DRC): $(tail -c 300 "$DLOG" | tr '\n' ' ')"; return 1; fi
    if [ -S "$SOCK" ] && grep -q -E "Replay reconcile:|^Reconcile:" "$DLOG"; then break; fi
  done
  [ -S "$SOCK" ] || { echo "daemon $DN not serving after 20 min"; return 1; }
  CUR_BACKEND=$(grep -m1 '^  Backend:' "$DLOG" | sed 's/^ *//')
  case $CUR_BACKEND in *CPU-reference*) ;; *) echo "not the CPU reference backend: '$CUR_BACKEND'"; kill -9 "$DPID" 2>/dev/null; wait "$DPID" 2>/dev/null; DPID=""; return 1 ;; esac
  return 0
}
reconcile_line() { grep -m1 '^Reconcile:' "$DLOG" | cut -c1-300; }
stop_graceful() { [ -n "$DPID" ] || return 0; cli compose shutdown >"$R/logs/shutdown-$DN.json" 2>&1; local i=0; while [ -d "/proc/$DPID" ] && [ $i -lt 600 ]; do sleep 0.1; i=$((i + 1)); done; wait "$DPID" 2>/dev/null; DPID=""; rm -f "$SOCK"; }
kill_daemon() { [ -n "$DPID" ] || return 1; kill -9 "$DPID" 2>/dev/null; wait "$DPID" 2>/dev/null; KILL_RC=$?; DPID=""; rm -f "$SOCK"; [ "$KILL_RC" -eq 137 ]; }
kill_executor() { [ -n "$EPID" ] || return 1; kill -9 "$EPID" 2>/dev/null; wait "$EPID" 2>/dev/null; EKILL_RC=$?; EPID=""; rm -f "$R/hold"; [ "$EKILL_RC" -eq 137 ]; }
trap '[ -n "$DPID" ] && kill -9 "$DPID" 2>/dev/null; [ -n "$EPID" ] && kill -9 "$EPID" 2>/dev/null' EXIT

# ---- steps ---------------------------------------------------------------------------------------------------
authorize() { # tag -> grant id on stdout (empty if refused)
  cli compose authorize --report "$REPORT" --workspace "$WS" --approver drake --desk 1 >"$ART/$1.json" 2>"$ART/$1.err"
  [ "$(jq -r '.ok' "$ART/$1.json" 2>/dev/null)" = true ] && jq -r '.authorization.id' "$ART/$1.json"
}
T_STATE=none; T_REF=""; T_TEXT=""
execute() { # tag grant: sets T_STATE, T_REF (refusal name), T_TEXT
  cli compose execute --report "$REPORT" --workspace "$WS" --authorization "$2" >"$ART/$1.json" 2>"$ART/$1.err"
  T_STATE=$(jq -r '.state // "none"' "$ART/$1.json" 2>/dev/null); [ -n "$T_STATE" ] || T_STATE=none
  T_TEXT=$(jq -r '.error // ""' "$ART/$1.json" 2>/dev/null | head -c 220 | tr '\n' ' ')
  [ -n "$T_TEXT" ] || T_TEXT=$(head -c 220 "$ART/$1.err" | tr '\n' ' ')
  T_REF=$(printf '%s' "$T_TEXT" | sed -n 's/^EFFECT_REFUSED \([A-Za-z]*\):.*/\1/p')
  [ -n "$T_REF" ] || T_REF=$(printf '%s' "$T_TEXT" | grep -o 'E_[A-Z_]*' | head -1)
  [ "$T_STATE" = DONE ]
}
hold_execute() { # point tag grant: start the executor held at point; true when the hold was reached
  rm -f "$R/hold"
  "${E[@]}" AIEN_FAULT_HOLD="$1" AIEN_FAULT_HOLD_FILE="$R/hold" "$BIN" compose execute --report "$REPORT" --workspace "$WS" --authorization "$3" \
    >"$ART/$2.json" 2>"$ART/$2.err" &
  EPID=$!
  local i=0
  while [ ! -s "$R/hold" ] && kill -0 "$EPID" 2>/dev/null && [ $i -lt 1200 ]; do sleep 0.05; i=$((i + 1)); done
  [ -s "$R/hold" ]
}
release_executor() { rm -f "$R/hold"; wait "$EPID" 2>/dev/null; ERC=$?; EPID=""; }
count_effects() { find "$WS" -name done.md -type f 2>/dev/null | wc -l; }
effect_file() { find "$WS" -name done.md -type f 2>/dev/null | head -1; }
ledger() { cli compose effects >"$ART/$1.json" 2>/dev/null; }
intent_state() { jq -r "[.ledger.intents[] | select(.authorization == $2)] | .[0].state // \"none\"" "$ART/$1.json" 2>/dev/null; }
intent_count() { jq -r "[.ledger.intents[] | select(.authorization == $2)] | length" "$ART/$1.json" 2>/dev/null; }

row_open() { # name: fresh row dir from the fixture, daemon 1 up, grant G minted
  RN=$1; R=$D/rows/$RN; ART=$OUT/artifacts/$RN; DN=0
  rm -rf "$R"; mkdir -p "$R"/{state,prov,logs} "$ART"
  cp -a "$D/fixture/compose" "$D/fixture/home" "$D/fixture/ws" "$R/"
  cp -a "$D/fixture/compose.machine-root" "$R/"; [ -e "$D/fixture/compose.cortex-mark" ] && cp -a "$D/fixture/compose.cortex-mark" "$R/"
  cp "$D/fixture/propose.json" "$ART/propose.json"; REPORT=$ART/propose.json
  mkenv
  start_daemon || return 1
  G=$(authorize authorize-1); [ -n "$G" ] || { echo "authorize refused: $(head -c 200 "$ART/authorize-1.json")"; return 2; }
  return 0
}
row_close() { stop_graceful; cp "$R"/logs/daemon-*.log "$ART/" 2>/dev/null; }
fields() { # kill_point restarts eb ea dup grant extra-json
  jq -nc --arg kp "$1" --argjson rs "$2" --argjson eb "$3" --argjson ea "$4" --argjson dup "$5" --argjson g "${6:-null}" --argjson x "${7:-{\}}" \
    '{kill_point:$kp, restart_count:$rs, effects_before_kill:$eb, effects_after_restart:$ea, duplicates:$dup, grant_id:$g} + $x'
}
dupflag() { # stat_first stat_last count -> 0/1
  if [ "$3" -le 1 ] && [ "$1" = "$2" ]; then echo 0; else echo 1; fi
}

# ---- fixture: one real propose --------------------------------------------------------------------------------
echo "run folder: $OUT" | tee "$OUT/console.log"
R=$D/fixture; ART=$OUT/artifacts/fixture; mkdir -p "$R"/{state,prov,logs,home,compose,ws/inbox,ws/outbox} "$ART"; mkenv
printf 'Harbour notes. Three boats came in before noon.\n' >"$WS/inbox/harbour-notes.txt"
cli compose desk-key --create 1 >"$ART/deskkey.json" 2>&1   # prints id and path, never the key
ABORT=""
if start_daemon; then
  PROP_OK=0
  for a in 1 2; do
    cli compose propose --goal "$OBJ6" --workspace "$WS" --context work >"$ART/propose.json" 2>"$ART/propose.err"
    [ "$(jq -r '.report.committed // false' "$ART/propose.json" 2>/dev/null)" = true ] && { PROP_OK=1; break; }
  done
  stop_graceful; cp "$R"/logs/daemon-*.log "$ART/" 2>/dev/null
  if [ $PROP_OK = 1 ]; then
    cp "$ART/propose.json" "$D/fixture/propose.json"
    PRE_ST=PASS; [ "$W_SHA" = "$C_W" ] && [ "$T_SHA" = "$C_T" ] || PRE_ST=FAIL
    rcpt PRE $PRE_ST "fault-hold build $BIN_SHA; backend '$CUR_BACKEND'; fixture proposal committed (path $(jq -r '.report.proposal_path // "?"' "$ART/propose.json")); model $C_ID" \
      "$(jq -nc --arg cw "$C_W" --arg ct "$C_T" --arg cid "$C_ID" --arg mdir "$MDIR" --arg q "$([ -s "$QUIET" ] && echo held || echo not-held)" --arg f "cargo feature aien-cli/fault-hold" --arg ok "$PRE_ST" \
        '{candidate_model_id:$cid, candidate_weights_sha256:$cw, candidate_tokenizer_sha256:$ct, model_dir:$mdir, quiet_flag:$q, build_feature:$f, weights_match_candidate:($ok=="PASS")}')"
  else
    rcpt PRE FAIL "fixture proposal did not commit: $(jq -r '.report.proposer_error // .error // "no proposal"' "$ART/propose.json" 2>/dev/null | cut -c1-200)" '{}'; ABORT=1
  fi
else
  rcpt PRE FAIL "fixture daemon did not start: $(tail -c 300 "$DLOG" 2>/dev/null | tr '\n' ' ')" '{}'; ABORT=1
fi

# ---- E6-before_intent ----------------------------------------------------------------------------------------
if [ -z "$ABORT" ]; then
  KP=before_intent
  if row_open E6-before_intent && hold_execute $KP held "$G"; then
    kill_daemon; DK=$KILL_RC; EB=$(count_effects); release_executor; XRC=$ERC
    XOK=$(jq -r '.ok // "none"' "$ART/held.json" 2>/dev/null); AFTER_REL=$(count_effects)
    if start_daemon; then
      RL=$(reconcile_line); ledger L1; IS1=$(intent_state L1 "$G")
      execute again "$G"; ST1=$T_STATE; F=$(effect_file); S1=$(tstat "${F:-/nonexistent}")
      execute third "$G"; THIRD=$T_REF; S2=$(tstat "${F:-/nonexistent}"); EA=$(count_effects); ledger L2
      DUP=$(dupflag "$S1" "$S2" "$EA")
      if [ "$DK" -eq 137 ] && [ "$XOK" = false ] && [ "$EB" -eq 0 ] && [ "$AFTER_REL" -eq 0 ] && [ "$IS1" = none ] && [ "$ST1" = DONE ] && [ "$EA" -eq 1 ] && [ "$THIRD" = AlreadySpent ] && [ "$DUP" -eq 0 ]; then ST=PASS; else ST=FAIL; fi
      rcpt E6-before_intent $ST "daemon SIGKILL rc $DK while the executor held at $KP; executor released: ok=$XOK rc=$XRC, files $AFTER_REL; restart ($RL); intent before retry: $IS1; same grant: $ST1; third execute: ${THIRD:-none}; files $EA; duplicates $DUP" \
        "$(fields $KP 1 "$EB" "$EA" "$DUP" "$G" "$(jq -nc --arg ok "$XOK" --arg th "${THIRD:-}" --arg is "$IS1" --arg rl "$RL" '{executor_ok_after_release:$ok, intent_after_restart_before_retry:$is, third_execute_refusal:$th, restart_reconcile_line:$rl}')")"
    else rcpt E6-before_intent FAIL "daemon did not restart: $(tail -c 300 "$DLOG" | tr '\n' ' ')" "$(fields $KP 1 "$EB" 0 0 "$G")"; fi
  else
    kill_executor; rcpt E6-before_intent FAIL "setup failed (daemon, authorize or hold not reached)" "$(fields $KP 0 0 0 0 null)"
  fi
  row_close
fi

# ---- E6-after_intent -----------------------------------------------------------------------------------------
if [ -z "$ABORT" ]; then
  KP=after_intent
  if row_open E6-after_intent && hold_execute $KP held "$G"; then
    kill_daemon; DK=$KILL_RC; kill_executor; EK=$EKILL_RC; EB=$(count_effects)
    if start_daemon; then
      RL=$(reconcile_line); ledger L1; IS1=$(intent_state L1 "$G")
      execute r1 "$G"; R1_ST=$T_STATE; R1=$T_REF; E1=$(count_effects)
      OUTCOME=unknown; RECON=""; NEWGRANT=no
      if [ "$R1_ST" = DONE ]; then OUTCOME=DONE_without_refusal
      elif [ "$R1" = ReconciliationRequired ]; then
        cli compose reconcile >"$ART/reconcile.json" 2>&1; RECON=$(jq -r '.reconcile.outcomes[0].state // .error // "none"' "$ART/reconcile.json" 2>/dev/null | cut -c1-120)
        execute r2 "$G"; R2=$T_REF; E2=$(count_effects)
        if [ "$T_STATE" = DONE ] && [ "$E2" -eq 1 ]; then OUTCOME="a_ReconciliationRequired_reconcile_${RECON}_then_DONE"
        elif [ "$R2" = AlreadySpent ] && [ "$E2" -eq 1 ]; then OUTCOME="b_ReconciliationRequired_reconcile_${RECON}_then_AlreadySpent_file_once"
        elif [ "$R2" = AlreadySpent ] && [ "$E2" -eq 0 ]; then OUTCOME="c_ReconciliationRequired_reconcile_${RECON}_then_AlreadySpent_file_absent"; NEWGRANT=need
        fi
      elif [ "$R1" = AlreadySpent ]; then
        if [ "$E1" -eq 1 ]; then OUTCOME=b_AlreadySpent_file_once
        elif [ "$E1" -eq 0 ]; then OUTCOME=c_AlreadySpent_file_absent_reconcile_at_start; NEWGRANT=need; fi
      else OUTCOME="unexpected_refusal_${R1:-none}"; fi
      F=$(effect_file); S1=$(tstat "${F:-/nonexistent}")
      if [ $NEWGRANT = need ]; then
        G2=$(authorize authorize-2)
        if [ -n "$G2" ] && execute new "$G2"; then NEWGRANT=DONE; else NEWGRANT=failed; fi
        F=$(effect_file); S1=$(tstat "${F:-/nonexistent}"); execute newagain "${G2:-0}"
      fi
      EA=$(count_effects); S2=$(tstat "${F:-/nonexistent}"); ledger L2
      DUP=$(dupflag "$S1" "$S2" "$EA")
      case $OUTCOME in a_*|b_*|c_*) NAMED=1 ;; *) NAMED=0 ;; esac
      OKC=1; [ "$NEWGRANT" = failed ] && OKC=0; [ "$NEWGRANT" = need ] && OKC=0
      if [ "$DK" -eq 137 ] && [ "$EK" -eq 137 ] && [ "$EB" -eq 0 ] && [ $NAMED = 1 ] && [ $OKC = 1 ] && [ "$EA" -eq 1 ] && [ "$DUP" -eq 0 ]; then ST=PASS; else ST=FAIL; fi
      rcpt E6-after_intent $ST "daemon and executor SIGKILL (rc $DK, $EK) at $KP, files $EB; restart ($RL); intent state $IS1; same grant: state $R1_ST refusal '${R1:-none}'; outcome $OUTCOME; new grant: $NEWGRANT; files at end $EA; duplicates $DUP" \
        "$(fields $KP 1 "$EB" "$EA" "$DUP" "$G" "$(jq -nc --arg o "$OUTCOME" --arg r1 "$R1" --arg is "$IS1" --arg rc "$RECON" --arg ng "$NEWGRANT" --arg rl "$RL" '{outcome:$o, first_retry_refusal:$r1, intent_after_restart:$is, reconcile_state:$rc, new_grant_result:$ng, restart_reconcile_line:$rl}')")"
    else rcpt E6-after_intent FAIL "daemon did not restart: $(tail -c 300 "$DLOG" | tr '\n' ' ')" "$(fields $KP 1 "$EB" 0 0 "$G")"; fi
  else
    kill_executor; rcpt E6-after_intent FAIL "setup failed (daemon, authorize or hold not reached)" "$(fields $KP 0 0 0 0 null)"
  fi
  row_close
fi

# ---- E6-after_write and CTRL-E6 / CTRL-E6b (the controls reuse the after_write crash) ------------------------
crash_after_write() { # name: row_open, hold after_write, kill both; sets G, EB, F, SW, DK, EK
  row_open "$1" || return 1
  hold_execute after_write held "$G" || return 1
  kill_daemon; DK=$KILL_RC; kill_executor; EK=$EKILL_RC
  EB=$(count_effects); F=$(effect_file); SW=$(tstat "${F:-/nonexistent}")
  return 0
}
if [ -z "$ABORT" ]; then
  KP=after_write
  if crash_after_write E6-after_write; then
    if start_daemon; then
      RL=$(reconcile_line); ledger L1; IS1=$(intent_state L1 "$G")
      execute again "$G"; AG=$T_REF; ST1=$T_STATE; EA=$(count_effects); S2=$(tstat "$F"); ledger L2; IS2=$(intent_state L2 "$G"); NI=$(intent_count L2 "$G")
      DUP=$(dupflag "$SW" "$S2" "$EA")
      if [ "$DK" -eq 137 ] && [ "$EK" -eq 137 ] && [ "$EB" -eq 1 ] && [ "$IS2" = DONE ] && [ "$NI" -eq 1 ] && [ "$ST1" != DONE ] && [ "$AG" = AlreadySpent ] && [ "$EA" -eq 1 ] && [ "$DUP" -eq 0 ]; then ST=PASS; else ST=FAIL; fi
      rcpt E6-after_write $ST "daemon and executor SIGKILL (rc $DK, $EK) at $KP, files $EB; restart ($RL); intent after restart $IS1; same grant: ${AG:-none} (state $ST1); intent final $IS2 (count $NI); files $EA; duplicates $DUP" \
        "$(fields $KP 1 "$EB" "$EA" "$DUP" "$G" "$(jq -nc --arg r "$AG" --arg is "$IS2" --argjson ni "$NI" --arg rl "$RL" '{same_grant_refusal:$r, intent_state_final:$is, intents_for_grant:$ni, restart_reconcile_line:$rl}')")"
    else rcpt E6-after_write FAIL "daemon did not restart: $(tail -c 300 "$DLOG" | tr '\n' ' ')" "$(fields $KP 1 "$EB" 0 0 "$G")"; fi
  else
    kill_executor; rcpt E6-after_write FAIL "setup failed (daemon, authorize or hold not reached)" "$(fields $KP 0 0 0 0 null)"
  fi
  row_close
fi

ctrl_row() { # step rowname remove_mark(0/1)
  local step=$1 rn=$2 rm_mark=$3 KP=after_write
  if crash_after_write "$rn"; then
    mv "$R/compose" "$R/compose.removed"; mkdir -p "$R/compose"
    if [ "$rm_mark" = 1 ]; then mv "$R/compose.cortex-mark" "$R/compose.cortex-mark.removed" 2>/dev/null; fi
    START="served"; REFTEXT=""
    if start_daemon; then
      RL=$(reconcile_line)
      execute ctl "$G"; REFTEXT=$T_TEXT; REFN=$T_REF; CST=$T_STATE
      ledger L1
    else START="refused to start (rc ${DRC:-?})"; REFTEXT=$(tail -c 300 "$DLOG" | tr '\n' ' '); REFN=$(printf '%s' "$REFTEXT" | grep -o 'E_[A-Z_]*\|ReconciliationRequired\|ReconcileFailed' | head -1); CST=none; RL=""; fi
    EA=$(count_effects); S2=$(tstat "${F:-/nonexistent}"); DUP=$(dupflag "$SW" "$S2" "$EA")
    if [ "$DK" -eq 137 ] && [ "$EK" -eq 137 ] && [ "$EB" -eq 1 ] && [ "$CST" != DONE ] && [ -n "$REFN" ] && [ "$EA" -eq 1 ] && [ "$DUP" -eq 0 ]; then ST=PASS; else ST=FAIL; fi
    rcpt "$step" $ST "after $KP kill, durable state removed (compose home replaced by an empty folder; record mark removed=$rm_mark); restart: $START ($RL); execute same grant: state $CST, refusal name '${REFN:-none}', text: $REFTEXT; files $EA; duplicates $DUP" \
      "$(fields $KP 1 "$EB" "$EA" "$DUP" "$G" "$(jq -nc --arg s "$START" --arg n "${REFN:-}" --arg t "$REFTEXT" --argjson rm "$rm_mark" --arg cs "$CST" '{daemon_start:$s, refusal_name:$n, refusal_text:$t, record_mark_removed:($rm==1), execute_state:$cs}')")"
  else
    kill_executor; rcpt "$step" FAIL "setup failed (daemon, authorize or hold not reached)" "$(fields $KP 0 0 0 0 null)"
  fi
  row_close
}
if [ -z "$ABORT" ]; then
  ctrl_row CTRL-E6 CTRL-E6 0
  ctrl_row CTRL-E6b CTRL-E6b 1
fi

# ---- start-up holds: reconcile_error, reconcile_panic --------------------------------------------------------
if [ -z "$ABORT" ]; then
  KP=reconcile_error
  if row_open FAULT-reconcile_error; then
    stop_graceful   # the grant is in the durable home; restart with the hold
    if start_daemon AIEN_FAULT_HOLD=reconcile_error; then
      RL=$(reconcile_line); ledger L1; IS0=$(intent_state L1 "$G")
      execute e1 "$G"; R1=$T_REF; E1=$(count_effects); ledger L2; IS1=$(intent_state L2 "$G")
      cli compose reconcile >"$ART/reconcile.json" 2>&1; RC_OK=$(jq -r '.ok // false' "$ART/reconcile.json" 2>/dev/null)
      execute e2 "$G"; ST2=$T_STATE; EA=$(count_effects); F=$(effect_file); S1=$(tstat "${F:-/nonexistent}")
      execute e3 "$G"; R3=$T_REF; S2=$(tstat "${F:-/nonexistent}"); DUP=$(dupflag "$S1" "$S2" "$EA")
      case $RL in "Reconcile: refused: fault hold reconcile_error"*) LINE_OK=1 ;; *) LINE_OK=0 ;; esac
      if [ $LINE_OK = 1 ] && [ "$R1" = ReconcileFailed ] && [ "$E1" -eq 0 ] && [ "$IS1" = none ] && [ "$RC_OK" = true ] && [ "$ST2" = DONE ] && [ "$EA" -eq 1 ] && [ "$R3" = AlreadySpent ] && [ "$DUP" -eq 0 ]; then ST=PASS; else ST=FAIL; fi
      rcpt FAULT-reconcile_error $ST "daemon restarted with the $KP hold: '$RL'; execute refused '${R1:-none}', files $E1, intent $IS1; reconcile ok=$RC_OK; same grant then $ST2; files $EA; next execute '${R3:-none}'; duplicates $DUP" \
        "$(fields $KP 1 0 "$EA" "$DUP" "$G" "$(jq -nc --arg r1 "$R1" --arg rl "$RL" --arg r3 "$R3" '{first_execute_refusal:$r1, restart_reconcile_line:$rl, third_execute_refusal:$r3}')")"
    else rcpt FAULT-reconcile_error FAIL "daemon with the hold did not serve: $(tail -c 300 "$DLOG" | tr '\n' ' ')" "$(fields $KP 1 0 0 0 "$G")"; fi
  else rcpt FAULT-reconcile_error FAIL "setup failed (daemon or authorize)" "$(fields $KP 0 0 0 0 null)"; fi
  row_close

  KP=reconcile_panic
  if row_open FAULT-reconcile_panic; then
    stop_graceful
    PANIC_UP=no
    if start_daemon AIEN_FAULT_HOLD=reconcile_panic; then PANIC_UP=yes; fi
    PRC=${DRC:-none}; LOG_NAMES=$(grep -c 'fault hold reconcile_panic' "$DLOG")
    if [ $PANIC_UP = yes ]; then kill_daemon; fi
    execute p1 "$G"; P1=$T_TEXT; E1=$(count_effects)
    rm -f "$SOCK"
    if start_daemon; then
      RL=$(reconcile_line); ledger L1; IS1=$(intent_state L1 "$G")
      execute p2 "$G"; ST2=$T_STATE; EA=$(count_effects); F=$(effect_file); S1=$(tstat "${F:-/nonexistent}")
      execute p3 "$G"; R3=$T_REF; S2=$(tstat "${F:-/nonexistent}"); DUP=$(dupflag "$S1" "$S2" "$EA")
      if [ $PANIC_UP = no ] && [ "$PRC" != none ] && [ "$PRC" != 0 ] && [ "$LOG_NAMES" -ge 1 ] && [ -n "$P1" ] && [ "$E1" -eq 0 ] && [ "$IS1" = none ] && [ "$ST2" = DONE ] && [ "$EA" -eq 1 ] && [ "$R3" = AlreadySpent ] && [ "$DUP" -eq 0 ]; then ST=PASS; else ST=FAIL; fi
      rcpt FAULT-reconcile_panic $ST "daemon with the $KP hold: served=$PANIC_UP, exit rc $PRC, log names the hold ($LOG_NAMES lines); execute while nothing serves: '$P1', files $E1; clean restart ($RL); intent $IS1; same grant then $ST2; files $EA; next execute '${R3:-none}'; duplicates $DUP" \
        "$(fields $KP 1 0 "$EA" "$DUP" "$G" "$(jq -nc --arg up "$PANIC_UP" --arg rc "$PRC" --arg p1 "$P1" --arg r3 "$R3" '{daemon_served_with_hold:$up, daemon_exit_rc:$rc, execute_while_down:$p1, third_execute_refusal:$r3}')")"
    else rcpt FAULT-reconcile_panic FAIL "clean restart failed: $(tail -c 300 "$DLOG" | tr '\n' ' ')" "$(fields $KP 1 0 0 0 "$G")"; fi
  else rcpt FAULT-reconcile_panic FAIL "setup failed (daemon or authorize)" "$(fields $KP 0 0 0 0 null)"; fi
  row_close
fi

# ---- manifest ------------------------------------------------------------------------------------------------
printf 'objective_id %s\n%s' "$OBJ_ID" "$ROWS" >"$OUT/VERDICT_ROWS.txt"
( cd "$OUT" && find . -type f ! -name SHA256SUMS -print0 | LC_ALL=C sort -z | xargs -0 sha256sum >chain/SHA256SUMS )
echo "run folder written: $OUT ($(ls "$CHAIN" | wc -l) chain files)"; cat "$OUT/VERDICT_ROWS.txt"
