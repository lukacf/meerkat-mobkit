# Meerkat upstream asks — work order

> **Handed to the meerkat coding agent by Luka (2026-07-02).** Eight asks for
> the meerkat repo, derived from the MobKit memory initiative
> ([`agent-memory-architecture.md`](agent-memory-architecture.md) §13) and
> refined by what its implementation actually found. **None of these block
> MobKit** — every ask has a shipped MobKit interim behavior, so land them in
> whatever order fits meerkat's own constraints. Each ask states the problem,
> code-verified evidence, a proposed shape (a starting point, not a contract —
> deviate where meerkat's internals argue otherwise, but preserve the stated
> *property*), and what MobKit does today / will do when it lands.
>
> **Suggested sequencing by impact:**
> 1. **Ask 1 + Ask 3** (same seam, one change set) — unlocks MobKit flipping
>    ambient per-turn memory injection on by default, echo-safe by
>    construction, and closes the injection-channel-authenticity gap.
> 2. **Ask 2 (+ Ask 8, natural pair)** — stops the per-build re-embed tax that
>    grows with every respawn/reset in a realm; operational debt accruing
>    daily.
> 3. **Ask 5** — closes the remaining persistent-prompt-injection gaps
>    (cross-process taint, send-time semantics, dispatch-time race).
> 4. **Ask 6** — deepest change, biggest architectural unlock (contained
>    fork-launched extraction with prompt-cache economics).
> 5. **Ask 4** (incl. the head-revision-read refinement) and **Ask 7** —
>    quality-of-life; no urgency.

Evidence citations were code-verified against meerkat 0.7.9 (the version
MobKit pins) and the mobkit `feat/agent-memory-system` branch; line numbers
may have drifted. The memory-survey archive
([`evidence/memory-survey/followups.md`](evidence/memory-survey/followups.md))
holds the original deep-dive evidence.

---

## Ask 1 — Typed injected-context message class, excluded from compaction indexing

**Title:** Add a typed injected-context message class that
`indexable_content()` excludes from compaction indexing

**Problem statement.** Hosts that inject ambient context into a turn (MobKit
injects agent-memory recall blocks) have no message class to deliver it in.
`ContentInput` has no role field, so the injection is fused into the user's
own message text and persists verbatim in the transcript. At compaction,
`indexable_content()` classifies `Message::User` as
`Indexable(u.text_content())` unconditionally, so every discarded injected
turn deposits a full copy of the injection block (~18.5 KB at MobKit
defaults) into session-scoped semantic memory. The result is linear,
unbounded-within-a-session pollution of `memory_search` (MobKit defect D1):
duplicate near-identical entries progressively dominate results for any query
overlapping the injected records' vocabulary.

**Evidence.**
- Per-message indexing seam ignores content provenance:
  `meerkat-core/src/types.rs:1201-1203` (`Message::User` →
  `Indexable(u.text_content())`, unconditional).
- `MemoryIndexExclusion` (`meerkat-core/src/types.rs:1237-1247`) is the right
  typed shape but has no variant for injected/synthetic user content — while
  `Message::ToolResults` is `Excluded(MemoryIndexExclusion::ToolResults)`
  (`types.rs:1220-1222`), which is exactly why recalled text never re-indexes
  and the loop stays linear rather than geometric.
- Indexing of every discarded message on every successful compaction:
  `meerkat-core/src/agent/state.rs:1265` (call site), `state.rs:1362-1431`
  (session-scoped `MemoryIndexScope`).
- MobKit side of the fusion: `meerkat-mobkit/src/identity_first/runtime.rs:2425-2448`
  (every non-Steer send passes through `inject_for_turn`),
  `identity_first/agent_memory.rs:854-871` (`prepend_memory_injection`
  physically concatenates: `format!("{injection}\n\nCurrent user
  message:\n{text}")`).
- No filter exists for the injection marker: `grep
  mobkit_memory_observation` over meerkat-core and meerkat-memory returns
  zero hits (survey, `followups.md` §1).

**Proposed shape.** A first-class injected-context delivery class — either a
new message variant or a typed transcript role that `indexable_content()`
actually consults — with a corresponding
`MemoryIndexExclusion::InjectedContext`, plus a delivery surface that lets a
host attach injected-context messages alongside (not inside) the user's
content on the submit-work path.

**Coupling note (important).** This ask alone is not sufficient, and it is
explicitly coupled to a MobKit-side restructuring
(`agent-memory-architecture.md` §9.1): because MobKit today *fuses* the
injection into the user's message text, a role class applied to the fused
message would either exclude nothing or exclude the operator's real words
along with the injection. MobKit must stop fusing and deliver injected bodies
as a **separate typed message** — the ask and the restructuring land
together.

**Second rationale: channel authenticity.** The injection envelope is plain
text an attacker can reproduce — a peer message or echoed web content can
forge "remembered operator preferences" verbatim, inheriting whatever
authority the model grants real memory. MobKit's interim mitigations (inbound
defanging of reserved envelope markers, a per-session nonce) are bar-raising,
not authoritative. A typed message class gives injected memory an
authenticated channel instead of a text pattern (§9.1 anti-spoofing).

**MobKit interim behavior.** Per-turn ambient push defaults **off**
(`agent_memory.per_turn_injection = "off" | "budgeted"`); the opt-in
`budgeted` mode carries the full budget ladder and dedup. All *default*
injection surfaces are already-excluded message classes (system prompt, tool
results). The per-turn default flips to on only when both halves — this ask
and MobKit's separate-typed-message delivery — have landed.

---

## Ask 2 — `MemoryStore` lifecycle: delete/GC and lazy per-scope index loading

**Title:** `MemoryStore` needs delete/GC (per-scope drop) and lazy per-scope
index loading

**Problem statement.** Session semantic memory is append-only and scoped to
exactly one `SessionId`, but session ids rotate under host lifecycle
operations, permanently orphaning rows. And the cost of orphans is not just
disk: the store re-embeds *every* row in the shared DB on *every* agent
build in the realm, so orphan growth taxes all future builds with CPU, RAM,
and startup latency — monotonically, with no cleanup facility of any kind
(MobKit defect D3, storage half).

**Evidence.**
- The `MemoryStore` trait has exactly three methods — `index_scoped`,
  `index_scoped_batch`, `search`; no delete/prune/expire API exists:
  `meerkat-core/src/memory.rs:404-434`. Scope is strict equality on one
  `session_id` (`memory.rs:10-27`).
- `HnswMemoryStore` persists into one shared `<dir>/memory.sqlite3`
  (`meerkat-memory/src/hnsw.rs:281`); inserts are append-only
  (`hnsw.rs:419-608`); the only `DELETE FROM memory_*` statements are the
  rollback repair of a partially failed batch (`hnsw.rs:548-559`) — not a
  cleanup facility.
- `HnswMemoryStore::open` runs inside every `build_agent`
  (`meerkat/src/factory.rs:5419-5420`, wiring `factory.rs:5411-5472`), does
  an unfiltered `SELECT` over every row (`hnsw.rs:319-335`), re-embeds every
  text (`hnsw.rs:344`), and constructs an in-memory HNSW graph per session id
  found — including orphaned sessions nothing will ever search
  (`hnsw.rs:348-354`).
- Host lifecycle rotates session ids: MobKit's `mobkit/respawn` RPC is always
  a mob-level retire+spawn that mints a fresh `SessionId` and rebinds
  continuity (`meerkat-mobkit/src/rpc.rs:2937-2972`; meerkat-mob
  `actor.rs:14553` builds the replacement with no `resume_session`); `reset`
  mints a new id by design; resume has a fresh-spawn fallback; retire/delete
  leave all rows behind forever (survival matrix in `followups.md` §2).

**Proposed shape.** Two additions, independently useful:
1. **Delete/GC on the trait** — a per-scope drop (`drop_scope(session_id)`
   or equivalent) so a host that rotates or deletes a session can reclaim its
   rows; record-level delete is a bonus, per-scope is the need.
2. **Lazy per-scope index loading** — `open()` (or first search) loads and
   embeds only the requested scope's rows instead of rebuilding every
   session's graph in the realm on every agent build.

**MobKit interim behavior.** Orphan growth is documented as a known cost.
The *knowledge* half of D3 is handled MobKit-side regardless:
distill-before-rotation (initiative P2) runs the Distiller over the session
before respawn/reset rotates it, so what mattered survives in MobKit's own
identity-keyed store even though the meerkat rows are stranded.

---

## Ask 3 — Compaction-summary indexing exemption

**Title:** Discarded compaction summaries should be excluded from compaction
indexing

**Problem statement.** The compactor injects its summary as a `Message::User`
carrying `TranscriptUserRole::CompactionSummary`, and discards prior
summaries at the next compaction — but `indexable_content()` ignores
`transcript_role`, so a discarded summary is itself indexed into session
semantic memory. This is model-mediated content (an LLM distillation of the
window, potentially including injected-memory text) being mechanically
indexed as if it were first-hand user input — a secondary echo edge on top
of ask 1.

**Evidence.**
- Summary injected as user message: `meerkat-session/src/compactor.rs:194-197`
  (`Message::User(UserMessage::compaction_summary(...))`), role type at
  `meerkat-core/src/types.rs:2171-2178`.
- Prior summaries discarded at the next compaction:
  `compactor.rs:226-229`.
- `indexable_content()` never inspects `transcript_role`
  (`types.rs:1201-1203`); the only consumer of `is_compaction_summary` in the
  codebase is `session_store.rs:701` (survey, `followups.md` §1, point 5).

**Proposed shape.** The same typed-exclusion fix as ask 1: have
`indexable_content()` consult the transcript role and return
`Excluded(MemoryIndexExclusion::CompactionSummary)` for summaries. The seam
is one line today; the role already exists and is already stamped.

**MobKit interim behavior.** None required — the edge is meerkat-internal.
MobKit's echo-safe injection posture (ask 1 interim) shrinks what leaks into
summaries in the first place, and the residual pollution is documented as a
known defect rather than worked around.

---

## Ask 4 — LLM-curated compaction seam (host-supplied curator)

**Title:** Let a host-supplied curator produce the compaction summary

**Problem statement.** Summary generation is fixed inside the compactor; a
host cannot supply or amend the summary content. MobKit's Hygienist
(`agent-memory-architecture.md` §8.6) targets the incremental-session-notes
pattern proven out by Claude Code's SessionMemory/SM-compact: a
continuously-maintained notes artifact that gets spliced in at the compaction
boundary, making compaction itself **zero-LLM-cost** — no summary call at the
boundary at all, because the notes were maintained incrementally during the
session. Without a curator seam, a host can only run its curation as a
separate boundary-triggered pass alongside meerkat's own summary call, paying
twice.

**Evidence.**
- The seam today: `DefaultCompactor::rebuild_history` constructs the summary
  internally and injects it (`meerkat-session/src/compactor.rs:194-197`);
  there is no host hook.
- The prior art being adopted: Claude Code's SM-compact splices a maintained
  session-notes artifact as the post-compact summary
  (`claude-code/src/services/compact/compact.ts:614-624`,
  `sessionMemoryCompact.ts:479-480`; survey `followups.md` §1) — flag-gated
  in CC but the right shape (`agent-memory-architecture.md` §5, adopt list).
- The MobKit consumer: Hygienist incremental-notes target, §8.6 — "a
  continuously-maintained notes artifact spliced in at the compaction
  boundary via upstream ask 4's curator seam would make compaction itself
  zero-LLM-cost."

**Proposed shape.** A compactor extension point where the host registers a
curator: given the transcript window being compacted (and the prior summary),
it returns the summary content — with the default behavior unchanged when no
curator is registered. Must preserve meerkat's ordering invariant (indexing
gates the compaction commit — "never lose the only copy") and compose with
ask 3's exclusion so a curated summary is not itself re-indexed later.
Marked **later** in §13 — valuable, not urgent.

**MobKit interim behavior.** Meerkat's standard compaction stays as-is. The
Hygienist ships as a boundary-triggered curation pass using meerkat's
existing audited same-session transcript revisions
(`session/rewrite_transcript` / `session/transcript_revision` /
`session/restore_transcript_revision`), staged and validated like any other
memory mutation.

**P4 implementation refinement (found building the Hygienist against
meerkat 0.7.9).** The revision *apply* surface is reachable and used:
`PersistentSessionService` implements `SessionServiceTranscriptEditExt`
(`rewrite_session_transcript` / `restore_session_transcript_revision`) and
MobKit threads the concrete typed handle to the Hygienist at gateway
bootstrap (the erased `MobSessionService` does not carry the edit
extension). One adjacent read gap remains: **no `SessionService`-level API
exposes the current transcript head revision id.**
`Session::transcript_revision()` computes it, but only on an owned
`Session`; `read_transcript_revision` requires already knowing a revision
id. Two consequences, both currently absorbed rather than closed:
(a) rewrites cannot compare-and-swap — `expected_parent_revision` is sent
as `None`, and the service's internal mutation guard plus
`TranscriptEditRunningBehavior::Reject` (refuse mid-turn) are the only
concurrency protections; (b) §7.1's revision-pinned `EvidenceRef`s cannot
be captured at write time — Distiller/Recorder provenance keeps
`revision: None` ("head at capture time") until a head-revision read
exists. Nothing dangles meanwhile (revisions are additive and parent
bodies are retained), but pinned-revision provenance resolution stays
approximate. Small ask: expose `current_transcript_revision(id)` (and
ideally a revision list) on `SessionService` or the history extension.

---

## Ask 5 — Comms envelope taint metadata + dispatch-time taint visibility

**Title:** Taint metadata on comms envelopes; tool-source taint visibility at
dispatch time

**Problem statement.** Two related gaps for persistent-prompt-injection
defense:
1. **Cross-agent taint has nowhere to ride.** `Envelope` is
   `{id, from, to, kind, sig}` with no metadata field, so a sender cannot
   carry a content-taint flag to its peers. Intra-mob peer messages are
   trusted-or-dropped by construction, which means a prompt-injected agent A
   can launder payloads through peer B, whose own session ingested nothing
   untrusted — B's memory writes look clean
   (`agent-memory-architecture.md` §10.1).
2. **Taint is only visible asynchronously.** The host learns that a session
   ingested untrusted content by watching the observe stream, so a memory
   write in the same turn as the session's *first* untrusted ingestion can
   race the tracker and land unquarantined. The typed facts needed for
   synchronous classification already exist (`ToolDef.provenance` with
   `ToolSourceKind::Mcp` + server source id; provider-native
   `ServerToolKind::WebSearch`) — they are just not exposed to the host at
   tool-dispatch time.

**Evidence.**
- Envelope shape: `meerkat-comms/src/types.rs:86-97` — fields
  `id, from, to, kind, sig`; the signature is over canonical CBOR of
  `[id, from, to, kind]` with field order fixed per spec, so there is no
  slot for metadata today and adding one touches the signing contract.
- No turn-level or session-level content-taint fact exists anywhere in the
  platform; the only "untrusted" concepts are comms sender admission and
  mobpack deploy trust, which are different facts
  (`agent-memory-architecture.md` §10.1).
- The dispatch-time race and its consequence (Recorder classifying against
  the observe stream instead of synchronously): §10.1, "its one honest gap
  is a race: a write in the same turn as the session's *first* untrusted
  ingestion can beat the observe stream."

**Proposed shape.**
1. An optional, signed metadata field on `Envelope` (protocol-versioned,
   since the canonical signable bytes are spec-fixed) able to carry a
   sender-session taint flag or labels, so receivers can propagate taint
   without out-of-band joins.
2. A smaller, independent ask: expose tool provenance/taint classification at
   tool-dispatch time (hook or ordering guarantee), so a host's memory write
   path can classify synchronously instead of racing the observe stream.
   Marked **later** in §13.

**MobKit interim behavior.** P1 ships the coarse deterministic posture: a
content-trust configuration (web/fetch always untrusted; MCP servers
untrusted by default with an explicit allowlist) plus an observe-stream
tracker that marks the session tainted — session-sticky, cleared only at
fresh-context boundaries — with the first-ingestion race documented. P2
ships the comms taint join over what the observe surface actually carries
(implemented in `meerkat-mobkit/src/memory/taint.rs`,
`observe_inbound_peer_content`), and building it sharpened this ask:
meerkat 0.7.9 has **no typed inbound peer-message event** — a delivery
surfaces only as injected prompt text on the receiver's `RunStarted`
(`format_peer_message_projection`, meerkat-core `interaction.rs:126`), so
the join parses the sender's `MemberCommsName` out of that text and checks
the sender's tracked taint at delivery-observe time, not send time. Three
gaps only envelope-level flags can close: (a) send-time vs observe-time
taint state can differ across a sender rotation; (b) peer *requests* render
a raw cryptographic peer id, unmappable to an identity host-side; (c)
cross-process senders are not in the host tracker at all. Deployments that
cannot accept the P1 race set `memory.llm_writes = "quarantined"` (every
LLM-authored write quarantines until steward/operator review).

---

## Ask 6 — Capability-gated tool authorization for fork-launched members

**Title:** Call-level tool authorization policy on fork/spawn launch modes, so
a fork-launched member can be capability-contained without changing its tool
list

**Problem statement.** MobKit's memory Distiller wants to run as a fork of
the live session — `Session::fork`/`fork_at` are O(1) copy-on-write, and a
fork shares the parent's prompt-cache prefix, making extraction nearly free
in input tokens (the pattern Claude Code uses for exactly this workload). But
prompt-cache identity requires a byte-identical prefix *including the tool
list*, and the only fork path MobKit can reach —
`MemberLaunchMode::Fork` / `fork_helper` — spawns a **live member carrying
the parent's full tool surface**. An LLM stage whose entire job is reading
possibly-poisoned transcript evidence cannot be handed live tools: that is an
uncontained extractor. Containment must therefore live at the *authorization*
layer (every call gated to an allowlist, e.g. read-only + a memory-write
seam), not at tool-list subsetting (which breaks the cache) — and no such
layer exists on the fork/spawn path today. The same gap forced MobKit's
memory Steward to be a shell-driven pipeline of structured calls instead of
an agentic dream member.

**Evidence.**
- Fork launch surfaces: `meerkat-mob/src/launch.rs:23-57` (`fork_helper`),
  `meerkat-mob/src/runtime/handle.rs:5946` — a fork-launched member gets the
  parent's tool surface; there is no per-launch authorization hook.
- CoW fork primitive: `meerkat-core/src/session.rs:3644` (`fork_at`),
  `:3764` (`fork`).
- The prior-art constraint (tool list is part of the prompt-cache key, so
  containment must be call-level): Claude Code keeps the parent's exact tool
  list and gates via a `canUseTool` closure —
  `evidence/memory-survey/followups.md` and
  `agent-memory-architecture.md` §8.4.
- MobKit's decision record: `meerkat-mobkit/src/memory/distiller.rs` module
  docs ("full tool surface … §8.4's fork containment") — the fork seam is
  named and parked there.

**Proposed shape.** A per-launch tool-authorization policy on member
launch (at minimum the fork mode; ideally any spawn):
`ToolCallAuthorizer`-style trait (`fn authorize(&self, tool_name, args) ->
Allow | Deny(reason)`) accepted by the launch/spawn config and enforced in
the tool dispatch path *after* ToolDef listing (list unchanged → cache
prefix unchanged; execution gated). Deny should surface to the model as an
ordinary tool error. Composes with meerkat's fail-closed capability
philosophy; dogma-wise it is an authorization seam, not a new semantic
authority.

**MobKit interim behavior.** The Distiller runs as a detached bounded
one-shot over a transcript slice read from the session store (correct but
full-input-price; spend bounded by MobKit's background resource guards), and
the Steward is a deterministic multi-phase pipeline of structured LLM calls
(the model holds no tools at all). When this ask lands, MobKit switches the
interaction-triggered and pre-rotation distillation paths to session forks
and can consider an agentic steward gather phase; the seam is documented in
`distiller.rs` module docs.

---

## Ask 7 — Internal-runnable schedule targets in meerkat-schedule

**Title:** Let `meerkat-schedule` target a host-registered runnable, not only
mob members/sessions

**Problem statement.** Schedule target bindings only address sessions and mob
members. Host-internal periodic work — MobKit's memory steward is the
concrete case: a scheduled consolidation "dream" that must NOT be a mob
member (see Ask 6's containment problem) — cannot be expressed as a
schedule, so it runs as a guarded `tokio` interval loop outside the schedule
subsystem: invisible to schedule listing/inspection surfaces, no shared
cadence semantics, separate operational story.

**Evidence.**
- Target vocabulary: `meerkat-schedule` `TargetBinding::{Session, Mob}` —
  verified during MobKit P3 implementation; no host-runnable variant.
- The workaround and its deliberate forward-compatibility:
  `meerkat-mobkit/src/memory/steward.rs` (`spawn_dream_loop`) uses the
  schedule subsystem's own interval-marker grammar
  (`parse_interval_marker_ms`) for its cadence config, so the config
  migrates unchanged the day a real target type exists; module docs carry
  the TODO.

**Proposed shape.** A `TargetBinding::HostRunnable { name }` (or
callback-registration equivalent): the host registers named runnables with
the schedule service at startup; occurrences invoke them through the normal
occurrence lifecycle (so listing, history, and failure surfaces work
unchanged). No new machine semantics — the schedule/occurrence machines
already own the lifecycle; this adds a target kind whose "delivery" is a
host callback.

**MobKit interim behavior.** The steward's interval loop stays; cadence
config is already schedule-grammar-compatible. On landing, MobKit registers
the dream as a host runnable and deletes the loop.

---

## Ask 8 — `MemoryStore` enumeration API

**Title:** Add a scoped enumeration/read API to the `MemoryStore` trait
(pairs with Ask 2's delete/GC)

**Problem statement.** The `MemoryStore` trait exposes exactly
`index_scoped`, `index_scoped_batch`, and `search` — similarity search is
the *only* read surface. Hosts that need to read back what a scope contains
must approximate enumeration with a generous-limit search query. MobKit hits
this in the post-compaction harvest: after `CompactionCompleted`, the
Distiller reads the discarded message range host-side from meerkat's own
session-memory store, and "read the discard range" becomes "one
big-limit scoped `search` and hope the limit covered it." Ask 2's GC/delete
work needs the same primitive (you cannot audit or selectively reap what you
cannot list), as does any re-derivation/re-indexing tooling.

**Evidence.**
- Trait surface: `meerkat-core/src/memory.rs:404-434` — three methods, no
  list/enumerate/count.
- The approximation and its honesty caveat:
  `meerkat-mobkit/src/memory/distiller.rs` (`HnswDiscardSource`) — one
  scoped `MemoryStore::search` with a generous limit per harvest, caveat
  documented in code ("search is the only read surface … approximates
  enumeration").

**Proposed shape.** `async fn enumerate_scoped(&self, scope:
MemorySearchScope, page: PageRequest) -> Result<Page<MemoryEntry>, _>`
(paged, deterministic order, optional `source_range` overlap filter so a
caller can fetch exactly the entries covering a transcript range). Read-only
and additive — no change to indexing or search semantics; implementations
that cannot page can return one page.

**MobKit interim behavior.** The generous-limit search approximation, with
the limit sized to compaction discard batches. On landing, the harvest reads
the exact range and the approximation caveat comes out of `distiller.rs`.

---

# Batch 2 — handed to the meerkat coding agents by Luka (2026-07-04)

> Seven further asks, derived from (a) the two §10.1 security gaps the shipped
> taint firewall documents as upstream-gated, (b) the fork-Distiller seam that
> ask 6's landing did not fully unlock, and (c) production evidence from
> **ob3_validator** — the most general MobKit deployment (one mob; hundreds of
> years-lived durable identities in the personal-agents + domain-agents +
> task-agents pattern; multi-GB sessions; BigQuery-backed custom stores; heavy
> agent-spawned worker churn). OB3's own filed docs (in
> `ob3_validator/docs/`, commit `c36c105`) are the authoritative specs for
> asks 11–13 — work from them directly. None of these block MobKit; every ask
> states the interim behavior.
>
> **Suggested sequencing by impact:**
> 1. **Ask 11** (incremental session persistence) — the standing scale wall;
>    two production fleet wedges already attributed to it.
> 2. **Ask 12 + Ask 13** — production incident classes at fleet scale
>    (stranded singletons; hot-loops and failure cascades).
> 3. **Ask 9** — closes the last two documented holes in the memory taint
>    firewall.
> 4. **Ask 10** — unlocks the deferred fork-Distiller (prompt-cache economics
>    over multi-GB transcripts).
> 5. **Ask 14 + Ask 15** — quality-of-life; no urgency.
>
> Evidence citations verified against meerkat 0.7.15 and mobkit `main`
> (post-#218/#219); OB3 citations against `ob3_validator` HEAD 2026-07-04.

## Ask 9 — Dispatch-time content-taint visibility + `ToolDef` provenance

**Title:** Give hosts a synchronous taint signal at tool dispatch, and typed
tool provenance (completes Ask 5)

**Problem statement.** MobKit's content-taint tracker derives taint from the
asynchronous observe stream, which leaves two documented, deliberate gaps:
(1) the **first-ingestion race** — a memory write in the same turn as the
session's *first* untrusted ingestion can reach the store before the taint
observer processes the tool event; (2) **name-based MCP classification** —
tool events carry only the tool NAME, so MCP tools can only be attributed to
a server when their names are server-qualified (`mcp__<server>__<tool>`);
anything else needs manual `untrusted_tools` config.

**Evidence.** Both gaps are documented as upstream-gated in
`meerkat-mobkit/src/memory/taint.rs` module docs ("Honest gaps that remain
(upstream asks, §13)"): the race, and "no `ToolDef.provenance`". Verified
still absent in meerkat-core 0.7.15 (`types.rs` has no tool provenance
field; no dispatch-time host taint seam).

**Proposed shape.** (a) A synchronous seam at tool dispatch — either a
pre-result host hook or a dispatch-time event with a completion barrier —
such that the host can classify "untrusted content is entering this session"
*before* the tool result lands in context. (b) `ToolDef.provenance` (typed:
builtin / MCP server id / bundle / host) surfaced on dispatch events, so
classification stops guessing from name shapes.

**MobKit interim behavior.** Session-sticky async taint (conservative), plus
the `agent_memory.llm_writes = "quarantined"` posture knob for deployments
that cannot accept the race. On landing: dispatch-time classification closes
the race; name-shape probing and the `untrusted_tools` escape hatch shrink to
compat.

## Ask 10 — Fork tool authorization + fork-from-persisted (completes Ask 6)

**Title:** Call-level `tool_access_policy` on session fork, and forking a
persisted (non-live) session

**Problem statement.** Ask 6 landed fork-launched members, but the
fork-Distiller remains blocked on two specifics: `SessionForkAtRequest`
carries no `tool_access_policy` (fork authorization is build-time only), and
`fork_session_at` requires a LIVE parent session — while MobKit's
reset/resume-fallback/compaction distillations run AFTER teardown, reading
from the session store.

**Evidence.** meerkat-core 0.7.15 `service/mod.rs:1632-1636`
(`SessionForkAtRequest { message_index, running_behavior }` — no policy
field); the fork-harness seam documented in
`meerkat-mobkit/src/memory/distiller.rs` module docs ("TODO(§8.4 fork
harness)").

**Proposed shape.** Add `tool_access_policy: Option<ToolAccessPolicy>` to
the fork request family (fork-at / fork-replace), and a
fork-from-persisted path (parent loaded from the session store when not in
the live map) preserving the prompt-cache prefix.

**MobKit interim behavior.** The Distiller runs detached bounded extraction
over a bare LLM client (zero tools — so no containment gap, only foregone
prompt-cache economics). On landing, extraction moves to a fork sharing the
parent's cached prefix — material at OB3-scale multi-GB transcripts.

## Ask 11 — Incremental session persistence (`IncrementalSessionStore`)

**Title:** Adopt OB3's incremental session persistence spec — O(delta) saves,
compaction that shrinks the persisted head

**Problem statement.** `SessionStore::save()` persists the whole session
blob every turn: O(session) write amplification on every turn of every
member. Measured in production: a 9.2 MB session that was 95% retained
compaction revisions and 5% live messages; BigQuery's 10 MB DML cap turned
this into HTTP 413 → **two fleet-wide wedges (2026-06-12)**. Compaction
currently *grows* the persisted blob (revisions are retained), so the
mechanism meant to bound context growth unbounds storage growth.

**Evidence + spec.** `ob3_validator/docs/MEERKAT_SPEC_incremental_session_persistence.md`
(authoritative; includes the trait sketch:
`append_messages` / `commit_rewrite` / `save_head` / `load_head` /
`load_messages`). OB3's chunked-persistence mitigation:
`ob3_validator/src/meerkat/session.rs:39-73`.

**Proposed shape.** As the OB3 spec: an additive `IncrementalSessionStore`
trait with append/rewrite-commit semantics; whole-blob `SessionStore` remains
the compat surface. Compaction commits a *rewrite* (shrinking the head), not
a superset blob.

**MobKit interim behavior.** None needed in mobkit itself; OB3 carries
chunked plain-text persistence as a workaround. This ask also sets the
persistence contract MobKit's memory providers will follow (memory writes
must stay O(delta) — architecture §12 discipline).

## Ask 12 — Compaction-archive must not strand singletons

**Title:** Fix retire-after-compaction stranding (`MonotonicityViolation` on
ArchiveSession shrink-save, no fallback, ghost roster entry)

**Problem statement.** When a member is retired after compaction, the
ArchiveSession shrink-save can hit `MonotonicityViolation`; there is no
bridge fallback, and the roster keeps a ghost entry. Respawn/reset cannot
recover the identity — only a full process restart. For fleets of years-lived
singletons this is a standing incident class, and compaction boundaries are
exactly where MobKit's Distiller harvest hooks live.

**Evidence + spec.** `ob3_validator/docs/MEERKAT_BUG_compaction_archive_singleton.md`
(authoritative repro + trace).

**Proposed shape.** Per the OB3 doc: make the archive save tolerate (or
sequence around) the shrink, and give the failure path a typed, recoverable
outcome instead of a ghost entry.

**MobKit interim behavior.** None possible mobkit-side (the seam is inside
meerkat's session archive path); hosts carry watchdogs + process restarts.

## Ask 13 — Event-forwarder backoff + per-member commit-failure quarantine

**Title:** Stop dead-stream hot-loops and fleet-wide commit cascades

**Problem statement.** (a) The agent-event forwarder retries dead streams at
~4 Hz per identity, forever — measured 208,693 warnings in 50 minutes on one
fleet; (b) a runtime-loop commit failure cascades fleet-wide instead of
quarantining the single failing member. At hundreds of members these are
outages, not nuisances.

**Evidence + spec.** `ob3_validator/docs/MOBKIT_EVIDENCE_forwarder_hotloop_and_commit_cascade.md`
(authoritative; includes measurements and call sites).

**Proposed shape.** Exponential backoff with a ceiling (and eventual typed
terminal state) on dead event streams; per-member isolation on commit
failure — one member degrades, the fleet continues.

**MobKit interim behavior.** None mobkit-side; OB3 wraps every mob call in
app-level timeouts and runs its own wedge sentinel.

## Ask 14 — Typed member health/progress surface

**Title:** A per-member liveness/progress signal, so hosts stop
reverse-engineering health from event streams

**Problem statement.** Nothing in meerkat-mob answers "is this member alive
and making progress?". Production hosts hand-build it: OB3 runs a
WedgeSentinel (accepted-work vs event-flow gap > 300 s → pod restart) and a
silence watchdog (compaction-failed-then-silent detection → auto-respawn with
cooldown + breaker). The console "working" indicator bug is the same gap
surfacing in UI (indicator driven by event recency, not run-open state).

**Evidence.** `ob3_validator/src/wedge.rs`, `ob3_validator/src/watchdog.rs`;
`ob3_validator/docs/MOBKIT_BUG_console_working_indicator_clears_mid_run.md`.

**Proposed shape.** A typed per-member health projection (last-event-at,
run-open/idle state, in-flight work count, wedge classification) queryable
from `MobHandle` and/or emitted as a low-rate status event. Design ask —
shape open.

**MobKit interim behavior.** Hosts keep their watchdogs; MobKit's console
approximates from event recency.

## Ask 15 — Persist `interaction_id` in the transcript

**SHIPPED in meerkat 0.7.25** (addendum included: `SubscribableInjector::inject_with_interaction_id` + `WorkSpec.interaction_id` thread host ids to runtime admission; `TranscriptMessageIdentity` persists onto committed messages). Mobkit adoption (0.7.30): identity-first console sends mint UUIDv5 interaction ids and thread them via `send_with_mode_and_interaction` → bridge `WorkSpec`; session-history backfill stamps `interaction_id`/`run_id` onto frames; console dedup treats UUID-form ids as authoritative twin identity. Residual: the CLASSIC console send path cannot thread (the external work door `external_turn_for_member` has no interaction parameter) — candidate follow-up ask.


**Title:** Give live and historical copies of the same assistant reply a
shared identity

**Problem statement.** The console must deduplicate a live-streamed reply
against its own persisted twin, but the two share no id — forcing lossy
content-heuristic dedup (the mobkit console over-cull class). The
`TranscriptMessageIdentity` machinery (meerkat #808) is the natural home;
it currently carries identity for other purposes but the interaction id is
not persisted through the transcript.

**Evidence.** MobKit console dedup investigation (2026-07-01): an
interaction-id-based mobkit fix was a no-op and was reverted because the
persisted transcript lacks the id; heuristic collapse remains.

**Proposed shape.** Persist `interaction_id` (and `run_id`, already modeled)
in `TranscriptMessageIdentity` for assistant messages, serde-defaulted so old
sessions deserialize unchanged.

**MobKit interim behavior.** Content-heuristic dedup with its documented
over-cull edge cases; removed when the id lands.

---

**Explicitly not asked (batch 2):** multi-embodiment / multi-bind primitives.
The two production deployments (OB3, HomeCore) both run strictly one live
session per identity; MobKit ships a fail-closed single-embodiment guard and
will file the ask when a real use case materializes.

---

# Batch 3 — schedule firing pipeline (2026-07-06, HomeCore 0.7.24 field report)

Field context: rpc_gateway persistent mode, on-disk SQLite schedule store
carried across upgrades since ~0.7.13-era binaries. Authoring and planning
work; NOTHING fires. Every occurrence in the store sat at `phase=pending`
with `lease_expires_at_ms=NULL` — including a fresh one-shot authored on a
just-cleaned store — with zero log output at any level. Root-caused in the
published 0.7.18 sources (all four asks are visible there); mobkit ships a
read-only watchdog (`spawn_schedule_claim_watchdog`) that names the stall
and the poisoned row, but only meerkat can make the driver survive one.

### Ask 15 addendum (2026-07-08, mobkit issue #254 item 3)

A second consumer class hit this: the console aggregator's session-history
backfill re-emits past turns as `interaction_complete` frames with
`session_id` set and `interaction_id: None` (nothing to stamp — the
transcript doesn't persist it), and identity queries force-trigger
backfill. A consumer correlating by session id alone treats old history as
fresh completions (field: false job completion). mobkit interim: frames
carry `source.kind == "session_history"` and the API docs now declare its
exclusion mandatory. Real fix remains this ask: persist interaction ids in
the transcript so backfill can stamp them.

## Ask 16 — `spawn_schedule_host` discards every driver tick error

**Problem statement.** `meerkat/src/surface/schedule_host.rs` runs the firing
loop as `let _ = driver.tick_once().await;` every 250 ms. When ticks fail
persistently (see asks 17–19 for why they do), schedules silently stop firing
forever: no ERROR, no WARN, nothing for `RUST_LOG=debug` to show — the error
value is dropped before tracing can see it. The HomeCore operator offered a
debug trace; there was literally nothing to capture.

**Proposed shape.** Log `tick_once` errors — on first occurrence and on
change at ERROR, with a rate-limited heartbeat while the same error persists;
count consecutive failures in the report struct. A driver that cannot claim
is an incident, not a no-op.

## Ask 17 — one poisoned row starves every schedule (all-or-nothing scans)

**Problem statement.** `ScheduleDriver::tick_once` begins with
`service.list()` — which fails wholesale on the FIRST schedule row whose
recovered machine state is rejected — and `claim_due_occurrences` (sqlite
store) deserializes and `classify_due_action`s EVERY occurrence row in the
store inside one transaction before leasing anything. Any single stale or
invariant-rejected row (old schema_version, `misfire deadline projection
does not match machine_state`, corrupted JSON) aborts the entire tick. One
bad row → zero claims, for every schedule, forever — combined with ask 16,
silently.

**Evidence.** HomeCore: 31/31 occurrences pending, no lease ever taken, on a
store whose rows span ~5 binary generations. A fresh, valid one-shot on the
same store never fired — starved by a neighbor row.

**Proposed shape.** Per-row tolerance in both scans: a row that fails
deserialization or due-classification is skipped and logged (schedule_id /
occurrence_id + reason), optionally quarantined to a dead-row table or a
`phase=quarantined` marker so operators can inspect and purge. The tick
claims everything healthy.

## Ask 18 — Deleted schedule tombstones fail recovery and kill `list()`

**Problem statement.** Deleting a schedule persists a tombstone row whose
recovered `ScheduleLifecycleMachine` state is then REJECTED on read:
`RecoveredStateInvariantRejected { phase: Deleted, invariant:
"deleted_has_no_planning_cursor" }`. Every subsequent `list schedules` (and
therefore every driver tick and every mobkit boot repair) fails until the
operator hand-deletes rows in sqlite. The writer and the recovery invariant
disagree about what a legal Deleted state looks like — one of them is wrong.

**Evidence.** HomeCore boot logs (every boot until manual cleanup of 16
tombstones): `list schedules: serialization error: generated
ScheduleLifecycleMachine rejected recovered machine_state: ...`.

**Proposed shape.** Either (a) the delete flow clears the planning cursor
before persisting the tombstone AND a one-time migration repairs existing
tombstones, or (b) recovery accepts Deleted-with-cursor and normalizes it.
Plus an upgrade-carry test: delete a schedule under version N, open under
N+1, `list()` must succeed.

## Ask 19 — claim scan reads the whole store per tick

**Problem statement.** `claim_due_occurrences_impl` SELECTs and deserializes
every occurrence (joined with its schedule) with no SQL predicate on phase or
due time, four times per second. Beyond the poison surface (ask 17), this is
O(store) per tick: OB3-scale stores (multi-GB, years of terminal receipts and
occurrences) pay full-table deserialization at 4 Hz.

**Proposed shape.** Push the filter into SQL — `WHERE phase-in-pending-set
AND due_at_ms <= :now + horizon` (due_at_ms is already a column and already
indexed by the ORDER BY) — and let per-row tolerance (ask 17) handle the
stragglers. Terminal rows never enter the scan.

**MobKit interim behavior (ships in 0.7.25).** Read-only
`spawn_schedule_claim_watchdog` in both gateways: probes `list()`, the
occurrence scan, and overdue-pending-unclaimed state every 60 s, and logs a
row-level diagnosis (poisoned row ids via direct sqlite triage) when the
pipeline is stalled. It makes the failure loud and attributable but cannot
make the driver claim past a poisoned row.

---

# Batch 4 — meerkat-studio field report (2026-07-06)

Reporter context: desktop app pinning meerkat =0.7.17 / mobkit =0.7.23; thread
mobs through an embedded gateway (FactoryAgentBuilder →
PersistentSessionService → MobBootstrapSpec → UnifiedRuntime), helper crews
through per-thread mobkit_gateway HTTP children. Asks M1–M4 below are the
reporter's, relayed verbatim-in-substance; ask 20 is mobkit's root-cause of
the reporter's K1, which turned out to be upstream.

## Ask 20 — retire/respawn of a never-ran member fails ArchiveSession and strands it in `retiring` (reported as mobkit K1) — P0

**Problem statement.** A session-bound mob member that has never run a turn
has no runtime-store session snapshot (the machine commits at run
boundaries). Retiring it runs the disposal pipeline to completion, but the
ArchiveSession step's authority lookup returns NotFound
(`load_runtime_authority_session_for_control` → no runtime snapshot →
`Ok(None)` → "mob archive authority returned NotFound for registered runtime
session …"), which `destroy_disposal_failure` escalates to a fatal error
("disposal completed but ArchiveSession failed"). The member is left in the
roster at `state=retiring`, `is_final=false` — permanently. `respawn` runs
retire first, checks `roster_still_contains_member` — still true — and
aborts, leaving a cancelled-kickoff zombie. Net: the only per-member control
primitives on a non-crashing path (see M1) fail for exactly the members an
operator most wants to manage.

**Evidence.** Deterministic mobkit repro
(`meerkat-mobkit/tests/studio_k_asks.rs`, the `#[ignore]`d persistent test):
3-member crew via `ensure_member` on
FactoryAgentBuilder→PersistentSessionService; both RPCs fail with the exact
field string; members verified stranded in `retiring`. The identity-first
bridge already classifies this exact string as recoverable for SESSION-OWNED
retire (mobkit `is_recoverable_session_owned_retire_cleanup_error`) — the mob-
member path needs the equivalent judgment at the source.

**Proposed shape.** In meerkat-mob (the escalation site is the archive
helper at `src/runtime/provisioner.rs:894`): when disposal COMPLETED and the
archive miss is `NotFound for registered runtime session`, treat the
session-owned member as already-archived — per the helper's own doc comment
— and let the retirement commit finish; respawn then proceeds naturally.
(Alternative: register/commit an initial runtime snapshot at member session
creation so the authority never misses; either side of the seam works, the
judgment belongs upstream.) Do NOT fix this with downstream tolerance:
retrying fails identically and `list_members` is unchanged afterwards — the
member is neither respawned nor removed — so a console-layer
"tolerate and report accepted" patch misreports reality (verified
empirically by both mobkit and the reporter, independently). mobkit's
identity-first bridge already carries the exact tolerance string
(`is_recoverable_session_owned_retire_cleanup_error`) but it never applies
on the console `handle.retire()`/`respawn()` path, and lifting it there
would hit the misreporting trap above. Regression: retire + respawn of a
freshly spawned, never-prompted member on a persistent service must
succeed; a `roster.get(...)`-after-failed-retire assertion to pin the
no-strand invariant.

## M1 — force_cancel_member stack-overflows (SIGABRT) mid-turn — P0

`MobHandle::force_cancel_member` → `MobActor::handle_force_cancel` →
provisioner `interrupt_member` → `LocalMobRuntimeBridge::interrupt_member` →
`MeerkatMachine::cancel_after_boundary` recurses inside
`execute_meerkat_machine_command` until the tokio worker aborts the process.
Reporter byte-diffed the chain: unchanged 0.7.4 → 0.7.17. No downstream
consumer can stop a running member; the model turn burns tokens to the next
boundary. Acceptance: cancel during an in-flight (not idle) turn interrupts
and returns without crashing; regression test for exactly that.

## M2 — no MCP path for library/factory embedders; per-spawn overlay lossy — P1

`AgentBuildConfig` has `external_tools`/`wait_for_mcp` but no MCP-server
config; `.rkat/mcp.toml` is CLI-only. The working embedder pattern
(`McpRouter::new_with_surface_handle(RuntimeExternalToolSurfaceHandle::ephemeral())`
→ stage_add → apply_staged → wait_until_ready → inject via
`SpawnMemberSpec.external_tools`) exists only in meerkat test code
(`service_factory.rs:1827`); `McpRouter::new()` fail-closes stage_add. With
`builtins = true`, `CompositeDispatcher` does not forward
`bind_external_tool_surface_handle`/`bind_mcp_server_lifecycle_handle` (only
`bind_ops_lifecycle`), so session-time late binding never reaches the
adapter. And `materialize_revived_member_session` composes
`external_tools_for_profile(&profile, None)` — revived members silently lose
per-spawn tools. Acceptance: a supported way to attach an MCP stdio server to
factory-built agents that composes with builtins, survives revival, and needs
no test-code incantation. (Mobkit shipped its half: a mob-wide
revival-surviving provider seam, `MobBootstrapSpec::with_default_external_tools_provider`.)

## M3 — semver discipline within 0.7.x — P1

0.7.16 changed `PersistentSessionService::new` (Option → required Arc);
0.7.12 added a required `content_taint` field to `CommsCommand::PeerMessage`.
Both broke 0.7-pinned downstreams. Ask: treat public-signature changes as
breaking (minor bump) or publish a meerkat↔mobkit compatibility matrix.
(Mobkit now exact-pins its meerkat family and records the pin per release.)

## M4 — durable run/interaction lifecycle query — P2

Run framing is broadcast-only; after a host restart there is no "did
interaction X reach a terminal state, and which?" query. Reporter hand-rolled
runs.jsonl + a lookup RPC. A first-class terminal-status-by-interaction/run-id
query in meerkat-session/runtime deletes that code for every embedder.

## Ask 21 — owned-but-snapshotless sessions still strand on archive (ask-20 residue) — P1

**Problem statement.** 0.7.19's ask-20 fix routes disposal on
`session_known_to_archive_authority`, but that gate only reroutes sessions
the authority does NOT own (`known=false`, host-adopted). A mob-CREATED
member that has never run a turn is `known=true` — its durable session
record exists (create persisted it) — while its RUNTIME SNAPSHOT does not
(the machine commits at run boundaries). The owned-path archive then fails
exactly as before: `archive_with_mob_lifecycle_authority` → control-read
(`load_persisted_session_for_control`) NotFounds on the missing snapshot →
"disposal completed but ArchiveSession failed: … NotFound for registered
runtime session" → member stranded `state=retiring`. The authority-read
(`load_authoritative_session`, store-projection-eligible) and the
control-read disagree about the same session.

**Evidence.** Deterministic mobkit repro on meerkat =0.7.19
(`meerkat-mobkit/tests/studio_k_asks.rs`, `#[ignore]`d persistent test):
3-member `ensure_member` crew on FactoryAgentBuilder→PersistentSessionService;
probe confirms `session_known_to_archive_authority = Ok(true)` for all three
idle members; retire still fails with the exact field string. The wrapper-
forwarding gap on mobkit's side is FIXED (mobkit forwards the seam through
`PreBuildMobSessionService`/`AfterCreateMobSessionService`); the remaining
failure is upstream.

**Proposed shape.** Make the owned-path archive tolerate the
never-committed-snapshot case: when the durable record exists, the runtime is
registered, and NO snapshot was ever committed, archive the durable record
and retire/release the runtime binding (the control-read should accept the
same store-projection eligibility the authority-read already accepts).
Regression: retire + respawn of a freshly created, never-prompted member on a
persistent service with a runtime store must succeed.

**MobKit interim behavior.** Identity-first gateways (default-on since
0.7.25) route crew members through the identity authority, which disposes
tolerantly — the strand is only reachable for mob-plane (worker) members
that never ran, a narrow window in practice since workers receive kickoff
messages at spawn.

## Ask 22 — past-due one-shot regenerates occurrences unboundedly (planning-cursor precision loss) — P0

**Field report (HomeCore on 0.7.25):** one one-shot with a fire time
near/just-past now produced 223 misfired occurrences + 223 receipts in ~2
minutes (~1/sec, unbounded) on a clean store; halting required stopping the
gateway and truncating the schedule tables.

**Root cause (found by Luka; repro sits as a failing test in
meerkat-schedule/src/driver.rs):** sub-millisecond precision loss in the
planning-cursor round-trip. A one-shot's `due_at_utc` is a full-precision
`DateTime<Utc>` (ns); the machine-owned planning cursor is
`planning_cursor_utc_ms` (ms) — `RecordPlanningWindow` stores
`truncate_ms(due)`. The planner's guard `next_due_after(Once, cursor)`
yields when `due > cursor`, and `truncate_ms(due) < due` by up to 999µs, so
the trigger re-yields the same due forever. The pending-occurrence dedupe
only covers PENDING dues — the moment the occurrence goes terminal
(misfired in the incident), nothing blocks the re-plan: new occurrence,
same due, next ordinal → immediately misfires → terminal → next tick
re-plans. Aggravations: the same loop shape fires for COMPLETED one-shots
(re-plan → re-dispatch = double-fire, not just misfire spam), and interval
triggers are plausibly exposed on their last-planned slot.

Mobkit cross-confirmation: the loop reproduces with pure meerkat service
APIs (`refill_horizon` + `claim_due_occurrences`) — no mobkit code in the
cycle; the #237 claim watchdog is read-only and ticks at 60s (cannot drive
a 1/s loop). The ms/ns mismatch also explains regeneration WITH a pending
occurrence present: the stored occurrence due is ms-truncated while the
trigger re-yields the ns-precision due, so the `existing_due` dedupe set
never matches. Carry guard: `one_shot_misfire_must_not_regenerate`
(`#[ignore]`d in schedule_wiring tests, un-ignored on the 0.7.20 upgrade).

**Why the machines didn't catch it (recorded for the RCT discipline):**
(1) an RCT failure, not transition legality — one semantic fact ("planning
has covered up to T") stored at ms precision but compared at ns across the
shell/machine boundary; no RCT existed for the cursor's time-precision
round-trip. (2) The machines verify per-entity safety, not cross-entity
convergence — every lap of the loop is legal (the M1 cancel-recursion
family); "a Once trigger plans at most one occurrence, ever" and "the
planning cursor is monotone/idempotent under re-planning" were never
encoded as machine-checkable invariants.

**Fix (dogmatic, for 0.7.20):** (1) normalize ALL schedule time facts to ms
precision at the domain admission boundary (Once due_at, interval
start/end, planned dues) — one precision for one fact everywhere, matching
the sqlite `due_at_ms` columns; (2) a machine-owned convergence invariant
(`trigger_exhausted`-style fact, or RecordPlanningWindow terminal for Once)
so even a future representation bug converges instead of running away;
(3) regressions: the failing one-shot misfire repro, completed-one-shot
no-refire, interval terminal-tail stability over N ticks.

### Ask 21 addendum (21b) — 0.7.20 verification: read fixed, archive still NotFounds afterwards

Verified against meerkat =0.7.20 with the mobkit repro: the archive-scoped
read (`load_persisted_session_for_archive`) works — traces show the durable
document COMMITTING (`discarding stale live session … reason=StoredArchived`)
— yet `archive_with_mob_lifecycle_authority` still returns
`SessionError::NotFound` for the never-run registered session, so the
provisioner escalation ("mob archive authority returned NotFound for
registered runtime session") and the `retiring` strand persist unchanged.
The failure now sits AFTER the snapshot resolution — candidates: the
runtime-retire realization for a registered-but-snapshotless machine session
(the register-before-retire fallback may still NotFound on the second
retire), or the post-commit archived-typed-NotFound contract folding into
the error path mid-protocol.

Empirically ruled out on the mobkit side: the two-adapter split (a separate
`with_session_runtime_adapter` machine vs the service's cached adapter) —
the repro fails identically when the mob shares the concrete service's own
cached machine as the sole authority. Wrapper forwarding of
`session_known_to_archive_authority` verified live (probe: `Ok(true)`).

Repro unchanged: `meerkat-mobkit/tests/studio_k_asks.rs`, `#[ignore]`d
persistent test; the trace capture recipe is a `tracing_subscriber` init
with `meerkat_mob=debug,meerkat_session=debug` in the test body.

## Ask 21c — 0.7.21's retire-completing archive arm deadlocks the whole mob (runtime-loop self-deadlock on the session mutation gate) — P0, blocks 0.7.21 adoption

Verified against meerkat =0.7.21 with the mobkit K1 persistent repro
(`meerkat-mobkit/tests/studio_k_asks.rs`,
`studio_k1_retire_respawn_succeed_on_persistent_ensure_member_crew`):
respawn of a never-run member now HANGS FOREVER at 0% CPU (deterministic,
4/4 runs) instead of 0.7.20's fast-fail NotFound. The wedged task is the
single-threaded MobActor, so the ENTIRE MOB is dead — every subsequent mob
command queues forever. Severity inverted: 0.7.20 stranded one member in
`retiring`; 0.7.21 bricks the mob. mobkit is HOLDING its pins at =0.7.20
and will not ship 0.7.21 to consumers until this is fixed.

**Root cause — a single-task self-deadlock in the runtime loop's
terminal-failure exit, PRE-EXISTING on 0.7.20 and newly load-bearing:**

1. During disposal, the boundary cancel makes the member's in-flight run
   fail; the runtime-loop task takes the terminal-failure arm and acquires
   the per-session mutation gate (`runtime_loop.rs:1547-1549`
   `lock_current_driver_authority` → `mod.rs:1292` →
   `lock_current_session_mutation_gate` → the entry's `mutation_gate`).
2. Recording the terminal fails — `driver.rs:1634` "generated RunFailed
   authority absent for run …" (the cancel already moved the generated
   lifecycle off the run) — and, STILL HOLDING THE GATE, the arm calls
   `stop_runtime_loop_executor_from_dsl_effect` (`runtime_loop.rs:1597-1603`;
   the guard drops only at scope exit, `:1613`).
3. That awaits inline `control_plane.rs:79
   executor.cleanup_after_runtime_stop_terminalized()` → the mob executor
   (`provisioner.rs:1841-1856`, the "mob runtime executor received stop"
   log) → `runtime_adapter.unregister_session` →
   `unregister_session_inner` (`session_management.rs:1357`) whose first
   await is `lock_current_session_mutation_gate` (`:1359`) — the SAME
   non-reentrant tokio mutex its own frame holds. The task parks forever
   (trace shows "unregister_session_inner start" but never "locked
   mutation gate"). Cycle of length 1: B waits on B.
4. First victim: the MobActor's disposal chain (`handle_respawn` →
   `dispose_member` → `SessionBackend::archive_with_authority_then_unregister`
   → `archive_with_mob_lifecycle_authority` →
   `PersistentSessionService::archive_with_machine_protocol`). The NEW #845
   arm (`session_document.rs:2744-2752`: Ready + Archived +
   `runtime_session_registered==true` → retire-only action vector,
   `write_document:false, retire_runtime:true`) proceeds where 0.7.20
   returned NotFound before any gate work, and calls
   `retire_runtime_control_plane` (`traits.rs:634`) → `gate.lock().await`
   (`traits.rs:645-647`) on the gate the parked loop task holds. Note
   `runtime_session_registered` is true precisely BECAUSE the unregister is
   the deadlocked continuation — the arm's own trigger condition is the
   deadlock's output.
5. Second victim (diagnostic red herring): mobkit's detached console
   event forwarder's 250ms reconcile sends
   `MobCommand::ApplyMachineInputEffects` (`handle.rs:6058`) to the wedged
   actor and parks on the reply. Not load-bearing.

**Why this is ALSO the true root of asks 20/21/21b:** the loop-task
self-deadlock exists on 0.7.20 with identical structure
(v0.7.20 `runtime_loop.rs:1545-1560`) — every never-run-member disposal
leaks a forever-parked runtime-loop task holding the gate AND leaves the
runtime session registered forever (unregister never runs). That
permanently-registered session is exactly the state 21/21b kept hitting.
0.7.21 fixed the SYMPTOM arm (archive of Archived+registered) while the
producer of that state still deadlocks upstream of it.

**Fix directions (in preference order):**
1. Drop the terminal-arm authority guard BEFORE
   `stop_runtime_loop_executor_from_dsl_effect` — the ChannelClosed arm
   already does exactly this (`runtime_loop.rs:1163` explicit
   `drop(effect_authority_guard)` with a comment); the terminal-failure arm
   builds the inverse of session_management.rs:1391-1397's own warning
   ("awaiting the loop under the gate would deadlock" — here the loop
   awaits unregister under the gate).
2. Audit the sibling stop-under-gate sites with the same latent shape:
   select! direct-effect arm (`runtime_loop.rs:768-777`), process_queue
   effect-drain (`:1140-1146`), queue-processing failure stops
   (`:1362`, `:1402`, `:1653`), Ok-arm commit/checkpoint-failure stops
   (`:1486`, `:1520` — guard from `:1439`).
3. Defense in depth: `retire_runtime_control_plane` should not block a mob
   actor indefinitely on the gate (bounded acquisition + typed busy error),
   so a future regression of this class fast-fails instead of wedging mobs.

**Acceptance (both mobkit repros, both must pass):**
- `studio_k1_retire_respawn_succeed_on_persistent_ensure_member_crew`
  (classic persistent chain) — currently DEADLOCKS on 0.7.21. Run it with
  a nextest per-test timeout; a hang is a fail, not a slow test.
- `doctrine_member_rpcs_route_identity_owned_members_through_identity_authority`
  (identity-first gateway construction, mob-plane worker respawn) —
  currently still fails on 0.7.21 with the ORIGINAL 21b error, fast:
  "respawn_member failed: … disposal completed but ArchiveSession failed:
  … mob archive authority returned NotFound for registered runtime session
  <id>". I.e. 0.7.21's #845 arms do not fire at all on this construction
  (first-archive of a never-run session: document not yet Archived, so the
  Ready+Archived+registered arm cannot match — ask 21's original
  owned-but-snapshotless strand). 21b is NOT converged on 0.7.21; fixing
  the loop-exit deadlock (which un-sticks unregister) likely changes this
  path too — re-derive the arms against a world where unregister actually
  completes.

Trace capture recipe unchanged (tracing_subscriber in the test body;
`meerkat_mob=debug,meerkat_runtime=debug` shows the full timeline:
RunFailed authority-absent ERROR → "unregister_session_inner start" with
no "locked mutation gate" → "retire_runtime_control_plane start" → silence).

### Ask 21 addendum (21d) — 0.7.22 verification: classic chain converged, identity-first-construction workers still archive-NotFound — P1

meerkat 0.7.22 (#847) verification with both mobkit acceptance repros:

- `studio_k1_retire_respawn_succeed_on_persistent_ensure_member_crew`
  (classic persistent chain): **PASSES** — retire AND respawn of never-run
  members converge in <0.3s. Ask 21c fixed as specified; the whole
  20/21/21b/21c family is closed on this construction. Un-ignored for good.
- `doctrine_member_rpcs_route_identity_owned_members_through_identity_authority`
  (identity-first gateway construction, mob-plane worker): **still fails**,
  fast, with the ORIGINAL error: "respawn_member failed: … disposal
  completed but ArchiveSession failed: … mob archive authority returned
  NotFound for registered runtime session <id>".

Trace delta vs the (now green) classic chain: on this construction
`SessionBackend::retire_runtime_before_archive` logs "calling runtime
retire" → "runtime retired" (the runtime is NOT already terminal, and the
0.7.22 bounded retire genuinely succeeds — no deadlock, no 30s stall),
then "archive_with_authority_then_unregister archiving session service"
→ the dispose ArchiveSession step fails NotFound with the session still
REGISTERED per the escalation text. I.e. the durable runtime is Retired
by the pre-archive retire, yet the archive protocol still resolves
NotFound and the machine still reports the session registered — the #845
Ready+Archived+registered retire-only arm apparently does not fire (or
fires and NotFounds) on this wiring. Construction difference to focus on:
the identity-first gateway runtime (mobkit `gateway_wiring::
open_identity_substrate` + identity_first bridge on MobHandle) with the
worker member on the mob plane; session service wiring differs from the
classic FactoryAgentBuilder → PersistentSessionService chain.

Repro: `meerkat-mobkit/tests/studio_k_asks.rs`,
`doctrine_member_rpcs_route_identity_owned_members_through_identity_authority`
— currently TOLERANT of the ArchiveSession failure class (assert-routing
only); tighten the branch at the end of the test when 21d lands. Trace
recipe: tracing_subscriber init in the test body,
`meerkat_mob=debug,meerkat_runtime=info,meerkat_session=debug`.

Severity: P1 not P0 — fast-fail (no wedge, no strand escalation beyond the
known retiring-retry state), identity-first DURABLE members are unaffected
(tolerant disposal on the identity plane), and the classic chain is fixed.
Affects mob-plane worker churn under identity-first gateways (HomeCore
agent-tool workers, OB3 worker plane after migration).

## Ask 23 — break-glass host reassignment for stuck bindings — P3 (reframed 2026-07-08)

**SHIPPED in meerkat 0.7.25** (`WorkGraphService::break_glass_reassign_attention`, host API only, mandatory principal+reason, event-stream audited). Mobkit adoption (0.7.30): `mobkit/workgraph/attention/break_glass_reassign` on the CONSOLE surface only; principal = authenticated console principal.


meerkat 0.7.23's `reassign_attention` requires the witness's
`can_link_derived_from`, which the machine derives ONLY for
coordinate-mode bindings (`tool_surface.rs` pins: coordinate=true,
pursue=false). That is the right model for AGENT-driven reassignment,
but it also binds the HOST: an operator console cannot move a
pursue/review/falsify/judge/observe binding to another member at all —
there is no host-authenticated reassign that bypasses the mode-derived
authority, and hosts must not forge projections. mobkit 0.7.30 surfaces
a precise error and hides the affordance for non-coordinate bindings;
the workaround (goal_request_close + create a new goal on the new
target) loses binding history/evidence continuity.

REFRAMED per doctrine (Luka, 2026-07-08): WorkGraphs are AGENT-operated;
humans debug. The mode-derived restriction is the design, not a gap — the
agent-native transfer is a COORDINATE-mode agent executing the move at a
human's conversational request (pursuers can only request, which is the
right shape: transfers are coordination acts). The one genuinely stuck
case is a binding on a wedged/retired agent with NO coordinator holding
authority over it — the graph cannot heal agent-natively.

Ask (narrowed): a BREAK-GLASS host reassign — explicitly attributed to an
authenticated principal, audit-logged, intended for debug/recovery only
(`with_trusted_principal`-style promotion on `AttentionReassignRequest`).
Not an operating path; the agent tool surface's mode-derived restriction
stays untouched.

## Ask 24 — filtered attention queries + terminal-binding GC — P2

**SHIPPED in meerkat 0.7.25** (SQL-pushed status/target filters over indexed columns with NULL-tolerant migration; `prune_terminal_attention`; clause-3 metadata-only session read seam as `SessionStore::load_meta`). Mobkit adoption (0.7.30): `mobkit/workgraph/attention/prune` + SDK methods.


`SqliteWorkGraphStore::list_sqlite_attention` (0.7.23 store.rs:1576) runs
`SELECT attention_json FROM workgraph_attention` with no WHERE clause and
JSON-decodes EVERY row — all realms, all statuses — before Rust-side
filtering; and every `reassign_attention` keeps the superseded row forever,
so the table grows monotonically with binding churn. Any host that must
read binding state on a hot path (mobkit's one-binding-per-target
admission guard runs under a runtime-wide gate and, on shared stores, a
cross-process lock) pays an unbounded scan while serializing all binding
mutations behind it; at OB3-style eternal-fleet churn this walks into
lock-timeout territory.

Ask: (1) push `realm_id`/`namespace`/`status`/`target` filters into the
SQL (indexed columns or generated columns over the JSON); (2) a GC/prune
facility for terminal (Superseded/Stopped) bindings, mirroring the ask-2
memory-store lifecycle shape. mobkit interim: realm+status-filtered list
calls (upstream still decodes every row) and an actionable lock-timeout
error; the guard's session→member resolution memoizes per session id, but
each FIRST resolution still pays `load_persisted_session`'s full-session
deserialization — `MobSessionService` has no metadata-only read seam (its
own ask candidate).

## Ask 25 — attention-binding uniqueness belongs in the service/store (or arbitrate like sessions) — P1

**SHIPPED in meerkat 0.7.25** — BOTH halves: store-level transactional uniqueness with typed `Conflict` naming the occupant, AND newest-binding-wins arbitration for legacy duplicates (`MultipleActiveBindings` removed). Mobkit's admission demotes to defense-in-depth + session↔identity alias unification; sidecar-lock removal is the 0.7.31 follow-up.


`MultipleActiveBindings` is a HARD per-turn error (0.7.23
meerkat/src/surface.rs:209): two active bindings matching one member's
turns brick that member until an operator intervenes. But nothing upstream
prevents the state: no store-level uniqueness constraint, and
`create_goal` / `reassign_attention` / `resume_attention` all mint
duplicates freely. mobkit 0.7.30 compensates with a host-layer admission
guard (occupancy check + per-runtime gate + cross-process SQLite sidecar +
write-time target normalization + session-metadata fallback) — five
review rounds of hardening for an invariant that can only be enforced
race-free next to the data. Residual holes remain by construction
(meerkat-CLI writes to a shared store bypass any host guard).

Ask, either (both is best):
1. Service/store-level uniqueness — active-binding-per-target enforced
   transactionally at create/reassign/resume (typed conflict naming the
   occupant), so hosts get the invariant instead of building it.
2. Degrade the overlay failure the way 0.7.23 degraded session
   arbitration (newest-session-wins): MultipleActiveBindings resolves
   deterministically (e.g. newest-binding-wins) with a loud diagnostic,
   instead of hard-failing every turn.

When either lands, mobkit's admission layer demotes to defense-in-depth.


## Ask 26 — structural reply affordance on comms deliveries — P2

**SHIPPED in meerkat 0.7.25** (`PeerReplyCapability` in the dispatch context, pre-addressed `reply_to_peer` comms tool, batch-level minting via the canonical `TurnToolOverlay::compose`). No mobkit change needed: nothing strips the overlay, and the console renders the tool generically. Verified at the 0.7.25 pin bump.


Peer messages arrive framed as prose ("Peer message from <endpoint>...")
with no structural reply affordance. Models reliably answer in their own
transcript instead of calling the comms tool back — charter/prompt
strengthening does not hold (mobkit consumer field report, 2026-07-08),
so every consumer ends up building an app-side relay.

Ask: deliver peer messages with a typed reply seam — e.g. a delivery
envelope carrying a `reply_to` capability/token the runtime turns into a
pre-addressed comms call (or a first-class `reply` tool scoped to the
delivery), so "answer the peer" is an affordance rather than a prompt
convention. mobkit interim: none (app-side relays).
