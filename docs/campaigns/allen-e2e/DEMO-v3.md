# ALLEN end-to-end demo v3 (predeclared)

Status: PREDECLARED. Committed and pushed before any v3 run. v1 (FAIL, RESULT-v1.md) and v2 (FAIL,
RESULT-v2.md) stay as recorded and are not edited.

v3 is DEMO-v2.md unchanged: the same request text (including the stated format rules), the same steps
S0-S8 and S3-red, the same pass criteria, the same document checker C1-C5, the same models and the same
kill -9 restarts. Nothing in v2 is relaxed, reworded or added to.

## Why there is a v3
v2 failed at S3 and S8 before any model ran: the runtime refused the request because it could not
interpret "Start with a Markdown heading line that begins with "# "" as a checkable requirement
(RESULT-v2.md). That is a product gap, fixed in code by aien-sovereign-core#319, which teaches the runtime
to recognize and check a first-line markdown heading. v3 repeats v2 on the stack that includes that fix.

## When it runs
Only after #319 is merged to main. The receipts carry the stack label the driver computes
(`main at <commit> plus demo-only files`, or `candidate stack at <commit>, not main` if anything else
differs). Label real-CPU. real-GB10 and native-AIENOS: NOT_RUN. mock: not used.

## Driver
`DEMO_VERSION=v3 scripts/allen_e2e_demo.sh run`. v3 applies exactly the v2 rules and writes
receipts-v3.jsonl, artifacts-v3/ and RESULT-v3.md.
