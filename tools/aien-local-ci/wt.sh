#!/usr/bin/env bash
# wt.sh: trusted-side worktree management for aien-run checks (A2: worktrees under /var/lib/aien-run/wt,
# the clone unwritable by aien-run, the worktree's .git link unwritable by aien-run).
#
#   wt.sh sync   <repo> <trusted-clone-dir>   create/refresh the bare mirror /var/lib/aien-run/mirror/<repo>.git
#   wt.sh add    <repo> <sha>                 check out <sha> at /var/lib/aien-run/wt/<repo>/<sha> for aien-run
#   wt.sh remove <repo> <sha>                 remove that worktree
#   wt.sh list                                list worktrees with age
#
# Why a mirror: the operator's clones live under ~/workspace (0700), which aien-run must not read. The mirror is a
# bare copy owned by the operator (0755, world-readable, not writable by aien-run); the worktree's .git file points
# into the mirror, so aien-run can read objects (git log/rev-parse work) but cannot write refs, objects or the index.
# Ownership of a worktree after `add`: root dir operator:aien-run mode 1775 (sticky: aien-run may create files but
# cannot remove or replace entries it does not own), `.git` file root:root 0644, everything else aien-run:aien-run.
set -euo pipefail
RUN_USER=aien-run
BASE=/var/lib/aien-run
MIRROR="$BASE/mirror"
WT="$BASE/wt"
OPERATOR=$(id -un)

die() { printf 'wt.sh: %s\n' "$*" >&2; exit 2; }
cmd=${1:-}; shift || true
case "$cmd" in
  sync)
    repo=${1:?repo}; clone=${2:?trusted-clone-dir}
    [[ $repo =~ ^[A-Za-z0-9._-]+$ ]] || die "bad repo name"
    git -C "$clone" rev-parse --git-dir >/dev/null || die "not a git dir: $clone"
    m="$MIRROR/$repo.git"
    if [[ ! -d $m ]]; then
      git init --bare --quiet "$m"
      git -C "$m" config core.sharedRepository 0644
    fi
    git -C "$m" fetch --quiet --prune "$clone" '+refs/heads/*:refs/heads/*' '+refs/remotes/*:refs/remotes/*' '+refs/tags/*:refs/tags/*'
    chmod -R o+rX,o-w "$m"
    printf 'mirror %s: %s refs\n' "$m" "$(git -C "$m" for-each-ref | wc -l)"
    ;;
  add)
    repo=${1:?repo}; sha=${2:?sha}
    [[ $repo =~ ^[A-Za-z0-9._-]+$ && $sha =~ ^[0-9a-f]{7,40}$ ]] || die "bad repo or sha"
    m="$MIRROR/$repo.git"; [[ -d $m ]] || die "no mirror for $repo (run: wt.sh sync $repo <clone>)"
    full=$(git -C "$m" rev-parse --verify "$sha^{commit}") || die "unknown commit $sha"
    d="$WT/$repo/$full"
    [[ ! -e $d ]] || die "exists: $d"
    install -d -m 0755 "$WT/$repo"
    git -C "$m" worktree add --quiet --detach "$d" "$full"
    sudo -n chown -R "$RUN_USER:$RUN_USER" "$d"
    sudo -n chown root:root "$d/.git"; sudo -n chmod 0644 "$d/.git"
    sudo -n chown "$OPERATOR:$RUN_USER" "$d"; sudo -n chmod 1775 "$d"
    printf '%s\n' "$d"
    ;;
  remove)
    repo=${1:?repo}; sha=${2:?sha}
    [[ $repo =~ ^[A-Za-z0-9._-]+$ && $sha =~ ^[0-9a-f]{7,40}$ ]] || die "bad repo or sha"
    m="$MIRROR/$repo.git"
    d=$(ls -d "$WT/$repo/$sha"* 2>/dev/null | head -1); [[ -n $d ]] || die "no worktree for $sha"
    sudo -n rm -rf "$d"
    git -C "$m" worktree prune
    printf 'removed %s\n' "$d"
    ;;
  list)
    for d in "$WT"/*/*/; do
      [[ -d $d ]] || continue
      printf '%s  %s days\n' "$d" "$(( ( $(date +%s) - $(stat -c %Y "$d") ) / 86400 ))"
    done
    ;;
  *) sed -n '2,8p' "$0" >&2; exit 2 ;;
esac
