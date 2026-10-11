#!/usr/bin/env bash
# install.sh: idempotent host setup for the sovereign local pipeline sandbox (lane INFRA, contracts C3/C8/A2).
# Run as drakestapleton; uses sudo -n inside. Safe to re-run: every step checks before it acts.
#
# What it sets up on the host:
#   * system user aien-run (home /var/lib/aien-run 0750 aien-run:drakestapleton, nologin, groups video+render)
#   * /var/lib/aien-run/{cargo,target,wt,mirror,rustup} with the ownership described in README.md
#   * cargo registry + git cache seeded from ~/.cargo (root-owned, read-only), toolchain copied from ~/.rustup
#     (root-owned, read-only), rustup proxies in /var/lib/aien-run/cargo/bin (no second rustup install)
#   * /usr/local/sbin/aien-run-exec (root 0755) and /etc/sudoers.d/aien-local-ci (validated with visudo -c)
#   * ~/.local/state/aien-local-ci/{trigger,evidence,bundles} for the trusted side
set -euo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
RUN_USER=aien-run
RUN_HOME=/var/lib/aien-run
OPERATOR=${AIEN_OPERATOR:-drakestapleton}
OP_HOME=$(getent passwd "$OPERATOR" | cut -d: -f6)
TOOLCHAIN=${AIEN_TOOLCHAIN:-1.98.1-aarch64-unknown-linux-gnu}
STATE="$OP_HOME/.local/state/aien-local-ci"

say() { printf '[install] %s\n' "$*"; }
S() { sudo -n "$@"; }

[[ $(id -un) == "$OPERATOR" ]] || { echo "run as $OPERATOR" >&2; exit 1; }
sudo -n true 2>/dev/null || { echo "passwordless sudo required" >&2; exit 1; }

# 1. user and groups (device groups: /dev/dri/card0 is root:video, /dev/dri/renderD128 is root:render,
#    /dev/nvidia* are 0666 root:root so they need no group).
if ! getent passwd "$RUN_USER" >/dev/null; then
  S useradd --system --home-dir "$RUN_HOME" --create-home --shell /usr/sbin/nologin --user-group \
    --groups video,render "$RUN_USER"
  say "created user $RUN_USER"
else
  say "user $RUN_USER exists"
fi
for g in video render; do
  id -nG "$RUN_USER" | tr ' ' '\n' | grep -qx "$g" || S usermod -aG "$g" "$RUN_USER"
done
S chown "$RUN_USER:$OPERATOR" "$RUN_HOME"
S chmod 0750 "$RUN_HOME"

# 2. directory layout
S install -d -o "$RUN_USER" -g "$RUN_USER" -m 0755 "$RUN_HOME/cargo" "$RUN_HOME/target"
S install -d -o "$OPERATOR" -g "$OPERATOR" -m 0755 "$RUN_HOME/wt" "$RUN_HOME/mirror"
S install -d -o root -g root -m 0755 "$RUN_HOME/rustup" "$RUN_HOME/rustup/toolchains" "$RUN_HOME/rustup/update-hashes" "$RUN_HOME/rustup/downloads" "$RUN_HOME/rustup/tmp"
install -d -m 0755 "$STATE/trigger" "$STATE/evidence" "$STATE/bundles"

# 3. caches: registry + git cache copied once from the operator's ~/.cargo, then root-owned read-only.
seed_ro() { # seed_ro SRC DST
  local src=$1 dst=$2
  if [[ ! -e $dst ]]; then
    [[ -d $src ]] || { say "skip $dst (no $src)"; return; }
    S rm -rf "$dst.tmp"
    S cp -a "$src" "$dst.tmp"
    S mv "$dst.tmp" "$dst"
    say "seeded $dst from $src"
  fi
  S chown -R root:root "$dst"
  S chmod -R a+rX,u-w,go-w "$dst"
}
seed_ro "$OP_HOME/.cargo/registry" "$RUN_HOME/cargo/registry"
seed_ro "$OP_HOME/.cargo/git" "$RUN_HOME/cargo/git"
seed_ro "$OP_HOME/.rustup/toolchains/$TOOLCHAIN" "$RUN_HOME/rustup/toolchains/$TOOLCHAIN"

# 4. rustup: the operator's rustup binary acts as the proxy (cargo/rustc/... are symlinks to it), with
#    RUSTUP_HOME=/var/lib/aien-run/rustup holding only settings.toml + the copied toolchain. No rustup-init,
#    no downloads: rust-toolchain.toml pins resolve against the copied toolchain only.
S install -d -o root -g root -m 0755 "$RUN_HOME/cargo/bin"
S install -o root -g root -m 0755 "$OP_HOME/.cargo/bin/rustup" "$RUN_HOME/cargo/bin/rustup"
for p in cargo rustc rustdoc rustfmt cargo-fmt cargo-clippy clippy-driver rust-gdb rust-lldb; do
  S ln -sfn rustup "$RUN_HOME/cargo/bin/$p"
done
printf 'version = "12"\ndefault_toolchain = "%s"\nprofile = "minimal"\n\n[overrides]\n' "$TOOLCHAIN" > /tmp/aien-rustup-settings.$$
S install -o root -g root -m 0644 /tmp/aien-rustup-settings.$$ "$RUN_HOME/rustup/settings.toml"
rm -f /tmp/aien-rustup-settings.$$
S chown root:root "$RUN_HOME/rustup"; S chmod 0755 "$RUN_HOME/rustup"

# 4b. git safe.directory for aien-run: worktree roots are operator-owned (sticky), so git must be told they are safe.
#     git 2.43 has no path globs for safe.directory, so it is "*"; aien-run can only reach /var/lib/aien-run anyway.
printf "[safe]\n\tdirectory = *\n" > /tmp/aien-gitconfig.$$
S install -o root -g root -m 0644 /tmp/aien-gitconfig.$$ "$RUN_HOME/.gitconfig"
rm -f /tmp/aien-gitconfig.$$

# 5. exec contract + sudoers
S install -o root -g root -m 0755 "$here/aien-run-exec" /usr/local/sbin/aien-run-exec
printf '%s ALL=(root) NOPASSWD: /usr/local/sbin/aien-run-exec\n' "$OPERATOR" > /tmp/aien-sudoers.$$
S visudo -cf /tmp/aien-sudoers.$$ >/dev/null
S install -o root -g root -m 0440 /tmp/aien-sudoers.$$ /etc/sudoers.d/aien-local-ci
rm -f /tmp/aien-sudoers.$$
S visudo -c >/dev/null && say "sudoers OK"

say "done. verify with: $here/tests/run.sh"
