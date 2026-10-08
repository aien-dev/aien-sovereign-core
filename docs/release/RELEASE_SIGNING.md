# Release signing

## Status of this key and of v0.1.0 / v0.1.1 (Drake decision, 2026-10-08)

- **v0.1.0 and v0.1.1 are internal test releases.** They are not public releases, whatever their release pages say. v0.1.0 carries only `SHA256SUMS.txt` and has no signature. v0.1.1 carries `SHA256SUMS.txt.sig`, made with the key pinned below (verified 2026-10-08).
- **The key pinned below is the old release-signing key.** It is NOT the dedicated offline release key that a public release requires (aien-architecture `CURRENT_EXECUTION_PLAN.md`, addendum 2026-10-06 (late), D1). The 2026-10-06 decision keeps the operator's home-server and login keys out of release signing; this key does not satisfy that gate.
- **It stays pinned for internal compatibility.** The pinned public key stays in `allowed_signers` and in `install.sh` (`AIEN_PINNED_SIGNER`), so internal installs, upgrades, the release gate and the existing receipts keep verifying. Anything signed with it is an internal test release.
- **A public release requires three steps.** Each needs Drake's separate go-ahead when it is done:
  1. a documented key-generation ceremony for the dedicated offline release key;
  2. custody verification of that key;
  3. a signature-verification test showing a release signed with the new key verifies against the newly pinned public key.
- **No key is generated now.** No ceremony date is set. Nothing in this repository counts the old key as the public-release key.

Every release carries `SHA256SUMS.txt` (one line per asset) and
`SHA256SUMS.txt.sig`, an OpenSSH signature made with the AIEN release key.
The public half of that key is `allowed_signers` in this folder, so the key a
release is checked against is pinned in the same commit the release is cut from.

Verify a download:

```sh
sha256sum -c SHA256SUMS.txt
ssh-keygen -Y verify -f docs/release/allowed_signers -I aien-release -n aien-release \
  -s SHA256SUMS.txt.sig < SHA256SUMS.txt
```

Sign (release author, on the machine that holds the private key; with the key pinned today this makes an internal test release only, see the status above):

```sh
ssh-keygen -Y sign -f ~/.ssh/id_ed25519_aien -n aien-release SHA256SUMS.txt
```

A GB10 release also carries `release_golden_path.json`, the artifact the
`release_golden_path_records_whether_gb10_ran` test prints on the Spark. A
release is only a GB10 release when that file says `"skipped": false`; it is
listed in `SHA256SUMS.txt` and therefore covered by the signature.
