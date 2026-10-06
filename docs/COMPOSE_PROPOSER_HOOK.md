# Compose proposer hook (design note)

Scope: how an approved proposal from an upstream approval desk (INTERPLANE
`ComposeLedgerAuthority`, interplane#77) enters the production compose path.
Line numbers are for the commit that adds this note.

## Entry point
`ProposerHook::submit` (crates/aien-runtime/src/approved.rs:152) checks the
proposal (`verify`, approved.rs:109), refuses duplicates, then calls
`ComposeBridge::run_approved_task` (crates/aien-runtime/src/spine.rs:1216),
which is `run_task_inner` (spine.rs:1225), the body `RunComposeTask` runs
(server.rs:593 -> spine.rs:1205). The approved text is staged per task and
the registered model Skill (spine.rs:1087) returns it instead of calling the
`ComposeProposer` (spine.rs:1092); one attempt, the same template check.

## Path to J-Space and World commit
`home.compose.run` (spine.rs:1266) -> `Compose::run`
(crates/aien-omega-compose/src/lib.rs:302) -> `rxc_host_run` in omega
`librx_compose.a` (omega.lock c0369e67, archive sha256 dfe0ffb8...): the Skill
result is staged on a J-Space branch, AEGIS calls the bridge's verify callback
(spine.rs:1127: result handle equals the Skill's text and it parses as one
file change), the winner commits through the World, Cortex records goal,
candidates, evidence and promotion (`cx_*` in `ComposeTaskReport`). The hook
returns Ok only if the run committed exactly the approved text.

## What the ledger hands over (`ApprovedProposal`, approved.rs:39)
request_id, trace_id, approval_id, approver, path, content,
approved_proposal_sha256, content_sha256. The hook returns
`ApprovedComposeReport` (approved.rs:57): the same ids plus
compose_proposal_sha256, grant_links and the compose `task` report.

## Hash semantics (fix)
- approved_proposal_sha256 = sha256 of compact JSON {"content","path"} with
  sorted keys (interplane#77). Checked, then carried for provenance.
- compose_proposal_sha256 = sha256 of the committed text
  `filename: <path>\n<content>`. The authorization grant and the
  `ComposeEffectIntent` must name this value: `aien compose authorize` does
  (crates/aien-cli/src/compose.rs:252) and `Ledger::check_intent` compares it
  (crates/aien-runtime/src/effects.rs:341). Grant links = [cx_promotion,
  cx_evidence], as compose.rs:271.
- Content the template parser would normalise (no final newline, leading
  blank line, code fence) is refused, so content_sha256 is the committed one.

## Refusals (never silent)
`PROPOSAL_REFUSED Unverified` (hash, ids, path, template round trip; nothing
appended), `Duplicate` (request or approval id already submitted in this
process), `ComposeError`, `NotCommitted`, `Mismatch`.

## Deferred
- No socket command: the hook is in-process only. A daemon command (and the
  `AIEN_COMPOSE_PROPOSER=interplane:<socket>` selection) is the next cut.
- Replay set is in memory; after a daemon restart a replay is stopped by the
  upstream desk (pending approvals are in memory) and by single-use grants,
  not by this hook. A durable request-id record is not written.
- The effect receipt (`record_effect_receipt`) still has no request_id.
- Real model leg NOT_RUN; no GPU.
