# Receipt cross-check fixtures

Receipts minted with aien-proof (`aien_proof::evidence`) for the cases the Omega
C reader's three fixtures (`omega/tests/resolve/receipts`) do not cover. Those
three are all `kind` `component-qualification`, tier `HOST_TEST`, mutation
`NONE`, no lease, no ledger. These cover what they leave out:

- a non-NONE declared and observed mutation, including observed below declared
- a lease binding and a ledger binding
- `kind` equal to a verifier profile (`host-v1`, `qemu-v1`, `production-v1`),
  which is what SPEC 5.1 rule 5 needs (`receipt.kind` equals `verifier_profile`)
- tiers above HOST_TEST: QEMU, MACHINE1_ATTENDED, PRODUCTION (with authority)
- lists in non-canonical order with duplicates and mixed digest spellings (the
  id is over the sorted unique form, so a reader that does not sort gets
  another id)
- four receipts a profile-aware reader must refuse

Each file is `<receipt id>.json`, the same layout as an aien-proof store
(`<store>/receipts/<id>.json`). `expected.txt` has one line per file:

```
<id> name=... kind=... tier=... result=... mutation=<declared>/<observed> lease=yes|no ledger=yes|no verify=<status> with_store=<status> accept=ACCEPT|REFUSE
```

## What a reader must do

`accept=ACCEPT` means: the id recomputes from the canonical bytes, the
single-receipt consistency rules pass (`verify=PASS`), and the receipt tier
satisfies the minimum tier of the profile named by `kind` (host-v1 needs
HOST_TEST, qemu-v1 needs QEMU, production-v1 needs PRODUCTION, compared with
`aien_proof::chain::tier_satisfies`). `accept=REFUSE` is the opposite:

| Fixture name | Why refused |
|---|---|
| host-v1-result-fail | result FAIL (`verify=FAIL`) |
| host-v1-hardware-without-lease | hardware tier PASS with no lease (`verify=INCOMPLETE`) |
| host-v1-test-only-trust | TEST_ONLY_TRUST satisfies nothing but itself |
| production-v1-on-qemu-tier | production-v1 needs a physical tier, QEMU is not |

## Known limit: ledger and lease bindings

Receipts with `lease=yes` or `ledger=yes` have `with_store=ERROR`: aien-proof
`verify_with_store` also checks the referenced ledger event and lease hold
against a proof store, and these fixtures point at events that exist in no
store. The Omega C reader does not check those bindings (stated limit in
`omega_receipt.h`), so for it the expected result is ACCEPT. A reader that does
check them must refuse these files without a backing store. The id covers the
binding fields either way: changing the lease or ledger fields without
re-minting breaks the id (`changing_any_bound_field_breaks_the_id`).

## How Omega imports them

Copy this directory next to `tests/resolve/receipts`, run the C reader on every
`<id>.json`, and compare with `expected.txt`: the id check and the consistency
rules must give `verify`, and the profile tier rule must give `accept`. Do not
edit the JSON by hand: the file name is the id and the test below pins the
bytes.

## Regeneration and tests

`cargo test -p aien-closure --offline --locked --test receipt_crosscheck`
checks that the committed bytes equal what the test mints (deterministic:
fixed timestamps, commits and digests), that every case is decided as the table
says, and that changing any of 17 bound fields (kind, mutation, lease, ledger,
authority, assertions, lists, ...) in a fixture breaks the id.

To rewrite the files after a deliberate change:
`cargo test -p aien-closure --offline --locked --test receipt_crosscheck -- --ignored regenerate_receipt_fixtures`.
