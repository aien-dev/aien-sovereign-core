# CAND-4 local release demo (lane G, cand4-release), 2026-10-06

Everything ran on the Spark (GB10). Nothing was published, no key was uploaded, and the release.yml workflow was not
run. Signing used only the local key whose public half is already in `docs/release/allowed_signers`.
Scratch root: `~/.claude/jobs/9bfe8553/tmp/laneG` (named `L/` below).

## Inputs

- sovereign-core release branch `cand4/release-candidate` 3c9791260a3e06614aaa330ff2bdefb39bf46439 (the CAND-4
  `release/candidate.toml` and oracle fixture). Build worktree at d5b78ff7a6be14d23e3cb00d3f9c4b4751442ffa.
- Fresh clones: omega c0369e67, aienos b84c0a6, physics 6d7cf0d. Fresh `CARGO_HOME`.
- Model: unsloth/Llama-3.2-1B-Instruct snapshot 5a8abab. Its 7 files hash equal to the digests in `[model]`.

## Build (G2 step 1): PASS

`L/scratch/release_build.sh`: `cargo fetch --locked`, then `scripts/release-build.sh --offline -vv` in the clean
worktree. `AIEN_DEV_FALLBACK`, `AIEN_FORCE_CPU_STUB` and `AIEN_OMEGA_COMPOSE_LIB` were unset. Result: rc 0, 2026-10-06
20:41Z to 20:44Z, and the log shows `has_omega_gpu`. The log `L/build-rel.log` has sha256
1aeba3455b7a6a6451bf21b34d76978e43246b847ee997632b45b1b47bbc1db2.

All 7 binaries equal the CAND-4 `[executables]` digests:

| binary | sha256 |
|---|---|
| aien-cli | ad6b7eb5330ea19d31d02e68271624d5c964bcbc24983742caae845f62c4c324 |
| spark-cockpit-rs | 618000924c5c6048c9dc034f0b40b8effbe3519fb20f96e950e97226b29e0dd4 |
| spark-inquisitor | e3ffb98a688cb06495a52b1a862433dd9628ea3d352694ee1a784c90006d97cc |
| cortex-encoder-rs | e6f290ebd0bc8b2a8d84bbb206edc52220bd04c6840e3c5dd3275273d227b1d5 |
| cortex-rs | 4df75cc0b351da753374ddbe5fe4b861edd048ef95965d591b35b80130c68440 |
| spark-supervisor | 0c74cdb5c1bf3612f847a07289dfc5ea8852954ecf9a1ba53c423a01ffa9f0a2 |
| spark-debugger | 95b866809da61fe877d7f1121fb33f7240060acd8a07de789729c48651b97288 |

## Release demo (`L/scratch/demo.sh`): full log `L/demo/demo.log` (sha256 9fece515bbd3ada0220f71d5eeea4528e1ffc48611a1c1bd640446c2a0417e51)

Install prefix: `L/prefix`, which is empty at the start. The installer runs in release mode from `file://` URLs, with
`AIEN_INSTALL_NO_PROFILE=1` and a private `HOME`.

| step | what | result |
|---|---|---|
| S1 | `package-release.sh`, which runs the gate, then `check-release-candidate.sh --package ... --native` | rc 0 and rc 0, `candidate=CAND-4`. Package sha256 e93a90d576dd17f5de5ead7bf0f04627b5cef8a1c83e82479a75609ef491b259. A second package build gave the same bytes (byte-reproducible). |
| S2 | prior release: a CAND-3-gated package (CAND-3 `candidate.toml` and omega.lock f816473 from d5b78ff, CAND-3 aien-cli 3a17ee79a238ad22..., plus the 6 CAND-4 helpers) | rc 0, `candidate=CAND-3`. Package sha256 0ecf750e43b8254463875ab0672dcb065e00e33ec3eaf74e16d02a9bf5c974a7 |
| S3 | `ssh-keygen -Y sign -n aien-release`, then `verify-release-assets.sh` and `ssh-keygen -Y verify` for both releases | all rc 0, `Good "aien-release" signature`. CAND-3: SHA256SUMS.txt 384ca8bd6604d3c71534117e5235c59d3b9b24f954fd32b8409258abbf024d8e, sig b9114080dc927dc584b0e1cb9c2f9cdcae46693a3abea124c62b9171a840e087. CAND-4: SHA256SUMS.txt fce93886a2c8ebc86547921a53583f6a27c72cf2170ada55e2ad0f3ffd034113, sig 7a05ffd4866580ccd2c53c99830dacd3d86a9c1a3d87b29029fe331db4acc611 |
| S4 | clean-prefix install of the prior release (CAND-3) | rc 0, `Live release is now 3a17ee79a238 (candidate CAND-3)`. `aien --version` rc 0 |
| S5 | upgrade to CAND-4 with `AIEN_TEST_PAUSE_AT=mid-copy`, installer killed with SIGKILL | live still 3a17ee79a238 (unchanged). The old `aien --version` rc 0, the CAND-3 gate and verify are still rc 0. 1 stage dir is left over, as designed (it is removed by the next run). |
| S6 | rerun the upgrade to CAND-4 | rc 0, live ad6b7eb5330e, 0 stage dirs. All 7 installed binaries are the same as the manifest. `installed.toml` has current ad6b7eb5330e, previous 3a17ee79a238, last-action install |
| S7 | upgrade again over itself | rc 0, `already staged and complete`, digests unchanged |
| S8 | `install.sh --rollback` twice | rc 0: live 3a17ee79a238 (CAND-3 back), last-action rollback. Then rc 0: live ad6b7eb5330e (CAND-4 digests back) |
| S9 | negative: `release.toml` aien-cli-sha256 altered and re-signed | gate rc 1 `package aien-cli-sha256 is not the digest of bin/aien`. Installer rc 1 `package does not match its release.toml. Nothing was installed.` Live unchanged |
| S10 | negative: `SHA256SUMS.txt.sig` removed | verify rc 1 `SHA256SUMS.txt.sig is missing: refusing an unsigned release`. Installer rc 1 `could not download SHA256SUMS.txt.sig`. Live unchanged |

The demo ended at 2026-10-06 20:45:49Z.

## Workflow smoke with the installed CAND-4 aien (`L/scratch/smoke.sh`): PASS

`L/smoke/smoke.txt` has sha256 b064c7e78a10e6aac76fd2bf026f4191303751b63cf4fae8790766721c27d52f. The smoke ran inside
one quietlock hold (rc 0, 20:47:46Z).

- The installed binary is `L/prefix/cfg/releases/ad6b7eb5330e/bin/aien`, sha256 ad6b7eb5330ea19d....
- One GB10 `StreamTurn` (`AIEN_REQUIRE_BLACKWELL=1`) returned rc 0 with `TurnFinished` text "An operating system (OS)
  is a software layer that manages computer hardware resources," and `total_tokens` 16.
- The NP1 workflow driver `docs/campaigns/next-phase-1/run-campaign.sh` ran with the installed aien: rc 0, steps S1 to S8
  all `ok:true`. It was run read-only from the repo; no campaign file was changed.

## Oracle independence check (G1): AGREE, text level only

- The CAND-4 aien-cli ran as a GB10 daemon (`NativeTransformerBackend/OmegaGb10Backend`). Its StreamTurn text equals the
  Hugging Face oracle's 16-token greedy text.
- `L/check/turn.jsonl` has sha256 e3f1ab79e808bef4d2fedebb3249b6a9dcbd9e814b7c6f4497622e8c38dbcc80.
- `L/check/daemon.log` has sha256 9affbec67ba7a659f50cc03aefbfc1604d3ec56cbb0e5d586b7d99cab798099a.
- The quietlock hold reported rc 1 (whisper 20:41:12Z). That status comes from the script's last line, a
  "daemon still running?" test, which returns 1 when the daemon has already exited. The turn itself was rc 0
  (`turn.rc`), `compose shutdown` returned `{"ok":true,"shutdown":true}`, and no "still running" line was written.
- Limit: aien-cli exposes no token ids or logits, so only the decoded text and the token count are compared.

## Limits

- The prior release (S2) is a CAND-3-gated package. CAND-3 pinned no helper binaries, so it carries the CAND-4 helpers;
  only aien-cli differs between the two releases.
- `file://` URLs stand in for GitHub Releases. Nothing was uploaded, and the release.yml workflow was not run.
- The CAND-4 binaries come from sovereign-core d5b78ff (the `[commits]` line). Main has since gained aien-runtime
  source changes (d6e102b, NP1 v6), so a native package built from a later main is expected to be refused by the gate's
  `aien-cli-native-release` check (fail closed). This was not tried: UNVERIFIED. A CAND-4 release is built from
  d5b78ff with this `release/candidate.toml`, as done here.
