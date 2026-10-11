#!/usr/bin/env bash
# tests/run.sh: proves the C8 exec contract on this host. Run as drakestapleton (uses sudo -n). Exit 1 on any FAIL.
set -uo pipefail
X=/usr/local/sbin/aien-run-exec
RUN_HOME=/var/lib/aien-run
OP_HOME=/home/drakestapleton
pass=0; fail=0
ok()  { pass=$((pass+1)); printf 'PASS %s\n' "$1"; }
bad() { fail=$((fail+1)); printf 'FAIL %s: %s\n' "$1" "$2"; }
run() { sudo -n "$X" "$@"; }   # run [exec args]... -- cmd

uid=$(id -u aien-run)
[[ -n $uid ]] || { echo "no aien-run user"; exit 1; }

# 1 runs as uid aien-run
out=$(run --cwd /tmp --timeout 10 --profile host -- id -u 2>&1)
[[ $out == "$uid" ]] && ok "runs as uid aien-run ($uid)" || bad "uid" "$out"

# 2 env cleared: planted secret absent, no SUDO_* leaks, only the allowed names
out=$(SECRET_PLANT=hunter2 run --cwd /tmp --timeout 10 --profile host -- sh -c 'echo "${SECRET_PLANT:-unset}"; env | cut -d= -f1 | grep -v -x -E "PATH|HOME|CARGO_HOME|RUSTUP_HOME|PWD|SHLVL|_|OLDPWD" | tr "\n" ","' 2>&1)
[[ $out == "unset" ]] && ok "env cleared (planted secret absent, no extra names)" || bad "env" "$out"

# 3 --env pass-through and refused names
out=$(run --cwd /tmp --timeout 10 --profile host --env FOO=bar -- sh -c 'echo $FOO' 2>&1)
[[ $out == bar ]] && ok "--env FOO=bar passes" || bad "--env" "$out"
run --cwd /tmp --timeout 10 --profile host --env LD_PRELOAD=x -- true >/dev/null 2>&1 && bad "--env LD_PRELOAD" "accepted" || ok "--env LD_PRELOAD refused"

# 4 no network for host profile
out=$(run --cwd /tmp --timeout 20 --profile host -- sh -c 'curl -s -m 3 https://github.com >/dev/null 2>&1; echo curl=$?; getent hosts github.com >/dev/null 2>&1; echo getent=$?; ip -o link | cut -d: -f2 | tr -d " " | tr "\n" ","' 2>&1)
out=$(tr "\n" " " <<<"$out"); [[ $out == *"curl="[1-9]* && $out == *"getent="[1-9]* && $out == *"lo,"* && $out != *"enP"* && $out != *"tailscale"* ]] && ok "host profile has no network ($out)" || bad "network" "$out"

# 5 timeout -> 124
t0=$(date +%s); run --cwd /tmp --timeout 1 --profile host -- sleep 30 >/dev/null 2>&1; rc=$?; t1=$(date +%s)
[[ $rc -eq 124 && $((t1-t0)) -lt 15 ]] && ok "timeout returns 124 in $((t1-t0))s" || bad "timeout" "rc=$rc after $((t1-t0))s"

# 6 exit code passthrough
run --cwd /tmp --timeout 10 --profile host -- sh -c 'exit 7' >/dev/null 2>&1; rc=$?
[[ $rc -eq 7 ]] && ok "exit code passthrough (7)" || bad "exit passthrough" "rc=$rc"

# 7 cwd honored
out=$(run --cwd "$RUN_HOME" --timeout 10 --profile host -- pwd 2>&1)
[[ $out == "$RUN_HOME" ]] && ok "cwd honored" || bad "cwd" "$out"

# 8 memory cap enforced (64M cap, allocate 300M in a shell variable)
out=$(run --cwd /tmp --timeout 60 --profile host --memory-max 64M -- sh -c 'x=$(head -c 300000000 /dev/zero | tr "\0" a); echo survived' 2>&1); rc=$?
[[ $rc -ne 0 && $out != *survived* ]] && ok "memory cap enforced (rc=$rc)" || bad "memory cap" "rc=$rc out=$out"

# 9 devices: host cannot open the GPU, gb10 can
out=$(run --cwd /tmp --timeout 10 --profile host -- sh -c 'exec 3<>/dev/nvidiactl' 2>&1); rc=$?
[[ $rc -ne 0 ]] && ok "host profile cannot open /dev/nvidiactl" || bad "host device policy" "opened nvidiactl"
out=$(run --cwd /tmp --timeout 10 --profile gb10 -- sh -c 'exec 3<>/dev/nvidiactl && exec 4<>/dev/nvidia0 && exec 5<>/dev/nvidia-uvm && exec 6<>/dev/dri/renderD128 && echo opened' 2>&1)
[[ $out == opened ]] && ok "gb10 profile opens /dev/nvidiactl, nvidia0, nvidia-uvm, dri/renderD128" || bad "gb10 devices" "$out"

# 10 secrets of the trusted side are unreadable (direct sudo -u, as the inventory demands)
for f in "$OP_HOME/.aien/verify.key" "$OP_HOME/.config/gh/hosts.yml" "$OP_HOME/.ssh/id_ed25519"; do
  sudo -n -u aien-run cat "$f" >/dev/null 2>&1 && bad "secret $f" "READABLE" || ok "denied: $f"
done
sudo -n -u aien-run ls "$OP_HOME/workspace" >/dev/null 2>&1 && bad "workspace" "listable" || ok "denied: $OP_HOME/workspace (0700)"

# 11 toolchain as aien-run
out=$(run --cwd /tmp --timeout 60 --profile host -- cargo --version 2>&1)
[[ $out == cargo\ 1.98.1* ]] && ok "cargo as aien-run: $out" || bad "cargo" "$out"

# 12 crash recovery: kill -9 the running child; exit non-zero, no strays in the scope
sudo -n "$X" --cwd /tmp --timeout 120 --profile host -- sh -c 'sleep 300 & exec sleep 200' >/dev/null 2>&1 &
bg=$!
sleep 2
victim=$(pgrep -u aien-run -f '^sleep 200$' | head -1)
[[ -n $victim ]] && sudo -n kill -9 "$victim"
wait $bg; rc=$?
sleep 1
strays=$(pgrep -u aien-run -f '^sleep 300$' | wc -l)
scopes=$(systemctl list-units --no-legend --plain 'aien-run-exec-*.scope' 2>/dev/null | wc -l)
[[ $rc -ne 0 && $strays -eq 0 && $scopes -eq 0 ]] && ok "crash recovery: rc=$rc, strays=0, scopes=0" || bad "crash recovery" "rc=$rc strays=$strays scopes=$scopes"

# 13 worktree protection (if a worktree exists)
wt=$(ls -d "$RUN_HOME"/wt/*/*/ 2>/dev/null | head -1)
if [[ -n $wt ]]; then
  out=$(run --cwd "$wt" --timeout 30 --profile host -- sh -c 'git rev-parse --short HEAD >/dev/null && echo git-ok; rm -f .git 2>/dev/null && echo REMOVED; m=$(sed "s|gitdir: ||;s|/worktrees/.*||" .git); touch "$m/refs/x" 2>/dev/null && echo MIRROR-WRITABLE; echo t > .probe && rm .probe && echo tree-writable' 2>&1 | tr '\n' ' ')
  [[ $out == *git-ok* && $out != *REMOVED* && $out != *MIRROR-WRITABLE* && $out == *tree-writable* ]] && ok "worktree: git reads, .git and mirror protected, tree writable" || bad "worktree" "$out"
fi

printf '\n%d passed, %d failed\n' "$pass" "$fail"
[[ $fail -eq 0 ]]
