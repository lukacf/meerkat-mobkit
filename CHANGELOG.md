# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Fixed

- **Memory scope keys are LOGICAL identities end to end** (HomeCore
  activation smoke, launch blocker). The platform's observe-stream paths
  keyed identity scopes by the mob-plane roster id - for identity-first
  members the comms-safe encoding of a generated runtime alias
  (`mk--rt_cidentity_cparent-1_c0`), one per respawn generation - while the
  SDK `agent_memory` RPC, per-turn injection, and the recorder key by the
  logical `AgentIdentity` (`identity:parent-1`). Result: distiller-extracted
  memories landed in per-incarnation scopes invisible to injection and to
  SDK reads, fragmenting further on every respawn. Fixes, all through ONE
  normalization primitive (`member_comms_id::logical_memory_identity`,
  the decode-then-strip parser the dispatch-taint join introduced,
  relocated to the codec owner):
  - the member event observer fans out the LOGICAL identity to every sink
    (distiller/hygienist triggers now key the same scope the SDK reads;
    taint attribution becomes consistent with the dispatch-time join);
  - the distiller and hygienist trigger sinks re-normalize as a cheap
    fixed point, so no direct caller can re-split scopes;
  - the classic-path spawn customizer strips the generated runtime-alias
    shape instead of keying build injection per incarnation;
  - store migration `mobkit-memory` v3 (ledger-gated, data-only) folds
    existing runtime-id-keyed rows into the logical scope across records,
    proposals, pending promotions, the injection ledger, and the harvest
    queue (PK collisions collapse; merged scopes may briefly hold
    duplicate content until the next steward dream - rows keep distinct
    ids, nothing is lost), and normalizes the `MemoryScope` embedded in
    surviving stage-table batches' Create ops - stage tokens outlive
    boots inside the 24h GC window and operator-gated promotions commit
    later, so a key-column rewrite alone could be undone by a stale
    batch apply. Existing v2 files migrate on first open behind a frozen
    v2 fingerprint verifier.
  - Composition-time collision WARN: a host callback tool named `memory`
    now warns loudly against the agent-memory recorder at build
    customization (the overlay layer already warned-and-shadowed; the
    wire-declared surface now warns too, naming both tools and the
    remediation).

- **Repair honesty: the typed `ArchivedNotRevivable` refusal parks on the
  FIRST pass instead of heal-looping** (OB3 prod-data rehearsal at the
  0.8.14/0.8.10 pins, 4258-session corpus: 4 personal-agent identities whose
  latest sessions carry a 0.6.x body-written archived terminal with no
  runtime record entered an endless heal/refusal loop - continuity repair
  reported healed at the roster level, the next inbound turn re-hit
  meerkat's typed `SessionUnavailableForResume { reason:
  ArchivedNotRevivable }` at materialize and re-marked Broken, repeat, with
  user-facing canned failures and no convergence; the 0.8.10 N=3
  byte-identical-failure park never engaged because the heal itself kept
  succeeding). The refusal is a stable, deterministic materialize
  precondition no retry can change, so it is now classified typed
  (`ResumeRejectionKind::ArchivedNotRevivable`, from the error VARIANT,
  never wording) and both resume doors - eager restore and on-demand
  materialize - record the terminal `continuity_unrecoverable` verdict on
  the FIRST refusal, the same producer pattern as
  `CommittedBoundaryRepair::Unprovable`. The repair supervisor then parks:
  zero heal/re-Break cycles, no heal-authority calls, no reconcile churn,
  and the durable session (the transcript) stays bound and untouched. The
  eager restore outcome surfaces under the terminal
  `CheckpointUnrecoverable` kind rather than the reconcile-retried
  `ResumeRejected`. The verdict reason carries the operator path inline:
  upstream archived-session revive lands in meerkat 0.8.15 (promoted to
  upgrade blocker on real user data); until then `mobkit/reset` is the
  deliberate fresh start, and the park is process-local (a gateway restart
  re-attempts once after an upstream fix).

- **Memory-taint first-ingestion race closed at dispatch time** (§10.1
  launch-audit item). Content-trust classification no longer depends on the
  observe-only ASYNC agent-event stream alone: an LLM memory write in the
  same turn as the session's FIRST untrusted tool ingestion could reach the
  store before the taint observer processed the tool event
  (`llm_writes = "observed"` default), and MCP tools with unqualified names
  could not be attributed to a server at all (events carry only the tool
  NAME). Both are closed by a synchronous LLM-boundary join:

  - Every mob member session create (spawn, resume, revival - all funnel
    through the bootstrap spec's pre-build seam) now composes a
    taint-observing wrapper onto the member's agent-facing LLM client via
    `SessionBuildOptions.agent_llm_client_decorator`. Before each LLM call
    it classifies the tool results newly present in the request - joining
    the tool name against the request catalog's typed `ToolDef.provenance`
    (`ToolSourceKind::Mcp` + server id), so unqualified MCP tool names
    attribute correctly; absent provenance falls back to the existing
    name-based classification unchanged. After each call it classifies the
    typed `ServerToolContent` blocks (provider-executed web search /
    grounding) before the loop can dispatch any same-response tool call.
    An LLM-authored write is always downstream of an LLM call that carried
    the untrusted result, so the tracker is marked strictly before the
    write reaches the store's gate. Observe-and-mark only: the wrapper
    never denies, never mutates, does no I/O.
  - Mechanism note: meerkat 0.8.14 DOES fire
    `HookPoint::PostToolExecution` synchronously with the typed provenance,
    but the mob member build path has no hook-engine carrier
    (`SessionBuildOptions` cannot ship an `Arc<dyn HookEngine>`;
    `AgentBuildConfig.hook_engine_override` is reachable only from the
    standalone facade builder), so the join rides the sanctioned
    per-build LLM-client decorator seam - identical ordering guarantee
    for LLM-authored writes.
  - The tracker slot is late-bound (`MobBootstrapSpec::dispatch_taint_slot`):
    the unified builder and the RPC gateway fill it when the full
    agent-memory stack attaches, and members built earlier (bootstrap
    roster) pick it up on their next LLM call. Compositions without the
    taint firewall pay nothing. The async observer stays wired as
    belt-and-suspenders (it also serves session-rotation mirroring); the
    dispatch join simply gets there first.

## [0.8.10] - 2026-08-03

### Changed

- **meerkat dependencies repinned `=0.8.11` → `=0.8.12`** (all 19 meerkat
  crates in `meerkat-mobkit`, all 5 in `mobkit-store-conformance`). 0.8.12 is
  an upstream hotfix release: fan-in stale admission drops, abandonment
  flattening, the silent live-actor discard (the actor is now preserved on
  recoverable turn errors), transient-context transcript-identity false
  re-proof, terminal-receipt sequence reuse, and a live-boundary run-advance
  race. Full suite complete-run green against the registry crates before any
  mobkit-side change (2181/2181, idle gate excluded per its shared-runner
  disposition).

### Fixed

- **Continuity repair no longer destroys a member's queued work** (OB3 field
  runs 33758a41 + 6bb7010e: repair healed a stream-dead-but-Attached review
  member by full disposal and took its 15 pending fan-in inputs with it —
  irrecoverably, on the ephemeral runtime-store shape). Three changes, all in
  the identity-first repair paths:

  - **Queue carry.** Before the destructive retire, the repair captures the
    member's pending machine ingress (admitted-but-not-run inputs, payloads
    included) through the composition's runtime machine, and re-admits it
    into the healed successor session after the resume — fresh input
    identity, idempotency keys dropped (the originals were durably
    terminalized by the disposal), admission order preserved. Prompt, peer,
    and external-event inputs carry; flow-step and runtime-internal inputs
    cannot (their correlation is owned by the flow engine / runtime) and are
    DESTROYED LOUDLY instead — one warn line per input id, with the class
    and reason, before disposal proceeds.

  - **Preconditions first.** The collision-repair retire now proves the
    resume source exists in DURABLE, NON-ACTOR authority (the continuity
    store row, else the session service's archive-authority predicate)
    BEFORE destroying the stale member. An actor-routed read is never used:
    it waits on the very wedged member the repair is disposing (proven live
    at the meerkat 0.8.13 repin as a self-deadlock inside the probe). The
    raw injected SessionStore row is also deliberately not consulted; since
    the 0.8.11 store-owned repin runtime-backed compositions never project
    into it. A confirmed-absent resume source refuses typed with no
    destructive step taken: retiring would destroy the only live copy of
    the session. A probe fault means absence cannot be confirmed, and repair
    proceeds as before the guard existed.

  - **Bounded non-identical retries.** The continuity repair supervisor
    tracks per-identity failure signatures across passes; three consecutive
    byte-identical failures park the identity typed
    (`continuity_unrecoverable`, reason naming the blocking failure verbatim)
    instead of re-executing destructive repair steps on a timer. An operator
    clears the park to retry; a changed failure signature resets the streak.

- The three tests parked on the upstream meerkat-mob actor defect family are
  re-armed — fixed by meerkat 0.8.12 (the lost-actor cleanup): S2
  (`explicit_identity_query_refreshes_stale_existing_session_history`, the
  "autonomous dispatch inject failed: inbox closed" class), S3
  (`agent_events_route_resolves_durable_identities_and_aliases`, HTTP 500 on
  the member-observation route), and S4
  (`studio_k0_identity_first_gateway_retire_respawn_succeed_on_idle_members`,
  the intermittent retire/respawn hang). Each passed 3/3 re-arm runs at the
  0.8.12 pin (S4 solo, well under its historical hang horizon); the
  `#[ignore]` attributes and their run records are removed.

## [0.8.9] - 2026-08-02

### Changed

- **BREAKING: meerkat dependencies repinned `=0.8.10` → `=0.8.11`** (all 19
  meerkat crates in `meerkat-mobkit`, all 5 in `mobkit-store-conformance`).
  0.8.11 is the store-owned session-authority reset; the visible behavior
  changes for MobKit embedders:

  - **Resume authors nothing.** The transcript is ordered rows; System
    messages are ordinary ordered `Message::System` entries preserved
    byte-for-byte across resumes. The per-boot "resume-system-prompt-refresh"
    rewrite is retired (and with it the 60-100x transcript-history revision
    bloat it minted). Per-turn System instructions ride the new explicit
    carrier: `WorkSpec.system_prompt` through the mob deliver path
    (`SessionBridge::deliver_with_mode_context_and_system_prompt` →
    `RuntimeTurnMetadata.system_prompts`), appended as ONE ordered System row
    at that turn's boundary.

  - **Pre-ledger storage refuses typed.** Databases written before the
    mobkit 0.8.8 schema-ledger floor (no `meerkat_schema` row for an owned
    domain) are refused at open with rows left untouched — across continuity,
    memory, metadata, and console stores. The former silent pre-floor
    convergence is retired; upgrade stepwise through a 0.8.8-0.8.10 binary
    first.

  - **Released 0.8.10 session envelopes import exactly once on load.** The
    continuity adapter's load path is the sole importer
    (`import_released_0810_session`); ordinary decoding rejects released
    carriers. Zero-rewrite released histories (the universal mob-supervisor
    shape: transcript graph with zero commits and a singleton live-head body)
    import via the upstream d6cafd405 acceptance; divergent bodies still
    refuse.

- **Profile-declared fields are auto-marked as resume overrides** ("profile
  declares it, profile means it"). Durable session metadata restores `model`,
  `provider`, and `provider_params` on resume, so a profile edit was inert on
  every identity that already had a session unless the profile also listed the
  field in `resume_overrides` — two production fleets shipped model migrations
  that silently did nothing (one had identities running a three-week-old model
  until a provider byte cap broke the deployment). Now, at runtime bootstrap,
  every inline definition profile gets its explicitly declared fields marked
  resume-overridden automatically. `model` and `provider` are treated as a
  **coherent pair, never independently masked** (OB3 cutover incident: a
  model-only mask let the durable provider survive under a profile model it
  was never registered for, and the resume was rejected typed with an invalid
  `(model, provider)` pair): a declared `provider`/`self_hosted_server_id`
  masks the pair as written; a declared model with no provider key derives
  the provider from the canonical model catalog (or the definition's
  `[models.<id>]` entry) and writes it onto the profile so both apply
  together; when no coherent provider is resolvable, neither field is masked
  and durable truth wins whole (with a unified resume-divergence INFO line —
  once per identity per boot — printing both halves of the pair).
  `provider_params` masks independently when present. Undeclared fields keep
  durable-wins semantics, and an explicit `resume_overrides` list is
  preserved (declared fields are added, never removed). The same pair rule
  covers customizer/draft model pins: the pinned-profile spawn snapshot now
  carries the DRAFT model's catalog owner instead of a cleared provider,
  which on resume fell back to the durable provider under the pinned model —
  the same invalid-pair mint through a different door.

  **Migration note:** fleets that relied on durable-wins for a *declared*
  profile field — e.g. a profile that names `model = "x"` but expects resumed
  identities to keep whatever model their durable session recorded — must now
  express that per-identity intent through a draft/identity-level override
  instead of an outdated profile declaration, because the profile declaration
  now wins on resume. Realm-referenced profiles (`realm_profile = "..."`) are
  not auto-marked; the new resume-divergence INFO line is the tripwire for
  those.

- **The 0.8.8 observability lines are now visible at default configuration**
  (all four legs of the field investigation). (a) Both gateway binaries
  default their tracing filter to `warn,meerkat_mobkit=info,<gateway>=info`
  instead of a blanket `warn`, so this crate's own INFO reporting (conversion
  progress, continuity repair) passes at default config while dependencies
  stay at WARN; `RUST_LOG` still overrides everything. (b) The Python SDK now
  inherits gateway stderr by default (`MOBKIT_GATEWAY_STDERR=devnull` opts
  out; `MOBKIT_GATEWAY_STDERR_FILE=<path>` redirects). (c) The TypeScript SDK
  pipes gateway stderr to the host process's stderr by default, same opt-out
  shape. (d) `mobkit_gateway` installs its tracing subscriber BEFORE the
  storage maintenance verbs (`storage-migrate`, `storage-prune`,
  `storage-adopt-checkpoints`), so migration progress prints live instead of
  being dropped pre-init. A production deploy was aborted because a
  supervisor read a silent-but-working migration as a hang, and a week of
  panic-hook lines went to `/dev/null` — these four legs are why.

### Removed

- **BREAKING: checkpoint adoption tooling.** The 0.8.11 reset retired the
  checkpoint vocabulary meerkat no longer reads: the
  `storage-adopt-checkpoints` maintenance walk, checkpoint adoption module,
  digest-format marker stamping, and their tests are deleted.

- **BREAKING: storage doctor finding codes**
  `legacy-unverified-continuity-snapshots`, `checkpoint-digest-mismatch`, and
  `checkpoint-metadata-invalid` are removed (their subjects no longer exist at
  0.8.11). Replacements: `released-0810-continuity-snapshots` (censuses
  released-format rows awaiting one-time import) and
  `continuity-head-materialization-failed` (a head-canonical session whose
  slim materialization fails, now reported with strand, row counts, rewrite
  count, and prefix/anchor presence). Operator tooling keying on the removed
  codes must migrate.

### Added

- **Host-rejected builds park typed instead of Broken-with-continuous-repair**
  (the herd-investigation gate-rejection class). When a member build fails
  because the app-side `callback/build_agent` round trip COMPLETED and the
  host answered with an error (the candidate-mode effect gate), the identity
  now parks with a typed, operator-visible verdict on the first attempt:
  materialization fails fast with `HostRejectedBuild` (no bridge/callback
  churn) and the continuity repair supervisor skips the identity instead of
  retrying at 30s→10min forever — each retry previously re-asked the same
  deterministic gate the same question at the cost of a full member build
  plus a callback round trip. The park is scoped to the exact roster spec:
  a spec change (digest mismatch) clears it and re-admits exactly one new
  attempt, as does `clear_host_rejected_build_park` (operator retry).
  Transport-tier callback failures (closed transport, timeouts) are NOT
  parked — those stay on the existing retry lanes, whose backoff is fixed
  separately upstream.

- **Resume-divergence tripwire**: when a resumed identity's durable session
  metadata restores a `model`/`provider` that differs from the profile's
  declaration and no `resume_overrides` mask covers the field, the identity
  session bridge logs an INFO line with both values, the profile name, and the
  identity — once per identity per boot. After declared-field auto-mark the
  only case that can fire today is an inline profile whose declared model
  resolves no coherent provider; realm-ref profile declarations are NOT
  visible to the tripwire (the realm profile store is not threaded into the
  session bridge), so a realm-profile edit that loses to durable metadata is
  currently silent — threading the realm store is a tracked follow-up.

### Fixed

- **Live reset/reprofile no longer wedges the gateway when the superseded
  session commits a boundary mid-replacement.** Live reset replaces the
  identity's continuity record first and retires the superseded runtime
  through deferred cleanup debt - deliberately, per the reset contract (the
  old bridge projection is rollback authority until the replacement
  commits, and reset must not wait on a hung old retire). In that window
  the superseded session's runtime is still live, and any boundary it
  committed failed its durable write-through projection with the store's
  cursor refusal ("continuity record not found"); propagated, that failed
  the committing verb, escalated the runtime into repair-blocked retention,
  wedged the deferred retire behind it, and blew the gateway's bounded
  shutdown horizon (caught by PR CI's Python gateway test - the local
  pytest gap had hidden it). A record that names a newer binding for the
  same identity IS the supersede fact, discovered lazily under the identity
  fence, so the projection now drops such a write with the exact semantics
  the superseded-session pins already establish (terminal writes drop
  without parking) - with NO persistent supersede mark, so a reset that
  rolls back re-enforces the same session's writes cleanly. Every other
  projection failure stays fail-closed. Retire-before-replace was
  considered and excluded: it is structurally contrary to the reset
  contract's own pinned tests.

- **Zero-semantic-change boots no longer rewrite session heads (exactly-once
  adoption restored).** Upstream projects `ToolNameSet` (a HashSet) through
  serde when stamping `session_tool_visibility_state_v1`, so a session
  carrying a multi-tool Allow filter re-stamps the SAME visibility fact as a
  differently-ordered array every boot (per-process hash order), and the
  boot also touches `updated_at` - the strict exact-resave equality saw a
  changed head and rewrote it every boot (field: one fleet session's head
  churned bytes/checkpoint on every boot after its one-time adoption,
  violating exactly-once). The exact-resave equality now recognizes
  precisely those two facts as zero durable change: `updated_at`
  (timestamps are not durable content) and the ORDER of the
  tool-visibility Allow/Deny arrays (set semantics by the type's own
  definition; no other array in the document is touched). Pinned on the
  fleet's real consecutive-boot head rows and by a zero-turn eager-boot leg
  over the adopted closure that asserts ZERO head writes. Filed upstream:
  durable bytes minted from HashSet iteration order.

- **Released rewrite-carrying heads adopt on the first projected write
  instead of dead-ending the boot.** A released 0.8.10 head that RETAINS
  REWRITES structurally cannot authorize a current mutation: its
  rewrite-generation authority predates the compact graph/rewrite-prefix
  carriers, so `session_head_cas_token` refuses it typed ("rewritten current
  head has no compact graph-prefix authority") and every ordinary write arm
  is unreachable. The head-lane import (below) made such corpora READABLE,
  but the first projected boundary write at boot - the resume-spawn control
  snapshot - still failed fleet-wide (17/17 identities degraded pending
  retry; fail-closed held, durable rows preserved). The write path now takes
  a sanctioned adoption lane: the stored released document is re-proved
  through the one-time importer inside the write transaction (the import
  receipt is the authorization, never the released head), the incoming
  document must be a legal successor of that imported reading (genuine
  divergence refuses typed), and the released representation is replaced
  wholesale with the current-format layout - the same strand/rewrite/head
  writer the legacy-blob migration uses. Released heads with
  `rewrite_count = 0` (the committed R1 realms' shape) keep their ordinary
  write arms unchanged. Transcript-rewrite commits over a released head
  remain fail-closed by design: at 0.8.11 resume authors nothing, so the
  first projection always adopts before any rewrite can arrive.

- **Released 0.8.10 HEAD-CANONICAL continuity documents import on load**
  (the head-row lane of the released-envelope import). The one-time importer
  covered whole-blob rows only, while every 0.8.10-written head row carries a
  v2 session envelope that current materialization refuses typed
  ("failed to restore session from head row: ... expected current 3, got
  2") - so an entire released head-canonical fleet was unreadable at resume
  (field: 17/17 identities degraded on the reset-mint leg; the fail-closed
  side held, durable rows preserved). `materialize_slim_in_txn` now routes a
  v2 head into `import_released_head_in_txn`: the exact durable strand rows
  are first proved against the released head commitment
  (`released_0810_transcript_serialized_rows_digest` must equal
  `head_revision`), the released envelope is reassembled from those exact
  bytes plus the head's inline envelope facts, and the sanctioned
  `import_released_0810_session` boundary interprets it (receipt digest and
  session id re-proved). Read-side interpret only: durable adoption follows
  the first write-path decode, which rebases the strand under a
  current-format head. Regression drives the committed released baseline
  realm with its runtime store deleted - HomeCore's exact binding leg.

- **A restored/rolled-back runtime store no longer serves stale committed
  authority over the newer durable continuity row** (advisory Form 1, the
  0.8.9 stale-runtime-snapshot failure recurring at the 0.8.11 store-owned
  repin). The write-through projection makes "durable strictly newer than
  committed runtime authority" impossible in normal operation, so observing
  the inversion proves runtime-store loss; the facade now probes once per
  runtime per process on the store-owned read verbs and re-seeds the
  committed runtime authority from the durable session row (ordering on the
  monotonic pair: transcript rewrite generation, then message count, so a
  compacted-shorter durable document still orders ahead). Resume replays the
  full committed head instead of silently dropping durably recorded turns,
  and the following boundary can no longer project the regression back over
  the durable document. Catalog entries carrying a lifecycle terminal are
  left untouched.

- **A divergent runtime reseed refuses typed instead of silently adopting
  the durable side.** The staleness freshen above initially left fork
  detection to the inner store's seed verb, but `commit_session_snapshot`
  consults its LEGACY previous-row table for the boundary save guard - empty
  on 0.8.11 store shapes - so a durable row that ordered newer WITHOUT
  extending the committed document was classified as first-save adoption and
  silently replaced the committed runtime authority (the pick-a-winner
  data-loss class; fleet operators restoring runtime stores from backups hit
  this path). The boundary save guard now runs facade-side against the exact
  committed snapshot before any reseed: genuine divergence surfaces the
  guard's typed continuity refusal, repeatably, with both documents left
  untouched. Caught by the direction pin written for the freshen, not by the
  suite.

- **Archived sessions read as archived through the resume seam again.** At
  meerkat 0.8.11 the archive protocol never rewrites session BODIES to carry
  archive authority - the absorbing terminal is a RuntimeStore-owned fact
  (catalog entry or Retired/Destroyed lifecycle row) - while the mob resume
  seam still classifies from the body terminal, so a retired member's intact
  preserved document read back `Revivable` with no archived terminal (the
  0.8.6 field failure shape: hosts rotated identities off preserved
  transcripts). The session-service facade now overlays the store-owned
  terminal onto `load_session_for_resume` reads; both gateway binaries and
  the persistent library composition thread the shared runtime store as the
  overlay authority (`with_runtime_archived_terminal_authority` for
  externally-composed specs).

- **A byte-exact head resave no longer masks a stale fencing token.** The
  head-path exact-resave noop (which keeps a zero-change save from minting a
  checkpoint version) now requires the store-side
  `session_head_matches_current` probe - head equality AND current write
  authority, the head-path mirror of the whole-blob match probe's fence
  predicate - so a fenced-out writer falls through to the fencing write verb
  and hears the ordinary stale-fence refusal instead of a silent `Ok`.

- **An unregistered save refused over a durable head-canonical document now
  publishes positive lifecycle authority.** The refusal path recorded
  nothing, leaving the session's lifecycle to be re-inferred from absence; it
  now records `DurableObserved` (observation-grade: newer in-flight evidence
  wins), keeping the head-canonical refusal on the same lifecycle footing as
  the blob parking guard.

- **Ephemeral-runtime durability round trip is now facade-owned** (the OB3
  pod-scratch shape: durable truth in an injected session/continuity store,
  runtime store on ephemeral pod scratch). At 0.8.11 the session service
  keeps no plain `SessionStore` write path (WholeBlob session authority lives
  only in the `RuntimeStore`), so `SessionStoreBackedRuntimeStore` now owns
  both halves of durable interop: committed session boundaries WRITE THROUGH
  to the injected store after every committing verb (fail-closed: an external
  write failure fails the commit, and a retry of the same prepared boundary
  converges from the already-current inner successor), and cold activations
  RE-MINT store-issued runtime authority from the durable row (per-runtime
  single-flight fencing; a late seed never overwrites an advanced boundary).
  The mint arms on EVERY composition carrying a durable session source,
  durable SQLite/provider runtime stores included: an absent runtime record
  over a durable inner store is either a never-persisted session (the mint
  declines, the typed refusal stands) or a reset/lost runtime store — the
  sanctioned recovery path, which now reseeds instead of refusing every
  resume. Destroyed sessions cannot resurrect through it because identity
  deletion removes the durable row under the identity fence. Both gateway
  binaries and the persistent/identity-first compositions thread the same
  facade (`epoch_tracking_runtime_store_with_durable_projection`). On
  incremental-capable substrates the continuity adapter BIRTHS the
  head-canonical representation on a registered session's first projected
  boundary (strand rows + initial head under a create-CAS; legacy/imported
  blobs convert via the synthesizing head read), so the O(delta) steady
  state engages for new sessions exactly as it did when the 0.8.10 service
  drove the incremental channel - head birth moved seams, it did not
  disappear. The bounded-bridge composition (ephemeral session service +
  runtime machine) completes the post-commit boundary acknowledgement the
  upstream ephemeral service refuses on principle, so runtime-backed
  ephemeral turns no longer fail after their boundary committed.

- **Migrated head-canonical sessions no longer fail cold materialization
  after one 0.8.11 turn.** Both head-canonical plain-append writers (the
  continuity adapter and the local store) re-minted the head's byte-exact
  row commitment from TODAY's re-serialization while durable rows keep the
  bytes they were written with; `SessionHead::into_session` verifies that
  commitment at 0.8.11, so every fleet session migrated by the 0.8.10
  head-canonical bump would refuse to materialize after its first
  post-upgrade turn. The successor head now extends the STORED commitment
  with only the appended rows' bytes
  (`SessionHead::from_session_with_proved_inline_storage_authority`).

- **Deleting an identity now deletes its durable session row** (CAS-delete
  under the advanced identity fence inside the delete transaction, after
  projection writes are quiesced). Previously the row outlived the identity,
  and on the ephemeral-runtime shape the activation mint would faithfully
  resurrect the deleted transcript on the next cold boot or identity reuse.
  Stores that cannot support session-scoped deletion are surfaced loudly and
  keep the record-scoped deletion contract.

- **Schedule deliveries to identity-first members carry the driver's stable
  delivery identity.** The internal delivery sink rewrote the receipt
  correlation to the member id, which 0.8.11's `validate_dispatch_receipt`
  rejects — every intercepted member delivery errored AFTER the turn ran
  (false Misfire + `DriverTickFailed` per fire; duplicate turns under
  catch-up windows). The sink now echoes `ScheduleDeliveryIdentity`'s
  occurrence correlation exactly and stages the idempotency key on
  `DispatchInput`. NOTE: admission-boundary dedup for the internal member
  lane is still absent (the staged key has no reader yet) — crash-redelivery
  dedup is a tracked follow-up needing an upstream seam decision.

- **Sender-side conversation view now renders outgoing peer communications
  after a history rebuild.** An outgoing peer send exists in the sender's
  transcript only as an assistant tool call (`send_message` / `send_request` /
  `send_response`) plus a nameless tool result; the session-history projection
  kept assistant text blocks only, so any conversation rebuilt from session
  history (console log lost or reset, live projection never attached for the
  turn) showed the RECIPIENT's arrivals — incoming typed comms notices survive
  history — while silently dropping the SENDER's own outgoing communications.
  The history projection now re-emits the live edge's `tool_call_requested`
  frame ({id, tool_call_id, name, args}) for each comms send call, so the
  console pairs it with the backfilled tool result by `tool_call_id` and
  renders the same outgoing peer item as a live turn; a live twin collapses
  through a new tool-call arm of the history counterpart fingerprint (keyed on
  the provider-minted tool_use_id). Regression coverage runs the two-member
  scenario through both projection sources (live drain and history-only
  rebuild) in `unified_console`.

### Known Issues

- Gateway retire/respawn of idle members can hang intermittently (upstream
  meerkat-mob actor defect family, filed as S4 alongside S2/S3; admin-path
  operation, not exercised by fleet acceptance flows). The pinning test is
  `#[ignore]`d with its run records and re-arms on the upstream fix.

- Zero-turn boots re-stamp `session_tool_visibility_state_v1` from a
  HashSet (per-process order) and touch `updated_at` upstream, so session
  head BYTES are not boot-idempotent on their own - fleet operators
  comparing raw DB digests across boots will see byte drift on sessions
  carrying multi-tool filters. The scoped exact-resave equality (above)
  keeps the durable row untouched on zero-semantic-change boots; filed
  upstream as S5 (durable bytes minted from HashSet iteration order).

## [0.8.8] - 2026-07-29

Observability release. No behaviour changes to storage, resume, or provisioning
— every change here makes an existing path *report itself*. Cut in response to
two overnight field investigations where the platform behaved correctly but said
nothing, and the silence itself cost hours: one 42-minute diagnosis that a single
log line would have made instant, and one deploy abort caused by a supervisor
reading a silent-but-working phase as a hung one.

### Added

- **Head-canonical conversion of a legacy blob logs entry and completion**
  (identity, message count, strand/rewrite-row counts, elapsed). This one-time
  per-session conversion is the slowest phase of a boot on a large legacy
  document — minutes of CPU — and it previously emitted nothing at any level. A
  supervised deploy read that silence as a stalled candidate and aborted a live
  activation. A long migration is now visibly a long migration.

- **Deterministic cross-boot recall guards for the LAZY bootstrap arm**
  (`identity_first_lazy_recall_continuity`). Both existing continuity guards
  covered `EagerMaterialize` only, so `LazyMaterialize` and
  `LazyWithBackgroundWarm` — which register identities Dormant and defer
  materialization to first send — shipped with no proof that a resumed agent can
  see its own history. The new guards assert on the BYTES of the post-restart LLM
  request (never on model wording) and run in memo-free child processes, so
  process-global decode/digest/snapshot memos cannot fake a resume that a real
  `execve` would fail. Two further axes — resume of a pre-0.8.10-encoded document
  and a mid-turn kill — ship `#[ignore]`d with their arming requirements
  recorded: their fixtures fail loudly rather than certify a vacuous pass.

### Changed

- **The `no_incremental_channel` head-canonical read decline warns once per
  adapter**, then logs at debug. On a blob-canonical store the incremental
  channel is absent from construction, so the previous per-read WARN fired on
  every load of every session — 6,504 lines in one observed production boot,
  drowning the signal the level exists for. The genuine fault case (a
  head-canonical deployment losing its delta channel) still surfaces on the
  first line.

### Added
  O(delta) instead of rewriting the whole session document (M4b
  un-deferral).** `LocalContinuityStore` now advertises the session-delta
  channel (`ContinuityStore::as_incremental_sessions`), and head+rows become
  the canonical durable representation of a session in `continuity.sqlite3`:
  a new `continuity_session_heads` / `continuity_strand_messages` /
  `continuity_session_rewrites` trio (ledger `mobkit-continuity` v2), each
  row stamped with its owning `(identity, generation)`. Because
  `PersistentSessionService` routes every boundary save through the
  incremental branch once the capability is `Some`, an ordinary turn on an
  identity gateway now moves the turn's delta plus a few KB of head instead
  of reading, reparsing, re-serializing and rewriting the entire document.
  On the profiled HomeCore 82 MB session that removes the continuity half of
  the per-turn write amplification, including the whole-document decode.

  The canonical-representation rule is the same one meerkat-store uses for
  its own head rows, and it is enforced by every verb rather than asserted:
  **a `continuity_session_heads` row means head+rows are that session's sole
  byte authority and its `session_snapshots` row is a frozen archive that is
  never read or written again.** A whole-document save on a head-canonical
  session converts into delta rows plus a small head (plain-append when the
  incoming transcript extends the persisted strand, otherwise a `rebase:`
  strand) and leaves the archive byte-identical; `load_session_snapshot`
  serves the slim materialization; the whole-blob exact-bytes no-op probe
  declines; CAS tokens derive from head+rows; and delete, identity deletion
  and reset rollback scope all four tables (rollback still deletes only the
  attempted generation's rows, so a prior generation's head+rows remain the
  rollback authority). Guard semantics are meerkat's published validators
  verbatim (`validate_save_head_transition`,
  `validate_commit_rewrite_transition`, `strand_layout_for_history`,
  `reconstruct_rewrite_record`), so the accept/reject boundary of the store
  mirror cannot drift from the session service's.

  Every delta mutation carries the continuity write discipline the
  whole-blob path applies — fencing-token compare-and-set and per
  `(identity, generation)` checkpoint-version monotonicity, per append and
  per head write — through a new mobkit-internal
  `ContinuityIncrementalSessions` trait taking an explicit
  `ContinuityWriteCursor`. Rows, the head row and the `continuity_records`
  advance commit in ONE transaction, so a partial append can never leave a
  torn document and the durable cursor can never point past durable data.

  Pre-registration delta writes **park** in process memory instead of being
  refused: member creation provably saves before the identity runtime
  publishes the owning cursor, and the service fails closed on projection
  errors. Parked state is never durable, serves the service's continuity
  preflight and head-CAS reads, and flushes under the real cursor when the
  session registers. Parked rows are dropped only once they have been
  adopted durably: a flush failure — and equally a registration that lands
  between the service's append and the head write that adopts it, leaving
  rows no head claims yet — restores the registry and markers, RETAINS the
  parked rows, and refuses the registration, so the retry replays the write
  instead of losing it. Parking a write and publishing its footprint are one
  operation, so the footprint the "nothing was parked" purge relies on
  cannot drift from what the store holds; abandoning a session (delete,
  unregister, retire) reclaims its parked rows rather than merely unrouting
  them.

  **Upgrade note — the ledger bump is one-way, and deliberately NOT applied
  at open.** Launching this release over an existing state directory leaves
  `mobkit-continuity` at v1; until a write creates head state, older
  binaries can still read the file. The v2 stamp is committed either by an
  incremental session write that actually CREATES HEAD STATE — the
  head-canonical DDL and the stamp ride inside that write's own transaction,
  so a write refused by a guard or by its own CAS rolls both back and leaves
  the file at v1 with no head rows — or by
  `mobkit_gateway storage-migrate --apply` under the exclusive maintenance
  fence; `--dry-run` announces the pending bump. An ACCEPTED
  write that creates no head state does not arm it either: an
  `append_messages` whose adopting head write has not landed yet commits its
  rows and its additive DDL and leaves the ledger at v1, because rows no
  head adopts are not part of any document and an older binary still
  correctly serves that session from its blob. Once a head row exists,
  binaries older than this release refuse the file at open with
  `SchemaFromTheFuture`, and there is no in-place path back: recovery from
  a bad upgrade is restoring the state directory — `continuity.*` and the
  rest — from a consistent backup taken before the upgrade, accepting the
  loss of the turns taken since. That backup must be SQLite-consistent and
  capture committed WAL state (a stopped-gateway copy of each database with
  its `-wal` sibling, or SQLite's own backup API) — never a bare copy of
  the main database file alone, and never a read through `immutable=1`.
  Take it before upgrading. The doctor reports frozen archives under the new
  `continuity-archived-snapshot` finding and censuses head-canonical
  sessions with the same stamp verification the blob path pays. Frozen
  archives are reclaimable dead weight until an archive-prune verb ships;
  none does yet, so a migrated realm's `continuity.sqlite3` transiently
  carries both representations for each migrated session.

- **Durability declaration completeness is enforced at the storage seam.**
  `enforce_fail_closed_store_set` now requires exactly one
  `DurabilityDeclaration` for every slot in the new
  `REQUIRED_MOBKIT_DURABILITY_DOMAINS` (`continuity`, `event_log`, `console`,
  `metadata`, `blobs`, `agent_memory`, `schedule`); a provider that omits or
  duplicates a domain is refused at composition instead of silently passing
  the fail-closed durability rule. Mirrors the meerkat-level
  `REQUIRED_DURABILITY_DOMAINS` hardening.

- **`mobkit_gateway storage-migrate` / `storage-prune` — MobKit migration
  participation (storage-unification Phase M6).** New library module
  `meerkat_mobkit::storage_migrate` plus two offline maintenance verbs on
  the gateway binary (argv verbs beside `storage-adopt-checkpoints`; no RPC
  method — migration is never an eager side effect of gateway startup).
  `storage-migrate --state-dir <dir> [--apply] [--adopt <path>] [--json]`
  runs the five migration cases under `MobKitMaintenanceFence` — one
  `meerkat_sqlite::ExclusiveFence` per materialized database (meerkat's
  Phase 6 primitive; MobKit builds no second fence), sorted acquisition
  order, all-or-nothing RAII:
  1. **Ledger baseline (auto-safe):** every existing MobKit-owned database
     opens through its normal M3 constructor so `meerkat_schema` domains
     stamp (continuity, metadata, console, per-realm agent-memory files);
     dry-run is a read-only version matrix. The workgraph admission sidecar
     is exempt by design (no ledger — M3 decision, noted in the report);
     meerkat-shared databases (sessions/runtime/schedule/workgraph) are
     report-only and converge through the owning meerkat store's next open.
  2. **File-name unification (auto-safe, rename-only):** a lone legacy
     spelling (`sessions.db`, `sessions.sqlite`, `continuity.db`,
     `identity_continuity.sqlite`, `mobkit_metadata.sqlite`,
     `mobkit_console.sqlite`, `agent-memory-sqlite/`) renames to its M2
     canonical name **only here, under the fence** (the resolver never
     renames at open), moving `-wal`/`-shm` siblings with the database — a
     non-empty WAL is checkpointed first and the move refuses if it cannot
     be. Every rename writes a registered marker
     (`<legacy>.pre-<version>-<ts>.renamed`) so doctor lists it and
     retention tooling recognizes the transition.
  3. **Twin reconciliation (manual, fail-closed):** both spellings
     populated → per-domain divergence report (row-level by primary key +
     content digest for continuity/metadata/console; file-digest for the
     rest) and a typed refusal that fails the whole run closed.
     Byte-identical twins dedup under plain `--apply`; divergent twins
     resolve only with `--apply --adopt <path>` — the adopted copy keeps
     its place (renamed to canonical if legacy-named), the rest archived
     read-only. **No synthesis**: continuity fencing tokens and console
     `AUTOINCREMENT` cursors are per-database sequences.
  4. **Continuity checkpoint adoption:** H3's snapshot-adoption walk runs
     under the same fence pass (new
     `adopt_continuity_snapshots_already_fenced` composition seam) and its
     report merges into the migrate report.
  5. **Leftovers (report-only):** legacy sharded-FS blob files, the
     admission sidecar, `*.pre-*`/`*.corrupt-*` artifacts, and dead
     `tux-runtimes.json` entries (recorded pid no longer alive).
  `storage-prune --state-dir <dir> [--apply] [--older-than-days N]
  [--json]` owns the registered-artifact lifecycle (`*.pre-*` backups,
  `*.corrupt-*` quarantines; default threshold 30 days; deletion refuses
  anything outside the registered naming). Backup naming is shared with
  meerkat verbatim (`<original>.pre-<version>-<timestamp>[.<purpose>]`,
  reusing `meerkat_store::migrate`'s helpers), so HomeCore-class
  generation-cloning recognizes MobKit artifacts the same way — but
  recognition validates the COMPLETE generated shape
  (`is_registered_backup_artifact_name` /
  `is_registered_quarantine_artifact_name`: dotted numeric version,
  all-digit timestamp, exact `.corrupt-<digits>` suffix), never a loose
  `.pre-` substring, so prune and doctor can never claim user files like
  `notes.pre-release`. Exit codes:
  0 clean, 1 refusals/fence/store failures, 2 usage errors; dry-run
  default. `MobKitStorageMigrator` gains concrete `migrate`/`prune`
  methods (the meerkat-owned `StorageMigrator` trait stays diagnose-only).
- **`MobKitStorageProvider` — the composite storage-provider seam
  (storage-unification Phase M4b, the "one remote bundle").** New module
  `meerkat_mobkit::storage_provider`: a downstream backend (BigQuery,
  Postgres, object stores) implements ONE provider that wraps or references
  a meerkat `RealmStorageProvider` (`meerkat_provider()`, the
  meerkat-shared stores) and opens the realm-wide MobKit store set
  (`open_realm(&MobKitRealmOpenContext) -> MobKitRealmStoreSet`:
  continuity, lease authority — a `LeaseProvider` or the fencing floor to
  seed one — event log, console timeline, metadata, binary blobs, agent
  memory, schedule), each slot paired with a machine-readable
  `meerkat_core::DurabilityDeclaration`; `migrator()` exposes the storage
  maintenance hook. Composition is fail-closed
  (`enforce_fail_closed_store_set`): a `Durable` slot resolving
  non-persistent without an explicit ephemeral declaration is a typed
  startup error, never a silent in-memory fallback. The built-in
  `DiskMobKitStorageProvider` reproduces today's SQLite/object-store layout
  through the M2 layout locators and M3 ledgered openers, and returns
  `MobKitStorageMigrator` as its maintenance hook.
  `UnifiedRuntimeBuilder::storage_provider(...)` installs a provider for
  identity-first builds (requires `roster_provider()`; the provider's
  continuity store is the session authority; realm root from
  `persistent_state()` or `scratch_dir()`); supplying the provider together
  with a per-slot seam it subsumes is a typed conflict. The acceptance
  proof lives in `mobkit-store-conformance`
  (`tests/one_remote_bundle.rs`): an in-crate, in-memory-declared reference
  provider (`ReferenceMemoryBundleProvider`) passes the full MobKit
  conformance suite through the single seam and the meerkat conformance
  profiles (baseline, append-only, incremental, blobs, artifacts) through
  its `meerkat_provider()`.
- **New `UnifiedRuntimeBuilder` seams:** `schedule_store(Arc<dyn
  ScheduleStore>)` (the public schedule-store seam ob3 lacked — schedule
  tools attach over the caller's store on both the persistent and the
  ephemeral/scratch paths; library mode wires no firing host, and injection
  is a foundation: shadow-scheduler deletion downstream still requires a
  feature-parity audit), `binary_blob_store(Arc<dyn BinaryBlobStore>)`
  (typed raw-bytes blob injection that skips the base64 adapter round-trip
  on the byte-serving paths; mutually exclusive with `blob_store()`), and
  `ephemeral_runtime_store(bool)` (the explicit in-memory runtime-store
  declaration; see the fail-closed change below).
  `schedule_wiring::attach_schedule_tools_with_store` and the
  `*_reporting` variants of the schedule/workgraph attach functions are
  public for gateway-grade composition.
- **Per-slot storage census on the health surfaces.**
  `ResolvedStorageSummary` grows a `slots` array (additive wire change on
  the `"storage"` object of `mobkit/status` / `mobkit/capabilities` /
  `mobkit/storage/doctor`): every composed slot reports its domain,
  durability class, resolution (`persistent` / `declared_ephemeral` /
  `non_persistent`), backend, degradation flag, and detail. Declared
  defaults are now visible instead of silent (the ephemeral gateway's
  in-memory console/metadata, `mobkit_gateway`'s in-memory console/metadata
  contract, an unconfigured event log = "events are not ingested"), the
  schedule/workgraph boot-without posture is a health-visible degraded slot
  entry instead of a warn line alone, and the three in-process ring buffers
  (gating audit 512 / delivery history 200 / routing resolutions 512) are
  classified `Scratch` explicitly — a documented decision; a durable
  gating-audit slot remains a flagged follow-up, not part of this arc.
- **Incremental-continuity capability seam (typed, discoverable —
  deliberately deferred for the bundled store).** `ContinuityStore` gains
  `as_incremental_sessions() -> Option<Arc<dyn IncrementalSessionStore>>`
  (default `None`), and `ContinuitySessionStoreAdapter::as_incremental`
  genuinely forwards it: `Some` exactly when the substrate advertises the
  session-delta channel, wrapped so every delta mutation preserves the
  adapter's registration/suspension/supersede discipline under the same
  per-session lock as the whole-blob paths (H3 lazy adoption included).
  The bundled `LocalContinuityStore` deliberately returns `None`: shipping
  a delta channel beside today's whole-snapshot byte authority (exact-match
  probes, parse-derived CAS revision tokens, `checkpoint_session`, H3's
  adoption byte custody) would create two write authorities over one
  session in one store with no reconciliation rule. The store-side channel
  lands when the snapshot representation itself moves to head+rows, gated
  by the meerkat-store-conformance incremental profile; the H2 whole-blob
  health flag and the M0 canary stay pinned `false` accordingly.
  `GatewayContinuityStore` cannot advertise the capability (its callback
  wire protocol has only whole-snapshot verbs).
- `runtime_options` (rpc_gateway) wire additions: `event_log.storage`
  accepts `"null"` (explicitly declared dropped events; the existing
  `"memory"` form keeps working), and the new `runtime_store` key accepts
  `{"storage": "memory"}` — the explicit ephemeral runtime-store
  declaration. Unknown keys still reject.
- **SDK storage durability vocabulary (storage-unification Phase M5).**
  Python gains the `config.runtime_store` (`runtime_store.memory()`) and
  `config.event_log` (`event_log.memory(batch_size, flush_interval_ms)` /
  `event_log.null()`) modules plus the builder methods
  `.runtime_store(config)` and the extended `.event_log(config)` (the
  legacy `event_log(storage=..., **kwargs)` keyword form keeps working);
  TypeScript mirrors them as the `runtimeStore` / `eventLog` config
  modules (`eventLog.nullStore()` — `null` is reserved) and
  `.runtimeStore(config)` / the extended `.eventLog(...)`. Both SDKs now
  parse the storage census: `StatusResult.storage` /
  `CapabilitiesResult.storage` carry a typed `StorageSummary` (H1 blob
  durability, the H2 incremental probe, and the M4 per-slot
  `StorageSlotSummary` census), with forward-tolerant parsing;
  TypeScript additionally exports `parseStorageSummary` for the raw
  `mobkit/storage/doctor` `storage` record. Blob-store durability
  remains embedder-only (no wire declaration; census-visible read-only).
- **Typed fail-closed storage refusals: JSON-RPC error `-32014`
  (`STORAGE_RESOLUTION_CODE`).** The `rpc_gateway` init storage refusals
  — file-name twins the layout refuses to pick between, a
  session/runtime/blob/metadata/console store that failed to open where
  the silent in-memory fallback used to be, an uncreatable state root —
  now answer `mobkit/init` with `-32014` instead of generic `-32603`,
  and both SDKs reify it as the new `StorageResolutionError`
  (`RpcError` subclass). Both SDK bootstraps also stop rewrapping a
  received structured init error as a transport failure when the gateway
  exits after writing it (the fail-closed refusals do exactly that).
  Refusals raised inside `UnifiedRuntime::bootstrap` (Rust builder
  surfaces) still surface as `-32603` with the typed message text.
- **M5 storage anti-regression gate** (`meerkat-mobkit/tests/storage_gate.rs`,
  default test lane): production code may not resolve ambient roots
  (`$HOME`/`$XDG_*`/`$LOCALAPPDATA`/`$TMPDIR` reads, `env::temp_dir`,
  `dirs::`-style derivations) or spell the canonical database file names
  outside `src/storage_layout.rs`; legitimate uses are allowlisted with
  documented reasons (mobpack's skill/MCP-config home discovery and
  scratch outputs, the feature-owned schedule/workgraph constants, the
  doctor's legacy-spelling census). The sweep it enforces moved the
  mobpack flow-editor draft-store default off its private
  `$XDG_STATE_HOME`/`$HOME` derivation onto
  `storage_layout::default_gateway_home()` (same resulting path).

### Fixed

- **Idle busy-loop, part 2: an idle gateway now costs ~zero CPU.** On
  0.8.2–0.8.4 a fully idle durable member burned ~0.3 core indefinitely
  (~1.1 cores for the two-identity production dump; ~5 cores extrapolated
  for a 17-member host). The 0.8.4 schema-caching fix removed the
  amplifier; this release fixes the drivers, in tandem with meerkat 0.8.6:
  - **Event-driven stream reconcile.** The console forwarder and identity
    health monitor replaced their 250 ms polling ticks (each paying a full
    fleet-state projection per pass, per handle) with `ReconcileCadence`:
    wakes on tracked mobs' machine-state watches, the managed-mob-set
    epoch, the earliest subscribe-backoff deadline, and a 30 s safety tick
    anchored at the last completed reconcile (so sustained event traffic
    cannot starve it). Stream closures still reconcile immediately —
    repair latency is unregressed; only watch-invisible signals (lease
    fencing motion without a machine transition) moved from 250 ms to
    ≤30 s detection.
  - **Steady-state console discovery is I/O-free.** The 5 s session-history
    discovery loop re-read the FULL session document every pass (two
    whole-document sqlite reads + deserialize + rewrite-replay canonical
    digest + a watermark refresh write, per member — the dominant burn on
    82 MB production documents). Session-scoped write epochs
    (`SessionSnapshotWriteEpochs`, bumped on both sides of every runtime
    store write) now gate the backfill: an unchanged session costs one
    in-memory epoch compare. Witnesses are dropped on both unregister and
    register-replacement so a restarted runtime's counter can never
    suppress its first backfill.
  - **The epoch gate actually engages in the gateway binaries.** Both
    gateways compose their own stores and session services through
    `MobBootstrapSpec::new`, which leaves the write-epoch witness absent —
    so the gate above silently stayed disabled in every production
    deployment while the library-composed test stayed green. New public
    seam for externally-composed runtimes:
    `epoch_tracking_runtime_store()` + `SessionWriteEpochsHandle` +
    `MobBootstrapSpec::with_session_write_epochs()`; both binaries now
    wrap their runtime store and thread the witness on all launch paths,
    and the idle gate composes at gateway parity so composition-path
    divergence in this class cannot pass the suite again. Real-dump A/B:
    ~0.97 cores idle pre-fix → ~0.03 post-fix.
  - **Restore elision.** `restore_flow` returns an empty snapshot for
    already-active identities instead of re-running adoption work.
  - **Idle-CPU regression gate** (`tests/idle_cpu_gate.rs`) strengthened to
    the class that escaped every suite: one member's session grows to
    ~12 MB, the console aggregator runs exactly as the gateways run it,
    then a bounded quiesce probe precedes a 30 s window asserting ≤3 s
    process CPU (verified red on the pre-fix code: 11.6 s).
- **BREAKING for external `ContinuityStore` implementors relying on the
  old default: `rollback_continuity_record`'s compatibility implementation
  now actually restores.** The previous default restored the pre-reset
  record via `upsert_continuity_record`, which the trait requires to be
  generation-monotonic — so on every conforming store the restore path
  failed with `StaleContinuityGeneration` and only the delete arm
  compensated. The default now validates the attempt row, deletes it under
  the fence CAS, and re-upserts the previous record as a fresh insert —
  semantics a conforming store CAN satisfy. Documented caveats (why stores
  should still override with one atomic CAS transaction, as
  `LocalContinuityStore` does): the path is non-atomic, and
  `delete_continuity_record` removes the identity's session snapshots —
  including the previous generation's rollback-authority snapshots, which
  the atomic override retains. The
  `mobkit-store-conformance` `RollbackPath::CompatibilityDefault` chapter
  pins the fixed behavior (working restore + the snapshot-loss caveat);
  external stores that rely on the default and depended on the old
  always-fail behavior must re-run the suite.

### Changed

- **BREAKING / fail-closed (M4): the runtime store no longer silently falls
  back to in-memory.** All three composition surfaces
  (`MobBootstrapSpec::persistent*` and the builder's `persistent_state()`
  path, `rpc_gateway`, `mobkit_gateway`) previously answered a failed
  `runtime.sqlite` open with a `tracing::warn!` plus a silent
  `InMemoryRuntimeStore` — a degraded mode in which resume across restart
  and archive operations fail long after boot (the fifth silent fallback of
  the storage-unification recon). A failed open is now a startup error
  whose message names the remediation (fix the database file, or declare
  the ephemeral choice explicitly via
  `UnifiedRuntimeBuilder::ephemeral_runtime_store(true)` /
  `runtime_options.runtime_store = {"storage": "memory"}`).
  `InMemoryRuntimeStore` remains constructible by declaration only.
  Signature note: `MobBootstrapSpec::persistent` /
  `persistent_with_hook` now return
  `Result<Self, StorageResolutionError>` (a composite of the H1 blob error
  and the new runtime-store error) instead of
  `Result<Self, BlobStoreResolutionError>`.
- **REQ-23 lifted (M4): `persistent_state()` and an external
  continuity/lease pair may coexist.** The external substrate (or a
  `storage_provider()`) stays the identity and session authority —
  `ContinuitySessionStoreAdapter` remains the session store — while the
  state directory supplies the meerkat-shared local stores (runtime,
  workgraph, blobs, topology control). The genuinely contradictory
  combinations keep typed errors: `persistent_state()` + `scratch_dir()`
  (two path roots for one realm), and half an external substrate
  (`continuity_store()` without `lease_provider()` or vice versa alongside
  `persistent_state()` — fencing floors are store-coupled).

- **BREAKING (pre-1.0): the judgment plane is de-welded onto capability
  traits (storage-unification Phase M4).** The taint firewall controls, the
  Steward's dream read/write surface, and the console Memory panel's read
  API — previously inherent methods on the concrete
  `SqliteAgentMemoryStore` — are now capability traits in
  `meerkat_mobkit::memory::capabilities`: `TaintableStore` (the five
  firewall setters `set_llm_write_gate`, `set_llm_write_gate_if_absent`,
  `set_evidence_resolver`, `set_event_sink`, `set_event_sink_if_absent`),
  `StewardStore: StagedMemoryStore + TombstoneSource` (the nineteen steward
  read/write methods, including the dream ledger writers), and
  `MemoryPanelStore: StewardStore` (the eight panel-only reads). The full
  plane — firewall, Steward, Hygienist span source, Selector fetch, console
  panel — now runs against ANY `AgentMemoryProvider` advertising the
  matching capability accessors (`as_taintable`, `as_steward_store`,
  `as_memory_panel_store`, `as_selected_record_fetch`,
  `as_tombstone_source`; all default `None`). Breaking API notes for
  embedders under the pre-1.0 policy:
  - `AgentMemoryProvider::as_sqlite_store()` is **deleted** (the trait no
    longer names its own implementation); probe the capability accessors
    instead.
  - The promoted store methods moved from inherent impls to the trait
    impls: call sites on the concrete `SqliteAgentMemoryStore` now need the
    corresponding trait in scope (`TaintableStore`, `StewardStore`,
    `MemoryPanelStore` — all re-exported from `meerkat_mobkit`).
  - `memory_wiring::attach_memory_engines` takes
    `Arc<dyn AgentMemoryProvider>` instead of the concrete store, and
    `AgentMemoryStack` carries `steward_store: Option<Arc<dyn StewardStore>>`
    and `panel: Option<Arc<dyn MemoryPanelStore>>` instead of the concrete
    `store` field. Missing capabilities are named errors, never silent
    downgrades.
  - `UnifiedRuntime::{set_memory_panel_store, memory_panel_store}` and the
    console router plumbing carry `Arc<dyn MemoryPanelStore>` instead of
    the concrete store by value.
  - `StewardEngine::new` and `StoreSpanReferenceSource::new` take
    `Arc<dyn StewardStore>`; `EvidenceRefResolver` moved to
    `memory::capabilities` (re-exported from the old `sqlite_store` path,
    together with the promoted row types).
  Behavior notes: the gateway's Selector wiring now fails with "requires a
  provider with selected-record fetch support (SelectedRecordFetch)"
  instead of "requires the sqlite agent-memory store", and reuses the
  configured provider's fetch handle instead of opening a second concrete
  store on the same files; the Markdown provider stays recall-only *by its
  capability flags* (it advertises none), and explicitly configuring
  distiller/steward/hygienist against a recall-only provider now fails
  init loudly instead of silently constructing nothing.

- **BREAKING / fail-closed (H1): persistent-mode blob storage no longer
  silently falls back to in-memory blobs.** Previously, when a
  persistent-mode runtime (`MobBootstrapSpec::persistent*`,
  `UnifiedRuntimeBuilder::persistent_state()`) failed to open the local blob
  directory (`<state>/blobs`), it logged one warning and continued on an
  in-memory blob store — every blob written after that point silently
  vanished on restart (the cause of a month-long silently-broken production
  deployment). **Deployments that are unknowingly running on this fallback
  today will now fail to boot.** That is deliberate: the failure was always
  there, it is now visible at startup instead of at data-loss time.
  Remediation: fix the blob directory (permissions, mount, read-only
  filesystem), or — if in-memory blobs are genuinely intended (tests,
  demos) — declare the choice explicitly with the new
  `UnifiedRuntimeBuilder::ephemeral_blobs(true)`.
  `MobBootstrapSpec::persistent` / `persistent_with_hook` now return
  `Result<_, BlobStoreResolutionError>`. Additionally, persistent mode now
  asserts `is_persistent()` on the *resolved* blob store at composition
  time, so a custom-injected non-persistent store also requires the
  `ephemeral_blobs(true)` declaration. Ephemeral launch modes (scratch or
  temp-dir) are unchanged — their in-memory blobs are the declared choice of
  the mode itself.

### Added

- **Continuity snapshot checkpoint adoption (storage-unification H3).**
  Session bytes written by 0.7.x-era MobKit live inside continuity
  `session_snapshots` rows; on 0.8.x they decode as legacy-unverified and
  hard-fail every resume through the identity-first path (the bridge
  correctly refuses fresh-spawn, so the identity stays `Broken` on every
  reconcile retry). Meerkat ≥0.8.3 auto-migrates the documents *it* reads
  (session store, runtime snapshots); continuity snapshots are the copies
  meerkat never sees, and this release heals them via the exported
  `meerkat_core::adopt_legacy_session` helper with the **observed cursor**
  (generation / checkpoint version) from the matching continuity record.
  Two sanctioned shapes:
  - **Batch, in a deploy/maintenance window**:
    `meerkat_mobkit::identity_first::adopt_continuity_snapshots` (library
    entry) and the new `mobkit_gateway storage-adopt-checkpoints
    (--db <path> | --state-dir <dir>) [--apply] [--json]` maintenance
    subcommand (never an eager side effect of gateway startup; dry-run by
    default and byte-identical without `--apply`; exit 1 on refusals or an
    unacquirable fence). The walk holds the exclusive maintenance fence on
    the database file, enumerates rows by direct SQL (the `ContinuityStore`
    trait has no enumeration API and cannot rewrite in place at the same
    `CheckpointVersion`), rewrites each legacy row's bytes in place, and
    deliberately leaves the row's `generation` / `checkpoint_version` /
    `fencing_token` columns untouched — the stamp binds to the observed
    cursor the row already records. Stale rows (record rebound away or
    generation superseded) are classified and reported, never adopted and
    never an error; a re-run is a byte-identical no-op.
  - **Lazy, at restore** (always-on single-replica deployments, where the
    pod restart is the window): `ContinuitySessionStoreAdapter` adopts a
    legacy snapshot at first load under the registered continuity cursor
    and persists the adopted bytes through the store's own CAS at the next
    checkpoint version. Named opt-in
    (`with_lazy_checkpoint_adoption(true)`), enabled on the identity-first
    gateway wirings (`rpc_gateway`, the builder's external-authoritative
    arm) — it is the fleet-unbrick behavior. Downstream lazy shims that
    stamped continuity copies by hand can retire on this release (retirement
    chain: meerkat #909 → this H3 → shim removal).

  **Ordering constraint:** meerkat's own lazy path seeds `INITIAL` cursors
  and a verified document never re-migrates, so on any fleet whose
  continuity rows record a **nonzero generation floor**, run H3 (batch verb,
  or restore through the lazy-enabled adapter) **before** meerkat's lazy
  path first touches those sessions. Generation-0 fleets are unaffected.
- **`MobKitStorageLayout` path authority + canonical-name-first probing
  (storage-unification Phase M2).** One module,
  `meerkat_mobkit::storage_layout`, now owns every storage root and
  canonical top-level database locator; `UnifiedRuntimeBuilder`,
  `mobkit_gateway`, and `rpc_gateway` all consume the layout instead of
  deriving file names inline (the three-way derivation duplication is
  deleted). Canonical spellings, decided once: stores shared with Meerkat
  keep Meerkat's names — sessions `sessions.sqlite3`, runtime
  `runtime.sqlite`, schedule `schedule.sqlite`, workgraph
  `workgraph.sqlite3` — and MobKit-owned files converge on `*.sqlite3`:
  continuity `continuity.sqlite3` (was `continuity.db` on the gateways,
  `identity_continuity.sqlite` in the builder), metadata
  `mobkit_metadata.sqlite3`, console `mobkit_console.sqlite3`, agent-memory
  root `agent-memory/` (was `agent-memory-sqlite/` for the builder's SQLite
  stack), blob root `blobs/`, and gateway-home files `peer_key.ed25519` /
  `tux-runtimes.json`.
  **Existing deployments keep working unchanged:** every locator resolves
  canonical-name-first and then probes the known legacy spellings in the
  same directory — exactly one spelling present means it is used *where it
  lies* (no rename at open; physical renames to canonical names arrive only
  with the Phase M6 migration verb, under the maintenance fence). Only
  *fresh* directories get the canonical names. When both the canonical and
  a legacy spelling (or two legacy spellings) of the same store exist, boot
  refuses with the typed `StorageLayoutError::FileNameTwins` pointing at
  the storage doctor — previously the surfaces would silently open one of
  the two and fork history. A gateway `store_path` with a file extension
  remains an explicit session-database override (now an explicit layout
  input instead of call-site extension sniffing). This also heals the
  cross-surface agent-memory hazard: `rpc_gateway` now finds and reuses a
  builder-created `agent-memory-sqlite/` corpus instead of silently
  starting an empty `agent-memory/` beside it.
- **Declared-ephemeral scratch layout for `rpc_gateway`.** With no
  `persistent_state`, the locally hosted identity substrate no longer lands
  on a silent pid-suffixed `$TMPDIR/mobkit-continuity-<pid>.db` at the call
  site; the gateway constructs the layout in explicit scratch mode rooted
  at `$TMPDIR/mobkit-scratch-<pid>/` and the choice is recorded in the
  serializable `layout_summary()` (`durability: declared_ephemeral`), which
  the Phase M1 storage doctor consumes.

- **Storage doctor (M1): `mobkit/storage/doctor` RPC + console read
  method.** A read-only diagnosis of a MobKit state directory, safe against
  a live gateway (read-only SQLite opens, no file creation, no leases, no
  schema-ledger runs), exposed on the module-only and unified stdin RPC
  surfaces and on `POST /console/rpc` (read method, `runtime.admin` grant;
  the console `mobkit/status` payload now advertises
  `storage.doctor_available: true`). Reports: the per-directory database
  inventory across every historical filename spelling, with schema-ledger
  domain versions (`no-schema-ledger` on pre-M3 files,
  `empty-database-shell` on table-less files); **file-name twins** as
  errors (`file-name-twins`) for `sessions.db`/`sessions.sqlite`/
  `sessions.sqlite3`, `continuity.db`/`identity_continuity.sqlite`/
  `continuity.sqlite3`, `mobkit_metadata.sqlite`/`.sqlite3`,
  `mobkit_console.sqlite`/`.sqlite3`, and `agent-memory/`/
  `agent-memory-sqlite/`; the continuity checkpoint-evidence census per
  identity (`legacy-unverified-continuity-snapshots`,
  `checkpoint-metadata-invalid`, `continuity-snapshot-undecodable`) that
  H3's adoption dry-run consumes; dangling console-frame blob references
  (`dangling-console-blob-reference`); blob-root inventory (`blob-root`,
  `legacy-fs-blobs`); gateway-home artifacts when scoped
  (`peer-key-file`, `runtime-registry`); the workgraph admission sidecar
  (`workgraph-admission-sidecar`); filesystem artifacts
  (`maintenance-fence-lock`, `backup-artifact`, `quarantine-artifact`); and
  the live H1/H2 durability resolution (`blob-durability`,
  `session-store-incremental`) when invoked through a live gateway, or
  `durability-census-unavailable` on a cold directory. `params.state_dir`
  is required until the Phase M2 layout authority lets the runtime report
  its own state directory — runtime-backed surfaces answer a missing
  `state_dir` with error `-32004`. New public seam
  `meerkat_mobkit::storage_doctor` (`diagnose_state_dir`,
  `MobKitStorageMigrator` implementing
  `meerkat_core::storage_diagnostics::StorageMigrator`) plus
  `storage_doctor(...)` / `storageDoctor(...)` wrappers in the Python and
  TypeScript SDKs. Additive method: `MOBKIT_CONTRACT_VERSION` stays at
  `0.4.0`.
- **Storage durability health surface (H1/H2).** `mobkit/status` (unified
  and console shapes) and the unified `mobkit/capabilities` now carry a
  `storage` object: `blob_durability`
  (`persistent_disk` | `declared_ephemeral` | `custom`),
  `blob_store_persistent` (bool), and `session_store_incremental`
  (bool, or null when no session persistence exists). New public vocabulary
  in `meerkat_mobkit::storage_health` (`ResolvedStorageSummary`,
  `BlobDurability`, `BlobStoreResolutionError`,
  `probe_session_store_incremental`), carried on
  `MobBootstrapSpec::resolved_storage` and readable via
  `UnifiedRuntime::resolved_storage()`.
- **Loud whole-blob degradation (H2).** At every composition site that
  builds a `PersistentSessionService`, MobKit now probes the session store's
  incremental-persistence capability (`as_incremental`) and logs a startup
  warning when it is absent, naming the store kind and the consequence
  (session persistence degrades to whole-blob saves on every turn). This
  makes the identity-first gateway shape — where `ContinuitySessionStoreAdapter`
  is the session authority and persists O(session) per turn — visible at
  startup and on the health surfaces (`session_store_incremental: false`)
  instead of silent. The structural fix (a genuine incremental channel on
  the continuity contract) is tracked as storage-unification Phase M4; this
  release makes the degradation honest, not fixed.

## [0.8.2] - 2026-07-22

### Changed

- Pinned the complete Meerkat dependency family to the public `0.8.3` crate
  set, including identity-first flow target provisioning and runtime reliability
  fixes.
- Memory-tool installation now follows the resolved profile's explicit memory
  policy, including realm-referenced profiles, instead of relying only on the
  provider-wide authored-write capability.

### Fixed

- Identity-first `run_flow` now materializes dormant targets through the shared
  Mob handle provisioner before dispatch, so admitted flows reliably become
  member turns.
- Permanent member event-stream loss now repairs the trusted durable identity,
  fences stale generations, and is detected independently of bounded console
  output consumption.
- Quarantined or torn-down identity sessions transition to `Broken` and enter
  the normal repair loop instead of remaining permanently `Active` zombies.

- All six in-crate SQLite openers (agent memory, continuity writer + read
  pool, runtime metadata, console aggregator, workgraph admission sidecar,
  schedule triage) now flow through the shared `meerkat-sqlite` mechanics
  crate under named connection profiles; MobKit no longer hand-rolls any
  PRAGMA setup (storage-unification Phase M3).
- **Console aggregator store gains WAL journaling, a busy timeout, and
  `synchronous=FULL` for the first time.** It previously set zero PRAGMAs
  (rollback journal, no busy handler). This is the highest-write-rate MobKit
  database: writes now fsync on commit and participate in WAL semantics, so
  concurrency-heavy deployments should re-run console-load benchmarks.
- **The continuity store's writer now runs with `synchronous=FULL`** (it
  previously set only WAL + busy timeout). Continuity writes sit on the
  identity-first hot path — every checkpoint/fencing CAS now fsyncs on
  commit. This is a deliberate durability upgrade; latency-sensitive
  deployments should note the fsync-rate change. The agent-memory and
  runtime-metadata stores gain the same explicit `synchronous=FULL`.
- Busy timeouts harmonized on the shared 60s production default (previously
  5s for memory/continuity/metadata, none for the console store). The
  admission sidecar keeps its deliberate 30s wait-then-fail-closed policy as
  a named per-open override.
- Every MobKit-owned SQLite database now carries a per-file migration ledger
  (`meerkat_schema` table, domains `mobkit-memory`, `mobkit-continuity`,
  `mobkit-metadata`, `mobkit-console`) and a `<file>.mfence` maintenance
  lock file appears next to each database. Files written by a newer MobKit
  are refused with a typed schema-from-the-future error instead of being
  mutated. The workgraph admission sidecar is deliberately exempt from the
  ledger (stamping it would take the very lock it arbitrates). Legacy files
  (including pre-`ever_quarantined`/`taint` agent-memory stores) converge on
  first open; the memory store's historical column probes are now versioned
  migrations.
- `ContinuityStoreError` gains a `Transient` variant: busy/locked SQLite
  failures are now classified transient (and corruption classified corrupt)
  at the store boundary, so identity reconcile can distinguish retry-worthy
  failures from poison without string-sniffing. Classification does not
  itself authorize retry; retry policy stays with callers.

## [0.8.1] - 2026-07-22

### Changed

- Pinned the complete Meerkat dependency family to the public `0.8.2` crate
  set. MobKit composes Meerkat's concrete `retire` authority for explicit
  member deletion but does not duplicate the still-unsupported generated
  `EnsureSessionAuthority` and `ReleaseSessionAuthority` actuators. Those
  generic recovery observations remain explicitly repair-blocked upstream.

### Fixed

- Snapshot coherence now treats a snapshot's fence as historical write
  provenance while requiring the presented fence to equal current write
  authority. Cross-identity and cross-generation snapshot replacement is
  rejected, and embedded session IDs must match their durable key.
- The continuity session adapter remains a strict projection store: it no
  longer runs a second transcript-recovery classifier or fabricates an empty
  session when durable bytes are missing. Authoritative projection CAS checks
  only the visible durable row.
- Identity registration now rejects conflicting owners, generations, and
  regressing fencing epochs, and ambiguous lost acknowledgements never rewind
  checkpoint allocation.

## [0.8.0] - 2026-07-18

### Added

- Exposed identity-first startup materialization through the SDK gateway and
  Python builder. Deployments can keep eager compatibility, register lazily,
  or return after metadata registration and warm the roster in a tracked,
  bounded background task. Typed status and wait RPCs report per-identity
  `dormant`, `warming`, `active`, and `broken` progress and support an exact
  startup-readiness barrier.
- `UnifiedRuntime::start_member_turn` now exposes completion-bearing member
  turns through `MemberTurnAdmission`. Callers can observe the exact bridge
  session, await the executor-applied model/provider/self-hosted route, stream
  live events, and separately await committed runtime completion without
  treating ingress admission as execution success.
- The shared console docking contract now includes a first-class browser host
  target, with collision-free grid placement and host-prop forwarding alongside
  normal topology panels.

### Changed

- The full Meerkat dependency family is now pinned exactly to the released
  `0.8.1` crate set, including memory, live, and scheduling surfaces.
- Explicit identity bootstrap modes now consistently declare identity-first
  intent and require a roster provider, including explicit eager mode; an
  omitted mode still preserves the classic gateway when no roster is present.
- Identity bootstrap and reconcile now share one mode-aware controller;
  background work is cancelled during shutdown and the concrete Mob bridge
  skips unused checkpoint payload reads without weakening custom bridge
  contracts.
- Identity continuity persistence now serializes mutations per session,
  short-circuits provenance-identical snapshot saves, and runs bundled SQLite
  work on blocking workers with one writer and a bounded WAL read pool.
- Gateway and console identity operations are now runtime-owned through their
  commit or rollback boundary. EOF, Ctrl-C, callback closure, HTTP draining,
  and the Python/TypeScript host transports preserve that cleanup ordering
  before escalating to bounded process termination.
- Persistent SDK shutdown now negotiates the stock gateway's explicit
  335-second bounded horizon and positively attests every cleanup boundary,
  including mob quiescence, exact authority release, event draining, and child
  process termination. Provider operations retain their public 120-second
  contract, with a 125-second SDK completion deadline and a 130-second gateway
  wire deadline; Python cancels the event-loop future and TypeScript exposes an
  abort signal while suppressing late responses. Older/custom gateways retain
  EOF compatibility.
- Destructive identity reset now retains exact old-generation cleanup debt
  after the replacement continuity head commits. MobKit captures memory first,
  quiesces the superseded member, CAS-deletes only its stale session snapshot,
  verifies structural roster absence, and retries the debt during shutdown
  before attesting cleanup or releasing identity authority.
- Machine-authorized boundary cancellation now treats typed missing-session and
  no-running-turn observations as already quiesced, while preserving every
  other cancellation error as a strict shutdown failure.
- Hosts embedding an `Arc<UnifiedRuntime>` can use
  `handle_unified_rpc_json_arc`; the gateway uses its live-handler counterpart
  so identity-owned cross-mob mutations remain supervised if the requesting
  connection disappears. The borrowed dispatcher fails those mutations
  closed because it cannot transfer runtime ownership into the supervisor.
- MobKit session-service wrappers now preserve Meerkat's runtime-turn-apply
  capability, allowing optional per-turn model selection in stock console
  integrations while unsupported autonomous, direct-session, and external
  member paths fail closed before admission.

### Fixed

- Release publication now requires all ten gateway archives, emits flat asset
  names in `index.json` and `checksums.sha256`, and keeps manual tagged builds,
  validation, and registry publication on the exact requested source tag. The
  registry job now retries public readback until the exact crate, Python wheel
  and source distribution, and npm package are all independently visible,
  non-yanked, downloadable, and checksum-valid.
- Runtime-turn diagnostics are structural-only and opt in with an explicit
  truthy value; prompts, appended context, tool dispatch metadata, and full
  runtime semantics are never written to the diagnostic log.
- The optional source-baseline verifier no longer embeds a developer machine
  path and fails closed with a typed error unless a checkout is supplied
  explicitly or through `MEERKAT_REPO`.
- Quiesced session persistence before lease rotation, retained exact committed
  grants across failed authority publication, reused live grants during eager
  reconcile, and drained identity-scoped orphan grants before direct lazy retry.
- Bound merged and protected structural event authorization to the emitting
  member's exact runtime incarnation and fence token, preventing a later
  public respawn of the same alias from disclosing an earlier secret event.
- Failed gateway identity bootstrap now installs runtime authority before
  materialization and runs the full ordered shutdown before returning the init
  error, releasing exact grants held by members that activated before a later
  eager-roster member failed.
- Serialized continuity generation changes with foreground materialization and
  reconcile, physically retired removed or changed roster members, and kept
  typed bootstrap readiness synchronized with lifecycle transitions.
- Hardened identity authority at raw member, RPC, console, SSE, and cross-mob
  boundaries: reserved aliases cannot be forged or preclaimed, policy checks
  use canonical targets, and operations resolve the current durable generation
  instead of a stale reset-era roster row.
- Prevented same-generation session rebinds from rewinding the durable
  checkpoint head, rejected continuity-generation regressions even under a
  newer fencing token, and moved persistent SQLite initialization and fencing
  floor reads off async executor workers.

## [0.7.39] - 2026-07-15

### Added

- Optional topology control plane, disabled by default: authorized operators
  can explicitly connect, disconnect, or reconnect agent pairs through
  revision-pinned, idempotent, durably journaled operations with recoverable
  receipts and a separately permissioned audit cursor. Cross-authority changes
  are supported only by the same-process bilateral coordinator, which checks
  both runtime authorities and rolls back partial physical changes; the local
  JSON-RPC surface fails cross-process mutation closed. The stock MobKit
  console gains a capability-gated Connections picker with pairwise actions
  and no implicit bulk-connect behavior.

### Changed

- The stock console now groups sidebar items semantically, keeps completed
  flow cards compact, targets flow restoration to the selected run, and gives
  long conversation tails their own scrollable region.

### Fixed

- Stabilized streamed conversation reconciliation: rich-content nodes retain
  their identity, conversation groups stay mounted, and group/message updates
  no longer replace or overwrite the active streaming tail.
- Restored canonical peer labels and transcript copy actions, and tightened
  activity-preview copy-action spacing across the shared console components.
- Hardened gateway test process draining and the stock Incident Commander
  acceptance path, including durable editable-topology state, replay-safe test
  sessions, and cold Cargo-lane fixture discovery.

## [0.7.38] - 2026-07-13

### Added

- Console health affordance from the machine-owned member liveness
  projection (ask 14 follow-up): sidebar rows show a `wedged`/`degraded`
  chip (silent when healthy), the roster dot tints by health, and the
  roster inspect pane gains Health / Run state / In flight / Last
  progress rows. Progress is fetched per non-final member and capped at
  64 members (`MOBKIT_CONSOLE_PROGRESS_MEMBER_CAP`; `0` disables) so
  large rosters do not fan out against the mob actor mailbox.

### Changed

- meerkat family pinned to `=0.7.31` (from `=0.7.30`): delivery-time
  mob member revival carries a machine-authorized
  missing-live-materialization intent, and executor publication is
  cancellation-safe (no false `Attached`/ready state without a live
  executor) — the delivery-time sibling of the ask 34 boot-revival fix.

## [0.7.37] - 2026-07-13

### Changed

- meerkat family pinned to `=0.7.30` (from `=0.7.29`): ask 34 ships —
  the retired-session revival flow completes end-to-end. The mobkit
  acceptance test (`identity_first_resume_revives_terminally_retired_
  runtime`, landed ignored in 0.7.36) is un-ignored and green: a
  terminally-retired durable session revives on ordinary resume with
  the transcript intact. Bug I is fully closed — existing victims
  revive automatically on their next boot; the manual `runtime_states`
  row repair is retired. Zero open upstream asks.

## [0.7.36] - 2026-07-12

### Changed

- Ask 29 (Bug G′) adopted: model-only reprofiles use the field-scoped
  `SpawnMemberSpec.model_override` seam — the pin is reapplied over the
  CURRENT definition profile on every materialization, so definition
  drift (tools, skills, peer posture) keeps reaching reprofiled members.
  The whole-profile snapshot survives only for provider-pinned profiles
  (upstream does not re-infer the provider under `model_override`).
  Realm-ref bindings now accept model overrides. Members frozen under
  the old whole-profile override heal on their next reset.

### Fixed

- The `MobSessionService` wrappers now forward the meerkat 0.7.29
  retired-session revival seam (`load_revivable_retired_session`,
  `promote_revivable_retired_session`,
  `create_session_with_machine_archived_resume_authority`,
  `load_persisted_session_metadata`) instead of masking it with the
  trait defaults. End-to-end revival still requires upstream ask 34
  (the revival flow stages executor registration against the
  still-Retired machine); the acceptance test ships `#[ignore]`d and
  un-ignores on the fixing meerkat release. Until then, existing Bug I
  victims still need the manual `runtime_states` row repair — new
  destruction is impossible on meerkat ≥0.7.29.

## [0.7.35] - 2026-07-12

### Changed

- meerkat family pinned to `=0.7.29` (from `=0.7.28`). Four upstream asks
  land and are adopted:
  - **Ask 32 (Bug I secondary-racer fence)**: retire disposition is
    machine-authorized and incarnation-scoped upstream; the 0.7.34
    collision drain-poll + retry-once workaround is removed from the
    resume bridge. The Bug I destruction-detection probe remains
    (ask 31 still open — `MOBKIT_IDENTITY_RESTORE_CONCURRENCY=1`
    remains the recommended boot mitigation until it ships).
  - **Ask 14 (member liveness)**: the machine-owned
    `MemberProgressSnapshot` (run_state, in_flight_work, last progress,
    health healthy/degraded/wedged) flows on `mobkit/member_status`,
    is typed as `MemberProgressSnapshot` on `RichMemberSnapshot` in the
    Python and TypeScript SDKs, and projects on the identity-inspect
    RPC as `progress`.
  - **Ask 28 (objective correlation)**:
    `objective_owner_bound`/`objective_concluded` mob events project
    through the structural event surface with member attribution.
  - **Ask 27 (delivery outcomes)**: typed
    `PeerDeliveryOutcome{Acked,HandedOff,Queued}` reaches agents
    upstream; verified no mobkit surface required exposure.

### Fixed

- `SessionStoreBackedRuntimeStore` now delegates every defaulted
  `RuntimeStore` method instead of masking the inner store's answers:
  the 0.7.29 compaction-projection outbox failed session create closed
  through the facade, and `is_runtime_projection_quarantined`
  (defaulting to "not quarantined") plus `delete_ops_lifecycle` were
  latent pre-existing masks.

## [0.7.34] - 2026-07-11

### Fixed

- Bug I mitigations (boot restore/retire race that terminally retires
  slow-restoring identities; root cause is upstream — asks 31/32/33):
  - `MOBKIT_IDENTITY_RESTORE_CONCURRENCY` env knob (clamped 1-16,
    default 4 unchanged); `=1` serializes identity restores, removing
    mobkit's contribution to SQLite writer contention during boot.
  - Collision-retry drain: after a roster-collision retire, the bridge
    polls roster absence (bounded 2s) before retrying the resume, and a
    retry canceled by the still-queued retire command is retried once
    more after a second drain.
  - Honest rejection contract: after any rejected resume the bridge
    probes the session store and logs explicitly when the durable
    session is GONE (destroyed by meerkat's spawn-failure rollback)
    instead of the now-sometimes-false "durable session preserved".

## [0.7.33] - 2026-07-11

### Changed

- meerkat family pinned to `=0.7.28` (from `=0.7.27`): GPT-5.6
  (Sol/Terra/Luna) in the catalog with `gpt-5.6-sol` as the new
  provider/catalog default (explicit GPT-5.5 pins stay honored; realtime
  capability unchanged), plus endpoint-recovery hardening (cold restart
  replays each member's exact generation peer endpoint; fail-closed on
  descriptor drift and PeerId reuse).
- Live seed bounding is upstream-owned: `seed_max_chars` (per-open or
  `runtime_options.live`) now delegates to meerkat's windowed projection —
  enabled root context, an affordable compaction summary, and the
  identity/tombstone/rewrite-generation/canonical-image sidecars are
  preserved, with explicit degraded-continuity reporting. The 0.7.32
  oldest-first clamp stopgap is removed; wire parameters are unchanged.
- The realtime open config and projection snapshot carry the
  `canonical_user_image_decoded_bytes` image-budget sidecar.

## [0.7.32] - 2026-07-10

### Added

- Live image input (meerkat 0.7.27): still images ride the existing
  `mobkit/live/send_input` as `{kind: "image", idempotency_key, mime,
  data}` — exact-retry deduplicated by the runtime's user-content identity
  lane, which also rides the open config so reopened channels do not
  replay committed images. SDK conveniences `live_send_input_image` /
  `liveSendInputImage`. Only `gpt-realtime-2` accepts image input in the
  shipped catalog.
- Deterministic (non-LLM) schedule targets from the SDK:
  `runtime_options.host_runnables` registers named host runnables whose
  fire forwards over the stdio callback bridge as `callback/schedule_fire`;
  Python `MobKitBuilder.host_runnables([...])` +
  `runtime.on_schedule_fire(name, handler)`. Agents target them through the
  `meerkat_schedule_*` tools' `host_runnable` target kind.
- Per-open instruction overlay on `mobkit/live/open` (`instructions`:
  string or array) — carried on the runtime-system-context lane, ephemeral
  to the open, never persisted into the durable transcript; drops on
  `live/refresh` by construction.
- Callback/SDK tools publish real input schemas:
  `SessionBuildOptions.register_tool(..., input_schema=...)` flows through
  the build callback into the tool defs the provider (and live seeds) see;
  schema-less registrations stay wire-compatible.
- `mobkit/live/truncate` + `live_truncate`/`liveTruncate` SDK methods.
- Live seed clamp: `runtime_options.live.seed_max_chars` (+ per-open
  override) drops whole seed messages oldest-first to fit the realtime
  provider's instruction cap — an explicit stopgap for upstream ask 30
  (seed-window/summarized projection belongs in core).

### Changed

- meerkat family pinned to `=0.7.27` (from `=0.7.26`): image input on the
  OpenAI Realtime live channel end to end, the realtime user-content
  identity lane (exact-retry + live rewrite guards), redacted image
  receipts synthesized only after durable reducer application, and an
  explicit durable start boundary for mob retirement (new mob event kinds,
  projected by the console/SSE surfaces).
- Identity-first bootstrap restores up to four durable members concurrently
  instead of serially, with bounded fan-out and per-member restore timing logs
  for slow resumes. This prevents large full-history rosters from accumulating
  every member's resume latency inside `mobkit/init`.

## [0.7.31] - 2026-07-09

### Added

- Live (realtime) member sessions through the gateway: meerkat's live/*
  surface on mob members. `mobkit/live/{open,status,close,refresh,
  send_input,commit_input,interrupt}` accept identity targets; the live
  WebSocket transport mounts on the gateway's EXISTING HTTP listener
  (`runtime_options.live = true | {public_base_url}`, default off,
  persistent mode only); tool calls made during live turns flow through
  the member's normal external-tool machinery (callback bridge + gating
  unchanged); live turns persist into the member's durable transcript
  through the continuity adapter. Machine-authority-backed single-use
  bootstrap tokens; per-open credential resolution matching text-turn
  auth; per-open `model` override. SDK methods in both languages. Design:
  docs/design/live-sessions.md.
- Definition wiring is a reconcilable desired state (HomeCore field
  report): `auto_wire_orchestrator`/`role_wiring` now converge regardless
  of member bring-up order and re-converge after restarts — a
  definition-derived default edge policy installs at bootstrap,
  `ensure_member` reconciles after every materialization, and the console
  surface's `mobkit/reconcile_edges` noop stub is a real wire-only
  reconcile. Host-made manual edges are never unwired.
- Cross-mob DX: `cross_mob/peer_info` accepts durable identities (roster
  `agent_identity` label fallback); `cross_mob/wire_local` accepts the
  `ed25519:`-prefixed `transport_public_key` spelling.

### Fixed

- `ContinuitySessionStoreAdapter` adopts the machine-owned
  `RebuildToAuthority` rollback (HomeCore Bug B-2): the torn-shutdown save
  wedge — a stamped intra-turn checkpoint head rejecting the resume's
  shorter committed-authority save with `MonotonicityViolation`, degrading
  the identity forever — now converges the row back onto committed truth.
  Unstamped rows and content forks keep failing closed.
- The BigQuery session-store adapter shares one process-wide HTTP client
  with per-request timeouts, and runtime bootstrap pre-warms the client
  stack: the realtime feature unification enabled reqwest's
  `system-proxy`, putting a ~700ms one-time macOS proxy scan inside the
  first HTTP-backed RPC's latency window.

### Changed

- meerkat family pinned to `=0.7.26` (from `=0.7.25`): cold revival of
  stopped sessions re-binds under the fresh registration epoch (the
  identity-first member-revival terminal failure), typed rejections are no
  longer laundered into "session not found in runtime adapter", one
  member's composition-dispatch rejection no longer terminates the whole
  mob actor, and the classic-store projection bridge consults the
  write-half rollback (the upstream half of Bug B-2).

## [0.7.30] - 2026-07-09

### Fixed

- The rpc_gateway's `callback/build_agent` path composes host-returned tools
  OVER pre-installed dispatchers instead of assigning the slot wholesale
  (HomeCore "Bug D"): callback-built agents keep the native agent-memory
  recorder's `memory` tool (a host tool named `memory` now shadows it by
  design — primary wins name collisions). New
  `meerkat_mobkit::tool_compose::ComposedExternalTools` is the canonical
  compose utility; both dispatch entry points forward, so the
  `ToolDispatchContext` attention witness survives.
- The rpc_gateway stdin dispatch loop serves RPC requests concurrently
  (each on its own task): a turn- or build-running RPC blocked on a host
  callback round-trip no longer starves reentrant requests — an
  `agent_memory/recall` issued from inside a callback tool handler used to
  queue behind the turn until the 120s callback timeout.
- `UnifiedRuntime::shutdown` quiesces in-flight member work before stopping
  the mob: meerkat 0.7.25's machine refuses `Stop` mid-work
  (`InvalidTransition`), so shutdown cancels member work and retries over a
  bounded window — a gateway going down mid-turn stops cleanly.

- Console live tails no longer starve on identities the read model has not
  observed (issue #254): identity-scoped SSE streams get the same
  own-identity allowance the windowed query path always had, and a frame
  for an unknown identity triggers a debounced identity read-model
  refresh, so members spawned mid-run (`ensure_member`) become visible on
  unscoped streams without a reconnect.
- The console aggregator gained `unregister_runtime` (issue #254
  follow-up): removes the registration, signals the live-projection task
  (previously immortal — its broadcast receiver could never observe
  `Closed` while the runtime lived), terminates the discovery loop, and
  refreshes the identity read model. Re-registering a key now also
  replaces its projection task instead of double-projecting.
- `/agents/{id}/events` accepts durable identities (issue #254 item 4):
  ids resolve via the roster's `agent_identity` label when direct encoding
  misses (the #252 canonicalization class), so identity-first consumers no
  longer need a `list_members` round-trip; unknown ids keep the proper 404.
- Docs: `mobkit/console/*` methods are HTTP-only (the stdio surface never
  dispatched them), and session-history backfill frames
  (`source.kind == "session_history"`, `interaction_id` null) are a
  mandatory exclusion when correlating completions.

### Added

- Full WorkGraph integration (meerkat 0.7.23's goals, work items, and
  attention bindings): a realm-scoped `WorkGraphService` per runtime with
  the member tool surface (profile `tools.workgraph`, default off),
  apply-time attention overlays on every mob-executor turn, the
  `mobkit/workgraph/*` JSON-RPC group (22 methods on the unified and
  console surfaces, `workgraph.view`/`workgraph.manage` ABAC actions,
  capabilities and experience projection), full Python and TypeScript SDK
  parity with typed results and conflict errors, and a conversation-native
  inline WorkGraph card in the console chat pane (live goal/work-item tree
  folded from agent tool calls, ABAC-gated operator actions with CAS
  revisions) plus a WorkGraph workbench panel. A one-binding-per-target
  admission layer (occupancy guard across create/reassign/resume on both
  the RPC and agent tool planes, session/identity target unification,
  cross-process serialization) protects members from upstream's
  MultipleActiveBindings hard-fail until meerkat lands binding uniqueness
  (upstream ask 25). Hardened by a six-round adversarial review battery
  (64 verified findings fixed) and live-fire verified end to end.

- `mobkit/workgraph/attention/prune` (upstream ask 24): terminal-binding GC
  on both RPC surfaces plus `workgraph_attention_prune` /
  `workgraphAttentionPrune` in the SDKs.
- `mobkit/workgraph/attention/break_glass_reassign` (upstream ask 23,
  console surface ONLY): host-plane recovery for a binding stuck on a
  wedged/retired agent with no coordinator holding authority. The principal
  is the authenticated console principal — never a wire parameter — a
  non-empty reason is mandatory, and upstream records both in the workgraph
  event stream. Deliberately absent from the stdin surface and the SDKs.
- Interaction identity threads end to end for identity-first console sends
  (upstream ask 15): the console mints deterministic UUIDv5 interaction ids
  and threads them through `WorkSpec` into meerkat runtime admission, the
  session-history backfill stamps `interaction_id`/`run_id` from persisted
  transcript messages, and the console dedup treats UUID-form ids as
  authoritative twin identity — exact live↔history joins, and repeated
  identical replies from DISTINCT interactions both render (the over-cull
  class). Classic sends keep the text heuristic (the external work door
  cannot thread an id yet).

### Changed

- meerkat family pinned to `=0.7.25` (from `=0.7.23`): the outstanding-asks
  sweep. Store-owned attention-binding uniqueness with typed occupant-naming
  conflicts (ask 25 — mobkit's admission layer demotes to defense-in-depth
  plus session↔identity alias unification; `MultipleActiveBindings` is gone),
  machine-owned revival of Stopped sessions (0.7.24), O(delta) incremental
  session persistence, the structural `reply_to_peer` affordance (ask 26),
  metadata-only session reads (ask 24 clause 3), and the schedule
  single-owner fix (`SessionRuntime` arms its own firing host — mobkit's
  gateways already bound tools+host from one `ScheduleService`, so no
  mobkit-side change was needed).

## [0.7.29] - 2026-07-08

### Changed

- meerkat family pinned to `=0.7.23` (from `=0.7.22`). Ask 21d verified
  fixed: never-run identity-first mob workers no longer strand in
  archive-NotFound during disposal — the doctrine worker-respawn test now
  asserts success (tolerance branch removed). With 21c (0.7.22) and 21d
  (0.7.23) the whole never-run-member disposal family (asks 20/21/21b/21c/
  21d) is closed on BOTH constructions. API drift absorbed: the
  runtime-backed schedule host takes an optional `WorkGraphService`
  (mobkit passes `None` — WorkGraph is not wired yet), and
  `flow_tool_overlay` → `turn_tool_overlay` on turn semantics/metadata.

### Fixed

- Schedule self-delivery now reaches identity-first members: the internal
  delivery lane canonicalizes the binding's member id (decode-then-encode)
  before the roster lookup. Binding member ids arrive in ROSTER space —
  identity-first bridge members' roster ids are the comms-encoded runtime id
  (`mk--…`) — and the previous lookup re-encoded them (the codec re-encodes
  marker-prefixed input by design), missed the roster, and silently fell
  through to the external door, reproducing the addressability rejection the
  internal lane exists to bypass ("mob member is not externally addressable:
  mk--rt_cdomain_chome_c0" on HomeCore 0.7.28). Plain member names are
  unaffected (canonicalization is the identity on them). E2e pins the
  roster-space binding shape end to end; the codec contract test pins
  decode-then-encode as the only correct roster-key derivation.

## [0.7.28] - 2026-07-07

### Changed

- meerkat family pinned to `=0.7.22` (from `=0.7.20`; 0.7.21 was skipped —
  its ask-21b archive arm routed never-run-member disposal into a
  pre-existing runtime-loop self-deadlock on the session mutation gate and
  wedged the whole mob, upstream ask 21c). 0.7.22 fixes the deadlock class
  structurally (stop realization is guard-free by construction) and with it
  the whole ask 20/21/21b/21c never-run-member family on the classic
  persistent chain: retire/respawn of never-run `ensure_member` crews now
  converges, and the K1 persistent regression test runs un-ignored.
  Residue: mob-plane workers under the identity-first gateway construction
  still archive-NotFound on respawn (upstream ask 21d, P1, fast-fail — no
  wedge, no new strand class).

### Fixed

- Schedule delivery to a mob member is now INTERNAL addressing: the schedule
  host wraps the mob delivery host so member-addressed prompts go through
  the internal work lane (`WorkOrigin::Internal` — the same door the
  identity bridge uses), not the external ingress door. Previously delivery
  routed through `member_send`'s external path, which rejected members whose
  profile is `external_addressable = false` ("mob member is not externally
  addressable") — HomeCore's domain agents are internal-only by design, and
  a schedule firing back into its own author's session was blocked by the
  external posture. Flows/helpers/probes keep the stock host behavior. E2e
  pins self-delivery to an internal_only author at `stage=completed`.

## [0.7.27] - 2026-07-07

### Fixed

- Agent-authored schedules now DELIVER on both gateways (the HomeCore 0.7.26
  "last link": authoring ✓ planning ✓ claim ✓ no runaway ✓ delivery ✗ with
  "scheduled identity targets are not supported by this session host").
  Root cause was mobkit-side: specs built via `MobBootstrapSpec::new` (both
  gateway binaries roll their own session services) never installed the
  agent mob tool surface, so `agent_mob_mcp_state()` was None — members
  still got mob tools through meerkat-mob's INTERNAL state, but mobkit's
  schedule host had no mob authority: the authoring-time rewrite to
  mob-member targets could not resolve sessions, and `spawn_schedule_host`
  fell back to the Noop mob host, leaving identity/mob targets undeliverable.
  New public seam `MobBootstrapSpec::with_agent_mob_tools` performs the
  install for externally-constructed specs; both gateways wire it. End-to-end
  tests pin the full chain (agent tool dispatch → planning → claim →
  delivery `completed`) for both the rewrite path and the delivery-time
  identity-recovery path.

## [0.7.26] - 2026-07-07

### Changed

- Upgraded the meerkat family 0.7.19 → 0.7.20 (exact pins). Carries the
  ask-22 fix for the P0 one-shot runaway (trigger yields/compares
  ms-truncated dues + a machine-owned planning-monotonicity invariant so
  future representation bugs converge as refill faults instead of
  generating occurrences unboundedly) — carry-verified by the previously
  failing `one_shot_misfire_must_not_regenerate` guard, now green. Also
  carries the ask-21 archive-scoped read fix.

### Known issues

- Ask 21b (docs/design/upstream-asks.md, ask 21 addendum): the 0.7.20
  archive-scoped read fix commits the durable document, but archiving a
  never-run registered session still returns NotFound afterwards — the
  `retiring` strand persists for mob-plane members that never ran.
  Two-adapter split ruled out empirically on the mobkit side. Identity-first
  gateways (default-on) remain unaffected for crews.

## [0.7.25] - 2026-07-07

### Changed

- Upgraded the meerkat family 0.7.18 → 0.7.19 (exact pins). Carries the full
  batch-3/4 ask set: schedule firing per-row fault tolerance + tick-health
  incidents + Deleted-tombstone healing + SQL-bounded claim (asks 16-19 —
  carry-verified: a poisoned occurrence row no longer starves healthy
  claims), bounded force_cancel (M1), declarative MCP for library embedders
  surviving revival (M2), versioning policy (M3), run-status reconciliation
  query (M4), and the ask-20 host-owned disposal fix. Also regenerates the
  bundled adaptive layer-decision schema against the 0.7.19 canonical.

### Fixed

- MobKit's session-service wrappers now forward meerkat 0.7.19's
  `session_known_to_archive_authority` disposal-routing seam (the trait
  default is fail-closed `true`; an unforwarded wrapper would have swallowed
  the inner persistent service's real store read — the compose-don't-assume
  wrapper class). The forwarding-probe test pins it.

### Known issues

- Ask 21 (docs/design/upstream-asks.md): mob-CREATED never-ran members are
  archive-authority-OWNED yet snapshotless, so the owned-path archive still
  strands them — 0.7.19's ask-20 fix reroutes only host-adopted sessions.
  Identity-first gateways (default-on) sidestep this for crews; the window
  on the worker plane is narrow (workers receive kickoff at spawn).

### Changed

- **`mobkit_gateway` is identity-first by default** (doctrine phase 2):
  every init now builds the durable-identity substrate — continuity store
  with fencing-floor seeding, lease provider, identity console RPC surface,
  Broken-identity repair task — via the new shared
  `gateway_wiring::open_identity_substrate` (the same code path
  `rpc_gateway` uses, ending the two-half-wired-gateways drift that caused
  the K1/K2 field failures). `identity_first: false` on init is a
  one-release opt-out restoring the pure mob-plane gateway. On an
  identity-first gateway `ensure_member` stands up durable identities;
  pass `plane: "worker"` to pin a spawn to the ephemeral mob plane
  (idle-retire reaping, no continuity record).
- `mobkit/capabilities` on both dispatchers now carries a top-level
  `identity_first` flag — the zero-flag-day migration signal consumers gate
  on (meerkat-studio contract point 5).

### Fixed

- Member-scoped RPCs can no longer mutate identity-owned members behind the
  IdentityRuntime's back: on any runtime carrying the identity substrate,
  `retire_member`/`respawn_member` addressed at a durable identity (plain
  name or `rt:` runtime alias) now route through the identity authority —
  tolerant retire, and reset-respawn (fresh session, same identity, new
  generation) — on BOTH dispatchers (SDK stdin `rpc.rs` and the console).
  Worker-plane members (not identity-registered) keep the classic path per
  the doctrine. Also parity: the console's classic `retire_member`/
  `respawn_member` arms now carry the same completed-disposal cleanup
  tolerance and respawn repair as the SDK dispatcher and the identity-named
  console paths (previously the console arms surfaced raw errors the other
  two surfaces tolerated).

### Added

- Identity-first doctrine recorded (docs/design/identity-first-doctrine.md):
  MobKit is dual-plane by decision — durable members on the identity plane
  (continuity, leases, reconcile), ephemeral workers on the mob plane
  (spawn/idle-retire); neither plane is deprecated, but building durable
  populations on the mob plane is. Published docs updated to match
  (roster.mdx frames the member API as the worker plane; the Python SDK's
  member-per-user "first contact" pattern now stands users up on the
  identity plane), and the mobkit-platform skill teaches the doctrine.

### Added

- Identity-first gateway mode (meerkat-studio ask K0): `identity_first: true`
  on `mobkit_gateway` init boots the durable-identity substrate — continuity
  records + lease-fenced embodiment (default providers constructed from the
  existing store paths), resume-first restore with the Broken-identity
  repair task, and the identity console RPC surface. An optional
  `identity_roster` init param seeds the desired identities;
  `mobkit/ensure_member` upserts the roster and reconciles at runtime, and
  `retire_member`/`respawn_member` route through the tolerant identity
  lifecycle — the ask-20 retire/respawn failure class does not exist on this
  surface (proved by test: retire + respawn of never-ran members succeed).
  Rust embedders get the same via the new
  `identity_first::MutableRosterProvider` + `UnifiedRuntime::set_console_identity_roster`
  + `attach_identity_first_context` (or `UnifiedRuntimeBuilder`'s existing
  roster/continuity/lease inputs on the definition path).

### Fixed

- `mobkit_gateway` now installs a tracing subscriber (stderr, `RUST_LOG`
  honored, default `warn`) and logs a version-stamped startup line.
  Previously the binary dropped every tracing event in the process —
  runtime failures, console internal-error logs, and the schedule claim
  watchdog's stall diagnosis were all invisible; meerkat-studio root-caused
  their opaque K1/K2 failures to exactly this gap on the child gateways
  their app spawns. `rpc_gateway` already had the subscriber.

### Added

- Mob-wide, revival-surviving external-tools seam (meerkat-studio ask K4):
  `MobBootstrapSpec::with_default_external_tools_provider` forwards to
  meerkat-mob's per-spawn provider, so tools attached there survive member
  respawn/revival — unlike the per-spawn `SpawnMemberSpec.external_tools`
  overlay, which revival drops. Note: a profile's `tools.mcp` allowlist gates
  what the provider exposes, and an EMPTY allowlist means the full surface.

### Changed

- Console JSON-RPC internal errors now carry the real failure reason on the
  wire (meerkat-studio ask K2): `message` and `data.detail` hold the error
  chain instead of an opaque `{"error":"internal_error"}` (the kind marker is
  kept for existing clients). This deliberately reverses the earlier
  redaction posture for THIS surface: every caller that reaches these
  handlers is an operator by construction (auth-gated consoles 401 first;
  open consoles are a trusted-local deployment choice), and the redaction
  made every -32000 undiagnosable. `console_send`'s public-message redaction
  is unchanged (that surface can reflect into agent-visible space). Also
  covers the intermittent mob-spawn 500s reported as K3 wherever they
  originate on mobkit's surface.
- The meerkat family is now exact-pinned (`=0.7.18`) in Cargo.toml
  (meerkat-studio ask K5): the compatible meerkat version is declared, not
  archaeologized from Cargo.lock at release tags.

### Known issues

- `mobkit/retire_member` / `mobkit/respawn_member` fail for never-ran
  members on persistent session services and strand them in `retiring`
  (meerkat-studio K1) — root-caused upstream (ask 20,
  docs/design/upstream-asks.md): the ArchiveSession disposal step NotFounds
  on a member that never committed a runtime snapshot. Repro ships as an
  `#[ignore]`d test in `tests/studio_k_asks.rs`; the fix lands with the next
  meerkat release.

- Schedule claim watchdog (both gateways): meerkat's firing driver discards
  its own tick errors, and one poisoned row anywhere in the schedule store
  (a Deleted tombstone the recovery invariant rejects, a stale-schema
  occurrence) aborts every tick before anything is claimed — due occurrences
  then sit `pending` forever with nothing in any log (the HomeCore 0.7.24
  report: 31/31 pending, no lease ever taken). The new read-only
  `spawn_schedule_claim_watchdog` probes the pipeline every 60s and, when
  due work is not being claimed, logs a row-level diagnosis naming the
  poisoned row. It cannot make the driver claim past a poisoned row — the
  driver fixes are upstream asks 16-19 (docs/design/upstream-asks.md).

## [0.7.24] - 2026-07-06

### Changed

- Upgraded the meerkat family 0.7.17 → 0.7.18. Carries the fix for the
  HomeCore cold-restart regression (meerkat #837): idle members whose
  turn-less boots chained resume-system-prompt-refresh rewrite commits were
  refused resume with "incoming append-only save would change retained
  transcript revision graph" — the rewrite-chain walk now proves continuity
  in authority order and no longer fails closed on the chained-refresh
  shape. New mobkit-side idle-member coverage: repeated turn-less restarts
  (with prompt drift presented) must keep resuming onto the same durable
  session and replay history on the eventual turn.

### Added

- Broken identities now self-heal: a background continuity-repair task
  (spawned by both the gateway and `UnifiedRuntimeBuilder` deployments)
  re-runs the reconcile flow with doubling backoff while any identity sits in
  the Broken state, so a resume that starts succeeding again — a transient
  cause cleared, or an upstream fix deployed — restores the identity to
  functional without a process restart. Previously "degraded pending
  reconcile retry" promised a retry that only a manual
  `mobkit/reconcile_identity` RPC or a restart delivered: delivery refuses
  Broken identities by design, and so does on-demand materialization, which
  is how HomeCore's 14 preserved-but-parked identities stayed parked. The
  task is quiet while nothing is Broken (no lease churn on healthy
  deployments) and the backoff bounds retry log noise.

- Console Memory panel Phase 2/3: Holdings reads real store totals with
  per-scope FLOOR PRESSURE markers and a data-driven STORE FLOOR verdict
  tile plus the pending-harvest queue; Knowledge shows the durable injection
  ledger with consecutive-duplicate DUP badges; Pipeline gains the pending
  proposals lane (taint badges) and the read-only "memories you might want
  to correct" audit review queue; Dreams renders the durable verdict sheets
  (per-partition runs with phases, verdict counters, and skips). Backed by
  four new read-only panel RPCs — `overview`, `proposals`, `injections`,
  `harvests` — under the existing memory read grant; the health snapshot
  stays deferred to the distinct-affordance design.

### Changed

- The rpc gateway's agent-memory assembly converged onto the `memory_wiring`
  module: the SQLite store, taint firewall, Distiller, and Steward are built
  by the same code path the Rust builder uses
  (`persistent_agent_memory_stack`), with the gateway's extras — outbound
  taint declarer, console panel registration, Hygienist, compaction-reset
  sink, observer spawn, and schedule-host dream suppression — layered as
  seams. One assembly, two hosts; behavior unchanged.

## [0.7.23] - 2026-07-05

### Changed

- Upgraded the meerkat family 0.7.15 → 0.7.17. Carries (from the batch-2
  upstream asks): typed `ToolProvenance` on tool definitions (ask 9's
  attribution half) and the compaction-archive fix that stranded compacted
  singletons on retire (`MonotonicityViolation`, ask 12 / OB3's report).
  Remaining asks (dispatch-time taint surfacing, fork authorization,
  incremental session persistence, forwarder backoff, member health,
  transcript interaction-id persistence) land in later meerkat releases.
  One API break absorbed: `PersistentSessionService::new` now requires the
  runtime store (previously `Option`).

### Added

- The full agent-memory stack is now reachable from the Rust builder
  (`UnifiedRuntimeBuilder::persistent_agent_memory_stack`): the bundled
  SQLite store with the taint firewall plus the Distiller and Steward
  engines, member-event observer, console panel registration, and the
  steward dream loop — the same stack the rpc gateway assembles, for
  embedders that drive the builder directly (no gateway). Assembly lives in
  the new `memory_wiring` module. v1 boundaries documented there: the
  Hygienist stays gateway-only, and engines are driven by the observe
  stream (the gateway's extra injector-side rotation hooks are follow-up).
- Operator-scope memory recall is live in stock deployments (provisional
  OperatorId keying: the console auth principal). With
  `agent_memory.operator_scope = "provisional"`, the gateway now installs a
  console-principal resolver: when an authenticated principal sends to an
  identity from the console, that principal becomes the identity's active
  operator and operator-scope records compose into recall — the durable
  replacement for live-behavior-correction broadcast hacks. Unauthenticated
  consoles keep the scope inert (activation requires config AND resolver AND
  a real principal). Keying stays swappable behind the OperatorResolver seam.
- Durable dream verdict sheets: every steward dream run persists to a new
  `dream_runs` table (partition label, timings, ops, and the full
  phases/verdicts/skips detail), and dead-weight usage-audit verdicts land in
  a durable review queue (`dream_audit_verdicts`) — the operator-facing
  "memories you might want to correct" list, resolvable per record. Two new
  read-only console RPCs serve them: `mobkit/memory/panel/dream_runs` and
  `mobkit/memory/panel/audit_verdicts` (same read grant as `panel/dreams`).
- Per-mob steward dreams (§8.5): with `agent_memory.steward.per_mob = true` on
  a multi-mob host, each dream attempt runs one partition per mob — that mob's
  scope plus its members' identity scopes, consolidated against that mob's own
  purpose/roster context — plus a realm-remainder partition owning
  operator/realm scopes, unrostered identities, and promotion/operator review.
  Each partition run takes its own runs-per-day budget slot. With 0–1 mobs the
  whole-realm dream is used unchanged (the flag was previously parsed but
  inert).

### Fixed

- The single-embodiment guard fails loudly: restoring or materializing an
  identity whose durable lease another live runtime instance holds now
  returns the typed `AlreadyEmbodied { identity, holder }` error naming the
  holding instance, instead of collapsing into a generic missing-lease
  error. This is a policy point, not a structural assumption — future
  multi-bind/forked-session semantics relax exactly this arm.
- Delivery repair no longer rotates a wedged member onto a fresh, empty
  session. The identity bridge's deliver-time repair used to blind-respawn
  (`MobHandle::respawn` = retire + fresh spawn), abandoning the durable
  transcript and rotating the bridge session out from under the durable alias
  (the OB3 `identity_alias_respawn_rotation` report). Repair now rebuilds the
  member ONTO its recorded durable session (`MemberLaunchMode::Resume`) —
  transcript preserved, no session rotation; the fresh respawn survives only
  as the fallback for meerkat's explicit "durable session snapshot is gone"
  answer, and every other resume failure fails the delivery loudly instead of
  destroying state.

## [0.7.22] - 2026-07-04

### Changed

- Upgraded the meerkat family 0.7.14 → 0.7.15. Carries the upstream
  transcript-restart-loss guard fix: on resume, a byte-identical re-sent base
  prompt is preserved untouched and a changed prompt becomes an audited
  transcript rewrite — both survive, composing with this release's
  Inherit-on-resume default.

### Fixed

- **A rejected resume no longer destroys the conversation.** The identity-first
  session bridge used to catch *any* resume error and fall back to a fresh,
  empty member spawn — permanently abandoning the durable transcript (the
  HomeCore restart-loss bug). A resume failure now keeps the identity → session
  binding intact, marks the identity **Broken** with the real error attached,
  and the next reconcile retries the resume. A roster collision (in-process
  restart) retires the stale member and retries the *resume*, never a fresh
  spawn.
- Reconcile outcome reporting is honest: a bridge resume that fresh-spawned
  reports `created` (never `resumed`), and a successful resume without a
  checkpoint snapshot reports `resumed` (the persisted mob session carried the
  history). Resume errors are classified from meerkat's typed errors
  (`MemberRestoreFailed`, transcript-continuity violations) instead of being
  bucketed into one `runtime_identity_incompatible` reason.
- Resume inherits the persisted System message instead of re-sending the
  spec's explicit prompt. Re-sending made meerkat re-assemble the prompt,
  which trips the session store's transcript-continuity guard on meerkat
  ≤0.7.14 whenever the persisted prompt carries runtime context appends —
  the cold-restart transcript-loss class. Dynamic per-boot context belongs in
  runtime system-context appends, not the base prompt.

### Added

- The cold-restart continuity regression now runs in CI (previously an
  ignored scaffold): boot → turn → full restart against the same on-disk
  store → assert the identity resumes onto the same session id with the
  transcript replayed, twice (second-restart variant). Its earlier "harness"
  failure was in fact the outcome-reporting lie this release fixes.

## [0.7.21] - 2026-07-04

### Changed

- Upgraded the meerkat family 0.7.13 → 0.7.14. No API breaks.

### Fixed

- Carried schedule stores authored before 0.7.20 (which hold `schedule_json`
  as TEXT) now read after upgrade instead of failing every read with
  `Invalid column type Text at index: 0, name: schedule_json` — meerkat 0.7.14
  reads schedule JSON columns through a `Text`/`Blob`-tolerant boundary
  (upstream ask A). This unblocks the identity-target schedule repair and the
  steward-dream find-or-create on upgraded deployments.
- A full process restart against a carried on-disk store now resumes each
  agent's transcript instead of falling back to an empty fresh spawn. meerkat
  0.7.14's append-only continuity guard compares the shared transcript prefix
  by content address, tolerating the bookkeeping-only divergence (re-stamped
  run identity + timestamps) a re-created runtime authority produces on cold
  restart (upstream ask B).

### Added

- **P3-outbound (upstream ask 5, outbound half):** a member whose session
  ingests untrusted content now stamps its own signed content-taint on outbound
  peer envelopes (`declare_member_outbound_taint`), cleared on a clean session
  rotation or reset. Receivers read the sender's declaration instead of
  reconstructing taint from host-side joins, propagating taint cross-process.
- **P5 (upstream ask 7):** the memory steward's dream now runs as a durable,
  misfire-aware host-runnable schedule occurrence (idempotent find-or-create
  against the persistent schedule store) instead of a bare in-process interval
  loop; the loop is kept only as the fallback on gateways with no schedule host.
- Upgrade-carry regression tests crossing a store-version boundary
  (`schedule_store_text_carry`; `identity_first_cold_restart_continuity` as an
  ignored scaffold).

## [0.7.20] - 2026-07-03

### Added

- Identity-first agent memory injection for Rust, JSON-RPC, Python, and
  TypeScript users, including bundled markdown persistence, contextual recall,
  explicit remember/recall/forget APIs, and next-turn injection for live
  identity sends.
- Classic (roster-less) agent memory: the recorder `memory` tool and build-time
  recall injection now attach to any mob member keyed on its `AgentIdentity`,
  without requiring the roster/IdentityRuntime layer.
- Ambient per-turn memory recall is delivered as a typed injected-context
  message (excluded from compaction indexing) rather than fused into the user's
  text, and is now **on by default** on the identity-first path — echo-safe by
  construction (meerkat 0.7.12 upstream ask 1).
- Compaction-discard harvest reads the exact session-memory range via
  `enumerate_scoped`, and permanently-orphaned session scopes are reclaimed via
  `drop_scope` at the identity-delete seam (asks 2 + 8).
- Distilled memory records pin the transcript head revision their evidence was
  read at, and Hygienist rewrites compare-and-swap on it (ask 4 refinement).
- The taint tracker consumes the typed `AgentEvent::PeerContentIngested`
  sender-taint declaration (canonical peer id, synchronous at ingestion),
  superseding the projection-text join for declared taint (ask 5, inbound).

### Changed

- Upgraded the meerkat dependency family to 0.7.13.

### Fixed

- Host `SessionHook`/customizers that install `external_tools` or
  `additional_instructions` on the session build now compose over the
  agent-memory recorder instead of overwriting it — a plain assignment silently
  dropped the `memory` tool and the build-time recall injection (fixed in the
  incident-command-center example; the recorder is unaffected on paths that
  compose).

## [0.6.39] - 2026-05-21

### Fixed

- Identity-first console records now expose the durable `agent_identity` label
  as the UI/send identity while preserving the runtime member id for bridge
  operations.
- Identity-first external-authoritative runtimes now resume from the supplied
  `ContinuitySessionStoreAdapter` after process restart instead of looking only
  at the process-local runtime snapshot store.
- Console live snapshots no longer read each member's session while building
  `/console/experience`, `/console/modules`, or console member JSON. Snapshot
  model capabilities now come from the mob roster profile, so busy in-flight
  agent turns cannot block the admin console.
- The bundled console's JSON fetch timeout now defaults to 60s and reports an
  explicit timeout reason when aborted. `ConsolePolicy.fetch_timeout_ms` can
  project a custom fetch ceiling to the frontend.

## [0.5.2] - 2026-03-30

### Added

- **PreBuildHook** — `persistent_with_hook()` / `ephemeral_with_hook()` accept a `PreBuildHook` closure that mutates `CreateSessionRequest` at `create_session` time, before the session service captures labels and LLM identity. Use for per-agent external tools, system prompt augmentation, label injection, and model overrides.
- `PreBuildMobSessionService` — wraps any `MobSessionService`, intercepting only `create_session`. Implemented via `delegate_mob_session_service!` macro for maintainability.
- `PreBuildHook` type alias exported from crate root
- 3 new tests: `persistent_with_hook_wraps_session_service`, `ephemeral_with_hook_creates_spec`, `pre_build_hook_mutates_create_session_request`

### Fixed

- **Session checkpointing disabled in v0.5.1** — `persistent()` passed `Some(InMemoryRuntimeStore)` to `PersistentSessionService`, which disabled the `StoreCheckpointer` (`enabled: self.runtime_store.is_none()`). Session data was never persisted after turns. Fixed by keeping `runtime_store = None` on the session service and supplying the adapter directly via `MobBootstrapSpec.runtime_adapter`.
- **Stop/resume state recovery** — `RuntimeSessionAdapter::ephemeral()` has no state recovery. Replaced with `RuntimeSessionAdapter::persistent(InMemoryRuntimeStore, blob_store)` on the spec, giving the mob actor a `PersistentRuntimeDriver` that recovers queued runtime inputs across stop/resume within the process lifetime.
- `MobBootstrapSpec` now carries `runtime_adapter: Option<Arc<RuntimeSessionAdapter>>`, wired into `MobBuilder::with_runtime_adapter()` before `with_session_service()` at bootstrap time.

## [0.5.1] - 2026-03-30

### Fixed

- **Comms drain never spawned** — `MobBootstrapSpec::persistent()` and the gateway binary both passed `None` for `runtime_store`, causing `PersistentSessionService::runtime_adapter()` to return `None`. The mob actor's comms drain was never spawned, so AutonomousHost agents exited after their kickoff turn and could not receive subsequent messages. Fixed by supplying an `InMemoryRuntimeStore`.
- Added `meerkat-runtime` as a direct dependency
- Added regression test `persistent_bootstrap_provides_runtime_store`

## [0.5.0] - 2026-03-29

### Added

- **Cross-mob communication** — contact directory (TOML), `wire_cross_mob`/`unwire_cross_mob`/`send_cross_mob` via `PeerTarget::External`, `peer_info` + `wire_local`/`unwire_local` for SDK-level peering, `::` separator addressing
- **Console RPC** — `POST /console/rpc` JSON-RPC endpoint with full auth gating, capabilities, event log queries, cross-mob peer_info/directory/wire_local/unwire_local
- **10 new 0.5 API methods** — `member_status`, `force_cancel_member`, `spawn_helper`, `fork_helper`, `attach_existing_session`, `cancel_flow`, `flow_status`, `collect_completed`, `member_current_session_id`, `member_session_ref`
- **Console frontend** — conversation transcript with SSE streaming (session-ID correlated), history replay from event log, optimistic entries with rollback
- **Python SDK** — all new RPC methods on `MobHandle`, `wire_cross_mob`/`send_cross_mob`/`peer_info`/`wire_local`/`unwire_local`, `wait_one`/`wait_all` polling, new types (`RichMemberSnapshot`, `HelperResult`, `MemberSessionRef`, `CrossMobContactEntry`)
- **Build isolation** — `scripts/repo-cargo` wrapper isolating `CARGO_HOME`/`CARGO_TARGET_DIR` per repo/worktree, `shasum`/`sha256sum` cross-platform fallback
- Contact directory loaded from `config/contacts.toml` via `ConventionalPaths` in gateway startup
- `EventLogStore` exposed via `event_log_store()` for console RPC event queries
- `parse_helper_options` shared as `pub(crate)` between main RPC and console RPC
- 176-line `test_rpc_method_names.py` covering all new SDK methods

### Changed

- **Meerkat 0.5.0 migration** — all deps bumped to 0.5.0, `MemberHandle.send()` replaces `send_message()`, `PeerTarget::External` for cross-mob wiring, subagents folded into builtins
- `MobBootstrapSpec::ephemeral`/`persistent` now enable `builtins`/`shell`/`memory` (matching gateway factories)
- `console_json_router_with_runtime` accepts `contact_directory` and `event_log` params
- Cross-mob wire/unwire/send capabilities gated on `has_inproc_contacts() && has_peer_mob_handles()`
- `rpc::mob_methods` module visibility changed to `pub(crate)`
- Gateway binary renamed from `phase0b_rpc_gateway` to `mobkit_gateway`
- Console frontend embedded bundle must be rebuilt and synced after source changes

### Fixed

- `wire_cross_mob` rolls back local side if remote wire fails
- Console RPC 401/404 exits use proper JSON-RPC format (not bare `{"error": ...}`)
- Console `mobkit/capabilities` correctly reports `is_authenticated = true` after auth gate
- Console `mobkit/status` returns `loaded_modules: []` (not member IDs)
- Console `ensure_member` rejects malformed `resume_session_id` and `additional_instructions`
- `sendInteraction` opens SSE stream before send to avoid missing fast completions
- Stream failure after successful send no longer rolls back the user message
- History query filters out agent-kind events (no payload in `UnifiedEvent::Agent`)
- History visit-level cache prevents transcript duplication on agent re-selection

## [0.4.13] - 2026-03-16

### Added

- Full multimodal `send_message` — `ContentInput` (text + images) flows end-to-end through the mob pipeline to the agent session
- `mobkit/send_message` RPC accepts `"content"` field with multimodal blocks alongside existing `"message"` string
- Python SDK `MobHandle.send(content=[...])` for multimodal delivery
- `mobkit/models/catalog` enriched with per-model `profile` including `vision` and `image_tool_results` capability flags
- Python SDK `CatalogEntry` gains `vision` and `image_tool_results` fields
- `MobBootstrapSpec::persistent()` convenience constructor for `PersistentSessionService` with session store forwarding
- `MobBootstrapSpec::ephemeral()` convenience constructor with optional `AgentSessionStore` override

### Changed

- Bump all meerkat crate dependencies to 0.4.13
- MCP `call_tool` adapted for `Vec<ContentBlock>` return type (with lossy-serialization warning for non-text blocks)
- `ToolResult.content` updated from `String` to `Vec<ContentBlock>`
- `RealMobRuntime::send_message` and `UnifiedRuntime::send_message` now accept `impl Into<ContentInput>`

### Fixed

- `FactoryAgentBuilder` session store forwarding — custom stores (e.g. BigQuery) are now passed through to `AgentFactory`, eliminating unnecessary JSONL fallback, redb lock contention, and duplicate persistence

## [0.4.11] - 2026-03-15

### Changed

- Bump all meerkat crate dependencies to 0.4.11
- Map `SessionError::Unsupported` to HTTP 422 in SSE and interaction endpoints
- `/interactions/stream` is now observe-only (message sending stays in `mobkit/send_message` RPC)
- `NotExternallyAddressable` errors return 403 instead of 500
- Renamed all `phase*` files to semantic names (34 files: binaries, tests, docs)
- `validate_phase0_governance_contracts` deprecated in favor of `validate_governance_contracts`

### Added

- `mobkit/models/catalog` RPC method in both standard and unified dispatchers
- Python SDK: `CatalogEntry`, `ProviderDefaults`, `ModelsCatalogResult` types
- Python SDK: `models_catalog()` on sync/async typed clients and `MobHandle`
- `InteractionComplete`/`InteractionFailed` as terminal SSE events
- Console live snapshot derives `loaded_modules` from discovered agents

### Fixed

- Console live snapshot `loaded_modules` was hard-coded to empty
- `phase_g` test: corrected `repo_root()` path, `.js` → `.cjs` refs, NVM PATH resolution

## [0.4.8] - 2026-03-13

First public release. Version aligned with Meerkat v0.4.8.

### Added

- Rust crate `meerkat-mobkit` published to crates.io
- Python SDK `meerkat-mobkit` published to PyPI
- TypeScript SDK `@rkat/mobkit-sdk` published to npm (full Python SDK parity)
- Gateway binary builds for 5 platforms (linux x86/arm, macOS x86/arm, Windows)
- CI/CD pipeline (GitHub Actions: fmt, clippy, test, audit, release)
- Release workflow with automated registry publishing
- Comprehensive clippy lint config (pedantic + deny unwrap/expect/panic)
- Pre-commit hooks (fmt on commit, gitleaks + clippy + tests on push)
- cargo-deny security auditing
- Version parity scripts across Rust, Python, and TypeScript
- Documentation site with architecture overview, quickstart, API reference
- MobKit logo and architecture diagram

### Changed

- Crate renamed from `meerkat-mobkit-core` to `meerkat-mobkit`
- Crate layout flattened (`crates/meerkat-mobkit-core/` → `meerkat-mobkit/`)
- TypeScript SDK renamed from `@meerkat/mobkit-sdk` to `@rkat/mobkit-sdk`
- Edition upgraded to 2024, rust-version to 1.94.0
- Meerkat dependencies bumped to 0.4.8 (resolved from crates.io)
- `spawn_many` now runs concurrently via `futures::try_join_all`
- RPC mob handlers extracted to `rpc/mob_methods.rs`
- Event log type aliases (`EventLogError`, `EventFilter`)
- Python SDK: `ensure_member()` and `find_members()` return typed `MemberSnapshot`
- Python SDK: `send()` returns `SendMessageResult` with `session_id`

## [0.4.6] - 2026-03-11

Initial internal release. Version aligned with Meerkat v0.4.6.

### Added

- Rust core orchestration engine
  - Unified runtime with module loading, mob lifecycle, and RPC gateway
  - Roster API: list, get, retire, and respawn mob members
  - Routing engine with wildcard matching and retry policies
  - Delivery subsystem with history tracking
  - Gating framework for risk-tiered action approval
  - Memory stores (knowledge graph, vector, timeline, todo, top-of-mind)
  - Session persistence with BigQuery adapter
  - Scheduling engine with cron and interval evaluation
  - Persistent operational event log
  - SSE event streaming for agent and mob observation
  - JWT/JWKS authentication with OIDC discovery
  - Admin console REST API
- Python SDK
  - Builder pattern for runtime configuration
  - Typed `MobHandle` with 30+ methods covering all RPC operations
  - Typed result models for all API responses
  - SSE bridge for real-time event streaming
  - ASGI app for serving the runtime over HTTP
  - Session agent builder protocol for callback-driven agents
  - Error event hooks for operational alerting
- Admin console (React)
- Mintlify documentation site
