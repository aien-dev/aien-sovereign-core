# AIEN 0.1 Native Golden Path: Release Gate

The 0.1 release is a single whole system acceptance test. It passes
reproducibly from a clean checkout, or 0.1 does not ship.

## Acceptance sequence

1. Boot from a clean checkout with one command. The daemon starts the native
   transformer backend, never the mock.
2. Load a real model on GB10. Blackwell path when hardware is present,
   explicit CPU fallback otherwise. The active path is printed, never silent.
3. Accept an operator prompt and retrieve Cortex memory for it.
4. Branch several agents with physical KV sharing (zero copy fork).
5. Stream tokens end to end through scheduler, KV, and backend.
6. Invoke one policy approved tool. It executes and records provenance.
7. Attempt one policy denied tool. It is blocked before any effect, with a
   reason returned to the operator.
8. Commit a World effect with provenance. The content hash is SHA-256 over
   canonical bytes.
9. Restart the process. Operator idempotency holds: replayed operation IDs
   are rejected, not re executed.
10. Shut down with zero leaked KV blocks and zero retained branch worlds.

## Current blockers

- PR 68 rebase: end to end completion primitives (`run_until_complete`,
  completion sinks, streamed routing). One file conflicts.
- PR 30 review: AEGIS pre-dispatch membrane wired for skills and shell.
  Mail, Git publishing, and network effects still need the same gate.
- PR 25 review: RSI envelope wired into promotion. Needs rebase and CI green.
- PR 74 split: cockpit rewrite must land as small scoped PRs, not one giant.
- PR 84 gate: mail send must sit behind the action policy boundary before it
  ships as agent capability.
- Packaging: one checkout, one command distribution. No sibling layout
  assumptions, no developer paths.
