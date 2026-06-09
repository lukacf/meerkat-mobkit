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
  const projectionSyncInFlight = React.useRef(false);
  const projectionSyncReset = React.useRef(0);
  const beginProjectionSync = React.useCallback(() => {
    projectionSyncInFlight.current = true;
    if (projectionSyncReset.current) window.cancelAnimationFrame(projectionSyncReset.current);
    projectionSyncReset.current = window.requestAnimationFrame(() => {
      projectionSyncReset.current = window.requestAnimationFrame(() => {
        projectionSyncInFlight.current = false;
        projectionSyncReset.current = 0;
      });
    });
  }, []);
  const beginSourceProjection = React.useCallback(() => {
    sourceProjectionVersion.current += 1;
    return sourceProjectionVersion.current;
  }, []);
  const sourceProjectionIsCurrent = React.useCallback((requestToken) =>
    requestToken === sourceProjectionVersion.current, []);
  const currentAuthoringRevision = React.useCallback(() => authoringRevision.current, []);
  const authoringRevisionIsCurrent = React.useCallback((requestToken) =>
    requestToken === authoringRevision.current, []);
  const applySourceProjectionPatch = React.useCallback((patch) => {
    const next = patch && typeof patch === "object" ? patch : {};
    if (Object.prototype.hasOwnProperty.call(next, "sourceOpen")) setSourceOpen(next.sourceOpen);
    if (Object.prototype.hasOwnProperty.call(next, "sourceDocument")) setSourceDocument(next.sourceDocument);
    if (Object.prototype.hasOwnProperty.call(next, "inlineSourceOpen")) setInlineSourceOpen(next.inlineSourceOpen);
    if (Object.prototype.hasOwnProperty.call(next, "inlineSourceSurface")) setInlineSourceSurface(next.inlineSourceSurface);
    if (Object.prototype.hasOwnProperty.call(next, "inlineSourceDocument")) setInlineSourceDocument(next.inlineSourceDocument);
    if (Object.prototype.hasOwnProperty.call(next, "inlineSourceBusy")) setInlineSourceBusy(next.inlineSourceBusy);
  }, []);
  const applyApiOverlayPatch = React.useCallback((patch) => {
    const next = patch && typeof patch === "object" ? patch : {};
    if (Object.prototype.hasOwnProperty.call(next, "drySim")) setDrySim(next.drySim);
    if (Object.prototype.hasOwnProperty.call(next, "drySimDocument")) setDrySimDocument(next.drySimDocument);
    if (Object.prototype.hasOwnProperty.call(next, "drySimPlan")) setDrySimPlan(next.drySimPlan);
    if (next.incrementDrySimKey) setDrySimKey(k => k + 1);
    if (Object.prototype.hasOwnProperty.call(next, "validate")) setValidate(next.validate);
  }, []);
  const clearSourceProjection = React.useCallback(() => {
    sourceProjectionVersion.current += 1;
    applySourceProjectionPatch(window.MobKitFlowController.sourceProjectionClearTransition());
  }, [applySourceProjectionPatch]);
  const markDraft = React.useCallback(() => {
    if (projectionSyncInFlight.current) return;
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
    contract,
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
    window.MobKitFlowController.configure({ rpcUrl: rpcUrlFromShell() });
    window.MobKitFlowController.loadSchema()
      .then(async (schema) => {
        window.MobKitFlowController.configureAuthoringMethodsFromSchema(schema);
        const catalogPayload = await window.MobKitFlowController.loadCatalogs();
        const registryPayload = await window.MobKitFlowController.listDocuments().catch(() => ({ rows: [] }));
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
          registryResult: registryPayload,
        });
        setTemplates(bootstrap.templates);
        setFlows(bootstrap.flows);
        if (bootstrap.initialHydration) {
          hydrateMobpackDocument(bootstrap.initialHydration.result, {
            ...bootstrap.initialHydration.options,
            contract: schema,
          });
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
      graphProjectionSig.current = window.MobKitFlowController.graphStructureSignature(projection.instances || [], projection.edges || [], { members: projection.members || studio.members, contract: projection.contract || contract });
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
    if (!window.MobKitFlowController?.graphProjectionDocument) return;
    let cancelled = false;
    const projectionDocument = buildAuthoringProjection().document;
    window.MobKitFlowController.graphProjectionDocument({
      ...projectionDocument,
      instances: [],
      edges: [],
      frames: [],
    })
      .then((projectionResult) => {
        if (cancelled) return;
        const projection = window.MobKitFlowController.graphProjectionFromMobKitResult(projectionResult);
        if (!projection) return;
        graphProjectionSig.current = window.MobKitFlowController.graphStructureSignature(projection.instances || [], projection.edges || [], { members: studio.members, contract });
        studio.setInstances(projection.instances || []);
        studio.setEdges(projection.edges || []);
        studio.setFrames(projection.frames || []);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [flow, editorMode, contract, studio.members]);

  React.useEffect(() => {
    if (editorMode !== "advanced") return;
    if (!window.MobKitFlowController?.graphToFlow) return;
    const sig = window.MobKitFlowController.graphStructureSignature(studio.instances, studio.edges, { members: studio.members, contract });
    if (sig === graphProjectionSig.current) return;
    graphProjectionSig.current = sig;
    skipNextGraphProjection.current = true;
    const nextFlow = window.MobKitFlowController.graphToFlow({
      instances: studio.instances,
      edges: studio.edges,
      members: studio.members,
      previousFlow: flow,
      contract,
    });
    if (nextFlow === flow) return;
    applyMobKitAuthoringReplacement({
      operationType: "replace_authoring_document",
      operation: { reason: "project_graph_to_flow" },
      flow: nextFlow,
    });
  }, [editorMode, studio.instances, studio.edges, studio.members, flow, contract]);

  React.useEffect(() => {
    const previousMembers = previousMembersRef.current || [];
    if (hydratingDocumentRef.current) {
      previousMembersRef.current = studio.members;
      return;
    }
    const result = window.MobKitFlowController.reconcileAuthoringForMembers({
      flow,
      instances: studio.instances,
      edges: studio.edges,
      mobSettings,
      previousMembers,
      members: studio.members,
    });
    const changed = result.flow !== flow
      || result.edges !== studio.edges
      || result.instances !== studio.instances
      || result.mobSettings !== mobSettings;
    previousMembersRef.current = studio.members;
    if (!changed) return;
    applyMobKitAuthoringReplacement({
      operationType: "replace_authoring_document",
      operation: { reason: "reconcile_members" },
      flow: result.flow,
      mobSettings: result.mobSettings,
      studio: {
        instances: result.instances,
        edges: result.edges,
      },
    });
  }, [studio.members, flow, studio.instances, studio.edges, mobSettings]);

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
    applyMobKitAuthoringReplacement({
      operationType: "replace_authoring_document",
      operation: { reason: "reconcile_condition_fields" },
      flow: result.flow,
      studio: {
        edges: result.edges,
      },
    });
  }, [flow, studio.edges, studio.instances, studio.members, studio.schemas]);

  React.useEffect(() => {
    if (hydratingDocumentRef.current) return;
    const result = window.MobKitFlowController.reconcileAuthoringWithContract({
      members: studio.members,
      skillRealms: studio.skillRealms,
      schemas: studio.schemas,
      deploySettings,
      mobSettings,
      flow,
      instances: studio.instances,
      edges: studio.edges,
      contract,
      modelCatalog: catalogs.models,
      toolCatalog: catalogs.toolCatalog,
      contractLoaded: !!catalogs.contractMeta.loaded,
    });
    if (!result.changed) return;
    applyMobKitAuthoringReplacement({
      operationType: "replace_authoring_document",
      operation: { reason: "reconcile_contract_refs" },
      flow: result.flow,
      deploySettings: result.deploySettings,
      mobSettings: result.mobSettings,
      studio: {
        members: result.members,
        instances: result.instances,
        edges: result.edges,
      },
    });
  }, [
    studio.members,
    studio.skillRealms,
    studio.schemas,
    deploySettings,
    mobSettings,
    flow,
    studio.instances,
    studio.edges,
    contract,
    catalogs.models,
    catalogs.toolCatalog,
    catalogs.contractMeta.loaded,
  ]);

  const selectInstance = (id) => setSelection(window.MobKitFlowController.graphSelectionProjection("instance", id));
  const selectEdge = (id) => setSelection(window.MobKitFlowController.graphSelectionProjection("edge", id));
  const clearSelection = (nextSelection = { kind: null, id: null }) => setSelection(nextSelection || { kind: null, id: null });

  React.useEffect(() => {
    const onKey = (e) => {
      const tg = e.target;
      if (tg.tagName === "INPUT" || tg.tagName === "TEXTAREA" || tg.tagName === "SELECT") return;
      if (e.key === "Backspace" || e.key === "Delete") {
        if (selection.kind === "instance") {
          const result = window.MobKitFlowController.studioDeleteInstancePatch({
            instances: studio.instances,
            edges: studio.edges,
          }, selection.id);
          applyMobKitAuthoringReplacement({
            operationType: "delete_graph_node",
            operation: { instance_id: selection.id },
            studio: { instances: result.instances, edges: result.edges },
            selection: result.selection,
          }).then(() => clearSelection(result.selection));
        }
        else if (selection.kind === "edge") {
          const result = window.MobKitFlowController.studioDeleteEdgePatch({ edges: studio.edges }, selection.id);
          applyMobKitAuthoringReplacement({
            operationType: "delete_graph_edge",
            operation: { edge_id: selection.id },
            studio: { edges: result.edges },
            selection: result.selection,
          }).then(() => clearSelection(result.selection));
        }
      }
      if ((e.metaKey || e.ctrlKey) && e.key === "z") {
        e.preventDefault();
        const result = e.shiftKey ? studio.redo() : studio.undo();
        if (result?.state) {
          applyMobKitAuthoringReplacement({
            operationType: "replace_authoring_document",
            operation: { reason: e.shiftKey ? "redo" : "undo" },
            studio: result.state,
          });
        }
      }
      if (e.key === "Escape") {
        clearSelection(); closeGraphAddMenu();
        applyApiOverlayPatch(window.MobKitFlowController.apiOverlayClearTransition()); clearSourceProjection();
        if (creating) setCreating(null);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const handleRequestAdd = (col, row) => {
    const result = window.MobKitFlowController.graphAddMenuOpenProjection({ col, row, grid: catalogs.grid });
    setAddAt(result.addAt);
  };
  const closeGraphAddMenu = () => {
    const result = window.MobKitFlowController.graphAddMenuCloseProjection();
    setAddAt(result.addAt);
  };

  const handlePick = (pick) => {
    if (!addAt) return;
    const inserted = window.MobKitFlowController.graphQuickInsertProjection({
      pick,
      at: addAt,
      members: studio.members,
      instances: studio.instances,
      edges: studio.edges,
      flow,
      contract,
      graphView: catalogs.graphView,
    });
    if (inserted.ok) {
      applyMobKitAuthoringReplacement({
        operationType: "insert_graph_node",
        operation: {
          instances: inserted.instances,
          edges: inserted.edges,
          flow: inserted.flow,
        },
        flow: inserted.flow,
        studio: {
          instances: inserted.instances,
          edges: inserted.edges,
        },
        selection: inserted.selectId ? { kind: "instance", id: inserted.selectId } : null,
      }).then(() => {
        if (inserted.selectId) selectInstance(inserted.selectId);
      });
    }
    setAddAt(inserted.addAt);
  };

  const handleAgentNavigation = (id) => {
    const next = window.MobKitFlowController.agentNavigationProjection(id);
    setAddAt(next.addAt);
    setView(next.view);
    setAgentSel(next.selection);
  };
  const handleTopRailNavigation = (target) => {
    const next = window.MobKitFlowController.topRailNavigationTransition(view, target);
    if (!next) return;
    setView(next.view);
  };
  const handleEditorModeSelection = (target) => {
    const next = window.MobKitFlowController.editorModeTransition(target);
    if (!next) return;
    setEditorMode(next.editorMode);
  };
  const handleThemeToggle = () => {
    const next = window.MobKitFlowController.themeToggleTransition(t.theme);
    setTweak(next.field, next.value);
  };

  const applyAuthoringDocumentProjection = (projection) => {
    const plan = window.MobKitFlowController.authoringProjectionApplyPlan(projection, {
      flow,
      studio: {
        members: studio.members,
        instances: studio.instances,
        edges: studio.edges,
        frames: studio.frames,
        schemas: studio.schemas,
        skillRealms: studio.skillRealms,
      },
      deploySettings,
      mobSettings,
      contract,
    });
    if (!plan.ok) return;
    if (plan.flow.changed) setFlow(plan.flow.value);
    if (plan.members.changed) studio.setMembers(plan.members.value);
    if (plan.skillRealms.changed) studio.setSkillRealms(plan.skillRealms.value);
    if (plan.schemas.changed) studio.setSchemas(plan.schemas.value);
    if (plan.graph.changed) {
      graphProjectionSig.current = plan.graph.signature;
      studio.setInstances(plan.graph.instances);
      studio.setEdges(plan.graph.edges);
    }
    if (plan.frames.changed) studio.setFrames(plan.frames.value);
    if (plan.deploySettings.changed) setDeploySettings(plan.deploySettings.value);
    if (plan.mobSettings.changed) setMobSettings(plan.mobSettings.value);
  };
  const currentFlowSelection = window.MobKitFlowController.flowRegistrySelectionState(flows, currentFlowId);
  const currentFlow = currentFlowSelection.row;
  const buildAuthoringProjection = (overrides = {}) => {
    const nextStudio = {
      members: studio.members,
      schemas: studio.schemas,
      instances: studio.instances,
      edges: studio.edges,
      frames: studio.frames,
      skillRealms: studio.skillRealms,
      ...(overrides.studio || {}),
    };
    return window.MobKitFlowController.authoringDocumentFromState({
    editorMode,
    flow: overrides.flow || flow,
    studio: nextStudio,
    currentFlow,
    deploySettings: overrides.deploySettings || deploySettings,
    mobSettings: overrides.mobSettings || mobSettings,
    contract,
    modelCatalog: catalogs.models,
    toolCatalog: catalogs.toolCatalog,
    contractLoaded: !!catalogs.contractMeta.loaded,
    });
  };
  const buildDocument = () => {
    const projection = buildAuthoringProjection();
    beginProjectionSync();
    applyAuthoringDocumentProjection(projection);
    return projection.document;
  };
  const applyMobKitAuthoringOperation = async (operation) => {
    const availability = window.MobKitFlowController.authoringOperationAvailability(catalogs.authoringOperations, operation?.type);
    if (!availability.supported) return { ok: false, error: availability.error };
    const requestToken = currentAuthoringRevision();
    const document = buildDocument();
    const result = await window.MobKitFlowController.applyAuthoringOperationDocument(document, operation);
    if (!authoringRevisionIsCurrent(requestToken)) {
      return { ok: false, error: "stale authoring operation" };
    }
    const projection = window.MobKitFlowController.authoringProjectionFromOperationResult(result, {
      deployDefaults: catalogs.deployDefaults,
      mobDefaults: catalogs.mobDefaults,
    });
    if (!projection) return { ok: false, error: "MobKit authoring operation did not return a document" };
    beginProjectionSync();
    applyAuthoringDocumentProjection(projection);
    markDraft();
    return result;
  };
  const applyMobKitAuthoringReplacement = async (overrides = {}) => {
    const operationType = overrides.operationType || "replace_authoring_document";
    const availability = window.MobKitFlowController.authoringOperationAvailability(catalogs.authoringOperations, operationType);
    if (!availability.supported) return { ok: false, error: availability.error };
    const requestToken = currentAuthoringRevision();
    const document = buildDocument();
    const operation = {
      type: operationType,
      ...(overrides.operation || {}),
      selection: overrides.selection || null,
    };
    if (operationType === "replace_authoring_document") {
      operation.document = buildAuthoringProjection(overrides).document;
    }
    const result = await window.MobKitFlowController.applyAuthoringOperationDocument(document, {
      ...operation,
    });
    if (!authoringRevisionIsCurrent(requestToken)) {
      return { ok: false, error: "stale authoring operation" };
    }
    const projection = window.MobKitFlowController.authoringProjectionFromOperationResult(result, {
      deployDefaults: catalogs.deployDefaults,
      mobDefaults: catalogs.mobDefaults,
    });
    if (!projection) return { ok: false, error: "MobKit authoring operation did not return a document" };
    beginProjectionSync();
    applyAuthoringDocumentProjection(projection);
    markDraft();
    return result;
  };
  const mobKitStudio = {
    ...studio,
    addInstance: (instance) => {
      const next = window.MobKitFlowController.studioAddInstancePatch({
        instances: studio.instances,
        members: studio.members,
      }, instance);
      if (next.ok && next.instance) {
        applyMobKitAuthoringReplacement({
          operationType: "insert_graph_node",
          operation: { instance: next.instance },
          studio: { instances: next.instances },
          selection: { kind: "instance", id: next.instance.id },
        });
      }
      return next;
    },
    updateInstance: (id, patch) => {
      const next = window.MobKitFlowController.studioUpdateInstancePatch({
        instances: studio.instances,
        members: studio.members,
      }, id, patch);
      applyMobKitAuthoringReplacement({
        operationType: "update_graph_node",
        operation: { instance_id: id, patch },
        studio: { instances: next.instances },
        selection: { kind: "instance", id },
      });
      return next;
    },
    deleteInstance: (id) => {
      const next = window.MobKitFlowController.studioDeleteInstancePatch({
        instances: studio.instances,
        edges: studio.edges,
      }, id);
      applyMobKitAuthoringReplacement({
        operationType: "delete_graph_node",
        operation: { instance_id: id },
        studio: { instances: next.instances, edges: next.edges },
        selection: next.selection,
      });
      return next;
    },
    addEdge: (edge) => {
      const next = window.MobKitFlowController.studioAddEdgePatch({
        edges: studio.edges,
        instances: studio.instances,
      }, edge);
      if (next.ok && next.edge) {
        applyMobKitAuthoringReplacement({
          operationType: "connect_graph_nodes",
          operation: { edge: next.edge },
          studio: { edges: next.edges },
          selection: { kind: "edge", id: next.edge.id },
        });
      }
      return next;
    },
    updateEdge: (id, patch) => {
      const next = window.MobKitFlowController.studioUpdateEdgePatch({
        edges: studio.edges,
        instances: studio.instances,
      }, id, patch);
      applyMobKitAuthoringReplacement({
        operationType: "update_graph_edge",
        operation: { edge_id: id, patch },
        studio: { edges: next.edges },
        selection: { kind: "edge", id },
      });
      return next;
    },
    deleteEdge: (id) => {
      const next = window.MobKitFlowController.studioDeleteEdgePatch({ edges: studio.edges }, id);
      applyMobKitAuthoringReplacement({
        operationType: "delete_graph_edge",
        operation: { edge_id: id },
        studio: { edges: next.edges },
        selection: next.selection,
      });
      return next;
    },
    addSchema: (schema) => {
      const next = window.MobKitFlowController.studioAddSchemaPatch({ schemas: studio.schemas }, schema);
      if (next.ok && next.schema) {
        applyMobKitAuthoringReplacement({
          operationType: "add_schema",
          operation: { schema: next.schema },
          studio: { schemas: next.schemas },
          selection: { kind: "schema", id: next.schema.id },
        });
      }
      return next;
    },
    updateSchema: (id, patch) => {
      const next = window.MobKitFlowController.studioUpdateSchemaPatch({ schemas: studio.schemas }, id, patch);
      applyMobKitAuthoringReplacement({
        operationType: "update_schema",
        operation: { schema_id: id, patch },
        studio: { schemas: next.schemas },
        selection: { kind: "schema", id },
      });
      return next;
    },
    deleteSchema: (id) => {
      const next = window.MobKitFlowController.studioDeleteSchemaPatch({
        schemas: studio.schemas,
        members: studio.members,
        flow,
        edges: studio.edges,
        instances: studio.instances,
      }, id);
      applyMobKitAuthoringReplacement({
        operationType: "delete_schema",
        operation: { schema_id: id },
        flow: next.flow,
        studio: { schemas: next.schemas, members: next.members, edges: next.edges },
        selection: next.selection,
      });
      return next;
    },
  };
  const saveRegistryDocument = (rowPatch) => {
    if (!rowPatch?.document) return;
    window.MobKitFlowController.saveDocument(rowPatch).catch(() => {});
  };
  React.useEffect(() => {
    let cancelled = false;
    setDeployCommandPreview("");
    if (!deployContractLoaded) {
      return () => {
        cancelled = true;
      };
    }
    const projection = buildAuthoringProjection();
    window.MobKitFlowController.deployCommandPreviewForDocument(projection.document)
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
  }, [
    deployContractLoaded,
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
    contract,
    catalogs.models,
    catalogs.toolCatalog,
    catalogs.contractMeta.loaded,
  ]);
  const persistCurrentOutcome = (outcome) => {
    const projection = window.MobKitFlowController.flowRegistryPersistOutcomeProjection(flows, {
      currentFlowId,
      outcome,
    });
    if (!projection.ok || !projection.changed) return projection;
    persistedDocumentSig.current = projection.signature;
    setFlows(projection.rows);
    saveRegistryDocument(projection.persistence?.rowPatch);
    return projection;
  };

  React.useEffect(() => {
    if (!currentFlowId || !currentFlow) return;
    let document;
    try {
      document = buildDocument();
    } catch {
      return;
    }
    const persistence = window.MobKitFlowController.flowRegistryPersistDocumentProjection(flows, {
      currentFlowId,
      document,
      validation: null,
      stage,
      previousSignature: persistedDocumentSig.current,
      skipIfUnchanged: true,
    });
    if (!persistence.changed) return;
    persistedDocumentSig.current = persistence.signature;
    setFlows(persistence.rows);
    saveRegistryDocument(persistence.rowPatch);
  }, [
    flows,
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
    contract,
    catalogs.models,
    catalogs.toolCatalog,
    catalogs.contractMeta.loaded,
  ]);

  const handleDrySim = async () => {
    let requestToken = null;
    setApiBusy(true);
    try {
      const document = buildDocument();
      requestToken = currentAuthoringRevision();
      const plan = await window.MobKitFlowController.deployDocument(document, { execute: false });
      if (!authoringRevisionIsCurrent(requestToken)) return;
      const outcome = window.MobKitFlowController.deployOutcome(document, plan, { execute: false });
      window.__mobkitFlowLastDocument = document;
      window.__mobkitFlowLastDeployPlanTrace = plan;
      persistCurrentOutcome(outcome);
      setValidationResults(outcome.validationRows);
      setStage(outcome.stage);
      applyApiOverlayPatch(window.MobKitFlowController.deployPlanTraceReadyTransition(document, plan));
    } catch (error) {
      if (requestToken !== null && !authoringRevisionIsCurrent(requestToken)) return;
      const outcome = window.MobKitFlowController.deployErrorOutcome(error, { execute: false, errorView: catalogs.errorView });
      setValidationResults(outcome.validationRows);
      applyApiOverlayPatch(window.MobKitFlowController.validationSheetOpenTransition());
      setStage(outcome.stage);
    } finally {
      setApiBusy(false);
    }
  };

  const renderCurrentSourceDocument = async (requestToken, projectedDocument = null) => {
    const document = projectedDocument || buildDocument();
    const result = await window.MobKitFlowController.sourceDocument(document);
    const projection = window.MobKitFlowController.sourceDocumentFromSourceResult(document, result, {
      sourceView: catalogs.sourceView,
    });
    if (!sourceProjectionIsCurrent(requestToken)) return null;
    window.__mobkitFlowLastDocument = document;
    window.__mobkitFlowLastSource = result;
    setValidationResults(projection.validationRows);
    setStage(projection.stage);
    return projection.sourceDocument;
  };

  const handleSource = async () => {
    if (sourceOpen) {
      clearSourceProjection();
      return;
    }
    let requestToken = null;
    setApiBusy(true);
    try {
      const document = buildDocument();
      requestToken = beginSourceProjection();
      const nextSourceDocument = await renderCurrentSourceDocument(requestToken, document);
      if (!nextSourceDocument || !sourceProjectionIsCurrent(requestToken)) return;
      applySourceProjectionPatch(window.MobKitFlowController.sourceDrawerReadyTransition(nextSourceDocument));
    } catch (error) {
      if (requestToken !== null && !sourceProjectionIsCurrent(requestToken)) return;
      const outcome = window.MobKitFlowController.sourceErrorOutcome(error, { errorView: catalogs.errorView });
      setValidationResults(outcome.validationRows);
      applyApiOverlayPatch(window.MobKitFlowController.validationSheetOpenTransition());
      setStage(outcome.stage);
    } finally {
      setApiBusy(false);
    }
  };

  React.useEffect(() => {
    clearSourceProjection();
  }, [view, editorMode, clearSourceProjection]);

  const handleInlineSource = async (surface = "basic") => {
    let requestToken = null;
    applySourceProjectionPatch(window.MobKitFlowController.inlineSourcePendingTransition(surface));
    setApiBusy(true);
    try {
      const document = buildDocument();
      requestToken = beginSourceProjection();
      applySourceProjectionPatch(window.MobKitFlowController.inlineSourcePendingTransition(surface));
      const nextSourceDocument = await renderCurrentSourceDocument(requestToken, document);
      if (!nextSourceDocument || !sourceProjectionIsCurrent(requestToken)) return;
      applySourceProjectionPatch(window.MobKitFlowController.inlineSourceReadyTransition(nextSourceDocument));
    } catch (error) {
      if (requestToken !== null && !sourceProjectionIsCurrent(requestToken)) return;
      const outcome = window.MobKitFlowController.sourceErrorOutcome(error, { errorView: catalogs.errorView });
      setValidationResults(outcome.validationRows);
      applyApiOverlayPatch(window.MobKitFlowController.validationSheetOpenTransition());
      setStage(outcome.stage);
    } finally {
      applySourceProjectionPatch(window.MobKitFlowController.inlineSourceBusyTransition(false));
      setApiBusy(false);
    }
  };

  React.useEffect(() => {
    const openGraphSourceFromHash = () => {
      const canvasView = window.MobKitFlowController.graphCanvasViewState(catalogs.graphView);
      if (!canvasView.sourceFileActivationHash || window.location.hash !== canvasView.sourceFileActivationHash) return;
      window.history.replaceState(null, "", `${window.location.pathname}${window.location.search}`);
      handleInlineSource("graph");
    };
    window.addEventListener("hashchange", openGraphSourceFromHash);
    openGraphSourceFromHash();
    return () => window.removeEventListener("hashchange", openGraphSourceFromHash);
  }, [handleInlineSource, catalogs.graphView]);

  const handleValidate = async () => {
    let requestToken = null;
    setApiBusy(true);
    try {
      const document = buildDocument();
      requestToken = currentAuthoringRevision();
      const result = await window.MobKitFlowController.validateDocument(document);
      if (!authoringRevisionIsCurrent(requestToken)) return;
      const outcome = window.MobKitFlowController.validationOutcome(document, result);
      window.__mobkitFlowLastDocument = document;
      window.__mobkitFlowLastValidation = result;
      persistCurrentOutcome(outcome);
      setValidationResults(outcome.validationRows);
      setStage(outcome.stage);
    } catch (error) {
      if (requestToken !== null && !authoringRevisionIsCurrent(requestToken)) return;
      const outcome = window.MobKitFlowController.validationErrorOutcome(error, { errorView: catalogs.errorView });
      setValidationResults(outcome.validationRows);
      setStage(outcome.stage);
    } finally {
      if (requestToken === null || authoringRevisionIsCurrent(requestToken)) {
        applyApiOverlayPatch(window.MobKitFlowController.validationSheetOpenTransition());
      }
      setApiBusy(false);
    }
  };

  const handlePublish = async () => {
    let requestToken = null;
    setApiBusy(true);
    try {
      const document = buildDocument();
      requestToken = currentAuthoringRevision();
      const result = await window.MobKitFlowController.exportDocument(document);
      if (!authoringRevisionIsCurrent(requestToken)) return;
      const outcome = window.MobKitFlowController.exportOutcome(document, result);
      window.__mobkitFlowLastDocument = document;
      window.__mobkitFlowLastExport = result;
      persistCurrentOutcome(outcome);
      if (!window.__mobkitFlowDisableDownload) {
        downloadExportResult(result);
      }
      setValidationResults(outcome.validationRows);
      setStage(outcome.stage);
      applyApiOverlayPatch(window.MobKitFlowController.validationSheetCloseTransition());
    } catch (error) {
      if (requestToken !== null && !authoringRevisionIsCurrent(requestToken)) return;
      const outcome = window.MobKitFlowController.exportErrorOutcome(error, { errorView: catalogs.errorView });
      setValidationResults(outcome.validationRows);
      applyApiOverlayPatch(window.MobKitFlowController.validationSheetOpenTransition());
      setStage(outcome.stage);
    } finally {
      setApiBusy(false);
    }
  };

  const handleDeploy = async ({ execute }) => {
    let requestToken = null;
    setApiBusy(true);
    try {
      const document = buildDocument();
      requestToken = currentAuthoringRevision();
      const result = await window.MobKitFlowController.deployDocument(document, { execute });
      if (!authoringRevisionIsCurrent(requestToken)) return;
      const outcome = window.MobKitFlowController.deployOutcome(document, result, { execute });
      window.__mobkitFlowLastDocument = document;
      window.__mobkitFlowLastDeploy = result;
      persistCurrentOutcome(outcome);
      setValidationResults(outcome.validationRows);
      setStage(outcome.stage);
      applyApiOverlayPatch(window.MobKitFlowController.validationSheetOpenTransition());
    } catch (error) {
      if (requestToken !== null && !authoringRevisionIsCurrent(requestToken)) return;
      const outcome = window.MobKitFlowController.deployErrorOutcome(error, { execute, errorView: catalogs.errorView });
      setValidationResults(outcome.validationRows);
      applyApiOverlayPatch(window.MobKitFlowController.validationSheetOpenTransition());
      setStage(outcome.stage);
    } finally {
      setApiBusy(false);
    }
  };
  const handleDeployPlan = () => handleDeploy({ execute: false });
  const handleDeployRun = () => handleDeploy({ execute: true });

  const hydrateMobpackDocument = (result, options = {}) => {
    const activeContract = options.contract || contract;
    const hydration = window.MobKitFlowController.hydrateMobpackDocumentState(result, {
      id: options.id,
      existingRows: options.existingRows,
      addToRegistry: options.addToRegistry,
      openEditor: options.openEditor,
      flowRow: options.flowRow || null,
      deployDefaults: options.deployDefaults || catalogs.deployDefaults,
      mobDefaults: options.mobDefaults || catalogs.mobDefaults,
      contractSkillRealms: contractSkillRealms.current,
      contract: activeContract,
      errorView: catalogs.errorView,
    });
    if (hydration.ok === false) {
      setValidationResults(hydration.validationRows || []);
      setStage(hydration.stage || "draft");
      applyApiOverlayPatch(window.MobKitFlowController.validationSheetOpenTransition());
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
    const graphProjectionToken = currentAuthoringRevision();
    window.MobKitFlowController.graphProjectionDocument(hydration.document)
      .then((projectionResult) => {
        if (!authoringRevisionIsCurrent(graphProjectionToken)) return;
        const projection = window.MobKitFlowController.graphProjectionFromMobKitResult(projectionResult);
        if (!projection) return;
        hydratingDocumentRef.current = true;
        graphProjectionSig.current = window.MobKitFlowController.graphStructureSignature(projection.instances || [], projection.edges || [], {
          members: hydration.members,
          contract: activeContract,
        });
        studio.setInstances(projection.instances || []);
        studio.setEdges(projection.edges || []);
        studio.setFrames(projection.frames || []);
        queueMicrotask(() => {
          hydratingDocumentRef.current = false;
        });
      })
      .catch(() => {});
  };

  const hydrateImportedDocument = (result) => {
    hydrateMobpackDocument(result, { existingRows: flows });
  };

  const openFlowRegistrySelection = (selection) => {
    if (selection?.hydration) {
      hydrateMobpackDocument(selection.hydration.result, selection.hydration.options);
      return true;
    }
    return false;
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
      applyApiOverlayPatch(window.MobKitFlowController.validationSheetOpenTransition());
      setStage(outcome.stage);
    } finally {
      setApiBusy(false);
    }
  };

  const shellState = window.MobKitFlowController.topRailState({ contract, deploySettings, stage, view, theme: t.theme, deployView: catalogs.deployView });

  return (
    <div className={"app density--" + t.density + " inspector--" + t.inspectorLayout + " view--" + view}>
      <TopRail
        stage={stage}
        view={view}
        onNavigate={handleTopRailNavigation}
        currentFlowName={currentFlow?.name || "—"}
        contract={contract}
        theme={t.theme}
        railState={shellState}
        onToggleTheme={handleThemeToggle}
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
            openFlowRegistrySelection(selection);
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
        <ModeToggle mode={editorMode} onSelectMode={handleEditorModeSelection} railState={shellState} />
      )}

      {view === "editor" && editorMode === "advanced" && (
        <div className="stage-area" onClick={(e) => { if (e.target === e.currentTarget) closeGraphAddMenu(); }}>
          <GraphEditor
            state={mobKitStudio}
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
            toolCatalog={catalogs.toolCatalog}
            applyAuthoringReplacement={applyMobKitAuthoringReplacement}
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
            onClose={closeGraphAddMenu}
            onJumpToAgents={handleAgentNavigation}
          />
          <aside className="inspector">
            <Inspector
              studio={mobKitStudio}
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
              selectMember={handleAgentNavigation}
              selectInstance={selectInstance}
              clearSelection={clearSelection}
            />
          </aside>
        </div>
      )}

      {view === "editor" && editorMode === "basic" && (
        <BuilderView
          studio={mobKitStudio}
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
          applyAuthoringReplacement={applyMobKitAuthoringReplacement}
        />
      )}

      {view === "agents" && (
        <AgentsView
          studio={mobKitStudio}
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
          applyAuthoringOperation={applyMobKitAuthoringOperation}
          applyAuthoringReplacement={applyMobKitAuthoringReplacement}
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
          onCreate={async (spec) => {
            if (!canCreateAuthoring) return;
            setApiBusy(true);
            try {
              const result = await window.MobKitFlowController.createDocument(spec);
              const row = result?.row;
              if (!row?.document) return;
              setFlows(Array.isArray(result?.rows) ? result.rows : window.MobKitFlowController.flowRegistryUpsertRowPatch(flows, row));
              hydrateMobpackDocument(
                { document: row.document, validation: row.validation || null },
                {
                  id: row.id,
                  flowRow: row,
                  addToRegistry: false,
                  openEditor: true,
                },
              );
              setCreating(null);
            } catch (error) {
              const outcome = window.MobKitFlowController.importErrorOutcome(error, { filename: "mobkit/mobpacks/create", errorView: catalogs.errorView });
              setValidationResults(outcome.validationRows);
              applyApiOverlayPatch(window.MobKitFlowController.validationSheetOpenTransition());
              setStage(outcome.stage);
            } finally {
              setApiBusy(false);
            }
          }}
        />
      )}

      <DrySim open={drySim} onClose={() => applyApiOverlayPatch(window.MobKitFlowController.deployPlanTraceCloseTransition())} onActiveStep={setActiveStepId} runKey={drySimKey} document={drySimDocument} plan={drySimPlan} deployView={catalogs.deployView} />
      <ValidateSheet open={validate} onClose={() => applyApiOverlayPatch(window.MobKitFlowController.validationSheetCloseTransition())} onPublish={handlePublish} onDeployPlan={handleDeployPlan} onDeployRun={handleDeployRun} results={validationResults} stage={stage} deployView={catalogs.deployView} />
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
        applyAuthoringReplacement={applyMobKitAuthoringReplacement}
        onLoadFlow={(id) => {
          const selection = window.MobKitFlowController.flowRegistrySelectionState(flows, id);
          openFlowRegistrySelection(selection);
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
  const download = window.MobKitFlowController.exportDownloadPayload(result);
  const bytes = Uint8Array.from(atob(download.contentBase64), (char) => char.charCodeAt(0));
  const blob = new Blob([bytes], { type: download.mediaType });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = download.filename;
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

function TopRail({ stage, view, onNavigate, currentFlowName, theme, railState, onToggleTheme, onValidate, onPublish, onDeployPlan, onDeployRun, onImport, onDrySim, onYaml, contract, deploySettings }) {
  return (
    <header className="toprail">
      <div className="brand">
        <span className="dot" />
        <span>{railState.brandLabel}</span>
      </div>
      <nav className="viewtabs">
        <button className={"viewtab" + (view === "flows" || view === "editor" ? " is-current" : "")} onClick={() => onNavigate("flows-tab")}>{railState.flowsTabLabel}</button>
        <button className={"viewtab" + (view === "agents" ? " is-current" : "")} onClick={() => onNavigate("agents-tab")}>{railState.agentsTabLabel}</button>
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
            <button className="crumb crumb--link" onClick={() => onNavigate("flows-crumb")}>{railState.flowsCrumbLabel}</button>
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
  const setField = (field, value) => setState((current) => window.MobKitFlowController.newFlowModalFieldPatch(current, field, value));
  const setStep = (step) => setState((current) => window.MobKitFlowController.newFlowModalStepPatch(current, step));
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
              <input className="field__input" autoFocus placeholder={modalState.namePlaceholder} value={modalState.name} onChange={e => setField("name", e.target.value)} />
            </div>
            <div className="field">
              <label className="field__label">{modalState.triggerLabel}</label>
              <input className="field__input" placeholder={modalState.triggerPlaceholder} value={modalState.trigger} onChange={e => setField("trigger", e.target.value)} />
            </div>
          </div>
        )}
        {modalState.step === 2 && (
          <div className="modal__body">
            <div className="field__label">{modalState.startFromLabel}</div>
            <div className="template-grid">
              {modalState.options.map(opt => (
                <button key={opt.id} className={opt.className} disabled={opt.disabled} onClick={() => setField("template", opt.id)}>
                  <div className="template-card__tier">{opt.tier}</div>
                  <div className="template-card__name">{opt.label}</div>
                  <div className="template-card__sub">{opt.sub}</div>
                </button>
              ))}
            </div>
          </div>
        )}
        <div className="modal__foot">
          {modalState.step > 1 ? <button className="btn btn--ghost btn--sm" onClick={() => setStep(modalState.step - 1)}>{modalState.backLabel}</button> : <span />}
          {modalState.step < 2 ? (
            <button className="btn btn--primary btn--sm" disabled={modalState.nextDisabled} onClick={() => setStep(2)}>{modalState.nextLabel}</button>
          ) : (
            <button
              className="btn btn--primary btn--sm"
              disabled={modalState.createDisabled}
              onClick={() => onCreate(window.MobKitFlowController.newFlowModalCreateSpec(modalState))}
            >{modalState.createLabel}</button>
          )}
        </div>
      </div>
    </div>
  );
}

function ModeToggle({ mode, onSelectMode, railState }) {
  return (
    <div className="modetoggle">
      <button className={"modetoggle__opt" + (mode === "basic" ? " is-active" : "")} onClick={() => onSelectMode("basic")} title={railState.basicModeTitle}>
        <svg width="13" height="13" viewBox="0 0 13 13" fill="none" stroke="currentColor" strokeWidth="1.3"><rect x="1.5" y="2.2" width="10" height="2.2"/><rect x="1.5" y="6.6" width="10" height="2.2"/></svg>
        <span>{railState.basicModeLabel}</span>
      </button>
      <button className={"modetoggle__opt" + (mode === "advanced" ? " is-active" : "")} onClick={() => onSelectMode("advanced")} title={railState.graphModeTitle}>
        <svg width="13" height="13" viewBox="0 0 13 13" fill="none" stroke="currentColor" strokeWidth="1.3"><rect x="1" y="4.5" width="4" height="4"/><rect x="8" y="1" width="4" height="4"/><rect x="8" y="8" width="4" height="4"/><path d="M5 6.5h1.6M6.6 6.5V3h1.4M6.6 6.5V10h1.4"/></svg>
        <span>{railState.graphModeLabel}</span>
      </button>
    </div>
  );
}

function Tweaks({ t, setTweak, flows = [], currentFlowId, deploySettings, setDeploySettings, mobSettings, setMobSettings, members = [], modelCatalog = [], contract, deployCommandPreview, settingsView = null, applyAuthoringReplacement = null, onLoadFlow }) {
  const setDeployField = (field, value) => {
    const next = window.MobKitFlowController.deploySettingsFieldPatch(deploySettings, field, value, { contract, modelCatalog });
    if (applyAuthoringReplacement) {
      applyAuthoringReplacement({
        operationType: "update_deploy_settings",
        operation: { deploy: next },
        deploySettings: next,
      });
    } else {
      setDeploySettings(next);
    }
  };
  const setMobField = (field, value) => {
    const next = window.MobKitFlowController.mobSettingsFieldPatch(mobSettings, field, value, { contract });
    if (applyAuthoringReplacement) {
      applyAuthoringReplacement({
        operationType: field === "roleWiring" ? "update_role_wiring" : "update_mob_settings",
        operation: field === "roleWiring" ? { role_wiring: next.roleWiring || [] } : { mob_settings: next },
        mobSettings: next,
      });
    } else {
      setMobSettings(next);
    }
  };
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
    <TweaksPanel title={controlState.panelTitle} closeLabel={controlState.panelCloseLabel}>
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
  const updateSource = (index, value) => {
    onChange(window.MobKitFlowController.mobRoleWiringSourcePatch(wiringState.wiring, index, value, wiringState.options));
  };
  const updateTarget = (index, value) => {
    onChange(window.MobKitFlowController.mobRoleWiringTargetPatch(wiringState.wiring, index, value, wiringState.options));
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
            <select className="twk-field" value={rule.a} onChange={e => updateSource(index, e.target.value)}>
              {wiringState.options.map(option => <option key={option.value} value={option.value}>{option.label}</option>)}
            </select>
            <select className="twk-field" value={rule.b} onChange={e => updateTarget(index, e.target.value)}>
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
