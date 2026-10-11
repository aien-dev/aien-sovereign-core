# Release signing

## Where things stand (Drake ruling 1, 2026-10-10, sc#385 and arch#190)

- **No public-release key exists yet.** A public release needs a new, dedicated, offline Ed25519 key. Its private half
  never exists on GitHub, in CI, in a secret, on a production machine, or in this repository. A CI secret does not
  qualify. It is made at a witnessed key ceremony with a verified backup and a public-key verification test
  (`KEY_CEREMONY.md`, operator checklist sc#118). Until then `allowed_signers` is a labelled **PLACEHOLDER** with no key
  line, `install.sh` has an empty `AIEN_RELEASE_SIGNER`, and nothing can verify or be recorded as a public release.
- **The old key is the internal test key.** It signed v0.1.1. It is not reused for public releases. Its public half
  moved to `internal_test_signers` (and `AIEN_INTERNAL_TEST_SIGNER` in `install.sh`), so internal installs keep
  working; anything it verifies is recorded as `signature_check: internal-test-key`.
- **v0.1.0 and v0.1.1 are internal test releases** (Drake decision 2026-10-08). v0.1.0 has only `SHA256SUMS.txt`;
  v0.1.1 carries `SHA256SUMS.txt.sig` made with the internal test key.
- Physical rollback is also required before any public tag (public-release gates, 2026-10-06); this file covers signing
  only.

## The three stages of a release (offline-signing handoff)

1. **CI builds and publishes checksums, unsigned.** A `v*` tag push runs `.github/workflows/release.yml`: build the
   reproducible archive, run the release gate, write `SHA256SUMS.txt`, and create a **draft** release holding the
   archives and `SHA256SUMS.txt`. The draft is not public. CI holds no signing key.
2. **The operator signs offline.** Carry `SHA256SUMS.txt` (only that file) to the offline machine on a USB stick and run
   `scripts/key-ceremony.sh sign <key> <allowed_signers> SHA256SUMS.txt`. It shows the files the signature vouches
   for, signs, and verifies the signature before anything leaves the machine. Carry `SHA256SUMS.txt.sig` back and
   attach it to the draft release (GitHub release page, "Edit", drop the file in; or `gh release upload <tag>
   SHA256SUMS.txt.sig`). **Human only.**
3. **CI verifies, then publishes.** Run `.github/workflows/release-publish.yml` by hand with the tag. It checks out the
   tagged commit, downloads the draft, and runs `scripts/release-publish-gate.sh` against that commit's
   `allowed_signers`. It publishes only if the signature is valid for the pinned release key, every asset matches its
   checksum, and nothing is a dry run. Unsigned, mismatched, dry-run, internal-test-key and placeholder-key releases
   are refused.

The release key is pinned in `allowed_signers` (and `install.sh`) **before** the tag is cut, so the key a release is
checked against is fixed in the same commit the release is built from.

## What `install.sh` records

`install.sh` release mode (`AIEN_RELEASE_TAG=vX.Y.Z`) verifies `SHA256SUMS.txt.sig` with `ssh-keygen -Y verify`, then the
archive checksum, then every file in the package's `release.toml`. The class is decided by which key verified, never by
what the artifact claims, and is written to `installed.toml` (`signature-check`) and to `install-receipt.json`
(`signature_check`, plus `artifact_sha256`, `files_verified`, `cli_sha256`, `daemon_sha256`):

| `signature_check` | verified by | meaning |
|---|---|---|
| `release` | `AIEN_RELEASE_SIGNER` = `allowed_signers` | a public release (impossible until the ceremony) |
| `internal-test-key` | `AIEN_INTERNAL_TEST_SIGNER` = `internal_test_signers` | internal test release, never public |
| `dry-run-key` | a throwaway signer file named in `AIEN_DRY_RUN_SIGNERS` | dry run or test, never a release |

A package whose `release.toml` says `dry-run = true` (or candidate `NOT-RELEASABLE`) is refused if it ever verifies as
`release`. A missing signature, a one-byte change, an edited checksum list and a wrong key are each refused by name and
install nothing (`scripts/test-install-release.sh`).

Install a dry-run artifact (the `dry-run-signed-assets` workflow artifact of a manual `release.yml` run, unzipped into a
folder):

```sh
AIEN_RELEASE_TAG=dry-run AIEN_RELEASE_BASE_URL="file:///path/to/folder" \
AIEN_DRY_RUN_SIGNERS=/path/to/folder/dry-run-signers bash install.sh
```

## Verify a download by hand

```sh
sha256sum -c SHA256SUMS.txt
ssh-keygen -Y verify -f docs/release/allowed_signers -I aien-release -n aien-release \
  -s SHA256SUMS.txt.sig < SHA256SUMS.txt
```

A GB10 release also carries `release_golden_path.json`, the artifact the
`release_golden_path_records_whether_gb10_ran` test prints on the Spark. A
release is only a GB10 release when that file says `"skipped": false`; it is
listed in `SHA256SUMS.txt` and therefore covered by the signature.

## Tests (all offline, throwaway keys only)

- `scripts/test-install-release.sh`: signature classes, receipts, negative install rows.
- `scripts/test-offline-signing.sh`: ceremony rehearsal, offline sign, publish gate refusals.
- `scripts/test-release-signing-workflow.sh`: no release key in CI; release.yml only drafts; publish only after the gate.
- `scripts/test-public-key-integration.sh`: from only the ceremony's public folder: the trust-config change is a reviewable 3-file diff, the record fingerprint matches the pinned key, matching key installs as `release`, other key / tampered archive / tampered checksums refused.
- `scripts/check-release-trust-config.sh`: run in CI; passes in the PLACEHOLDER state and, after the ceremony, only if `allowed_signers`, `install.sh` and `ceremony-record.txt` agree.
- `scripts/test-release-checks.sh`: signature and checksum verification, placeholder refusal.
