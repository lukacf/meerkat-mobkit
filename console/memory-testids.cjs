// Selector contract between the memory console UI (UI-P1.B) and the
// memory e2e (memory-e2e.cjs). The panel's data-testids MUST match this
// inventory — the e2e flows locate every element through it and nothing
// else. Names extend the scheme MemoryPanel.tsx already ships (memory-*
// prefix, `:`-separated dynamic segments).
//
// UPDATED by the UI-P1.B panel agent to the names MemoryPanel.tsx actually
// ships (this file is the single source of truth — flows that used the
// earlier placeholder names need the same rename).

module.exports = {
  // ── Navigation (Sidebar.tsx renders `nav:${kind}`) ──
  // Rendered only when /console/experience reports memory.can_read.
  NAV_MEMORY: "nav:memory",

  // ── Panel chrome ──
  PANEL: "memory-panel",
  UNAVAILABLE: "memory-unavailable", // store not configured on the runtime
  ERROR: "memory-error",
  REFRESH: "memory-refresh",

  // ── Tabs ──
  // `memory-tab:${id}` where id ∈ holdings | records | knowledge |
  // pipeline | dreams. All five render for every principal; access shows up
  // per-section inside a tab ("no grant" notes), not by hiding tabs.
  // `memory-tab:quarantine` still exists as a ONE-RELEASE redirect alias
  // (open Q6): an invisible button (opacity 0, 1×1) that switches to
  // Pipeline. It keeps a bounding box so driver clicks land.
  tab: (id) => `memory-tab:${id}`,

  // ── Records rows ──
  // `memory-group:${scopeGroupKey}` e.g. memory-group:identity:default:router
  // (grouped view renders only when no filter is active and sort=recency).
  // `memory-record:${memoryId}` one row per record, body-free list row.
  group: (scopeKey) => `memory-group:${scopeKey}`,
  record: (memoryId) => `memory-record:${memoryId}`,

  // ── Filter bar + paging (Records tab) ──
  FILTER: "memory-filter", // container
  FILTER_INPUT: "memory-filter-input", // <input> identity / scope key (Enter or blur applies)
  FILTER_SCOPE: "memory-filter:scope", // <select> all|identity|mob|operator|realm
  FILTER_STATUS: "memory-filter:status", // <select> all|active|quarantined|superseded|tombstoned
  FILTER_CLEAR: "memory-filter-clear", // renders only while a filter is active
  SORT: "memory-sort", // <select> recency|utility (utility = Recall Utility mode)
  UTILITY_NOTE: "memory-utility-note", // approximation disclaimer, shows in utility mode
  LOAD_MORE: "memory-load-more", // renders only while a keyset cursor exists (single-realm)

  // ── Record detail / Biography ──
  DETAIL: "memory-detail", // the Biography container (kept from P3b)
  DETAIL_BACK: "memory-detail-back",
  DETAIL_BODY: "memory-detail-body",
  // Biography sections:
  DETAIL_BORN: "memory-detail-born", // author + evidence refs
  DETAIL_LINEAGE: "memory-detail-lineage", // supersede lane, newest-first
  DETAIL_LIFE: "memory-detail-life", // usage counters + injection ledger
  DETAIL_DREAMS: "memory-detail-dreams", // lossy memory_ids join
  // `memory-chain:${memoryId}` — lineage lane rows (now buttons; clicking a
  // non-current node loads that Biography in place; data-current / data-dimmed).
  chainEntry: (memoryId) => `memory-chain:${memoryId}`,
  // Evidence click-through: `memory-evidence:${index}` buttons per ref;
  // result renders memory-evidence-excerpt (transcript window) or
  // memory-evidence-degraded (session gone — label-only fallback).
  evidenceRef: (index) => `memory-evidence:${index}`,
  EVIDENCE_EXCERPT: "memory-evidence-excerpt",
  EVIDENCE_DEGRADED: "memory-evidence-degraded",

  // ── Holdings tab ──
  HOLDINGS: "memory-holdings",
  HOLDINGS_DENIED: "memory-holdings-denied", // records -32030 → "no grant" note
  // `memory-holdings-scope:${scopeGroupKey}` — scope rows; click pivots to
  // Records pre-filtered to that scope.
  holdingsScope: (scopeKey) => `memory-holdings-scope:${scopeKey}`,

  // ── Verdict strip (Holdings header) ──
  VERDICT_STRIP: "memory-verdict-strip",
  // `memory-verdict:${id}` — id is the STABLE tile id:
  //   echo-safety | taint-wall | lattice | recall | dreams | store-floor.
  // The verdict state rides the `data-status` attribute:
  //   holding | degraded | violated | unverifiable | no-grant.
  // Tiles are doors: clicking opens the tab holding the evidence.
  verdictTile: (id) => `memory-verdict:${id}`,

  // ── Pipeline tab (quarantine folded in) ──
  PIPELINE: "memory-pipeline",
  PIPELINE_STAGES: "memory-pipeline-stages", // PROPOSED ─▶ PENDING GATE ─▶ COMMITTED · QUAR strip
  PIPELINE_PROPOSALS: "memory-pipeline-proposals", // honest "needs surface 4" note
  PIPELINE_NO_GRANT: "memory-pipeline-no-grant", // shown without memory.quarantine.review
  QUARANTINE_NOTE: "memory-quarantine-note", // read-only disclaimer (kept from P3b)
  quarantineRecord: (memoryId) => `memory-quarantine-record:${memoryId}`, // now a button → Biography
  pendingPromotion: (pendingId) => `memory-pending:${pendingId}`,
  // `memory-pipeline-decide:${pendingId}` — deep link into the Gating inbox.
  pipelineDecide: (pendingId) => `memory-pipeline-decide:${pendingId}`,

  // ── Live memory-event strip (bottom of Pipeline tab) ──
  LIVE_STRIP: "memory-live-strip",
  liveRow: (frameId) => `memory-live-row:${frameId}`,
  // "state here" pivot per live row (renders when the payload names a record).
  livePivot: (frameId) => `memory-live-pivot:${frameId}`,
  LIVE_JUMP: "memory-live-jump", // "N behind · jump to live" (renders while paused)
  LIVE_SEAM: "memory-live-seam", // "ring history starts here" marker

  // ── Knowledge tab ──
  KNOWLEDGE: "memory-knowledge",
  KNOWLEDGE_IDENTITY: "memory-knowledge-identity", // identity <select>
  // `memory-knowledge-segment:${label}` — composition segments; click pivots
  // to Records filtered to that scope.
  knowledgeSegment: (label) => `memory-knowledge-segment:${label}`,
  KNOWLEDGE_AS_INJECTED: "memory-knowledge-as-injected", // honest surface-10 placeholder
  KNOWLEDGE_HISTORY: "memory-knowledge-history", // honest surface-6/8 placeholder

  // ── Dreams tab ──
  dream: (runId) => `memory-dream:${runId}`,
  // `memory-dream-record:${runId}:${memoryId}` — touched-record links → Biography.
  dreamRecord: (runId, memoryId) => `memory-dream-record:${runId}:${memoryId}`,

  // ── Signals rail (SignalsRail.tsx) ──
  SIGNALS_RAIL: "signals-rail",
  signal: (signalId) => `signal:${signalId}`, // memory.* frames map to signals
  signalsFilter: (presetId) => `signals-filter:${presetId}`,
  // "State here" pivot inside single-item memory signal rows: opens the
  // Memory panel and loads the record Biography when the frame payload
  // names a record_id. (Clicking the row body does the same via onSelect;
  // multi-item groups expand first, and their per-event buttons fire
  // onSelect per item.)
  SIGNAL_MEMORY_PIVOT: "signal-memory-pivot",
};
