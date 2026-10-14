# Persistent-KV cache files: real exposure on the Spark (ticket #497, map #494)

Facts only, for a later security decision. Date of inspection: 2026-10-13.
Tags: VERIFIED (seen, cited), INFERRED (reasoned, source named), UNKNOWN.
PR branches inspected: #488 `pkv/484-persistent-kv`, #489 `pkv/c4-host-fs`, #490 `pkv/pr3-worker-review`, #491 `pkv/pr4-runtime`, #492 `pkv/pr5-restart`, #493 `pkv/pr6-concurrency-bench` (#487 `fix/preempt-resume-reemit` is a scheduler fix; not examined for file code). Line numbers below are for #493 `crates/aien-kv-store/src/host_fs.rs` unless stated.

## 1. What exists on main

- VERIFIED: main (b9b52f9) has no `aien-kv-store` crate and no `host_fs.rs` (`git ls-tree -r main | grep -c aien-kv-store` -> `0`). Today main creates no persistent-KV files at all.
- VERIFIED: `host_fs.rs` first appears in #489. #490, #491 and #492 have byte-identical copies (same md5 `ad219165...`). #493 differs (md5 `d8b5fdb3...`). #489 differs from #490 in line positions only for the items below; the permission-relevant calls are the same in all three versions (see section 3).

## 2. Where the cache directory lives

- VERIFIED: there is no default path in code. The root comes only from env var `AIEN_KV_STORE_DIR`; unset means persistence is off and nothing is created (`crates/aien-runtime/src/shared_kv.rs:420`, `store_dir: Option<PathBuf>` in `KvPersistSettings`; `open_host_kv_residency` returns `Ok(None)` when it is `None`, #493). `docs/persistent-kv/METRICS_AND_SETTINGS.md:9` says "unset = persistence off". `docs/FIRST_RUN.md:41` says "off by default".
- VERIFIED: no systemd unit, script or doc on #493 sets a value for it (`git grep AIEN_KV_STORE_DIR pr/493` finds only code, tests and docs; tests use `/tmp/kv`, `/tmp/kv-c7` as examples).
- VERIFIED: the layout under the root is `STORE_ID`, `objects/00..ff/<64-hex>.kvc`, `tmp/` (`host_fs.rs:10-21`, `object_path_for` and `tmp_path_for` at ~596-615). File names are built only from the hex of a 32-byte hash plus numbers (job id, sequence id, epoch); no request text or user-supplied string becomes part of a path.
- VERIFIED (live Spark): the running server has no such variable and its binary has no such code. `/proc/3178352/environ` has 0 `AIEN_KV*` entries; `strings -a /proc/3178352/exe | grep -c AIEN_KV_STORE_DIR` -> `0`. So the live server on :18091 does not write persistent-KV files today.
- UNKNOWN: which directory an operator would choose once the feature is turned on. Nothing in the repo or on the Spark fixes it.

## 3. What permissions host_fs.rs creates (all PR branches)

- VERIFIED: the code never sets a mode. In #493 there is no `PermissionsExt`, `.mode(`, `set_permissions`, `O_NOFOLLOW`, `canonicalize` or `umask` anywhere in `crates/aien-kv-store/src` or `shared_kv.rs` (`git grep -E 'PermissionsExt|\.mode\(|set_permissions|O_NOFOLLOW|canonicalize|umask' pr/493 -- crates/aien-kv-store crates/aien-runtime/src/shared_kv.rs` only hits the test file `tests/host_fs_hardening_tests.rs:15,24,31`, which uses chmod to simulate errors). Same calls in #489 and #490.
- VERIFIED: every file and directory is made with plain std calls, so the result is `0666 & ~umask` for files and `0777 & ~umask` for directories:
  - root: `std::fs::create_dir_all(root)` (`:2764`); `objects` (`:2770`), `tmp` (`:2776`), 256 shard dirs (`:2790`): `std::fs::create_dir`.
  - object temp file: `OpenOptions::new().create(true).write(true).truncate(true).open(&tpath)` (`:1958-1962`), then renamed over the final name (`std::fs::rename(&tpath, &opath)`, `:2137`).
  - `STORE_ID`: same `OpenOptions` pattern (`:2821-2825`), then rename (`:2837`).
- VERIFIED: running server's umask is `0002` (`grep Umask /proc/3178352/status` -> `Umask: 0002`).
- INFERRED (from the two facts above): if the feature ran under that same process settings, directories would be `drwxrwxr-x` (0775) and files `-rw-rw-r--` (0664): readable by every local user, writable by the owner's group (group `drakestapleton`, plus anyone placed in that group). Readability by "others" is the part that matters for confidentiality. The final mode also depends on the parent directory's own permissions (section 5).
- VERIFIED: `create_dir_all` and `is_dir()` pre-checks accept an existing root as-is. If the root, `objects`, `tmp` or a shard already exists, its mode and owner are not checked or changed (`:2763-2793`).

## 4. Who and what umask the runtime runs as

- VERIFIED: `ss -ltnp` -> `127.0.0.1:18091 ... ("aien-serve-http",pid=3178352)`. `ps`: user `drakestapleton`, cmd `/home/drakestapleton/handoffs/aien-serve-http.rc-8c47c84 --engine omega --model-dir /home/drakestapleton/models/qwen3-30b-a3b-instruct-2507-fp8 --port 18091`.
- VERIFIED: `/proc/3178352/status`: `Uid: 1000 1000 1000 1000`, `Gid: 1000 ...`, `Umask: 0002`, supplementary groups include `988` (docker), `27` (sudo), `4` (adm).
- VERIFIED: it is not a systemd service. Its cgroup is `.../app-t3code.slice/app-t3code-agents.slice/t3code-claude-746b41ef.scope` (a coding-agent session scope, started by hand/by an agent), cwd `/home/drakestapleton/workspace/sc-own30b-rc`. `grep -rl 18091 /etc/systemd/system ~/.config/systemd` found nothing relevant (only an unrelated unit file mentioned the number in a path search; no unit starts this server).
- VERIFIED: because it is a login-session child, its umask is inherited from that session (`umask` in my shell also prints `0002`). The other user services in `~/.config/systemd/user/` that set `UMask=0077` (e.g. `atlas-forgejo.service`) do not apply to it.
- VERIFIED: the server is reachable beyond localhost through a bridge: `socat UNIX-LISTEN:/tmp/own30b-bridge/s.sock,fork,mode=666 TCP:127.0.0.1:18091` (user drakestapleton) and a root `socat-host TCP-LISTEN:18091 ... UNIX-CONNECT:/b/s.sock`. The socket `/tmp/own30b-bridge/s.sock` is `srw-rw-rw-` inside `drwxrwxrwx`. This matters for who can send prompts (and so create cache files) but not for file permissions.

## 5. Candidate directories on the Spark (stat, read-only)

- `drwxr-xr-x drakestapleton /home/drakestapleton` (home is world-traversable: others can enter it).
- `drwxrwxr-x drakestapleton /home/drakestapleton/handoffs` (group-writable).
- `drwxrwxrwt root /tmp` (world-writable, sticky).
- `drwx------ drakestapleton /home/drakestapleton/.local/share` and `.cache` (owner only).
- VERIFIED: a cache root placed under `.local/share` or `.cache` would be shielded by those 0700 parents even with 0775/0664 inside. A root under `/tmp`, `handoffs` or the home directory would not be shielded.
- INFERRED: the repo's own tests and examples point at `/tmp/kv` (`kv_restart_test.rs:153,809`, `kv_park_restore_test.rs:914`); if an operator copied that, the files would sit in a world-writable parent.

## 6. Can symlink or path tricks redirect writes

Preconditions for all of these: an attacker who can already write inside the cache root or one of its parent directories (local user or group member on a loose directory). Names come from hashes, not requests, so a remote prompt cannot inject `..` or a path (VERIFIED, `object_path_for`/`tmp_path_for`). I did not find any code path that joins a request-supplied string into a path (INFERRED from reading those two functions and `from_fields`; I did not audit every caller).

- VERIFIED: the temp file is opened with `create(true).truncate(true)` and no `create_new` (O_EXCL) and no O_NOFOLLOW (`:1958-1962`). If a symlink already sits at the temp name, the open follows it and truncates and overwrites the target (as the server's user). The temp name is `tmp/<64hex>.<job_id>.<seq_id>.<epoch>.part` (`:611-614`), so it is guessable only if the attacker knows the chunk hash and counters (INFERRED: the hash is derived from the prompt tokens and the namespace, so someone who can guess a prompt can compute it, but the job id/sequence/epoch numbers also have to match; difficulty UNKNOWN).
- VERIFIED: `STORE_ID.part` is a fixed name (`:2817`), opened the same way without O_EXCL or O_NOFOLLOW (`:2821-2825`). A symlink planted at `<root>/tmp/STORE_ID.part` before the first start would be followed (writes 16 random bytes into the target). Only when `STORE_ID` does not exist yet.
- VERIFIED: `rename` onto the final object name replaces whatever is there, including a symlink, without following it (POSIX rename semantics; `:2137`). So a planted link at the final name is replaced, not written through.
- VERIFIED: reads of existing objects use `File::open(&opath)` (`:2388`), which follows symlinks. A symlink planted at an object name would be opened and its first bytes read; recovery and purge use `symlink_metadata` (`:1351`, `:1667`) and, for `tmp/`, only delete regular files ending `.part` and count others as foreign (`:1357-1359`). Content is checked by record validation (`validate_record` at ~736) before use, so a wrong file is rejected rather than served (INFERRED from the call sites; I did not run it).
- VERIFIED: if root or a shard directory is itself a symlink to elsewhere, `is_dir()` follows it and the store writes there (`:2763-2793`); there is no `canonicalize` or owner check on the root.
- VERIFIED: races are possible by construction: the code checks `is_dir()`/`exists()` then creates (`:2763-2776`, `:2806`), and writes temp then renames in separate steps. A directory swapped between steps by someone with write access to the root could redirect. Without write access to the root or its parents, none of these apply (INFERRED).
- INFERRED: the exposure is therefore local-user-only; there is no remote path from an HTTP request to a path component.

## 7. What is in a cached file (for sizing the confidentiality question)

- VERIFIED: a record is a 4096-byte header region plus a payload of KV tensor blocks (`record.rs:10-13`, `payload_len` field `:46`; header holds `model_sha256`, `token_count`, `chunk_tokens`, hashes).
- INFERRED: the payload is the model's internal attention key/value numbers for a conversation prefix, which are derived from the prompt text.
- UNKNOWN: whether the header or payload carries the raw token ids or text, and how easily the prompt can be reconstructed from the payload. I did not read the record writer in full and did not test inversion.

## 8. Not checked

- #487 file code; whether #488's `kv-store` crate (no `host_fs.rs`) creates any files (it is the contract and format only; INFERRED from file list).
- Any running instance with the feature on; none exists (section 2).
- Disk-full, quota and eviction behaviour (out of scope for exposure).
