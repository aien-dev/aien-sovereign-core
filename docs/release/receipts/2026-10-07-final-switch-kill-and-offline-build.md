# Receipt: kill -9 at the final install switch, and offline build (2026-10-07)

Tree: branch c3/release-prep from main 12c1a5f.

## Kill -9 at the final switch
install.sh switches `releases/current` with one rename(2) of a temp symlink (`mv -T`). That step is atomic; no bug found.
New hold points (env `AIEN_TEST_PAUSE_AT`): `pre-rename` (temp link made, rename not done), `after-links`, and `after-swap` inside `--rollback`.
`scripts/test-install-release.sh` case 12b kills -9 at each, in a scratch prefix only:
- install killed at pre-rename: wholly old, runs, record unchanged; rerun gives wholly new, no stray temp link.
- install killed at after-links: wholly new, runs; rerun repairs the record.
- rollback killed at pre-rename: wholly old; rerun completes.
- rollback killed at after-swap: wholly switched; rerun leaves live release and record in agreement.
Result: PASS (about 3 s). Still NOT_RUN: real power loss / filesystem-level crash (kill -9 does not test unflushed data).
Small change: set_current now removes leftover `.current.new.*` temp links.

## Offline build
`scripts/test-offline-build.sh`: offline build from a warm cargo cache (cargo fetch --locked, then --offline --locked). Not vendored.
Result: PASS, 170 s, crates aien-cli spark-cockpit-rs spark-inquisitor cortex-encoder-rs cortex-rs spark-supervisor spark-debugger.
Network denial NOT enforced: `unshare -rn` fails here (uid_map write not permitted); only `--offline` was used.
Not added to CI: needs network for the fetch and takes about 3 minutes.
