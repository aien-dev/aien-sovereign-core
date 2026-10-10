# WHOLE-SYSTEM-E2E, RUN-1 dry run 2: result

Rows: `ACCEPTANCE-v1.md` as amended by `ACCEPTANCE-v1.1.md` (both committed before the harness change, 620795e
before 92a3dbc). Harness: `scripts/whole_system_e2e.sh` v1.1 at 92a3dbc. Run folder:
`runs/RUN-1-dry-20261010T134202Z/` (16 chain receipts plus `chain/SHA256SUMS`, `VERDICT_ROWS.txt`, artifacts,
daemon logs, the two acceptance files as run). Host: the Spark, Linux, CPU-reference backend, CAND-4 model
(Llama 3.2 1B; weights and tokenizer digests equal `release/candidate.toml`, in every receipt). 2026-10-10 13:42:02
to 13:58:04 UTC. Milestone: R4 of aien-architecture#190. **No PASS claim for the program; no step of the contract
is claimed.** The run folder is kept exactly as written.

Digest note: `VERDICT_ROWS.txt` of this run carries `contract_sha256 5fd4b3d4...` (the contract at
aien-architecture 0631cf39), because the harness ran before aien-architecture#196 (contract wording, main 3c584a49,
sha256 `8ba65384...`) was merged. The E3 objective text in this run is already the #196 wording, so `objective_id`
`093e66ec92a6b428b49a0aa2a75678e9fdb155f423e1e7c71c5ae9fef11a60b1` is the identity the frozen contract will carry.
The harness now writes the `8ba65384...` digest (commit after this run); the run's own file is not changed.

## Rows as measured (OBSERVED from the chain)

| row | status | what the receipt says |
|---|---|---|
| PRE | PASS | backend line CPU-reference; model digests equal the candidate record; quiet flag not held |
| CTRL-E3a | PASS | desk required and no key: the daemon exited 1 before serving; log names `PROPOSAL_REFUSED NoDesk` |
| E1 | NOT_RUN | nothing installed in RUN-1 |
| E2 | NOT_RUN | harness-side objective receipt only; no installation-side record before work (lane L6, sc#380) |
| E3 | FAIL | the #196 wording is now accepted (both requirements recognized, none uncertain). Attempt 1: 243 s, 537 tokens, `unmet requirement: at most 200 words, found 339`. Attempt 2: ran out of the proposal budget at 356 s. No third attempt. The text named an invented `ws-contract.txt` ten times and none of the three real files |
| E3m | PASS | state DONE; `report.md` written in the workspace (177 words, first line `# Harbour, Orchard, and Windmill`, all three topics present); content sha256 equals disk sha256; second execute refused `AlreadySpent` |
| E4 | FAIL | `remember` ok (constraint record id 31); SIGKILL (exit 137); restart (`Reconcile: checked 0 unsettled`); second objective refused at propose: `reply cut at the token limit after 48 tokens (finish_reason max_tokens)` on all three attempts; `memory.state not_engaged`, `items_included 0` (ALLEN not engaged) |
| E6 | PASS | authorize, SIGKILL, execute while dead: no write; restart; same grant DONE once; third execute `AlreadySpent`; duplicates 0 |
| CTRL-E3b | FAIL | propose refused: `the proposal writes outbox/ctrl.md but the goal named the destination ctrl.md`; revoke never reached |
| NET | PASS | 0 established TCP sockets of the daemon |
| CTRL-E4 | PASS | compose store removed between stop and start: second objective refused with a named reason (`E_MARK_TRUNCATED`: journal holds 0 records, record mark says 76); `report.md` unchanged |
| E5, CTRL-E1, CTRL-E2, CTRL-E5, CTRL-E6 | NOT_RUN | lanes L4, L5, L6, L4, L2 |

Compared with dry run 1: E3m went from a propose refusal to PASS (topic wording), E4 and CTRL-E4 were reached for
the first time, E3 moved from a goal-interpretation refusal to a model-side failure.

## What follows (INFERRED, by the stated rules)

1. The chain and the harness mechanics hold for a second run: 16 receipts linked by `prev_receipt_sha256`,
   manifest, verdict rows. Run time 16 minutes, of which about 10 were E3 on the 1B model on CPU.
2. The engine does the core loop on this build when the objective is phrased in its recognized forms: propose,
   desk-approved authorize, execute to a file, spend-once (E3m), and that loop survives a SIGKILL between approval
   and effect (E6, second time). These are the mechanics of steps E3 and E6, not the contract objective.
3. Memory as the contract means it is not available on this configuration: `compose remember` stores, but
   `propose` only consults memory when an ALLEN identity is engaged, and the harness (like the ALLEN demo) runs with
   `AIEN_ALLEN_SUBJECT` unset. Filed as sc#386 with the second cause (48-token small-edit budget, the model re-emits
   the document instead of appending one line). E4 cannot PASS until the harness engages ALLEN or lane L3
   (aien-architecture#197) decides a memory path without identity.
4. CTRL-E4 PASS shows store-loss detection with a named refusal and no silent output. It does not show memory-loss
   detection, because no memory was being recalled (point 3). The control row stays PASS as pre-registered; its
   meaning is narrower than the contract intends and is recorded here.
5. The contract objective (E3) now reaches the model. Its failure is the known read-files gap (sc#382: the proposer
   cannot see the inbox, so it invents a file name) plus a length miss and the CPU time budget. On this model and
   backend, the 200-word document in one attempt is not reliable; RUN-2 (accelerator backend) or the public model
   (lane L10) measures whether that changes.
6. sc#383 item 1 (requirement phrasing) is closed by wording; items 2 and 3 (file names as write targets, folder
   destination) remain and still block CTRL-E3b.

## Not claimed

Nothing about an installed artifact, a public model, the accelerator path, native AIENOS, a hostile operator, the
model's prose beyond the checks, memory recall of any kind, or any step of the contract as PASS.

## Limits

One host, one model, one attempt per declared run (rule 7). `E3m` wrote `report.md` at the workspace root (the
objective did not name a folder). The `Revoked` refusal is still unexercised in this chain. The two dry runs share
one build (92a3dbc for the second; 3885600 for the first) and one job directory; nothing was installed.

## Next for lane L1

- RUN-1 proper needs: sc#382 (read files), sc#383 items 2 and 3 (CTRL-E3b), sc#386 (memory engagement and the
  append budget), and a decision on ALLEN engagement in the harness (depends on aien-architecture#197).
- The harness keeps every receipt field the verifier (lane L4, sc#381) will read; the hardcoded contract digest is
  the only field that changes when the contract is frozen.
