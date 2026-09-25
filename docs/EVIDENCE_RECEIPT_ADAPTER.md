# Evidence Receipt Adapter Specification for `aienos`

Scope: how `aienos` test setups emit machine readable qualification results
that `aien-proof` turns into sealed evidence receipts. This document is an
input contract only. Every JSON block marked EXAMPLE is a shape example, not
evidence. No gate has passed unless `aien-proof gate status` says PASS over
real stored receipts.

You never need to edit `aien-proof` or depend on its Rust internals. You need
only:

1. a built `aien-proof` binary on PATH (or `cargo run -p aien-proof --`),
2. a shared receipt store (the proof board dir, `AIEN_PROOF_DIR`),
3. an assertion file your test setup writes,
4. one `aien-proof receipt import` (alias: `aien-proof qualify`) call per run,
5. for Machine 1 runs, a `hold` lease around the run.

## 1. Assertion file contract

Your setup writes one JSON file per run. Prose is never parsed. The file
must match this shape exactly (unknown fields are refused):

```json
{
  "assertions": [
    {
      "id": "artifact_signature_valid",
      "expected": "valid",
      "observed": "valid",
      "result": "pass",
      "source": "scripts/qemu_seed0b.sh",
      "note": ""
    }
  ],
  "result": "PASS"
}
```

Rules:

- `id` is stable across runs (`secure_boot_unchanged`, `boot_order_unchanged`,
  `artifact_signature_valid`, `granted_subset_requested`,
  `w_x_never_simultaneous`, `unauthorized_write_denied`,
  `all_task_frames_reclaimed`, `store_previous_or_new_only`,
  `root_graph_valid`, `machine_returned_to_linux`).
- `result` per assertion is `pass` or `fail` (or boolean true/false).
- Top level `result` is optional. When absent it is derived (all pass gives
  PASS, else FAIL). A claimed PASS with any failing assertion is refused.
  A failing run can never be upgraded to PASS at import.
- At least one assertion is required for a PASS receipt.

## 2. Import command contract

EXAMPLE (SEED-0B QEMU run):

```sh
aien-proof receipt import \
  --repo https://github.com/aien-dev/aienos \
  --commit 3fa4c8e2b1d04f7a9c6e5b8d2a1f4c7e9b0d3f21 \
  --gate seed-0b-qemu \
  --tier QEMU \
  --procedure "scripts/qemu_seed0b.sh" \
  --machine qemu-virt-aarch64 \
  --toolchain "$(rustc -vV)" \
  --assert /tmp/seed0b-assert.json \
  --artifact blake3:<hex-of-boot-image> \
  --output-file /tmp/seed0b-output.log \
  --dep <m3-receipt-id> \
  --dep <recovery-media-receipt-id>
```

Field notes:

- `--commit` is the exact 40 digit lowercase git SHA (64 digits accepted).
- `--tier` is one of `HOST_TEST QEMU QEMU_SECURITY MACHINE1_READ_ONLY
  MACHINE1_ATTENDED MACHINE1_MUTATING TEST_ONLY_TRUST PRODUCTION`.
  The environment label always equals the tier, so a QEMU run can never be
  filed as physical qualification.
- `--artifact` digests are `<algo>:<hex>` with algo `blake3`, `sha256`, or
  `sha512` (repeatable).
- `--output-file` is hashed to the receipt output digest. Prefer it over
  `--output-digest`.
- `--dep` lists prerequisite receipt IDs (repeatable). Missing deps stay
  missing: verification reports INCOMPLETE, never PASS.
- `--dirty` marks uncommitted source changes. Gates that require clean
  sources block dirty receipts.
- On success the command prints the receipt ID (the BLAKE3 of the canonical
  encoding) and stores `<store>/receipts/<id>.json`.

## 3. Machine 1 runs: the `hold` lease

Every `MACHINE1_*` receipt must bind an exclusive hold. Wrap the run:

```sh
HOLD=$(aien-proof hold --resource machine-1 --job seed-0b-machine1 -- \
  scripts/seed0b_machine1.sh)
```

Then import with:

```sh
  --tier MACHINE1_ATTENDED \
  --machine machine-1 \
  --procedure "scripts/seed0b_machine1.sh" \
  --declared-mutation REMOVABLE_MEDIA_ONLY \
  --lease-hold "$HOLD" \
  --lease-resource machine-1 \
  --output-file <hold-log-or-collector-output>
```

Rules:

- `--procedure` must equal the held command line exactly.
- `--machine` must equal the leased resource.
- `--output-file` (or digest) must equal the held run output digest.
- `--declared-mutation` is required for hardware tiers. Allowed classes:
  `NONE VOLATILE_ONLY REMOVABLE_MEDIA_ONLY ONE_TIME_BOOT_SELECTION
  BOUNDED_TEST_REGION_WRITE BOOT_CONFIGURATION_CHANGE TRUST_ROOT_CHANGE
  TPM_POLICY_CHANGE DESTRUCTIVE_STORAGE`.
- A read-only run (`MACHINE1_READ_ONLY`) cannot claim more than
  `REMOVABLE_MEDIA_ONLY`. Observed mutation beyond declared mutation fails.
- A Machine 1 receipt without a matching exclusive hold verifies as
  INCOMPLETE, never PASS.

## 4. EXAMPLE input contracts per gate input

All values below are placeholders. Replace them with measured values.

### 4.1 SEED-0B QEMU (owner: SEED-0B completion lane)

EXAMPLE assertion file:

```json
{
  "assertions": [
    {"id": "artifact_identity", "expected": "<image-digest>", "observed": "<image-digest>", "result": "pass", "source": "scripts/qemu_seed0b.sh"},
    {"id": "artifact_signature_valid", "expected": "valid", "observed": "valid", "result": "pass", "source": "scripts/qemu_seed0b.sh"},
    {"id": "granted_subset_requested", "expected": "subset", "observed": "subset", "result": "pass", "source": "admission-check"},
    {"id": "authorized_op_permitted", "expected": "permit", "observed": "permit", "result": "pass", "source": "admission-check"},
    {"id": "unauthorized_write_denied", "expected": "deny", "observed": "deny", "result": "pass", "source": "admission-check"},
    {"id": "all_task_frames_reclaimed", "expected": "0 live", "observed": "0 live", "result": "pass", "source": "cleanup-check"},
    {"id": "machine_returned_to_linux", "expected": "linux", "observed": "linux", "result": "pass", "source": "cleanup-check"}
  ]
}
```

Import with `--gate seed-0b-qemu --tier QEMU`, plus `--dep` on the M3
receipt and the recovery media receipt IDs. The hostile input matrix is a
separate import with `--gate seed-0b-hostile-matrix --tier QEMU`.

### 4.2 Native boot rollback (owner: M0 rollback lane)

EXAMPLE assertion file:

```json
{
  "assertions": [
    {"id": "secure_boot_unchanged", "expected": "on", "observed": "on", "result": "pass", "source": "rollback-verify.sh"},
    {"id": "boot_order_unchanged", "expected": "<efibootmgr-before>", "observed": "<efibootmgr-after>", "result": "pass", "source": "rollback-verify.sh"},
    {"id": "rollback_boot_ok", "expected": "linux", "observed": "linux", "result": "pass", "source": "rollback-verify.sh"}
  ]
}
```

QEMU rehearsal: `--gate native-rollback --tier QEMU`. Hardware gate input:
`--tier MACHINE1_ATTENDED` under a `hold` lease with
`--declared-mutation ONE_TIME_BOOT_SELECTION` (or higher if the run writes
more), following section 3 exactly.

### 4.3 System Store v1 crash consistency (owner: System Store lane)

EXAMPLE assertion file:

```json
{
  "assertions": [
    {"id": "store_previous_or_new_only", "expected": "no torn writes", "observed": "no torn writes", "result": "pass", "source": "crash-qualify.sh"},
    {"id": "root_graph_valid", "expected": "valid", "observed": "valid", "result": "pass", "source": "crash-qualify.sh"},
    {"id": "w_x_never_simultaneous", "expected": "never", "observed": "never", "result": "pass", "source": "crash-qualify.sh"}
  ]
}
```

Import with `--gate store-v1-crash --tier QEMU` (or the tier the lane
declares), with `--dep` on the ADR 0015 acceptance receipt once it exists.

## 5. Verification commands (read only)

```sh
aien-proof receipt verify <receipt-file>
aien-proof verify-chain <receipt-file>
aien-proof gate status SEED-0B
aien-proof gate explain P3_ENTRY
```

`gate status` prints one line per prerequisite (`PASS`, `FAIL`, `BLOCKED`
with `MISSING` details) and names the responsible item. Example shape:

```text
P3_ENTRY: BLOCKED
PASS    M3 ...
MISSING SEED-0B-MACHINE1 ...
```

Materialize the checked in example manifests with:

```sh
aien-proof gate examples --out <store>/gates
```

## 6. What the evidence lane will not do

- Change store, boot, artifact, admission, Secure Boot, TPM, trust root,
  native boot, or SEED-0B execution behavior.
- Infer completion from issue status or prose. If your run lacks a required
  input, the receipt schema or verification reports the gap instead.
- Upgrade failing runs, satisfy physical requirements with QEMU receipts,
  or close gates on BLOCKED runs.
