# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

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
