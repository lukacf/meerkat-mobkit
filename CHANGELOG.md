# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Fixed

- **The console copy button lied on plain http, and one of the two still did.**
  `navigator.clipboard` exists only in a secure context - https, or localhost -
  and the console is routinely served over plain http on a LAN address, where it
  is `undefined`.

  ChatPane called `navigator.clipboard.writeText(...)` directly; it threw before
  reaching the clipboard, into a `catch {}` that existed to keep a clipboard
  error from breaking the hover affordance. The button did nothing, silently,
  for every LAN user.

  The tool-call button on the shared conversation path was worse. It ran
  `navigator.clipboard?.writeText(text).catch(() => {})`, and with `clipboard`
  undefined the optional chain short-circuits the **whole** expression - `.catch`
  is never reached, nothing throws - after which `setCopied(true)` ran
  unconditionally. It showed a checkmark having copied nothing, so the user
  pasted whatever was already on the clipboard. A silent lie is worse than a
  visible failure.

  Both now use `copyTextToClipboard`, which already existed in
  `packages/console-components/src/shared.ts` with an `execCommand` fallback and
  a boolean return. It was package-internal and unexported, which is how a
  second implementation came to be written elsewhere: a helper you cannot import
  is a helper you rewrite. Exported rather than duplicated, so one owner remains.
  Both buttons now show an explicit failure mark with matching `aria-label` and
  `title`.

  Also fixed: both leaked their reset timer into an unmounted tree, where React
  reads `window` before discovering the update is a no-op and therefore throws
  rather than no-opping.

  Tested with real clicks against a navigator carrying no `clipboard` - the
  plain-http shape, not a clipboard that rejects, which is a different branch and
  was never the bug. The secure-context trigger is in the test names because the
  defect is invisible on localhost, which is exactly where a copy button gets
  tested. Mutation-proven: restoring the original call plus the unconditional
  success mark turns both new tests red.

  `packages/flow-editor-components` carries the same raw call in two places and
  is deliberately untouched: it declares no dependency on `console-components`,
  so reusing the helper there would add a cross-package dependency purely to
  share a utility. It needs a decision about where a shared helper lives.


### Added

- **CI now runs the TypeScript SDK suite.** It ran nowhere. 705 tests across 20
  files existed and gated nothing: no job touched `sdk/typescript`, and the only
  Makefile reference is `publish-dry-run-typescript`, which builds and packs
  without testing. A wrong RPC method name, or a parser reading camelCase where
  the gateway sends snake_case, reached npm behind a typecheck that cannot see
  either.

  Found by applying the reviewer's own standard to this release's TypeScript
  contract test: having just been asked to prove the gateway-backed Python test
  executes rather than skips, the same question asked of the new TS test
  produced "it never runs in CI at all".

  The job runs `validate` (typecheck, then build, then test) rather than `test`,
  because most of these tests import the built `dist/` and `test` alone fails
  with `ERR_MODULE_NOT_FOUND`. It is wired into `gate`'s result comparison, not
  merely into `needs` - with `if: always()` a job listed only in `needs` is
  advisory and cannot fail the suite. Two contract tests in
  `scripts/test_ci_workflow.py` pin both halves, and both are mutation-checked:
  downgrading the step to `typecheck`, or dropping the job from the result
  comparison, each turn one red.


- **`mobkit/identity/routing_status` exposes meerkat's typed model-routing
  status per identity.** The result is
  `meerkat_core::image_generation::SessionModelRoutingStatus` verbatim
  (`WireSessionModelRoutingStatus` is a type alias to it), flattened under
  `identity` and `session_id`. MobKit declares **no mirror type**: a second
  declaration of an upstream shape is a second place for it to drift, so the
  payload cannot disagree with meerkat's contract.

  The fact this carries that nothing else does is `session_provider`, the typed
  provider of the session's current LLM identity. Meerkat documents re-deriving
  a provider from the effective model string as silently wrong for models owned
  by a custom `ModelRegistry`; an absent `session_provider` means the machine
  has no hydrated session LLM identity yet, and is **not** an invitation to
  re-derive one.

  Wired on **both** dispatch planes - the stdin JSON-RPC router in `rpc.rs` and
  the HTTP console's own match in `http_console.rs` - plus the console ABAC
  classifier (`ACTION_AGENT_VIEW`, the same grant as `identity/resolved_tools`),
  both capability advertisements, and the Python and TypeScript SDKs. The
  classifier entry is not optional bookkeeping: an unmapped console method
  returns `None` from `console_rpc_access_requirements`, which makes the access
  gate short-circuit to *allowed* and the capability filter advertise it to
  everyone. Omitting it produces no compile error and no test failure.

  **This method requires a resolved session.** Identity-first materializes an
  identity without activating it, so an identity that has been materialized but
  never addressed has no session and therefore no routing status. That is
  structural - the status is per-session machine state - and it is the expected
  state after a restart, not a defect. A fleet sweep must address before reading
  and label its coverage post-address.

  Failures carry a machine-readable `reason` so a sweep can classify an identity
  rather than only fail it. Each reason is derived from a fact MobKit actually
  observed, never inferred from a single upstream error:

  | `reason` | Derived from |
  |----------|--------------|
  | `runtime_unsupported` | the session service exposes no runtime adapter |
  | `no_current_session` | the roster reports no current session (see below) |
  | `member_lookup_failed` | the mob could not resolve the member at all |
  | `session_not_held` | a session resolved **and** meerkat answered `NotFound` |
  | `upstream_read_failed` | a session resolved and the read failed some other way |
  | `invalid_identity` | no usable identity was supplied |

  `no_current_session` is named for the fact observed, not a cause inferred from
  it. MobKit's `member_status` answers an unknown member with a well-formed
  "unknown" status carrying no session rather than an error, so this one
  observation covers **both** an identity materialized but never addressed (the
  normal state at boot) **and** an identity that does not exist at all,
  including a typo. This surface cannot distinguish them, so a caller sweeping a
  fleet must assert it received a status for every identity it expected rather
  than merely that nothing raised. Distinguishing them would need a roster read
  that reports absence as absence; that belongs to `member_status`, and is
  deliberately not papered over here. An earlier spelling of this reason
  asserted the first case and was wrong for the second.

  `session_not_held` is matched on the `RuntimeDriverError::NotFound` **variant**,
  never on message text. Because that error is `#[non_exhaustive]`, every other
  upstream failure - `NotReady`, `Destroyed`, `RecoveryCorruption`, and anything
  added upstream later - lands in `upstream_read_failed`, which deliberately
  diagnoses nothing. A consumer escalates on `session_not_held`; widening it to
  mean "the read failed" would fire that escalation on states that are not
  missing sessions at all.

  Mutation-proven rather than merely green, on every surface. On the Rust side,
  removing either dispatch arm, the typed error payload, the ABAC classifier
  entry, or the `NotFound` narrowing each turns a specific test red. The
  TypeScript contract test is executed rather than typechecked, and detects a
  wrong RPC method name, a parser reading camelCase instead of the wire's
  snake_case, and a coerced-away `session_provider`. The gateway-backed Python
  test was confirmed to **execute** rather than skip, by building `rpc_gateway`
  and then breaking the SDK method name to watch it go red. An earlier sweep was **discarded** as
  worthless - its three "failures" were rustc ICEs in `identity_first/runtime.rs`
  under incremental compilation, so the tests never ran. A build that breaks is
  not a test that detects.

## [0.8.28] - 2026-08-29

Pairs with meerkat `0.8.31` (tag `v0.8.31`, commit
`83d5be6d75ebdf9a54106bf7b102617fb8114669`) - unchanged from 0.8.27. All 25 pin
sites are byte-identical to the 0.8.27 release commit; this train carries no
upstream movement.

The backlog this release clears was known during 0.8.27 and left out of it. The
0.8.31 deadline was meerkat's, not MobKit's, so holding these items back bought
nothing.

### Fixed

- **A graceful gateway exit now releases the schedule executor lease.** Both
  gateway binaries bound `_schedule_host` and relied on `Drop`. Dropping a
  `ScheduleHostHandle` only *signals* the supervisor; only
  `ScheduleHostHandle::shutdown().await` calls
  `driver.release_executor_lease()`, and tokio gives no guarantee the woken
  supervisor is polled between main's completion and runtime teardown. Measured
  on a production store: `owner_id`, `lease_token`, `acquired_at_ms` and
  `expires_at_ms` all still set 57s past **gateway exit**. (That observation was
  originally reported as following a graceful shutdown; its owner has since
  corrected it. The exit was driven by stdin EOF, so it observed the `Drop` path
  rather than a graceful one. The row reading is unaffected, and both paths run
  the same teardown in `main`. Direct evidence for the graceful path is this
  release's own mutation test, below, which SIGTERMs the binary.) The
  replacement process
  then gets `AcquireScheduleExecutorLeaseOutcome::Busy`, its tick returns
  without claiming due occurrences, and **schedules do not fire for up to the
  60s lease duration after every restart**. Nothing surfaced this: the claim
  watchdog's overdue threshold is longer than the window, and the old code
  compiled clean with no warning. The release now runs before
  `composition.shutdown()`, because releasing the lease is a store write and
  composition teardown is what closes the store.
- **`delegate` now counts as evidence that the mob tool surface is present.**
  `DeclaredToolCategory::Mob` matched nine exact names plus the `mob_` prefix
  and missed `delegate`, so a catalog carrying only `delegate` read as a `Gap`
  against a declared Mob category and parked a healthy member. Latent rather
  than live: meerkat enables the mob tools as a group and a `mob_`-prefixed
  sibling has always co-occurred, so no spurious park was ever observed. The
  classifier should not depend on that.

### Added

- **The decorator-authority defect class is watched by a gate rather than by
  memory.** `scripts/verify-decorator-authority.py` enumerates every
  `impl AgentLlmClient`, classifies production from `#[cfg(test)]` structurally,
  and fails any production decorator missing `request_attempt_authority`. This
  is the 0.8.27 defect that stranded 72 identities while compiling clean; the
  per-wrapper unit tests cover the wrappers that exist, and this covers the ones
  that do not exist yet. It runs in the `fmt-lint` CI job and on pre-push, and a
  contract test in `scripts/test_ci_workflow.py` pins that invocation - a
  structural gate nobody runs is a control that cannot fail.
- A subprocess regression test for the schedule lease
  (`tests/gateway_schedule_lease_release.rs`), covering both binaries. It
  asserts that the running gateway holds the lease and then that a SIGTERM exit
  releases it; without the first assertion a gateway that never acquired a lease
  would also end with `owner_id` NULL and the test would pass while observing
  nothing.

  Reverting the fix and re-running is honest about what each leg is worth:
  `rpc_gateway` fails on the exact assertion first try, and `mobkit_gateway`
  passes 7 runs out of 7. Only the `rpc_gateway` leg is a mutation-proven guard.
  Under `mobkit_gateway`'s teardown the signalled supervisor does get polled in
  time and `Drop` releases the lease anyway - which is luck the harness cannot
  remove, not a property the code states, so the explicit call stays in both
  binaries. The `mobkit_gateway` leg is kept for the harder regression (a lease
  never released at all) and is documented in the test as not proving the
  narrower one.

### Changed

- The three hardcoded mob tool-name lists are now named constants that state the
  question each one answers: `MOB_SPAWN_TOOL_VOCABULARY` (did this call create a
  member the console must render?), `SPAWN_INITIAL_MESSAGE_TOOLS` (a documented
  subset whose argument shapes the initial-message extractor understands), and
  `MOB_UNPREFIXED_TOOL_NAMES` (does this catalog prove the mob surface is
  wired?). They are deliberately **not** unified: they answer different
  questions and are free to diverge. Each carries a pinning test that forces a
  human decision rather than accepting a diff.
- `memory::dispatch_taint`'s module doc no longer claims the module collapses
  onto an upstream hook slot once one lands. Hook points did land upstream and
  this module does not collapse onto them: meerkat's own reference states that
  post-commit hooks are not synchronous policy seams.

### Not in this release

- **The shutdown wedge remains open, and it is not MobKit's code.** A graceful
  stop can fail to converge in `shutdown_runtime_unregister`, escalating SIGTERM
  to SIGKILL. The symbols live upstream: `shutdown_runtime_unregister_observers`
  is defined and driven in `meerkat-mob/src/runtime/actor.rs`, as is the
  `shutdown session binding teardown failed` warning. Both are absent from
  `meerkat-mobkit` entirely, which only calls into them through
  `UnifiedRuntime::shutdown()`. Earlier notes, including mine, put this in
  MobKit's own teardown; that attribution was wrong.

  **It is not a regression.** ob3 reproduced it on meerkat 0.8.29 / MobKit
  0.8.24 - their current production pins - with the same harness, phases and a
  freshly cloned dataset. Across three pin pairs on the same arm: 0.8.29/0.8.24
  hung, 0.8.30/0.8.26 hung 2 of 2, 0.8.31/0.8.27 hung 4 of 6. It predates all of
  them, and no CHANGELOG should carry it as new in 0.8.30, 0.8.31 or here.

  It went unseen because the harness could not express it: before 2026-08-28 the
  twin's `Server.kill()` escalated SIGTERM to SIGKILL after 30s, silently and
  without an assertion, while a healthy shutdown on that fleet takes 74 to 92
  seconds. Every shutdown was killed at 30s and wedged and healthy runs looked
  identical - green. The defect is old; the instrument that can see it is days
  old.

  It is a **race**, not a deterministic trigger: ob3 measured 4 hangs in 6 runs
  on identical 0.8.31 Rust, alternating clean and hung with the same dataset
  shape, phases and spawn count. The conditions previously believed to form a
  required conjunction only raise the probability.

  **Do not read "4 in 6" as the odds that any given graceful stop hangs.** It is
  a conditional rate under adversarial setup: every one of those runs booted 161
  identities, drove a real OpenAI turn, a real Anthropic turn and a spawn, then
  SIGTERMed immediately with no idle period. ob3 also corroborated it in real
  production: 6 of 9 pod shutdowns in a 30-day window did not complete, with a
  control ruling out log loss.

  **There is no second-fleet counter-datum.** An earlier draft of this entry
  cited a second fleet's 79 clean restarts. Its owner retracted that measurement
  outright: the sample was from July, on versions no longer running, under a
  since-replaced deploy path. Their instrument then stopped emitting in mid-July
  - startup and shutdown lines disappeared together, which is a logging change
  rather than a behaviour change - so that fleet's current shutdown behaviour is
  simply **unobserved**, in either direction. It is withdrawn rather than
  weakened, and nothing here rests on it.

  So no fleet-shape hypothesis is offered either. The one previously carried
  here - fleet size, 17 identities against 161 - existed only to explain a
  discrepancy between two fleets, and it does not survive the discrepancy being
  withdrawn. The reproducer is held by its owner.

  Two consequences worth carrying forward. Any regression test for this must
  **repeat rather than sample** - a single green run has roughly a 1 in 3 chance
  of meaning nothing under ob3's conditions. And the teardown retry count is
  **not** a severity signal: across two independent measurement sets the highest
  count (209, and earlier 161) sat on the only *clean* run each time.

  This release's lease fix is a different defect and almost certainly does not
  address this one. The lease release should nonetheless survive a wedged run,
  on structural grounds rather than measurement: `GatewayComposition::shutdown`
  is what calls `state.runtime.shutdown()`, and the lease release is placed
  before that call, with everything in between either bounded (the inflight
  drain, by `GATEWAY_RPC_DRAIN_TIMEOUT`) or non-awaiting. That ordering was
  chosen for an unrelated reason - the release is a store write and composition
  teardown closes the store - so the independence is a consequence, not a design
  goal, and it has not been observed against ob3's reproducer.

  Named here so it is explicitly outstanding rather than silently dropped.

## [0.8.27] - 2026-08-29

Pairs with meerkat `0.8.31` (tag `v0.8.31`, commit
`83d5be6d75ebdf9a54106bf7b102617fb8114669`), bound to the published registry
release across all **25** pin sites (20 in `meerkat-mobkit`, 5 in
`mobkit-store-conformance`).

The 0.8.26 entry below says "26 pin sites". That number is wrong; it has been 25
in both directions, measured out of the transitional git-rev state and again
here. Correcting it in place would rewrite a shipped entry, so it is corrected
here instead.

### Fixed

- **Agent-LLM decorators no longer downgrade the clients they wrap.**
  `AgentLlmClient::request_attempt_authority` carries a default returning
  `LegacySplit`, so MobKit's decorators silently reported their own authority
  instead of forwarding the inner client's - compiling cleanly, with no error,
  warning or test failure. Meerkat 0.8.31 rejects `materialize resume` for a
  client reporting `LegacySplit` over a `Unified` adapter, which stranded every
  identity resuming a durable session. Measured downstream on a cloned
  production store: 72 identities marked Broken at boot before the fix, 0 after,
  against 0 on three separate 0.8.30 runs. Addressing a stranded identity did
  not recover it, because addressing triggers the resume that is rejected.
  Both production decorators - `ReplaySanitizingAgentLlmClient` and
  `TaintObservingLlmClient` - now forward. One contract test per wrapper, each
  using a non-default inner so forwarding is distinguishable from inheriting.

### Changed

- Repinned all 25 exact meerkat pins from `=0.8.30` to `=0.8.31`. Resolution is
  asserted rather than assumed: 35 meerkat packages, all from crates.io, none
  from git, none off-version.
- `ProviderBindingConfig` gained `credential_account`; MobKit's single
  construction site supplies `None`, which is behaviour-preserving.

### Added

- **The release path refuses a commit that was never green on main.** A
  `require_ci_green` job now gates the whole release DAG; previously
  `release_validate` had no `needs` at all, so a tag published from any commit
  and nothing checked it. It refuses rather than waits, and the dry-run lane is
  deliberately exempt because it publishes nothing.
- **Binaries carry build provenance.** `attest-build-provenance` over `dist/*`;
  the workflow granted `attestations: write` and never attested anything.
- **Every workspace member's publish status is explicit.**
  `verify-crate-publication.py` fails unless each member is either published by
  `release.yml` or declares `publish = false`, deriving the published set from
  the workflow rather than restating it.
- e2e suites can execute a prebuilt example via `MOBKIT_EXAMPLE_BIN_DIR` instead
  of invoking cargo. Set nowhere; behaviour is unchanged.

### CI

- Split the serial `console` job into `console`, `console-fixtures` and
  `console-acceptance`. Two steps accounted for 22.3 of its ~38 minutes while
  all nine npm check scripts took 0.0m each, so separating "fast checks" from
  "slow e2e" would have bought nothing. All three are named in `gate`.

## [0.8.26] - 2026-08-27

Pairs with meerkat `0.8.30` (exact rev `5e229e0b8379ac162a6b3c69187d186b570535e9`),
bound as an immutable Git rev across all 26 pin sites.

### Added

- **Experimental GPT Live channel surface** (`experimental-gpt-live`, off by
  default). Realtime audio/live sessions route through the client context, with
  a live surface in both SDKs (`sdk/typescript/src/live.ts`, Python live
  contracts) and console replay wired to live subscription. The feature is
  opt-in: a default-feature embedder acquires none of its dependency graph.
- An exclusive session-identity config root declared by the gateway, so the
  identity root a session resolves is stated rather than inherited ambiently.

### Changed - BREAKING for host/SDK implementors

- `resolve_record_by_session` is now **required** on `ContinuityStore`; its
  `Ok(None)` default is gone, and both SDKs answer it. A store that silently
  reported "no record" for a session it was holding made a resume look like a
  first boot, and a default is exactly how an implementor inherited that
  behaviour without choosing it. Hosts implementing the trait must add the
  method; the compiler names every site.

### Fixed

- Persisted owner authority is published **before** mob prepare, so a revived
  session's owner is registered when the lift happens instead of after it.
- Persisted runtime authority converges before the bounded resume window, moving
  memoized convergence work out of a budget it was exhausting on large rosters.
- MobKit satisfies meerkat 0.8.30's now-compile-required committed-parent
  projection seam. Wrappers holding an inner forward; genuinely non-persistent
  doubles answer zero behind a fail-closed `supports_persistent_sessions()`
  guard rather than a bare `Ok(0)`.

### Security / advisories

- RUSTSEC-2026-0150 (`audiopus_sys` 0.2.2 unmaintained) is waived with its scope
  recorded in `deny.toml`. It is informational with no patched release, reachable
  only under `experimental-gpt-live`, and its real exposure is a CMake 4 **build**
  failure rather than a vulnerability. See the waiver for the 0.8.31 follow-up.

### Note on the missing 0.8.24 and 0.8.25 entries

`v0.8.24` and `v0.8.25` were tagged and published without changelog entries; the
previous entry below is `0.8.23`. That gap is recorded here rather than papered
over, and it is deliberately NOT reconstructed after the fact - anything written
now would be inference about what shipped, not a record of it. For their actual
contents, read the tags directly:

    git log v0.8.23..v0.8.24
    git log v0.8.24..v0.8.25

Note also that this candidate's pre-bump commits self-reported `0.8.25` while
differing from the published `v0.8.25` tree, which is why this release is
`0.8.26` rather than a re-cut of `0.8.25`.

## [0.8.23] - 2026-08-25

Pins meerkat `=0.8.28`, which carries three upstream fixes this release depends
on. First, accepting an encoded stable roster identity as the successor of a
legacy generation-zero runtime binding - that is what lets already-persisted
sessions resume under the durable-roster contract with no migration on the
MobKit side. Second, `respawn_with_successor_spec`, the atomic successor-spec
respawn that restores reprofile-via-reset (see below). Third, treating an
omitted binding on a successor spec as "preserve the current one" rather than
"clear it"; without that, a successor respawn was refused for any locally-bound
member, because the roster does not expose a binding for the caller to restate.

### Changed

- **BEHAVIOR CHANGE: a persistent launch now pins `mob_config`.** Mob storage is
  persistent on launches that already persist everything else, which is what
  makes adopted identity declarations survive a restart. The cost is that the
  mob definition is pinned once the storage exists: meerkat refuses a definition
  that disagrees with the persisted spec store, on create and on resume alike
  (verified against 0.8.26 and unchanged in the pinned 0.8.28), so durable mob
  state and an editable `mob_config` cannot both hold on one storage path
  WITHOUT the declared spec update described under Added - which is the
  sanctioned way through this refusal, not around it.

  Editing `mob_config` and restarting against the same state directory is now
  refused with a typed error that names the diverged fields and states the
  remedy, instead of failing later as an internal store mismatch. Today the
  remedies are a new state directory (and re-adoption), or declaring mob state
  ephemeral with `runtime_options.mob_storage = {"storage": "memory"}`, which
  keeps `mob_config` editable at the cost of durable adoptions. A future
  upstream definition-migration path would update the authoritative event-log
  definition; rewriting the recorded provenance is deliberately NOT offered,
  because certifying a definition the event log does not hold would boot stale
  config silently.

### Added

- **A declared spec update, so the `mob_config` pin has a door.** A persistent
  launch pins the mob definition, and the typed refusal for a diverged definition
  is the right default - booting stale config silently is worse. But a refusal
  with no sanctioned way through it is a dead end, and it refuses the standard
  operating mode of any deployment that edits `mob.toml` between activations and
  clones its state directory (the persisted spec rides through every clone, so
  the next config-touching activation is refused).

  `runtime_options.declare_spec_update = {"expected_revision": N}` is an explicit
  operator declaration that the persisted spec now matches this definition,
  compare-and-swapped on the revision the divergence was observed at. Present
  only on the activation that intends to move the pin: a declared transition, not
  a mode. `expected_revision` is required rather than defaulted, because a
  declaration without the revision it was made against is not a declaration - it
  is "accept whatever is there", which is indistinguishable from having no pin.

  It refuses, fail-closed, when the revision moved between observation and
  declaration, when the payload names a different mob than its definition
  carries, when nothing is pinned, when the definition already matches, and when
  declared alongside an in-memory `mob_storage` (which pins nothing, so there
  would be no spec to move). Each of those would otherwise be a silent lie in an
  activation log. The receipt is logged with the previous revision, the committed
  revision and the declared fields.

  Divergence is reported as dotted field paths - `profiles.security.model` rather
  than `profiles` - because an operator about to declare through a refusal has to
  see what actually moved.

- **Reprofile-via-reset is restored.** Lowering destructive reset to one
  authoritative respawn removed it: a respawn preserves the predecessor's
  profile, so reset with a changed roster profile returned a typed refusal rather
  than silently keeping the old profile. meerkat 0.8.28 adds
  `respawn_with_successor_spec`, which applies the successor spec atomically, and
  reset now binds to it - so a reset that changes profile reprofiles again. The
  typed refusal remains for any case the successor transition cannot satisfy;
  what it will never do is report a reset that kept the old profile.

- **`runtime_options.mob_storage`.** Declares mob storage in-memory on an
  otherwise persistent launch, mirroring `runtime_options.runtime_store`. The
  storage census now reports the mob slot on both GATEWAY launch paths
  (`mobkit_gateway`, `rpc_gateway`); it previously declared nine slots and
  omitted this one, which is why an in-memory mob storage on a persistent launch
  was invisible to healthz and the storage doctor.

  Scope, stated because it is narrower than it first reads: the embedder path
  (`UnifiedRuntime::builder()` with no mob storage argument) is unchanged. It
  still composes `MobStorage::in_memory()`, so it declares no mob census slot
  and its `mob_config` stays editable - and its mob state is NOT durable, so an
  embedder that adopts identity declarations still loses them on restart. The
  durability fix below covers the two gateway paths only.

### Fixed

- **Mob roster identity is now the durable identity, for every binding.** MobKit
  lowered its own `AgentRuntimeId` into `SpawnMemberSpec.identity` for local and
  session-backed members, and the durable identity only for external ones.
  Meerkat defines `AgentRuntimeId` as a stable identity plus a
  machine-owned generation, where different generations are successive
  incarnations of the same member, and `SpawnMemberSpec.identity` is the roster
  identity. Nesting one inside the other made the roster id per-incarnation, so
  durable state keyed on it belonged to a single binding and could not survive a
  respawn or a restart. MobKit's `AgentRuntimeId` remains binding and
  incarnation detail; it is no longer a roster spelling, and every surface that
  used to convert it into a roster key now resolves through the durable
  identity with exact-session agreement.
- **A resumed mob is lifted out of the prior graceful stop before identity
  restore runs.** A replayed event log ends at the `MobStopped` of the last
  clean shutdown, so a resumed handle can come back `Stopped`. Bootstrap now
  lifts `Stopped` to `Running` once, and refuses `Completed`, `Destroyed`, and
  an unsettled `Creating` with typed errors, instead of handing identity restore
  a mob it can never spawn into.
- **Adopted identity declarations and mob events now survive a restart, on the
  two gateway launch paths.** `mobkit_gateway` and `rpc_gateway` composed
  in-memory mob storage while persisting sessions, so every restart presented as
  a healthy boot with the right member count and no durable mob state. The
  embedder path is NOT covered - see the scope note under Added. Storage mode is now attributed per launch branch: the
  ephemeral branches are unchanged and declare themselves in the census.
  Create-versus-resume is decided in one place from the event log rather than
  at each launch site, and storage supplied through the public bootstrap API
  with a non-empty log and no provenance declaration fails closed rather than
  resuming unverified.
- **A closing member event stream no longer races MobMachine for actuation.**
  When the identity store holds a valid `Present` intent, MobMachine is the
  sole actuator: the stream-health monitor no longer marks the identity-first
  runtime broken, and the resume path's roster-collision handler no longer
  retires the occupant. Both now read the same authoritative intent, and an
  unreadable or ambiguous intent fails closed and retryable instead of being
  treated as permission to destroy a live member. Only a valid `Absent` intent
  leaves identity-first its existing repair ownership.


## [0.8.22] - 2026-08-24

Pins meerkat `=0.8.26`.

### Fixed

- **Member declaration RPCs now have parity across stdin and the HTTP console.**
  Full-runtime console clients can adopt, read, and apply member tool declarations
  through the same canonical handler as stdin. Capabilities advertise only the
  methods the active console policy can serve, and read-only consoles expose the
  read method without exposing mutation methods.
- **Identity-first member aliases now resolve at the declaration boundary.**
  Public aliases are translated to the reserved roster identity internally and
  mirrored back on output, so callers never need to discover or submit an
  internal `mk--` identity. Scoped reads retain member-view ABAC checks, while
  adopt and apply remain guarded by runtime administration authority.

## [0.8.21] - 2026-08-23

Pins meerkat `=0.8.26`.

### Added

- **Member declaration control plane on the gateway.** Three typed stdin-RPC
  methods, mirroring meerkat's own catalog names and wire types exactly:
  - `mob/adopt_member_identity_declaration` - bring a live member under
    declarative management (one-shot CAS against live intent state).
  - `mob/apply_member_tool_declaration` - atomically update only the tool
    portion of an existing durable member intent.
  - `mob/member_tool_declaration` - read the live declaration and its desired
    intent revision, so the apply CAS is not guess-and-retry.

  Previously the gateway linked these types but reached none of them, so a host
  composing through MobKit could supply a compiled tool policy at init and never
  bind a member to it: every member stayed `Unmanaged` and the installed provider
  governed nothing. Requests are delegated straight to the canonical Meerkat Mob
  handle; no validation is reimplemented. Errors carry meerkat's typed jsonrpc
  codes and structured data rather than a generic failure. A request naming a
  different mob is refused with `foreign_mob_target` instead of being silently
  retargeted at the gateway's own mob.

  No SDK change is needed to call them: `MobKitRuntime._rpc_sync` takes an
  arbitrary method string. Note that `_rpc_error_from_payload` maps only MobKit's
  own codes to typed exceptions, so a typed Meerkat mob error arrives as a generic
  `RpcError` carrying `code` and `data`.

### Fixed

- **Supervisor cleanups retired by replacement are now owned and joined.**
  `start_identity_first_supervisors` cancelled the displaced lease-renewal and
  continuity-repair supervisors, then spawned their cleanup detached and dropped
  the handle, so a cleanup could outlive `shutdown()` while still holding the
  authority it was releasing. They are now retained and joined at shutdown, and a
  cleanup that does not reach its release boundary makes
  `runtime_cleanup_completed` false instead of being reported as a clean stop.

- **Shutdown responses name which phase blocked.** A failed runtime cleanup now
  carries typed per-phase diagnostics (drain timeout, mob stop, identity-authority
  release, orphan module processes, retired-supervisor cleanup) with numeric
  counts, rather than a single boolean. Successful responses are unchanged.

### Changed

- The advertised stdio shutdown horizon is 337s (was 335s), covering the new
  bounded retired-supervisor join. The Python and TypeScript SDK shutdown grace
  constants follow it.

## [0.8.20] - 2026-08-23

Pins meerkat `=0.8.26`.

### Added

- **MobKit now serves compiled application tool policies, because Meerkat
  defines that contract but ships no implementation of it.** Meerkat 0.8.26
  owns the compiled artifact, the provider and snapshot traits, and the registry
  that binds a member to a policy; the only implementations of those traits in
  the published crate live inside its own test module. `member_tool_policy`
  supplies the production pair: `CompiledPolicyProvider` serves the policies a
  boot was configured with, and `CompiledPolicySnapshot` evaluates one.
- **Declared in the gateway init payload as `"application_tool_policies":
  ["<canonical json>", ...]`.** Each entry is the exact canonical bytes of one
  compiled policy, carried as a string rather than a nested object on purpose:
  parsing verifies the digest against the bytes it is given, so re-serialising a
  nested object would check a digest against bytes MobKit had just manufactured
  instead of the ones the operator compiled. A malformed entry, a rejected
  policy or a registry that will not build refuses the boot, for the same reason
  a malformed `role_migrations` does - an operator who supplied a policy expects
  it in force, and arming nothing would resurface later as an unexplained access
  denial with nothing pointing at the payload.
- **Declarable from the Python SDK, which is how the only consumer composes.**
  `MobKitBuilder.application_tool_policies([...])` takes the compiler's canonical
  JSON as `str` or `bytes` and passes it through unchanged into the gateway's
  init params. An earlier draft parsed the parameter in the gateway while no SDK
  could send it, so the feature was reachable only by hand-writing raw
  InitParams JSON. The role-migration carrier shipped with the same gap and was
  caught the same way; one shared wire fixture is now read by both the Rust
  parser test and the Python builder test, so renaming the key goes red on one
  side instead of both staying green while a host arms nothing. TypeScript is
  deliberately not given this surface: no deployment composes through it.
- **The provider identity comes from the artifact, never from MobKit.** A
  compiled policy declares its own author, and a member's
  `ApplicationToolPolicyBinding::Provider { provider_id, policy_id }` is
  resolved by that carried id, so the gateway registers one provider per
  distinct author it was given and names none itself. An earlier draft
  hardcoded a single provider id, which would have refused every policy
  compiled by anyone else - not a narrower feature but a dead one.
- **Rollback refusal is MobKit's, by Meerkat's design.** The provider trait
  documents that the provider owns the snapshot pointer and must reject a
  revision below one already accepted, because Meerkat "deliberately keeps no
  second accepted-revision store". So the fence lives here, and it refuses in
  Meerkat's own vocabulary: a lower revision is `RevisionRollback`, and a repeat
  of an accepted revision whose bytes differ is `RevisionDigestConflict`. The
  second half matters because a content swap under a stable revision would leave
  the digest as the only evidence and nothing downstream re-checks it.
- **Execution authority is the grant, not the consequence class.** In v1 a grant
  is one exact allow entry with no wildcards and no deny entries, its only
  action is `Invoke`, and neither the request nor the verdict carries a
  threshold. So an exact member/tool grant allows, a miss defers to the
  artifact's own `default_deny`, and the `R0`-`R3` consequence class is reported
  without gating execution. Choosing a threshold here would have made MobKit the
  authority on what `R2` means, which is Meerkat's to define if it is ever
  wanted.

## [0.8.19] - 2026-08-21

Pins meerkat `=0.8.25`.

### Added

- **Boot-scoped member role migrations, so a role change is a decision the host
  states out loud rather than a silent restamp.** A durable member whose role
  changed refuses to resume (`MobError::MemberRoleMigrationRequired`) until this
  activation's host declares the migration. That refusal is deliberate: a stray
  role edit in a roster otherwise restamps the member's durable role, comms name
  and binding, and nothing afterwards points at the edit as the cause. Declare it
  in the gateway init payload as `"role_migrations": [{"identity":
  "domain:home-automation", "from_role": "domain"}]`, or from the Python SDK as
  `MobKitBuilder.role_migrations([...])`, which takes
  `RoleMigrationDeclaration` dataclasses or plain dicts and validates both.
- **The declaration is authority for one boot, not state.** It is installed on
  `MobSessionBridge` - which lives for exactly one boot - via
  `with_role_migration_declarations`, and `resume_session` passes it into
  `MemberLaunchMode::Resume { resume_from_role, .. }` by EXACT identity lookup:
  no durable read, no inference from stored role metadata, no retry of a resume
  meerkat refused. Nothing persists it, so it is absent by default and dropping
  it from the next boot payload is how the authority goes away. Meerkat decides;
  MobKit only carries. It re-verifies the declared predecessor against durable
  state and refuses on mismatch (`MobError::MemberRoleMigrationRejected`), so a
  mistyped `from_role` cannot authorize an unintended restamp, and it ignores
  the declaration entirely once the roles already agree - a declaration left in
  place after a completed migration is inert, not a repeat restamp.
- **Wired in BOTH gateway binaries, whose names differ by one word.**
  `rpc_gateway` reads it from its untyped init params and installs it inside the
  roster-provider branch (the identity-first path the SDKs drive);
  `mobkit_gateway` takes it as a typed `InitParams.role_migrations` field and
  installs it inside its identity-first block. Declarations that reach
  `rpc_gateway` with no roster provider, or `mobkit_gateway` with
  `identity_first: false`, arm nothing, because there is no identity plane to
  migrate on. On both binaries the per-declaration log line sits immediately
  before the install, inside that same branch, because a restamp of durable
  role, comms name and binding belongs in the boot record; a payload that
  reaches neither identity plane installs nothing and is not logged either.
- **A malformed payload refuses the boot** rather than arming nothing and
  leaving the operator to rediscover it as the original refusal. Both binaries
  refuse it with `-32602`, and neither gates that refusal on having an identity
  plane: `rpc_gateway` parses the key at init scope, outside its
  roster-provider branch, and `mobkit_gateway`'s typed init params fail to
  deserialize before any of the boot runs, so a host with no identity plane
  still learns its payload is broken. Declaring one identity twice with CONFLICTING `from_role`
  values is refused too, because collecting them into a map would let hash
  order pick which predecessor role becomes authority - but the two binaries
  place that check differently. `rpc_gateway` refuses with `-32602` alongside
  the malformed case, ungated. `mobkit_gateway` checks only inside its
  identity-first block, so it refuses from there as a `-32603` internal error
  with a null `id` rather than the answered `-32602`, and under
  `identity_first: false` a self-contradicting payload is not refused at all;
  `identity_first` defaults to true, so a fresh boot on the default path does
  refuse. The Python builder raises `ValueError` on the conflicting pair before
  any of this reaches a gateway. An IDENTICAL repeat is accepted on purpose: a host proves
  inertness by carrying the same declaration into a later activation and showing
  that nothing moves. Unused, the Python builder emits no
  `role_migrations` init-params key at all.
- The wire key names are pinned by one shared fixture,
  `meerkat-mobkit/tests/fixtures/role_migrations_init_params.json`, read by both
  the Rust parser test and its Python twin, so a rename on either side goes red
  instead of both staying green while a host arms nothing.

### Fixed

- **A gating caller could claim its own risk tier and have the claim
  honoured.** `apply_gateway_runtime_config_to_request` filled `risk_tier` only
  when the caller had omitted it, so a client sending `risk_tier: "r0"` on
  `mobkit/gating/evaluate` for an action the compiled `action_risk_tiers` table
  declares `r3` kept its own claim. `evaluate_gating_action` branches on
  whatever tier it is handed, so the table compiled from
  `runtime_options.gating_config_path` was advisory against exactly the caller
  it exists to constrain. A policy table a caller can override is not a policy,
  it is a default - and the name says otherwise, which is the part that bites:
  the next person wiring gating reads `action_risk_tiers` as authority. The
  configured tier now wins whenever the action appears in the table. An
  overridden caller claim is logged at WARN with both tiers rather than dropped
  silently, because a caller that keeps sending an overridden tier must be able
  to find out it is being overridden. An action absent from the table still
  keeps the caller's value: the table binds what it declares and no more.
  Reported by HomeCore's tools/grants fork with file:line, verified here before
  fixing. The regression test is mutation-proven: restoring the
  `!params.contains_key("risk_tier")` guard turns it red with left `"r0"`,
  right `"r3"`.

- Corrected the public documentation, crate rustdoc, and repository guidance
  to match the shipping gateway handshake, event subscription and SSE names,
  runtime-owned gating, delivery and module contracts, member roster
  projections, gateway profiles, and the Meerkat/MobKit ownership boundary.
  The obsolete storage-unification implementation plan is now retained only
  as a dated historical record.

## [0.8.18] - 2026-08-19

Pins meerkat `=0.8.24`.

### Operator and adopter guidance

This section carries the operator-facing guidance for the release. The GitHub
release body is written FROM it, not alongside it: 0.8.17 shipped with its
guidance only in the release body and no CHANGELOG section at all, and nothing
in CI can see that drift.

**If you switch on the new `ErrorCategory` members, write explicit match arms.**
Both SDKs have always passed unknown categories through as raw strings, so an
`if/elif` chain or a `switch` written against the old five members still
compiles, still runs, and now sends six additional categories - including every
one we tell you to page on - down the catch-all branch. Adding the members to
the enum does not route them; your arms do. In Python, match on
`ErrorCategory` and keep a final `case _:` that logs the raw tag so a future
addition is loud rather than silent. In TypeScript, exhaustive-check the union
with a `never` assertion in the default branch so the same addition fails at
compile time. Treat `actor_loop_recovered` as a RESOLUTION, not an incident -
use `ErrorEvent.is_resolution` / `isResolutionErrorEvent()` rather than
listing categories yourself, or a resolved stall raises a second page.

**Operator recovery matrix** (supersedes the 0.8.17 matrix; the
`compact_member` row changed this release):

| Situation | Action |
|---|---|
| Member's context has overgrown | `mobkit/compact_member`, then `mobkit/bound_member_transcript` if it is still too large |
| `compact_member` timed out | Read the error: it now names the turn's fate. "Quiesced by the rollback" means the member is yours again and you may retry. "Still running on the floored build" means the member is briefly unresponsive - wait, do not stack a second verb on it |
| Console reads returning `-32017` | The member is mid-turn on a long tool chain or compaction, not wedged. The `awaiting` field names which loop. Retry, or raise `MOBKIT_CONSOLE_READ_TIMEOUT_SECS` for a known-slow deployment |
| `ActorLoopStalled` paged | Wait for the `ActorLoopRecovered` carrying the same `stall_id` before escalating. Zero `prior_resolved_stalls` and no resolution arriving is the wedged shape; a high count is merely a chronically busy loop |
| `CompactionPersistenceRejected` paged | Check the typed fit and `attempted_entries`. On a meerkat pin older than 0.8.23 this is the epoch-seam defect and the member is permanently wedged: retire and respawn |
| Member is wedged | `respawn` does NOT recover it (continuity-preserving; it resumes the same strand). `reset` recovers but destroys the member's entire history - last resort |

**Quiesce before you stop.** `bound_member_transcript` already refuses with a
typed Busy on an in-flight turn, and teardown now reports
`ProceededWithoutInterrupt` rather than claiming a clean stop it did not
achieve - but both of those are the platform telling you it could not do the
safe thing, not the platform doing it for you. Let in-flight turns finish, or
stop them deliberately, before tearing a mob down. A stop that races a member's
kickoff leaves a turn admitted and possibly still running, and the honest
report of that is still an operational loose end you have to close.

### Fixed

- **Console read arms could hang indefinitely.** The gateway awaited the member
  session task or the mob actor loop with no bound at all, and both are strict
  sequential command loops, so any long turn (a tool chain, a post-cycle
  compaction) made every console read on that member queue silently instead of
  degrading. Reported by OB3 as `mobkit/identity/resolved_tools` hanging past 60
  seconds with no completion. The seven read arms that cross those two loops -
  `identity/resolved_tools`, `member_status`, `list_members`, `get_member`,
  `find_members`, `flow_status`, `list_runs` - are now bounded and return a
  typed error code `-32017` naming the arm and the seam it was awaiting, with
  structured `data` (`kind`, `arm`, `awaiting`, `timeout_secs`) for SDK callers.
  Budget is 30s, overridable via `MOBKIT_CONSOLE_READ_TIMEOUT_SECS` and clamped
  to `[1, 3600]`. Reads that do NOT cross those loops are deliberately left
  unbounded: `memory/query` can legitimately run long over a large HNSW index,
  and `cross_mob/peer_info` runs inside a member authority transaction where
  abandoning the future is not safe.
- **`mobkit/compact_member` left its maintenance turn running on timeout,** so
  later console reads queued behind the very turn the operator had given up on.
  The turn is now actually stopped, through the mechanism already on the path:
  the error path's rollback rebuild retires the member, and retirement quiesces
  the session's active runtime turn. The error now states the turn's real fate -
  quiesced by the rollback, or still running on the floored build with the
  member briefly unresponsive - rather than leaving the caller to guess. The
  post-timeout transcript read that decides whether the forced compaction had
  already landed runs under its own 5s bound, so unproven evidence is dropped
  from the message instead of traded for a second hang.
- **A member could appear to answer nothing at all, for days, while answering
  normally.** On identity-first gateways `MobKitConsoleAggregator::send`
  reserved its pending interaction under `(alias, alias)`, and since the roster
  alias there IS the fenced runtime incarnation, that self-map OVERWROTE the
  spawn path's incarnation-to-durable-identity registration - process-wide, for
  every ingress, until the process restarted. From then on the console resolved
  every live agent event for that member to `{identity}:{generation}`, while the
  `user_input` frames (built separately, from the durable identity) kept landing
  on the bare conversation the UI renders. The result is a thread showing
  questions and never answers.

  **Scope, corrected by HomeCore after the first fix:** this was reported as a
  `/dev/dispatch` problem because that is where it was first hit, and describing
  it that way understates it badly. A recurring SCHEDULED prompt reaches the
  runtime through `WorkSpec`/`MobHandle` and reserves nothing on the console
  store, so it never keyed anything itself - it was collateral damage of the
  poisoned map, and it is the path scheduled household work actually runs on.
  The operator-visible symptom is a scheduled agent that appears dead. If you
  have an agent that "stopped responding" and looked healthy when you checked,
  this is a candidate explanation, and it is not a dispatch story.

  **Whether you saw it depends on which history your UI renders, not on which
  version you run.** The split is in MobKit's console conversation store, so it
  reaches operators whose UI reads that store. An adopter whose dashboard reads
  its OWN session store (OB3 renders `/api/dashboard/chat/history` off their
  BigQuery store) saw completions normally throughout and would never have
  noticed. Check which endpoint your console calls before concluding you were
  unaffected - and note that mounting `/console/*` is enough to be exposed:
  anyone opening the stock console directly on such a deployment hits this in
  full, however insulated the primary dashboard is.

  Fixed at the existing mechanism, no new mirroring layer: the reservation keys
  the pending interaction under the member's DURABLE console identity with the
  incarnation as the mapping KEY - the same registration the spawn path makes.
  Classic (non-identity-first) mobs were never affected, because there the
  roster alias IS the identity and the self-map was benign.
- **`send_message` self-mapped the incarnation on its `AuthorityUnavailable`
  arm,** the same shape on a second call site, also found by HomeCore. It now
  reserves under the durable identity with the incarnation as the mapping key.
- **Every `ErrorEvent` now reaches the log whether or not an error hook is
  wired.** Previously the three fire points only logged through a registered
  hook, so a host that never called `on_error` - HomeCore's case - had every
  `ErrorEvent` this platform ever emitted go to `None` with nothing in the log
  either. The default sink logs at ERROR (INFO for resolutions) and records
  `hook_registered`, so an absent hook is visible as an absent hook rather than
  as an absence of failures.
- The Python and TypeScript SDK `ErrorCategory` enums declared only five of the
  nine Rust `ErrorEvent` variants, so a host could not write a typed alert arm
  for `compaction_persistence_rejected`, `actor_loop_stalled`,
  `event_log_flush_failure`, or `identity_materialization_failure` - the very
  events operators are told to page on. The events always arrived (both SDKs
  pass unknown categories through as raw strings), but only as a catch-all.
  All four are now declared in both SDKs, with `message` formatting mirroring
  the Rust `Display` impl instead of falling through to a raw JSON dump.
- Two further variants added by sibling lanes in this same release,
  `actor_loop_recovered` and `mob_stop_proceeded_without_interrupt`, are
  likewise declared in both SDKs with faithful `message` arms. The parity gate
  below caught both on its first integration run, before either could ship
  invisible to SDK hosts.

### Added

- **A per-turn wall-clock ceiling you can actually set: `max_turn_duration` in
  mobpack layer specs** (via meerkat 0.8.24's `BudgetLimits`). This is the
  missing owner for the question operators have been asking all cycle. Every
  segment of a turn already had its own bound - per-call LLM timeout,
  stream-inactivity watchdog, per-tool-call timeout - but their SUM had no
  ceiling, so a turn could legally spend an hour without anything deciding that
  was too long. Note the epoch: `max_duration` is agent-lifetime, measured once
  at agent construction and never re-armed, so it is not a per-turn deadline;
  `max_turn_duration` is re-armed at each turn entry.
  Absence stays absent - a spec that does not set it keeps its historical
  canonical bytes, so `spec_digest` pins already recorded in host stores still
  match, and leaving it unset is a real choice for unbounded turns rather than a
  default someone invented for you.
- **An ambiguous peer delivery is no longer rendered as a plain failure on the
  SSE surface.** `map_runtime_error` now has a typed arm for
  `SendError::AmbiguousDelivery`, returning `delivery_outcome_ambiguous` with the
  `envelope_id` and `required_action: "reconcile"` instead of collapsing into
  `internal_server_error`. The envelope may already be on the receiver's queue,
  so "retry" - the action a bare 500 invites - is the one unsafe move.
  `retry_safe` is deliberately ABSENT rather than `false`: 0.8.24 comms still
  folds connection-refused into this variant, where nothing left the host, so
  asserting non-retryability would be a claim we cannot substantiate. The HTTP
  status is not the signal here and is not meant to be read as one; the body is.
- **`ErrorEvent::ActorLoopRecovered`, and a stall you can close.** 0.8.17's
  `ActorLoopStalled` could only ever open an incident: nothing ever reported the
  stall ENDING, and its one number (`probe_waited_secs`) was the configured
  budget echoed back, identical on every page. The stall now carries `stall_id`
  plus `prior_resolved_stalls`, and the resolution arrives as a sibling event
  carrying the same `stall_id` and the true `stalled_for_secs`. Note the
  direction before wiring an alert: the probe parks on the same round trip
  instead of starting a new one, so a genuinely WEDGED loop pages once and never
  increments again, while a merely slow loop recovers and stalls afresh. A high
  `prior_resolved_stalls` therefore means "repeatedly late"; the wedged loop is
  the one sitting at zero priors with no resolution ever arriving. Distinguishing
  busy from wedged still needs a progress counter on the meerkat-owned loop,
  which mobkit cannot observe from here; that ask is filed upstream.
- **`UnifiedRuntime::stop_mob_for_teardown()` returning `MobStopOutcome`.**
  Teardown could be refused for its entire window on runtime-attach readiness
  and still be reported as a clean stop. The outcome is now explicit -
  `Stopped`, `ProceededWithoutInterrupt { waited_ms, member, error }`, or
  `Failed(..)` - and the middle variant says exactly that and nothing stronger:
  a mid-attach member has already had a turn admitted and its kickoff is about
  to bind, so calling that an interrupt would be a false success the caller
  cannot detect. It also fires as `ErrorEvent::MobStopProceededWithoutInterrupt`,
  so the condition reaches a pager and is never swallowed. Upstream root cause
  (a refusal the caller has no action to clear) is accepted as a meerkat P1;
  this degrade makes mobkit teardown independent of that timing either way.
- **The compaction rejection alert now carries meerkat's typed fit.**
  `CompactionPersistenceRejected` previously reported only a reason string, so a
  host could not tell a rejected commit that preserved usable history from one
  that did not. It now carries the typed `CompactionPreservedHistoryFit` and,
  where meerkat supplies it, `attempted_entries` - the count of real work at risk
  of being discarded.
- `ErrorEvent.is_resolution` (Python) / `isResolutionErrorEvent()` plus
  `RESOLUTION_ERROR_CATEGORIES` (TypeScript) distinguish the one category that
  reports a failure ENDING (`actor_loop_recovered`) from the rest. Mirrors the
  Rust runtime's `log_error_event`, which logs that variant at INFO rather than
  ERROR. Without it a host wired to a pager raises a second incident for a
  stall that already resolved.

- `meerkat-mobkit/tests/sdk_error_category_parity.rs` fails the build when the
  SDK `ErrorCategory` sets drift from the Rust `ErrorEvent` enum, naming the
  missing/extra tags and the file to edit. The authoritative Rust set is read
  back out of serde itself, so a per-variant `#[serde(rename = "...")]` cannot
  slip past it.

- `meerkat-mobkit/tests/sdk_enum_mirror_parity.rs` extends that gate to the two
  SDK mirrors of MEERKAT-owned enums, `MobRunStatus` and `MobMemberStatus`.
  These are the more fragile case despite being the smaller one: drift arrives
  with an upstream version bump and no mobkit commit involved, so it appears at
  the repin and is invisible in a review of the repin diff. Three declaration
  sites per vocabulary, not two - `parseMobRunStatus` carries a runtime
  allowlist that is a third mirror, and it is the one that decides what a host
  actually observes.

### Changed

- **Meerkat pins move to `=0.8.24`** (24 exact pins across `meerkat-mobkit` and
  `mobkit-store-conformance`).
- **BREAKING for anyone reading job health: `delivery_backlog` is gone, replaced
  by two fields that are not the same number.** Both gateway surfaces -
  `jobs/health` (typed) and the `healthz` projection (JSON) - now publish
  `pending_outbox_jobs` and `runtime_inbox_backlog` separately, plus `coverage`.
  This is a correctness fix, not a rename: 0.8.23 published the SUM of the two
  under one name, and they describe different wedges. `pending_outbox_jobs` is a
  job whose delivery was never handed to a runtime; `runtime_inbox_backlog` is a
  delivery a runtime accepted and never drained. Their sum is a number that means
  neither, and an operator reading it could not tell which failure they had - or
  which side to go look at. Alerts keying on `delivery_backlog` must be repointed;
  they will not silently keep working, which is deliberate.
  **Note the differing scopes, because they are not both realm numbers.**
  `pending_outbox_jobs` is realm-scoped. `runtime_inbox_backlog` is a HOST-STORE
  total and cannot be narrowed to a realm: the durable runtime store is one file
  per realm root and serves every session the host built, including members built
  under `mob.<mob_id>`, and runtime ids carry no realm. On a host serving more
  than one logical realm this number is therefore host-wide by construction.
  Over-inclusion is the safe direction - every counted row is a real undrained
  delivery on this host - whereas the realm-derived sum it replaces silently
  missed any runtime whose job had aged out, publishing a false zero exactly
  when a backlog was the thing being asked about.
- **`status` on both job-health surfaces gains a third value, `unreadable`.**
  Meerkat's census answers `Unreadable` when its scan window did not reach every
  row, and it is NOT a rung between `ok` and `degraded`: it says the census
  established nothing, so `stale_leases = 0` means "none seen", not "none exist".
  Mobkit now carries that rung through instead of collapsing it - folding it into
  `degraded` would assert a fault nobody observed, and folding it into `ok` would
  publish a green that means the instrument did not answer.
  **Repoint alerts from `status == "degraded"` to `status != "ok"`**, or an
  unreadable census will read as healthy.
  Honest limitation: mobkit cannot currently EMIT `unreadable`, because both of
  its census calls request an unbounded window and truncation only occurs when a
  bounded one fills. It is carried so that a future bounded window cannot
  silently collapse into a false rung.
- **`cross_mob/send` errors no longer prefix a verdict.** The message was
  `"cross_mob/send failed: {err}"`, which renders an ambiguous delivery as
  `"failed: ... is ambiguous"` - a contradiction whose leading verb wins for
  every reader, including the model-mediated ones that decide retries by reading
  prose. It is now `"cross_mob/send: {err}"`: context composes, verdicts do not.
- Test-suite timing hygiene: assertions that bounded a STRUCTURAL property with
  a wall clock (waiting for a spawn, a settle, a drain) now poll for the
  condition under a generous liveness backstop instead of asserting a duration
  the machine's load decides. Genuinely duration-shaped assertions - the ones
  that verify nothing happened inside a window - were deliberately left alone,
  since load only makes those safer. No production behaviour changes.
- The same sweep, extended to the JS suites and to one clock bug the first pass
  missed. The `flow-editor` browser smoke carried 31 unnamed 30s ceilings, every
  one guarding a structural question ("does this element ever appear"); they are
  now a named `STRUCTURAL_TIMEOUT_MS`. And
  `compact_member_forces_one_compaction_and_restores_the_profile` read the
  durable session surface on the identity's completion cursor, which leads it -
  the same clock split already documented in that file and already fixed once in
  its sibling. Its baseline is now settled and the growth polled, because a
  baseline snapshotted mid-write drifts upward on its own and would satisfy
  "did it grow" without the append it names. Both were found the same way: a
  diff that could not possibly have caused the failure it was blamed for.

### Known limitations

- **`MobRunStatus` coerces an unrecognized status to `pending` in both SDKs**
  (Python `MobRunStatus.parse`, TypeScript `parseMobRunStatus`). A status this
  release does not know is therefore reported to a host as NOT STARTED rather
  than as unknown - a wrong and actionable value. The new parity gate makes the
  drift that would trigger this a build failure, so the fallback cannot be
  reached silently by an upstream bump; changing the fallback itself is an SDK
  contract change and is deferred rather than made mid-release.
- **The console send path still flattens an ambiguous delivery into a failure.**
  The SSE surface and `cross_mob/send` were fixed this release; the console was
  not, and the reason is that its fix is not local. `IdentityRuntimeError`
  stringifies into `detail: String` at all four of its delivery phases, so the
  typed `AmbiguousDelivery` is already gone before `ConsoleSendError` sees it -
  it arrives as `Dispatch(String)` and is then reported as `"console send
  failed"` with a 500. Preserving it means changing that phase vocabulary, which
  is a multi-layer change to a hot path with no compile pressure forcing it, and
  release eve is the wrong time. Deferred to 0.8.19, named here rather than left
  for an operator to discover: **on the console surface specifically, a "console
  send failed" does not rule out that the message was delivered.**

## [0.8.17] - 2026-08-16

Paired release on meerkat v0.8.23.

**Mixed-version rollout warning (inherited from meerkat 0.8.23).**
`ToolAccessPolicy::ReadOnly` is serde-closed and persisted in durable session
metadata, so the hazard triggers on first USE, not on upgrade: once any session
carries a ReadOnly policy, pre-0.8.23 binaries cannot decode it. Roll the fleet
forward one-way BEFORE enabling read-only sessions.

### Changed
- Meerkat pins `=0.8.23`, which fixes the compaction-epoch seam defect: a
  seam-routed member with durable semantic memory could never persist a
  compaction, warn-only, retriggering every turn. Field evidence across the
  pin boundary: 36 attempts / 36 failures on 0.8.22, 2 attempts / 2 successes
  on 0.8.23.
- **Fail-closed supervisor routes** - no silent in-proc displacement anywhere.
  Same-id succession now requires the predecessor's terminal shutdown, which
  correctly releases the route (it previously leaked on every same-process clean
  restart). Two further shutdown-ordering bugs fixed: identities no longer park
  Broken during teardown, and a failed pre-spawn hook no longer leaks the route.
  **Adopter note:** a test suite that constructs multiple runtimes in one
  process under a shared participant name now fails closed with a message naming
  a public key. That reads as a crypto problem but is a test-isolation problem -
  before 0.8.17 each registration silently displaced the previous one, so such
  suites passed by accident. Use unique per-test mob ids, or have the incumbent
  stand down via terminal shutdown where succession is the intent.

### Added
- `ErrorEvent::CompactionPersistenceRejected` fires through the error hook on
  every rejected compaction commit (previously warn-only - fleets discovered
  wedged members by silence).
- `ErrorEvent::ActorLoopStalled` from a periodic heartbeat round trip, the only
  observation that can honestly claim loop scope. Knobs
  `MOBKIT_ACTOR_LOOP_PROBE_INTERVAL_SECS` / `_BUDGET_SECS`.
- Gateway operator verbs `mobkit/compact_member` (forced compaction via a
  profile floor plus rebuild) and `mobkit/bound_member_transcript` (pair-safe
  keep-last-N; typed Busy refusal on in-flight turns - quiesce first).
- `mobkit_repair` tool-result bounding: `--truncate-tool-results <max_bytes>`
  and `--drop-tool-results-older-than <N>`, in place, pair-preserving, and
  idempotent via the marker prefix `[mobkit-repair:tool-result-`.

### Fixed
- **P0**: `storage-migrate --apply` stamped the irreversible continuity ledger
  while converting nothing. Conversion now precedes the stamp, and blobs that
  changed after an earlier conversion are reconverted before it.
- `ActorAdmissionTimeout` stopped asserting loop scope it cannot observe.

## [0.8.16] - 2026-08-11

Paired release on meerkat v0.8.22. Delivers the owner-ratified 26-item
program; every item's disposition is stated below, including the items
deliberately refused and the ones shipped as documented partials.

### Removed

- **Item 14 phase B** - removed the second scheduling authority as one breaking
  contract change. The stateless `mobkit/scheduling/evaluate` and
  `mobkit/scheduling/dispatch` methods now return `-32601`,
  `runtime_options.scheduling_files` is rejected, and the Rust/Python/
  TypeScript static-evaluation surfaces are gone. `MOBKIT_CONTRACT_VERSION`
  is now `0.5.0`. Durable Meerkat `ScheduleService` storage, schedule tools,
  firing hosts, targets, `host_runnables`, and `callback/schedule_fire` remain
  the single scheduling path.

### Changed
- Meerkat pins `=0.8.22`. Ported nine adaptation seams plus the normalized
  provider-accounting contract: a stream that carries no `UsageUpdate` now
  fails its turn closed, so every LLM double must emit accounting.
- **Item 15** - console identity self-heal is demoted. The four
  identity-authority mutation sites in `inspect_identity` return typed errors
  instead of healing. A READ-ONLY console session (not merely a VIEW-scoped
  principal) could previously reach Suspend -> acquire_leases -> fence advance
  -> Broken; healing keeps its four write-path callers.
- **Item 13a** - the schedule executor lease is adopted as OBSERVATION ONLY.
  0.8.22's lease fences *claiming*, not firing-intent writes, so
  `FiringHostGatedScheduleTools` is RETAINED. Operator text now states that
  `Held` is a BOUNDED FACT and not proof of liveness: a host that died inside
  its lease window still reads `Held` for up to the lease duration.
- **Items 9, 10, 16** - dispatcher, gateway-composition and identity-resolution
  unification are partial BY DESIGN; the ratified text forbids a big-bang
  rewrite, so only domains that could be unified completely were, and the
  remainder is handed forward as a survey and a delta list.
- **Item 21** - scoped to the dead `Resumed.snapshot` field. `RestoreOutcome`
  has four live production consumers, so the item's title overreached.

### Added
- **Item 2** - remote runtime-host lifecycle: durable pairing, endpoint
  identity, capability discovery, health, reconnect and placement.
- **Item 3** - real fork through `UnifiedRuntime`, wrapping meerkat's durable
  `fork_member` and preserving running-source rejection.
- **Item 5** - per-slot WorkGraph store injection. `workgraph_store()` on
  `UnifiedRuntimeBuilder`, threaded through all THREE bootstrap paths;
  wiring only the persistent paths would have left a caller who sets a store
  without a state path silently on the memory fallback.
- **Item 6** - host-granted WorkGraph namespaces, scoped to meerkat's
  canonical mob realm.
- **Item 7** - WorkGraph transition facts bridged into event and SSE surfaces
  as lossy wake accelerants.
- **Item 11** - embodiment and restore doors merged into one per-identity
  park-and-continue path.
- **Item 12** - the declared-versus-resolved capability invariant.
- **Item 19** - thin transcript-edit exposure.
- **Item 23** - the LLM selector is re-homed and then deleted, in that order:
  `AnnotatedRecord` serves the lexical recall path, so deleting first would
  have broken default recall.
- **Item 25** - BREAKING Rust API removal: `MarkdownAgentMemoryStore` and
  `UnifiedRuntimeBuilder::persistent_agent_memory(...)` are deleted. Markdown
  is no longer a live agent-memory backend; its parser and file layout remain
  crate-private solely for the one-shot SQLite import, which preserves ids,
  tags and timestamps before renaming each source to `.md.imported`.
- **Item 26** - public-surface cleanup for deleted and parked capabilities.
- **Item 20** - Hygienist is PARKED, not deleted, pending real fork plus
  bounded projection proven on OpenAI and Anthropic.
- Memory output budgets (`max_output_tokens` on Steward and Distiller) are
  reachable from the Python and TypeScript SDKs and from the gateway
  allowlists. Hygienist is intentionally excluded from that public surface:
  absent/false/disabled config remains migration-compatible, while any
  activation-shaped value is refused with invalid params.

### Fixed
- **SIGTERM is handled by both gateway binaries.** They waited on
  `ctrl_c()`, which is SIGINT only, so container runtimes never ran the
  graceful path - and since 0.8.22 an ungraceful stop leaves the schedule
  executor lease held for up to 60s, silently suppressing schedule firing.
  The 2-minute claim watchdog is too coarse to observe it.
- The scope-pinned WorkGraph wrapper forwards the inner catalog surface.
  Leaving `tool_catalog_capabilities()`/`tool_catalog()` on their trait
  defaults silently downgraded the composed dispatcher to a non-exact, empty
  catalog - a different surface than the one being wrapped.
- Test integrity: MobKit's LLM doubles conform to the 0.8.22 accounting
  contract; assertions select their request by CONTENT rather than by
  position; turn readiness is a terminal event rather than a timer. A
  non-conformant double had been failing every turn in one file since 0.8.22
  while three tests passed anyway, because they only asserted that a request
  was ISSUED.

### Refused, with primary-source evidence
- **Item 22** (remove single-shot gateway mode) - REFUSED. Its gating
  condition was "if a downstream census confirms no use", and it does not
  clear: the TypeScript SDK publicly exports the one-shot transports and
  clients, and they spawn this binary with no arguments.
- **Item 24** (remove operator-scope machinery) - NO CHANGE. The premise
  ("zero activators") is false: `rpc_gateway` installs
  `ConsolePrincipalOperatorResolver`. `TrustTier::Operator` and re-derivation
  metadata carry real policy meaning and are retained.
- **Item 8** (delete `workgraph_admission`) - REFUSED pending the
  store-owned refusal contract actually being active.

### Not in this release
- **Item 4** (bounded helper-result path) and **item 27** (shell Allow-8
  deletion and heal) are not started.
- **Item 17** (recall and projection routing) is externally gated on field
  confirmation of typed recall without fallback.

## [0.8.15] - 2026-08-08

Paired release on meerkat v0.8.21.

### Changed
- Adapted to the meerkat 0.8.21 resume-verdict contract: session-service wrappers delegate `observe_session_resume_authority`; the operational resume entry is the upstream verdict composer, running through each wrapper's own overlaid load.
- `cross_head_canonical_authority` (required upstream format-door crossing): the continuity adapter routes to the owning store (superseded sessions present as absent), and the mobkit continuity trait carries a bridged default performing real reverification; absent/blob-canonical sessions return `NotApplicable`, preserving the legacy lane.
- Schedule claim results consume the upstream `TransitionCommit` carrier.
- Meerkat pins `=0.8.21`.

### Fixed
- Firing-intent schedule writes (`create`/`update`/`resume`) refuse typed while a gateway-owned store has no firing host bound (Bug C class); caller-injected library stores are never gated.
- External-tool composition warns per shadowed pre-installed tool (the scoped form of the recorder clobber).
- mobkit-repair prints a load-bearing post-apply reminder for the runtime scratch-store drop and ships `bytes_semantics` in its report (head-strand bytes, not disk).

### Added
- Regressions pinning the resume-tooling defect (activation-41) on both halves: a created member resolves declared categories, and a category declared AFTER creation resolves on resume (the meerkat 0.8.21 unified merge).
- Binary-level regression pinning same-identity typed agent-memory recall while the member's `callback/build_agent` is parked.
- Python SDK: typed `MobKitRuntime.list_identities()` returning `ConsoleIdentityRecord` (the supported identity -> session/profile map).
- `docs/proposals/simplification-audit-mobkit.md`: capability audit under the owner's simplification directive; seeds the 0.8.16 program.


## [0.8.13] - 2026-08-06

Paired release on meerkat 0.8.20 (the direct pairing: 0.8.19's
resume-authoring fix and 0.8.20's singular input-lane authority ship
together under one adoption; no intermediate 0.8.19 artifact exists).
Field-driven: every fix below was drilled against HomeCore's byte-exact
production corpus or OB3's 69-board rig, and the exact shipping pair
(this tree + the 0.8.20 release tree) passed the 17-member restore gate,
the three-boot accretion drill, and a real end-to-end member
resurrection before tagging.

### Changed

- **Paired on meerkat 0.8.20.** All meerkat crates repin to the published
  0.8.20 line (both workspace pin sites). The pairing floor is MANDATORY:
  cold-mint graph hydration requires preserving_live audited-history
  acceptance (0.8.17+), the first-wake steer regression requires the
  queue-behind-kickoff fix (0.8.18+), and resumed builds author no prompt
  rows under 0.8.19+'s resume contract - all guarded live by this
  branch's regression suite on the exact pairing.

### Fixed

- **Cold mint hydrates the rewrite graph** (HomeCore window-4 fleet
  blocker): `mint_runtime_authority_from_durable` seeded the runtime
  WholeBlob from the SLIM durable materialization, so on a REWRITTEN head
  every boundary projection composed rewrite-shaped state without compact
  graph authority and the store refused fail-closed ("rewritten session
  has no validated compact graph authority") - a sanctioned runtime-store
  reset then wedged every rewritten member in a refuse-retry loop
  (0 active / 17 broken). The mint now hydrates the graph from the
  store's own adopted rewrite records before sealing the seed bytes;
  legacy/import shapes whose records fail reconstruction pre-adoption
  seed slim as before, loudly. Drill-verified on the byte-exact field
  corpus: 17/17 mints at exact durable counts, zero refusals, echo turns
  green (parent-1: 4s/turn on the deduped head, down from 29s).
- **Broken registrations name their typed reason at warn**: both
  orchestrator Broken registration sites carried the typed
  `ContinuityFailure { kind, detail }` only into the roster outcome -
  never to the log stream - so a warn-baseline gateway saw 17 Broken
  identities with zero log lines. Each Broken registration now emits one
  warn line with identity + kind + detail.
- **App-supplied delivery correlations are canonicalized at the dispatch
  door** (HomeCore 2h outage on 0.8.12 + meerkat 0.8.16): non-UUID app
  correlation strings now map deterministically to UUIDv5 under the
  mobkit delivery namespace instead of drawing typed refusals from the
  fail-closed delivery-identity matrix; the schedule lane stays strict.

### Added

- **Console WorkGraph graph view**: a Tree|Graph toggle in the workgraph
  panel rendering a layered DAG (hand-rolled SVG, zero new dependencies)
  with status-colored nodes, arrowed parent edges, dashed labeled blocks
  edges, wheel-zoom/drag-pan/fit, click-to-inspect, and a 200-node cap
  with honest "+N more" overflow on both views. Playwright e2e
  (`e2e:workgraph`) boots a real console against a seeded fixture and is
  wired into CI.
- **Operator surgery helper** `examples/dedup_system_rows.rs`: drops
  replayed duplicate System rows from a live session's durable transcript
  through the typed rewrite door - groups by (content, identity), keeps
  the first occurrence per group, hydrates the rewrite graph from the
  store's own records, one full-range rewrite commit + authoritative
  projection. Field-proven in HomeCore window 4 (17/17 applies, parent-1
  36 -> 4 System rows, heads 80-460KB from 82MB-class documents).
- **Load representation traces**: `load_persisted_session` and
  `load_previous_session_for_save` emit representation+count debug lines
  so a mint-vs-save-guard representation split is attributable from a
  debug log instead of a store dump.

### Tests

- `reset_after_operator_rewrite_cold_mints_with_graph_authority`:
  window-5 mirror (operator dedup rewrite -> sanctioned reset -> cold
  boot -> real turn); red with the byte-exact field refusal pre-fix.
- `reset_after_repeated_customizer_boots_preserves_exact_counts_and_projects`:
  multi-boot count faithfulness with 12 seeded field-shape duplicate
  System rows, exact counts asserted at every hop from durable load to
  first projection.

## [0.8.12] - 2026-08-05

### Changed

- **Paired on meerkat 0.8.16.** All meerkat crates repin to the published
  0.8.16 registry release. What the pair carries for MobKit deployments:
  - The fan-in admission stall is fixed upstream: durable input admission
    returns without awaiting any ephemeral actor-boundary handshake, so a
    wedged live-boundary preparation can no longer silently starve every
    queued peer delivery behind it (OB3 field class: 30-of-69 completion
    pings, then an idle member with queued inputs and no wake).
  - WorkGraph/Mob Flow interoperation (meerkat PR #941): flow executions
    bind WorkGraph work items (`MobRun.flow_definition_digest`,
    `mob/flow_status.execution_binding`, `Realizing`/`ExistingRealizing`
    realization states); MobKit's workgraph tool surface carries the new
    optional typed execution provenance on `WorkEvidenceRef`
    (`execution_binding_id`; MobKit's attention-keyed provenance paths and
    the RPC wire surface mint no execution binding, explicit `None`).

### Fixed

- **Converged rows no longer WARN from the tear-repair diagnostic.**
  Repair passes after a heal match no admission by design (nothing to
  repair), and the "no repair admission holds" WARN read as a failure in
  HomeCore's v0.8.11 production validation. Content equality with the
  committed authority now logs debug and returns.


## [0.8.11] - 2026-08-04

### Fixed

- **Internal deliveries dedup across crash redelivery, and the dispatch
  door runs full delivery preparation** (meerkat 0.8.15 pair; held
  uncommitted until the repin). Two halves of one change set:
  - Delivery-identity threading: both internal work doors - the identity
    bridge's `InternalBridgeWork` submit and the schedule host's direct
    fallback - now submit through meerkat's deduplicating
    `submit_work_with_mode_and_delivery_identity` whenever a delivery
    identity (idempotency key + occurrence-UUID correlation) is present.
    meerkat derives the WorkRef from mob + member identity + idempotency
    key, stable across lease-expiry reclaim, so a crash redelivery of the
    same occurrence resolves to the SAME work instead of a duplicate turn.
    The scheduler sink now threads BOTH identity halves through
    `DispatchInput` into runtime admission (the 0.8.12-era "internal lane
    closed upstream" note is retired with the mechanism it documented).
    Admission is fail closed and typed, validated BEFORE the bridge:
    (None, None) is an ordinary delivery; a full pair validates upstream
    (canonical non-nil UUID correlation) and carries, with the correlation
    id also riding as the interaction id; EVERY other combination -
    half-pair, invalid UUID - raises `InvalidDeliveryIdentity` and NO
    delivery occurs. `SessionBridge` deliveries were rebuilt around one
    typed request: the REQUIRED `deliver_admitted(runtime_id,
    BridgeDelivery)` method that every implementation must service (the
    convenience `deliver*` methods are provided forwarders that build the
    request), so no implementation - production or test double - can
    silently drop a delivery identity; a bridge that cannot honor one
    refuses typed.
  - Shared delivery preparation: `dispatch_with_expected_member_alias`
    previously delivered RAW - internal dispatches (schedules foremost)
    skipped inbound defanging, taint session attribution, and ambient
    memory injection for the member's whole lifetime (HomeCore: zero
    surface=Turn injection-ledger rows ever). Both member doors now run
    the ONE preparation helper (note_current_session + generation bind,
    defang, inject_for_turn), and dispatch populates the admitted
    delivery request with the injected recall as its own typed body.
    `DispatchOrigin` is untouched; Steer keeps its deliberate bypass on
    the send door.

- **Steward dream honesty: turn-only usage evidence, quarantine window
  pin-down, durable run rows** (HomeCore dream rehearsal). Three findings,
  one root shape:
  - Usage-audit data boundary: the audit now judges only `surface=Turn`
    injection evidence - build-surface rows are spawn hydration
    bookkeeping, not proof a record earned its context slot. A store with
    zero Turn rows for the scope SKIPS the audit and queues NOTHING
    (HomeCore: 53/53 dead-weight noise verdicts minted from hydration
    counts on a store where turn evidence could not yet accrue).
  - Legacy quarantine review: proven from code that no
    "new-since-last-dream" window exists - every dream loads the full
    quarantine queue (capped per dream) regardless of record age; the
    rehearsal's unverdicted legacy records were starved by the pipeline
    aborting in the consolidate phase AFTER the audit sheet persisted.
    A regression pins the contract: a quarantined record older than the
    newest recorded dream still enters signals and gets its verdict.
  - Dream-run bookkeeping: the `dream_runs` row is now written at run
    START (in-flight), replaced at the tail with final numbers, and
    replaced with an honest failure row (committed-op count + failed
    phase) when the pipeline aborts - verdict rows can no longer
    reference run ids that resolve nowhere, and the console dream-runs
    panel sees failed runs instead of nothing.

- **WholeBlob-to-durable projection tear healed: missing rewrite commits
  replay through the typed rewrite door** (HomeCore parent-1 production
  park). The runtime-store facade projected committed WholeBlob
  boundaries into the durable session store with a plain authoritative
  projection, which cannot INSTALL a new rewrite generation - a
  wedged-turn retire committing a rewrite-advanced boundary left the
  durable row torn (graph one generation ahead of its head), and
  meerkat's rewrite-save invariant then refused every subsequent resume.
  The projection now runs per-runtime single-flight, loads the exact
  committed successor and the durable predecessor, and when the
  successor's PROVED graph extends durable state it installs each
  missing rewrite commit through `SessionStore::save_transcript_rewrite`
  (exact prefix session projected at that commit) before the ordinary
  trailing-append/envelope projection. Every step is monotonic and
  validated against the head the previous step installed, so a partial
  failure re-converges on the exact retry; no branch overwrites durable
  state the successor graph does not prove. The freshness probe gained
  the matching half: durable-behind-committed is now distinguished from
  fresh and runs the same reconciliation on a PLAIN RESUME (no new
  committing verb required), including under a lifecycle terminal -
  repairing the durable projection of already-committed authority mints
  no runtime life; the terminal gate stays on the reseed direction where
  resurrection risk actually lives. Field-recovery property:
  parent-1-class tears self-heal on the first post-upgrade resume - the
  committed runtime authority still holds the full graph, and the
  resume-path freshness pass replays the missing commit into the durable
  row; no hand-repair of wedged stores. The append-before-compact shape
  (a committed rewrite whose parent revision is a strict APPEND-extension
  of the durable head) is covered too: the reconciler first installs the
  exact proof-carrying parent via meerkat 0.8.15's
  `Session::with_validated_transcript_rewrite_parent_projection`, then
  replays the rewrite commit against its exact parent.
  Real-bytes corpus hardening (frozen parent-1 backup, five field
  iterations): the repair works on a PARKED member whose session is
  explicitly unregistered - the projection doors admit the
  durable-observed non-superseded head as a parked repair, write
  authority hydrates from the durable continuity record via the new
  fail-closed `ContinuityStore::resolve_record_by_session` (identity,
  generation, fencing token, fence-current checkpoint re-seed), and
  archives, creation windows, removed documents, and suspended sessions
  keep their refusals. Two proof-carrying admissions extend the chain
  walk for tear shapes it cannot prove, both by exact content digest:
  the durable row extending PAST the sealed parent (its unacknowledged
  suffix is REBASED over the compacted head, preserved, with the
  committed snapshot converged to the rebased state in the same pass)
  and the true parent-1 shape - the durable row as a strict PREFIX of
  the sealed parent (a failed projection; admission authored by
  HomeCore and validated on the live corpus, GATE_PASS 17/17). All
  admission verdicts log unconditionally with both digests; a durable
  row proving neither shape is a foreign lineage and keeps the typed
  refusal, byte-untouched.

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
