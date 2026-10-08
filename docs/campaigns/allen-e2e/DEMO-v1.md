# ALLEN end-to-end demo v1 (predeclared)

Status: PREDECLARED. This file is committed before any demo run. A run that needs a change to these rules
records the change as v2 with its reason; it never edits v1 after a run.

Campaign 2, aien-architecture#162. Base: aien-sovereign-core main at the commit this file lands on.

## What the demo must show
One persistent ALLEN identity carried through the real chain, using the binaries built from this repository:

    request -> ALLEN identity and context -> inference (real model) -> capability grant -> authorization
    (desk MAC on) -> approved effect -> World commit -> memory (scoped) -> evidence -> restart -> same
    identity and state -> model swap -> same identity and state

No synthetic fixtures in the chain. Test helpers may create the clean state directory and send operator
input; they may not fake a model output, a grant, a ledger record or a receipt.

## Environment and labels (every receipt carries one)
- `real-CPU`: the daemon and CLI on the Spark host Linux, model on CPU. This is the label this demo targets.
- `real-GB10`: NOT_RUN in v1 (the GPU lane belongs to another session).
- `native-AIENOS`: NOT_RUN in v1.
- `mock`: not used anywhere in v1.
Every receipt records: repository commit, sha256 of the daemon and CLI binaries, the backend log line,
the model directory and its weights digest, the ALLEN LogicalAgentId, and UTC time.

## Models
- M-A: SmolLM2-1.7B-Instruct, `~/models/SmolLM2-1.7B-Instruct-31b70e2e869a` (read-only).
- M-B: Qwen3-4B-Instruct-2507, `~/models/qwen3-4b-instruct-2507-cdbee75` (read-only).
Weights keep their own licences (ATTRIBUTION.md); nothing is redistributed.

## Steps and pass criteria
S0 Clean state. A fresh state directory and compose directory. `AIEN_COMPOSE_AUTHORIZE_REQUIRES_DESK=1`.
   PASS: the daemon starts on M-A, the log shows `Authorize MAC: on`, and no ALLEN state exists before S1.

S1 Attach identity. Create or attach the owner's ALLEN through the existing identity path
   (`LogicalAgentId`, `AgentRoot`; no second identity system).
   PASS: `allen status` reports one LogicalAgentId, recorded as ID0.

S2 Personalize. Set `plain-language=1` in the profile. Put note N-work in context `work`:
   "The garden plan must mention tomatoes." Put note N-pers in context `personal`:
   "PERSONAL-CANARY-7741". Add goal G1 in `work`: "Keep the garden plan short."
   PASS: profile, memory inspect and goals list show exactly these, under ID0.

S3 Task on M-A, context `work`. Request: write `garden.md` in the workspace, a garden plan.
   Predeclared document constraints, checked by code after the commit (not by the model):
   C1 the file exists in the workspace and is valid UTF-8;
   C2 its first line is a Markdown heading starting with `# `;
   C3 it contains the word `tomato` (any case, any suffix);
   C4 it has at most 200 words;
   C5 it does not contain `PERSONAL-CANARY-7741` (context isolation).
   The write goes model proposal -> grant -> authorize with desk MAC -> intent -> write -> ack -> World commit.
   PASS: the commit is recorded DONE, the MemoryReport for the task shows context `work` with
   items_included >= 1, and C1-C5 hold. If the model output fails C1-C4, the step is FAIL, recorded
   verbatim; the demo never retries with a changed prompt in v1.

S3-red Negative control. The same checker runs on a hand-written document that breaks C3 and C5.
   PASS: the checker reports exactly C3 and C5 as failed.

S4 Provenance. PASS: the ledger links, for the S3 commit, the grant id, proposal sha256, content sha256
   (equal to the sha256 of `garden.md` on disk), model digest of M-A and ID0.

S5 Restart. `kill -9` the daemon after S4 (no clean stop). Start it again on M-A.
   PASS: `allen status` shows ID0; profile, notes and G1 unchanged; the S3 commit is still DONE and is
   not re-executed; `garden.md` is byte-identical.

S6 Forget. Forget N-pers (`personal`).
   PASS: memory inspect no longer shows it, and a recall in `personal` returns no item.

S7 Model swap. `kill -9` the daemon; start it on M-B.
   PASS: the backend line and the model digest changed; `allen status` shows ID0; profile, N-work and G1
   unchanged; N-pers still absent.

S8 Task on M-B, context `work`. Same request, writing `garden-b.md`, same constraints C1-C5.
   PASS: as S3, with M-B's digest in the provenance.

## Unresolved outcomes
Any step whose outcome cannot be reconciled from the ledger is recorded UNRESOLVED, never PASS or DONE.

## Report
One receipts file (JSON lines, one per step), plus a summary table: step, PASS/FAIL/UNRESOLVED/NOT_RUN,
evidence. The overall verdict is PASS only if S0-S8 and S3-red all PASS. A FAIL is reported as a FAIL.
