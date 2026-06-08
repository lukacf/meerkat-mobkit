/* global React */
// Inspector — context-sensitive panel.
//
// Selection modes:
//   { kind: null }              → TemplateInspector
//   { kind: "instance", id }    → InstanceInspector  (template-only fields + member summary)
//   { kind: "edge",     id }    → EdgeInspector
//
// AddNodeMenu lets the user place existing real members. Flow controls are
// projected from the Basic Editor's deployable flow model.

function Inspector({ studio, selection, selectMember, selectInstance, clearSelection, template, templateSeed, templateView, flow, contract }) {
  const selectionState = window.MobKitFlowController.graphSelectionState({
    selection,
    instances: studio.instances,
    edges: studio.edges,
  });
  if (selectionState.kind === "instance") {
    if (!selectionState.instance) return <TemplateInspector studio={studio} template={template} templateSeed={templateSeed} templateView={templateView} />;
    return <InstanceInspector studio={studio} flow={flow} inst={selectionState.instance} selectMember={selectMember} clearSelection={clearSelection} contract={contract} />;
  }
  if (selectionState.kind === "edge") {
    if (!selectionState.edge) return <TemplateInspector studio={studio} template={template} templateSeed={templateSeed} templateView={templateView} />;
    return <EdgeInspector studio={studio} flow={flow} edge={selectionState.edge} clearSelection={clearSelection} contract={contract} />;
  }
  return <TemplateInspector studio={studio} template={template} templateSeed={templateSeed} templateView={templateView} />;
}

// ── Template (no selection) ──────────────────────────────────────
function TemplateInspector({ studio, template, templateSeed, templateView }) {
  const templateState = window.MobKitFlowController.graphTemplateInspectorState({ studio, template, templateSeed, templateView });
  return (
    <>
      <div className="inspector__head">
        <div className="inspector__eyebrow">{templateState.templateEyebrow}</div>
        <div className="inspector__title">{templateState.name}</div>
        <div className="inspector__id">{templateState.repo} · {templateState.version}</div>
      </div>
      <div className="inspector__body">
        <div className="section">
          <div className="section__title">{templateState.summaryTitle}</div>
          <dl className="kv">
            {templateState.summaryRows.map(row => (
              <React.Fragment key={row.key}>
                <dt>{row.label}</dt><dd>{row.value}</dd>
              </React.Fragment>
            ))}
          </dl>
        </div>
        <div className="section">
          <div className="section__title">{templateState.triggersTitle}</div>
          <dl className="kv">
            {templateState.triggerRows.map(row => (
              <React.Fragment key={row.key}>
                <dt>{row.label}</dt><dd>{row.value}</dd>
              </React.Fragment>
            ))}
          </dl>
        </div>
        <div className="section section--hint">
          <div className="hint__title">{templateState.quickStartTitle}</div>
          {templateState.quickStartRows.map(row => (
            <div className="hint__line" key={row.key}>
              {row.parts.map(part => {
                if (part.kind === "strong") return <strong key={part.key}>{part.text}</strong>;
                if (part.kind === "code") return <code key={part.key}>{part.text}</code>;
                return <React.Fragment key={part.key}>{part.text}</React.Fragment>;
              })}
            </div>
          ))}
        </div>
      </div>
    </>
  );
}

// ── Gate (fork / join / branch) ───────────────────────────────────
function GateInspector({ studio, flow, inst, clearSelection, contract }) {
  const change = (patch) => studio.updateInstance(inst.id, patch);
  const kind = inst.gateKind;
  const gateState = window.MobKitFlowController.graphGateControlState(inst, {
    edges: studio.edges,
    members: studio.members,
    contract,
  });
  const branchRows = kind === "branch"
    ? window.MobKitFlowController.graphBranchConditionRows({
      inst,
      edges: studio.edges,
      instances: studio.instances,
      members: studio.members,
      schemas: studio.schemas,
      flow,
      contract,
    })
    : [];

  return (
    <>
      <div className="inspector__head">
        <div className="row row--between">
          <div>
            <div className="inspector__eyebrow">{gateState.eyebrow}</div>
            <div className="inspector__title">{gateState.title}</div>
            <div className="inspector__id">{gateState.idLine}</div>
          </div>
          <button className="btn btn--ghost btn--sm" onClick={() => { studio.deleteInstance(inst.id); clearSelection(); }}>{gateState.deleteLabel}</button>
        </div>
      </div>
      <div className="inspector__body">
        <div className="section">
          <div className="section__title">{gateState.labelTitle}</div>
          <input className="field__input" value={inst.label} onChange={e => change(window.MobKitFlowController.graphInstanceLabelPatch(e.target.value))} />
        </div>
        <div className="section">
          <div className="section__title">{gateState.kindTitle}</div>
          <select className="field__select" value={gateState.gateKind} onChange={e => change(window.MobKitFlowController.graphGateKindPatch(e.target.value, contract))}>
            {gateState.gateKindOptions.map(option => (
              <option key={option.value} value={option.value} disabled={option.disabled}>{option.label}</option>
            ))}
          </select>
          {gateState.selectedGateKind?.reason && <div className="kv__hint" style={{ color: "var(--warn)" }}>{gateState.selectedGateKind.reason}</div>}
        </div>
        {kind === "join" && (
          <div className="section">
            <div className="section__title">{gateState.collectionTitle}</div>
            <select className="field__select" value={gateState.collection} onChange={e => {
              change(window.MobKitFlowController.graphJoinCollectionPatch(inst, e.target.value, {
                incomingCount: gateState.incoming.length,
                firstMemberId: gateState.firstMemberId,
                contract,
              }));
            }}>
              {gateState.collectionOptions.map(option => (
                <option key={option.value} value={option.value} disabled={option.disabled}>{option.label}</option>
              ))}
            </select>
            {gateState.selectedCollection?.reason && <div className="kv__hint" style={{ color: "var(--warn)" }}>{gateState.selectedCollection.reason}</div>}
            {gateState.collection === "quorum" && (
              <div className="row" style={{ marginTop: 8 }}>
                <input className="field__input field__input--num" type="number" min="1"
                  value={inst.quorum?.n || gateState.incoming.length || 1}
                  onChange={e => change(window.MobKitFlowController.graphJoinQuorumPatch(inst, e.target.value, gateState.incoming.length))} />
                <span className="kv__hint">{gateState.quorumIncomingLabel}</span>
              </div>
            )}
            {gateState.collection && gateState.collection !== "all" && (
              <div className="field" style={{ marginTop: 8 }}>
                <label className="field__label">{gateState.joinMemberLabel}</label>
                <select className="field__select" value={inst.controllerRole || ""} onChange={e => change(window.MobKitFlowController.graphJoinControllerRolePatch(e.target.value, studio.members))}>
                  <option value={gateState.joinMemberPlaceholderOption.value}>{gateState.joinMemberPlaceholderOption.label}</option>
                  {gateState.memberOptions.map(option => <option key={option.value} value={option.value}>{option.label}</option>)}
                </select>
                <div className="kv__hint">{gateState.joinMemberHint}</div>
              </div>
            )}
          </div>
        )}
        {kind === "fork" && (
          <div className="section">
            <div className="section__title">{gateState.dispatchTitle}</div>
            <select className="field__select" value={gateState.dispatch} onChange={e => change(window.MobKitFlowController.graphForkDispatchPatch(inst, e.target.value, contract))}>
              {gateState.dispatchOptions.map(option => (
                <option key={option.value} value={option.value} disabled={option.disabled}>{option.label}</option>
              ))}
            </select>
            {gateState.selectedDispatch?.reason && <div className="kv__hint" style={{ color: "var(--warn)" }}>{gateState.selectedDispatch.reason}</div>}
            <div className="kv__hint">{gateState.dispatchHint}</div>
          </div>
        )}
        {kind === "branch" && (
          <div className="section">
            <div className="section__title">{gateState.conditionsTitle}</div>
            {branchRows.length === 0 && <div className="kv__hint">{gateState.emptyBranchHint}</div>}
            {branchRows.map(row => {
              const e = row.edge;
              const setCondOwner = (instanceId) => studio.updateEdge(e.id, window.MobKitFlowController.graphEdgeConditionOwnerPatch(e, row.conditionOptions, instanceId, {
                defaultOperator: row.defaultOperator,
                forceLabel: true,
                includeKind: true,
              }));
              const setCondField = (field) => studio.updateEdge(e.id, window.MobKitFlowController.graphEdgeConditionFieldPatch(e, row.conditionOptions, field, {
                defaultOperator: row.defaultOperator,
                forceLabel: true,
                includeKind: true,
              }));
              return (
                <div key={e.id} className="branch-cond-row">
                  <div className="row row--gap">
                    <select className="field__select" value={row.modeValue} onChange={ev => {
                      const patch = window.MobKitFlowController.graphBranchConditionModePatch(e, ev.target.value, {
                        conditionOptions: row.conditionOptions,
                        firstOwnerId: row.firstOwnerId,
                        defaultOperator: row.defaultOperator,
                        contract,
                      });
                      if (patch) studio.updateEdge(e.id, patch);
                    }}>
                      {row.modeOptions.map(option => <option key={option.value} value={option.value}>{option.label}</option>)}
                    </select>
                    <span className="kv__hint">{row.targetPrefix} {row.targetLabel}</span>
                  </div>
                  {row.isCondition && (
                    !row.hasConditionOptions ? (
                      <div className="kv__hint" style={{ color: "var(--warn)" }}>{row.noConditionOptionsHint}</div>
                    ) : (
                      <div className="bld-cond" style={{ marginTop: 8 }}>
                        <select className="field__select" value={row.ownerValue} onChange={ev => setCondOwner(ev.target.value)}>
                          {row.ownerOptions.map(option => <option key={option.value} value={option.value}>{option.label}</option>)}
                        </select>
                        <select className="field__select" value={row.fieldValue} onChange={ev => setCondField(ev.target.value)}>
                          <option value={row.fieldPlaceholderOption.value}>{row.fieldPlaceholderOption.label}</option>
                          {row.fieldOptions.map(option => <option key={option.value} value={option.value}>{option.label}</option>)}
                        </select>
                        <select className="field__select bld-cond__op" value={row.operatorValue} onChange={ev => studio.updateEdge(e.id, window.MobKitFlowController.graphEdgeConditionOperatorPatch(e, ev.target.value, { defaultOperator: row.defaultOperator, contract }))}>
                          {row.operatorOptions.map(option => (
                            <option key={option.value} value={option.value} disabled={option.disabled}>{option.label}</option>
                          ))}
                        </select>
                        <GraphCondValue field={row.condField} value={e.cond?.val} onChange={val => studio.updateEdge(e.id, window.MobKitFlowController.graphEdgeConditionValuePatch(e, val, { defaultOperator: row.defaultOperator }))} />
                      </div>
                    )
                  )}
                </div>
              );
            })}
          </div>
        )}
        <div className="section">
          <div className="section__title">{gateState.wiringTitle}</div>
          <dl className="kv">
            <dt>{gateState.incomingLabel}</dt><dd>{gateState.incomingCount}</dd>
            <dt>{gateState.outgoingLabel}</dt><dd>{gateState.outgoingCount}</dd>
          </dl>
        </div>
      </div>
    </>
  );
}

// ── Instance (graph node) — TEMPLATE-ONLY fields + member summary ──
function InstanceInspector({ studio, flow, inst, selectMember, clearSelection, contract }) {
  const instanceState = window.MobKitFlowController.graphInstanceControlState({
    inst,
    instances: studio.instances,
    members: studio.members,
    schemas: studio.schemas,
  });
  const member = instanceState.member;

  if (inst.isGate) {
    return <GateInspector studio={studio} flow={flow} inst={inst} clearSelection={clearSelection} contract={contract} />;
  }

  if (inst.isTerminal) {
    const terminalState = window.MobKitFlowController.graphTerminalControlState(inst, contract);
    return (
      <>
        <div className="inspector__head">
          <div className="row row--between">
            <div>
              <div className="inspector__eyebrow">{terminalState.eyebrow}</div>
              <div className="inspector__title">{terminalState.title}</div>
              <div className="inspector__id">{terminalState.idLine}</div>
            </div>
            <button className="btn btn--ghost btn--sm" onClick={() => { studio.deleteInstance(inst.id); clearSelection(); }}>{terminalState.deleteLabel}</button>
          </div>
        </div>
        <div className="inspector__body">
          <div className="section">
            <div className="section__title">{terminalState.labelTitle}</div>
            <input className="field__input" value={terminalState.labelValue} onChange={e => studio.updateInstance(inst.id, window.MobKitFlowController.graphInstanceLabelPatch(e.target.value))} />
          </div>
          <div className="section">
            <div className="section__title">{terminalState.kindTitle}</div>
            <select className="field__select" value={terminalState.terminalKind} onChange={e => studio.updateInstance(inst.id, window.MobKitFlowController.graphTerminalKindPatch(e.target.value, contract))}>
              {terminalState.terminalKindOptions.map(option => (
                <option key={option.value} value={option.value} disabled={option.disabled}>{option.label}</option>
              ))}
            </select>
            {terminalState.selectedTerminalKind?.reason && <div className="kv__hint" style={{ color: "var(--warn)" }}>{terminalState.selectedTerminalKind.reason}</div>}
          </div>
        </div>
      </>
    );
  }

  const launchState = window.MobKitFlowController.launchModeControlState(inst, contract);

  return (
    <>
      <div className="inspector__head">
        <div className="row row--between">
          <div>
            <div className="inspector__eyebrow">{instanceState.eyebrow}</div>
            <div className="inspector__title">{instanceState.title}</div>
            <div className="inspector__id">{instanceState.idLine}</div>
          </div>
          <button className="btn btn--ghost btn--sm" onClick={() => { studio.deleteInstance(inst.id); clearSelection(); }}>{instanceState.deleteLabel}</button>
        </div>
      </div>

      <div className="inspector__body">

        {/* MEMBER SUMMARY (read-only summary, click to edit) */}
        {member && (
          <div className="section section--member-card">
            <div className="member-card">
              <div className="member-card__head">
                <span className="member-card__role">{instanceState.memberRoleLabel}</span>
                <button className="btn btn--ghost btn--sm" onClick={() => selectMember(instanceState.memberId)}>{instanceState.editMemberLabel}</button>
              </div>
              <div className="member-card__name">{instanceState.memberName}</div>
              <dl className="kv kv--small">
                {instanceState.memberSummaryRows.map(row => (
                  <React.Fragment key={row.key}>
                    <dt>{row.label}</dt><dd>{row.value}</dd>
                  </React.Fragment>
                ))}
              </dl>
              <div className="member-card__hint">{instanceState.memberHint}</div>
            </div>
          </div>
        )}

        {/* TEMPLATE-ONLY FIELDS */}
        <div className="section">
          <div className="section__title">{launchState.graphLaunchTitle}</div>
          <select
            className="field__select"
            value={launchState.launchKind}
            onChange={e => {
              studio.updateInstance(inst.id, window.MobKitFlowController.launchModeKindPatch(inst, e.target.value, contract, { firstForkSourceId: instanceState.firstForkSourceId }));
            }}
          >
            {launchState.launchOptions.map(option => (
              <option key={option.value} value={option.value} disabled={option.disabled}>{option.label}</option>
            ))}
          </select>
          {launchState.selectedLaunchMode?.reason && <div className="hint__line" style={{ color: "var(--warn)" }}>{launchState.selectedLaunchMode.reason}</div>}
          {launchState.launchKind === "Resume" && (
            <div className="field" style={{ marginTop: 8 }}>
              <label className="field__label">{launchState.resumeSessionLabel}</label>
              <input
	                className="field__input"
	                value={launchState.launchMode.sessionId || ""}
	                placeholder={launchState.resumeSessionPlaceholder}
	                onChange={e => studio.updateInstance(inst.id, window.MobKitFlowController.launchModeSessionPatch(inst, e.target.value, contract))}
	              />
            </div>
          )}
          {launchState.launchKind === "Fork" && (
            <>
              <div className="field" style={{ marginTop: 8 }}>
                <label className="field__label">{launchState.forkSourceLabel}</label>
                <select className="field__select" value={launchState.launchMode.from || ""} onChange={e => studio.updateInstance(inst.id, window.MobKitFlowController.launchModeForkSourcePatch(inst, e.target.value, contract, { sourceOptions: instanceState.forkSourceOptions }))}>
                  {instanceState.forkSourceOptions.map(option => <option key={option.value} value={option.value}>{option.label}</option>)}
                </select>
              </div>
              <div className="field">
                <label className="field__label">{launchState.graphForkContextLabel}</label>
                <select className="field__select" value={launchState.forkContextValue} onChange={e => studio.updateInstance(inst.id, window.MobKitFlowController.launchModeForkContextPatch(inst, e.target.value, contract))}>
                  {launchState.forkContextOptions.map(option => (
                    <option key={option.value} value={option.value} disabled={option.disabled}>{option.label}</option>
                  ))}
                </select>
              </div>
              {launchState.selectedForkContext?.reason && <div className="hint__line" style={{ color: "var(--warn)" }}>{launchState.selectedForkContext.reason}</div>}
            </>
          )}
          <div className="field" style={{ marginTop: 8 }}>
            <label className="field__label">{launchState.budgetPolicyLabel}</label>
            <select className="field__select" value={launchState.budgetSplitPolicy.kind} onChange={e => studio.updateInstance(inst.id, window.MobKitFlowController.launchBudgetKindPatch(inst, e.target.value, contract))}>
              {launchState.budgetOptions.map(option => (
                <option key={option.value} value={option.value} disabled={option.disabled}>{option.label}</option>
              ))}
            </select>
            {launchState.selectedBudgetPolicy?.reason && <div className="hint__line" style={{ color: "var(--warn)" }}>{launchState.selectedBudgetPolicy.reason}</div>}
          </div>
          {launchState.budgetSplitPolicy.kind === "Fixed" && (
            <div className="field">
              <label className="field__label">{launchState.fixedBudgetLabel}</label>
              <input className="field__input" type="number" min="1" step="1" value={launchState.fixedBudgetValue} onChange={e => studio.updateInstance(inst.id, window.MobKitFlowController.launchBudgetFixedLimitPatch(inst, e.target.value, contract))} />
            </div>
          )}
        </div>

        <div className="section">
          <div className="section__title">{instanceState.positionTitle}</div>
          <dl className="kv kv--small">
            {instanceState.positionRows.map(row => (
              <React.Fragment key={row.key}>
                <dt>{row.label}</dt><dd>{row.value}</dd>
              </React.Fragment>
            ))}
          </dl>
        </div>

        {member && (
          <div className="section">
            <div className="section__title">{instanceState.outputTitle}</div>
            {instanceState.outputSchema && (
              <ul className="schema-fields">
                {instanceState.outputFieldRows.map(f => (
                  <li key={f.id}>
                    <span className="sf__name">{f.name}</span>
                    <span className="sf__type">{f.type}</span>
                    {f.required && <span className="sf__req">{f.requiredLabel}</span>}
                  </li>
                ))}
              </ul>
            )}
            <div className="hint__line" style={{ marginTop: 6 }}>{instanceState.outputHint} <button className="link" onClick={() => selectMember(instanceState.memberId)}>{instanceState.outputOpenMemberLabel}</button></div>
          </div>
        )}
      </div>
    </>
  );
}

function GraphCondValue({ field, value, onChange }) {
  const control = window.MobKitFlowController.conditionValueControl(field, value);
  if (control.kind === "enum") {
    return (
      <select className="field__select" value={control.value} onChange={e => onChange(e.target.value)}>
        {control.optionRows.map(row => <option key={row.value || "blank"} value={row.value}>{row.label}</option>)}
      </select>
    );
  }
  if (control.kind === "boolean") {
    return (
      <select className="field__select" value={control.value} onChange={e => onChange(e.target.value)}>
        {control.optionRows.map(row => <option key={row.value || "blank"} value={row.value}>{row.label}</option>)}
      </select>
    );
  }
  return <input className="field__input" placeholder={control.placeholder} value={control.value} onChange={e => onChange(e.target.value)} />;
}

// ── Edge ─────────────────────────────────────────────────────────
function EdgeInspector({ studio, flow, edge, clearSelection, contract }) {
  const edgeState = window.MobKitFlowController.graphEdgeInspectorState({
    edge,
    instances: studio.instances,
    members: studio.members,
    schemas: studio.schemas,
    flow,
    contract,
  });
  const change = (patch) => studio.updateEdge(edge.id, patch);
  const setEdgeKind = (kind) => change(window.MobKitFlowController.graphEdgeKindPatch(edge, kind, {
    defaultOperator: edgeState.defaultOperator,
    conditionPatch: edgeState.conditionPatch,
    forceLabel: true,
    contract,
  }));
  const setCondOwner = (instanceId) => change(window.MobKitFlowController.graphEdgeConditionOwnerPatch(edge, edgeState.conditionOptions, instanceId, {
    defaultOperator: edgeState.defaultOperator,
    forceLabel: true,
    contract,
  }));
  const setCondField = (field) => change(window.MobKitFlowController.graphEdgeConditionFieldPatch(edge, edgeState.conditionOptions, field, {
    defaultOperator: edgeState.defaultOperator,
    forceLabel: true,
    contract,
  }));
  return (
    <>
      <div className="inspector__head">
        <div className="row row--between">
          <div>
            <div className="inspector__eyebrow">{edgeState.eyebrow}</div>
            <div className="inspector__title">{edgeState.title}</div>
            <div className="inspector__id">{edgeState.idLine}</div>
          </div>
          <button className="btn btn--ghost btn--sm" onClick={() => { studio.deleteEdge(edge.id); clearSelection(); }}>{edgeState.deleteLabel}</button>
        </div>
      </div>
      <div className="inspector__body">
        <div className="section">
          <div className="section__title">{edgeState.kindTitle}</div>
          <select className="field__select" value={edgeState.edgeKind} onChange={e => setEdgeKind(e.target.value)}>
            {edgeState.edgeKindOptions.map(option => (
              <option key={option.value} value={option.value} disabled={option.disabled}>{option.label}</option>
            ))}
          </select>
          {edgeState.selectedEdgeKind?.reason && <div className="hint__line" style={{ color: "var(--warn)" }}>{edgeState.selectedEdgeKind.reason}</div>}
        </div>
        <div className="section">
          <div className="section__title">{edgeState.labelTitle}</div>
          <input className="field__input" value={edge.label || ""} onChange={e => change(window.MobKitFlowController.graphEdgeLabelPatch(e.target.value))} />
        </div>
        {edgeState.isCondition && (
          <div className="section">
            <div className="section__title">{edgeState.conditionTitle}</div>
            {!edgeState.hasConditionOptions ? (
              <div className="hint__line" style={{ color: "var(--warn)" }}>{edgeState.noConditionOptionsHint}</div>
            ) : (
            <div className="cond-row">
              <select className="field__select" value={edgeState.ownerValue} onChange={e => setCondOwner(e.target.value)}>
                <option value={edgeState.ownerPlaceholderOption.value}>{edgeState.ownerPlaceholderOption.label}</option>
                {edgeState.ownerOptions.map(option => <option key={option.value} value={option.value}>{option.label}</option>)}
              </select>
              <select className="field__select" value={edgeState.fieldValue} disabled={!edgeState.condOwner} onChange={e => setCondField(e.target.value)}>
                <option value="">{edgeState.fieldPlaceholder}</option>
                {edgeState.fieldOptions.map(option => <option key={option.field.id || option.value} value={option.value}>{option.label}</option>)}
              </select>
              <select className="field__select" style={{ width: 60 }} value={edgeState.operatorValue} onChange={e => change(window.MobKitFlowController.graphEdgeConditionOperatorPatch(edge, e.target.value, { defaultOperator: edgeState.defaultOperator, contract }))}>
                {edgeState.operatorOptions.map(option => (
                  <option key={option.value} value={option.value} disabled={option.disabled}>{option.label}</option>
                ))}
              </select>
              <GraphCondValue field={edgeState.condField} value={edge.cond?.val} onChange={val => change(window.MobKitFlowController.graphEdgeConditionValuePatch(edge, val, { defaultOperator: edgeState.defaultOperator }))} />
            </div>
            )}
          </div>
        )}
        <div className="section">
          <div className="section__title">{edgeState.fromTitle}</div>
          <dl className="kv">
            {edgeState.fromRows.map(row => (
              <React.Fragment key={row.key}>
                <dt>{row.label}</dt><dd>{row.value}</dd>
              </React.Fragment>
            ))}
          </dl>
        </div>
        <div className="section">
          <div className="section__title">{edgeState.toTitle}</div>
          <dl className="kv">
            {edgeState.toRows.map(row => (
              <React.Fragment key={row.key}>
                <dt>{row.label}</dt><dd>{row.value}</dd>
              </React.Fragment>
            ))}
          </dl>
        </div>
      </div>
    </>
  );
}

// ── Add-node dialog ────────────────────────────────────────────────
// In topology view, you can place existing agents and MobKit flow gates.
// To define a new agent, jump to the Agents view.
function AddNodeMenu({ at, members, contract, onPick, onClose, onJumpToAgents }) {
  const [q, setQ] = React.useState("");
  React.useEffect(() => { setQ(""); }, [at]);
  if (!at) return null;

  const menuState = window.MobKitFlowController.graphAddNodeMenuState({ members, contract, query: q });

  return (
    <div className="add-menu" style={{ left: at.x, top: at.y }} onClick={e => e.stopPropagation()} onMouseDown={e => e.stopPropagation()}>
      <div className="add-menu__search">
        <span className="add-menu__search-icon">{menuState.searchIcon}</span>
        <input className="add-menu__search-input" autoFocus placeholder={menuState.searchPlaceholder} value={q} onChange={e => setQ(e.target.value)}
          onKeyDown={e => { if (e.key === "Escape") onClose(); }} />
        <button className="add-menu__x" onClick={onClose} title={menuState.closeTitle}>{menuState.closeLabel}</button>
      </div>

      <div className="add-menu__scroll">
        {menuState.hasMembers && <div className="add-menu__label">{menuState.agentsLabel}</div>}
        {menuState.memberRows.map(row => (
          <button key={row.id} className="add-menu__row" onClick={() => onPick(row.pick)}>
            <span className="add-menu__dot" data-role={row.role} />
            <span className="add-menu__row-name">{row.name}</span>
            <span className="add-menu__row-meta">{row.model}</span>
          </button>
        ))}

        {menuState.hasControls && <div className="add-menu__label">{menuState.controlsLabel}</div>}
        {menuState.controlRows.map(row => (
          <button key={row.id} className="add-menu__row" onClick={() => onPick(row.pick)}>
            <span className="add-menu__glyph">{row.glyph}</span>
            <span className="add-menu__row-name">{row.label}</span>
            <span className="add-menu__row-meta">{row.meta}</span>
          </button>
        ))}

        {menuState.isEmpty && (
          <div className="add-menu__empty">{menuState.emptyLabel}</div>
        )}
      </div>

      {onJumpToAgents && (
        <button className="add-menu__foot" onClick={() => onJumpToAgents(null)}>{menuState.jumpLabel}</button>
      )}
    </div>
  );
}

window.Inspector = Inspector;
window.AddNodeMenu = AddNodeMenu;
