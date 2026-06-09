/* global React */
// AgentsView — full-pane Agents catalog editor.
//
// Two side-by-side stacks:
//   ┌─ AGENTS ────┐ ┌─ EDITOR ─────────────────────────────┐
//   │ planner     │ │ identity, prompt, tools, schema ref, │
//   │ coder       │ │ used in (instances)                  │
//   │ reviewer ●  │ │                                       │
//   │ + new       │ │ ── SCHEMA LIBRARY (collapsible) ──    │
//   ├─ SCHEMAS ───┤ │ visual field editor for selected     │
//   │ PlanArtifact│ │ schema (no JSON)                      │
//   │ ReviewGate  │ └───────────────────────────────────────┘
//
// The right pane swaps based on what's selected:
//   { kind: "agent", id }   → AgentEditor
//   { kind: "schema", id }  → SchemaEditor (visual, field-by-field)
//   null                    → empty hint

function AgentsView({ studio, agentSel, setAgentSel, contract, deploySettings, flow, setFlow, mobSettings, setMobSettings, toolCatalog = [], modelCatalog = [], agentDefinitions = [], agentView = null, agentDetailView = null, agentAccessView = null, schemaView = null }) {
  return (
    <div className="agents-view">
      <AgentsList studio={studio} agentSel={agentSel} setAgentSel={setAgentSel} contract={contract} deploySettings={deploySettings} agentDefinitions={agentDefinitions} toolCatalog={toolCatalog} modelCatalog={modelCatalog} agentView={agentView} />
      <div className="agents-view__main">
        <AgentsMain studio={studio} agentSel={agentSel} setAgentSel={setAgentSel} contract={contract} deploySettings={deploySettings} flow={flow} setFlow={setFlow} mobSettings={mobSettings} setMobSettings={setMobSettings} toolCatalog={toolCatalog} modelCatalog={modelCatalog} agentView={agentView} agentDetailView={agentDetailView} agentAccessView={agentAccessView} schemaView={schemaView} />
      </div>
    </div>
  );
}

function AgentsList({ studio, agentSel, setAgentSel, contract, deploySettings, agentDefinitions, toolCatalog = [], modelCatalog = [], agentView = null }) {
  const [schemaAddResult, setSchemaAddResult] = React.useState(null);
  const listState = window.MobKitFlowController.agentListState({
    members: studio.members,
    instances: studio.instances,
    schemas: studio.schemas,
    selection: agentSel,
    agentView,
  });
  const schemaAddErrorState = window.MobKitFlowController.schemaDefinitionAddErrorState(schemaAddResult);
  return (
    <aside className="agents-list">
      <div className="agents-list__head">
        <span className="agents-list__title">{listState.agentsHeading}</span>
        <span className="agents-list__count">{listState.memberCount}</span>
      </div>
      <div className="agents-list__scroll">
        {listState.memberRows.map(row => {
          return (
            <button
              key={row.id}
              className={row.itemClass}
              onClick={() => setAgentSel({ kind: "agent", id: row.id })}
            >
              <span className="agents-list__bullet" data-role={row.bulletRole}>●</span>
              <div className="agents-list__col">
                <span className="agents-list__name">{row.name}</span>
                <span className="agents-list__sub">{row.subLabel}</span>
              </div>
              <span className={row.placedClass}>
                {row.placedLabel}
              </span>
            </button>
          );
        })}
        <AddAgentControl studio={studio} setAgentSel={setAgentSel} agentDefinitions={agentDefinitions} contract={contract} deploySettings={deploySettings} toolCatalog={toolCatalog} modelCatalog={modelCatalog} agentView={agentView} />
      </div>

      <div className="agents-list__head agents-list__head--sub">
        <span className="agents-list__title">{listState.schemasHeading}</span>
        <span className="agents-list__count">{listState.schemaCount}</span>
      </div>
      <div className="agents-list__scroll">
        {listState.schemaRows.map(row => {
          return (
            <button
              key={row.id}
              className={row.itemClass}
              onClick={() => setAgentSel({ kind: "schema", id: row.id })}
            >
              <span className="agents-list__bullet" data-role={row.bulletRole}>▢</span>
              <div className="agents-list__col">
                <span className="agents-list__name">{row.id}</span>
                <span className="agents-list__sub">{row.subLabel}</span>
              </div>
            </button>
          );
        })}
        <button
          className="agents-list__add"
          onClick={() => {
            const result = window.MobKitFlowController.schemaDefinitionAddPatch(studio.schemas, contract);
            setSchemaAddResult(result);
            if (result.ok === false) return;
            if (studio.snap) studio.snap();
            studio.setSchemas(result.schemas);
            setSchemaAddResult(null);
            setAgentSel({ kind: "schema", id: result.schema.id });
          }}
        >{listState.addSchemaLabel}</button>
        {schemaAddErrorState.hasError && <div className="hint__line">{schemaAddErrorState.text}</div>}
      </div>
    </aside>
  );
}

function AddAgentControl({ studio, setAgentSel, agentDefinitions = [], contract = null, deploySettings = null, toolCatalog = [], modelCatalog = [], agentView = null }) {
  const [lastAddResult, setLastAddResult] = React.useState(null);
  const definitionState = window.MobKitFlowController.agentDefinitionAddControlState(agentDefinitions, agentView);
  const definitionErrorState = window.MobKitFlowController.agentDefinitionAddErrorState(lastAddResult, agentView);
  const createFromDefinition = (definitionId) => {
    const result = window.MobKitFlowController.agentDefinitionAddByIdPatch(agentDefinitions, definitionId, {
      members: studio.members,
      schemas: studio.schemas,
      contract,
      deploySettings,
      modelCatalog,
      toolCatalog,
      skillRealms: studio.skillRealms,
    });
    setLastAddResult(result);
    if (!result.ok) return;
    if (studio.snap) studio.snap();
    if (result.schemas !== studio.schemas) studio.setSchemas(result.schemas);
    studio.setMembers(result.members);
    setAgentSel({ kind: "agent", id: result.member.id });
  };
  if (!definitionState.hasDefinitions) {
    return (
      <>
        <button
          className={definitionState.controlClass}
          disabled
          title={definitionState.title}
        >{definitionState.unavailableLabel}</button>
        {definitionErrorState.hasError && <div className="hint__line">{definitionErrorState.text}</div>}
      </>
    );
  }
  return (
    <>
      <select
        className={definitionState.controlClass}
        value={definitionState.value}
        title={definitionState.title}
        onChange={e => {
          const id = e.target.value;
          if (!id) return;
          createFromDefinition(id);
          e.target.value = "";
        }}
      >
        <option value={definitionState.placeholderOption.value}>{definitionState.placeholderOption.label}</option>
        {definitionState.optionRows.map(option => (
          <option key={option.value} value={option.value}>{option.label}</option>
        ))}
      </select>
      {definitionErrorState.hasError && <div className="hint__line">{definitionErrorState.text}</div>}
    </>
  );
}

function AgentsMain({ studio, agentSel, setAgentSel, contract, deploySettings, flow, setFlow, mobSettings, setMobSettings, toolCatalog, modelCatalog, agentView = null, agentDetailView = null, agentAccessView = null, schemaView = null }) {
  const selectionState = window.MobKitFlowController.agentSelectionState({
    selection: agentSel,
    members: studio.members,
    schemas: studio.schemas,
    agentView,
  });
  if (selectionState.kind === "empty") {
    return (
      <div className="agents-empty">
        <div className="agents-empty__head">{selectionState.emptyState.title}</div>
        {selectionState.emptyState.lines.map((line, index) => (
          <div className="agents-empty__line" key={index}>{line}</div>
        ))}
      </div>
    );
  }
  if (selectionState.kind === "schema") {
    if (!selectionState.schema) return <div className="agents-empty">{selectionState.missingSchemaLabel}</div>;
    return <SchemaEditor studio={studio} schema={selectionState.schema} setAgentSel={setAgentSel} contract={contract} flow={flow} setFlow={setFlow} schemaView={schemaView} />;
  }
  if (!selectionState.member) return <div className="agents-empty">{selectionState.missingAgentLabel}</div>;
  return <AgentEditor studio={studio} member={selectionState.member} setAgentSel={setAgentSel} contract={contract} deploySettings={deploySettings} flow={flow} setFlow={setFlow} mobSettings={mobSettings} setMobSettings={setMobSettings} toolCatalog={toolCatalog} modelCatalog={modelCatalog} agentDetailView={agentDetailView} agentAccessView={agentAccessView} />;
}

// ── Agent editor ────────────────────────────────────────────────────
function AgentEditor({ studio, member, setAgentSel, contract, deploySettings, flow, setFlow, mobSettings, setMobSettings, toolCatalog = [], modelCatalog = [], agentDetailView = null, agentAccessView = null }) {
  const [memberEditError, setMemberEditError] = React.useState("");
  const change = (patch) => {
    if (!patch || typeof patch !== "object" || !Object.keys(patch).length) return;
    const result = window.MobKitFlowController.memberUpdateCascadePatch({
      memberId: member.id,
      members: studio.members,
      flow,
      instances: studio.instances,
      edges: studio.edges,
      mobSettings,
      contract,
    }, patch);
    if (!result.ok) {
      setMemberEditError(result.error || "");
      return;
    }
    if (studio.snap) studio.snap();
    studio.setMembers(result.members);
    if (result.flow !== flow && setFlow) setFlow(result.flow);
    if (result.instances !== studio.instances) studio.setInstances(result.instances);
    if (result.edges !== studio.edges) studio.setEdges(result.edges);
    if (result.mobSettings !== mobSettings && setMobSettings) setMobSettings(result.mobSettings);
    setMemberEditError("");
  };
  const [toolDraft, setToolDraft] = React.useState("");
  const [toolDraftError, setToolDraftError] = React.useState("");
  const [schemaChangeResult, setSchemaChangeResult] = React.useState(null);
  const toolAccessState = window.MobKitFlowController.memberToolAccessState(member, toolCatalog, agentAccessView);
  const editorState = window.MobKitFlowController.agentEditorControlState({
    member,
    instances: studio.instances,
    schemas: studio.schemas,
    contract,
    deploySettings,
    modelCatalog,
    agentDetailView,
  });
  const schemaErrorState = window.MobKitFlowController.memberSchemaChangeErrorState(schemaChangeResult);
  const addToolAccess = (raw) => {
    const result = window.MobKitFlowController.memberToolAccessCascadePatch({
      memberId: member.id,
      members: studio.members,
      flow,
      instances: studio.instances,
    }, raw, toolCatalog, agentAccessView);
    if (!result.ok) {
      setToolDraftError(result.error || "");
      return;
    }
    if (result.patch) {
      if (studio.snap) studio.snap();
      studio.setMembers(result.members);
      if (result.flow !== flow && setFlow) setFlow(result.flow);
      if (result.instances !== studio.instances) studio.setInstances(result.instances);
    }
    setToolDraft("");
    setToolDraftError("");
  };
  const removeToolAccess = (toolId) => {
    const result = window.MobKitFlowController.memberToolRemoveCascadePatch({
      memberId: member.id,
      members: studio.members,
      flow,
      instances: studio.instances,
    }, toolId);
    if (!result.ok || !result.patch) return;
    if (studio.snap) studio.snap();
    studio.setMembers(result.members);
    if (result.flow !== flow && setFlow) setFlow(result.flow);
    if (result.instances !== studio.instances) studio.setInstances(result.instances);
  };
  const changeSchema = (rawSchema) => {
    const result = window.MobKitFlowController.memberSchemaCascadePatch({
      memberId: member.id,
      members: studio.members,
      flow,
      edges: studio.edges,
      instances: studio.instances,
      schemas: studio.schemas,
    }, rawSchema);
    setSchemaChangeResult(result);
    if (!result.ok) return;
    if (studio.snap) studio.snap();
    studio.setMembers(result.members);
    if (result.flow !== flow && setFlow) setFlow(result.flow);
    if (result.instances !== studio.instances) studio.setInstances(result.instances);
    if (result.edges !== studio.edges) studio.setEdges(result.edges);
    setSchemaChangeResult(null);
  };

  return (
    <div className="agent-editor">
      <div className="agent-editor__head">
        <div className="row row--between">
          <div>
            <div className="inspector__eyebrow">AGENT · {member.role}</div>
            <input
              className="agent-editor__title-input"
              value={member.name}
              onChange={e => change(window.MobKitFlowController.memberNamePatch(e.target.value))}
            />
            <div className="inspector__id">{editorState.idLine}</div>
            {memberEditError && <div className="hint__line" style={{ color: "var(--danger)" }}>{memberEditError}</div>}
          </div>
          <button className="btn btn--ghost btn--sm" onClick={() => {
            if (editorState.deleteNeedsConfirmation) {
              if (!confirm(editorState.deleteConfirmMessage)) return;
            }
            const result = window.MobKitFlowController.memberDeleteCascadePatch({
              memberId: member.id,
              members: studio.members,
              instances: studio.instances,
              edges: studio.edges,
              flow,
              mobSettings,
            });
            if (!result.ok) return;
            if (studio.snap) studio.snap();
            studio.setMembers(result.members);
            studio.setInstances(result.instances);
            studio.setEdges(result.edges);
            if (setFlow) setFlow(result.flow);
            if (setMobSettings) setMobSettings(result.mobSettings);
            setAgentSel(null);
          }}>{editorState.deleteLabel}</button>
        </div>
      </div>

      <div className="agent-editor__body">
        <div className="agent-editor__cols">
          {/* LEFT COLUMN — identity, prompt */}
          <div className="agent-editor__col">

            <div className="section">
              <div className="section__title">{editorState.identityTitle}</div>
              <div className="field">
                <label className="field__label">{editorState.profileBindingLabel}</label>
                <select
                  className="field__select"
                  value={editorState.profileBinding}
                  onChange={e => change(window.MobKitFlowController.memberProfileBindingPatch(member, e.target.value, contract))}
                >
                  {editorState.bindingOptions.map(option => (
                    <option key={option.value} value={option.value} disabled={option.disabled}>{option.label}</option>
                  ))}
                </select>
                {editorState.selectedBinding?.reason && <div className="hint__line" style={{ color: "var(--warn)" }}>{editorState.selectedBinding.reason}</div>}
              </div>
              {editorState.isRealmProfile ? (
                <div className="field">
                  <label className="field__label">{editorState.realmProfileLabel}</label>
                  <input
                    className="field__input field__input--mono"
                    value={member.realmProfile || ""}
                    placeholder={editorState.realmProfilePlaceholder}
                    onChange={e => change(window.MobKitFlowController.memberRealmProfilePatch(e.target.value))}
                  />
                  <div className="hint__line">{editorState.realmProfileImportHint}</div>
                </div>
              ) : (
                <>
              <div className="field">
                <label className="field__label">{editorState.modelLabel}</label>
                <select className="field__select" value={member.model} onChange={e => change(window.MobKitFlowController.memberModelPatch(e.target.value, modelCatalog))}>
                  {editorState.modelOptions.map(option => <option key={option.value} value={option.value}>{option.label}</option>)}
                </select>
              </div>
              <div className="field">
                <label className="field__label">{editorState.runtimeModeLabel}</label>
                <select className="field__select" value={editorState.runtimeMode} onChange={e => change(window.MobKitFlowController.memberRuntimeModePatch(e.target.value, contract, deploySettings))}>
                  {editorState.runtimeOptions.map(option => (
                    <option key={option.value} value={option.value} disabled={option.disabled}>
                      {option.label}
                    </option>
                  ))}
                </select>
                {editorState.selectedRuntime?.reason && <div className="hint__line" style={{ color: "var(--warn)" }}>{editorState.selectedRuntime.reason}</div>}
              </div>
              <div className="field">
                <label className="field__label">{editorState.backendLabel}</label>
                <select className="field__select" value={editorState.backendValue} onChange={e => change(window.MobKitFlowController.memberBackendPatch(e.target.value, contract))}>
                  {editorState.backendOptions.map(option => (
                    <option key={option.value || "default"} value={option.value} disabled={option.disabled}>{option.label}</option>
                  ))}
                </select>
                {editorState.selectedBackend?.reason && <div className="hint__line" style={{ color: "var(--warn)" }}>{editorState.selectedBackend.reason}</div>}
              </div>
              <div className="field">
                <label className="field__label">{editorState.inlinePeerNotificationsLabel}</label>
                <input
                  className="field__input"
                  type="number"
                  min="-1"
                  step="1"
                  value={member.maxInlinePeerNotifications ?? ""}
                  placeholder={editorState.inlinePeerNotificationsPlaceholder}
                  onChange={e => change(window.MobKitFlowController.memberMaxInlinePeerNotificationsPatch(e.target.value))}
                />
              </div>
              <ProviderParamsEditor member={member} change={change} agentDetailView={agentDetailView} />
                </>
              )}
            </div>

            {!editorState.isRealmProfile && (
              <div className="section">
                <div className="section__title section__title--row">
                  <span>{editorState.systemPromptTitle}</span>
                  <button className="ghost-btn" onClick={() => change(window.MobKitFlowController.memberSystemPromptPatch(window.MobKitFlowController.memberPromptSkeleton(member)))} title={editorState.applySkeletonTitle}>{editorState.applySkeletonLabel}</button>
                </div>
                <textarea
                  className="field__textarea"
                  rows={8}
                  value={member.systemPrompt || ""}
                  onChange={e => change(window.MobKitFlowController.memberSystemPromptPatch(e.target.value))}
                  placeholder={editorState.systemPromptPlaceholder}
                />
              </div>
            )}

            <div className="section">
              <div className="section__title">{editorState.sourceProvenance.title}</div>
              {editorState.sourceProvenance.hasRows ? (
                <dl className="kv kv--small">
                  {editorState.sourceProvenance.rows.map(row => (
                    <React.Fragment key={row.label}>
                      <dt>{row.label}</dt>
                      <dd>{row.value}</dd>
                    </React.Fragment>
                  ))}
                </dl>
              ) : (
                <div className="hint__line">{editorState.sourceProvenance.emptyHint}</div>
              )}
            </div>

          </div>

          {/* RIGHT COLUMN — tools, schema, usage */}
          <div className="agent-editor__col">

            {editorState.isRealmProfile ? (
              <div className="section">
                <div className="section__title">{editorState.realmProfileTitle}</div>
                <div className="hint__line">
                  {editorState.realmProfileReferenceHintBefore} <code>{editorState.realmProfileReferenceLabel}</code> {editorState.realmProfileReferenceHintAfter}
                </div>
              </div>
            ) : (
              <>
            <div className="section">
              <div className="section__title">{toolAccessState.title}</div>
              <div className="hint__line" style={{ marginBottom: 8 }}>
                {toolAccessState.hint}
              </div>
              {toolAccessState.rows.map(row => {
                return (
                  <div key={row.id} className={row.className}>
                    <div>
                      <div className="name">{row.name}</div>
                      <div className="auth">{row.description}</div>
                    </div>
                    <button onClick={() => removeToolAccess(row.id)}>{row.removeLabel}</button>
                  </div>
                );
              })}
              <select
                className="field__select"
                value={toolAccessState.addSelectValue}
                onChange={e => {
                  const id = e.target.value; if (!id) return;
                  addToolAccess(id);
                }}
              >
                <option value={toolAccessState.addSelectValue}>{toolAccessState.addSelectPlaceholder}</option>
                {toolAccessState.addableRows.map(row => (
                  <option key={row.id} value={row.value}>{row.optionLabel}</option>
                ))}
              </select>
              <div className="field" style={{ marginTop: 8 }}>
                <label className="field__label">{toolAccessState.sourceLabel}</label>
                <div className="row row--gap">
                  <input
                    className="field__input field__input--mono"
                    value={toolDraft}
                    placeholder={toolAccessState.sourcePlaceholder}
                    onChange={e => { setToolDraft(e.target.value); setToolDraftError(""); }}
                    onKeyDown={e => { if (e.key === "Enter") addToolAccess(toolDraft); }}
                  />
                  <button className="btn btn--ghost btn--sm" onClick={() => addToolAccess(toolDraft)}>{toolAccessState.addButtonLabel}</button>
                </div>
                {toolDraftError && <div className="hint__line">{toolDraftError}</div>}
              </div>
            </div>

            <div className="section">
              <div className="section__title">{editorState.outputSchemaTitle}</div>
              <select
                className="field__select"
                value={member.schema || ""}
                onChange={e => changeSchema(e.target.value)}
              >
                {editorState.schemaOptions.map(option => <option key={option.value || "none"} value={option.value}>{option.label}</option>)}
              </select>
              {schemaErrorState.hasError && <div className="hint__line">{schemaErrorState.text}</div>}
              {editorState.hasOutputSchema ? (
                <>
                  <ul className="schema-fields schema-fields--preview">
                    {editorState.schemaPreviewRows.map(f => (
                      <li key={f.id}>
                        <span className="sf__name">{f.name}</span>
                        <span className="sf__type">{f.type}</span>
                        {f.required && <span className="sf__req">{f.requiredLabel}</span>}
                      </li>
                    ))}
                  </ul>
                  <button className="link" onClick={() => setAgentSel(editorState.editSchemaSelection)}>
                    {editorState.editSchemaLabel}
                  </button>
                </>
              ) : (
                <div className="hint__line" style={{ marginTop: 6 }}>
                  {editorState.emptySchemaHint}
                </div>
              )}
            </div>

            <div className="section">
              <SkillAccess studio={studio} member={member} agentAccessView={agentAccessView} />
            </div>
              </>
            )}

            <div className="section">
              <div className="section__title">{editorState.usageTitle}</div>
              {editorState.placedCount === 0 && (
                <div className="hint__line">{editorState.emptyUsageHint}</div>
              )}
              {editorState.usageRows.map(row => (
                <div key={row.id} className="usage-row usage-row--ro">
                  <span className="usage-row__label">{row.id}</span>
                  <span className="usage-row__cell">{row.cellLabel}</span>
                  <span className="usage-row__lane">{row.laneLabel}</span>
                </div>
              ))}
            </div>

          </div>
        </div>
      </div>
    </div>
  );
}

// ── Schema editor (visual, field-by-field) ──────────────────────────
function SchemaEditor({ studio, schema, setAgentSel, contract, flow, setFlow, schemaView = null }) {
  const change = (patch) => studio.updateSchema(schema.id, patch);
  const [fieldAddResult, setFieldAddResult] = React.useState(null);
  React.useEffect(() => setFieldAddResult(null), [schema?.id]);
  const schemaState = window.MobKitFlowController.schemaEditorControlState({
    schema,
    members: studio.members,
    schemaView,
  });
  const fieldAddErrorState = window.MobKitFlowController.schemaFieldAddErrorState(fieldAddResult);

  const renameField = (fieldId, oldName, newName) => {
    const result = window.MobKitFlowController.schemaFieldRenameCascadePatch({
      schema,
      schemas: studio.schemas,
      flow,
      edges: studio.edges,
      members: studio.members,
      instances: studio.instances,
    }, fieldId, newName, oldName, contract);
    const flowChanged = result.flow !== flow;
    const edgesChanged = result.edges !== studio.edges;
    if (studio.snap) studio.snap();
    studio.setSchemas(result.schemas);
    if (flowChanged && setFlow) setFlow(result.flow);
    if (edgesChanged) studio.setEdges(result.edges);
  };

  const updateField = (fieldId, patch) => {
    if (Object.prototype.hasOwnProperty.call(patch || {}, "name")) {
      change(window.MobKitFlowController.schemaFieldUpdatePatch(schema, fieldId, patch, contract));
      return;
    }
    const result = window.MobKitFlowController.schemaFieldUpdateCascadePatch({
      schema,
      schemas: studio.schemas,
      flow,
      edges: studio.edges,
      members: studio.members,
      instances: studio.instances,
    }, fieldId, patch, contract);
    const flowChanged = result.flow !== flow;
    const edgesChanged = result.edges !== studio.edges;
    if (studio.snap) studio.snap();
    studio.setSchemas(result.schemas);
    if (flowChanged && setFlow) setFlow(result.flow);
    if (edgesChanged) studio.setEdges(result.edges);
  };

  const deleteField = (fieldId) => {
    const result = window.MobKitFlowController.schemaFieldDeleteCascadePatch({
      schema,
      schemas: studio.schemas,
      flow,
      edges: studio.edges,
      members: studio.members,
      instances: studio.instances,
    }, fieldId);
    if (studio.snap) studio.snap();
    studio.setSchemas(result.schemas);
    if (result.flow !== flow && setFlow) setFlow(result.flow);
    if (result.edges !== studio.edges) studio.setEdges(result.edges);
  };

  const addField = () => {
    const result = window.MobKitFlowController.schemaFieldAddPatch(schema, contract);
    setFieldAddResult(result);
    if (result.ok === false) return;
    setFieldAddResult(null);
    change(result.patch);
  };

  const deleteSchema = () => {
    const result = window.MobKitFlowController.studioDeleteSchemaPatch({
      schemas: studio.schemas,
      members: studio.members,
      flow,
      edges: studio.edges,
      instances: studio.instances,
    }, schema.id);
    if (studio.snap) studio.snap();
    studio.setSchemas(result.schemas);
    studio.setMembers(result.members);
    if (result.flow !== flow && setFlow) setFlow(result.flow);
    if (result.edges !== studio.edges) studio.setEdges(result.edges);
    setAgentSel(null);
  };

  const renameSchema = (newId) => {
    const result = window.MobKitFlowController.renameSchemaDefinition({
      schemas: studio.schemas,
      members: studio.members,
      flow,
    }, schema.id, newId);
    if (!result.renamed) return;
    if (studio.snap) studio.snap();
    studio.setSchemas(result.schemas);
    studio.setMembers(result.members);
    if (result.flow !== flow && setFlow) setFlow(result.flow);
    setAgentSel({ kind: "schema", id: String(newId || "").trim() });
  };

  return (
    <div className="agent-editor">
      <div className="agent-editor__head">
        <div className="row row--between">
          <div>
            <div className="inspector__eyebrow">{schemaState.eyebrow}</div>
            <input
              className="agent-editor__title-input"
              defaultValue={schema.id}
              onBlur={e => renameSchema(e.target.value)}
              onKeyDown={e => { if (e.key === "Enter") e.target.blur(); }}
            />
            <div className="inspector__id">{schemaState.usageLabel}</div>
          </div>
          <button
            className="btn btn--ghost btn--sm"
            disabled={!schemaState.canDelete}
            title={schemaState.deleteTitle}
            onClick={deleteSchema}
          >{schemaState.deleteLabel}</button>
        </div>
      </div>

      <div className="agent-editor__body">
        <div className="section">
          <div className="section__title">{schemaState.descriptionTitle}</div>
          <textarea
            className="field__textarea"
            rows={2}
            value={schema.description || ""}
            placeholder={schemaState.descriptionPlaceholder}
            onChange={e => change(window.MobKitFlowController.schemaDescriptionPatch(e.target.value))}
          />
        </div>

        <div className="section">
          <div className="row row--between" style={{ marginBottom: 6 }}>
            <div className="section__title">{schemaState.fieldsTitle}</div>
            <button className="btn btn--ghost btn--sm" onClick={addField}>{schemaState.addFieldLabel}</button>
          </div>
          {fieldAddErrorState.hasError && <div className="hint__line">{fieldAddErrorState.text}</div>}

          <div className="schema-builder">
            <div className="schema-builder__header">
              <span className="sb-col sb-col--name">{schemaState.headerLabels.name}</span>
              <span className="sb-col sb-col--type">{schemaState.headerLabels.type}</span>
              <span className="sb-col sb-col--req">{schemaState.headerLabels.required}</span>
              <span className="sb-col sb-col--desc">{schemaState.headerLabels.description}</span>
              <span className="sb-col sb-col--act">{schemaState.headerLabels.action}</span>
            </div>
            {schemaState.fieldRows.map(({ field: f }) => (
              <SchemaField
                key={f.id}
                field={f}
                normalizeName={(raw) => window.MobKitFlowController.uniqueSchemaFieldName(schema.fields, raw, f.id)}
                onChange={(patch) => updateField(f.id, patch)}
                onRename={(oldName, newName) => renameField(f.id, oldName, newName)}
                onDelete={() => deleteField(f.id)}
                contract={contract}
                schemaView={schemaView}
              />
            ))}
            {schemaState.fieldRows.length === 0 && (
              <div className="schema-builder__empty">{schemaState.emptyFieldsHint}</div>
            )}
          </div>
        </div>

        <div className="section">
          <div className="section__title">{schemaState.usedByTitle}</div>
          {schemaState.usedCount === 0 && <div className="hint__line">{schemaState.emptyUsedByHint}</div>}
          {schemaState.usedBy.map(row => (
            <button
              key={row.id}
              className="usage-row"
              onClick={() => setAgentSel(row.selection)}
            >
              <span className="usage-row__label">{row.name}</span>
              <span className="usage-row__cell">{row.role}</span>
              <span className="usage-row__lane">{row.model}</span>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

function SchemaField({ field, normalizeName, onChange, onRename, onDelete, contract, schemaView = null }) {
  const nameBeforeEdit = React.useRef(field.name);
  const fieldState = window.MobKitFlowController.schemaFieldRowControlState(field, contract, schemaView);
  const typeState = fieldState.typeState;
  const values = fieldState.enumValues;

  return (
    <div className="schema-field">
      <input
        className="sb-input sb-col--name"
        value={field.name}
        onFocus={() => { nameBeforeEdit.current = field.name; }}
        onChange={e => onChange({ name: e.target.value })}
        onBlur={e => {
          const normalized = normalizeName(e.target.value);
          const previous = String(nameBeforeEdit.current || "").trim();
          onChange({ name: normalized });
          if (previous && previous !== normalized && onRename) onRename(previous, normalized);
        }}
        placeholder={fieldState.namePlaceholder}
      />
      <select
        className="sb-select sb-col--type"
        value={typeState.type}
        onChange={e => {
          onChange(window.MobKitFlowController.schemaLikeFieldTypePatch(field, e.target.value, contract));
        }}
      >
        {typeState.typeOptions.map(option => (
          <option key={option.value} value={option.value} disabled={option.disabled}>{option.label}</option>
        ))}
      </select>
      {typeState.selectedType?.reason && <div className="hint__line" style={{ color: "var(--warn)" }}>{typeState.selectedType.reason}</div>}
      <label className="sb-col--req sb-checkbox">
        <input
          type="checkbox"
          checked={!!field.required}
          onChange={e => onChange({ required: e.target.checked })}
        />
      </label>
      <input
        className="sb-input sb-col--desc"
        value={field.description || ""}
        onChange={e => onChange({ description: e.target.value })}
        placeholder={fieldState.descriptionPlaceholder}
      />
      <button className="sb-del" onClick={onDelete} title={fieldState.removeTitle}>×</button>
      {field.type === "enum" && (
        <div className="sb-enum">
          <span className="sb-enum__label">{fieldState.enumLabel}</span>
          <div className="sb-enum__chips">
            {values.map((v, i) => (
              <span key={i} className="chip">
                <input
                  className="chip__input"
                  value={v}
                  onChange={e => onChange(window.MobKitFlowController.enumValueDraftPatch(field, i, e.target.value))}
                  onBlur={e => onChange(window.MobKitFlowController.enumValueCommitPatch(field, i, e.target.value))}
                />
                <button
                  className="chip__x"
                  onClick={() => onChange(window.MobKitFlowController.enumValueDeletePatch(field, i))}
                >×</button>
              </span>
            ))}
            <button
              className="chip chip--add"
              onClick={() => onChange(window.MobKitFlowController.enumValueAddPatch(field, fieldState.enumAddValue))}
            >{fieldState.enumAddLabel}</button>
          </div>
        </div>
      )}
    </div>
  );
}

function ProviderParamsEditor({ member, change, agentDetailView = null }) {
  const paramsState = window.MobKitFlowController.memberProviderParamsEditorState(member, agentDetailView);
  const [draft, setDraft] = React.useState(paramsState.text);
  const [error, setError] = React.useState("");
  React.useEffect(() => {
    setDraft(paramsState.text);
    setError("");
  }, [member.id, paramsState.text]);
  const commit = (next) => {
    setDraft(next);
    const result = window.MobKitFlowController.memberProviderParamsPatch(next, agentDetailView);
    if (!result.ok) {
      setError(result.error || paramsState.invalidJsonLabel);
      return;
    }
    setError("");
    change(result.patch);
  };
  return (
    <div className="field">
      <label className="field__label">{paramsState.label}</label>
      <textarea
        className="field__textarea field__textarea--mono"
        rows={paramsState.rows}
        value={draft}
        placeholder={paramsState.placeholder}
        onChange={e => commit(e.target.value)}
      />
      {error && <div className="hint__line" style={{ color: "var(--danger)" }}>{error}</div>}
    </div>
  );
}

// ── Skill access (realm picker + per-skill toggles, baked into the pack) ──
function SkillAccess({ studio, member, agentAccessView = null }) {
  const realms = studio.skillRealms || [];
  const initialSkillState = window.MobKitFlowController.memberSkillAccessState({ member, skillRealms: realms, accessView: agentAccessView });
  const [realmId, setRealmId] = React.useState(initialSkillState.realmId);
  const [inlineOpen, setInlineOpen] = React.useState(false);
  const [inlineLabel, setInlineLabel] = React.useState("");
  const [inlineContent, setInlineContent] = React.useState("");
  const [inlineError, setInlineError] = React.useState("");
  const skillState = window.MobKitFlowController.memberSkillAccessState({ member, skillRealms: realms, realmId, inlineOpen, accessView: agentAccessView });
  React.useEffect(() => {
    if (skillState.realmId !== realmId) setRealmId(skillState.realmId);
  }, [skillState.realmId, realmId]);
  const applySkillCascade = (result) => {
    if (!result.ok || !result.patch) return false;
    if (studio.snap) studio.snap();
    studio.setMembers(result.members);
    if (result.skillRealms !== realms) studio.setSkillRealms(result.skillRealms || []);
    return true;
  };
  const toggle = (sid) => {
    applySkillCascade(window.MobKitFlowController.memberSkillToggleCascadePatch({
      memberId: member.id,
      members: studio.members,
      skillRealms: realms,
    }, sid));
  };
  const removeSkill = (sid) => {
    applySkillCascade(window.MobKitFlowController.memberSkillRemoveCascadePatch({
      memberId: member.id,
      members: studio.members,
      skillRealms: realms,
    }, sid));
  };
  const addInlineSkill = () => {
    try {
      const result = window.MobKitFlowController.memberInlineSkillCascadePatch({
        memberId: member.id,
        members: studio.members,
        skillRealms: realms,
      }, {
        label: inlineLabel,
        content: inlineContent,
      }, agentAccessView);
      if (!applySkillCascade(result)) return;
      setRealmId(result.realmId);
      setInlineLabel("");
      setInlineContent("");
      setInlineError("");
      setInlineOpen(false);
    } catch (err) {
      setInlineError(err?.message || skillState.inlineErrorFallback);
    }
  };
  return (
    <>
      <div className="section__title section__title--row">
        <span>{skillState.sectionTitle}</span>
        <button className="ghost-btn" onClick={() => setInlineOpen(open => !open)}>
          {skillState.inlineToggleLabel}
        </button>
      </div>
      <div className="hint__line" style={{ marginBottom: 8 }}>
        {skillState.hint}
      </div>
      {inlineOpen && (
        <div className="inline-skill">
          <input
            className="field__input"
            value={inlineLabel}
            placeholder={skillState.inlineLabelPlaceholder}
            onChange={e => { setInlineLabel(e.target.value); setInlineError(""); }}
          />
          <textarea
            className="field__textarea field__textarea--mono"
            rows={skillState.inlineContentRows}
            value={inlineContent}
            placeholder={skillState.inlineContentPlaceholder}
            onChange={e => { setInlineContent(e.target.value); setInlineError(""); }}
          />
          <div className="row row--between">
            <span className="hint__line">{skillState.inlineCreateHint}</span>
            <button className="btn btn--ghost btn--sm" onClick={addInlineSkill}>{skillState.inlineAddLabel}</button>
          </div>
          {inlineError && <div className="hint__line" style={{ color: "var(--danger)" }}>{inlineError}</div>}
        </div>
      )}
      {!skillState.hasRealms ? (
        <div className="hint__line" style={{ color: "var(--warn)" }}>
          {skillState.noRealmsMessage}
        </div>
      ) : (
        <>
          <div className="field">
            <label className="field__label">{skillState.realmLabel}</label>
            <select className="field__select" value={skillState.realmId} onChange={e => setRealmId(e.target.value)}>
              {skillState.realmOptions.map(realm => <option key={realm.id} value={realm.id}>{realm.label}</option>)}
            </select>
          </div>
          <div className="skill-list">
            {skillState.skillRows.map(row => {
              return (
                <button key={row.id} className={row.className} onClick={() => toggle(row.id)}>
                  <span className="skill-row__check">{row.checkLabel}</span>
                  <span className="skill-row__text">
                    <span className="skill-row__name">{row.name}</span>
                    <span className="skill-row__desc">{row.desc}</span>
                  </span>
                </button>
              );
            })}
          </div>
        </>
      )}
      {skillState.selectedOutsideRealm.length > 0 && (
        <div className="skill-other">
          <span className="hint__line">{skillState.outsideRealmHeading}</span>
          {skillState.selectedOutsideRealm.map(skill => (
            <span key={skill.id} className={skill.className} title={skill.title}>
              {skill.label}
              <em>{skill.detail}</em>
              <button onClick={() => removeSkill(skill.id)}>{skill.removeLabel}</button>
            </span>
          ))}
        </div>
      )}
      {skillState.unavailableSelected.length > 0 && (
        <div className="skill-other">
          <span className="hint__line" style={{ color: "var(--warn)" }}>{skillState.unavailableHeading}</span>
          {skillState.unavailableSelected.map(sid => (
            <span key={sid.id} className={sid.className}>
              {sid.label}
              <button onClick={() => removeSkill(sid.id)}>{sid.removeLabel}</button>
            </span>
          ))}
        </div>
      )}
    </>
  );
}

window.AgentsView = AgentsView;
