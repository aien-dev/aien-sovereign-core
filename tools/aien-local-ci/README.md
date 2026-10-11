# aien-local-ci: host sandbox for the sovereign local pipeline (lane INFRA)

Owner: lane INFRA of the sovereign local pipeline mission (inventory and contracts in
`~/handoffs/2026-10-11-sovereign-pipeline-M1-inventory.md`, contracts C3, C7, C8, A2, A3).
Everything here is bash. No Python. Host systemd units are used only for transient scopes; nothing here touches
AIENOS.

## Files

| file | role |
|---|---|
| `aien-run-exec` | source of truth for `/usr/local/sbin/aien-run-exec`, the C8 exec contract |
| `install.sh` | idempotent host install (user, dirs, caches, rustup proxies, exec script, sudoers) |
| `wt.sh` | trusted-side mirror + worktree management under `/var/lib/aien-run` |
| `forgejo-webhook.sh` | push trigger: Forgejo post-receive hook that writes the worker trigger file (A3) |
| `health.sh` | one-screen health (Forgejo, user, caches, worktrees, scopes, disk, GPU lock, quiet flag) |
| `gc.sh` | worktree age + target-dir quota cleanup, dry run by default |
| `tests/run.sh` | proofs of the contract on this host (18 checks) |

## C8 exec contract

```
sudo -n /usr/local/sbin/aien-run-exec --cwd DIR --timeout SECS --profile host|qemu|gb10 [--env K=V]... -- PROGRAM [ARGS...]
```

Chain (all on the privileged side, then drop): `systemd-run --scope` under `aien-run.slice` with `MemoryMax`,
`MemorySwapMax=0`, `TasksMax`, `CPUQuota`, `DevicePolicy` -> `unshare -n` (host|qemu) -> `prlimit --core=0
--nofile=65536` -> `timeout --signal=TERM --kill-after=10 SECS` (host|qemu) -> `setpriv --reuid aien-run --regid
aien-run --init-groups --reset-env --no-new-privs` -> `env -i PATH HOME CARGO_HOME RUSTUP_HOME [K=V...] PROGRAM`.
After PROGRAM exits the scope is killed and stopped, so a crashed check leaves no strays. Exit code passes through
(124 timeout, 137 killed/OOM). Unprivileged user namespaces are refused on this kernel, which is why the namespace
is created before dropping privileges.

| profile | memory | tasks | cpu | network | devices | timeout |
|---|---|---|---|---|---|---|
| host | 48G | 4096 | 1600% (16 cores) | none (fresh netns, only `lo`) | closed (null, zero, random, tty, ptmx) | yes |
| qemu | 48G | 4096 | 1600% | none | closed + `/dev/kvm` | yes |
| gb10 | 96G | 4096 | 1800% | host network | all (`/dev/nvidia*` are 0666; `/dev/dri/*` via groups video, render) | NO kill |

Optional flags `--memory-max`, `--tasks-max`, `--cpu-quota` override the profile defaults (tests use `--memory-max 64M`).
`--env` refuses `PATH`, `HOME`, `CARGO_HOME`, `RUSTUP_HOME`, `LD_*`, `BASH_ENV`, `ENV`.
Sandbox PATH is `/var/lib/aien-run/cargo/bin:/usr/local/bin:/usr/bin:/bin`; trusted copies of extra tools a gate needs
(for example `crumb`) belong in `/usr/local/bin`, never in the operator's home.

## Egress decision

No nftables rule for uid aien-run. host and qemu checks run in a fresh network namespace with no interfaces, which is
stronger than a packet filter (no DNS, no loopback). gb10 keeps the host network per the contract; a later nft rule
for uid aien-run would be the only way to close that and is deliberately not added by this lane.

## Layout under /var/lib/aien-run (home of aien-run, 0750 aien-run:drakestapleton)

| path | owner / mode | purpose |
|---|---|---|
| `cargo/` | aien-run 0755 | CARGO_HOME (`.package-cache` lock lives here) |
| `cargo/registry`, `cargo/git` | root, read-only | seeded once from `~/.cargo` (1.9 GB + 1.3 GB; the git cache is needed because `Cargo.lock` has 3 git sources) |
| `cargo/bin` | root | the operator's `rustup` binary + proxy symlinks; no second rustup install |
| `rustup/` | root, read-only | `settings.toml` (default 1.98.1) + `toolchains/1.98.1-aarch64-unknown-linux-gnu` copied (631 MB) |
| `.gitconfig` | root 0644 | `safe.directory = *` (git 2.43 has no path globs; aien-run reaches nothing else) |
| `mirror/<repo>.git` | drakestapleton 0755 | bare mirror of the trusted clone, refreshed by `wt.sh sync`; unwritable by aien-run |
| `wt/<repo>/<sha>` | root dir drakestapleton:aien-run 1775, `.git` root 0644, files aien-run | checkouts; aien-run can add files, cannot replace `.git` or write the mirror |
| `target/<repo>/{pr,main}` | aien-run | per trust class (A2); `main` is wiped after FAIL/timeout by the engine |

## Trigger file contract (A3)

`~/.local/state/aien-local-ci/trigger/<repo>.<sha>` with one line `<ref> <old> <new> <unix-time>`, written by the
Forgejo post-receive hook (`forgejo-webhook.sh install <repo>`), consumed and deleted by `aien-test worker`.
forgejo-runner stays off (its user unit is disabled); Forgejo is git hosting + PR UI + status display only.

## Operate

```
tools/aien-local-ci/install.sh               # idempotent
tools/aien-local-ci/tests/run.sh             # 18 proofs
tools/aien-local-ci/wt.sh sync aien-sovereign-core ~/workspace/aien-sovereign-core
tools/aien-local-ci/wt.sh add aien-sovereign-core <sha>
tools/aien-local-ci/health.sh
tools/aien-local-ci/gc.sh [--apply] [--days 3] [--target-quota-gb 120]
```

Measured 2026-10-11 (sovereign-core 5ef7720, 20 cores, caches seeded): cold `cargo build --workspace --all-targets
--locked --offline` as aien-run 65.0 s (target dir 27 GB); warm rebuild after touching one crate 2.2 s.
