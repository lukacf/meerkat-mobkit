# Memory Console UI Proposal — The Ledger of What It Knows

**Status:** Proposal (design of record candidate)
**Scope:** MobKit console memory inspection and debugging UI
**Builds on:** P3b MemoryPanel (`console/src/panels/MemoryPanel.tsx`), the four `mobkit/memory/panel/*` RPCs (`meerkat-mobkit/src/http_console.rs`), `docs/design/agent-memory-architecture.md` rev 3
**Feasibility note:** every data source named below was re-verified against the code in this worktree; where the investigation reports were wrong, this document records the correction inline rather than propagating it (see §2.1).

---

## 1. Purpose

The memory system makes an agent's knowledge durable and makes falsifiable promises about how that knowledge behaves: injections never echo back into the store, tainted content cannot launder itself into trusted memory, LLM-authored records never exceed `agent_observed`, recall stays inside a byte budget, and dreams consolidate honestly. Today an operator can list records and read a detail pane, but cannot answer the two questions that matter:

1. **Understanding** — *what does this agent (or this mob) know right now, how much does it trust each piece, and how did that piece come to be here?*
2. **Verification** — *are the architecture's claims actually holding in this deployment, and if one broke, which exact rows falsify it?*

Both questions exist at two levels. **Agent level:** one identity's holdings, its composed injection block, its taint state, its distiller cursor. **Mob level:** the shared mob scope, cross-member promotion through the gate, whether every member is paying build-bytes for shared dead weight, and whether one member's experience demonstrably reaches the others.

This UI is read-only by design. Quarantine verdicts and gated promotions ride the existing gating flow (`gating.decide`); `mob.memory.propose`/`mob.memory.commit` remain reserved and unmapped (`http_console.rs:1618-1621`). The panel never grows write verbs.

---

## 2. The organizing concept

**The store is the primary object.** Every screen answers "what is known, with what trust, and how did it get here." Process — distill, dream, inject, supersede, quarantine — is never a separate event feed the operator must correlate by hand; it is rendered as the *biography attached to each record* and the *conveyor attached to each scope*. This inverts the timeline problem: instead of chasing 17 ephemeral `memory.*` event subtypes through a lossy in-memory ring (1024 frames/identity, 4096 total, gone on restart — `unified_runtime/console_events.rs`), the operator anchors on durable SQLite state — records, supersede chains, the injections ledger, audit rows, proposals, pending promotions/harvests — and uses live SSE events only as freshness signals.

Three grafted elements complete the concept (per the judged bake-off, Appendix A):

- **The Verify tab** (from Claims Board): the Ledger's procedural verification story becomes *ambient, continuously-evaluated proof* — claim tiles with a `HOLDING / DEGRADED / VIOLATED / UNVERIFIABLE` verdict vocabulary, where `UNVERIFIABLE` is first-class and names the missing surface.
- **Events terminate in state** (from Flight Recorder): every rendered memory event offers "state here" pivots into Ledger views; live lists use pause-on-scroll with a "paused — N behind ▸ jump to live" banner; wherever ring-backed data renders, a hairline marks "ring history starts here" so *no events* is never mistaken for *nothing happened*.
- **A later-phase Loop Trace lane view** (from Flight Recorder) repairs the Ledger's weakest criterion — the loop in motion at mob/realm scale — without letting its heavy composite-trace RPC block phase 1.

### 2.1 Corrections applied to the investigation record

Verified against this worktree; the proposal below uses the corrected facts:

| Investigation claim | Reality in code |
|---|---|
| "16 `memory.*` event subtypes" | **17** — `memory.quarantine.release_blocked` exists (`memory/events.rs:169`) and must be handled by any subtype switch. |
| Injection assembly budget "18KB" (Claims Board mock) | `MAX_INJECTED_ASSEMBLY_BYTES = 20 * 1024` (**20KB**, `memory/coordinator.rs:46`). |
| `assemble_build_injection` at `coordinator.rs:735` | It is at **`coordinator.rs:753`** (`inject_for_turn` at `:432`, nonce minting at `:405`). |
| Evidence click-through is "zero backend change" via `query_timeline` | **Partially true.** `mobkit/console/query_timeline` (contract TYPE-021) filters by `identity`/`conversation_id`/`after`/`before`/`mode`/`limit` — **not** by `session_id`. `MemoryEvidenceRef` carries `{session_id, generation, revision, range}` (`records.rs:215`, `types.ts:396-401`). Phase 1 ships click-through by querying the identity's timeline and matching `session_id` on the returned frames client-side; if that proves lossy, a `session_id` request param is a small contract addition (ranked in §6). Degrades to today's `evidenceLabel` text. |
| `ever_quarantined` "dropped in deserialization" | Correct for the **wire record** (`records.rs` has no field; `memory_panel_record_json` at `http_console.rs:1879` never emits it) — but the column *is* already read internally by the staged-batch validator view (`sqlite_store.rs:~2812`), so surfacing it is a wire-model field addition, not a new query. |
| Assorted line drift | `DreamRun` at `steward.rs:814`, `DreamOutcome` at `:850`; `resolved_at_ms` column at `sqlite_store.rs:157`; `injection_log_for_record` at `sqlite_store.rs:2120`. All other cited symbols confirmed: `scope_overview` `:1379`, `scope_floors` `:1373`, `injection_log` `:994`, `pending_proposals` `:1439`, `pending_harvests` `:1653`, `SessionTaintTracker` `taint.rs:227` (maps `:209-221`), `BackgroundBudget` `guards.rs:61` (`WindowExhausted`/`ConcurrencyCeiling` `:92-93`), `WindowState` `distiller.rs:980`, `DistillOutcome` `:997`, `handleLiveFrame` `ConsoleApp.tsx:2338`, `refreshMemoryData` `:2032`, `loadMemoryRecordDetail` `:2092`, both `describeMemoryTimelineEvent` copies (`packages/console-core/src/adapters.ts:439`, `console/src/lib/adapters.ts:435`). |

### 2.2 Standing constraints (apply to every view)

- **Adapters own formatting.** No raw envelope or event JSON reaches React. All `memory.*` event text goes through `describeMemoryTimelineEvent` (or a sibling pure helper in the adapters layer); new subtypes must land in **both** copies plus `adapters.test.ts`.
- **Nav and affordances follow the experience projection.** The Memory nav entry appears only from server-projected `experience.memory.can_read` (`ConsoleApp.tsx:1916-1934`); `console_config.rs` is view-level only and never authorizes. New surfaces gate on projected affordances, never on config.
- **ABAC is per-row, not per-method.** Entry gates via `console_rpc_access_requirements` (`http_console.rs:1574`), then per-row `memory_panel_record_visible` / `memory_panel_promotion_visible`: identity rows need scoped `agent.memory.read` + `agent.view`; mob rows `mob.memory.read`; operator rows explicit `operator.memory.read`; quarantined rows additionally `memory.quarantine.review`. Every new RPC replicates the two-layer pattern; hidden rows never consume page budget. `-32030` is tolerated per section, as `refreshMemoryData` already does.
- **Capabilities never surface.** `stage_token` (`http_console.rs:2137`) and the envelope nonce (`coordinator.rs:405`) appear in no response, log line, or error string.
- **Contract bijection is CI-enforced.** Every new RPC requires the `CONSOLE_RPC_METHODS` constant + a `docs/rct/console-rest-sse-contract-v0.5.0.json` entry + an `http_console.rs` dispatch arm visible to the `contract.test.ts` regex parser, plus the embedded-bundle rebuild (`console/build.cjs` → `console-dist` → freshness check).
- **List rows are body-free** (`body_bytes` stands in); only record detail carries bodies. Keyset paging is single-realm only, so paging UI always scopes to a realm first.

---

## 3. Views

The MemoryPanel grows from three tabs to a tab set over one shared data layer:

`[Holdings] [Records] [Knowledge] [Pipeline] [Dreams] [Verify]` + a Health strip footer, + (later phase) `[Trace]`.

All views reuse the `gating__*`/`gpolicy` CSS vocabulary, the Chip/`data-tone` idiom, and the inline back-button DetailView pattern MemoryPanel already ships — no routes, no orphaned detail pages, no new graph libraries.

**Cross-cutting interaction rules (grafts, apply everywhere):**
- Any live-refreshing list pauses follow on scroll and shows `⏸ paused — N behind ▸ jump to live`.
- Any surface backed by the SSE ring renders the hairline `── ring history starts here ──`.
- Any selected memory event (SignalsRail signal, timeline row) offers "state here" pivots: `[record biography] [scope overview] [taint snapshot] [budgets]`.

### 3.1 Holdings — store overview (per realm, per scope)

**Purpose.** The front door: one glance answers "what does this mob know, where is it concentrated, is anything unhealthy." Scope rows reuse `groupRecordsByScope`'s identity > mob > operator > realm ranking.

```
┌─ Memory · realm: homecore ─────────────────────────────────────────┐
│ [Holdings] [Records] [Knowledge] [Pipeline] [Dreams] [Verify]  ⟳  │
├────────────────────────────────────────────────────────────────────┤
│ SCOPE             ACTIVE  QUAR  SUPER  TOMB   BYTES   FLOOR        │
│ ▸ identity:ada      41     1     12     3    412KB  ▓▓░░░░ 12%     │
│ ▸ identity:bob      17     0      2     0     88KB  ▓░░░░░  4%     │
│ ▸ mob:research      23     2      5     1    301KB  ▓▓░░░░  9%     │
│ ▸ operator           6     0      0     0     14KB  ░░░░░░  1%     │
│ ▸ realm              9     0      1     0     52KB  ░░░░░░  2%     │
├────────────────────────────────────────────────────────────────────┤
│ TRUST MIX  operator ▓▓ · app ▓ · verified ▓▓▓ · observed ▓▓▓▓▓▓    │
│ LAST DREAM 2h ago · 14 ops · ⚠1 quarantined    PENDING GATE  2     │
│ QUAR QUEUE 3        PROPOSALS 4 pending / 1 held (taint)           │
├────────────────────────────────────────────────────────────────────┤
│ HEALTH  taint ada:clean bob:⚠mcp · distiller 7/12hr · [expand ▾]   │
└────────────────────────────────────────────────────────────────────┘
```

**Data sources.**
- **NEW** RPC `mobkit/memory/panel/overview` — thin handler over `SqliteAgentMemoryStore::scope_overview()` (`sqlite_store.rs:1379`, doc-comment already names the panel) + `scope_floors()` (`:1373`; floors 4000 records / 32MB). Zero new queries.
- **TODAY** `mobkit/memory/panel/dreams` — LAST DREAM strip.
- **TODAY** `mobkit/memory/panel/quarantine` — quarantine + pending-gate counts, rendered only when `experience.memory.can_review_quarantine`.
- **NEW** RPC `mobkit/memory/panel/proposals` over `pending_proposals()` (`sqlite_store.rs:1439`) — proposal counters incl. propose-time taint holds.
- **TODAY** SSE `memory.*` frames as a client-only refresh trigger: `frame.event.startsWith('memory.')` in `handleLiveFrame` (`ConsoleApp.tsx:2338`) re-runs `refreshMemoryData` while a memory panel is docked. Counters become live with zero new server surface.

**Interactions.** Scope row → Records pre-filtered to that scope (the records RPC already accepts `scope`/`identity`/`scope_key` — `refreshMemoryData` just never passes them). QUAR/PENDING GATE/PROPOSALS badges → Pipeline filtered to that stage. LAST DREAM → Dreams scrolled to that run. Trust-mix segment → Records filtered by trust (client-side; trust is on every row). Floor bar past 80% takes the warning tone from the existing chip vocabulary.

### 3.2 Records — master list + Record Biography detail pane

**Purpose.** The heart of the state-first design. Left: body-free rows with a scope/status/kind/trust filter bar and keyset load-more (all server-ready, unused params today). Right: the **Biography** — the record's entire life as attached history: BORN (author + evidence with transcript click-through), LINEAGE (single vertical supersede lane, ancestors dimmed, trust chip on every node so taint/trust flow is visible along the chain), LIFE (usage counters + per-record injection ledger), DREAMS (which steward runs touched it and what they did). Supersede-not-delete becomes navigable history — no trash icon anywhere.

```
┌─ Records ──────────────────────────┬─ Biography · rec_7f3a ────────┐
│ scope[mob:research▾] status[all▾]  │ "API uses keyset pagination"  │
│ kind[all▾] trust[all▾]             │ gotcha · agent_observed ·     │
│────────────────────────────────────│ ACTIVE · ⚑ ever-quarantined   │
│ ● rec_7f3a gotcha  observed  2d    │───────────────────────────────│
│ ● rec_91bc fact    verified  5d    │ BODY 1.2KB        [copy JSON] │
│ ○ rec_004d fact    observed  9d ᴰ  │ multi-realm queries merge one │
│ ⚠ rec_aa21 proc   untrusted 1h Q   │ bounded page per realm…       │
│ ● rec_c2e0 pref    operator  30d   │───────────────────────────────│
│   …                                │ LINEAGE                       │
│ [load more · cursor ms:id]         │  ● rec_7f3a  observed  now    │
│                                    │  ○ rec_51e0  observed  6d ᴰ88 │
│                                    │  ○ rec_0b77  untrusted 21d ⚑  │
│                                    │ BORN distiller run d_42       │
│                                    │  evidence sess_9 gen3 [12–19] │
│                                    │  → open transcript range      │
│                                    │ LIFE inj×14 recall×2 useful×5 │
│                                    │  last injected 2h ago (build) │
│                                    │ DREAMS run_88 set_rank ·      │
│                                    │  run_84 supersede← rec_51e0   │
└────────────────────────────────────┴───────────────────────────────┘
```

**Data sources.**
- **TODAY** `mobkit/memory/panel/records` — the filter bar maps 1:1 onto its existing `scope`/`identity`/`scope_key`/`status`/`limit`/`cursor` params; load-more uses `next_cursor` (single-realm only, `http_console.rs:1956` — the realm selector keeps paging honest).
- **TODAY** `mobkit/memory/panel/record` — full body + supersede chain (max 32) + per-record injections (max 50, via `injection_log_for_record`, `sqlite_store.rs:2120`). LINEAGE and LIFE render from this response with **zero backend change**.
- **TODAY, with the §2.1 caveat** `mobkit/console/query_timeline` — evidence click-through: query by identity, match `session_id` + range on returned frames client-side; **NEW (cheap)** `session_id` request param if client matching proves lossy. Degrades to `evidenceLabel` text.
- **NEW FIELDS** on `panel/record` JSON — `ever_quarantined` + `rank_set_at_ms` (durable columns `sqlite_store.rs:82`/`:72`, absent from the wire record). The ⚑ chip explains permanently-ceilinged trust.
- **NEW** `history[]` array on `panel/record` — per-record audit rows (audit table keys by `memory_id` with `op_kind`/`detail`/`applied_at_ms`, `sqlite_store.rs:102`; `dream_history` proves the read path). Makes the DREAMS section exact instead of a lossy client join through `MemoryDreamRun.memory_ids` samples.
- **TODAY** `packages/console-components/src/copy-button.tsx` — boundary-clean copy-as-JSON on body/provenance.

**Interactions.** Row click → Biography via existing `loadMemoryRecordDetail` (`ConsoleApp.tsx:2092`); list context never lost. Lineage node click → that record's Biography in place, back-stack breadcrumb at pane top. "Open transcript range" → dock opens the identity's conversation at the evidence range. DREAMS entry → Dreams view at that run. ⚑ chip → quarantine reason tooltip; click-through to Pipeline if still quarantined. Filter changes re-issue the records RPC; `-32030` tolerance per section unchanged.

### 3.3 Knowledge Lens — what one agent (or the mob) actually sees

**Purpose.** The state-first answer to the injection question: not "what events fired" but "what is in this agent's head right now." Composition strip shows the §7.2 scope union (Identity ∪ Mob ∪ Operator ∪ Realm, `coordinator.rs:109/:350`) as clickable segments; AS-INJECTED shows the exact composed memory block from the last build (the Letta "exactly what the model sees" borrow — the single highest-trust surface); INJECTION HISTORY is the per-identity ledger with **first-class DUP semantics and per-turn/build grouping** (grafts 4 and 9) — precisely the instrumentation that would have caught the historical ~18.5KB/turn duplication defect in minutes.

```
┌─ Knowledge · [identity: ada ▾]  (or [mob: research ▾]) ───────────┐
│ COMPOSITION identity(41) ∪ mob:research(23) ∪ operator(6) ∪       │
│             realm(9)                                              │
│ BUDGET build block ▓▓▓▓▓░░ 9.1KB/20KB · turn: off (pre-0.7.18)    │
│───────────────────────────────────────────────────────────────────│
│ AS-INJECTED (last build · 2h ago)     │ INJECTION HISTORY (ada)   │
│ ┌ index ─────────────────────────┐    │ ▾ 14:02 build 12 rec 5.3KB│
│ │ [identity] prefers dark mode   │    │ ▾ 13:10 turn   3 rec 1.1KB│
│ │ [mob] api uses keyset paging   │    │ ▾ 12:44 build 12 rec 5.3KB│
│ │ [realm] no friday deploys      │    │   ⚠ same 12 ids as 14:02  │
│ └────────────────────────────────┘    │   overlap 12/12           │
│ each line → opens Record Biography    │ 11:58 turn rec_7f3a DUP×2 │
│                                       │   → click filters to rows │
└───────────────────────────────────────────────────────────────────┘
```

**Data sources.**
- **TODAY** `mobkit/memory/panel/records` — composition counts: one call per scope section, tolerating `-32030` per section (`experience.memory.can_read` is deliberately coarse).
- **NEW** RPC `mobkit/memory/panel/injections` — realm-wide injection ledger over `injection_log()` (`sqlite_store.rs:994`, already consumed by the steward's usage audit at `steward.rs:~1450`) with `identity`/`session_key`/`surface` filters. Rows carry `record_id`/`identity`/`session_key`/`surface(build|turn)`/`at_ms`. Durable — the dedup proof survives restarts. Nonce and `stage_token` never appear. DUP badge = client-side group by `(record_id, identity, session_key)` with count > 1; consecutive-build overlap annotation = client-side id-set comparison.
- **NEW** RPC `mobkit/memory/panel/context` — last-rendered composed injection block per identity from the `RecallCoordinator` (`assemble_build_injection`, `coordinator.rs:753`, already produces block + provenance labels), plus `injected_bytes` vs `MAX_INJECTED_ASSEMBLY_BYTES` (20KB) for the gauge. Snapshot semantics: coordinator state is in-memory and resets on compaction (`on_session_compacted`, `:380`) — the gauge is labeled "this session." Per-turn section honestly reads "off until echo-safe delivery (0.7.18 ask 1)" rather than faking data.

**Interactions.** Identity/mob selector swaps the lens; mob mode shows mob+realm scopes plus a member table (name · identity-scope count · last injection) from per-member records calls. Composition segment → Records filtered to that scope. AS-INJECTED line → Record Biography (provenance label carries the record id). ⚠ DUP row → expands the falsifying ledger rows side by side. Gauge > 80% → warning tone; click → injection history filtered to this session.

### 3.4 Pipeline — knowledge in transit

**Purpose.** State that hasn't finished becoming knowledge, as a left-to-right conveyor with the REASON rendered at the decision point (the compliance-UX finding: a bare queue divorced from rationale destroys operator trust). Deliberately read-only — every actionable row deep-links into GatingInboxPanel rather than growing a parallel decision surface.

```
┌─ Pipeline ────────────────────────────────────────────────────────┐
│ PROPOSED(4) ─▶ HELD(1) ─▶ PENDING GATE(2) ─▶ COMMITTED · QUAR(3)  │
│───────────────────────────────────────────────────────────────────│
│ ⚠ prop_31 "auth token rotates hourly"   author distiller d_51     │
│    taint@propose: untrusted-mcp (sess_12) → HELD                  │
│ ⧖ pend_09 rec_aa21 → mob:research  "seen 3× across members"       │
│    waiting 41m · → decide in Gating inbox                         │
│ ✓ pend_04 rec_91bc → mob:research  approved · latency 12m         │
│───────────────────────────────────────────────────────────────────│
│ QUARANTINE (reviewer-only)                                        │
│ 🛑 rec_aa21 · reason: tainted evidence (mcp source)               │
│    evidence sess_12 gen2 [8–9] → open transcript                  │
│    verdict rides the steward dream + gating flow — not here       │
└───────────────────────────────────────────────────────────────────┘
```

**Data sources.**
- **NEW** RPC `mobkit/memory/panel/proposals` — `pending_proposals()` (`sqlite_store.rs:1439`): `proposal_id`, scope, record JSON, author, status `pending|held|accepted|rejected`, `created_at_ms`, and the **propose-time taint column** (captured durably at `sqlite_store.rs:96-99` precisely because tracker state is in-memory). Row visibility mirrors `memory_panel_promotion_visible` (unknown scope kinds denied).
- **TODAY** `mobkit/memory/panel/quarantine` — quarantined records + `pending_promotions`; `stage_token` stays server-side.
- **NEW FIELDS** on `panel/quarantine` — `resolved_at_ms` + resolved `pending_promotions` rows (column exists, `sqlite_store.rs:157`; update path at `:1841`) so the ✓ lane shows gate-decision latency and history, not just the open queue.
- **TODAY** `mobkit/gating/pending` + `mobkit/gating/audit` (`ACTION_GATING_VIEW`) — `pending_id` → gating entry join for the deep link and the decider identity.
- **TODAY** SSE `memory.promotion.pending_gate` / `memory.record.promoted` / `memory.quarantine.verdict` / `memory.quarantine.release_blocked` / `memory.write.quarantined` — refresh triggers only; any inline echo goes through `describeMemoryTimelineEvent`.

**Interactions.** Stage header → filter to that stage. "→ decide in Gating inbox" → `dock.openTarget` on the GatingInboxPanel target at that pending entry; the decision itself uses `gating.decide`, untouched. Quarantined record → Biography with reason + ⚑ chip; "open transcript" targets the tainted evidence window — the operator sees the poisoned turn itself. ✓ row → decision, decider, latency. Quarantine section renders only under `memory.quarantine.review`.

### 3.5 Dreams — the ledger of state mutations

**Purpose.** Dreams framed state-first: every mutation the steward committed against the store, plus its verdict sheet. Collapsible run cards (progressive disclosure: collapse repetitive `set_rank` ops, always-expand and red-flag quarantined ops); every touched `memory_id` links into a Record Biography, closing the node→run→node loop.

```
┌─ Dreams · realm: homecore ────────────────────────────────────────┐
│ ▾ run_88  today 04:00 · 14 ops · ⚠1 quarantined                   │
│    ├ orient       ok                                              │
│    ├ gather       31 candidates                                   │
│    ├ consolidate  2 supersede · 1 contradiction emitted           │
│    └ prune        9 set_rank (collapsed) · 1 tombstone            │
│    verdicts props 3✓ 1✗ 1⏸ 2⛩ · quar 1↩ 1† · usage 4 load-bearing │
│    touched rec_7f3a rec_91bc rec_0b77 +9 more                     │
│ ▸ run_84  yest 22:00 · skipped: min_signals (2 < 3)               │
│ ▸ run_83  yest 16:00 · 6 ops                                      │
└───────────────────────────────────────────────────────────────────┘
```

**Data sources.**
- **TODAY** `mobkit/memory/panel/dreams` — `run_id`, timestamps, `op_kinds` histogram, `quarantined_ops`, `memory_ids` (≤12 sample), `rationales` (≤6 sample), reconstructed from audit rows (`sqlite_store.rs:1888`, 5000-row scan cap): enough for the collapsed card + badges today.
- **NEW PERSISTENCE + FIELD:** persist `DreamRun` detail — phases, the 13 `DreamVerdicts` counters, skips (`steward.rs:814`/`:793`/`:850`) — as one JSON audit/`dream_runs` row at commit time, surfaced on `panel/dreams`. Today this detail exists **only** in the ephemeral `memory.dream.completed` event and dies with the ring. The single biggest steward-observability gap; the phase tree and verdict line depend on it. Until it lands, cards render `op_kinds` only and say so; skip rows appear live-only.
- **TODAY** SSE `memory.dream.started/completed/skipped` — live "dreaming now…" pulse; refresh on completed. Verdicts caught live are cached client-side and labeled cached-vs-durable.
- **TODAY** `describeMemoryTimelineEvent` (both copies) for any inline event text.

**Interactions.** Run header → expand/collapse phase tree (virtualized list — must not break at 50 runs, per the timeline-scalability plan). Touched `memory_id` → Records with that Biography open (reuses `loadMemoryRecordDetail` verbatim — a flagged zero-backend win). Supersede op → LINEAGE lane of the surviving record. Skip reason → tooltip naming the gate (`min_signals`, `runs_per_day`) linking to the Health strip. SignalsRail memory signals gain `onSelect` → dock opens the Memory panel at the relevant view (the rail already carries record ids for `record.promoted`/`quarantine.verdict`).

### 3.6 Verify — the claims board (grafted, mandatory)

**Purpose.** Turns verification from an operator procedure into ambient proof: six claim tiles, each a continuously-evaluated verdict — `HOLDING / DEGRADED / VIOLATED / UNVERIFIABLE` — with the one number that proves or falsifies it, and a drill-down terminating at the exact rows that would make it red. `UNVERIFIABLE` names its missing surface ("needs panel/overview RPC") instead of faking green. Full proof procedures per tile in §5.

```
┌─ Verify · realm: homecore ────────────────────────────────────────┐
│ live window: since 14:02 (312 memory.* events, ring-bounded)      │
│ ── ring history starts here ──          durable checks: sqlite    │
│ ┌ ECHO-SAFETY ─┐ ┌ TAINT WALL ──┐ ┌ LATTICE ─────┐ ┌ RECALL ────┐ │
│ │ HOLDING      │ │ HOLDING      │ │ HOLDING      │ │ DEGRADED   │ │
│ │ 0 dup-inject │ │ 4 quarantined│ │ 0 violations │ │ 61 records │ │
│ │ 2 budget.den │ │ 0 laundered  │ │ checked      │ │ dead weight│ │
│ │ bytes ▁▂▁▃▁▂ │ │ q/hr ▂▁▄▁▁▂  │ │ 214/214      │ │ (28%)      │ │
│ └──────────────┘ └──────────────┘ └──────────────┘ └────────────┘ │
│ ┌ DREAMS ──────────────┐ ┌ STORE FLOOR ───────────┐               │
│ │ UNVERIFIABLE after   │ │ 5% floor               │               │
│ │ restart — needs      │ │ 214/4000 rec 3.1/32MB  │               │
│ │ DreamRun persistence │ │ per scope ▷            │               │
│ └──────────────────────┘ └────────────────────────┘               │
└───────────────────────────────────────────────────────────────────┘
```

**Data sources.**
- **TODAY** `memory.*` SSE frames (client-side aggregation ring, honestly banner-labeled as ring-bounded) — sparklines and live counters.
- **TODAY** `mobkit/memory/panel/records` paged walk (single-realm keyset cursor) — LATTICE invariants (client-side over `trust` + `provenance.author` + `derived_from` + `supersedes` on every row) and RECALL utility ratios (`UsageStats` already serializes on every row).
- **NEW** `panel/overview` → STORE FLOOR; **NEW** `panel/injections` → ECHO-SAFETY dup census; **NEW** `ever_quarantined` field → the taint-cap invariant; **NEW** DreamRun persistence → durable DREAMS verdicts. Each tile's verdict downgrades to `UNVERIFIABLE` with the surface named until its dependency lands.

**Interactions.** Tile → drill-down inside the tab (inline back-button, same DetailView idiom). Lattice drill-down shows proof lines with `checked N/M` partial-progress honesty; violation rows link to the offending record's LINEAGE lane. Recall drill-down ranks by `judged_useful/injected`, shows the dead-weight census with bytes-spent (approximated as `injected_count × body_bytes` until the ledger RPC lands, approximation labeled), and offers **pin-dead-weight-ids → diff against the next dream's ops** — the operator literally watches the demotion claim verify itself.

### 3.7 Health strip — the in-memory truth (taint · budgets · cursors)

**Purpose.** A slim always-honest strip (footer of Holdings, expandable) for state that is in-memory *by design* and therefore invisible after the fact. Answers the three questions currently answerable only by catching transition events live in the lossy ring: why was this run skipped, why did this write quarantine, why did nothing distill.

```
┌─ Health (live snapshot · not durable · resets on restart) ────────┐
│ TAINT   ada: clean · bob: ⚠ tainted (mcp, 14:02, session-sticky)  │
│ BUDGET  distiller 7/12 hr · steward 1/4 day · hygienist 0/2 day   │
│ CURSORS ada sess_9: distilled→gen3 · 2 interactions pending       │
│ HARVEST queue: bob (retired 3h, cause: retire) awaiting dream     │
└───────────────────────────────────────────────────────────────────┘
```

**Data sources.**
- **NEW** RPC `mobkit/memory/panel/health` — snapshot serialization of: `SessionTaintTracker` maps (`current_session`/`tainted`/`pending_identity_taint`/`reset_boundaries`, `taint.rs:209-221`), `BackgroundBudget` used/cap per realm-stage (`guards.rs:61`), distiller `WindowState` cursors (`distiller.rs:980`), steward signal counter. Explicitly labeled "live snapshot — resets on restart"; durable re-derivation is impossible by design.
- **NEW** `pending_harvests()` read (`sqlite_store.rs:1653`) — as a field on `/health` or its own RPC: which retired identities await exit-interview, cause, age. Currently invisible until `memory.harvest.completed` fires — exactly when it stops mattering.
- **TODAY** SSE `memory.taint.transition` / `memory.budget.denied` / `memory.distill.timed_out` — flip the strip live between snapshots.

**Interactions.** Tainted identity → explanation card (source, when, session-sticky rule, evidence-gate retention across rotation) linking to that identity's held proposals in Pipeline. Budget meter at cap → last `budget.denied` reason (`WindowExhausted{used,cap}` vs `ConcurrencyCeiling`) if still in the ring, else "window resets at T" — with the ring hairline. Harvest row → the retired member's identity-scope records in Records.

### 3.8 Loop Trace — lanes (later phase, grafted from Flight Recorder)

**Purpose.** Repairs the Ledger's weakest criterion: the loop in motion at mob/realm scale. Per-identity lanes plus system lanes (steward, hygienist, mob-scope), time on the horizontal axis, with by-agent/by-mob lane grouping — mob mode collapses member lanes under a mob header and puts mob-scope events (`record.promoted`, `promotion.pending_gate`) on the header lane, so "the mob's shared memory converging" reads as marks flowing from member lanes up and back down into every member's injection marks.

**Ships live-only first** over the existing SSE frames (all 17 subtypes, formatted via `describeMemoryTimelineEvent`, identity attribution via `MemoryTimelineEvent::identity()`), with the ring hairline and pause-on-scroll banner. Upgraded to scrubbable durable history **only if/when** the composite trace RPC (`panel/trace`: time-ordered merge of audit + injections + proposals + pending_promotions + pending_harvests, single-realm keyset cursor, per-row ABAC) proves justified by usage. Every mark's detail rail ends in "state here" pivots into the other views — events always terminate in durable state, never in a payload dump. The live view is materially better once the upstream `memory.distill.completed` event exists; without it the loop's most frequent hop is invisible unless it fails (`DistillOutcome::Completed` reaches logs/tests only, `distiller.rs:997`).

---

## 4. The follow-a-fact walkthrough

Luka tells agent **ada**: "we use keyset pagination, cursors are ms:id."

1. **Turn.** Nothing appears yet — correct behavior made legible: the **Health strip** shows ada's distiller cursor at `sess_9: 2 interactions pending` (min_interactions=3), so "why did nothing distill" has an answer instead of a mystery.
2. **Distiller.** After the third interaction the distiller fires. A new row `rec_7f3a` appears in **Records** (live, because the `memory.*` SSE trigger re-runs `refreshMemoryData` while the panel is docked — today only failures would signal this hop; the `memory.distill.completed` event upgrades it to a real-time pulse). Its **Biography** reads: BORN distiller run d_42, evidence sess_9 gen3 [12–19], trust `agent_observed` — the LLM-ceiling chip itself teaches the lattice. Clicking the evidence opens the transcript at the exact turn Luka said it: the fact's birth certificate.
3. **Record → Selection.** Over the next sessions, the **Verify → Recall** drill-down shows `rec_7f3a`'s `UsageStats` climbing — selected, not dead weight.
4. **Injection.** On ada's next build, the **Knowledge Lens** AS-INJECTED block contains `[identity] api uses keyset pagination`; the budget gauge ticks up ~1.2KB toward 20KB; the Biography LIFE section shows `inj×1, surface=build` with the ledger row timestamped. The INJECTION HISTORY's DUP column stays empty — echo-safety's duplication half, verified ambiently on every glance.
5. **Dream.** At 04:00 the **Dreams** tab pulses "dreaming…", then run_88's card lands: consolidate shows 1 supersede — the steward merged `rec_7f3a` with an older, vaguer `rec_51e0`.
6. **Supersede.** Back in the **Biography**, the LINEAGE lane now shows `rec_7f3a ●` current above `rec_51e0 ○` dimmed `ᴰ88`, and DREAMS lists run_88. The fact completed the full loop — turn → distiller → record → selection → injection → dream → supersede — with the process arriving as history attached to state, never as a feed to correlate.

**The same loop at mob scale.** Three research-mob members independently learn the fact; **Holdings** shows identity-scope counts rising. The next dream's gather phase spots the repetition and stages a promotion: **Pipeline** shows `pend_09 rec_aa21 → mob:research, "seen 3× across members"` in PENDING GATE, and the SignalsRail's `memory.promotion.pending_gate` signal `onSelect` opens that exact row. The operator reads the rationale plus source records (each one click from its evidence), decides via the normal Gating inbox, `memory.record.promoted {gated:true}` fires, Pipeline's ✓ lane records the 12-minute gate latency, Holdings' mob count increments — and every member's **Knowledge Lens** now carries the fact under `[mob]`. In the later-phase **Loop Trace**, this whole passage is one visual: marks flowing from member lanes to the mob header lane and back down into each member's next injection mark.

---

## 5. Verification widgets

Each widget names the architecture claim it proves and the mechanism. Every check is read-only, per-row ABAC'd, and every green verdict is one click from the rows that would have made it red.

| Widget | Claim proved | How |
|---|---|---|
| **Knowledge Lens DUP column + Verify ECHO-SAFETY tile** | Injection dedup holds; the ~18.5KB/turn duplication class stays dead | Group durable ledger rows by `(record_id, identity, session_key)`; any count > 1 in a session is a red DUP badge — the exact fingerprint of the historical defect, as a standing regression tripwire. Consecutive-build overlap annotation (`⚠ same 12 ids, overlap 12/12`) makes budget pressure read per turn. Survives restarts (sqlite). |
| **Biography evidence click-through + author filter** | Echo-safety's harvest half: the distiller never re-harvests injected memory | Confirm BORN evidence points at a real transcript range authored by a human/tool turn; adversarial probe: after repeated injections, filter Records to `author=distiller` created since, and verify no new record's evidence window overlaps an injected block — a two-pane visual comparison. |
| **Verify TAINT WALL tile + Pipeline taint@propose + ⚑ chip** | Tainted content cannot become trusted memory, at three durable seams | (a) proposals carry propose-time taint (immune to tracker restarts); (b) quarantined records carry their reason; (c) `ever_quarantined` permanently marks anything ever caught, trust ceilinged even after release. Live pairing `taint.transition → write.quarantined` shows the wall catching in real time; session rotation must show a `reset_boundary` while the Health strip keeps the taint (evidence-gate retention). |
| **Verify LATTICE audit (exhaustive)** | Trust lattice intact: LLM ceiling, taint cap, acyclic chains | Keyset page-walk of every active record in the realm asserting: no `agent`/`steward`/`distiller`-authored record above `agent_observed`; no `ever_quarantined` record above its cap; supersede chains acyclic with no dangling references; plus the laundering invariant `trust ≤ min(derived_from parents)` — the calibration suite's `transitive_laundering_rejected` fixture promoted from CI assertion to production monitor. Header states `checked N/M` so partial walks never masquerade as proofs; violations link to the record's LINEAGE lane. |
| **Knowledge Lens budget gauge + Health budget meters + Dreams skip rows** | Budgets enforce | Gauge must stay ≤ 20KB and reconcile arithmetically with the sum of ledger rows per session; forcing a burst pins the Health meter at cap while `memory.budget.denied` lands with `WindowExhausted{used,cap}`, and the corresponding SKIPPED dream card names the same reason — denial and absence cross-check each other. |
| **Verify RECALL drill-down** | Recall earns its bytes; dreams demote dead weight | Utility ratio ranking over `UsageStats`, dead-weight census with bytes-spent; pin dead-weight ids and diff against the next dream's ops — watch the demotion claim verify itself. |
| **Pipeline ✓ lane × gating audit** | Gate integrity | Every `gated:true` promotion must have a matching gating decision with a decider; a promoted record with no gate entry renders as an orphan row. `stage_token`/nonce never appear in any response, by contract. |
| **Principal-swap (procedure, documented in the panel help)** | ABAC scoping | Without `memory.quarantine.review`: the Quarantine section and quarantined rows vanish (per-row, not just the tab). Without `operator.memory.read`: operator-scope rows absent while Holdings' operator row shows access-denied tone — unscoped `agent.memory.read` provably does not leak operator scope. |
| **UNVERIFIABLE verdicts everywhere** | The dashboard itself stays honest | A verification surface that silently degrades to vibes is worse than none: tiles missing a surface name it; ring-backed data carries the hairline; the Health strip says "resets on restart." |

---

## 6. New surfaces needed, ranked by cost

Every RPC/field below pays the CI-enforced process tax: `CONSOLE_RPC_METHODS` constant + contract-JSON entry (request/success/error shapes) + `http_console.rs` dispatch arm visible to the `contract.test.ts` parser (bijection both ways) + `console_rpc_access_requirements` entry gate + per-row filtering + embedded-bundle rebuild.

**Tier 0 — client-only (no new server surface):**
1. Live refresh trigger: `frame.event.startsWith('memory.')` → `refreshMemoryData` in `handleLiveFrame` (`ConsoleApp.tsx:2338`) while a memory panel is docked.
2. Filter bar + load-more wiring of the records RPC's existing `scope`/`identity`/`scope_key`/`status`/`limit`/`cursor` params.
3. Dreams `memory_ids[]` → Biography links (data already returned; render-layer only).
4. SignalsRail memory-signal `onSelect` → `dock.openTarget('memory')` + `loadMemoryRecordDetail`.
5. Evidence click-through via existing `query_timeline` with client-side `session_id` matching (§2.1 caveat); CopyButton in DetailView; Lattice/Recall client-side checks over the records walk.

**Tier 1 — thin wrappers over already-built store readers (near-zero server cost):**
6. RPC `mobkit/memory/panel/overview` — `scope_overview()` + `scope_floors()` (`sqlite_store.rs:1379`/`:1373`; doc-comment already names the panel).
7. RPC `mobkit/memory/panel/proposals` — `pending_proposals()` (`:1439`) incl. propose-time taint; visibility mirrors `memory_panel_promotion_visible`.
8. RPC `mobkit/memory/panel/injections` — `injection_log()` (`:994`) + `identity`/`session_key`/`surface` filters; two-layer ABAC; never carries nonce/stage_token.
9. `pending_harvests()` read (`:1653`) — own RPC or a `/health` field.

**Tier 2 — field additions to existing responses (small):**
10. `panel/record`: `ever_quarantined` + `rank_set_at_ms` (columns at `:82`/`:72`; needs wire-model plumbing, not new queries) and a `history[]` array from per-record audit rows (`:102`).
11. `panel/quarantine`: `resolved_at_ms` + resolved `pending_promotions` rows (column at `:157`).
12. `query_timeline`: optional `session_id` request param (only if Tier-0 client matching proves lossy).

**Tier 3 — new snapshot serialization (moderate):**
13. RPC `mobkit/memory/panel/health` — `SessionTaintTracker` maps (`taint.rs:209-221`), `BackgroundBudget` used/cap (`guards.rs`), distiller `WindowState` cursors (`distiller.rs:980`), steward signal counter; labeled restart-volatile.
14. RPC `mobkit/memory/panel/context` — last-rendered injection block per identity from `RecallCoordinator` (`assemble_build_injection`, `coordinator.rs:753`) + `injected_bytes`/cap; nonce stripped; per-turn section honestly absent pre-0.7.18.

**Tier 4 — new persistence / upstream (largest, each independently deferrable):**
15. Persist `DreamRun` detail (phases, 13 `DreamVerdicts` counters, skips — `steward.rs:814`/`:793`) as one audit/`dream_runs` row at commit time, surfaced on `panel/dreams`. The single biggest steward-observability gap.
16. Upstream event `memory.distill.completed` (`DistillOutcome::Completed`, `distiller.rs:997`, currently logs/tests only) — required for the "record just born" live pulse; must land in **both** `describeMemoryTimelineEvent` copies + `adapters.test.ts` + SignalsRail classification. Candidate for the upstream-ask channel.
17. RPC `mobkit/memory/panel/trace` — composite durable event query for scrubbable Loop Trace history. Built **only if** the live-only lane view earns it.
18. (Optional) durable calibration scorecard artifact — `scripts/memory-evals` prints to stdout only; a JSON artifact would let record detail render calibration status beside the `CalibrationRef` each record already carries.

---

## 7. Phased implementation plan

**Phase 1 — existing data only; standalone value.** Tier 0 items 1–5. Delivers: live-refreshing panel with pause-on-scroll banners, scope/status/kind/trust filtering + load-more, Record Biography with LINEAGE lane and per-record injection LIFE (from the existing `panel/record` response), dream→record cross-navigation, rail→panel deep links, evidence click-through (best-effort, honest degradation), and the Verify tab's day-one-green checks: Lattice invariants (LLM ceiling, acyclic chains, laundering), Recall Utility + dead-weight census, and per-record dup detection — all client-side over data already flowing. Ships without touching Rust except the embedded-bundle rebuild.

**Phase 2 — the thin wrappers.** Tier 1 + Tier 2. Unlocks: Holdings (overview RPC), Pipeline (proposals + resolved promotions + gate latency), realm-wide INJECTION HISTORY with first-class DUP proofs (injections RPC), the ⚑ ever-quarantined chip and exact per-record DREAMS history, harvest queue. Verify tiles ECHO-SAFETY, TAINT WALL, STORE FLOOR flip from `UNVERIFIABLE` to evaluated.

**Phase 3 — snapshots.** Tier 3. Unlocks: the Health strip (taint/budgets/cursors) and the Knowledge Lens AS-INJECTED block + budget gauge. Decide here whether these snapshots deserve a distinct projected affordance (open question 3).

**Phase 4 — persistence and upstream.** Tier 4, each landing independently: DreamRun persistence completes Dreams and makes the DREAMS Verify tile durable; `memory.distill.completed` completes the live loop; the Loop Trace ships live-only and is upgraded to durable scrubbing only if the composite trace RPC is justified by observed use.

Each phase degrades honestly: any view whose surface hasn't landed renders its labeled partial state, never fake completeness.

---

## 8. Open questions

1. **DreamRun persistence locus.** One JSON blob in the existing audit table vs. a `dream_runs` table — and does any part belong meerkat-side (upstream-ask channel) vs. mobkit-side, given the 0.7.18 checklist deletes the steward loop?
2. **Evidence → transcript mapping.** Is client-side `session_id` matching over identity-scoped `query_timeline` results reliable enough across compaction/hygiene revisions (evidence refs are revision-pinned), or do we take the `session_id` param (Tier 2, item 12) immediately?
3. **Affordance granularity for snapshots.** Do `panel/health` and `panel/context` ride the existing coarse `experience.memory.can_read`, or do runtime snapshots (taint state, composed injection blocks) deserve a distinct projected affordance (e.g. `can_read_runtime`)? The composed block reveals cross-scope content the principal can otherwise only see row-by-row under per-scope grants — the context RPC likely needs to filter its sections by the same per-scope read actions.
4. **Multi-realm Verify.** Tiles are per-realm by paging constraint; is a one-row-per-realm board acceptable, or does the realm selector suffice for HomeCore-scale deployments?
5. **Quarantine-release calibration feedback.** The "[false-positive? feed calibration]" affordance is text-only; where would an operator's release-was-right/wrong signal actually land (memory-evals fixtures? taint tracker tuning?), and is it worth a surface?
6. **Loop Trace go/no-go criterion.** What usage signal from the live-only lane view justifies building the composite `panel/trace` RPC (the heaviest surface on the list)?
7. **Timeline scalability governance.** Dreams and Trace must adopt `docs/design/console-timeline-scalability-plan.md` virtualization from day one — does that plan need extending for horizontal lane rendering before Phase 4?

---

## Appendix A — Judge scores (concept bake-off)

| Concept | C1 understand+verify | C2 loop in motion | C3 console/data fit | C4 shippability | Total |
|---|---|---|---|---|---|
| **The Ledger of What It Knows** (winner) | 8 | 6.5 | 9.5 | 9 | **33** |
| The Claims Board | 8.5 | 7 | 8 | 8 | 31.5 |
| Memory Flight Recorder | 7 | 9.5 | 6 | 6 | 28.5 |

The Ledger won on console/data fit (direct maturation of the existing MemoryPanel idiom; new RPCs are thin wrappers over built store readers) and shippability (phase 1 is almost entirely client-only). Its two weaknesses — procedural rather than ambient verification, and no realm-wide loop-in-motion view — are repaired by the mandatory Verify-tab graft from the Claims Board and the later-phase Loop Trace graft from the Flight Recorder, both mounted over the same data layer.
