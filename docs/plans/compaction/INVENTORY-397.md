# Context compaction hardening (aien-sovereign-core#397): inventory and test matrix

Owner: campaign session 1ea888, 2026-10-10. Branch `fix/context-compaction-safety-397`.
Motus (PyPI `lithosai-motus` 0.4.3, `memory/compaction_*.py`) was read for ideas only: keep tool call and
result together, store originals before summarizing, mark summaries as derived. No Python enters this repo.

## 1. Entry points and state ownership (verified by reading the code)

| Site | What it does | Compaction? |
|---|---|---|
| `crates/aien-cli/src/main.rs`, interactive loop (`loop {` after the banner), after each user turn and after each assistant turn | the only production caller of `ContextCompactor::compact_if_needed`; the message list is a `Vec<serde_json::Value>` of `{role, content}`; the whole list is written to `basecamp/sessions/<session_id>.json` after every turn | yes, this PR |
| `crates/aien-cli/src/main.rs`, single-shot (`session-single-*`) and `auto-goal-*` modes | build their own short message lists; never call the compactor | no |
| `crates/aien-cli/src/hooks.rs` `AgentHook::on_compaction` / `run_on_compaction` | hook slot; no hook implements it and nothing calls `run_on_compaction` | no (unchanged) |
| `crates/aien-cli/src/client.rs` `stream_turn` | sends `role` and `content` only; extra fields on a message (`derived`, `authority`) are ignored by the wire | reads the list |
| `crates/aien-cli/src/client.rs` `extract_tool_calls` | the tool-call grammar (`<tool_call>` JSON, or a fenced JSON block); reused by the compactor to recognise an assistant tool request | shared |
| `crates/aien-runtime/src/context.rs` `ContextComposer`, `ContextRevision` | immutable tokenized revisions for the scheduler; a different object (token ids, not chat messages); not touched | no |
| compose daemon (`crates/aien-runtime/src/spine.rs`) | its own Cortex journal and record marks; not a chat transcript | no |
| `aegis-runtime/src/agent.rs` | read for the issue's "parallel compaction" question: at `aegis-runtime` main on 2026-10-10 there is no compaction routine there (grep `compact` finds nothing); AEGIS is out of release v1 (Drake, aien-architecture#190) | no |

Tool results in this loop are `role: user` messages whose content starts with `<tool_response name="...">`
(the tokenizer refuses a `tool` role). The compose daemon's approval waits never pass through this loop today;
the approval markers the compactor protects (`approval_pending`, `requires_approval`, `"status": "pending"`)
are a documented convention for the day they do.

## 2. What changed (`crates/aien-cli/src/compaction.rs`)

- Tool-aware groups: an assistant tool request plus its following tool results is one group; groups are
  pruned or recapped whole. A tool result with no request before it (detached) or a historical request with
  missing results (malformed) refuses the whole compaction with a named reason. The last two groups and
  everything from an approval wait onward are protected.
- Durable originals first: a `CompactionRecord` (every replaced message with its digest, the removed range
  with its originals, transcript digests before and after, `after_len`) is written to
  `basecamp/sessions/<session_id>-compaction/NNNN-<utc>.json` (dir 0o700, file 0o600, write to temp then
  rename) before the list changes. Write failure leaves the list untouched and is reported. `reconstruct`
  undoes one record and `CompactionStore::replay` undoes all of them newest first, verifying every digest;
  the current transcript may have grown after the compaction.
- Tokens: `TokenCounter` trait; `TokenizerCounter` from `AIEN_TOKENIZER_JSON` (model tokenizer, falls back
  per text on error); otherwise `CharEstimate` (one token per three bytes, rounded up, tested conservative).
  Non-string content counts from its JSON serialization; four tokens of overhead per message; configurable
  tool-schema overhead added once.
- Oversized single tool results (default over 24,000 bytes) are head and tail truncated wherever they are,
  with the original in the record.
- Trust: the recap is `role: user` for wire compatibility but tagged `derived="true" authority="none"` with
  `"derived": true` on the message; its lines have `<`, `>` and control characters removed, so a tool result
  cannot open a `<system>` or close a `<tool_response>` through the recap. Recap lines and stubs pass through
  the configured redactor (`vault::redact_secrets` in the chat loop).
- Statistics: `CompactionReport { applied, skipped: Option<SkipReason>, stats }` with token source, pruned
  and oversized counts, saved bytes, removed segments, protected groups, recovery pointer and digest. Skips
  are printed in the chat loop (except NotNeeded and TooShort), never silent.

## 3. Test matrix (`cargo test -p aien-cli compaction`)

| Acceptance point (issue) | Before (main, 1 test) | After (16 tests) |
|---|---|---|
| tool request + results retained or compacted as one group | not tested | `tool_group_is_atomic_and_originals_reconstruct` |
| pending tool call protected | not tested | `pending_tool_call_in_tail_is_protected` |
| interrupted or detached tool response | not tested (would be recapped) | `detached_tool_result_refuses` |
| multi-tool turn with missing result | not tested | `historical_incomplete_group_refuses` |
| approval-waiting trace stays intact | not tested | `approval_wait_protects_everything_after_it` |
| original bytes reconstructable, digests verify | impossible (originals discarded) | `tool_group_is_atomic_and_originals_reconstruct`, `oversized_result_in_tail_is_truncated_and_recoverable` |
| compaction refuses without durable record | no | `no_store_refuses_lossy_compaction`, `store_write_failure_leaves_list_untouched` |
| restart and replay reproduce the transcript | no | `replay_after_restart_rebuilds_two_compactions` |
| hostile tool text cannot forge authority | no | `hostile_tool_text_cannot_forge_authority_in_recap` |
| tampering detected | no | `tampered_record_is_rejected` |
| no secret in compaction output; originals kept protected | no | `secrets_are_redacted_in_recap_and_stubs_but_kept_in_record`, `vault_redactor_strips_known_patterns` |
| budgets enforced with mixed content | no (non-string content counted as 0) | `mixed_content_counts_toward_budget`, `estimate_is_conservative_against_known_counts` |
| semantics not only size | size only | the tests above assert structure, protection and reconstruction |

## 4. Limits (stated, not hidden)

- The interactive chat loop is the only exercised entry point. The compose daemon and AEGIS are unchanged.
- The tokenizer path is exercised only when `AIEN_TOKENIZER_JSON` is set; the tests run on the estimate.
- Records are retained until the operator deletes the session folder; no automatic retention policy.
- Redaction uses the existing vault patterns; a secret the vault does not know and that matches no pattern
  is not redacted from a recap line (it never leaves the compaction record otherwise).
- The recap keeps `role: user` because the wire sends only role and content; the derived marker is in the
  text and on the message object.
