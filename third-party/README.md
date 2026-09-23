# third-party

Pinned upstream dependencies, checked in as git submodules so a clean
clone builds with no sibling checkouts.

## aien-protocols

- Upstream: https://github.com/aien-dev/aien-protocols (mirrored per
  `../aien-protocols.git` relative URL, so Forgejo mirrors resolve too)
- Pinned to tag `v0.1.0` (commit `7ac6facb630ca7e9a6ab4125b292203fe2bb6687`),
  the protocol version all consumers target per the upstream CONSUMERS.md.
- Consumed via in-tree path dependencies, e.g.
  `crates/spark-harvester` depends on
  `third-party/aien-protocols/crates/aien-protocol-types`.

Clone with submodules:

```bash
git clone --recurse-submodules https://github.com/aien-dev/aien-sovereign-core.git
```

To move to a newer upstream tag:

```bash
cd third-party/aien-protocols
git fetch --tags origin
git checkout vX.Y.Z
cd ../..
git add third-party/aien-protocols
```

Record the new version in the upstream CONSUMERS.md table in the same
change, per the aien-protocols versioning policy.
