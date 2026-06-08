/* global React */
// BuilderView — linear top→bottom mob flow builder. Emits a mob flow
// (the [flows.<id>.root] FrameSpec of a mobpack's mob.toml).
//
// Real meerkat model, not generic SaaS actions:
//   - A flow node is a MEMBER TURN: a profile/role runs with an instruction
//     `message`, gated by `depends_on`.
//   - Flow primitives: repeat-until (loop_id/body/until/max_iterations),
//     branch (condition + collection over downstream), parallel fan-out/
//     fan-in (dispatch_mode + collection_policy).
//   - "Input" is the mob's ingress: the task the run is started with
//     (`rkat mob deploy <pack> "task…"` / `run_flow(input)`), plus any typed
//     input fields. It is NOT an event source — schedulers/event sources live
//     outside the mobpack.
//
// Entry nodes of the flow are simply those with depends_on = [] (the steps
// right after Input).
// The vertical START → step → step layout mirrors how a FrameSpec reads
// top-to-bottom along its depends_on chain.

function CondValue({ field, value, onChange }) {
  const control = window.MobKitFlowController.conditionValueControl(field, value);
  if (control.kind === "enum") {
    return (
      <select className="field__select bld-cond__val" value={control.value} onChange={e => onChange(e.target.value)}>
        {control.optionRows.map(row => <option key={row.value || "blank"} value={row.value}>{row.label}</option>)}
      </select>
    );
  }
  if (control.kind === "boolean") {
    return (
      <select className="field__select bld-cond__val" value={control.value} onChange={e => onChange(e.target.value)}>
        {control.optionRows.map(row => <option key={row.value || "blank"} value={row.value}>{row.label}</option>)}
      </select>
    );
  }
  return <input className="field__input bld-cond__val" placeholder={control.placeholder} value={control.value} onChange={e => onChange(e.target.value)} />;
}

function InputParamField({ param, normalizeName, onRename, onChange, onDelete, contract }) {
  const fieldState = window.MobKitFlowController.inputParamFieldControlState(param, contract);
  const values = fieldState.enumValues;
  const previousNameRef = React.useRef(null);
  const typeState = fieldState.typeState;
  return (
    <div className="schema-field">
      <input
        className="sb-input sb-col--name"
        value={param.name || ""}
        onFocus={() => { previousNameRef.current = param.name || ""; }}
        onChange={e => onChange({ name: e.target.value })}
        onBlur={e => {
          const previousName = previousNameRef.current ?? param.name;
          previousNameRef.current = null;
          onRename?.(normalizeName(e.target.value), previousName);
        }}
        placeholder={fieldState.namePlaceholder}
      />
      <select
        className="sb-select sb-col--type"
        value={typeState.type}
        onChange={e => {
          onChange(window.MobKitFlowController.schemaLikeFieldTypePatch(param, e.target.value, contract));
        }}
      >
        {typeState.typeOptions.map(option => (
          <option key={option.value} value={option.value} disabled={option.disabled}>{option.label}</option>
        ))}
      </select>
      {typeState.selectedType?.reason && <div className="hint__line" style={{ color: "var(--warn)" }}>{typeState.selectedType.reason}</div>}
      <label className="sb-col--req sb-checkbox">
        <input type="checkbox" checked={param.required !== false} onChange={e => onChange({ required: e.target.checked })} />
      </label>
      <input
        className="sb-input sb-col--desc"
        value={param.description || ""}
        onChange={e => onChange({ description: e.target.value })}
        placeholder={fieldState.descriptionPlaceholder}
      />
      <button className="sb-del" onClick={onDelete} title={fieldState.removeTitle}>×</button>
      {param.type === "enum" && (
        <div className="sb-enum">
          <span className="sb-enum__label">{fieldState.enumLabel}</span>
          <div className="sb-enum__chips">
            {values.map((value, index) => (
              <span key={index} className="chip">
                <input
	                  className="chip__input"
	                  value={value}
	                  onChange={e => onChange(window.MobKitFlowController.enumValueDraftPatch(param, index, e.target.value))}
	                  onBlur={e => onChange(window.MobKitFlowController.enumValueCommitPatch(param, index, e.target.value))}
	                />
	                <button className="chip__x" onClick={() => onChange(window.MobKitFlowController.enumValueDeletePatch(param, index))}>×</button>
	              </span>
	            ))}
	            <button className="chip chip--add" onClick={() => onChange(window.MobKitFlowController.enumValueAddPatch(param, fieldState.enumAddValue))}>{fieldState.enumAddLabel}</button>
          </div>
        </div>
      )}
    </div>
  );
}

function BranchConditionEditor({ index, branch, options, schemas, onChange, contract, basicView = null }) {
  const conditionState = window.MobKitFlowController.basicBranchConditionControlState({
    branch: { ...branch, index },
    options,
    schemas,
    contract,
    basicView,
  });
  return (
    <div className="bld-branch-card">
      <div className="bld-branch-card__head">{conditionState.rowTitle}</div>
      {!conditionState.hasConditionOptions ? (
        <div className="bld-hint" style={{ color: "var(--warn)" }}>{conditionState.emptyHint}</div>
      ) : (
        <>
          <div className="bld-cond">
            <select className="field__select" value={conditionState.cond.stepId || ""} onChange={e => onChange(window.MobKitFlowController.basicConditionSourcePatch(options, e.target.value, { includeNamespace: true }))}>
              <option value="">{conditionState.sourcePlaceholder}</option>
              {conditionState.sourceOptions.map(option => <option key={option.value} value={option.value}>{option.label}</option>)}
            </select>
            <select className="field__select" value={conditionState.cond.field || ""} onChange={e => onChange(window.MobKitFlowController.basicConditionFieldPatch(e.target.value, conditionState.fieldOptions))} disabled={!conditionState.fields.length}>
              <option value="">{conditionState.fieldPlaceholder}</option>
              {conditionState.fieldOptions.map(option => <option key={option.field.id || option.value} value={option.value}>{option.label}</option>)}
            </select>
            <select className="field__select bld-cond__op" value={conditionState.operatorValue} onChange={e => onChange(window.MobKitFlowController.basicConditionOperatorPatch(e.target.value, contract))}>
              {conditionState.operatorOptions.map(option => (
                <option key={option.value} value={option.value} disabled={option.disabled}>{option.label}</option>
              ))}
            </select>
            <CondValue field={conditionState.field} value={conditionState.cond.val} onChange={v => onChange(window.MobKitFlowController.basicConditionValuePatch(v))} />
          </div>
          <div className="bld-cond__preview">{conditionState.previewPrefix} <code>{conditionState.previewLabel}</code></div>
        </>
      )}
    </div>
  );
}

function BuilderView({ studio, mode = "build", flow: flowProp, setFlow: setFlowProp, sel: selProp, setSel: setSelProp, onShowSource, sourceOpen = false, sourceDocument = null, sourceBusy = false, onCloseSource, contract, toolCatalog = [], sourceView = null, basicView = null, launchView = null }) {
  const members = studio?.members || [];
  const [flowLocal, setFlowLocal] = React.useState(() => window.MobKitFlowController.emptyAuthoringFlowState());
  const [selLocal, setSelLocal] = React.useState(null);
  const flow = flowProp || flowLocal;
  const setFlow = setFlowProp || setFlowLocal;
  const sel = selProp !== undefined ? selProp : selLocal;
  const setSel = setSelProp || setSelLocal;
  const [picker, setPicker] = React.useState({ open: false });
  const [view, setView] = React.useState({ scale: 1, tx: 0, ty: 0 });
  const hostRef = React.useRef(null);
  const panRef = React.useRef(null);
  const isFlow = mode === "flow";
  const viewState = window.MobKitFlowController.basicEditorViewState(basicView);
  const canvasView = Math.abs(view.ty) > 1200
    ? { ...view, ty: 0 }
    : view;

  const update = (id, patch) =>
    setFlow(f => window.MobKitFlowController.flowStepUpdatePatch(f, id, patch, { members }));
  const reconcileInputParamReferences = (oldName, newName) => {
    if (!window.MobKitFlowController?.reconcileInputParamReferences) return;
    setFlow(current => {
      const reconciled = window.MobKitFlowController.reconcileInputParamReferences({
        flow: current,
        edges: studio?.edges || [],
        oldName,
        newName,
      });
      if (reconciled.edges !== studio?.edges && studio?.setEdges) {
        if (studio?.snap) studio.snap();
        studio.setEdges(reconciled.edges);
      }
      return reconciled.flow;
    });
  };
  const selStep = findStep(flow.steps, sel);

  const insertAt = (laneRef, pick) => {
    const newStep = window.MobKitFlowController.flowStepTemplate(pick, contract, { flow });
    if (!newStep) return;
    setFlow(f => window.MobKitFlowController.flowStepInsertPatch(f, laneRef, newStep, { members }));
    setSel(newStep.id);
    setPicker({ open: false });
  };
  const removeStep = (id) => {
    setFlow(f => window.MobKitFlowController.flowStepDeletePatch(f, id));
    setSel(null); setPicker({ open: false });
  };
  const openPicker = (laneRef) => setPicker({ open: true, at: laneRef });

  // pan / zoom
  const onWheel = (e) => {
    if (e.ctrlKey || e.metaKey) {
      e.preventDefault();
      const fz = Math.exp(-e.deltaY * 0.0015);
      setView(v => {
        const r = hostRef.current.getBoundingClientRect();
        const cx = e.clientX - r.left, cy = e.clientY - r.top;
        const next = Math.max(0.4, Math.min(2, v.scale * fz));
        const k = next / v.scale;
        return { scale: next, tx: cx - (cx - v.tx) * k, ty: cy - (cy - v.ty) * k };
      });
    } else {
      e.preventDefault();
      setView(v => ({ ...v, tx: v.tx - e.deltaX, ty: v.ty - e.deltaY }));
    }
  };
  React.useEffect(() => {
    const el = hostRef.current; if (!el) return;
    const h = (e) => onWheel(e);
    el.addEventListener("wheel", h, { passive: false });
    return () => el.removeEventListener("wheel", h);
  });
  const onHostDown = (e) => {
    if (e.target === hostRef.current || e.target.classList?.contains("bld-canvas")) {
      panRef.current = { sx: e.clientX, sy: e.clientY, tx: view.tx, ty: view.ty };
      const move = (ev) => setView(v => ({ ...v, tx: panRef.current.tx + (ev.clientX - panRef.current.sx), ty: panRef.current.ty + (ev.clientY - panRef.current.sy) }));
      const up = () => { window.removeEventListener("mousemove", move); window.removeEventListener("mouseup", up); };
      window.addEventListener("mousemove", move); window.addEventListener("mouseup", up);
      setSel(null); setPicker({ open: false });
    }
  };

  return (
    <div className={"builder" + (isFlow ? " builder--flow" : "")}>
      <div className="bld-stage" ref={hostRef} onMouseDown={onHostDown}>
        <div className="bld-canvas" style={{ transform: `translate(calc(-50% + ${canvasView.tx}px), ${canvasView.ty}px) scale(${canvasView.scale})` }}>
          <div className="bld-start">{viewState.startLabel}</div>
          <Lane studio={studio} mode={mode} steps={flow.steps} laneRef={{ lane: "main" }} sel={sel}
            contract={contract}
            basicView={basicView}
            setSel={(id) => { setSel(id); setPicker({ open: false }); }}
            openPicker={openPicker} />
        </div>

        <button className="bld-toml-toggle" onMouseDown={e => e.stopPropagation()} onClick={() => onShowSource && onShowSource()}>
          {"{ } mob.toml"}
        </button>
        <InlineSourceEditor
          open={sourceOpen}
          onClose={() => onCloseSource && onCloseSource()}
          state={sourceDocument}
          busy={sourceBusy}
          sourceView={sourceView}
        />

        <div className="zoom-controls" onMouseDown={e => e.stopPropagation()}>
          <button className="zoom-btn" onClick={() => setView(v => ({ ...v, scale: Math.max(0.4, v.scale / 1.2) }))}>−</button>
          <button className="zoom-btn zoom-btn--pct" onClick={() => setView({ scale: 1, tx: 0, ty: 0 })}>{Math.round(view.scale * 100)}%</button>
          <button className="zoom-btn" onClick={() => setView(v => ({ ...v, scale: Math.min(2, v.scale * 1.2) }))}>+</button>
        </div>
      </div>

      <aside className="bld-panel">
        {picker.open ? (
          <StepPicker
            members={members}
            isKickoff={picker.at?.lane === "main" && picker.at?.index === 0 && kickoffSlotEmpty(flow)}
            contract={contract}
            basicView={basicView}
            onPick={(pick) => insertAt(picker.at, pick)}
            onClose={() => setPicker({ open: false })}
          />
        ) : selStep ? (
          <StepInspector studio={studio} members={members} flow={flow} step={selStep} update={update} onDelete={() => removeStep(selStep.id)} contract={contract} toolCatalog={toolCatalog} basicView={basicView} launchView={launchView} onInputParamReferenceChange={reconcileInputParamReferences} />
        ) : (
          <EmptyPanel state={viewState} />
        )}
      </aside>
    </div>
  );
}

function Lane({ studio, mode, steps, laneRef, sel, setSel, openPicker, contract, basicView = null }) {
  const viewState = window.MobKitFlowController.basicEditorViewState(basicView);
  return (
    <div className="bld-lane">
      {steps.map((step, i) => (
        <React.Fragment key={step.id}>
        <StepCard studio={studio} step={step} index={i} selected={sel === step.id} onSelect={() => setSel(step.id)} contract={contract} basicView={basicView} />
          {step.type === "branch" || step.type === "parallel" ? (
            <>
              <Fork studio={studio} mode={mode} step={step} sel={sel} setSel={setSel} openPicker={openPicker} contract={contract} basicView={basicView} />
              <InsertBtn mode={mode} mid={i < steps.length - 1} title={viewState.addStepTitle} onClick={() => openPicker({ ...laneRef, index: i + 1 })} />
            </>
          ) : step.type === "repeat" ? (
            <>
              <RepeatBody studio={studio} mode={mode} step={step} sel={sel} setSel={setSel} openPicker={openPicker} contract={contract} basicView={basicView} />
              <InsertBtn mode={mode} mid={i < steps.length - 1} title={viewState.addStepTitle} onClick={() => openPicker({ ...laneRef, index: i + 1 })} />
            </>
          ) : (
            <InsertBtn mode={mode} mid={i < steps.length - 1} title={viewState.addStepTitle} onClick={() => openPicker({ ...laneRef, index: i + 1 })} />
          )}
        </React.Fragment>
      ))}
      {steps.length === 0 && <InsertBtn mode={mode} title={viewState.addStepTitle} onClick={() => openPicker({ ...laneRef, index: 0 })} />}
    </div>
  );
}

function Fork({ studio, mode, step, sel, setSel, openPicker, contract, basicView = null }) {
  const forkState = window.MobKitFlowController.basicForkCanvasState({ step, contract });
  const viewState = window.MobKitFlowController.basicEditorViewState(basicView);
  return (
    <div className={forkState.className}>
      <div className="bld-fork__bar" />
      {forkState.showRail && <div className="bld-fork__rail" />}
      <div className="bld-fork__lanes">
        {forkState.lanes.map(l => (
          <div className="bld-fork__lane" key={l.id}>
            <div className="bld-fork__drop" />
            <div className="bld-fork__label">{l.label}</div>
            <div className="bld-fork__drop" />
            {l.steps.length === 0
              ? <InsertBtn mode={mode} title={viewState.addStepTitle} onClick={() => openPicker({ lane: "branch", parentId: step.id, branchId: l.id, index: 0 })} />
              : <Lane studio={studio} mode={mode} steps={l.steps} laneRef={{ lane: "branch", parentId: step.id, branchId: l.id }} sel={sel} setSel={setSel} openPicker={openPicker} contract={contract} basicView={basicView} />}
            {forkState.isParallel && <div className="bld-fork__drop" />}
          </div>
        ))}
      </div>
      {forkState.isParallel ? (
        <>
          {forkState.showRail && <div className="bld-fork__rail bld-fork__rail--join" />}
          <div className="bld-fork__bar" />
          <div className="bld-join">{forkState.joinLabel}</div>
        </>
      ) : (
        // Branch paths reconverge to a single downstream column so the
        // following main-lane step connects cleanly (no diagonal jump).
        forkState.showRail && <div className="bld-fork__rail bld-fork__rail--join" />
      )}
    </div>
  );
}

function RepeatBody({ studio, mode, step, sel, setSel, openPicker, contract, basicView = null }) {
  const repeatState = window.MobKitFlowController.basicRepeatCanvasState({ step, members: studio?.members || [], contract, basicView });
  const viewState = window.MobKitFlowController.basicEditorViewState(basicView);
  return (
    <div className="bld-repeat">
      <div className="bld-fork__bar" />
      <div className="bld-loop">
        <div className="bld-loop__rail">
          <span className="bld-loop__rail-glyph">↻</span>
        </div>
        <div className="bld-loop__frame">
          <div className="bld-loop__head">
            <span className="bld-loop__badge">{viewState.loopBadge}</span>
            <span className="bld-loop__meta">{repeatState.whileLabel} <strong>{repeatState.notLabel}</strong> ({repeatState.conditionLabel}) · {repeatState.maxIterationsLabel}</span>
          </div>
          {step.steps.length === 0
            ? <InsertBtn mode={mode} title={viewState.addStepTitle} onClick={() => openPicker({ lane: "branch", parentId: step.id, branchId: "body", index: 0 })} />
            : <Lane studio={studio} mode={mode} steps={step.steps} laneRef={{ lane: "branch", parentId: step.id, branchId: "body" }} sel={sel} setSel={setSel} openPicker={openPicker} contract={contract} basicView={basicView} />}
          <div className="bld-loop__back">{repeatState.loopBackLabel}</div>
        </div>
      </div>
      <div className="bld-loop__exit">{repeatState.exitLabel}</div>
    </div>
  );
}

function StepCard({ studio, step, index, selected, onSelect, contract, basicView = null }) {
  const cardState = window.MobKitFlowController.basicStepCardState({ step, members: studio?.members || [], contract, basicView });

  return (
    <div
      className={"bld-card" + (selected ? " is-selected" : "") + (!cardState.configured ? " is-empty" : "") + (cardState.isFlowCard ? " bld-card--flow" : "")}
      onMouseDown={(e) => { e.stopPropagation(); onSelect(); }}
    >
      <div className="bld-card__head">
        <span className="bld-card__index">{index}.</span>
        {cardState.icon && <span className={"bld-card__icon tint--" + cardState.iconTint}>{cardState.icon}</span>}
        <span className="bld-card__title">{cardState.title}</span>
      </div>
      {cardState.configured ? (
        <div className="bld-card__body"><span className="bld-card__desc">{cardState.desc}</span></div>
      ) : (
        <div className="bld-card__skeleton"><span /><span /></div>
      )}
    </div>
  );
}

function InsertBtn({ onClick, mid, mode, title = "" }) {
  if (mode === "flow") {
    return (
      <div className={"bld-insert bld-insert--conn" + (mid ? " bld-insert--mid" : "")}>
        <div className="bld-insert__line" />
        <span className="bld-insert__dot" />
        {mid && <div className="bld-insert__line" />}
      </div>
    );
  }
  return (
    <div className={"bld-insert" + (mid ? " bld-insert--mid" : "")}>
      <div className="bld-insert__line" />
      <button className="bld-insert__btn" onMouseDown={(e) => { e.stopPropagation(); onClick(); }} title={title}>+</button>
      {mid && <div className="bld-insert__line" />}
    </div>
  );
}

// ── Picker ──
function StepPicker({ members, isKickoff, contract, onPick, onClose, basicView = null }) {
  const [q, setQ] = React.useState("");
  const pickerState = window.MobKitFlowController.basicStepPickerState({ members, contract, query: q, isKickoff, basicView });
  if (pickerState.mode === "kickoff") {
    return (
      <div className="bld-panel__inner">
        <PanelHead title={pickerState.title} sub={pickerState.sub} onClose={onClose} />
        <div className="bld-hint">{pickerState.kickoffHint}</div>
      </div>
    );
  }
  return (
    <div className="bld-panel__inner">
      <PanelHead title={pickerState.title} sub={pickerState.sub} onClose={onClose} />
      <div className="bld-search">
        <span className="bld-search__icon">{pickerState.searchIcon}</span>
        <input className="bld-search__input" placeholder={pickerState.searchPlaceholder} value={q} onChange={e => setQ(e.target.value)} autoFocus />
      </div>

      <div className="bld-opts__group">{pickerState.membersLabel}</div>
      <div className="bld-opts">
        {pickerState.memberRows.map(row => (
          <button key={row.id} className="bld-opt" onClick={() => onPick(row.pick)}>
            <span className={"bld-opt__icon tint--" + row.iconTint}>{row.icon}</span>
            <span className="bld-opt__text">
              <span className="bld-opt__label">{row.name}</span>
              <span className="bld-opt__sub">{row.sub}</span>
            </span>
          </button>
        ))}
        {!pickerState.hasConfiguredMembers && <div className="bld-hint" style={{ padding: "4px 8px" }}>{pickerState.emptyMembersHint}</div>}
      </div>

      <div className="bld-opts__group">{pickerState.flowLabel}</div>
      <div className="bld-opts">
        {pickerState.primitiveRows.map(row => (
          <button key={row.id} className="bld-opt" onClick={() => onPick(row.pick)}>
            <span className={"bld-opt__icon tint--" + row.tint}>{row.glyph}</span>
            <span className="bld-opt__text">
              <span className="bld-opt__label">{row.label}{row.isNew && <span className="bld-opt__new">{pickerState.newBadgeLabel}</span>}</span>
              <span className="bld-opt__sub">{row.sub}</span>
            </span>
          </button>
        ))}
      </div>
    </div>
  );
}

// ── Inspector ──
function StepInspector({ studio, members, flow, step, update, onDelete, contract, toolCatalog, basicView = null, launchView = null, onInputParamReferenceChange }) {
  const viewState = window.MobKitFlowController.basicEditorViewState(basicView);
  if (step.type === "input") {
    const inputState = window.MobKitFlowController.basicInputControlState(step, contract);
    const params = inputState.params;
    const updateParam = (id, patch) => update(step.id, window.MobKitFlowController.inputParamUpdatePatch(params, id, patch, contract));
    const deleteParam = (id) => {
      const result = window.MobKitFlowController.inputParamDeletePatch(params, id, contract);
      update(step.id, result.patch);
      if (result.removed?.name) onInputParamReferenceChange?.(result.removed.name, "");
    };
    const renameParam = (id, rawName, previousName) => {
      const result = window.MobKitFlowController.inputParamRenamePatch(params, id, rawName, contract);
      update(step.id, result.patch);
      if (String(previousName || "").trim() !== result.name) {
        onInputParamReferenceChange?.(previousName, result.name);
      }
    };
    const addParam = () => {
      const result = window.MobKitFlowController.inputParamAddPatch(params, contract);
      if (result.ok === false) return;
      update(step.id, result.patch);
    };
    return (
      <div className="bld-panel__inner">
        <PanelHead icon={inputState.panelIcon} iconTint="member" title={inputState.panelTitle} sub={inputState.panelSub} onClose={onDelete} deleteMode />
        <Field label={inputState.taskLabel}><textarea className="field__textarea" rows={3} placeholder={inputState.taskPlaceholder} value={step.task || ""} onChange={e => update(step.id, window.MobKitFlowController.flowStepTaskPatch(e.target.value))} /></Field>
        <div className="section">
          <div className="row row--between" style={{ marginBottom: 6 }}>
            <div className="section__title">{inputState.paramsTitle}</div>
            <button className="btn btn--ghost btn--sm" onClick={addParam}>{inputState.addParamLabel}</button>
          </div>
          <div className="schema-builder">
            <div className="schema-builder__header">
              {inputState.headerRows.map(row => <span key={row.key} className={row.className}>{row.label}</span>)}
            </div>
            {params.map(param => (
              <InputParamField
                key={param.id}
                param={param}
                normalizeName={(raw) => window.MobKitFlowController.uniqueInputParamName(params, raw, param.id)}
                onRename={(raw, previousName) => renameParam(param.id, raw, previousName)}
                onChange={(patch) => updateParam(param.id, patch)}
                onDelete={() => deleteParam(param.id)}
                contract={contract}
              />
            ))}
            {params.length === 0 && (
              <div className="schema-builder__empty">
                {inputState.emptyParamsParts.map(part => (
                  part.kind === "code"
                    ? <code key={part.key}>{part.text}</code>
                    : <React.Fragment key={part.key}>{part.text}</React.Fragment>
                ))}
              </div>
            )}
          </div>
        </div>
        <PanelTips title={viewState.tipsTitle} items={inputState.tips} />
      </div>
    );
  }

  if (step.type === "branch" || step.type === "parallel") {
    const branchState = window.MobKitFlowController.basicBranchParallelControlState({
      step,
      flow,
      members: studio?.members || [],
      contract,
      basicView,
    });
    const setBranchCondition = (branch, patch) => {
      update(step.id, window.MobKitFlowController.basicBranchConditionPatch(step, branch.id, patch, contract));
    };
    const addBranch = () => update(step.id, window.MobKitFlowController.basicBranchAddPatch(step, { flow }));
    return (
      <div className="bld-panel__inner">
        <PanelHead icon={branchState.panelIcon} iconTint="member" title={branchState.panelTitle} sub={branchState.panelSub} onClose={onDelete} deleteMode />
        <Field label={branchState.controllerLabel}>
          <select className="field__select" value={branchState.controllerRole} onChange={e => update(step.id, window.MobKitFlowController.flowStepControllerRolePatch(e.target.value, members))}>
            <option value="">{branchState.controllerPlaceholderLabel}</option>
            {branchState.memberOptions.map(option => <option key={option.value} value={option.value}>{option.label}</option>)}
          </select>
        </Field>
        {!branchState.controllerRole && <div className="bld-hint" style={{ marginTop: 8 }}>{branchState.emptyControllerHint}</div>}
        {!branchState.isParallel && <>
          <div className="bld-section-label">{branchState.branchConditionTitle}</div>
          <div className="bld-hint">{branchState.branchConditionIntro}</div>
          {step.branches.map((b, i) => (
            <BranchConditionEditor
              key={b.id}
              index={i}
              branch={b}
              options={branchState.conditionOptions}
              schemas={studio?.schemas || []}
              onChange={(patch) => setBranchCondition(b, patch)}
              contract={contract}
              basicView={basicView}
            />
          ))}
          <button className="bld-add-row" onClick={addBranch}>{branchState.addBranchLabel}</button>
          <div className="bld-branch-card bld-branch-card--fallback"><div className="bld-branch-card__head">{branchState.fallbackTitle}</div><div className="bld-hint">{branchState.fallbackHint}</div></div>
        </>}
        {branchState.isParallel && <>
          <Field label={branchState.dispatchLabel}><select className="field__select" value={branchState.dispatchValue} onChange={e => update(step.id, window.MobKitFlowController.flowStepParallelDispatchPatch(e.target.value, contract))}>
            {branchState.dispatchOptions.map(option => (
              <option key={option.value} value={option.value} disabled={option.disabled}>{option.label}</option>
            ))}
          </select></Field>
          {branchState.selectedDispatch?.reason && <div className="bld-hint" style={{ color: "var(--warn)" }}>{branchState.selectedDispatch.reason}</div>}
          <Field label={branchState.collectionLabel}><select className="field__select" value={branchState.collectionValue} onChange={e => update(step.id, window.MobKitFlowController.flowStepCollectionPatch(e.target.value, contract))}>
            {branchState.collectionOptions.map(option => (
              <option key={option.value} value={option.value} disabled={option.disabled}>{option.label}</option>
            ))}
          </select></Field>
          {branchState.selectedCollection?.reason && <div className="bld-hint" style={{ color: "var(--warn)" }}>{branchState.selectedCollection.reason}</div>}
          {branchState.showQuorum && (
            <Field label={branchState.quorumLabel}><input className="field__input" type="number" min="1" value={step.quorum ?? ""} placeholder={branchState.quorumPlaceholder} onChange={e => update(step.id, window.MobKitFlowController.flowStepQuorumPatch(e.target.value))} /></Field>
          )}
          <button className="bld-add-row" onClick={addBranch}>{branchState.addBranchLabel}</button>
        </>}
        <Field label={branchState.dependencyLabel}><select className="field__select" value={branchState.dependencyValue} onChange={e => update(step.id, window.MobKitFlowController.flowStepDependencyModePatch(e.target.value, contract))}>
          {branchState.dependencyOptions.map(option => (
            <option key={option.value} value={option.value} disabled={option.disabled}>{option.label}</option>
          ))}
        </select></Field>
        {branchState.selectedDependency?.reason && <div className="bld-hint" style={{ color: "var(--warn)" }}>{branchState.selectedDependency.reason}</div>}
      </div>
    );
  }

  if (step.type === "repeat") {
    const repeatState = window.MobKitFlowController.basicRepeatControlState({
      step,
      members: studio?.members || [],
      schemas: studio?.schemas || [],
      contract,
      basicView,
    });
    const setCond = (patch) => update(step.id, window.MobKitFlowController.flowStepRepeatConditionPatch(step, patch));
    return (
      <div className="bld-panel__inner">
        <PanelHead icon={repeatState.panelIcon} iconTint="member" title={repeatState.panelTitle} sub={repeatState.panelSub} onClose={onDelete} deleteMode />
        <Field label={repeatState.loopIdLabel}><input className="field__input field__input--mono" value={step.loopId || ""} placeholder={repeatState.loopIdPlaceholder} onChange={e => update(step.id, window.MobKitFlowController.flowStepLoopIdPatch(e.target.value))} /></Field>

        <div className="bld-section-label" style={{ marginTop: 16 }}>{repeatState.conditionTitle}</div>
        <div className="bld-hint">{repeatState.conditionIntro}</div>
        {!repeatState.hasBodyMembers ? (
          <div className="bld-hint" style={{ marginTop: 10, color: "var(--warn)" }}>{repeatState.emptyBodyHint}</div>
        ) : (
          <div className="bld-cond">
            <select className="field__select" value={repeatState.cond.stepId || ""} onChange={e => setCond(window.MobKitFlowController.basicConditionSourcePatch(repeatState.bodyMembers, e.target.value))}>
              <option value="">{repeatState.memberPlaceholderLabel}</option>
              {repeatState.bodyMemberOptions.map(option => <option key={option.value} value={option.value}>{option.label}</option>)}
            </select>
            <select className="field__select" value={repeatState.cond.field || ""} onChange={e => setCond(window.MobKitFlowController.basicConditionFieldPatch(e.target.value, repeatState.fieldOptions))} disabled={!repeatState.condSchema}>
              <option value="">{repeatState.fieldPlaceholder}</option>
              {repeatState.fieldOptions.map(option => <option key={option.field.id || option.value} value={option.value}>{option.label}</option>)}
            </select>
            <select className="field__select bld-cond__op" value={repeatState.operatorValue} onChange={e => setCond(window.MobKitFlowController.basicConditionOperatorPatch(e.target.value, contract))}>
              {repeatState.operatorOptions.map(option => (
                <option key={option.value} value={option.value} disabled={option.disabled}>{option.label}</option>
              ))}
            </select>
            <CondValue field={repeatState.condField} value={repeatState.cond.val} onChange={v => setCond(window.MobKitFlowController.basicConditionValuePatch(v))} />
          </div>
        )}
        <div className="bld-cond__preview">{repeatState.previewLabel} <code>{repeatState.repeatUntilExpression || repeatState.previewFallback}</code></div>

        <Field label={repeatState.iterationInputLabel}>
          <select className="field__select" value={repeatState.iterationInputValue} onChange={e => update(step.id, window.MobKitFlowController.flowStepIterationInputPatch(e.target.value, contract))}>
            {repeatState.iterationInputOptions.map(option => (
              <option key={option.value} value={option.value} disabled={option.disabled}>{option.label}</option>
            ))}
          </select>
        </Field>
        {repeatState.selectedIterationInput?.reason && <div className="bld-hint" style={{ color: "var(--warn)" }}>{repeatState.selectedIterationInput.reason}</div>}

        <Field label={repeatState.maxIterationsLabel}><input className="field__input" type="number" min="1" placeholder={repeatState.maxIterationsPlaceholder} value={step.maxIterations ?? ""} onChange={e => update(step.id, window.MobKitFlowController.flowStepMaxIterationsPatch(e.target.value))} /></Field>
        <PanelTips title={viewState.tipsTitle} items={repeatState.tips} />
      </div>
    );
  }

  // member step
  const memberStepState = window.MobKitFlowController.basicMemberStepControlState({
    step,
    flow,
    members,
    contract,
    basicView,
    launchView,
  });
  const m = memberStepState.member;
  const launchState = memberStepState.launchState;
  return (
    <div className="bld-panel__inner">
      <PanelHead icon="◆" iconTint="accent" title={memberStepState.panelTitle} sub={memberStepState.panelSub} onClose={onDelete} deleteMode />
      <Field label={memberStepState.memberFieldLabel}><select className="field__select" value={step.role || ""} onChange={e => update(step.id, window.MobKitFlowController.flowStepMemberRolePatch(e.target.value, members))}>
        <option value="">{memberStepState.memberPlaceholderLabel}</option>
        {memberStepState.memberOptions.map(option => <option key={option.value} value={option.value}>{option.label}</option>)}
      </select></Field>
      <Field label={launchState.launchTitle}>
        <select className="field__select" value={launchState.launchKind} onChange={e => {
          update(step.id, window.MobKitFlowController.launchModeKindPatch(step, e.target.value, contract, { firstForkSourceId: memberStepState.firstLaunchSourceId }));
        }}>
          {launchState.launchOptions.map(option => (
            <option key={option.value} value={option.value} disabled={option.disabled}>{option.label}</option>
          ))}
        </select>
      </Field>
      {launchState.selectedLaunchMode?.reason && <div className="bld-hint" style={{ color: "var(--warn)" }}>{launchState.selectedLaunchMode.reason}</div>}
      {launchState.launchKind === "Resume" && (
        <Field label={launchState.resumeSessionLabel}>
          <input className="field__input" value={launchState.launchMode.sessionId || ""} placeholder={launchState.resumeSessionPlaceholder} onChange={e => update(step.id, window.MobKitFlowController.launchModeSessionPatch(step, e.target.value, contract))} />
        </Field>
      )}
      {launchState.launchKind === "Fork" && (
        <>
          <Field label={launchState.forkSourceLabel}>
            <select className="field__select" value={launchState.launchMode.from || ""} onChange={e => update(step.id, window.MobKitFlowController.launchModeForkSourcePatch(step, e.target.value, contract, { sourceOptions: memberStepState.launchSourceOptions }))}>
              {memberStepState.launchSourceOptions.map(option => <option key={option.value} value={option.value}>{option.label}</option>)}
            </select>
          </Field>
          <Field label={launchState.forkContextLabel}>
            <select className="field__select" value={launchState.forkContextValue} onChange={e => update(step.id, window.MobKitFlowController.launchModeForkContextPatch(step, e.target.value, contract))}>
              {launchState.forkContextOptions.map(option => (
                <option key={option.value} value={option.value} disabled={option.disabled}>{option.label}</option>
              ))}
            </select>
          </Field>
          {launchState.selectedForkContext?.reason && <div className="bld-hint" style={{ color: "var(--warn)" }}>{launchState.selectedForkContext.reason}</div>}
        </>
      )}
      <Field label={launchState.budgetPolicyLabel}>
        <select className="field__select" value={launchState.budgetSplitPolicy.kind} onChange={e => update(step.id, window.MobKitFlowController.launchBudgetKindPatch(step, e.target.value, contract))}>
          {launchState.budgetOptions.map(option => (
            <option key={option.value} value={option.value} disabled={option.disabled}>{option.label}</option>
          ))}
        </select>
      </Field>
      {launchState.selectedBudgetPolicy?.reason && <div className="bld-hint" style={{ color: "var(--warn)" }}>{launchState.selectedBudgetPolicy.reason}</div>}
      {launchState.budgetSplitPolicy.kind === "Fixed" && (
        <Field label={launchState.fixedBudgetLabel}>
          <input className="field__input" type="number" min="1" step="1" value={launchState.fixedBudgetValue} onChange={e => update(step.id, window.MobKitFlowController.launchBudgetFixedLimitPatch(step, e.target.value, contract))} />
        </Field>
      )}
      <Field label={memberStepState.instructionLabel}><textarea className="field__textarea" rows={4} placeholder={memberStepState.instructionPlaceholder} value={step.instruction || ""} onChange={e => update(step.id, window.MobKitFlowController.flowStepInstructionPatch(e.target.value))} /></Field>
      <Field label={memberStepState.dispatchLabel}>
        <select className="field__select" value={memberStepState.dispatchValue} onChange={e => update(step.id, window.MobKitFlowController.flowStepDispatchModePatch(e.target.value, contract))}>
          {memberStepState.dispatchOptions.map(option => (
            <option key={option.value} value={option.value} disabled={option.disabled}>{option.label}</option>
          ))}
        </select>
      </Field>
      {memberStepState.selectedDispatch?.reason && <div className="bld-hint" style={{ color: "var(--warn)" }}>{memberStepState.selectedDispatch.reason}</div>}
      <Field label={memberStepState.collectionLabel}>
        <select className="field__select" value={memberStepState.collectionValue} onChange={e => update(step.id, window.MobKitFlowController.flowStepCollectionPatch(e.target.value, contract))}>
          {memberStepState.collectionOptions.map(option => (
            <option key={option.value} value={option.value} disabled={option.disabled}>{option.label}</option>
          ))}
        </select>
      </Field>
      {memberStepState.selectedCollection?.reason && <div className="bld-hint" style={{ color: "var(--warn)" }}>{memberStepState.selectedCollection.reason}</div>}
      {memberStepState.showQuorum && (
        <Field label={memberStepState.quorumLabel}>
          <input className="field__input" type="number" min="1" step="1" value={step.quorum ?? ""} placeholder={memberStepState.quorumPlaceholder} onChange={e => update(step.id, window.MobKitFlowController.flowStepQuorumPatch(e.target.value))} />
        </Field>
      )}
      <Field label={memberStepState.timeoutLabel}>
        <input className="field__input" type="number" min="1" step="1" placeholder={memberStepState.timeoutPlaceholder} value={step.timeoutMs ?? ""} onChange={e => update(step.id, window.MobKitFlowController.flowStepTimeoutPatch(e.target.value))} />
      </Field>
      <Field label={memberStepState.outputFormatLabel}>
        <select className="field__select" value={memberStepState.outputValue} onChange={e => update(step.id, window.MobKitFlowController.flowStepOutputFormatPatch(e.target.value, contract))}>
          {memberStepState.outputOptions.map(option => (
            <option key={option.value} value={option.value} disabled={option.disabled}>{option.label}</option>
          ))}
        </select>
      </Field>
      {memberStepState.selectedOutput?.reason && <div className="bld-hint" style={{ color: "var(--warn)" }}>{memberStepState.selectedOutput.reason}</div>}
      <ToolScopeEditor
        label={memberStepState.allowedToolsLabel}
        emptyLabel={memberStepState.allowedToolsEmptyLabel}
        member={m}
        selected={step.allowedTools || []}
        onChange={tools => update(step.id, window.MobKitFlowController.flowStepAllowedToolsPatch(tools, { member: m, toolCatalog }))}
        mode="member"
        toolCatalog={toolCatalog}
        basicView={basicView}
      />
      <ToolScopeEditor
        label={memberStepState.blockedToolsLabel}
        emptyLabel={memberStepState.blockedToolsEmptyLabel}
        member={m}
        selected={step.blockedTools || []}
        onChange={tools => update(step.id, window.MobKitFlowController.flowStepBlockedToolsPatch(tools, { toolCatalog }))}
        mode="catalog"
        toolCatalog={toolCatalog}
        basicView={basicView}
      />
      {memberStepState.schemaHint && (
        <div className="bld-hint" style={{ marginTop: 10 }}>
          {memberStepState.schemaHint.parts.map(part => (
            part.kind === "code"
              ? <code key={part.key}>{part.text}</code>
              : <React.Fragment key={part.key}>{part.text}</React.Fragment>
          ))}
        </div>
      )}
      <Field label={memberStepState.dependencyLabel}><select className="field__select" value={memberStepState.dependencyValue} onChange={e => update(step.id, window.MobKitFlowController.flowStepDependencyModePatch(e.target.value, contract))}>
        {memberStepState.dependencyOptions.map(option => (
          <option key={option.value} value={option.value} disabled={option.disabled}>{option.label}</option>
        ))}
      </select></Field>
      {memberStepState.selectedDependency?.reason && <div className="bld-hint" style={{ color: "var(--warn)" }}>{memberStepState.selectedDependency.reason}</div>}
    </div>
  );
}

function ToolScopeEditor({ label, emptyLabel, member, selected, onChange, mode = "member", toolCatalog = [], basicView = null }) {
  const field = mode === "catalog" ? "blockedTools" : "allowedTools";
  const scope = window.MobKitFlowController.stepToolScopeState({ member, selected, mode, toolCatalog, basicView });
  const remove = (id) => {
    const result = window.MobKitFlowController.stepToolScopeRemovePatch(selected, id, { field });
    if (result.patch) onChange(result.patch[field] || []);
  };
  const add = (id) => {
    const result = window.MobKitFlowController.stepToolScopeAddPatch(selected, id, { member, mode, toolCatalog, field, basicView });
    if (result.patch) onChange(result.patch[field] || []);
  };
  return (
    <Field label={label}>
      {scope.selectedTools.length === 0 ? (
        <div className="bld-hint">{emptyLabel}</div>
      ) : (
        scope.rows.map(row => {
          return (
            <div key={row.id} className={row.className}>
              <div>
                <div className="name">{row.name}</div>
                <div className="auth">{row.description}</div>
              </div>
              <button onClick={() => remove(row.id)}>{row.removeLabel}</button>
            </div>
          );
        })
      )}
      <select className="field__select" value={scope.addSelectValue} disabled={scope.disabled} onChange={e => { add(e.target.value); e.target.value = ""; }}>
        <option value={scope.addSelectValue}>{scope.addSelectPlaceholder}</option>
        {scope.addableRows.map(row => (
          <option key={row.id} value={row.value}>{row.optionLabel}</option>
        ))}
      </select>
    </Field>
  );
}

function PanelHead({ icon, iconTint, title, sub, onClose, deleteMode }) {
  return (
    <div className="bld-panel__head">
      <div className="bld-panel__head-main">
        {icon && <span className={"bld-panel__icon tint--" + (iconTint || "muted")}>{icon}</span>}
        <div><div className="bld-panel__title">{title}</div>{sub && <div className="bld-panel__sub">{sub}</div>}</div>
      </div>
      <button className="bld-panel__close" onClick={onClose} title={deleteMode ? "Delete step" : "Close"}>{deleteMode ? "🗑" : "✕"}</button>
    </div>
  );
}
function Field({ label, children }) {
  return <div className="field" style={{ marginTop: 14 }}><label className="field__label">{label}</label>{children}</div>;
}
function PanelTips({ title, items }) {
  return <div className="bld-tips"><div className="bld-tips__head">{title}</div><ul>{items.map((t, i) => <li key={i}>{t}</li>)}</ul></div>;
}
function EmptyPanel({ state }) {
  return (
    <div className="bld-panel__inner bld-panel__empty">
      <div className="bld-panel__title">{state.emptyPanelTitle}</div>
      <div className="bld-panel__sub">
        {state.emptyPanelSubtitleParts.map(part => {
          if (part.kind === "code") return <code key={part.key}>{part.text}</code>;
          if (part.kind === "strong") return <strong key={part.key}>{part.text}</strong>;
          return <React.Fragment key={part.key}>{part.text}</React.Fragment>;
        })}
      </div>
    </div>
  );
}

// ── step model ──
function kickoffSlotEmpty(flow) {
  // The first node is always the (fixed) Input ingress; clicking it shows the
  // Input explainer rather than the step picker.
  const first = flow.steps[0];
  return !!first && first.type === "input";
}
function childLanes(s) {
  if (s.type === "branch") return [...s.branches, { id: "fallback", steps: s.fallback }];
  if (s.type === "parallel") return s.branches;
  if (s.type === "repeat") return [{ id: "body", steps: s.steps, _direct: true }];
  return [];
}
function findStep(steps, id) {
  for (const s of steps) {
    if (s.id === id) return s;
    for (const l of childLanes(s)) { const r = findStep(l.steps, id); if (r) return r; }
  }
  return null;
}

window.BuilderView = BuilderView;
