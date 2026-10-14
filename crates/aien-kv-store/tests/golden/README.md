# Golden vectors for the persistent KV record (ADR 0003)

`kv_record_v1.json` pins the engine-config digest, the compute-config digest
(namespace field 14, which since sc#504 binds the engine identity, ADR 1.2.2),
the namespace derivation (14 fields, domain `AIEN-PKV-NS-V2`, ADR 1.2), the
chunk key chain, the 256-byte record header bytes and the 4096-byte header
region (segment digest table, payload root; ADR 2.1 and section 12, cut C1b).
Eight vectors: host bf16 with a partial tail, the Llama-3.2-1B geometry, native
f32, a qk_norm/empty-sharing case, two that differ from
`host-small-partial-tail` in one compute field only (`rms-norm-eps-1e-6-only`,
`arith-epoch-2`), and two that differ from it in one engine input only
(`weight-format-fp8-experts-only`, `whole-prompt-prefill-only`).

- vectors_sha256 = `6e798652f6fd65cc023a8c5a3264d0ab0bd8cbc19d29dbcad465c23b613376f6`
- format_version = 1 (the record format did not change; only identity did)
- supersedes the C1b vectors (`7683cab9f5b1d5127773afb8a350ac1b3324afa67b80d2466e64bb61fb57fb29`,
  namespace domain `AIEN-PKV-NS-V1`) and the C1 vectors at ce5d9eb
  (`7c532a54a7d792c37e29a13db5afb55a836617f80fa17309595b35227243b89c`); neither may be pinned.

The aienos side (`native/kv/CONTRACT.lock`, cut N1) records the same hash. A
vector change is a deliberate two-repo bump: regenerate with
`AIEN_KV_REGEN_GOLDEN=1 cargo test -p aien-kv-store --test golden_tests regenerate_golden`,
update this hash, then bump the lock. The sc#504 bump of the aienos lock is
still to do.

Independent check of the C1b vectors (VERIFIED at that generation, with
`printf`, `xxd` and `sha256sum` only, no Rust): for the
`qk-norm-empty-sharing-one-full-chunk` vector the compute-config digest, the
namespace id, chunk key 0, both segment digests, the payload root, the full
256-byte header and the SHA-256 of the whole 4096-byte region were rebuilt byte
by byte and matched the file. The sc#504 vectors have not been rebuilt
independently yet; their derivation is pinned by the hand-assembled hashes in
`identity_tests.rs` (`field_14_binds_compute_config_then_engine_identity`,
`namespace_id_appends_compute_digest_after_sharing_namespace`).
