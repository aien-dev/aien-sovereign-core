# Daemon generation record

Evidence that this daemon generated a piece of text from the model files it
loaded. It exists so a provenance chain (INTERPLANE, interplane#76) does not
have to take the run driver's word for "this text came from this model".

## What the daemon does

1. At start, after the model loads, the CLI holds the sha256 of the model file
   (the single safetensors file, streamed in 1 MiB chunks, hashed once) and of
   the tokenizer file, with the path each came from. The daemon passes both to
   the server (`set_model_identity`). The same digests are in the
   "checkpoint loaded" log line.
2. When a `StreamTurn` finishes, and the compose ledger is open, the daemon
   appends one record to the ledger and returns its id in
   `TurnFinished.generation_record` (optional field; absent when there is no
   record, so older clients keep working).

## The record

An `effect`-class host note whose JSON has the marker field `generation`:

| field | meaning |
|---|---|
| `generation`, `v` | marker (1) and record version (1) |
| `model_sha256`, `model_path` | the loaded weights file's digest and canonical path |
| `tokenizer_sha256`, `tokenizer_path` | the loaded tokenizer file's digest and path |
| `prompt_ids_sha256` | `token_ids_sha256` of the submitted prompt token ids |
| `output_token_ids_sha256` | same hash over the generated token ids |
| `output_text_sha256` | sha256 of the exact `text` returned in `TurnFinished` |
| `total_tokens`, `output_tokens` | token count the engine reported; generated tokens |
| `finish_reason` | `eos`, `max_tokens`, `aborted` or `preempted` |
| `daemon` | `pid`, `start_ticks` (process start, clock ticks) and `started_unix_ms` |
| `request_id`, `operation_id` | the request envelope's ids, exactly as the client sent them |

The envelope ids are chosen by the client. They are recorded, not trusted.

## What it proves

The daemon process with that pid and start time generated text with that
digest, from prompt ids and output ids with those digests, while it held model
and tokenizer files with those digests. The record sits in the compose ledger, whose records carry digests the engine
re-checks on every read (`verified`), so an edited record is detected rather than
silently accepted.

## What it does not prove

- No capability, permission or authority. Nothing reads this record: no grant,
  intent, replay, commit or effect decision depends on it
  (`effects::generation_records_change_no_decision`).
- Not that the file on disk still has that digest later, only what was loaded.
- Not that the text is correct, safe or what an operator approved.
- A caller cannot write one: `ComposeNote` refuses kind `generation` and any
  `effect` note carrying the `generation` field.

## No record means no claim

The turn still succeeds, with `generation_record` absent, when:

- there is no compose ledger;
- the daemon has no model identity (a stub run, or the reference-weights
  fallback, or a tokenizer that did not load);
- the ledger append failed. The failure is logged
  (`generation record not written`); an id is never returned for a record that
  was not written.

A verifier must treat an absent record as an absent claim, not as a pass.
