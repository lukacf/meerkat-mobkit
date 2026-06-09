/* global React, ReactDOM, MOBKIT_BOOT, useStudioState, GraphEditor, Inspector, AddNodeMenu, DrySim, ValidateSheet, SourceDrawer, InlineSourceEditor, useTweaks, TweaksPanel, TweakSection, TweakRadio, AgentsView, BuilderView */

const TWEAK_DEFAULTS = /*EDITMODE-BEGIN*/{
  "edgeStyle": "text",
  "density": "comfortable",
  "theme": "light",
  "inspectorLayout": "right"
}/*EDITMODE-END*/;

const CATALOG_BOOT = {
  grid: MOBKIT_BOOT.GRID,
  cellXY: MOBKIT_BOOT.cellXY,
  template: MOBKIT_BOOT.template,
};

function App() {
  const [stage, setStage] = React.useState("draft");
  // Shared mob-flow step-tree — the single source of truth for both the
  // Build (editor) and Flow (diagram) views.
  const [flow, setFlow] = React.useState(() => window.MobKitFlowController.emptyAuthoringFlowState());
  const [stepSel, setStepSel] = React.useState(null);
  // Editor sub-mode: "basic" (vertical builder) | "advanced" (grid graph).
  const [editorMode, setEditorMode] = React.useState("basic");

  // view: "flows" (registry) | "editor" | "agents"
  const [view, setView] = React.useState("editor");
  const [flows, setFlows] = React.useState([]);
  const [currentFlowId, setCurrentFlowId] = React.useState("");
  const [templates, setTemplates] = React.useState([]);
  const [creating, setCreating] = React.useState(null); // { step, name, trigger } | null

  const [agentSel, setAgentSel] = React.useState(null);
  const [selection, setSelection] = React.useState({ kind: null, id: null });
  const [activeStepId, setActiveStepId] = React.useState(null);
  const [drySim, setDrySim] = React.useState(false);
  const [drySimKey, setDrySimKey] = React.useState(0);
  const [drySimDocument, setDrySimDocument] = React.useState(null);
  const [drySimPlan, setDrySimPlan] = React.useState(null);
  const [validate, setValidate] = React.useState(false);
  const [validationResults, setValidationResults] = React.useState([]);
  const [apiBusy, setApiBusy] = React.useState(false);
  const [contract, setContract] = React.useState(null);
  const contractSkillRealms = React.useRef([]);
  const [catalogs, setCatalogs] = React.useState(() => window.MobKitFlowController.emptyMobKitCatalogs(CATALOG_BOOT));
  const [sourceOpen, setSourceOpen] = React.useState(false);
  const [sourceDocument, setSourceDocument] = React.useState(null);
  const [inlineSourceOpen, setInlineSourceOpen] = React.useState(false);
  const [inlineSourceSurface, setInlineSourceSurface] = React.useState(null);
  const [inlineSourceDocument, setInlineSourceDocument] = React.useState(null);
  const [inlineSourceBusy, setInlineSourceBusy] = React.useState(false);
  const authoringRevision = React.useRef(0);
  const sourceProjectionVersion = React.useRef(0);
  const [addAt, setAddAt] = React.useState(null);
  const [deploySettings, setDeploySettings] = React.useState(() => window.MobKitFlowController.deployDefaultsFromSchema(null));
  const [deployCommandPreview, setDeployCommandPreview] = React.useState("");
  const [mobSettings, setMobSettings] = React.useState(() => window.MobKitFlowController.mobDefaultsFromSchema(null));
  const beginSourceProjection = React.useCallback(() => {
    sourceProjectionVersion.current += 1;
    return sourceProjectionVersion.current;
  }, []);
  const sourceProjectionIsCurrent = React.useCallback((requestToken) =>
    requestToken === sourceProjectionVersion.current, []);
  const currentAuthoringRevision = React.useCallback(() => authoringRevision.current, []);
  const authoringRevisionIsCurrent = React.useCallback((requestToken) =>
    requestToken === authoringRevision.current, []);
  const clearSourceProjection = React.useCallback(() => {
    sourceProjectionVersion.current += 1;
    setSourceOpen(false);
    setSourceDocument(null);
    setInlineSourceOpen(false);
    setInlineSourceSurface(null);
    setInlineSourceDocument(null);
    setInlineSourceBusy(false);
  }, []);
  const markDraft = React.useCallback(() => {
    authoringRevision.current += 1;
    setStage("draft");
    setValidationResults([]);
    clearSourceProjection();
    if (currentFlowId) {
      setFlows((rows) => window.MobKitFlowController.flowRegistryMarkDraftPatch(rows, currentFlowId));
    }
  }, [clearSourceProjection, currentFlowId]);
  const setAuthoringFlow = React.useCallback((next) => {
    markDraft();
    setFlow(next);
  }, [markDraft]);
  const studio = useStudioState({
    members: [],
    instances: [],
    edges: [],
    frames: [],
    schemas: [],
    skillRealms: [],
  }, markDraft, {
    flow,
    setFlow: setAuthoringFlow,
  });
  const setAuthoringDeploySettings = React.useCallback((next) => {
    markDraft();
    setDeploySettings(next);
  }, [markDraft]);
  const setAuthoringMobSettings = React.useCallback((next) => {
    markDraft();
    setMobSettings(next);
  }, [markDraft]);
  const graphProjectionSig = React.useRef("");
  const pendingGraphProjection = React.useRef(null);
  const skipNextGraphProjection = React.useRef(false);
  const persistedDocumentSig = React.useRef("");
  const importInputRef = React.useRef(null);
  const previousMembersRef = React.useRef([]);
  const hydratingDocumentRef = React.useRef(false);

  const [t, setTweak] = useTweaks(TWEAK_DEFAULTS);
  const canCreateAuthoring = !!catalogs.contractMeta.loaded && !contract?.error;
  const deployContractLoaded = !!catalogs.contractMeta.loaded;

  React.useEffect(() => {
    document.documentElement.dataset.ccVariant = "rams";
    document.documentElement.dataset.ccTheme = t.theme || "light";
  }, [t.theme]);

  React.useEffect(() => {
    let cancelled = false;
    setDeployCommandPreview("");
    if (!deployContractLoaded) {
      return () => {
        cancelled = true;
      };
    }
    window.MobKitFlowController.deployCommandPreview(deploySettings, {
      packPath: "<pack.mobpack>",
      prompt: deploySettings.prompt || "<prompt>",
    })
      .then((preview) => {
        if (!cancelled) {
          setDeployCommandPreview(preview?.command || "");
        }
      })
      .catch(() => {
        if (!cancelled) {
          setDeployCommandPreview("");
        }
      });
    return () => {
      cancelled = true;
    };
  }, [deploySettings, deployContractLoaded]);

  React.useEffect(() => {
    let cancelled = false;
    window.MobKitFlowController.configure({ rpcUrl: rpcUrlFromShell() });
    window.MobKitFlowController.loadSchema()
      .then(async (schema) => {
        window.MobKitFlowController.configureAuthoringMethodsFromSchema(schema);
        const catalogPayload = await window.MobKitFlowController.loadCatalogs();
        if (cancelled) return;
        const nextCatalogs = window.MobKitFlowController.mobKitCatalogsFromSchema(schema, CATALOG_BOOT, catalogPayload);
        setCatalogs(nextCatalogs);
        setDeploySettings(nextCatalogs.deployDefaults);
        setMobSettings(nextCatalogs.mobDefaults);
        contractSkillRealms.current = nextCatalogs.skillRealms;
        studio.setSkillRealms(nextCatalogs.skillRealms);
        const bootstrap = window.MobKitFlowController.flowCatalogBootstrapState(catalogPayload, {
          openEditor: view === "editor",
          deployDefaults: nextCatalogs.deployDefaults,
          mobDefaults: nextCatalogs.mobDefaults,
        });
        setTemplates(bootstrap.templates);
        setFlows(bootstrap.flows);
        if (bootstrap.initialHydration) {
          hydrateMobpackDocument(bootstrap.initialHydration.result, bootstrap.initialHydration.options);
        }
        setContract(schema);
      })
      .catch((error) => {
        if (!cancelled) setContract({ error: error?.message || String(error) });
      });
    return () => { cancelled = true; };
  }, []);

  // Keep the Flow grid in sync with the shared step-tree, so a mob loaded /
  // edited in Build shows in Flow and vice-versa (one source of truth).
  React.useEffect(() => {
    if (pendingGraphProjection.current) {
      const projection = pendingGraphProjection.current;
      pendingGraphProjection.current = null;
      skipNextGraphProjection.current = false;
      graphProjectionSig.current = window.MobKitFlowController.graphStructureSignature(projection.instances || [], projection.edges || []);
      studio.setInstances(projection.instances || []);
      studio.setEdges(projection.edges || []);
      studio.setFrames(projection.frames || []);
      queueMicrotask(() => {
        hydratingDocumentRef.current = false;
      });
      return;
    }
    if (skipNextGraphProjection.current) {
      skipNextGraphProjection.current = false;
      return;
    }
    if (editorMode === "advanced") return;
    if (!window.MobKitFlowController?.graphProjectionForFlow) return;
    const { instances, edges, frames } = window.MobKitFlowController.graphProjectionForFlow(flow, studio.members, contract);
    graphProjectionSig.current = window.MobKitFlowController.graphStructureSignature(instances, edges);
    studio.setInstances(instances);
    studio.setEdges(edges);
    studio.setFrames(frames || []);
  }, [flow, editorMode, contract]);

  React.useEffect(() => {
    if (editorMode !== "advanced") return;
    if (!window.MobKitFlowController?.graphToFlow) return;
    const sig = window.MobKitFlowController.graphStructureSignature(studio.instances, studio.edges);
    if (sig === graphProjectionSig.current) return;
    graphProjectionSig.current = sig;
    markDraft();
    skipNextGraphProjection.current = true;
    setFlow((current) => window.MobKitFlowController.graphToFlow({
      instances: studio.instances,
      edges: studio.edges,
      members: studio.members,
      previousFlow: current,
      contract,
    }));
  }, [editorMode, studio.instances, studio.edges, studio.members, markDraft, contract]);

  React.useEffect(() => {
    const previousMembers = previousMembersRef.current || [];
    const result = window.MobKitFlowController.reconcileAuthoringForMembers({
      flow,
      instances: studio.instances,
      edges: studio.edges,
      mobSettings,
      previousMembers,
      members: studio.members,
    });
    if (result.flow !== flow) setFlow(result.flow);
    if (result.edges !== studio.edges) studio.setEdges(result.edges);
    if (result.instances !== studio.instances) studio.setInstances(result.instances);
    if (result.mobSettings !== mobSettings) setMobSettings(result.mobSettings);
    previousMembersRef.current = studio.members;
  }, [studio.members]);

  React.useEffect(() => {
    if (!window.MobKitFlowController?.reconcileConditionFieldAvailability) return;
    if (hydratingDocumentRef.current) return;
    const result = window.MobKitFlowController.reconcileConditionFieldAvailability({
      flow,
      edges: studio.edges,
      members: studio.members,
      instances: studio.instances,
      schemas: studio.schemas,
    });
    const flowChanged = result.flow !== flow;
    const edgesChanged = result.edges !== studio.edges;
    if (!flowChanged && !edgesChanged) return;
    if (edgesChanged && studio.snap) {
      studio.snap();
    } else {
      markDraft();
    }
    if (flowChanged) setFlow(result.flow);
    if (edgesChanged) studio.setEdges(result.edges);
  }, [flow, studio.edges, studio.instances, studio.members, studio.schemas, markDraft]);

  React.useEffect(() => {
    if (!window.MobKitFlowController?.reconcileMemberSkillRefs) return;
    studio.setMembers((current) => window.MobKitFlowController.reconcileMemberSkillRefs(
      current,
      studio.skillRealms,
      { strictEmpty: !!catalogs.contractMeta.loaded },
    ));
  }, [studio.members, studio.skillRealms, catalogs.contractMeta.loaded]);

  React.useEffect(() => {
    if (!window.MobKitFlowController?.reconcileDeploySettingsWithContract) return;
    setDeploySettings((current) => window.MobKitFlowController.reconcileDeploySettingsWithContract(
      current,
      contract,
      catalogs.models,
      { strictEmptyModels: !!catalogs.contractMeta.loaded },
    ));
  }, [contract, catalogs.models, catalogs.contractMeta.loaded]);

  React.useEffect(() => {
    if (!window.MobKitFlowController?.reconcileMembersWithContract) return;
    studio.setMembers((current) => window.MobKitFlowController.reconcileMembersWithContract(
      current,
      contract,
      deploySettings,
      catalogs.models,
      catalogs.toolCatalog,
      {
        strictEmptyModels: !!catalogs.contractMeta.loaded,
        strictEmptyTools: !!catalogs.contractMeta.loaded,
      },
    ));
  }, [studio.members, contract, deploySettings, catalogs.models, catalogs.toolCatalog, catalogs.contractMeta.loaded]);

  React.useEffect(() => {
    if (!window.MobKitFlowController?.reconcileMobSettingsWithContract) return;
    setMobSettings((current) => window.MobKitFlowController.reconcileMobSettingsWithContract(
      current,
      contract,
    ));
  }, [contract]);

  const selectInstance = (id) => setSelection({ kind: "instance", id });
  const selectEdge = (id) => setSelection({ kind: "edge", id });
  const clearSelection = () => setSelection({ kind: null, id: null });

  React.useEffect(() => {
    const onKey = (e) => {
      const tg = e.target;
      if (tg.tagName === "INPUT" || tg.tagName === "TEXTAREA" || tg.tagName === "SELECT") return;
      if (e.key === "Backspace" || e.key === "Delete") {
        if (selection.kind === "instance") { studio.deleteInstance(selection.id); clearSelection(); }
        else if (selection.kind === "edge") { studio.deleteEdge(selection.id); clearSelection(); }
      }
      if ((e.metaKey || e.ctrlKey) && e.key === "z") {
        e.preventDefault();
        if (e.shiftKey) studio.redo(); else studio.undo();
      }
      if (e.key === "Escape") {
        clearSelection(); setAddAt(null);
        setDrySim(false); setValidate(false); setSourceOpen(false);
        if (creating) setCreating(null);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const handleRequestAdd = (col, row) => {
    const { x } = catalogs.cellXY(col, row);
    setAddAt({ col, row, x: x + catalogs.grid.cellW * 0.5 - 130, y: 90 });
  };

  const handlePick = (pick) => {
    if (!addAt) return;
    if (pick.kind === "memberInstance") {
      const instance = window.MobKitFlowController.graphMemberInstanceShape({
        memberId: pick.memberId,
        at: addAt,
        instances: studio.instances,
        contract,
      });
      if (instance) {
        studio.addInstance(instance);
        selectInstance(instance.id);
      }
    }
    if (pick.kind === "gate") {
      const inserted = window.MobKitFlowController.graphControlShape({
        gateKind: pick.gateKind,
        at: addAt,
        members: studio.members,
        instances: studio.instances,
        edges: studio.edges,
        flow,
        contract,
        graphView: catalogs.graphView,
      });
      if (inserted) {
        studio.snap();
        if (inserted.flow && inserted.flow !== flow) setAuthoringFlow(inserted.flow);
        studio.setInstances(current => window.MobKitFlowController.studioAppendInstancesPatch({
          instances: current,
          members: studio.members,
        }, inserted.instances).instances);
        studio.setEdges(current => window.MobKitFlowController.studioAppendEdgesPatch({
          edges: current,
          instances: [...studio.instances, ...inserted.instances],
        }, inserted.edges).edges);
        if (inserted.selectId) selectInstance(inserted.selectId);
      }
    }
    setAddAt(null);
  };

  const currentFlowSelection = window.MobKitFlowController.flowRegistrySelectionState(flows, currentFlowId);
  const currentFlow = currentFlowSelection.row;
  const effectiveAuthoringFlow = () => {
    if (editorMode !== "advanced" || !window.MobKitFlowController?.graphToFlow) return flow;
    return window.MobKitFlowController.graphToFlow({
      instances: studio.instances,
      edges: studio.edges,
      members: studio.members,
      previousFlow: flow,
      contract,
    });
  };
  const buildDocument = () => window.MobKitFlowController.buildDocument({
    flow: effectiveAuthoringFlow(),
    studio: { ...studio, mobSettings },
    currentFlow,
    deploySettings,
    contract,
  });
  const rememberCurrentDocument = (document, validation, nextStage = stage) => {
    const persistence = window.MobKitFlowController.flowRegistryDocumentPersistence({
      currentFlowId,
      document,
      validation,
      stage: nextStage,
    });
    if (!persistence.ok || !persistence.rowPatch) return;
    persistedDocumentSig.current = persistence.signature;
    setFlows((rows) => window.MobKitFlowController.flowRegistryRememberDocumentPatch(rows, persistence.rowPatch));
  };

  React.useEffect(() => {
    if (!currentFlowId || !currentFlow) return;
    let document;
    try {
      document = buildDocument();
    } catch {
      return;
    }
    const persistence = window.MobKitFlowController.flowRegistryDocumentPersistence({
      currentFlowId,
      document,
      validation: null,
      stage,
      previousSignature: persistedDocumentSig.current,
      skipIfUnchanged: true,
    });
    if (!persistence.changed || !persistence.rowPatch) return;
    persistedDocumentSig.current = persistence.signature;
    setFlows((rows) => window.MobKitFlowController.flowRegistryRememberDocumentPatch(rows, persistence.rowPatch));
  }, [
    currentFlowId,
    currentFlow,
    stage,
    editorMode,
    flow,
    studio.members,
    studio.instances,
    studio.edges,
    studio.frames,
    studio.schemas,
    studio.skillRealms,
    deploySettings,
    mobSettings,
  ]);

  const handleDrySim = async () => {
    const requestToken = currentAuthoringRevision();
    setApiBusy(true);
    try {
      const document = buildDocument();
      const plan = await window.MobKitFlowController.deployDocument(document, { execute: false });
      if (!authoringRevisionIsCurrent(requestToken)) return;
      const outcome = window.MobKitFlowController.deployOutcome(document, plan, { execute: false });
      window.__mobkitFlowLastDocument = document;
      window.__mobkitFlowLastDeployPlanTrace = plan;
      setDrySimDocument(document);
      setDrySimPlan(plan);
      rememberCurrentDocument(outcome.document, outcome.validation, outcome.stage);
      setValidationResults(outcome.validationRows);
      setStage(outcome.stage);
      setDrySim(true);
      setDrySimKey(k => k + 1);
    } catch (error) {
      if (!authoringRevisionIsCurrent(requestToken)) return;
      const outcome = window.MobKitFlowController.deployErrorOutcome(error, { execute: false, errorView: catalogs.errorView });
      setValidationResults(outcome.validationRows);
      setValidate(true);
      setStage(outcome.stage);
    } finally {
      setApiBusy(false);
    }
  };

  const exportCurrentSourceDocument = async (requestToken) => {
    const document = buildDocument();
    const result = await window.MobKitFlowController.exportDocument(document);
    const projection = window.MobKitFlowController.sourceDocumentFromExport(document, result, {
      sourceView: catalogs.sourceView,
    });
    if (!sourceProjectionIsCurrent(requestToken)) return null;
    window.__mobkitFlowLastDocument = projection.document;
    window.__mobkitFlowLastSource = result;
    rememberCurrentDocument(projection.document, projection.validation, projection.stage);
    setValidationResults(projection.validationRows);
    setStage(projection.stage);
    return projection.sourceDocument;
  };

  const handleSource = async () => {
    if (sourceOpen) {
      clearSourceProjection();
      return;
    }
    const requestToken = beginSourceProjection();
    setApiBusy(true);
    try {
      const nextSourceDocument = await exportCurrentSourceDocument(requestToken);
      if (!nextSourceDocument || !sourceProjectionIsCurrent(requestToken)) return;
      setSourceDocument(nextSourceDocument);
      setSourceOpen(true);
    } catch (error) {
      if (!sourceProjectionIsCurrent(requestToken)) return;
      const outcome = window.MobKitFlowController.sourceErrorOutcome(error, { errorView: catalogs.errorView });
      setValidationResults(outcome.validationRows);
      setValidate(true);
      setStage(outcome.stage);
    } finally {
      setApiBusy(false);
    }
  };

  React.useEffect(() => {
    clearSourceProjection();
  }, [view, editorMode, clearSourceProjection]);

  const handleInlineSource = async (surface = "basic") => {
    const requestToken = beginSourceProjection();
    setInlineSourceSurface(surface);
    setInlineSourceOpen(true);
    setInlineSourceBusy(true);
    setApiBusy(true);
    try {
      const nextSourceDocument = await exportCurrentSourceDocument(requestToken);
      if (!nextSourceDocument || !sourceProjectionIsCurrent(requestToken)) return;
      setInlineSourceDocument(nextSourceDocument);
    } catch (error) {
      if (!sourceProjectionIsCurrent(requestToken)) return;
      const outcome = window.MobKitFlowController.sourceErrorOutcome(error, { errorView: catalogs.errorView });
      setValidationResults(outcome.validationRows);
      setValidate(true);
      setStage(outcome.stage);
    } finally {
      setInlineSourceBusy(false);
      setApiBusy(false);
    }
  };

  const handleValidate = async () => {
    const requestToken = currentAuthoringRevision();
    setApiBusy(true);
    try {
      const document = buildDocument();
      const result = await window.MobKitFlowController.validateDocument(document);
      if (!authoringRevisionIsCurrent(requestToken)) return;
      const outcome = window.MobKitFlowController.validationOutcome(document, result);
      window.__mobkitFlowLastDocument = document;
      window.__mobkitFlowLastValidation = result;
      rememberCurrentDocument(outcome.document, outcome.validation, outcome.stage);
      setValidationResults(outcome.validationRows);
      setStage(outcome.stage);
    } catch (error) {
      if (!authoringRevisionIsCurrent(requestToken)) return;
      const outcome = window.MobKitFlowController.validationErrorOutcome(error, { errorView: catalogs.errorView });
      setValidationResults(outcome.validationRows);
      setStage(outcome.stage);
    } finally {
      if (authoringRevisionIsCurrent(requestToken)) {
        setValidate(true);
      }
      setApiBusy(false);
    }
  };

  const handlePublish = async () => {
    const requestToken = currentAuthoringRevision();
    setApiBusy(true);
    try {
      const document = buildDocument();
      const result = await window.MobKitFlowController.exportDocument(document);
      if (!authoringRevisionIsCurrent(requestToken)) return;
      const outcome = window.MobKitFlowController.exportOutcome(document, result);
      window.__mobkitFlowLastDocument = document;
      window.__mobkitFlowLastExport = result;
      rememberCurrentDocument(outcome.document, outcome.validation, outcome.stage);
      if (!window.__mobkitFlowDisableDownload) {
        downloadExportResult(result);
      }
      setValidationResults(outcome.validationRows);
      setStage(outcome.stage);
      setValidate(false);
    } catch (error) {
      if (!authoringRevisionIsCurrent(requestToken)) return;
      const outcome = window.MobKitFlowController.exportErrorOutcome(error, { errorView: catalogs.errorView });
      setValidationResults(outcome.validationRows);
      setValidate(true);
      setStage(outcome.stage);
    } finally {
      setApiBusy(false);
    }
  };

  const handleDeploy = async ({ execute }) => {
    const requestToken = currentAuthoringRevision();
    setApiBusy(true);
    try {
      const document = buildDocument();
      const result = await window.MobKitFlowController.deployDocument(document, { execute });
      if (!authoringRevisionIsCurrent(requestToken)) return;
      const outcome = window.MobKitFlowController.deployOutcome(document, result, { execute });
      window.__mobkitFlowLastDocument = document;
      window.__mobkitFlowLastDeploy = result;
      rememberCurrentDocument(outcome.document, outcome.validation, outcome.stage);
      setValidationResults(outcome.validationRows);
      setStage(outcome.stage);
      setValidate(true);
    } catch (error) {
      if (!authoringRevisionIsCurrent(requestToken)) return;
      const outcome = window.MobKitFlowController.deployErrorOutcome(error, { execute, errorView: catalogs.errorView });
      setValidationResults(outcome.validationRows);
      setValidate(true);
      setStage(outcome.stage);
    } finally {
      setApiBusy(false);
    }
  };
  const handleDeployPlan = () => handleDeploy({ execute: false });
  const handleDeployRun = () => handleDeploy({ execute: true });

  const hydrateMobpackDocument = (result, options = {}) => {
    const hydration = window.MobKitFlowController.hydrateMobpackDocumentState(result, {
      id: options.id,
      existingRows: options.existingRows,
      addToRegistry: options.addToRegistry,
      openEditor: options.openEditor,
      flowRow: options.flowRow || null,
      deployDefaults: options.deployDefaults || catalogs.deployDefaults,
      mobDefaults: options.mobDefaults || catalogs.mobDefaults,
      contractSkillRealms: contractSkillRealms.current,
      contract,
      errorView: catalogs.errorView,
    });
    if (hydration.ok === false) {
      setValidationResults(hydration.validationRows || []);
      setStage(hydration.stage || "draft");
      setValidate(true);
      return;
    }
    const hydrationPersistence = window.MobKitFlowController.flowRegistryDocumentPersistence({
      currentFlowId: hydration.id,
      document: hydration.document,
      validation: hydration.validation,
      stage: hydration.stage,
    });
    if (hydrationPersistence.ok) {
      persistedDocumentSig.current = hydrationPersistence.signature;
    }
    hydratingDocumentRef.current = true;
    setCatalogs((current) => window.MobKitFlowController.catalogSkillRealmsPatch(current, hydration.skillRealms));
    studio.setSkillRealms(hydration.skillRealms);
    studio.setMembers(hydration.members);
    studio.setSchemas(hydration.schemas);
    pendingGraphProjection.current = hydration.graphProjection;
    setFlow(hydration.flow);
    setStepSel(null);
    setDeploySettings(hydration.deploySettings);
    setMobSettings(hydration.mobSettings);
    if (!pendingGraphProjection.current) {
      queueMicrotask(() => {
        hydratingDocumentRef.current = false;
      });
    }
    if (hydration.addToRegistry) {
      setFlows(fs => window.MobKitFlowController.flowRegistryUpsertRowPatch(fs, hydration.registryRow));
    }
    setCurrentFlowId(hydration.id);
    setStage(hydration.stage);
    setValidationResults(hydration.validationRows);
    if (hydration.openEditor) setView("editor");
  };

  const hydrateImportedDocument = (result) => {
    hydrateMobpackDocument(result, { existingRows: flows });
  };

  const handleImportFile = async (event) => {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) return;
    setApiBusy(true);
    try {
      const result = await window.MobKitFlowController.importDocument(await importParamsFromFile(file));
      window.__mobkitFlowLastImport = result;
      hydrateImportedDocument(result);
    } catch (error) {
      const outcome = window.MobKitFlowController.importErrorOutcome(error, { filename: file.name, errorView: catalogs.errorView });
      setValidationResults(outcome.validationRows);
      setValidate(true);
      setStage(outcome.stage);
    } finally {
      setApiBusy(false);
    }
  };

  const shellState = window.MobKitFlowController.topRailState({ contract, deploySettings, stage, view, theme: t.theme, deployView: catalogs.deployView });

  return (
    <div className={"app density--" + t.density + " inspector--" + t.inspectorLayout + " view--" + view}>
      <TopRail
        studio={studio} stage={stage}
        view={view} setView={setView}
        editorMode={editorMode} setEditorMode={setEditorMode}
        currentFlowName={currentFlow?.name || "—"}
        contract={contract}
        theme={t.theme}
        railState={shellState}
        onToggleTheme={() => setTweak("theme", t.theme === "dark" ? "light" : "dark")}
        onValidate={handleValidate}
        onPublish={handlePublish}
        onDeployPlan={handleDeployPlan}
        onDeployRun={handleDeployRun}
        onImport={() => importInputRef.current?.click()}
        onDrySim={handleDrySim}
        onYaml={handleSource}
        deploySettings={deploySettings}
      />
      <input
        ref={importInputRef}
        type="file"
        accept=".mobpack,.json,.toml,application/json,application/gzip"
        style={{ display: "none" }}
        onChange={handleImportFile}
      />

      {view === "flows" && (
        <FlowsView
          flows={flows}
          currentFlowId={currentFlowId}
          onOpen={(id) => {
            const selection = window.MobKitFlowController.flowRegistrySelectionState(flows, id);
            if (selection.hydration) {
              hydrateMobpackDocument(selection.hydration.result, selection.hydration.options);
              return;
            }
            if (!selection.fallback) return;
            setCurrentFlowId(selection.fallback.currentFlowId);
            setStage(selection.fallback.stage);
            setView(selection.fallback.view);
          }}
          canCreate={canCreateAuthoring}
          flowRegistryView={catalogs.flowRegistryView}
          onNew={() => {
            if (!canCreateAuthoring) return;
            setCreating(window.MobKitFlowController.newFlowInitialState({ blankTemplate: catalogs.blankMobpack }));
          }}
        />
      )}

      {view === "editor" && (
        <ModeToggle mode={editorMode} setMode={setEditorMode} railState={shellState} />
      )}

      {view === "editor" && editorMode === "advanced" && (
        <div className="stage-area" onClick={(e) => { if (e.target === e.currentTarget) setAddAt(null); }}>
          <GraphEditor
            state={studio}
            selection={selection}
            selectInstance={selectInstance}
            selectEdge={selectEdge}
            clearSelection={clearSelection}
            activeStepId={activeStepId}
            edgeStyle={t.edgeStyle}
            density={t.density}
            onRequestAdd={handleRequestAdd}
            onOpenSourceFile={() => handleInlineSource("graph")}
            memberFocus={null}
            grid={catalogs.grid}
            contract={contract}
            graphView={catalogs.graphView}
          />
            <InlineSourceEditor
              open={inlineSourceOpen && inlineSourceSurface === "graph"}
              onClose={clearSourceProjection}
              state={inlineSourceDocument}
              busy={inlineSourceBusy}
              surface="graph"
              sourceView={catalogs.sourceView}
          />
          <AddNodeMenu
            at={addAt}
            members={studio.members}
            contract={contract}
            graphView={catalogs.graphView}
            onPick={handlePick}
            onClose={() => setAddAt(null)}
            onJumpToAgents={(id) => { setAddAt(null); setView("agents"); setAgentSel({ kind: "agent", id }); }}
          />
          <aside className="inspector">
            <Inspector
              studio={studio}
              selection={selection}
              flow={flow}
              template={currentFlow}
              templateSeed={catalogs.template}
              templateView={catalogs.graphTemplateView}
              launchView={catalogs.launchView}
              graphView={catalogs.graphView}
              conditionView={catalogs.conditionView}
              contract={contract}
              deploySettings={deploySettings}
              selectMember={(id) => { setView("agents"); setAgentSel({ kind: "agent", id }); }}
              selectInstance={selectInstance}
              clearSelection={clearSelection}
            />
          </aside>
        </div>
      )}

      {view === "editor" && editorMode === "basic" && (
        <BuilderView
          studio={studio}
          mode="build"
          flow={flow}
          setFlow={setAuthoringFlow}
          sel={stepSel}
          setSel={setStepSel}
          onShowSource={() => handleInlineSource("basic")}
          sourceOpen={inlineSourceOpen && inlineSourceSurface === "basic"}
          sourceDocument={inlineSourceDocument}
          sourceBusy={inlineSourceBusy}
          onCloseSource={clearSourceProjection}
          contract={contract}
          toolCatalog={catalogs.toolCatalog}
          sourceView={catalogs.sourceView}
          basicView={catalogs.basicView}
          launchView={catalogs.launchView}
          conditionView={catalogs.conditionView}
        />
      )}

      {view === "agents" && (
        <AgentsView
          studio={studio}
          agentSel={agentSel}
          setAgentSel={setAgentSel}
          contract={contract}
          deploySettings={deploySettings}
          flow={flow}
          setFlow={setAuthoringFlow}
          mobSettings={mobSettings}
          setMobSettings={setAuthoringMobSettings}
          toolCatalog={catalogs.toolCatalog}
          modelCatalog={catalogs.models}
          agentDefinitions={catalogs.agentDefinitions}
          agentView={catalogs.agentView}
          agentDetailView={catalogs.agentDetailView}
          agentAccessView={catalogs.agentAccessView}
          schemaView={catalogs.schemaView}
        />
      )}

      {creating && (
        <NewFlowModal
          state={creating}
          setState={setCreating}
          templateOptions={window.MobKitFlowController.newFlowTemplateOptions(templates, {
            canCreateBlank: canCreateAuthoring,
            blankTemplate: catalogs.blankMobpack,
          })}
          newFlowView={catalogs.newFlowView}
          onCreate={(spec) => {
            if (!canCreateAuthoring) return;
            const draft = window.MobKitFlowController.createFlowDraftFromSpec({
              spec,
              templates,
              existingRows: flows,
              blankTemplate: catalogs.blankMobpack,
              deploySettings,
              mobSettings,
            });
            if (!draft?.document || !draft?.row) return;
            setFlows(fs => window.MobKitFlowController.flowRegistryAppendRowPatch(fs, draft.row));
            hydrateMobpackDocument({ document: draft.document, validation: null }, {
              id: draft.id,
              flowRow: draft.row,
              addToRegistry: false,
            });
            setCreating(null);
          }}
        />
      )}

      <DrySim open={drySim} onClose={() => setDrySim(false)} onActiveStep={setActiveStepId} runKey={drySimKey} document={drySimDocument} plan={drySimPlan} deployView={catalogs.deployView} />
      <ValidateSheet open={validate} onClose={() => setValidate(false)} onPublish={handlePublish} onDeployPlan={handleDeployPlan} onDeployRun={handleDeployRun} results={validationResults} stage={stage} deployView={catalogs.deployView} />
      <SourceDrawer open={sourceOpen} onClose={clearSourceProjection} state={sourceDocument} sourceView={catalogs.sourceView} />
      <Tweaks
        t={t}
        setTweak={setTweak}
        flows={flows}
        currentFlowId={currentFlowId}
        deploySettings={deploySettings}
        setDeploySettings={setAuthoringDeploySettings}
        mobSettings={mobSettings}
        setMobSettings={setAuthoringMobSettings}
        members={studio.members}
        modelCatalog={catalogs.models}
        contract={contract}
        deployCommandPreview={deployCommandPreview}
        settingsView={catalogs.settingsView}
        onLoadFlow={(id) => {
          const selection = window.MobKitFlowController.flowRegistrySelectionState(flows, id);
          if (!selection.hydration) return;
          hydrateMobpackDocument(selection.hydration.result, selection.hydration.options);
        }}
      />
    </div>
  );
}

function rpcUrlFromShell() {
  const meta = document.querySelector('meta[name="mobkit-base-url"]');
  const base = (meta?.getAttribute("content") || "").trim().replace(/\/+$/, "");
  return `${base}/flow-editor/rpc`;
}

function downloadExportResult(result) {
  const content = String(result?.content_base64 || "").trim();
  const mediaType = String(result?.media_type || "").trim();
  const filename = String(result?.filename || "").trim();
  if (!content) throw new Error("mobkit/mobpacks/export did not return content_base64");
  if (!mediaType) throw new Error("mobkit/mobpacks/export did not return media_type");
  if (!filename) throw new Error("mobkit/mobpacks/export did not return filename");
  const bytes = Uint8Array.from(atob(content), (char) => char.charCodeAt(0));
  const blob = new Blob([bytes], { type: mediaType });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  URL.revokeObjectURL(url);
}

async function importParamsFromFile(file) {
  const bytes = new Uint8Array(await file.arrayBuffer());
  const filename = file.name || "";
  const mediaType = file.type || "";
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return window.MobKitFlowController.importParamsFromDecodedFile({
    filename,
    mediaType,
    text: new TextDecoder("utf-8").decode(bytes),
    contentBase64: btoa(binary),
  });
}

function TopRail({ studio, stage, view, setView, editorMode, setEditorMode, currentFlowName, theme, railState, onToggleTheme, onValidate, onPublish, onDeployPlan, onDeployRun, onImport, onDrySim, onYaml, contract, deploySettings }) {
  return (
    <header className="toprail">
      <div className="brand">
        <span className="dot" />
        <span>{railState.brandLabel}</span>
      </div>
      <nav className="viewtabs">
        <button className={"viewtab" + (view === "flows" || view === "editor" ? " is-current" : "")} onClick={() => setView(view === "editor" ? "flows" : "editor")}>{railState.flowsTabLabel}</button>
        <button className={"viewtab" + (view === "agents" ? " is-current" : "")} onClick={() => setView("agents")}>{railState.agentsTabLabel}</button>
      </nav>
      <div className="mob-status" title={railState.mobStatusTitle}>
        <span className="glyph" />
        <span className="name">{railState.mobFileLabel}</span>
        <span className="env">· {railState.contractState}</span>
      </div>
      <div className="mob-status mob-status--env" title={railState.deployCommand}>
        <span className="env">{railState.deployPrefixLabel}</span>
        <span className="name">{railState.deploySurface}</span>
      </div>
      <nav className="crumbs">
        {railState.inEditor && (
          <>
            <button className="crumb crumb--link" onClick={() => setView("flows")}>{railState.flowsCrumbLabel}</button>
            <span className="crumb crumb--sep">{railState.crumbSeparator}</span>
            <span className="crumb is-current">{currentFlowName}</span>
          </>
        )}
      </nav>
      <div className="actions">
        {railState.inEditor && (
          <>
            <span className="stage" data-state={stage}><span className="glyph" />{stage}</span>
            <button className="btn btn--ghost btn--sm" onClick={onDrySim}>{railState.planTraceLabel}</button>
            <button className="btn btn--ghost btn--sm" onClick={onImport}>{railState.importLabel}</button>
            <button className="btn btn--ghost btn--sm" onClick={onValidate}>{railState.validateLabel}</button>
            <button className="btn btn--primary btn--sm" disabled={railState.deployActionsDisabled} onClick={onPublish}>{railState.publishLabel}</button>
            <button className="btn btn--ghost btn--sm" disabled={railState.deployActionsDisabled} onClick={onDeployPlan}>{railState.deployPlanLabel}</button>
            <button className="btn btn--primary btn--sm" disabled={railState.deployActionsDisabled} onClick={onDeployRun}>{railState.deployLabel}</button>
          </>
        )}
        <button
          className="btn btn--ghost btn--sm theme-toggle"
          onClick={onToggleTheme}
          title={railState.themeToggleTitle}
        >
          {railState.themeToggleLabel}
        </button>
      </div>
    </header>
  );
}

// ── Flows registry view ───────────────────────────────────────────
function FlowsView({ flows, currentFlowId, onOpen, onNew, canCreate, flowRegistryView = null }) {
  const registryState = window.MobKitFlowController.flowRegistryViewState(flows, currentFlowId, { canCreate, flowRegistryView });
  return (
    <div className="flows-view">
      <div className="flows-view__head">
        <div>
          <div className="inspector__eyebrow">{registryState.eyebrow}</div>
          <div className="flows-view__title">{registryState.title}</div>
        </div>
        <button
          className="btn btn--primary"
          disabled={registryState.createDisabled}
          title={registryState.createTitle}
          onClick={onNew}
        >{registryState.createLabel}</button>
      </div>
      <div className="flows-list">
        <div className="flows-list__head">
          {registryState.columns.map(column => <span key={column.key}>{column.label}</span>)}
        </div>
        {registryState.rows.map(f => (
          <button key={f.id} className={f.className} onClick={() => onOpen(f.id)}>
            <span className="flows-list__name">{f.name}</span>
            <span className="flows-list__sub">{f.trigger}</span>
            <span className="flows-list__sub">{f.version}</span>
            <span className="stage" data-state={f.stage}><span className="glyph" />{f.stage}</span>
          </button>
        ))}
      </div>
    </div>
  );
}

// ── New Flow modal (3-step) ───────────────────────────────────────
function NewFlowModal({ state, setState, onCreate, templateOptions = [], newFlowView = null }) {
  const set = (patch) => setState({ ...state, ...patch });
  const modalState = window.MobKitFlowController.newFlowModalState(state, templateOptions, newFlowView);

  return (
    <div className="modal-backdrop" onClick={() => setState(null)}>
      <div className="modal modal--new" onClick={e => e.stopPropagation()}>
        <div className="modal__head">
          <div className="inspector__eyebrow">{modalState.eyebrow}</div>
          <button className="btn btn--ghost btn--sm" onClick={() => setState(null)}>{modalState.closeLabel}</button>
        </div>
        {modalState.step === 1 && (
          <div className="modal__body">
            <div className="field">
              <label className="field__label">{modalState.nameLabel}</label>
              <input className="field__input" autoFocus placeholder={modalState.namePlaceholder} value={modalState.name} onChange={e => set({ name: e.target.value })} />
            </div>
            <div className="field">
              <label className="field__label">{modalState.triggerLabel}</label>
              <input className="field__input" placeholder={modalState.triggerPlaceholder} value={modalState.trigger} onChange={e => set({ trigger: e.target.value })} />
            </div>
          </div>
        )}
        {modalState.step === 2 && (
          <div className="modal__body">
            <div className="field__label">{modalState.startFromLabel}</div>
            <div className="template-grid">
              {modalState.options.map(opt => (
                <button key={opt.id} className={opt.className} disabled={opt.disabled} onClick={() => set({ template: opt.id })}>
                  <div className="template-card__tier">{opt.tier}</div>
                  <div className="template-card__name">{opt.label}</div>
                  <div className="template-card__sub">{opt.sub}</div>
                </button>
              ))}
            </div>
          </div>
        )}
        <div className="modal__foot">
          {modalState.step > 1 ? <button className="btn btn--ghost btn--sm" onClick={() => set({ step: modalState.step - 1 })}>{modalState.backLabel}</button> : <span />}
          {modalState.step < 2 ? (
            <button className="btn btn--primary btn--sm" disabled={modalState.nextDisabled} onClick={() => set({ step: 2 })}>{modalState.nextLabel}</button>
          ) : (
            <button
              className="btn btn--primary btn--sm"
              disabled={modalState.createDisabled}
              onClick={() => onCreate({ name: modalState.name, trigger: modalState.trigger, template: modalState.template })}
            >{modalState.createLabel}</button>
          )}
        </div>
      </div>
    </div>
  );
}

function ModeToggle({ mode, setMode, railState }) {
  return (
    <div className="modetoggle">
      <button className={"modetoggle__opt" + (mode === "basic" ? " is-active" : "")} onClick={() => setMode("basic")} title={railState.basicModeTitle}>
        <svg width="13" height="13" viewBox="0 0 13 13" fill="none" stroke="currentColor" strokeWidth="1.3"><rect x="1.5" y="2.2" width="10" height="2.2"/><rect x="1.5" y="6.6" width="10" height="2.2"/></svg>
        <span>{railState.basicModeLabel}</span>
      </button>
      <button className={"modetoggle__opt" + (mode === "advanced" ? " is-active" : "")} onClick={() => setMode("advanced")} title={railState.graphModeTitle}>
        <svg width="13" height="13" viewBox="0 0 13 13" fill="none" stroke="currentColor" strokeWidth="1.3"><rect x="1" y="4.5" width="4" height="4"/><rect x="8" y="1" width="4" height="4"/><rect x="8" y="8" width="4" height="4"/><path d="M5 6.5h1.6M6.6 6.5V3h1.4M6.6 6.5V10h1.4"/></svg>
        <span>{railState.graphModeLabel}</span>
      </button>
    </div>
  );
}

function Tweaks({ t, setTweak, flows = [], currentFlowId, deploySettings, setDeploySettings, mobSettings, setMobSettings, members = [], modelCatalog = [], contract, deployCommandPreview, settingsView = null, onLoadFlow }) {
  const setDeployField = (field, value) => setDeploySettings((current) =>
    window.MobKitFlowController.deploySettingsPatch(current, { [field]: value }, { contract, modelCatalog })
  );
  const setMobField = (field, value) => setMobSettings((current) =>
    window.MobKitFlowController.mobSettingsPatch(current, { [field]: value }, { contract })
  );
  const controlState = window.MobKitFlowController.tweaksControlState({
    flows,
    deploySettings,
    mobSettings,
    members,
    modelCatalog,
    contract,
    settingsView,
  });
  return (
    <TweaksPanel title={controlState.panelTitle}>
      <TweakSection title={controlState.loadMobTitle}>
        <TweakSelect
          label={controlState.loadMobLabel}
          value={currentFlowId || ""}
          options={controlState.loadableFlowOptions}
          onChange={(id) => { onLoadFlow && onLoadFlow(id); }}
        />
      </TweakSection>
      <TweakSection title={controlState.canvasTitle}>
        <TweakRadio label={controlState.edgeStyleLabel} value={t.edgeStyle} onChange={v => setTweak("edgeStyle", v)}
          options={controlState.edgeStyleOptions} />
        <TweakRadio label={controlState.densityLabel} value={t.density} onChange={v => setTweak("density", v)}
          options={controlState.densityOptions} />
      </TweakSection>
      <TweakSection title={controlState.themeTitle}>
        <TweakRadio label={controlState.themeModeLabel} value={t.theme} onChange={v => setTweak("theme", v)}
          options={controlState.themeModeOptions} />
      </TweakSection>
      <TweakSection title={controlState.mobTitle}>
        <TweakSelect
          label={controlState.orchestratorLabel}
          value={mobSettings.orchestrator || ""}
          options={controlState.profileOptions}
          onChange={v => setMobField("orchestrator", v)}
        />
        <TweakRadio label={controlState.autoWireLabel} value={mobSettings.autoWireOrchestrator ? "yes" : "no"} onChange={v => setMobField("autoWireOrchestrator", v === "yes")}
          options={controlState.autoWireOptions} />
        <RoleWiringEditor
          value={mobSettings.roleWiring || []}
          profileOptions={controlState.profileChoices}
          settingsView={settingsView}
          onChange={(roleWiring) => setMobField("roleWiring", roleWiring)}
        />
        <TweakSelect label={controlState.defaultBackendLabel} value={mobSettings.backendDefault || ""} onChange={v => setMobField("backendDefault", v)}
          options={controlState.mobBackendOptions} />
        {(mobSettings.backendDefault === "external" || mobSettings.externalAddressBase) && (
          <TweakText label={controlState.externalBaseLabel} value={mobSettings.externalAddressBase || ""} placeholder={controlState.externalBasePlaceholder} onChange={v => setMobField("externalAddressBase", v)} />
        )}
        <AdvancedMobSettingsEditor
          value={mobSettings.advanced || {}}
          settingsView={settingsView}
          onChange={(advanced) => setMobField("advanced", advanced)}
        />
      </TweakSection>
      <TweakSection title={controlState.deployTitle}>
        <TweakSelect label={controlState.surfaceLabel} value={deploySettings.surface} onChange={v => setDeployField("surface", v)}
          options={controlState.surfaceOptions} />
        <TweakSelect label={controlState.trustLabel} value={deploySettings.trustPolicy} onChange={v => setDeployField("trustPolicy", v)}
          options={controlState.trustOptions} />
        <TweakSelect
          label={controlState.modelLabel}
          value={deploySettings.model || ""}
          options={controlState.modelOptions}
          onChange={v => setDeployField("model", v)}
        />
        <TweakText label={controlState.durationLabel} value={deploySettings.maxDuration || ""} placeholder={controlState.durationPlaceholder} onChange={v => setDeployField("maxDuration", v)} />
        <TweakNumber label={controlState.toolCallsLabel} value={deploySettings.maxToolCalls ?? ""} min={controlState.toolCallsMin} max={controlState.toolCallsMax} onChange={v => setDeployField("maxToolCalls", v)} />
        <TweakNumber label={controlState.tokensLabel} value={deploySettings.maxTotalTokens ?? ""} min={controlState.tokensMin} max={controlState.tokensMax} onChange={v => setDeployField("maxTotalTokens", v)} />
        <TweakRadio label={controlState.realmLabel} value={deploySettings.isolated ? "isolated" : "shared"} onChange={v => setDeployField("isolated", v === "isolated")}
          options={controlState.realmOptions} />
        {!deploySettings.isolated && <TweakText label={controlState.realmIdLabel} value={deploySettings.realm || ""} placeholder={controlState.realmIdPlaceholder} onChange={v => setDeployField("realm", v)} />}
        <TweakSelect label={controlState.backendLabel} value={deploySettings.realmBackend || ""} onChange={v => setDeployField("realmBackend", v)}
          options={controlState.realmBackendOptions} />
        <TweakText label={controlState.promptLabel} value={deploySettings.prompt || ""} placeholder={controlState.promptPlaceholder} onChange={v => setDeployField("prompt", v)} />
        <div className="twk-row">
          <div className="twk-lbl"><span>{controlState.commandLabel}</span></div>
          <code className="deploy-command">{deployCommandPreview || controlState.commandFallback}</code>
        </div>
      </TweakSection>
      <TweakSection title={controlState.inspectorTitle}>
        <TweakRadio label={controlState.inspectorLayoutLabel} value={t.inspectorLayout} onChange={v => setTweak("inspectorLayout", v)}
          options={controlState.inspectorLayoutOptions} />
      </TweakSection>
    </TweaksPanel>
  );
}

function RoleWiringEditor({ value, profileOptions, settingsView, onChange }) {
  const wiringState = window.MobKitFlowController.mobRoleWiringEditorState(value, profileOptions, settingsView);
  const updateRule = (index, patch) => {
    onChange(window.MobKitFlowController.mobRoleWiringUpdatePatch(wiringState.wiring, index, patch, wiringState.options));
  };
  const removeRule = (index) => {
    onChange(window.MobKitFlowController.mobRoleWiringDeletePatch(wiringState.wiring, index));
  };
  const addRule = () => {
    onChange(window.MobKitFlowController.mobRoleWiringAddPatch(wiringState.wiring, wiringState.options));
  };
  return (
    <div className="twk-row">
      <div className="twk-lbl">
        <span>{wiringState.label}</span>
        <span>{wiringState.countLabel}</span>
      </div>
      <div style={{ display: "grid", gap: 6 }}>
        {wiringState.wiring.map((rule, index) => (
          <div key={`${rule.a}:${rule.b}:${index}`} style={{ display: "grid", gridTemplateColumns: "1fr 1fr 26px", gap: 6 }}>
            <select className="twk-field" value={rule.a} onChange={e => updateRule(index, { a: e.target.value })}>
              {wiringState.options.map(option => <option key={option.value} value={option.value}>{option.label}</option>)}
            </select>
            <select className="twk-field" value={rule.b} onChange={e => updateRule(index, { b: e.target.value })}>
              {wiringState.options.map(option => <option key={option.value} value={option.value}>{option.label}</option>)}
            </select>
            <button className="twk-field" style={{ padding: 0 }} type="button" onClick={() => removeRule(index)}>×</button>
          </div>
        ))}
        <button className="twk-field" type="button" disabled={wiringState.addDisabled} onClick={addRule}>{wiringState.addLabel}</button>
      </div>
    </div>
  );
}

function AdvancedMobSettingsEditor({ value, settingsView, onChange }) {
  const advancedState = window.MobKitFlowController.advancedMobSettingsEditorState(value, settingsView);
  const [draft, setDraft] = React.useState(advancedState.text);
  const [error, setError] = React.useState("");
  React.useEffect(() => {
    setDraft(advancedState.text);
    setError("");
  }, [advancedState.text]);
  const commit = (next) => {
    setDraft(next);
    const result = window.MobKitFlowController.advancedMobSettingsDraftPatch(next, settingsView);
    setError(result.error || "");
    if (result.ok) onChange(result.value);
  };
  return (
    <div className="twk-row">
      <div className="twk-lbl"><span>{advancedState.label}</span>{error && <span>{error}</span>}</div>
      <textarea
        className="twk-field"
        style={{ height: 118, paddingTop: 7, fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace", resize: "vertical" }}
        value={draft}
        onChange={(e) => commit(e.target.value)}
      />
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root")).render(<App />);
