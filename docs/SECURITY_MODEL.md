# AIEN Security & Telemetry Model

This document establishes the normative security architecture, terminology, and invariants for the AIEN ecosystem.

---

## 1. Core Security Invariant

AIEN provides no unsolicited outbound telemetry, avoids persistent plaintext secret storage, and uses the strongest available platform-backed secret provider where supported.

---

## 2. Telemetry and Observability Definitions

To maintain linguistic and technical precision:

### Local Observability
Metrics, traces, structured logs, benchmark counters, hardware telemetry (such as GPU temperature and power draw), and diagnostic health endpoints (`/api/pulse`, `/healthz`) retained strictly on the local machine or served to local administrative consoles. Local observability is essential for systems engineering, performance verification, and operational reliability.

### Explicit Network Activity
Network communication directly initiated by an operator or required by a user-configured feature, including dispatching prompts to configured model API endpoints, downloading verified model weights from public hubs, or synchronizing repositories across designated peer nodes.

### Outbound Telemetry
Automatically transmitted usage analytics, diagnostic beacons, behavioral tracking, user profiling, or background heartbeat telemetry transmitted to third-party endpoints.

### The Invariant
**AIEN core components MUST NOT transmit unsolicited outbound telemetry.** Local observability data remains local unless the operator explicitly exports it.

---

## 3. Secret Protection at Rest and In-Flight

### No Persistent Plaintext Storage
AIEN software does not persist unencrypted API keys, cryptographic tokens, or private credentials to disk files. Configuration templates, source repositories, and persistent working trees maintain a strict zero-plaintext-credentials standard.

### Materialization Policy
Persisted credentials are encrypted or hardware-bound at rest and are materialized into process memory only when required for authorized operations. Ephemeral buffers holding decrypted secret material are zeroized upon deallocation.

---

## 4. Platform Secret Provider Architecture

Hardware security primitives vary across operating systems. AIEN abstracts credential protection behind a uniform `SecretProvider` capability interface:

```rust
pub trait SecretProvider: Send + Sync {
    fn capabilities(&self) -> SecretCapabilities;
    fn protection_level(&self) -> SecretProtectionLevel;
    fn seal(&self, name: &str, secret: &[u8]) -> Result<SealedSecret, SecretError>;
    fn unseal(&self, secret: &SealedSecret) -> Result<Zeroizing<Vec<u8>>, SecretError>;
    fn delete(&self, name: &str) -> Result<(), SecretError>;
}
```

### Protection Levels

1. **`HardwareBackedNonExportable`**: Asymmetric private keys generated inside secure hardware silicon that cannot be extracted, where cryptographic operations occur directly within the hardware boundary.
2. **`HardwareBackedSealed`**: Secrets encrypted at rest using keys bound to hardware silicon and platform PCR state (such as TPM 2.0 sealed storage).
3. **`OsProtected`**: Credentials protected by operating-system keyring facilities (such as Linux kernel keyrings or macOS Keychain access).
4. **`MemoryOnly`**: Volatile in-process credential store for ephemeral test environments, wiped immediately on process exit.

### Platform Implementations

- **NVIDIA DGX Spark / Linux Systems**: Implemented via hardware TPM 2.0 (`/dev/tpmrm0`) with dynamic in-memory unsealing.
- **Apple Silicon (macOS)**: Implemented via Secure Enclave and hardware-backed Keychain APIs.
- **Air-Gapped & Offline Hosts**: Implemented using local hardware silicon roots of trust with zero external network verification dependencies.
