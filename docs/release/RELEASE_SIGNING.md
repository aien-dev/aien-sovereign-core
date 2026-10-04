# Release signing

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

Sign (release author, on the machine that holds the private key):

```sh
ssh-keygen -Y sign -f ~/.ssh/id_ed25519_aien -n aien-release SHA256SUMS.txt
```

A GB10 release also carries `release_golden_path.json`, the artifact the
`release_golden_path_records_whether_gb10_ran` test prints on the Spark. A
release is only a GB10 release when that file says `"skipped": false`; it is
listed in `SHA256SUMS.txt` and therefore covered by the signature.
