/* global React */
// Studio state hook + GraphEditor.
// Studio has TWO entities: members (registry) and instances (graph nodes).

const NODE_W = 200;
const NODE_H = 156;

// Compute dynamic grid dims from instances. Always at least the base,
// and one extra empty col/row past the rightmost/bottommost node so
// users have somewhere to drop or click-add.
function dynGrid(instances, gridBase) {
  let maxCol = gridBase.cols - 1;
  let maxRow = gridBase.rows - 1;
  for (const i of instances) {
    if (i.col > maxCol) maxCol = i.col;
    if (i.row > maxRow) maxRow = i.row;
  }
  return { ...gridBase, cols: maxCol + 2, rows: maxRow + 2 };
}

function useStudioState(initial, onDirty, authoring = {}) {
  const [members, setMembers] = React.useState(initial.members);
  const [instances, setInstances] = React.useState(initial.instances);
  const [edges, setEdges] = React.useState(initial.edges);
  const [frames, setFrames] = React.useState(initial.frames);
  const [schemas, setSchemas] = React.useState(initial.schemas);
  const [skillRealms, setSkillRealms] = React.useState(initial.skillRealms || []);
  const [history, setHistory] = React.useState([]);
  const [future, setFuture] = React.useState([]);

  const studioState = React.useCallback(() => ({
    members,
    instances,
    edges,
    frames,
    schemas,
    skillRealms,
  }), [members, instances, edges, frames, schemas, skillRealms]);

  const applyStudioState = React.useCallback((state) => {
    setMembers(state.members);
    setInstances(state.instances);
    setEdges(state.edges);
    setFrames(state.frames);
    setSchemas(state.schemas);
    setSkillRealms(state.skillRealms || []);
  }, []);

  const snap = React.useCallback(() => {
    if (onDirty) onDirty();
    const next = window.MobKitFlowController.studioHistorySnapshotPatch({
      history,
      future,
      state: studioState(),
    });
    setHistory(next.history);
    setFuture(next.future);
  }, [history, future, studioState, onDirty]);

  const undo = () => {
    const next = window.MobKitFlowController.studioUndoPatch({ history, future, state: studioState() });
    if (!next) return;
    if (onDirty) onDirty();
    setHistory(next.history);
    setFuture(next.future);
    applyStudioState(next.state);
  };
  const redo = () => {
    const next = window.MobKitFlowController.studioRedoPatch({ history, future, state: studioState() });
    if (!next) return;
    if (onDirty) onDirty();
    setHistory(next.history);
    setFuture(next.future);
    applyStudioState(next.state);
  };

  // Mutations (each takes a snapshot)
  const addMember = (m) => {
    snap();
    const next = window.MobKitFlowController.studioAddMemberPatch({ members }, m);
    setMembers(next.members);
  };
  const updateMember = (id, patch) => {
    snap();
    const next = window.MobKitFlowController.studioUpdateMemberPatch({ members }, id, patch);
    setMembers(next.members);
  };
  const deleteMember = (id) => {
    snap();
    const next = window.MobKitFlowController.studioDeleteMemberPatch({ members, instances, edges }, id);
    setMembers(next.members);
    setInstances(next.instances);
    setEdges(next.edges);
  };
  const addInstance = (i) => {
    snap();
    const next = window.MobKitFlowController.studioAddInstancePatch({ instances, members }, i);
    setInstances(next.instances);
  };
  const updateInstance = (id, patch) => {
    snap();
    const next = window.MobKitFlowController.studioUpdateInstancePatch({ instances, members }, id, patch);
    setInstances(next.instances);
  };
  const deleteInstance = (id) => {
    snap();
    const next = window.MobKitFlowController.studioDeleteInstancePatch({ instances, edges }, id);
    setInstances(next.instances);
    setEdges(next.edges);
  };
  const addEdge = (e) => {
    snap();
    const next = window.MobKitFlowController.studioAddEdgePatch({ edges, instances }, e);
    setEdges(next.edges);
  };
  const updateEdge = (id, patch) => {
    snap();
    const next = window.MobKitFlowController.studioUpdateEdgePatch({ edges, instances }, id, patch);
    setEdges(next.edges);
  };
  const deleteEdge = (id) => {
    snap();
    const next = window.MobKitFlowController.studioDeleteEdgePatch({ edges }, id);
    setEdges(next.edges);
  };

  const addSchema = (s) => {
    snap();
    const next = window.MobKitFlowController.studioAddSchemaPatch({ schemas }, s);
    setSchemas(next.schemas);
  };
  const updateSchema = (id, patch) => {
    snap();
    const next = window.MobKitFlowController.studioUpdateSchemaPatch({ schemas }, id, patch);
    setSchemas(next.schemas);
  };
  const deleteSchema = (id) => {
    snap();
    const next = window.MobKitFlowController.studioDeleteSchemaPatch({
      schemas,
      members,
      flow: authoring.flow,
      edges,
      instances,
    }, id);
    setSchemas(next.schemas);
    setMembers(next.members);
    if (next.flow !== authoring.flow && authoring.setFlow) authoring.setFlow(next.flow);
    if (next.edges) setEdges(next.edges);
  };
  const updateSkillRealms = (next) => {
    snap();
    setSkillRealms(Array.isArray(next) ? next : []);
  };

  return {
    members, instances, edges, frames, schemas, skillRealms,
    setMembers, setInstances, setEdges, setFrames, setSchemas, setSkillRealms,
    snap, undo, redo, canUndo: !!history.length, canRedo: !!future.length,
    addMember, updateMember, deleteMember,
    addInstance, updateInstance, deleteInstance,
    addEdge, updateEdge, deleteEdge,
    addSchema, updateSchema, deleteSchema,
    updateSkillRealms,
  };
}

// ── Geometry ──
function cellXYFor(g, col, row) {
  return {
    x: g.padX + col * (g.cellW + g.gapX),
    y: g.padY + row * (g.cellH + g.gapY),
  };
}
function nodeBox(g, n) {
  const { x, y } = cellXYFor(g, n.col, n.row);
  if (n.isSourceFile) {
    const sw = 210, sh = 58;
    return { x: x + (g.cellW - sw) / 2, y: y + (g.cellH - sh) / 2, w: sw, h: sh };
  }
  if (n.isGate) {
    // Gate nodes render smaller — a compact pill in the cell center.
    const gw = 156, gh = 56;
    return { x: x + (g.cellW - gw) / 2, y: y + (g.cellH - gh) / 2, w: gw, h: gh };
  }
  return {
    x: x + (g.cellW - NODE_W) / 2,
    y: y + (g.cellH - NODE_H) / 2,
    w: NODE_W, h: NODE_H,
  };
}
function portOut(g, n) { const b = nodeBox(g, n); return { x: b.x + b.w, y: b.y + b.h / 2 }; }
function portIn(g, n)  { const b = nodeBox(g, n); return { x: b.x,         y: b.y + b.h / 2 }; }

function edgePath(a, b) {
  if (b.x < a.x - 20) {
    const dropY = Math.max(a.y, b.y) + 90;
    const dx = 60;
    return `M ${a.x} ${a.y} C ${a.x + dx} ${a.y}, ${a.x + dx} ${dropY}, ${a.x} ${dropY} L ${b.x} ${dropY} C ${b.x - dx} ${dropY}, ${b.x - dx} ${b.y}, ${b.x} ${b.y}`;
  }
  const dx = Math.max(40, (b.x - a.x) * 0.5);
  return `M ${a.x} ${a.y} C ${a.x + dx} ${a.y}, ${b.x - dx} ${b.y}, ${b.x} ${b.y}`;
}
function midpoint(a, b) {
  if (b.x < a.x - 20) return { x: (a.x + b.x) / 2, y: Math.max(a.y, b.y) + 90 };
  return { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 - 6 };
}

function occMap(insts) {
  const m = new Map();
  for (const i of insts) m.set(i.col + ":" + i.row, i);
  return m;
}

function GraphEditor({ state, selection, selectInstance, selectEdge, clearSelection, activeStepId, edgeStyle, density, onRequestAdd, onOpenSourceFile, memberFocus, grid, contract, graphView = null }) {
  const hostRef = React.useRef(null);
  const [drag, setDrag] = React.useState(null);
  const [conn, setConn] = React.useState(null);
  const [hoverInId, setHoverInId] = React.useState(null);
  const [hoverCell, setHoverCell] = React.useState(null);
  const canvasView = window.MobKitFlowController.graphCanvasViewState(graphView);

  // ── View transform (pan + zoom) ──
  const [view, setView] = React.useState({ scale: 1, tx: 0, ty: 0 });
  const viewRef = React.useRef(view);
  React.useEffect(() => { viewRef.current = view; }, [view]);
  const [panDrag, setPanDrag] = React.useState(null);

  const g = dynGrid(state.instances, grid);
  const totalW = g.padX * 2 + g.cols * g.cellW + (g.cols - 1) * g.gapX;
  const totalH = g.padY * 2 + g.rows * g.cellH + (g.rows - 1) * g.gapY;
  const occ = occMap(state.instances);

  // Fit-to-content (used on mount and on the ⤢ button)
  const fitToBounds = React.useCallback(() => {
    const host = hostRef.current;
    if (!host) return;
    const r = host.getBoundingClientRect();
    const scale = Math.min(1, Math.min((r.width - 32) / totalW, (r.height - 32) / totalH));
    const tx = (r.width - totalW * scale) / 2;
    const ty = Math.max(8, (r.height - totalH * scale) / 2);
    setView({ scale, tx, ty });
  }, [totalW, totalH]);

  // Auto-fit only on first mount (and when host first measures).
  const didFit = React.useRef(false);
  React.useEffect(() => {
    if (didFit.current) return;
    if (hostRef.current?.offsetWidth > 0) {
      fitToBounds();
      didFit.current = true;
    } else {
      const id = setTimeout(() => { fitToBounds(); didFit.current = true; }, 50);
      return () => clearTimeout(id);
    }
  }, [fitToBounds]);

  const screenToWorld = (sx, sy) => {
    const r = hostRef.current.getBoundingClientRect();
    const v = viewRef.current;
    return { x: (sx - r.left - v.tx) / v.scale, y: (sy - r.top - v.ty) / v.scale };
  };

  // Zoom around a screen point (so the cursor anchor stays under the cursor)
  const zoomAt = (factor, sx, sy) => {
    const v = viewRef.current;
    const r = hostRef.current.getBoundingClientRect();
    const cx = sx - r.left;
    const cy = sy - r.top;
    const next = Math.max(0.3, Math.min(2.5, v.scale * factor));
    const k = next / v.scale;
    setView({
      scale: next,
      tx: cx - (cx - v.tx) * k,
      ty: cy - (cy - v.ty) * k,
    });
  };

  const cellAt = (x, y) => {
    const col = Math.floor((x - g.padX + g.gapX / 2) / (g.cellW + g.gapX));
    const row = Math.floor((y - g.padY + g.gapY / 2) / (g.cellH + g.gapY));
    if (col < 0 || col >= g.cols || row < 0 || row >= g.rows) return null;
    return { col, row };
  };

  const onNodeDown = (e, inst) => {
    if (e.target.classList.contains("port")) return;
    e.stopPropagation();
    selectInstance(inst.id);
    const w = screenToWorld(e.clientX, e.clientY);
    const b = nodeBox(g, inst);
    setDrag({ instId: inst.id, dx: w.x - b.x, dy: w.y - b.y, origCol: inst.col, origRow: inst.row });
  };
  const onPortDown = (e, inst) => {
    e.stopPropagation();
    const p = portOut(g, inst);
    setConn({ from: p, fromId: inst.id, to: p });
  };

  // Background mouse-down → start panning (only on the canvas-host itself,
  // not on a node/cell/port/edge — those have their own handlers).
  const onHostMouseDown = (e) => {
    if (e.button !== 0 && e.button !== 1) return;
    const target = e.target;
    // Allow pan when starting on the empty canvas background or grid dots.
    if (target === hostRef.current || target.classList?.contains("canvas")) {
      setPanDrag({ sx: e.clientX, sy: e.clientY, tx0: viewRef.current.tx, ty0: viewRef.current.ty });
      e.preventDefault();
    }
  };

  const onHostWheel = (e) => {
    if (!hostRef.current) return;
    // Pinch-zoom on touchpads sends ctrlKey=true. Cmd/Ctrl+wheel zooms too.
    if (e.ctrlKey || e.metaKey) {
      e.preventDefault();
      const factor = Math.exp(-e.deltaY * 0.0015);
      zoomAt(factor, e.clientX, e.clientY);
    } else {
      // Two-finger scroll on touchpad pans.
      e.preventDefault();
      setView(v => ({ ...v, tx: v.tx - e.deltaX, ty: v.ty - e.deltaY }));
    }
  };

  // Attach wheel listener with passive:false so we can preventDefault.
  React.useEffect(() => {
    const el = hostRef.current;
    if (!el) return;
    const handler = (e) => onHostWheel(e);
    el.addEventListener("wheel", handler, { passive: false });
    return () => el.removeEventListener("wheel", handler);
  });

  React.useEffect(() => {
    const move = (e) => {
      if (panDrag) {
        setView(v => ({ ...v, tx: panDrag.tx0 + (e.clientX - panDrag.sx), ty: panDrag.ty0 + (e.clientY - panDrag.sy) }));
      }
      if (drag) {
        const w = screenToWorld(e.clientX, e.clientY);
        const cx = w.x - drag.dx + NODE_W / 2;
        const cy = w.y - drag.dy + NODE_H / 2;
        const cell = cellAt(cx, cy);
        if (cell) setHoverCell(cell);
      }
      if (conn) {
        const w = screenToWorld(e.clientX, e.clientY);
        setConn(c => ({ ...c, to: { x: w.x, y: w.y } }));
        const t = document.elementFromPoint(e.clientX, e.clientY);
        const closest = t?.closest?.("[data-inst-id]");
        if (closest && closest.dataset.instId !== conn.fromId) setHoverInId(closest.dataset.instId);
        else setHoverInId(null);
      }
    };
    const up = (e) => {
      if (drag) {
        const w = screenToWorld(e.clientX, e.clientY);
        const cx = w.x - drag.dx + NODE_W / 2;
        const cy = w.y - drag.dy + NODE_H / 2;
        const cell = cellAt(cx, cy);
        if (cell && (cell.col !== drag.origCol || cell.row !== drag.origRow)) {
          state.snap();
          const next = window.MobKitFlowController.studioMoveInstancePatch({
            instances: state.instances,
          }, drag.instId, cell, {
            col: drag.origCol,
            row: drag.origRow,
          });
          state.setInstances(next.instances);
        }
        setDrag(null); setHoverCell(null);
      }
      if (conn) {
        const t = document.elementFromPoint(e.clientX, e.clientY);
        const closest = t?.closest?.("[data-inst-id]");
        if (closest && closest.dataset.instId !== conn.fromId) {
          const toId = closest.dataset.instId;
          const fromI = state.instances.find(i => i.id === conn.fromId);
          const toI = state.instances.find(i => i.id === toId);
          const newEdge = window.MobKitFlowController.graphConnectionEdgeDraft({
            from: fromI,
            to: toI,
            edges: state.edges,
            contract,
          });
          if (newEdge) {
            state.addEdge(newEdge);
            selectEdge(newEdge.id);
          }
        }
        setConn(null); setHoverInId(null);
      }
      if (panDrag) setPanDrag(null);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
    return () => { window.removeEventListener("mousemove", move); window.removeEventListener("mouseup", up); };
  });

  const fit = view;

  const cells = [];
  for (let c = 0; c < g.cols; c++) {
    for (let r = 0; r < g.rows; r++) {
      const occupied = occ.has(c + ":" + r);
      const { x, y } = cellXYFor(g, c, r);
      cells.push(
        <div key={`cell-${c}-${r}`}
          className={"cell" + (occupied ? " is-occupied" : "") + (hoverCell?.col === c && hoverCell?.row === r ? " is-hover" : "")}
          style={{ left: x, top: y, width: g.cellW, height: g.cellH }}
          onMouseDown={(e) => e.stopPropagation()}
          onClick={(e) => { e.stopPropagation(); if (!occupied) onRequestAdd(c, r); }}
        >
          {!occupied && <div className="cell__add"><span className="cell__plus">+</span></div>}
        </div>
      );
    }
  }

  const colHeads = [];
  for (let c = 0; c < g.cols; c++) {
    const { x } = cellXYFor(g, c, 0);
    colHeads.push(<div key={"col-" + c} className="grid-head grid-head--col" style={{ left: x, top: 28, width: g.cellW }}>{String(c + 1).padStart(2, "0")}</div>);
  }
  const rowHeads = [];
  for (let r = 0; r < g.rows; r++) {
    const { y } = cellXYFor(g, 0, r);
    rowHeads.push(<div key={"row-" + r} className="grid-head grid-head--row" style={{ left: 14, top: y + g.cellH/2 - 8 }}>{String.fromCharCode(65 + r)}</div>);
  }

  const frameEls = state.frames.map(fr => {
    const a = cellXYFor(g, fr.colStart, 0);
    const b = cellXYFor(g, fr.colEnd, g.rows - 1);
    const x = a.x - 14, y = a.y - 18;
    const w = (b.x + g.cellW) - x + 14;
    const h = (b.y + g.cellH) - y + 18;
    return (
      <React.Fragment key={fr.id}>
        <div className="frame" style={{ left: x, top: y, width: w, height: h }} />
        <div className="frame-label" style={{ left: x + 12, top: y - 10 }}>{fr.label}</div>
      </React.Fragment>
    );
  });

  const edgeEls = state.edges.map(edge => {
    const fi = state.instances.find(i => i.id === edge.from);
    const ti = state.instances.find(i => i.id === edge.to);
    if (!fi || !ti) return null;
    const a = portOut(g, fi), b = portIn(g, ti);
    const d = edgePath(a, b);
    const mid = midpoint(a, b);
    const isActive = activeStepId === edge.from;
    const isSelected = selection.kind === "edge" && selection.id === edge.id;
    const edgeState = window.MobKitFlowController.graphEdgeCanvasState({
      edge,
      to: ti,
      active: isActive,
      selected: isSelected,
      edgeStyle,
      contract,
    });

    let labelEl;
    if (edgeState.mode === "icons") {
      labelEl = <g transform={`translate(${mid.x}, ${mid.y})`}><rect x={-9} y={-9} width={18} height={16} className="edge-label-bg"/><text textAnchor="middle" y={4} className={edgeState.iconLabelClass}>{edgeState.iconGlyph}</text></g>;
    } else if (edgeState.mode === "colored") {
      labelEl = <g transform={`translate(${mid.x}, ${mid.y})`}><rect x={-edgeState.labelWidth/2} y={-8} width={edgeState.labelWidth} height={14} className="edge-label-bg"/><text textAnchor="middle" y={3} className="edge-label" style={{ fill: edgeState.labelFill }}>{edgeState.labelText}</text></g>;
    } else {
      labelEl = <g transform={`translate(${mid.x}, ${mid.y})`}><rect x={-edgeState.labelWidth/2} y={-8} width={edgeState.labelWidth} height={14} className="edge-label-bg"/><text textAnchor="middle" y={3} className={edgeState.textLabelClass}>{edgeState.labelText}</text></g>;
    }

    return (
      <g key={edge.id} className="edge" onClick={(e) => { e.stopPropagation(); selectEdge(edge.id); }}>
        <path d={d} className="edge-hit" />
        <path d={d} className={edgeState.lineClass} markerEnd={edgeState.markerEnd} />
        {labelEl}
      </g>
    );
  });

  const canvasInstances = window.MobKitFlowController.graphCanvasInstances({ instances: state.instances, graphView: canvasView });
  const nodeEls = canvasInstances.map(inst => {
    if (inst.isGate) {
      return (
        <GateView key={inst.id}
          g={g} inst={inst}
          selected={selection.kind === "instance" && selection.id === inst.id}
          activeStep={activeStepId === inst.id}
          hoverIn={hoverInId === inst.id}
          onMouseDown={onNodeDown}
          onPortDown={onPortDown}
          portDragTitle={canvasView.portDragTitle}
          state={state}
        />
      );
    }
    return (
      <NodeView key={inst.id}
        g={g}
        inst={inst}
        nodeState={window.MobKitFlowController.graphNodeCanvasState({ inst, members: state.members, density, graphView: canvasView })}
        selected={selection.kind === "instance" && selection.id === inst.id}
        memberHighlight={memberFocus && inst.memberId === memberFocus}
        memberDim={!!memberFocus && inst.memberId !== memberFocus && !inst.isTerminal}
        activeStep={activeStepId === inst.id}
        hoverIn={hoverInId === inst.id}
        onMouseDown={onNodeDown}
        onPortDown={onPortDown}
        portDragTitle={canvasView.portDragTitle}
        onOpenSourceFile={onOpenSourceFile}
      />
    );
  });

  return (
    <div ref={hostRef} className={"canvas-host" + (memberFocus ? " is-member-focus" : "") + (panDrag ? " is-panning" : "")}
      onMouseDown={onHostMouseDown}
      onClick={(e) => { if (e.target === hostRef.current || e.target.classList?.contains("canvas")) clearSelection(); }}
    >
      <div className="canvas" style={{ width: totalW, height: totalH, transform: `translate(${fit.tx}px, ${fit.ty}px) scale(${fit.scale})`, transformOrigin: "0 0" }}>
        {colHeads}
        {rowHeads}
        {frameEls}
        {cells}
        <svg className="edges-svg" width={totalW} height={totalH}>
          <defs>
            <marker id="arr"     viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto"><path d="M 0 0 L 10 5 L 0 10 z" fill="var(--ink)"/></marker>
            <marker id="arr-red" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto"><path d="M 0 0 L 10 5 L 0 10 z" fill="var(--danger)"/></marker>
            <marker id="arr-acc" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto"><path d="M 0 0 L 10 5 L 0 10 z" fill="var(--accent)"/></marker>
            <marker id="arr-dim" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto"><path d="M 0 0 L 10 5 L 0 10 z" fill="var(--subtle)"/></marker>
          </defs>
          {edgeEls}
          {conn && <path d={edgePath(conn.from, conn.to)} className="edge-line is-ghost" markerEnd="url(#arr-acc)" />}
        </svg>
        {nodeEls}
      </div>

      {/* Zoom controls — fixed in the corner, outside the scaled canvas. */}
      <div className="zoom-controls" onMouseDown={e => e.stopPropagation()}>
        <button className="zoom-btn" title={canvasView.zoomOutTitle} onClick={() => {
          const r = hostRef.current.getBoundingClientRect();
          zoomAt(1 / 1.2, r.left + r.width / 2, r.top + r.height / 2);
        }}>−</button>
        <button className="zoom-btn zoom-btn--pct" title={canvasView.fitTitle} onClick={fitToBounds}>
          {Math.round(view.scale * 100)}%
        </button>
        <button className="zoom-btn" title={canvasView.zoomInTitle} onClick={() => {
          const r = hostRef.current.getBoundingClientRect();
          zoomAt(1.2, r.left + r.width / 2, r.top + r.height / 2);
        }}>+</button>
      </div>
    </div>
  );
}

function NodeView({ g, inst, nodeState, selected, memberHighlight, memberDim, activeStep, hoverIn, onMouseDown, onPortDown, portDragTitle, onOpenSourceFile }) {
  const b = nodeBox(g, inst);

  if (nodeState.isTerminal) {
    const openSourceFile = (event) => {
      if (!nodeState.isSourceFile) return;
      event.stopPropagation();
      onOpenSourceFile?.(inst);
    };
    if (nodeState.isSourceFile) {
      return (
        <div data-inst-id={inst.id}
          className={"node node--term node--source-file" + (selected ? " is-selected" : "") + (activeStep ? " is-active-step" : "") + (hoverIn ? " is-target" : "")}
          data-kind={nodeState.dataKind}
          role={nodeState.role}
          tabIndex={nodeState.tabIndex}
          aria-label={nodeState.ariaLabel}
          style={{ left: b.x, top: b.y, width: b.w, height: b.h }}
          onMouseDown={(e) => {
            e.stopPropagation();
          }}
          onClick={openSourceFile}
          onKeyDown={(e) => {
            if (e.key !== "Enter" && e.key !== " ") return;
            e.preventDefault();
            openSourceFile(e);
          }}
        >
          <span className="source-file__glyph">{nodeState.sourceGlyph}</span>
          <span className="source-file__label">{nodeState.title}</span>
        </div>
      );
    }
    return (
      <div data-inst-id={inst.id}
        className={"node node--term" + (nodeState.isSourceFile ? " node--source-file" : "") + (selected ? " is-selected" : "") + (activeStep ? " is-active-step" : "") + (hoverIn ? " is-target" : "")}
        data-kind={nodeState.dataKind}
        role={nodeState.role}
        tabIndex={nodeState.tabIndex}
        aria-label={nodeState.ariaLabel}
        style={{ left: b.x, top: b.y, width: b.w, height: b.h }}
        onMouseDown={(e) => {
          if (nodeState.isSourceFile) {
            e.stopPropagation();
            return;
          }
          onMouseDown(e, inst);
        }}
        onClick={openSourceFile}
        onKeyDown={(e) => {
          if (!nodeState.isSourceFile || (e.key !== "Enter" && e.key !== " ")) return;
          e.preventDefault();
          openSourceFile(e);
        }}
      >
        <div className="node__head"><span className="node__role">{nodeState.roleLabel}</span></div>
        <div className="node__body">
          <div className="node__name">{nodeState.title}</div>
          <div className="node__model">{nodeState.subtitle}</div>
        </div>
      </div>
    );
  }

  if (nodeState.hidden) return null;

  return (
    <div data-inst-id={inst.id}
      className={"node" + (selected ? " is-selected" : "") + (memberHighlight ? " is-member-highlight" : "") + (memberDim ? " is-member-dim" : "") + (activeStep ? " is-active-step" : "") + (hoverIn ? " is-target" : "") + (nodeState.isCompact ? " is-compact" : "")}
      style={{ left: b.x, top: b.y, width: b.w, height: b.h }}
      onMouseDown={(e) => onMouseDown(e, inst)}
    >
      <div className="port port-out" onMouseDown={(e) => onPortDown(e, inst)} title={portDragTitle} />
      <div className="node__head">
        <span className="node__role">{nodeState.roleLabel}</span>
        <span className="node__idx">{nodeState.launchLabel}</span>
      </div>
      <div className="node__body">
        <div className="node__name">{nodeState.title}</div>
        <div className="node__model">{nodeState.subtitle}</div>
      </div>
      {!nodeState.isCompact && (
        <div className="node__tools">
          {nodeState.toolRows.map(row => <span key={row.id} className={row.className}>{row.id}</span>)}
          {nodeState.overflowLabel && <span className="tag">{nodeState.overflowLabel}</span>}
        </div>
      )}
    </div>
  );
}

function GateView({ g, inst, selected, activeStep, hoverIn, onMouseDown, onPortDown, portDragTitle, state }) {
  const b = nodeBox(g, inst);
  const gateState = window.MobKitFlowController.graphGateCanvasState({ inst, edges: state.edges });

  return (
    <div data-inst-id={inst.id}
      className={"node node--gate gate--" + gateState.gateKind + (selected ? " is-selected" : "") + (activeStep ? " is-active-step" : "") + (hoverIn ? " is-target" : "")}
      style={{ left: b.x, top: b.y, width: b.w, height: b.h }}
      onMouseDown={(e) => onMouseDown(e, inst)}
    >
      <div className="port port-out" onMouseDown={(e) => onPortDown(e, inst)} title={portDragTitle} />
      <span className="gate__glyph">{gateState.glyph}</span>
      <span className="gate__label">{gateState.sublabel}</span>
    </div>
  );
}

function computeFit(vw, vh, tw, th) {
  const scale = Math.min(1, Math.min((vw - 24) / tw, (vh - 24) / th));
  const left = (vw - tw * scale) / 2;
  const top  = Math.max(8, (vh - th * scale) / 2);
  return { scale, left, top };
}

window.useStudioState = useStudioState;
window.GraphEditor = GraphEditor;
