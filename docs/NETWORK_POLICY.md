# AIEN Network Policy & Egress Governance

This document defines the normative network architecture, egress classification, and offline operation contracts for AIEN core services.

---

## 1. Explicit Network Purpose Classification

Every outbound network socket or HTTP request initiated by AIEN components must declare an explicit `NetworkPurpose`:

```rust
pub enum NetworkPurpose {
    /// Outbound call to a user-configured model inference API or proxy.
    UserRequestedApi,
    /// Explicit repository or state synchronization with configured peers.
    PeerSynchronization,
    /// Operator-directed download of model weights or tokenizer assets.
    ModelDownload,
    /// Optional check for signed software release updates.
    UpdateCheck,
    /// Unsolicited usage metrics, analytics, or behavioral tracking.
    Telemetry,
}
```

---

## 2. Policy Enforcement & Deny Rules

AIEN network gateways and transport layers enforce strict destination and purpose checking:

| Network Purpose | Default Policy | Condition |
| :--- | :--- | :--- |
| `UserRequestedApi` | **Allow** | Target matches operator-configured endpoint whitelist. |
| `PeerSynchronization` | **Allow** | Explicitly configured peer identity or Radicle URI. |
| `ModelDownload` | **Allow** | Operator-initiated CLI command with verified hash. |
| `UpdateCheck` | **Operator Controlled** | Disabled by default in offline profiles. |
| `Telemetry` | **Deny (Hard Block)** | Unsolicited outbound telemetry is rejected at the transport layer. |

Any network connection attempting transmission under `NetworkPurpose::Telemetry` results in an immediate connection failure and security audit alert.

---

## 3. Air-Gap-Capable Deployment Profile

AIEN is engineered for deployment in high-security, sovereign, and disconnected environments:

1. **Zero External Service Dependencies**: Core runtime execution, physical KV cache allocation, sequence batching, Cortex epistemic storage, and hardware secret resolution function fully without active internet connectivity.
2. **Local Model Provisioning**: All required model architectures, safetensors weights, and tokenizers are resolved from local filesystem paths or hardware-attached storage.
3. **Egress-Disabled Testing**: Integration test suites are validated inside network namespaces with default-route egress disabled to verify that no code path panics or deadlocks in the absence of internet access.
