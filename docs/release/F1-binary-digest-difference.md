# F1: why the same source gives two different aien-cli digests

Measured 2026-10-06 on the Spark (aarch64, host CPU only). Source: sovereign-core 80e071a (the CAND-3 source), omega
f816473 and physics 6d7cf0d (CAND-3), rustc 1.98.1, fresh CARGO_HOME, `scripts/repro-build.sh`, three target directories.

| Build | Command | aien-cli sha256 |
|---|---|---|
| A | the six other packages together, then `-p aien-cli` alone | 3a17ee79a238... (equals CAND-3 `aien-cli-native-release`) |
| B | same as A, second target directory | 3a17ee79a238... (identical) |
| C | all seven packages in one cargo command | ccecf9a13ef2... (differs) |

- A and B are byte-identical for all seven binaries: the CAND-3 recipe is reproducible here, and `aien-cli` equals the CAND-3 digest.
- C differs from A in all seven binaries, not only aien-cli. spark-debugger: 6,386,800 bytes (A) vs 6,452,400 bytes (C), so the
  code itself differs. aien-cli: same file size, 6,399,222 differing bytes; the first differing ELF section is `.text`
  (0x5e7f4c in A, 0x5e7fcc in C); the build-id differs, as expected.
- Cause (verified at the dependency level, not by bisecting bytes): Cargo unifies features across every package named in one
  command. `cargo tree -f '{p} {f}'` for `-p spark-debugger` alone and for all seven together shows 31 shared crates with a
  different feature set (examples: cc `parallel`, bitflags `std`, futures-util `async-await,sink,...`, hyper, rustls, reqwest,
  rusqlite, log, memchr). Different features, different code. Not timestamps, not paths (the remap works), not the compiler.
- Consequence: a binary's digest depends on which other packages are in the same cargo command. The release workflow
  therefore builds `aien-cli` in its own last command and builds the other six as one fixed group; the six-group digests
  are stable (A equals B) but have no CAND-3 manifest entry to compare with.
- Not done: a byte-level map of which feature changes which function; a cargo-config fix that stops unification.
