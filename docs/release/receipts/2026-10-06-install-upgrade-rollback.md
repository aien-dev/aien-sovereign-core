# Receipt: install, execute, upgrade, rollback, 2026-10-06 (NEXT-PHASE-4)

Machine: NVIDIA DGX Spark (aarch64, Linux 7.0.0-1019-nvidia), host CPU only. No model was loaded and
no GPU work was run (see step 2 for how `--version` was kept off nvidia-smi). Repository: aien-sovereign-core
main at 80e071a (the CAND-3 sovereign-core pin). Nothing was written outside a scratch prefix; `$HOME` for the
installer was an empty temporary directory, no `sudo`, no PATH profile edits (`AIEN_INSTALL_NO_PROFILE=1`).

## Candidate (CAND-3 pins, frozen manifest qualification/candidates/CAND-3.toml)

- sovereign-core 80e071a3ef700dbe31b1b081a957eebdcbf5ffc3
- omega f816473df391bc4fbcf99df722edd70ed26ebef1 (checked out at that commit, `AIEN_OMEGA_DIR`)
- physics 6d7cf0d4d8eb2cda7b512100ff6058e25dbb3ddf (equals omega `physics.lock`, `AIEN_PHYSICS_DIR`)
- toolchain: rust-toolchain.toml, 1.98.1; fresh `CARGO_HOME`, `cargo fetch --locked`; `mojo` not on PATH
- manifest digest `aien-cli-native-release` = 3a17ee79a238ad223fd59f4436d98906a0144b8d5ac47809da194a9ac1d493db

Signing note (limit of this receipt): the private half of the AIEN release key is not available to this job
and none was created or imported. The archives were signed with a throwaway key made inside the scratch prefix
(`ssh-keygen -t ed25519`, comment np4-throwaway) and `AIEN_ALLOWED_SIGNERS` pointed at its public line, the same
mechanism `scripts/test-install-release.sh` uses. So the signature-and-checksum path ran for real, but against a
throwaway key, not the pinned release key. NOT_RUN: verification of a release signed by the pinned key. The
refusal side was run: the same install with no signer override (pinned key from install.sh) printed
"SHA256SUMS.txt signature is NOT valid. Nothing was installed." and created no bin directory.

## 1. INSTALL: PASS

Build (log np4-logs/build1.log):

```
cargo fetch --locked      (CARGO_HOME = fresh directory in the prefix)
scripts/repro-build.sh -p aien-cli
scripts/repro-build.sh -p spark-cockpit-rs -p spark-inquisitor -p cortex-encoder-rs -p cortex-rs -p spark-supervisor -p spark-debugger -p aien-cli
scripts/repro-build.sh -p aien-cli     (again, last, see finding F1)
scripts/package-release.sh <rel1>/sovereign-linux-aarch64.tar.gz
(sign SHA256SUMS.txt with the throwaway key, namespace aien-release)
AIEN_RELEASE_TAG=v0.0.0-np4 AIEN_RELEASE_BASE_URL=file://<rel1> AIEN_BIN_DIR=... AIEN_CONFIG_DIR=... \
  AIEN_ALLOWED_SIGNERS=... AIEN_INSTALL_NO_PROFILE=1 bash <standalone copy of install.sh>   (env -i, empty HOME)
```

Results:

- install.sh: "Release mode without a source tree", "Signature and checksum verified", 7 binaries installed
  (aien, cortex, cortex-encoder-rs, spark-cockpit, spark-debugger, spark-inquisitor, spark-supervisor), config and
  EN2 imprint written to the config directory. Empty HOME stayed empty.
- archive sha256 e7411c7f9c0a4147... (archives are not byte-reproducible, tar stores mtimes; the binaries inside are)
- installed `aien` sha256 3a17ee79a238ad223fd59f4436d98906a0144b8d5ac47809da194a9ac1d493db
- Digest comparison to CAND-3 `aien-cli-native-release`: **EQUAL**.

Finding F1 (measured, cause UNVERIFIED): `repro-build.sh -p aien-cli` alone gives 3a17ee79... (EQUAL to CAND-3).
Building the seven packaged crates in one cargo command and taking that `aien-cli` gives
ccecf9a13ef2c7a17eb54375b1f35efba2782b967635737bd19d34298b406ac9 (same size, 6,399,222 differing bytes). I believe
dependency feature unification or code layout differs when more packages are in one invocation (UNVERIFIED, not
investigated). The CAND-3 recipe is per-package, so the last command above rebuilt `aien-cli` alone before packaging.
Also, `.github/workflows/release.yml` builds `cargo build --locked --release --workspace` (no repro-build, no
`AIEN_OMEGA_DIR`), so a published release binary is not the candidate binary (UNVERIFIED which digest it yields;
not built here).

## 2. EXECUTE: PASS

Run from an empty directory with `env -i`, no source tree. `--version` calls `nvidia-smi` when it is on PATH
(crates/aien-inference-abi `ExecutionSurface::detect`), so it was run with PATH set to an empty directory.

- `aien --version` -> `AIEN CLI v0.1.0 (Host: Linux CPU (20 Cores, 121 GB RAM))`, exit 0
- `aien --help` -> slash command list, exit 0
- `ldd` lists no cuda or nvidia library; `scripts/zero-cuda-gate.sh <installed aien>` -> ZERO_CUDA PASS; `--self-test` -> PASS
  (negative control rejected a CUDA probe)
- Not run: `aien doctor` and anything that probes the GB10 or loads a model (NOT_RUN, no-GPU rule).

## 3. UPGRADE: PASS (by overwrite; no dedicated upgrade path exists)

Change (scratch worktree only, not in this PR): one README line and the `--version` string
`v0.1.0` -> `v0.1.0+np4-upgrade` in crates/aien-cli/src/main.rs. Rebuilt `-p aien-cli`, packaged release 2
(archive sha256 8dec09b3f088fa9b...), signed with the same throwaway key, and ran the same install command over
the first install after appending `# user edit` to operator.toml.

- installed `aien` sha256 e789c04d55b1963efc5b766d23d9c7693863cc37200dbca51adb44d5fee7db46, changed from step 1
- `aien --version` -> `AIEN CLI v0.1.0+np4-upgrade (Host: Linux CPU ...)`, exit 0
- operator.toml kept (log: "Existing configuration found"), the user edit survived

## 4. ROLLBACK: PASS as a fallback, MISSING: no native rollback

install.sh keeps no previous version. Fallback used, labelled as such: run the same install command against the
release 1 assets again.

- installed `aien` sha256 3a17ee79a238ad223fd59f4436d98906a0144b8d5ac47809da194a9ac1d493db, equal to step 1 and to the
  CAND-3 digest; `cmp` against the binary inside the release 1 archive: byte-equal
- `aien --version` back to the step 1 output; zero-CUDA gate PASS; operator.toml unchanged by the rollback
- Only `aien` was compared to a CAND-3 digest. The other six binaries came from the same archive; they were not in the
  manifest and were not individually re-hashed after rollback beyond the archive match.

## Gaps (the supported procedure lacks these; not implemented here)

1. MISSING: no native rollback. No previous-version copy, no `--rollback`, no backup of the replaced binaries.
   Rollback needs the old signed assets to still be available.
2. MISSING: no recorded installed version. Nothing in the bin or config directory says which release is installed;
   the same tag with different content installed without complaint (test used one tag for both releases), and there is
   no downgrade or same-version guard.
3. MISSING: no atomic replace. `cp` writes over each binary in place. Replacing a running binary may fail with
   "Text file busy" or leave a half-written file on interruption (UNVERIFIED, not tested here). No `install -m` to temp
   plus `mv`, and no all-or-nothing across the seven files.
4. MISSING: no uninstall, and no list of installed files; a binary dropped from a later release (or `spark-cockpit-rs`
   renamed `spark-cockpit`) is never removed. Also `cortex-encoder-rs` is installed under its crate name while the
   others are renamed.
5. GAP: EN2 imprint is copied with `cp -r` over the existing imprint every run, so local edits there are overwritten
   (read from install.sh, not tested). operator.toml is preserved.
6. GAP: published releases are not the candidate. release.yml builds `--workspace` without repro-build.sh or the omega pins
   (finding F1). Next cut: make the release workflow build per the CAND recipe and publish the CAND digest list.
7. GAP: no published linux-aarch64 asset has been verified on the Spark (inherited from the 2026-10-04 receipt), and the
   pinned release key was never exercised here (NOT_RUN above).
8. GAP: `package-release.sh` output is not byte-reproducible (tar mtimes, owner); only the binaries inside are.

Next cut: one script (`scripts/upgrade-release.sh` or a mode of install.sh) that stages the new release, verifies, swaps
atomically, records the installed tag and digests in a manifest file, and keeps one previous version for `--rollback`;
plus the release workflow change in gap 6.
