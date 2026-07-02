import React from "react";
import type {
  MemoryAuthor,
  MemoryDreamRun,
  MemoryEvidenceRef,
  MemoryFullRecord,
  MemoryInjectionEntry,
  MemoryPanelRecord,
  MemoryPendingPromotion,
  MemoryRecordScope,
  MemoryRecordStatus,
  MemoryTrust,
} from "../types";

export interface MemoryRecordDetail {
  realm: string;
  record: MemoryFullRecord;
  chain: MemoryPanelRecord[];
  injections: MemoryInjectionEntry[];
}

export interface MemoryPanelProps {
  records: MemoryPanelRecord[];
  realms: string[];
  quarantineRecords: MemoryPanelRecord[];
  pendingPromotions: MemoryPendingPromotion[];
  dreams: MemoryDreamRun[];
  detail: MemoryRecordDetail | null;
  detailLoading?: boolean;
  canReviewQuarantine?: boolean;
  unavailable?: boolean;
  error?: string | null;
  onRefresh: () => void;
  onSelectRecord: (realm: string | undefined, memoryId: string) => void;
  onClearDetail: () => void;
}

type Tab = "records" | "quarantine" | "dreams";

// ── Pure helpers (exported via __memoryTest) ──────────────────────────────

export function realmOfRecord(record: Pick<MemoryPanelRecord, "scope">): string {
  return record.scope.realm;
}

/// Stable grouping key for a record scope. Each identity gets its own group;
/// mob/operator/realm scopes collapse to one group per (realm, kind).
export function scopeGroupKey(scope: MemoryRecordScope): string {
  switch (scope.scope) {
    case "identity":
      return `identity:${scope.realm}:${scope.identity}`;
    case "mob":
      return `mob:${scope.realm}:${scope.mob}`;
    case "operator":
      return `operator:${scope.realm}:${scope.operator}`;
    case "realm":
      return `realm:${scope.realm}`;
  }
}

export function scopeGroupLabel(scope: MemoryRecordScope): string {
  switch (scope.scope) {
    case "identity":
      return scope.identity;
    case "mob":
      return `Mob: ${scope.mob}`;
    case "operator":
      return `Operator: ${scope.operator}`;
    case "realm":
      return "Realm";
  }
}

/// Rank scope groups so identity groups sort first (alphabetically), then mob,
/// operator, and realm. Keeps the records tab visually stable.
function scopeGroupRank(scope: MemoryRecordScope): number {
  switch (scope.scope) {
    case "identity":
      return 0;
    case "mob":
      return 1;
    case "operator":
      return 2;
    case "realm":
      return 3;
  }
}

export interface MemoryScopeGroup {
  key: string;
  label: string;
  scope: MemoryRecordScope;
  records: MemoryPanelRecord[];
}

export function groupRecordsByScope(records: MemoryPanelRecord[]): MemoryScopeGroup[] {
  const byKey = new Map<string, MemoryScopeGroup>();
  for (const record of records) {
    const key = scopeGroupKey(record.scope);
    let group = byKey.get(key);
    if (!group) {
      group = {
        key,
        label: scopeGroupLabel(record.scope),
        scope: record.scope,
        records: [],
      };
      byKey.set(key, group);
    }
    group.records.push(record);
  }
  return Array.from(byKey.values()).sort((a, b) => {
    const rankDelta = scopeGroupRank(a.scope) - scopeGroupRank(b.scope);
    if (rankDelta !== 0) return rankDelta;
    return a.label.localeCompare(b.label);
  });
}

const TRUST_LABEL: Record<MemoryTrust, string> = {
  untrusted: "untrusted",
  agent_observed: "observed",
  agent_verified: "verified",
  application: "application",
  operator: "operator",
};

export function trustLabel(trust: MemoryTrust | undefined): string {
  return (trust && TRUST_LABEL[trust]) || String(trust || "unknown");
}

/// Visual tone bucket for a trust level, reusing the gpolicy state vocabulary.
export function trustTone(trust: MemoryTrust | undefined): "positive" | "neutral" | "muted" {
  switch (trust) {
    case "operator":
    case "application":
    case "agent_verified":
      return "positive";
    case "agent_observed":
      return "neutral";
    default:
      return "muted";
  }
}

export function statusLabel(status: MemoryRecordStatus | undefined): string {
  if (!status) return "unknown";
  switch (status.status) {
    case "active":
      return "active";
    case "superseded":
      return status.by ? `superseded → ${status.by}` : "superseded";
    case "quarantined":
      return status.reason ? `quarantined: ${status.reason}` : "quarantined";
    case "tombstoned":
      return "tombstoned";
  }
}

export function statusTone(
  status: MemoryRecordStatus | undefined,
): "positive" | "warning" | "muted" {
  if (!status) return "muted";
  switch (status.status) {
    case "active":
      return "positive";
    case "quarantined":
      return "warning";
    default:
      return "muted";
  }
}

const RELATIVE_UNITS: Array<[number, string]> = [
  [365 * 24 * 60 * 60 * 1000, "y"],
  [24 * 60 * 60 * 1000, "d"],
  [60 * 60 * 1000, "h"],
  [60 * 1000, "m"],
  [1000, "s"],
];

export function relativeAge(atMs: number | undefined, now = Date.now()): string {
  if (!atMs || atMs <= 0) return "—";
  const diff = now - atMs;
  if (diff < 0) return "now";
  if (diff < 1000) return "now";
  for (const [unitMs, suffix] of RELATIVE_UNITS) {
    if (diff >= unitMs) {
      return `${Math.floor(diff / unitMs)}${suffix} ago`;
    }
  }
  return "now";
}

/// Human label for a single evidence reference. NOTE: there is no transcript
/// fetch yet — this is a label only, never a link to a live transcript.
export function evidenceLabel(evidence: MemoryEvidenceRef): string {
  const parts: string[] = [];
  if (evidence.session_id) parts.push(`session ${evidence.session_id}`);
  if (typeof evidence.generation === "number") parts.push(`gen ${evidence.generation}`);
  if (evidence.revision) parts.push(`rev ${evidence.revision}`);
  if (evidence.range && evidence.range.length === 2) {
    const [start, end] = evidence.range;
    parts.push(`msgs ${start}–${end}`);
  }
  return parts.join(" • ") || "evidence";
}

export function authorLine(author: MemoryAuthor | undefined): string {
  if (!author) return "unknown author";
  switch (author.author) {
    case "agent":
      return author.identity ? `agent ${author.identity}` : "agent";
    case "steward":
      return author.run_id ? `steward run ${author.run_id}` : "steward";
    case "distiller":
      return author.run_id ? `distiller run ${author.run_id}` : "distiller";
    case "operator":
      return "operator";
    case "application":
      return "application";
  }
}

/// Compact one-line summary of a dream run's op mix, e.g. "3 create · 1 tombstone".
export function dreamOpKindsSummary(opKinds: Record<string, number> | undefined): string {
  if (!opKinds) return "";
  const entries = Object.entries(opKinds)
    .filter(([, count]) => typeof count === "number" && count > 0)
    .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
  if (entries.length === 0) return "";
  return entries.map(([kind, count]) => `${count} ${kind}`).join(" · ");
}

export function dreamTimeRange(run: Pick<MemoryDreamRun, "first_op_at_ms" | "last_op_at_ms">, now = Date.now()): string {
  const first = run.first_op_at_ms;
  const last = run.last_op_at_ms;
  if (!first && !last) return "—";
  if (first && last && first !== last) {
    return `${relativeAge(first, now)} → ${relativeAge(last, now)}`;
  }
  return relativeAge(last || first, now);
}

export function injectionLine(injection: MemoryInjectionEntry, now = Date.now()): string {
  const surface = injection.surface === "build" ? "build" : "turn";
  return `${surface} • ${injection.identity} • ${relativeAge(injection.at_ms, now)}`;
}

export const __memoryTest = {
  scopeGroupKey,
  scopeGroupLabel,
  groupRecordsByScope,
  trustLabel,
  trustTone,
  statusLabel,
  statusTone,
  relativeAge,
  evidenceLabel,
  authorLine,
  dreamOpKindsSummary,
  dreamTimeRange,
  injectionLine,
  realmOfRecord,
};

// ── Presentational sub-components ─────────────────────────────────────────

function Chip({ label, tone }: { label: string; tone?: string }): React.JSX.Element {
  return (
    <span className="chip memory-chip" data-tone={tone || "neutral"}>
      {label}
    </span>
  );
}

function RecordRow({
  record,
  onSelect,
}: {
  record: MemoryPanelRecord;
  onSelect: () => void;
}): React.JSX.Element {
  return (
    <button
      type="button"
      className="memory-row"
      data-testid={`memory-record:${record.id}`}
      onClick={onSelect}
    >
      <span className="memory-row__title">{record.title || record.id}</span>
      <span className="memory-row__meta">
        <Chip label={record.kind} />
        <Chip label={trustLabel(record.trust)} tone={trustTone(record.trust)} />
        <Chip label={statusLabel(record.status)} tone={statusTone(record.status)} />
        <span className="memory-row__age">{relativeAge(record.updated_at_ms)}</span>
      </span>
    </button>
  );
}

function DetailView({
  detail,
  onBack,
}: {
  detail: MemoryRecordDetail;
  onBack: () => void;
}): React.JSX.Element {
  const { record, chain, injections } = detail;
  const provenance = record.provenance;
  const evidence = provenance?.evidence || [];
  const verification = provenance?.verification;
  const usage = record.usage;
  return (
    <div className="memory-detail" data-testid="memory-detail">
      <div className="memory-detail__head">
        <button type="button" className="memory-back" onClick={onBack} data-testid="memory-detail-back">
          ← Back
        </button>
        <h3>{record.title || record.id}</h3>
        <span className="memory-detail__chips">
          <Chip label={record.kind} />
          <Chip label={trustLabel(record.trust)} tone={trustTone(record.trust)} />
          <Chip label={statusLabel(record.status)} tone={statusTone(record.status)} />
        </span>
      </div>

      {record.description ? (
        <p className="memory-detail__description">{record.description}</p>
      ) : null}

      <pre className="memory-detail__body" data-testid="memory-detail-body">{record.body}</pre>

      {record.tags && record.tags.length > 0 ? (
        <div className="memory-detail__tags">
          {record.tags.map((tag) => (
            <Chip key={tag} label={tag} tone="muted" />
          ))}
        </div>
      ) : null}

      <div className="memory-detail__section">
        <span className="memory-detail__label">Provenance</span>
        <div className="memory-detail__line">{authorLine(provenance?.author)}</div>
        {evidence.length > 0 ? (
          <div className="memory-evidence">
            {/* Label only — there is no transcript fetch yet. */}
            {evidence.map((ref, index) => (
              <span className="memory-evidence__ref" key={`ev-${index}`}>
                {evidenceLabel(ref)}
              </span>
            ))}
          </div>
        ) : null}
        {verification?.checked ? (
          <div className="memory-detail__line memory-detail__verification">
            verified: {verification.checked}
          </div>
        ) : null}
      </div>

      {usage ? (
        <div className="memory-detail__section">
          <span className="memory-detail__label">Usage</span>
          <div className="memory-detail__line">
            injected {usage.injected_count ?? 0} · recalled {usage.explicit_recall_count ?? 0} ·
            judged useful {usage.judged_useful_count ?? 0}
          </div>
        </div>
      ) : null}

      {injections.length > 0 ? (
        <div className="memory-detail__section">
          <span className="memory-detail__label">Injections</span>
          {injections.map((injection, index) => (
            <div className="memory-detail__line" key={`inj-${index}`}>
              {injectionLine(injection)}
            </div>
          ))}
        </div>
      ) : null}

      {chain.length > 0 ? (
        <div className="memory-detail__section">
          <span className="memory-detail__label">Supersede chain</span>
          <div className="memory-chain">
            {chain.map((entry) => (
              <div
                className="memory-chain__row"
                key={entry.id}
                data-current={entry.id === record.id ? "true" : undefined}
                data-testid={`memory-chain:${entry.id}`}
              >
                <span className="memory-chain__title">{entry.title || entry.id}</span>
                <Chip label={statusLabel(entry.status)} tone={statusTone(entry.status)} />
              </div>
            ))}
          </div>
        </div>
      ) : null}
    </div>
  );
}

export function MemoryPanel({
  records,
  realms,
  quarantineRecords,
  pendingPromotions,
  dreams,
  detail,
  detailLoading = false,
  canReviewQuarantine = false,
  unavailable = false,
  error,
  onRefresh,
  onSelectRecord,
  onClearDetail,
}: MemoryPanelProps): React.JSX.Element {
  const [tab, setTab] = React.useState<Tab>("records");

  const tabs: Tab[] = canReviewQuarantine
    ? ["records", "quarantine", "dreams"]
    : ["records", "dreams"];

  const groups = React.useMemo(() => groupRecordsByScope(records), [records]);

  if (unavailable) {
    return (
      <div className="gating memory-panel" data-testid="memory-panel">
        <div className="gating__head">
          <h2>Memory</h2>
        </div>
        <div className="gating__empty" data-testid="memory-unavailable">
          The memory panel is not configured on this runtime.
        </div>
      </div>
    );
  }

  return (
    <div className="gating memory-panel" data-testid="memory-panel">
      <div className="gating__head">
        <h2>Memory</h2>
        <p>
          {records.length} records
          {realms.length > 1 ? ` · ${realms.length} realms` : ""}
        </p>
      </div>
      {error ? (
        <div className="gating__empty" data-testid="memory-error">
          {error}
        </div>
      ) : null}
      <div className="gating__tabs">
        {tabs.map((candidate) => (
          <button
            key={candidate}
            className={`gating__tab ${tab === candidate ? "is-active" : ""}`}
            onClick={() => setTab(candidate)}
            data-testid={`memory-tab:${candidate}`}
          >
            {candidate === "records" ? "Records" : candidate === "quarantine" ? "Quarantine" : "Dreams"}
            {candidate === "quarantine" ? (
              <span className="n">{quarantineRecords.length}</span>
            ) : null}
            {candidate === "dreams" ? <span className="n">{dreams.length}</span> : null}
          </button>
        ))}
        <button className="gating__tab" onClick={onRefresh} data-testid="memory-refresh">
          Refresh
        </button>
      </div>

      <div className="gating__list memory-panel__body">
        {tab === "records" ? (
          detail ? (
            <DetailView detail={detail} onBack={onClearDetail} />
          ) : detailLoading ? (
            <div className="gating__empty">Loading record…</div>
          ) : groups.length === 0 ? (
            <div className="gating__empty">No memory records yet.</div>
          ) : (
            <div className="memory-groups">
              {groups.map((group) => (
                <div className="memory-group" key={group.key} data-testid={`memory-group:${group.key}`}>
                  <div className="memory-group__label">{group.label}</div>
                  {group.records.map((record) => (
                    <RecordRow
                      key={record.id}
                      record={record}
                      onSelect={() => onSelectRecord(realmOfRecord(record), record.id)}
                    />
                  ))}
                </div>
              ))}
            </div>
          )
        ) : null}

        {tab === "quarantine" && canReviewQuarantine ? (
          <div className="memory-quarantine">
            <div className="memory-note" data-testid="memory-quarantine-note">
              Read-only. Verdicts are decided by the memory steward's dream and the
              gating flow — this queue cannot be actioned here.
            </div>
            {quarantineRecords.length === 0 && pendingPromotions.length === 0 ? (
              <div className="gating__empty">Quarantine queue is empty.</div>
            ) : null}
            {quarantineRecords.length > 0 ? (
              <div className="memory-group">
                <div className="memory-group__label">Quarantined records</div>
                {quarantineRecords.map((record) => {
                  const reason =
                    record.status.status === "quarantined" ? record.status.reason : undefined;
                  return (
                    <div
                      className="memory-row memory-row--static"
                      key={record.id}
                      data-testid={`memory-quarantine-record:${record.id}`}
                    >
                      <span className="memory-row__title">{record.title || record.id}</span>
                      <span className="memory-row__meta">
                        {reason ? <span className="memory-row__reason">{reason}</span> : null}
                        <Chip label={trustLabel(record.trust)} tone={trustTone(record.trust)} />
                        <span className="memory-row__age">{relativeAge(record.created_at_ms)}</span>
                      </span>
                    </div>
                  );
                })}
              </div>
            ) : null}
            {pendingPromotions.length > 0 ? (
              <div className="memory-group">
                <div className="memory-group__label">Pending gated promotions</div>
                {pendingPromotions.map((pending) => (
                  <div
                    className="memory-row memory-row--static"
                    key={pending.pending_id}
                    data-testid={`memory-pending:${pending.pending_id}`}
                  >
                    <span className="memory-row__title">
                      {pending.record_id} → {pending.scope_kind}:{pending.scope_key}
                    </span>
                    <span className="memory-row__meta">
                      {pending.rationale ? (
                        <span className="memory-row__reason">{pending.rationale}</span>
                      ) : null}
                      {pending.status ? <Chip label={pending.status} tone="muted" /> : null}
                      <span className="memory-row__age">{relativeAge(pending.created_at_ms)}</span>
                    </span>
                  </div>
                ))}
              </div>
            ) : null}
          </div>
        ) : null}

        {tab === "dreams" ? (
          <div className="memory-dreams">
            {dreams.length === 0 ? (
              <div className="gating__empty">No dream runs recorded yet.</div>
            ) : (
              dreams.map((run) => {
                const summary = dreamOpKindsSummary(run.op_kinds);
                return (
                  <div className="gpolicy memory-dream" key={run.run_id} data-testid={`memory-dream:${run.run_id}`}>
                    <div className="gpolicy__head">
                      <span className="gpolicy__action">{run.run_id}</span>
                      {run.quarantined_ops && run.quarantined_ops > 0 ? (
                        <Chip label={`${run.quarantined_ops} quarantined`} tone="warning" />
                      ) : null}
                    </div>
                    <div className="gpolicy__meta">{dreamTimeRange(run)}</div>
                    <div className="gpolicy__meta">
                      {typeof run.ops === "number" ? `${run.ops} ops` : "—"}
                      {summary ? ` · ${summary}` : ""}
                    </div>
                    {(run.rationales || []).map((rationale, index) => (
                      <div className="memory-dream__rationale" key={`r-${index}`}>
                        {rationale}
                      </div>
                    ))}
                  </div>
                );
              })
            )}
          </div>
        ) : null}
      </div>
    </div>
  );
}
