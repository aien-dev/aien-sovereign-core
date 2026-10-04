# Receipt: offline release install, DGX Spark, 2026-10-04

Machine: NVIDIA DGX Spark (GB10, aarch64, Linux). Repository: fresh clone of
aien-sovereign-core, main at 58f7c34 (after #192) plus this branch. Every
install step below ran with the release assets on local disk (file:// base URL),
so no network was needed for verification or install.

## 1. Published release v0.1.1: signature and checksum chain

Commands (run on the Spark):

```
gh release download v0.1.1 -R aien-dev/aien-sovereign-core -p 'SHA256SUMS.txt*' -p 'sovereign-linux-x86_64.tar.gz'
ssh-keygen -Y verify -f docs/release/allowed_signers -I aien-release -n aien-release -s SHA256SUMS.txt.sig < SHA256SUMS.txt
sha256sum -c SHA256SUMS.txt --ignore-missing
tar -tzf sovereign-linux-x86_64.tar.gz | head
```

Results:

- Good "aien-release" signature for aien-release with ED25519 key SHA256:H0BmJOBKGKioiB/PZjf4Yo3c+4KrDgtArDi/+AECj1Y
- sovereign-linux-x86_64.tar.gz: OK
- The archive carries bin/, imprints/en2-trinity/, install.sh, README.md, CONSTITUTION.md.

Not run to completion: `AIEN_RELEASE_TAG=v0.1.1 bash install.sh` on this machine.
v0.1.1 has no linux-aarch64 asset; the release workflow gained that target in
#192, so the next tag will carry it. UNVERIFIED: a published aarch64 binary
installing on the Spark. No x86_64 emulation is available here (docker
--platform linux/amd64 fails with exec format error) and the Mac was not
reachable, so the x86_64 and macos-arm64 assets could not be installed either.

## 2. Standalone offline install into a clean HOME (fixture release)

Command: `bash scripts/test-install-release.sh` on this branch, cases 5 and 6.

What it proves: a copy of install.sh on its own (no checkout, no Cargo.toml, no
git clone), HOME set to an empty temporary directory, release assets served
over file://, installs the signed fixture release (bin/aien, bin/cortex and the
EN2 imprint from the archive) and writes nothing into HOME. With no signer
override, the key pinned inside install.sh (AIEN_PINNED_SIGNER, checked equal
to docs/release/allowed_signers) refuses the fixture because it is signed by a
throwaway key; nothing is installed.

Result: PASS, 2026-10-04, Spark aarch64. The same test runs in CI (ci.yml, step
"Verify Signed Release Install") on x86_64.

## 3. What a fresh machine needs

- install.sh, plus either network access to the GitHub release or the three
  assets on disk (SHA256SUMS.txt, SHA256SUMS.txt.sig, the archive) with
  `AIEN_RELEASE_BASE_URL=file:///path/to/assets`.
- ssh-keygen, curl, tar, and sha256sum or shasum. No git, no cargo, no Python.
- Command: `AIEN_RELEASE_TAG=v0.1.1 bash install.sh`

## Next

Cut the next release after #192 so the linux-aarch64 asset exists, sign
SHA256SUMS.txt by hand per docs/release/RELEASE_SIGNING.md, then rerun section 1
for that tag and perform the real install on the Spark in a clean HOME. Append
the result to this file.
