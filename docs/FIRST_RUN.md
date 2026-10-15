# First run: from install to a recorded objective

This is the order a new operator follows on a machine where AIEN was installed from a build or a release archive. Every step below is read from the code or scripts named in brackets. Nothing here claims a public release exists (see the README for the current release state).

## 1. Install

`install.sh` builds from source, or with `AIEN_RELEASE_TAG` installs a published archive after checking its signature (see the README). It puts the `aien` command on your path. It does not provision a model and does not create the desk key.

## 2. Create the approval desk key (once)

```
aien compose desk-key --create 1
```

The desk key is how you approve what AIEN is allowed to do. `aien start` refuses to run without it and tells you this exact command [`commands.rs: handle_start_command`, `compose/bridge.rs: check_desk_key_at_start`]. The key is stored in the compose home: `$AIEN_COMPOSE_DIR`, else `$AIEN_RUNTIME_STATE_DIR/compose`, else `$XDG_STATE_HOME/aien-runtime/compose`, else `~/.local/state/aien-runtime/compose` [`spine.rs: compose_dir_from_env`]. The command prints the key id and path, never the key.

## 3. Provision the model

The installer ships no model weights. The daemon needs one `*.safetensors` checkpoint of a `LlamaForCausalLM` model, with `tokenizer.json` and `config.json` in the same folder [`commands.rs: CheckpointPolicy`].

The candidate this tree is released against is named in `release/candidate.toml`, table `[model]`: `model-id` (currently `unsloth/Llama-3.2-1B-Instruct`), the `snapshot`, and the sha256 of `model.safetensors` and `tokenizer.json`. Its licence (Llama 3.2 Community License) and its distribution note ("internal-only until an open-licence model passes") are in the same table. Put the files in one folder, for example `~/models/llama-3.2-1b-instruct/`.

Tell the daemon where they are, by one of (first match wins):

- `AIEN_MODEL_PATH=/path/to/model.safetensors`, and optionally `AIEN_TOKENIZER_PATH=/path/to/tokenizer.json` (default: `tokenizer.json` beside the model file);
- `AIEN_MODEL_DIR=/path/to/folder`, scanned for a `*.safetensors` file directly or one folder down;
- otherwise the folders `./models` and `~/models` are scanned.

A release build is strict by default: with no checkpoint found the daemon refuses to start instead of falling back to reference weights. `AIEN_REQUIRE_CHECKPOINT=0` or `AIEN_DEV_FALLBACK=1` are development switches, not for a real run [`commands.rs: CheckpointPolicy::from_lookup`, `strict.rs: production_strict`].

Checking the files: the daemon computes the sha256 of the model file and of `tokenizer.json` it loaded and records them in its generation records [`commands.rs: model_identity`]. It does not compare them to `release/candidate.toml` itself. You compare them: run `sha256sum model.safetensors tokenizer.json` and check the two values against `model-safetensors-sha256` and `tokenizer-json-sha256` in `[model]`. `scripts/whole_system_e2e.sh` makes the same comparison for its own runs. A mismatch means a different model than the one qualified; do not treat results as the candidate's.

## 4. Start

```
aien start
```

This starts the runtime daemon and waits for its socket. Without the desk key (step 2) it stops with a plain message naming the command. A missing model (step 3) is refused by the daemon it starts, with the reason printed in its own output, and `aien start` then reports that the socket did not become ready. `aien status` shows whether it is running.

Optional, persistent KV cache (off by default): set `AIEN_KV_STORE_DIR` to a folder before starting, and the daemon saves the key/value cache of each finished turn there and reuses it when a later prompt starts with the same text, also after a restart [`aien-runtime/src/shared_kv.rs`, `enable_persistent_kv`]. The daemon prints one `persistent KV:` line at start; the folder is scanned in the background, so the first requests after a start run as usual until the scan ends. A cache saved for other weights, another model configuration or another backend is never reused. A bad persistent-KV setting stops the start with the setting's name; the full list of settings is in `docs/persistent-kv/METRICS_AND_SETTINGS.md`. Leaving `AIEN_KV_STORE_DIR` unset changes nothing.

## 5. Record an objective

```
aien objective record --text "what you want done"
```

The record is written under the provenance folder, `objectives/<id>.json` [`aien-runtime/src/objective.rs`]. The folder is `$AIEN_PROVENANCE_DIR` if set, else `$AIEN_RUNTIME_STATE_DIR/provenance`, else `$XDG_STATE_HOME/aien-runtime/provenance`, else `~/.local/state/aien-runtime/provenance`. It is never `/tmp`. The daemon and the command line use the same rule; if they ever pointed at different folders an approval would be refused as `ObjectiveMissing`, so set the same environment for both.

## What is not covered here

`aien harness` is not shipped in this release and exits with a refusal.
