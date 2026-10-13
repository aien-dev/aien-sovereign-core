# Golden vectors for the persistent KV record (ADR 0003)

`kv_record_v1.json` pins the namespace derivation, the chunk key chain and the
256-byte record header bytes (4 vectors: host bf16 with a partial tail, the
Llama-3.2-1B geometry, native f32, and a qk_norm/empty-sharing case).

- vectors_sha256 = `7c532a54a7d792c37e29a13db5afb55a836617f80fa17309595b35227243b89c`
- format_version = 1

The aienos side (`native/kv/CONTRACT.lock`, cut N1) records the same hash. A
vector change is a deliberate two-repo bump: regenerate with
`AIEN_KV_REGEN_GOLDEN=1 cargo test -p aien-kv-store --test golden_tests regenerate_golden`,
update this hash, then bump the lock.

Independent check (VERIFIED at generation): the namespace id, chunk key 0 and
the full 256-byte header of the `qk-norm-empty-sharing-one-full-chunk` vector
were rebuilt byte by byte with `printf` and `sha256sum` and match the file.
