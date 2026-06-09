/* global React */
// Studio state hook + GraphEditor.
// Studio has TWO entities: members (registry) and instances (graph nodes).

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
    const next = window.MobKitFlowController.studioAddMemberPatch({ members, contract: authoring.contract }, m);
    setMembers(next.members);
  };
  const updateMember = (id, patch) => {
    snap();
    const next = window.MobKitFlowController.studioUpdateMemberPatch({ members, contract: authoring.contract }, id, patch);
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

  const gridState = window.MobKitFlowController.graphGridState({ instances: state.instances, gridBase: grid });
  const g = gridState.grid;
  const totalW = gridState.totalW;
  const totalH = gridState.totalH;

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

  const onNodeDown = (e, inst) => {
    if (e.target.classList.contains("port")) return;
    e.stopPropagation();
    selectInstance(inst.id);
    const w = screenToWorld(e.clientX, e.clientY);
    const b = window.MobKitFlowController.graphNodeBox(g, inst);
    setDrag({ instId: inst.id, dx: w.x - b.x, dy: w.y - b.y, origCol: inst.col, origRow: inst.row });
  };
  const onPortDown = (e, inst) => {
    e.stopPropagation();
    const p = window.MobKitFlowController.graphPortOut(g, inst);
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
  const openSourceFromEvent = (e) => {
    const sourceEl = e.target?.closest?.(".node--source-file");
    if (!sourceEl || !hostRef.current?.contains(sourceEl)) return false;
    e.preventDefault();
    e.stopPropagation();
    onOpenSourceFile?.({
      id: sourceEl.dataset.instId || "",
      kind: sourceEl.dataset.kind || "source",
    });
    return true;
  };
  const onHostMouseDownCapture = (e) => {
    if (e.button !== 0) return;
    openSourceFromEvent(e);
  };
  const onHostKeyDownCapture = (e) => {
    if (e.key !== "Enter" && e.key !== " ") return;
    openSourceFromEvent(e);
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
        const cell = window.MobKitFlowController.graphDragCellAt(g, w, drag);
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
        const cell = window.MobKitFlowController.graphDragCellAt(g, w, drag);
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

  const cellRows = window.MobKitFlowController.graphCellCanvasRows({ grid: g, instances: state.instances, hoverCell });
  const headerRows = window.MobKitFlowController.graphGridHeaderCanvasRows({ grid: g });
  const cells = cellRows.map(row => (
    <div key={row.key}
      className={row.className}
      style={row.style}
      onMouseDown={(e) => e.stopPropagation()}
      onClick={(e) => { e.stopPropagation(); if (!row.occupied) onRequestAdd(row.col, row.row); }}
    >
      {row.addVisible && <div className="cell__add"><span className="cell__plus">+</span></div>}
    </div>
  ));
  const colHeads = headerRows.columns.map(row => (
    <div key={row.key} className={row.className} style={row.style}>{row.label}</div>
  ));
  const rowHeads = headerRows.rows.map(row => (
    <div key={row.key} className={row.className} style={row.style}>{row.label}</div>
  ));

  const frameEls = state.frames.map(fr => {
    const frameState = window.MobKitFlowController.graphFrameCanvasState({ frame: fr, grid: g });
    return (
      <React.Fragment key={frameState.id}>
        <div className="frame" style={frameState.frameStyle} />
        <div className="frame-label" style={frameState.labelStyle}>{frameState.label}</div>
      </React.Fragment>
    );
  });

  const edgeEls = state.edges.map(edge => {
    const fi = state.instances.find(i => i.id === edge.from);
    const ti = state.instances.find(i => i.id === edge.to);
    if (!fi || !ti) return null;
    const a = window.MobKitFlowController.graphPortOut(g, fi), b = window.MobKitFlowController.graphPortIn(g, ti);
    const d = window.MobKitFlowController.graphEdgePath(a, b);
    const mid = window.MobKitFlowController.graphEdgeMidpoint(a, b);
    const isActive = activeStepId === edge.from;
    const isSelected = selection.kind === "edge" && selection.id === edge.id;
    const edgeState = window.MobKitFlowController.graphEdgeCanvasState({
      edge,
      to: ti,
      active: isActive,
      selected: isSelected,
      edgeStyle,
      contract,
      graphView: canvasView,
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
          contract={contract}
          graphView={canvasView}
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
      onMouseDownCapture={onHostMouseDownCapture}
      onKeyDownCapture={onHostKeyDownCapture}
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
          {conn && <path d={window.MobKitFlowController.graphEdgePath(conn.from, conn.to)} className="edge-line is-ghost" markerEnd="url(#arr-acc)" />}
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
  const b = window.MobKitFlowController.graphNodeBox(g, inst);

  if (nodeState.isTerminal) {
    const openSourceFile = (event) => {
      if (!nodeState.isSourceFile) return;
      event.stopPropagation();
      onOpenSourceFile?.(inst);
    };
    if (nodeState.isSourceFile) {
      return (
        <a href="#mobkit-graph-source" data-inst-id={inst.id}
          className={"node node--term node--source-file" + (selected ? " is-selected" : "") + (activeStep ? " is-active-step" : "") + (hoverIn ? " is-target" : "")}
          data-kind={nodeState.dataKind}
          role={nodeState.role}
          tabIndex={nodeState.tabIndex}
          aria-label={nodeState.ariaLabel}
          style={{ left: b.x, top: b.y, width: b.w, height: b.h }}
          onMouseDown={(e) => {
            e.stopPropagation();
          }}
        >
          <span className="source-file__glyph">{nodeState.sourceGlyph}</span>
          <span className="source-file__label">{nodeState.title}</span>
        </a>
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

function GateView({ g, inst, selected, activeStep, hoverIn, onMouseDown, onPortDown, portDragTitle, state, contract, graphView }) {
  const b = window.MobKitFlowController.graphNodeBox(g, inst);
  const gateState = window.MobKitFlowController.graphGateCanvasState({ inst, edges: state.edges, contract, graphView });

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
