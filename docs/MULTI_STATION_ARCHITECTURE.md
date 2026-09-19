# AIEN Multi-Station Federation and Sovereign Mesh Architecture

This specification outlines the network topology, cryptographic identity, and memory replication protocols connecting multiple sovereign computing stations across the AIEN ecosystem without centralized cloud dependencies.

---

## 1. Architectural Motivation

AIEN operates as a sovereign computing architecture. Individual nodes execute work independently on dedicated hardware. To scale beyond a single workstation without surrendering sovereignty to multi-tenant cloud providers or external API coordinators, AIEN implements a peer-to-peer mesh federation.

Core requirements:
1. **Zero External Cloud Dependencies**: No third-party relays, authorization servers, or metered API gateways.
2. **Hardware-Bound Cryptographic Identity**: Every station authenticates via hardware TPM-generated keys.
3. **Deterministic Memory Convergence**: Knowledge graph updates converge across nodes using append-only change receipts.
4. **Heterogeneous Hardware Execution**: Workloads route dynamically between heavy GPU accelerators and local CPU unified memory.

---

## 2. Station Taxonomy

Nodes in an AIEN mesh belong to one of three operational roles:

```
+-----------------------------------------------------------------------+
|                 Tier-Alpha: Reference Station (DGX Spark)             |
|  - Grace Blackwell GB10 (121 GB Unified LPDDR5X)                      |
|  - High-concurrency NVFP4 Serving (Qwen 2.5 7B, Nemotron 30B)         |
|  - Canonical Cortex Store (atlas-memory) & Dream State Engine         |
+-----------------------------------^-----------------------------------+
                                    |
            WireGuard Mesh Overlay  |  Radicle P2P Git Anchors
            ChaCha20-Poly1305       |  Signed Delta Receipts
                                    |
        +---------------------------+---------------------------+
        |                                                       |
+-------v-------------------------------+       +---------------v-----------------------+
|  Tier-Beta: Operator Workstations     |       |  Tier-Gamma: Sovereign Edge Sentinels |
|  - Apple Silicon MacBooks (M1-M4)     |       |  - Compact x86_64 / ARM64 Servers     |
|  - Native CPU Fallback Inference      |       |  - spark-inquisitor Diff Auditing     |
|  - aien CLI & Subagent Orchestration  |       |  - AEGIS Perimeter Containment       |
+---------------------------------------+       +---------------------------------------+
```

### Tier-Alpha: Reference Computing Stations (`spark`)
- **Hardware Profile**: NVIDIA DGX Spark (Grace Blackwell GB10, aarch64, 121 GB unified memory).
- **Workloads**:
  - Continuous batching neural model serving (ModelOpt NVFP4, BF16).
  - Canonical Cortex memory database with bi-encoder vector indexing (`cortex-encoder-rs`).
  - Autonomous background maintenance daemons (Dream cycle state consolidation, Spark supervisor watchdog).

### Tier-Beta: Operator Workstations (`workstation`)
- **Hardware Profile**: Apple Silicon MacBooks (M1 / M2 / M3 / M4, 8 GB to 128 GB unified memory) and generic Linux workstations.
- **Workloads**:
  - Interactive operator interfaces (`aien` CLI, `spark-cockpit-rs`).
  - Local CPU fallback execution via `NativeCpuInferenceBackend` for offline tasks.
  - Subagent task graph dispatch and local code verification.

### Tier-Gamma: Sovereign Edge Sentinels (`sentinel`)
- **Hardware Profile**: Compact x86_64 or aarch64 edge servers.
- **Workloads**:
  - Continuous diff auditing via `spark-inquisitor`.
  - Zero-telemetry egress monitoring and perimeter defense.

---

## 3. Mesh Transport & Cryptographic Identity

### WireGuard Peer-to-Peer Overlay
- Inter-station communication occurs across a direct WireGuard overlay network.
- Point-to-point tunnels link stations without intermediate proxy servers.
- Station IP addresses route within a private, non-routable subnet (`10.42.0.0/16`).

### Hardware TPM Key Derivation
- Station identity keys derive directly from the onboard hardware security module (`atlas-vault`).
- Session handshakes use Curve25519 ECDH for key agreement and ChaCha20-Poly1305 for AEAD payload encryption.
- Station certificates expire every 24 hours and renew dynamically from the local TPM.

### Radicle Git Anchor Synchronization
- Code repositories, architectural manifests, and configuration files synchronize peer-to-peer via Radicle (`rad-id-sync`).
- Updates require cryptographic commit signatures from authorized station keys.

---

## 4. Memory Federation Protocol

Cortex memory replication maintains epistemic consistency between the Reference Station and satellite workstations:

### 1. Canonical Storage
The Reference Station maintains the canonical SQLite WAL database for space `atlas-memory`.

### 2. Delta Event Streaming
When an operator workstation writes an entity, claim, or discovery receipt:
1. The workstation commits the record locally with a monotonic logical timestamp.
2. An encrypted delta payload (`POST /api/cortex/sync`) transmits to the Reference Station over WireGuard.
3. The Reference Station validates the signature, writes the record, and dispatches the text to `cortex-encoder-rs` to compute high-precision 768-dimensional dense vector embeddings.

### 3. Conflict Resolution
- Entity and claim updates are strictly additive with version-stamped revisions.
- In the event of concurrent writes to the same canonical entity name, Cortex merges metadata dictionaries and preserves both text revisions with distinct cryptographic identifiers.
- Retractions operate as tombstone claims, preserving the complete historical audit trail without destructive in-place deletion.

---

## 5. Failure Modes and Partition Handling

| Failure Scenario | System Behavior | Recovery Action |
| :--- | :--- | :--- |
| **Network Partition (Air-Gap)** | Tier-Beta workstations switch directly to local SQLite storage and `NativeCpuInferenceBackend`. | Queued write receipts sync to Reference Station upon mesh reconnection. |
| **Reference Station Offline** | Satellite stations execute inference locally on host CPU memory; Cortex reads serve from local replicas. | Supervisor daemon restarts reference services; stations resume delta sync. |
| **Unauthorized Station Detection** | AEGIS perimeter detection flags non-whitelisted WireGuard handshake attempts. | Offending public key permanently blocked at kernel firewall table. |
