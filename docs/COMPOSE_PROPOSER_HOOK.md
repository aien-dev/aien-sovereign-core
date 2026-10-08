# Compose proposer hook (design note)

Scope: how an approved proposal from an upstream approval desk (INTERPLANE
`ComposeLedgerAuthority`, interplane#77) enters the production compose path.
Line numbers are for the commit that adds this note.

## Entry point
`ProposerHook::submit` (crates/aien-runtime/src/approved.rs:152) checks the
proposal (`verify`, approved.rs:109), refuses duplicates, then calls
`ComposeBridge::run_approved_task` (crates/aien-runtime/src/spine.rs:1362),
which is `run_task_inner` (spine.rs:1371), the body `RunComposeTask` runs
(server.rs:593 -> spine.rs:1351). The approved text is staged per task and
the registered model Skill (spine.rs:1224) returns it instead of calling the
`ComposeProposer` (spine.rs:1232); one attempt, the same template check, and edit
= None (spine.rs:1245): an approved proposal never goes through
`merge_edit_reply`, so an existing file is replaced byte-exact.

## Path to J-Space and World commit
`home.compose.run` (spine.rs:1412) -> `Compose::run`
(crates/aien-omega-compose/src/lib.rs:302) -> `rxc_host_run` in omega
`librx_compose.a` (omega.lock c0369e67, archive sha256 dfe0ffb8...): the Skill
result is staged on a J-Space branch, AEGIS calls the bridge's verify callback
(spine.rs:1273: result handle equals the Skill's text and it parses as one
file change), the winner commits through the World, Cortex records goal,
candidates, evidence and promotion (`cx_*` in `ComposeTaskReport`). The hook
returns Ok only if the run committed exactly the approved text.

## What the ledger hands over (`ApprovedProposal`)
request_id, trace_id, approval_id, approver, path, content,
approved_proposal_sha256, content_sha256, approval_mac, requirements, requirements_mac. `requirements` is the original goal text whose measurable
requirements the approved bytes must meet; `requirements_mac` is the desk key's HMAC over the approval binding and that text (tag `aien.requirements.v1`), so
a caller cannot swap or drop it. `Some("")` is a signed "no requirements"; a missing
`requirements` is refused (`RequirementsUnbound`), a goal with an unreadable requirement
is refused (`RequirementsUncertain`), and bytes that miss a requirement are refused
(`RequirementsUnmet`), all before any claim or compose run. A count such as "add 2 lines to NOTES.md" is judged on the diff between the existing workspace file (empty when absent) and the approved bytes. Existing lines must all survive, so such an approval can only add. Such a goal also needs `requirements_base`: the sha256 (hex) of the existing file as the desk saw it, or `absent`, covered by `requirements_mac` (tag `aien.requirements.v2`; v1 when no requirement depends on the file). It is refused when missing (`RequirementsUnbound`) or when the file differs (`BaseChanged`), and it is checked again under the compose-home lock when the grant is written, so a file that changed after the check gets no grant. Limit: a file over 8192 bytes (or not UTF-8) cannot be measured and is refused. The hook returns
`ApprovedComposeReport`: state (COMMITTED | ALREADY_COMMITTED), the same
ids, compose_proposal_sha256, grant_links, desk_key_id, approval_key,
replay_claim and the compose `task` report (None for ALREADY_COMMITTED); or
`ApprovedRefusal` {refused_by, name, detail, request_id, trace_id,
approval_id, replay_claim}.

## Daemon command (#249 C)
`ControlCommand::ComposeApprovedProposal { proposal, workspace }` on the
daemon socket (same-user peer check, 0600 socket, like every compose
command). Answered `ComposeApprovedResult(report)` or
`ComposeApprovedRefused(refusal)`. It runs `ProposerHook::submit`; there is
no unauthenticated variant. It writes nothing to the workspace. After the
replay commit the daemon itself writes the one grant for the approval
(`approved_grant` below) and returns its id (`approved_grant`) and target;
the caller then needs only `ComposeEffectIntent` and `ComposeEffectAck`,
both naming compose_proposal_sha256. A caller never writes its own grant
for an approved write.

## Daemon-written grant and workspace confinement (476ca4 A1/A1b)
- **Approved grant.** Kind `authorization`, field `approved_grant: 1`,
  keyed on `proposal_sha256 = compose_proposal_sha256`, naming `workspace`
  (canonical), `target = workspace/path`, `prior_sha256`, `approval_key`,
  `replay_claim`, `cx_promotion`, `cx_evidence`, `request_id`, `trace_id`,
  `approval_id`, `desk_key_id`, `approved_proposal_sha256`; Cortex links
  [cx_promotion, cx_evidence, replay_claim]. Reserved: `ComposeNote` refuses
  any authorization carrying `approved_grant`. At the intent the daemon
  requires the replay claim to be COMMITTED with the same approval key and
  commit evidence naming the same proposal, promotion and evidence, and the
  record to link all three. A crash between the replay commit and the grant
  leaves the approval spent with no grant: no effect, a new approval is
  needed (`approved_crash_test` boundary 3).
- **Confinement, every grant.** `effects::confine_target`: the workspace is
  an absolute canonical directory other than `/`; `path` has plain
  components only; `target == workspace/path`; the target's directory
  resolves (symlinks followed) to itself inside the workspace; an existing
  target is a regular file, never a symlink. Checked when a grant is written
  through `ComposeNote` and again at every `ComposeEffectIntent`. A grant
  with no `workspace` opens no intent. `aien compose authorize` now writes
  `workspace`; older unspent grants without it are refused (fail closed).
- **Closed (sc#261): only daemon-minted grants are honoured.** `ComposeNote`
  refuses every `authorization` record, whatever it says (also inside
  `ComposeBridge::note`, so no in-process path writes one). Two grant kinds
  exist, both written by the daemon:
  - `approved_grant`, from `ComposeApprovedProposal`, backed by a COMMITTED
    replay claim and the desk-key MAC (above);
  - `minted_grant`, from the new `ComposeAuthorize` command that
    `aien compose authorize` now sends. When the daemon's own compose run
    commits a one-file proposal on the ordinary path it writes a reserved
    `compose_commit` record (promotion, evidence, proposal digest, path,
    content digest, canonical workspace). `ComposeAuthorize` names only the
    promotion, the proposal digest, the workspace the operator expects and
    the approver; path, content and target come from the commit record, so
    the caller never supplies them. It is refused when no commit record
    matches, when the workspace differs from the one the compose ran for,
    when `confine_target` fails, or when an earlier grant for the same commit
    is still live or has an unsettled intent. Revoked, stale and settled
    (NOT_DONE) grants do not block a new one; each grant is spent once. After a
    grant for a commit settled DONE, no new grant is minted for that commit:
    one committed proposal gives at most one DONE effect (new content needs a
    new compose).
    At `ComposeEffectIntent` the grant is checked against its commit record.
  - Old records: an `authorization` record with neither marker (written by a
    client before this change) is read but never honoured. It opens no intent;
    an intent it opened earlier is settled UNRESOLVED with `disk_error`
    `NotDaemonMinted` (the world check never yields DONE or NOT_DONE for it),
    and an operator cannot declare it DONE (an operator `--declare not_done` is
    still accepted). An old committed proposal has no commit record, so it must be
    proposed again before it can be authorized.
  - The approval in the ordinary flow carries the desk-key MAC by default
    (sovereign-core #297, required by default since #328, Drake decision b,
    2026-10-08): `aien compose authorize ... --desk 1` signs it, and an
    authorize without it is refused (`DeskMacRequired`). The daemon refuses to
    start when the desk key is missing (`aien compose desk-key --create 1`
    makes it). The only way back to the OS-user-only authorize is the dev
    opt-out `AIEN_COMPOSE_AUTHORIZE_REQUIRES_DESK=0`, accepted only in a dev
    run (`AIEN_DEV_FALLBACK=1`), refused in a strict run (which release
    qualification is), and announced at startup as
    `Authorize MAC: OFF, DEV OPT-OUT`. Code that embeds the runtime and builds
    a bridge with `with_authorize_requires_desk(false)` is held to the same
    rule: the server refuses to start with it in a strict run (the `aien
    daemon` program never builds one). Bridge-level tests of the legacy path
    call the bridge directly, without a server. Also, a
    grant can exist only for content the daemon itself committed, at the
    workspace and path the daemon recorded, and only the daemon writes it.
    The approved-proposal path keeps its MAC.
  `aien compose desk-key [--create 1]` prints the
desk key id and path (never the key) and creates the key if none exists.

## Authentication design (#249 A)
Reused primitive: none of the existing grant records can carry it. A
`ComposeNote` `authorization` grant is written by any same-user socket
caller, so binding the approval to one would be caller text in two steps;
the aien-mcp `ApprovalGrant` lives in the INTERPLANE process memory and the
daemon cannot check it. So the smallest verifiable evidence is used:
`approval_mac` = HMAC-SHA256 (RFC 2104 over the existing sha2 crate, no new
dependency) keyed by the approval desk key, over the canonical binding
(`approved_auth::binding_bytes`): compact JSON, keys sorted, of
approval_id, approved_proposal_sha256, approver, content_sha256,
desk_key_id, path, request_id, trace_id, workspace and
`"v":"aien.approval.v2"`. `workspace` is the canonical absolute path
(symlinks resolved): the daemon canonicalises the command's `workspace`
and binds that, so a valid approval presented with another workspace fails
the MAC (`Unauthenticated`, nothing consumed; 476ca4 c17), and the daemon's
grant target is derived only from the bound workspace plus path. v2 because
the binding gained a field (nothing was released under v1).
The daemon recomputes it with its copy of the key and compares in constant
time; anything else is `PROPOSAL_REFUSED Unauthenticated`.
- Key: 32 bytes from /dev/urandom, `<compose dir>/approval-desk.key`,
  64 lowercase hex, created 0600 with O_EXCL|O_NOFOLLOW. Every submission
  re-loads it: a symlink, a non-regular file, another owner, any group or
  other permission bit, or a malformed key is `NoDesk`. Only desk_key_id
  (first 16 hex of sha256 of the key) is ever printed or recorded.
- Confinement: the compose home and the submission's workspace must not
  overlap (either inside the other), else `Confinement`: the model-facing
  tools read inside the workspace, never the key or the journal.
- What it proves: the holder of the desk key approved exactly these fields.
  Not which human pressed approve: the desk (INTERPLANE host-only approval
  continuation) names the approver, and the MAC stops anyone without the key
  from changing it. Caller text can never carry a valid MAC.
- Rotation / loss: replacing the key refuses every MAC made with the old
  one (approvals issued and not yet submitted must be approved again);
  claims already recorded keep their keys. Without a key every submission
  is `NoDesk`.
- Limit: the OS user is still the outer boundary; a process of the
  daemon's user that can read the compose home can read the key.

## Replay (#249 B, crate::approved_replay, session 476ca4)
`approval_key` = sha256 of the binding above; the claim keys are the
approval key, the request id and the approval id, each on its own. Order in
`ProposerHook::submit`: verify -> reconcile gate -> desk key -> confinement
-> MAC -> `claim` (durable `accepted`) -> `mark_in_flight` -> one
`run_approved_task` -> `commit` | `fail` (ran, not committed) | `uncertain`
(run error, or committed but not the approved text). A refused claim is
returned with its state; a retry of the very same approval (all keys equal)
whose claim committed gets the original result back as ALREADY_COMMITTED,
never a second run. Daemon start runs `approved_replay::reconcile_at_start`
after the effect reconcile (accepted -> not_executed, in_flight ->
uncertain); a failure gates effect commands and approved proposals.

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
`PROPOSAL_REFUSED` `Unverified` (hash, ids, path, template round trip),
`ReconcileFailed`, `NoDesk`, `Confinement`, `Unauthenticated` (nothing
appended for any of these); `REPLAY_REFUSED <Name>` from the claim
(AlreadyCommitted, AlreadyFailed, AlreadyConsumed, Uncertain, InFlight, ...);
after a claim: `ComposeError` and `Mismatch` (claim UNCERTAIN),
`NotCommitted` (claim FAILED); after the replay commit: `GrantNotWritten`
(the approval is spent, no grant, no effect). At the intent:
`EFFECT_REFUSED OutsideWorkspace`, and `NotAuthorized` for a grant with no
workspace or an approved grant without its committed backing. At ack and
reconcile the confinement runs again (476ca4 c28/c28b: a parent directory
swapped for a symlink after the intent): if it refuses, the state recorded is
UNRESOLVED with `disk_error` `OutsideWorkspace: ...`, never DONE, and an
operator declaration of DONE is refused.

## Deferred
- The effect receipt (`record_effect_receipt`) still has no request_id or
  trace_id (#76; see below). The replay `accepted` record carries
  `request_id` and `trace_id` (and `approval_id`, `approval_key`).
- Real model leg NOT_RUN; no GPU.

## Correlation for retained records (#76, design note only)
Today trace_id and request_id survive in: the command's report and refusal
(`request_id`, `trace_id`), the replay `accepted` record (Cortex host
record, kind effect, fields `request_id`, `trace_id`, `approval_id`,
`approval_key`), and the compose goal text (request only). Since the A1 fix the
daemon's approved grant carries `request_id`, `trace_id` and `approval_id`
and links the replay claim; the intent and ack records and
`record_effect_receipt` still carry neither.
Recommended: option B, bind rather than re-version: a verifier walks
ack -> intent (`authorization`) -> approved grant (`replay_claim`, Cortex
link) -> claim and reads trace_id/request_id from the immutable claim
record (and the grant text). Receipt files stay version 1. Done by the A1
fix: the grant links the claim; a grant whose claim is not COMMITTED with
matching evidence is refused at intent; the approved grant cannot be
forged. Tests still to add: walk from an ack recovers trace_id and
request_id; `record_effect_receipt` for approved writes names the grant.
