import React from "react";
import { DenseGraphMap } from "./DenseGraphMap";
import {
  graphStats,
  groupMatrix,
  groupSummaries,
  type TopoActivity,
  type TopoAgent,
  type TopoGraph,
} from "./data";

interface LargeGraphSummaryProps {
  graph: TopoGraph;
  live: TopoActivity;
}

const EDGE_SAMPLE_LIMIT = 1500;

function fmt(value: number): string {
  return new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 }).format(value);
}

function percent(value: number): string {
  return `${fmt(value * 100)}%`;
}

function agentRank(graph: TopoGraph): TopoAgent[] {
  return graph.agents.slice().sort((a, b) => {
    const degreeDelta = (graph.degree[b.id] || 0) - (graph.degree[a.id] || 0);
    if (degreeDelta !== 0) return degreeDelta;
    const busyDelta = Number(!!a.labels.parent_identity) - Number(!!b.labels.parent_identity);
    if (busyDelta !== 0) return busyDelta;
    return a.id.localeCompare(b.id);
  });
}

export function LargeGraphSummary({
  graph,
  live,
}: LargeGraphSummaryProps): React.JSX.Element {
  const stats = React.useMemo(() => graphStats(graph), [graph]);
  const groups = React.useMemo(() => groupSummaries(graph), [graph]);
  const matrix = React.useMemo(() => groupMatrix(graph, 8), [graph]);
  const ranked = React.useMemo(() => agentRank(graph), [graph]);
  const [query, setQuery] = React.useState("");
  const [selectedId, setSelectedId] = React.useState<string>(() => ranked[0]?.id || "");

  React.useEffect(() => {
    if (!selectedId || !graph.byId.has(selectedId)) {
      setSelectedId(ranked[0]?.id || "");
    }
  }, [graph, ranked, selectedId]);

  const matches = React.useMemo(() => {
    const q = query.trim().toLowerCase();
    const source = q
      ? ranked.filter((a) =>
          a.id.toLowerCase().includes(q)
          || a.label.toLowerCase().includes(q)
          || a.group.toLowerCase().includes(q)
          || a.role.toLowerCase().includes(q)
        )
      : ranked;
    return source.slice(0, 80);
  }, [query, ranked]);

  const selected = graph.byId.get(selectedId) || ranked[0];
  const peers = selected
    ? selected.wiredTo
        .map((id) => graph.byId.get(id))
        .filter((a): a is TopoAgent => !!a)
        .sort((a, b) => a.group.localeCompare(b.group) || a.id.localeCompare(b.id))
    : [];
  const activeCount = Object.keys(live.active).length;
  const busyCount = Object.values(live.busy).filter(Boolean).length;
  const visiblePeerPreview = peers.slice(0, 150);

  return (
    <div className="topo-summary" data-testid="topology-summary">
      <DenseGraphMap
        graph={graph}
        live={live}
        selectedId={selected?.id}
        onSelect={setSelectedId}
      />

      <div className="topo-summary__stats" aria-label="Topology scale">
        <ScaleStat label="Agents" value={String(stats.nodeCount)} />
        <ScaleStat label="Edges" value={String(stats.edgeCount)} sub={`${percent(stats.density)} density`} />
        <ScaleStat label="Degree" value={`${fmt(stats.avgDegree)} avg`} sub={`${stats.minDegree}-${stats.maxDegree}`} />
        <ScaleStat label="Live" value={String(activeCount)} sub={busyCount > 0 ? `${busyCount} working` : "idle"} />
        {stats.edgeCount > EDGE_SAMPLE_LIMIT && (
          <ScaleStat label="Graph views" value={`${EDGE_SAMPLE_LIMIT} edges`} sub="sampled" />
        )}
      </div>

      <div className="topo-summary__grid">
        <section className="topo-summary__section topo-summary__section--groups" aria-label="Mob groups">
          <div className="topo-summary__section-head">
            <h3>Mobs</h3>
            <span>{groups.length} groups</span>
          </div>
          <div className="topo-summary__group-list">
            {groups.map((g) => (
              <div key={g.group} className="topo-summary__group-row">
                <span className="topo-summary__group-name" title={g.group}>{g.group}</span>
                <span>{g.count}</span>
                <span>{g.internalEdges} internal</span>
                <span>{g.externalEdges} cross</span>
              </div>
            ))}
          </div>
        </section>

        <section className="topo-summary__section topo-summary__section--matrix" aria-label="Group edge matrix">
          <div className="topo-summary__section-head">
            <h3>Edge Matrix</h3>
            <span>{matrix.length} populated cells</span>
          </div>
          <div className="topo-summary__matrix">
            {matrix.map((cell) => (
              <div key={`${cell.from}:${cell.to}`} className="topo-summary__matrix-row">
                <span title={cell.from}>{cell.from}</span>
                <span title={cell.to}>{cell.to}</span>
                <strong>{cell.edges}</strong>
              </div>
            ))}
          </div>
        </section>

        <section className="topo-summary__section topo-summary__section--agent" aria-label="Selected agent ego network">
          <div className="topo-summary__section-head">
            <h3>Ego Network</h3>
            <span>{selected ? `${peers.length} peers` : "none"}</span>
          </div>
          <div className="topo-summary__agent-tools">
            <input
              className="topo-summary__search"
              type="search"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Filter agents"
              aria-label="Filter topology agents"
            />
            <select
              className="topo-summary__select"
              value={selected?.id || ""}
              onChange={(event) => setSelectedId(event.target.value)}
              aria-label="Select topology agent"
            >
              {matches.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.label}
                </option>
              ))}
            </select>
          </div>
          {selected && (
            <div className="topo-summary__agent-card" data-testid={`topology-ego:${selected.id}`}>
              <div>
                <strong>{selected.label}</strong>
                <span>{selected.id}</span>
              </div>
              <div>
                <span>{selected.group}</span>
                <span>{selected.role}</span>
                <span>{selected.state || "unknown"}</span>
              </div>
            </div>
          )}
          <div className="topo-summary__peer-list">
            {visiblePeerPreview.map((peer) => (
              <button
                key={peer.id}
                type="button"
                className="topo-summary__peer"
                onClick={() => setSelectedId(peer.id)}
                title={`${peer.label} - ${peer.group}`}
              >
                <span>{peer.label}</span>
                <small>{peer.group}</small>
              </button>
            ))}
          </div>
        </section>
      </div>
    </div>
  );
}

function ScaleStat({
  label,
  value,
  sub,
}: {
  label: string;
  value: string;
  sub?: string;
}): React.JSX.Element {
  return (
    <div className="topo-summary__stat">
      <span>{label}</span>
      <strong>{value}</strong>
      {sub && <small>{sub}</small>}
    </div>
  );
}
