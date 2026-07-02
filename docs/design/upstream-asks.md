# Meerkat upstream asks — issue drafts

> **DRAFTS — not filed.** These are ready-to-file issue drafts for the meerkat
> repo, derived from
> [`agent-memory-architecture.md`](agent-memory-architecture.md) §13. Filing
> needs Luka's go-ahead. None of these block the MobKit memory initiative —
> each ask has a specified MobKit interim behavior, and each removes a class
> of defect at the right layer when it lands.

Evidence citations come from the memory survey
([`evidence/memory-survey/followups.md`](evidence/memory-survey/followups.md))
and were code-verified against the meerkat and mobkit working trees at survey
time (June 2026); line numbers may have drifted since.

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
