# ADR 0002: aien-mcp mints AuthorizedEffect only through a host-supplied EffectAuthority

Status: Proposed (2026-10-04)

`AuthorizedEffect` had no production constructor. This change adds one, and only one:
`EffectLane::authorize` (and `authorize_and_execute`) in `crates/aien-mcp`. The lane builds an
`AuthorityContext` from the live session (descriptor, catalog digest), asks an `EffectAuthority`,
and mints through a crate-private function on `Allow` or `AllowRestricted`.
`RequireApproval` returns `Pending` carrying the intent digest. `Deny` and `Contain` return
errors. A stale or unknown tool is denied before the authority is asked or by the authority.
Nothing is persisted: aien-mcp does not wire the aegis-runtime `Approval` types, so no approval is
created or consumed here.

`AuthorityDecision` mirrors aegis-runtime `DoctrineDecision` one-to-one. aien-mcp cannot depend on
aegis-runtime, so the shape is repeated, not extended. Consolidating `SafetyEngine`,
`ProbePolicyGuard` and `DoctrineDecision` into one authority is a separate AIEN milestone; this
change adds no new policy language.

`EffectClassAuthority` decides only from the `ToolEffects` bits the catalog already declares:
`EXTERNAL_IRREVERSIBLE` Deny; `EXTERNAL_WRITE`, `WORLD_MUTATION`, `SECRET_BEARING`,
`SPAWN_PROCESS` RequireApproval; `PURE`, `READ_FILESYSTEM`, `READ_NETWORK`, `LOCAL_EPHEMERAL`
Allow; unknown tool or unknown bits Deny.

Known limit: `EffectAuthority` is a public trait, so whoever holds an `EffectLane` and can write
an implementation can supply an allow-all authority. The seam keeps the intent from being
self-authorizing; it does not by itself stop a host that chooses a permissive authority. Binding
the authority to the AIENOS capability root (ADR 0016) is future work.
