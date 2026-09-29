# spark-aegis

Sovereign defensive boundary and containment engine for the NVIDIA DGX Spark workstation.

## Sovereign Doctrine

"The network is the house. Cut the session. Isolate the host. Close the door. Keep the evidence."

## Invariants

- **SECURE_TPM_ONLY**: Plaintext secrets and `.env` files must never touch disk. All credentials resolve dynamically via the hardware TPM vault.
- **UNSLOP_COMPLIANCE**: Strict zero em dash and en dash policy. All AI buzzwords, transitional filler, and antithesis tropes are forbidden.
- **PURE_NATIVE_SYSTEMS**: The entire security runtime is compiled native Rust with Mojo SIMD vector acceleration.
- **BLAKE3_CORTEX_TRAIL**: Every incident and triage verdict commits a Blake3 cryptographic audit entity to Cortex memory.

## Architecture

`spark-aegis` consists of the following components:

- `doctrine`: Formal definition of the four defensive actions and system invariants.
- `scanner`: High-speed scanner detecting secrets, slop, and dangerous commands across files, directories, and git diffs.
- `containment`: Realization of the four defensive directives (`cut_session`, `isolate_host`, `close_door`, `keep_evidence`).
- `pr_triage`: Autonomous PR review and diff verification engine outputting cryptographically signed safety verdicts.
- `mojo_bridge`: Dynamic bridge to Mojo 1.0 SIMD vector routines with pure Rust compiled fallback.
- `audit`: Live system audit inspecting listening sockets, workspace confinement, and Cortex connectivity.

## CLI Usage

### Deep Scan
```bash
spark-aegis scan /path/to/target
```

### PR Triage and Review
```bash
spark-aegis review-pr diff.patch
git diff | spark-aegis review-pr -
```

### Live System Audit
```bash
spark-aegis audit
```

### Containment Actions
```bash
# Terminate session
spark-aegis contain --session pts/2

# Isolate process
spark-aegis contain --pid 12345

# Quarantine path
spark-aegis contain --close /path/to/malicious_file
```

### Display Doctrine
```bash
spark-aegis doctrine
```

## License

Apache-2.0 WITH LLVM-exception
