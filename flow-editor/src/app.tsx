// MobKit Flow Editor shell (S23 end-state): a real module bundled by
// esbuild from this single entry. Views come from @flow-editor-components
// and the controller plane from @flow-editor-core; React/ReactDOM still
// resolve from the window globals react-globals.js provides (ambient
// declarations in globals.d.ts). data.js must execute before this module
// body so window.MOBKIT_BOOT exists — the import keeps that ordering.
import "./data.js";
import { createMobKitFlowController } from "@flow-editor-core";
import {
  AddNodeMenu,
  AgentsView,
  BuilderView,
  DeployPlanTrace,
  GraphEditor,
  InlineSourceEditor,
  Inspector,
  SourceDrawer,
  TweakNumber,
  TweakRadio,
  TweakSection,
  TweakSelect,
  TweakText,
  useStudioState,
  useTweaks,
  ValidateSheet,
} from "@flow-editor-components";

// The controller facade is constructed exactly once, at module scope; the
// shell uses this module-scoped reference directly. The window assignment
// stays as a deliberate back-compat surface: the browser smokes, the live
// verification scripts, the @flow-editor-components views (which call the
// facade through window at render time), and embedders all still reach the
// controller through window.MobKitFlowController.
//
// The `: any` is migration-window typing: the views consume the facade
// stringly through the window contract, and the shell keeps the same
// looseness until the strictness ratchet (key-set parity is enforced by
// controller-export-keys.test.cjs instead).
const MobKitFlowController: any = createMobKitFlowController({ includeTestExports: false });
window.MobKitFlowController = MobKitFlowController;

// Deep-link boot intent, parsed exactly once from the query string. The
// embedding contract (?open=<id>, ?intent=new[&template=<id>], ?embedded=1)
// is documented in docs/guides/flow-editor.mdx.
const BOOT_INTENT = MobKitFlowController.bootIntentFromQuery(window.location.search);

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
  const [flow, setFlow] = React.useState(() => MobKitFlowController.emptyAuthoringFlowState());
  const [authoringDocument, setAuthoringDocument] = React.useState(null);
  const [stepSel, setStepSel] = React.useState(null);
  // Editor sub-mode: "basic" (vertical builder) | "advanced" (grid graph).
  const [editorMode, setEditorMode] = React.useState("basic");

  // view: "library" (home: the mob registry) | "editor" (FLOW section) |
  // "agents" | "settings". The three section views are tabs of the open
  // mob; the library is where the app starts and needs no tabs.
  const [view, setView] = React.useState("library");
  const [flows, setFlows] = React.useState([]);
  const [currentFlowId, setCurrentFlowId] = React.useState("");
  const [templates, setTemplates] = React.useState([]);
  const [creating, setCreating] = React.useState(null); // { name, template } | null

  const [agentSel, setAgentSel] = React.useState(null);
  const [selection, setSelection] = React.useState({ kind: null, id: null });
  const [activeStepId, setActiveStepId] = React.useState(null);
  const [deployPlanOpen, setDeployPlanOpen] = React.useState(false);
  const [deployPlanKey, setDeployPlanKey] = React.useState(0);
  const [deployPlanDocument, setDeployPlanDocument] = React.useState(null);
  const [deployPlanResult, setDeployPlanResult] = React.useState(null);
  const [validate, setValidate] = React.useState(false);
  const [validationResults, setValidationResults] = React.useState([]);
  const [apiBusy, setApiBusy] = React.useState(false);
  const [contract, setContract] = React.useState(null);
  const [capabilities, setCapabilities] = React.useState(null);
  const contractSkillRealms = React.useRef([]);
  const [catalogs, setCatalogs] = React.useState(() => MobKitFlowController.emptyMobKitCatalogs(CATALOG_BOOT));
  const [sourceOpen, setSourceOpen] = React.useState(false);
  const [sourceDocument, setSourceDocument] = React.useState(null);
  const [inlineSourceOpen, setInlineSourceOpen] = React.useState(false);
  const [inlineSourceSurface, setInlineSourceSurface] = React.useState(null);
  const [inlineSourceDocument, setInlineSourceDocument] = React.useState(null);
  const [inlineSourceBusy, setInlineSourceBusy] = React.useState(false);
  const authoringRevision = React.useRef(0);
  const authoringDocumentRef = React.useRef(null);
  const sourceProjectionVersion = React.useRef(0);
  const [addAt, setAddAt] = React.useState(null);
  const [deploySettings, setDeploySettings] = React.useState(() => MobKitFlowController.deployDefaultsFromSchema(null));
  const [deployCommandPreview, setDeployCommandPreview] = React.useState("");
  const [mobSettings, setMobSettings] = React.useState(() => MobKitFlowController.mobDefaultsFromSchema(null));
  const authoringRunnerContext = React.useRef({});
  const authoringOperationRunner = React.useRef(null);
  const projectionSyncInFlight = React.useRef(false);
  const projectionSyncReset = React.useRef(0);
  // System reconciles must not retry without bound: a client/server
  // disagreement (failed validation, or a change the server will not make)
  // would otherwise turn into an unbounded RPC retry loop. Each reconcile
  // intent gets a few attempts per edit epoch; the latch releases on the
  // next real edit. The epoch is bumped by every non-system authoring
  // operation and by document hydration — authoringRevision cannot serve
  // here, because the runner's markDraft call is suppressed during
  // projection sync, so op-based edits never bump it.
  const RECONCILE_MAX_ATTEMPTS_PER_EPOCH = 4;
  const reconcileEpoch = React.useRef(0);
  const reconcileFailureEpoch = React.useRef(null);
  const reconcileAttempts = React.useRef({ epoch: null, counts: {} });
  const reconcileLatched = () =>
    reconcileFailureEpoch.current !== null
    && reconcileFailureEpoch.current === reconcileEpoch.current;
  const reconcileShouldRun = (intent) => {
    if (reconcileLatched()) return false;
    const attempts = reconcileAttempts.current;
    if (attempts.epoch !== reconcileEpoch.current) {
      attempts.epoch = reconcileEpoch.current;
      attempts.counts = {};
    }
    attempts.counts[intent] = (attempts.counts[intent] || 0) + 1;
    if (attempts.counts[intent] > RECONCILE_MAX_ATTEMPTS_PER_EPOCH) {
      reconcileFailureEpoch.current = reconcileEpoch.current;
      console.warn(`MobKit ${intent} did not converge; reconciliation paused until the next edit`);
      return false;
    }
    return true;
  };
  // A converged reconcile (nothing left to change) clears its attempt count,
  // so the cap only trips on consecutive non-converging attempts.
  const markReconcileConverged = (intent) => {
    const attempts = reconcileAttempts.current;
    if (attempts.epoch === reconcileEpoch.current) {
      attempts.counts[intent] = 0;
    }
  };
  // Latch only genuine server-validated failures (the result carries a
  // document) at the epoch the reconcile was issued: synthetic stale
  // results from a mid-flight hydration must not poison the new document.
  const latchReconcileFailure = (epochAtIssue) => (result) => {
    if (result?.ok === false && result?.document && reconcileEpoch.current === epochAtIssue) {
      reconcileFailureEpoch.current = epochAtIssue;
      console.warn("MobKit system reconcile paused until the next edit:", result?.error || result?.validation?.display_rows?.[0]?.sub || "validation failed");
    }
    return result;
  };
  if (!authoringOperationRunner.current) {
    authoringOperationRunner.current = MobKitFlowController.createAuthoringOperationRunner({
      getAuthoringOperations: () => authoringRunnerContext.current.catalogs?.authoringOperations,
      getCurrentDocument: () => authoringRunnerContext.current.currentMobKitDocument(),
      getDraftGuard: () => authoringRunnerContext.current.currentDraftGuard(),
      getCatalogSnapshot: () => authoringRunnerContext.current.catalogs?.catalogSnapshot,
      getCurrentRevision: () => authoringRunnerContext.current.currentAuthoringRevision(),
      isRevisionCurrent: (requestToken) => authoringRunnerContext.current.authoringRevisionIsCurrent(requestToken),
      getProjectionDefaults: () => ({
        deployDefaults: authoringRunnerContext.current.catalogs?.deployDefaults,
        mobDefaults: authoringRunnerContext.current.catalogs?.mobDefaults,
      }),
      getStaleError: () => authoringRunnerContext.current.catalogs?.errorView?.authoringOperationStaleError,
      getMissingDocumentError: () => authoringRunnerContext.current.catalogs?.errorView?.authoringOperationMissingDocumentError,
      beginProjectionSync: () => authoringRunnerContext.current.beginProjectionSync(),
      applyProjection: (projection) => authoringRunnerContext.current.applyAuthoringDocumentProjection(projection),
      markDraft: () => authoringRunnerContext.current.markDraft(),
    });
  }
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
  const setCurrentAuthoringDocument = React.useCallback((document) => {
    const next = document && typeof document === "object" ? document : null;
    authoringDocumentRef.current = next;
    setAuthoringDocument(next);
  }, []);
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
    if (Object.prototype.hasOwnProperty.call(next, "deployPlanOpen")) setDeployPlanOpen(next.deployPlanOpen);
    if (Object.prototype.hasOwnProperty.call(next, "deployPlanDocument")) setDeployPlanDocument(next.deployPlanDocument);
    if (Object.prototype.hasOwnProperty.call(next, "deployPlanResult")) setDeployPlanResult(next.deployPlanResult);
    if (next.incrementDeployPlanKey) setDeployPlanKey(k => k + 1);
    if (Object.prototype.hasOwnProperty.call(next, "validate")) setValidate(next.validate);
  }, []);
  const clearSourceProjection = React.useCallback(() => {
    sourceProjectionVersion.current += 1;
    applySourceProjectionPatch(MobKitFlowController.sourceProjectionClearTransition());
  }, [applySourceProjectionPatch]);
  const markDraft = React.useCallback(() => {
    if (projectionSyncInFlight.current) return;
    authoringRevision.current += 1;
    setStage("draft");
    setValidationResults([]);
    clearSourceProjection();
    if (currentFlowId) {
      setFlows((rows) => MobKitFlowController.flowRegistryMarkDraftPatch(rows, currentFlowId));
    }
  }, [clearSourceProjection, currentFlowId]);
  const beginDocumentHydration = React.useCallback(() => {
    authoringRevision.current += 1;
    reconcileEpoch.current += 1;
    clearSourceProjection();
  }, [clearSourceProjection]);
  const showAuthoringFailure = React.useCallback((resultOrError, fallbackHead = "") => {
    const errorView = catalogs.errorView || {};
    const authoringHead = fallbackHead || errorView.authoringOperationFailedHead;
    const validation = resultOrError?.validation || null;
    const validationRows = validation
      ? MobKitFlowController.diagnosticsToRows(validation)
      : null;
    const outcome = validationRows?.length
      ? { validationRows, stage: "draft" }
      : MobKitFlowController.criticalErrorOutcome({
          head: authoringHead,
          error: resultOrError?.error || resultOrError,
          meta: errorView.authoringOperationMeta,
          errorView,
        });
    setValidationResults(outcome.validationRows);
    setStage(outcome.stage);
    applyApiOverlayPatch(MobKitFlowController.validationSheetOpenTransition());
    return outcome;
  }, [applyApiOverlayPatch, catalogs.errorView]);
  const authoringFailureHead = React.useCallback((key) =>
    catalogs.errorView.authoringOperationFallbackHeads?.[key] || catalogs.errorView.authoringOperationFailedHead,
  [catalogs.errorView]);
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
  // Always-current view of `flows`, reassigned every render. Operations that
  // run AFTER a long await (deploy execute:true blocks for the whole
  // `rkat mob run`) must project against the freshest rows, not the row
  // snapshot captured in their click-time closure — otherwise a registry row
  // updated mid-await (e.g. an autosave response carrying the new draft
  // revision) is clobbered back to the stale snapshot, which then re-arms the
  // save-conflict loop single-tab.
  const flowsRef = React.useRef(flows);
  flowsRef.current = flows;

  const [t, setTweak] = useTweaks(TWEAK_DEFAULTS);
  const canCreateAuthoring = !!catalogs.contractMeta.loaded && !contract?.error;
  const deployContractLoaded = !!catalogs.contractMeta.loaded;
  const currentFlowSelection = MobKitFlowController.flowRegistrySelectionState(flows, currentFlowId);
  const currentFlow = currentFlowSelection.row;

  React.useEffect(() => {
    document.documentElement.dataset.ccVariant = "rams";
    document.documentElement.dataset.ccTheme = t.theme || "light";
  }, [t.theme]);

  // Embedded hosts (?embedded=1) own the chrome: the body flag hides the
  // brand block and the theme toggle (see styles.css).
  React.useEffect(() => {
    document.body.classList.toggle("is-embedded", BOOT_INTENT.embedded);
  }, []);

  React.useEffect(() => {
    let cancelled = false;
    const abort = new AbortController();
    MobKitFlowController.configure({ rpcUrl: rpcUrlFromShell() });
    const rpcOptions = { signal: abort.signal };
    MobKitFlowController.loadSchema(rpcOptions)
      .then(async (schema) => {
        MobKitFlowController.configureAuthoringMethodsFromSchema(schema);
        const capabilityPayload = await MobKitFlowController.loadCapabilities(rpcOptions);
        const catalogPayload = await MobKitFlowController.loadCatalogs(rpcOptions);
        // The library is home: list saved drafts for the registry rows and
        // land there. Nothing is auto-created and nothing hydrates until a
        // row is opened or a mob is created.
        const registryPayload = await MobKitFlowController.listDocuments({}, rpcOptions).catch((error) => {
          if (abort.signal.aborted) throw error;
          return { rows: [] };
        });
        if (cancelled) return;
        setCapabilities(capabilityPayload);
        const nextCatalogs = MobKitFlowController.mobKitCatalogsFromSchema(schema, CATALOG_BOOT, catalogPayload);
        setCatalogs(nextCatalogs);
        setDeploySettings(nextCatalogs.deployDefaults);
        setMobSettings(nextCatalogs.mobDefaults);
        contractSkillRealms.current = nextCatalogs.skillRealms;
        studio.setSkillRealms(nextCatalogs.skillRealms);
        const bootstrap = MobKitFlowController.flowCatalogBootstrapState(catalogPayload, {
          registryResult: registryPayload,
        });
        setTemplates(bootstrap.templates);
        setFlows(bootstrap.flows);
        setContract(schema);
      })
      .catch((error) => {
        if (abort.signal.aborted) return;
        if (!cancelled) setContract({ error: error?.message || String(error) });
      });
    return () => {
      cancelled = true;
      abort.abort();
    };
  }, []);

  // Keep the Flow grid in sync with the shared step-tree, so a mob loaded /
  // edited in Build shows in Flow and vice-versa (one source of truth).
  React.useEffect(() => {
    if (pendingGraphProjection.current) {
      const projection = pendingGraphProjection.current;
      pendingGraphProjection.current = null;
      skipNextGraphProjection.current = false;
      graphProjectionSig.current = MobKitFlowController.graphStructureSignature(projection.instances || [], projection.edges || [], { members: projection.members || studio.members, contract: projection.contract || contract });
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
    if (!MobKitFlowController?.graphProjectionDocument) return;
    let cancelled = false;
    const abort = new AbortController();
    if (!authoringDocumentRef.current) return;
    const projectionDocument = currentMobKitDocument();
    MobKitFlowController.graphProjectionDocument({
      ...projectionDocument,
      instances: [],
      edges: [],
      frames: [],
    }, { ...MobKitFlowController.flowRegistryDraftGuard(currentFlow, currentFlowId), signal: abort.signal })
      .then((projectionResult) => {
        if (cancelled) return;
        const projection = MobKitFlowController.graphProjectionFromMobKitResult(projectionResult);
        if (!projection) return;
        graphProjectionSig.current = MobKitFlowController.graphStructureSignature(projection.instances || [], projection.edges || [], { members: studio.members, contract });
        studio.setInstances(projection.instances || []);
        studio.setEdges(projection.edges || []);
        studio.setFrames(projection.frames || []);
      })
      .catch((error) => {
        if (abort.signal.aborted) return;
        if (cancelled) return;
        showAuthoringFailure(error, authoringFailureHead("graph_projection"));
      });
    return () => {
      cancelled = true;
      abort.abort();
    };
  }, [flow, editorMode, contract, studio.members, currentFlow, currentFlowId]);

  React.useEffect(() => {
    if (editorMode !== "advanced") return;
    const sig = MobKitFlowController.graphStructureSignature(studio.instances, studio.edges, { members: studio.members, contract });
    if (sig === graphProjectionSig.current) return;
    graphProjectionSig.current = sig;
    skipNextGraphProjection.current = true;
    applyMobKitAuthoringOperation({
      intent: "system.syncGraphToFlow",
      reason: "advanced_graph_changed",
    }).then((result) => {
      if (result?.ok === false) showAuthoringFailure(result, authoringFailureHead("graph_sync"));
    }).catch((error) => showAuthoringFailure(error, authoringFailureHead("graph_sync")));
  }, [editorMode, studio.instances, studio.edges, studio.members, flow, contract]);

  React.useEffect(() => {
    const previousMembers = previousMembersRef.current || [];
    if (hydratingDocumentRef.current) {
      previousMembersRef.current = studio.members;
      return;
    }
    const result = MobKitFlowController.reconcileAuthoringForMembers({
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
    if (!changed) {
      markReconcileConverged("system.reconcileMembers");
      return;
    }
    if (!reconcileShouldRun("system.reconcileMembers")) return;
    applyMobKitAuthoringOperation({
      intent: "system.reconcileMembers",
    }).then(latchReconcileFailure(reconcileEpoch.current));
  }, [studio.members, flow, studio.instances, studio.edges, mobSettings]);

  React.useEffect(() => {
    if (!MobKitFlowController?.reconcileConditionFieldAvailability) return;
    if (hydratingDocumentRef.current) return;
    const result = MobKitFlowController.reconcileConditionFieldAvailability({
      flow,
      edges: studio.edges,
      members: studio.members,
      instances: studio.instances,
      schemas: studio.schemas,
    });
    const flowChanged = result.flow !== flow;
    const edgesChanged = result.edges !== studio.edges;
    if (!flowChanged && !edgesChanged) {
      markReconcileConverged("system.reconcileConditionFields");
      return;
    }
    if (!reconcileShouldRun("system.reconcileConditionFields")) return;
    applyMobKitAuthoringOperation({
      intent: "system.reconcileConditionFields",
    }).then(latchReconcileFailure(reconcileEpoch.current));
  }, [flow, studio.edges, studio.instances, studio.members, studio.schemas]);

  React.useEffect(() => {
    if (hydratingDocumentRef.current) return;
    const result = MobKitFlowController.reconcileAuthoringWithContract({
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
    if (!result.changed) {
      markReconcileConverged("system.reconcileContractRefs");
      return;
    }
    if (!reconcileShouldRun("system.reconcileContractRefs")) return;
    applyMobKitAuthoringOperation({
      intent: "system.reconcileContractRefs",
    }).then(latchReconcileFailure(reconcileEpoch.current));
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
    authoringDocument,
  ]);

  const selectInstance = (id) => setSelection(MobKitFlowController.graphSelectionProjection("instance", id));
  const selectEdge = (id) => setSelection(MobKitFlowController.graphSelectionProjection("edge", id));
  const clearSelection = (nextSelection = { kind: null, id: null }) => setSelection(nextSelection || { kind: null, id: null });

  React.useEffect(() => {
    const onKey = (e) => {
      const tg = e.target;
      if (tg.tagName === "INPUT" || tg.tagName === "TEXTAREA" || tg.tagName === "SELECT") return;
      if (e.key === "Backspace" || e.key === "Delete") {
        if (selection.kind === "instance") {
          const nextSelection = { kind: null, id: null };
          applyMobKitAuthoringOperation({
            intent: "graph.deleteNode",
            instanceId: selection.id,
          }).then((result) => {
            if (result?.ok === false) return;
            clearSelection(nextSelection);
          });
        }
        else if (selection.kind === "edge") {
          const nextSelection = { kind: null, id: null };
          applyMobKitAuthoringOperation({
            intent: "graph.deleteEdge",
            edgeId: selection.id,
          }).then((result) => {
            if (result?.ok === false) return;
            clearSelection(nextSelection);
          });
        }
      }
      if ((e.metaKey || e.ctrlKey) && e.key === "z") {
        e.preventDefault();
        handleHistoryStep(e.shiftKey ? "redo" : "undo");
      }
      if (e.key === "Escape") {
        clearSelection(); closeGraphAddMenu();
        applyApiOverlayPatch(MobKitFlowController.apiOverlayClearTransition()); clearSourceProjection();
        if (creating) setCreating(null);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const handleRequestAdd = (col, row) => {
    const result = MobKitFlowController.graphAddMenuOpenProjection({ col, row, grid: catalogs.grid });
    setAddAt(result.addAt);
  };
  const closeGraphAddMenu = () => {
    const result = MobKitFlowController.graphAddMenuCloseProjection();
    setAddAt(result.addAt);
  };

  const handlePick = (pick) => {
    if (!addAt) return;
    const nextMenu = MobKitFlowController.graphAddMenuCloseProjection();
    setAddAt(nextMenu.addAt);
    applyMobKitAuthoringOperation({
      intent: "graph.insertNode",
      pick,
      cell: addAt,
    }).then((result) => {
      if (result?.ok === false) {
        showAuthoringFailure(result, authoringFailureHead("graph_node_insert"));
        return;
      }
      const id = result?.selection?.id;
      if (id) selectInstance(id);
    }).catch((error) => showAuthoringFailure(error, authoringFailureHead("graph_node_insert")));
  };

  // MobKit-owned undo/redo: the draft store keeps bounded document history
  // recorded by its own saves; stepping returns the restored row, which
  // hydrates like any other registry selection.
  const handleHistoryStep = async (direction) => {
    if (!currentFlowId) return;
    setApiBusy(true);
    try {
      const stepper = direction === "redo"
        ? MobKitFlowController.redoDocument
        : MobKitFlowController.undoDocument;
      let result;
      try {
        result = await stepper({ id: currentFlowId, ...currentDraftGuard() });
      } catch (error) {
        if (!MobKitFlowController.isDraftGuardConflictError(error)) throw error;
        // An in-flight autosave just bumped the revision; the store is still
        // the single writer of history, so step it without the stale guard.
        result = await stepper({ id: currentFlowId });
      }
      if (!result?.stepped || !result?.row?.document) return;
      setFlows((rows) => MobKitFlowController.flowRegistryUpsertRowPatch(rows, result.row));
      hydrateMobpackDocument(
        { document: result.row.document, validation: result.row.validation || null },
        {
          id: result.row.id,
          flowRow: result.row,
          addToRegistry: false,
          openEditor: false,
        },
      );
    } catch (error) {
      showAuthoringFailure(error, authoringFailureHead(direction));
    } finally {
      setApiBusy(false);
    }
  };

  const handleAgentNavigation = (id) => {
    const next = MobKitFlowController.agentNavigationProjection(id);
    setAddAt(next.addAt);
    setView(next.view);
    setAgentSel(next.selection);
  };
  const handleTopRailNavigation = (target) => {
    const next = MobKitFlowController.topRailNavigationTransition(view, target, { mobOpen: !!currentFlowId });
    if (!next) return;
    setView(next.view);
  };
  React.useEffect(() => {
    if (view !== "agents") return;
    const next = MobKitFlowController.agentDefaultSelectionProjection({
      selection: agentSel,
      members: studio.members,
      schemas: studio.schemas,
      agentView: catalogs.agentView,
    });
    if ((next?.kind || null) === (agentSel?.kind || null) && (next?.id || null) === (agentSel?.id || null)) {
      return;
    }
    setAgentSel(next);
  }, [view, agentSel, studio.members, studio.schemas, catalogs.agentView]);
  const handleEditorModeSelection = (target) => {
    const next = MobKitFlowController.editorModeTransition(target);
    if (!next) return;
    setEditorMode(next.editorMode);
  };
  const handleThemeToggle = () => {
    const next = MobKitFlowController.themeToggleTransition(t.theme);
    setTweak(next.field, next.value);
  };

  const applyAuthoringDocumentProjection = (projection) => {
    const plan = MobKitFlowController.authoringProjectionApplyPlan(projection, {
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
    if (projection.document) setCurrentAuthoringDocument(projection.document);
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
  const currentMobKitDocument = () => {
    if (authoringDocumentRef.current) return authoringDocumentRef.current;
    throw new Error(catalogs.errorView.authoringOperationMissingDocumentError || "MobKit authoring operation did not return a document");
  };
  const currentDraftGuard = () => MobKitFlowController.flowRegistryDraftGuard(currentFlow, currentFlowId);
  const buildMobKitProjectedDocument = async (overrides = {}) => {
    const requestToken = currentAuthoringRevision();
    if (editorMode !== "advanced") {
      try {
        const document = currentMobKitDocument();
        return { document, requestToken };
      } catch (error) {
        return { document: null, requestToken, error: error?.message || String(error) };
      }
    }
    const result = await applyMobKitAuthoringOperation({
      intent: "system.syncGraphToFlow",
      reason: "build_projected_document",
    });
    if (!authoringRevisionIsCurrent(requestToken)) {
      return { document: null, requestToken, stale: true };
    }
    if (!result?.document) {
      return {
        document: null,
        requestToken,
        error: result?.error || catalogs.errorView.authoringOperationMissingDocumentError,
      };
    }
    return { document: result.document, requestToken };
  };
  authoringRunnerContext.current = {
    catalogs,
    currentMobKitDocument,
    currentDraftGuard,
    currentAuthoringRevision,
    authoringRevisionIsCurrent,
    beginProjectionSync,
    applyAuthoringDocumentProjection,
    markDraft,
  };
  const applyMobKitAuthoringOperation = React.useCallback((operation) => {
    if (!String(operation?.intent || "").startsWith("system.")) {
      reconcileEpoch.current += 1;
    }
    return authoringOperationRunner.current(operation);
  }, []);
  const mobKitStudio = {
    ...studio,
  };
  const editGraphNode = React.useCallback((id, action, payload = {}) =>
    applyMobKitAuthoringOperation({
      intent: "graph.editNode",
      instanceId: id,
      action,
      payload,
    }), [applyMobKitAuthoringOperation]);
  const editGraphEdge = React.useCallback((id, action, payload = {}) =>
    applyMobKitAuthoringOperation({
      intent: "graph.editEdge",
      edgeId: id,
      action,
      payload,
    }), [applyMobKitAuthoringOperation]);
  const saveRegistryDocument = (rowPatch) => {
    if (!rowPatch?.document) return;
    MobKitFlowController.saveDocument(rowPatch)
      .then((result) => {
        if (result?.row) {
          setFlows((rows) => MobKitFlowController.flowRegistryUpsertRowPatch(rows, result.row));
          emitHostEvent("saved", { id: result.row.id, stage: result.row.stage, ok: true });
        }
      })
      .catch((error) => {
        if (MobKitFlowController.isDraftGuardConflictError(error)) {
          // A concurrent writer already bumped the draft revision. This save
          // carries the older document, so retrying it would overwrite the
          // newer draft. Refetch the authoritative server row to adopt its
          // refreshed revision/etag, THEN force the persistence effect to
          // re-save the CURRENT document against the refreshed guard.
          //
          // Refetching is load-bearing: the conflicting rowPatch's revision
          // is stale, so without refreshing the registry row the re-save
          // re-sends the same expected_revision, conflicts again, and re-arms
          // itself forever (~250 RPC/s autosave loop). Exit requires a
          // revision the server will accept.
          console.warn("MobKit draft save superseded; refreshing draft revision before re-persisting:", error?.message || error);
          const conflictId = rowPatch?.id || rowPatch?.document?.mob_id || currentFlowId;
          MobKitFlowController.getDocument(conflictId)
            .then((refreshed) => {
              const serverRow = refreshed?.row;
              if (serverRow) {
                setFlows((rows) =>
                  MobKitFlowController.flowRegistryRefreshGuardPatch(rows, conflictId, serverRow));
              }
              persistedDocumentSig.current = "";
              setFlows((rows) => (Array.isArray(rows) ? [...rows] : rows));
            })
            .catch((refetchError) => {
              // The refresh itself failed: surface it rather than re-arming a
              // loop that can never converge.
              showAuthoringFailure(refetchError, authoringFailureHead("draft_save"));
            });
          return;
        }
        showAuthoringFailure(error, authoringFailureHead("draft_save"));
      });
  };
  React.useEffect(() => {
    let cancelled = false;
    const abort = new AbortController();
    setDeployCommandPreview("");
    if (!deployContractLoaded) {
      return () => {
        cancelled = true;
        abort.abort();
      };
    }
    buildMobKitProjectedDocument()
      .then(({ document, stale }) => {
        if (cancelled || stale || !document) return null;
        return MobKitFlowController.deployCommandPreviewForDocument(document, { ...currentDraftGuard(), signal: abort.signal });
      })
      .then((preview) => {
        if (!cancelled) {
          setDeployCommandPreview(preview?.command || "");
        }
      })
      .catch(() => {
        if (abort.signal.aborted) return;
        if (!cancelled) {
          setDeployCommandPreview("");
        }
      });
    return () => {
      cancelled = true;
      abort.abort();
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
    // Project against the freshest rows (flowsRef), not the click-time `flows`
    // closure: a blocking deploy can resolve long after this handler was
    // bound, by which point an in-flight autosave may have advanced a row's
    // revision. Clobbering that back to the stale snapshot re-arms the
    // save-conflict loop.
    const projection = MobKitFlowController.flowRegistryPersistOutcomeProjection(flowsRef.current, {
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
      document = currentMobKitDocument();
    } catch {
      return;
    }
    const persistence = MobKitFlowController.flowRegistryPersistDocumentProjection(flows, {
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
    authoringDocument,
  ]);

  const handleDeployPlanTrace = async () => {
    let requestToken = null;
    setApiBusy(true);
    try {
      const projected = await buildMobKitProjectedDocument();
      if (projected.stale || !projected.document) return;
      const document = projected.document;
      requestToken = projected.requestToken;
      const plan = await MobKitFlowController.deployDocument(document, { execute: false, ...currentDraftGuard() });
      if (!authoringRevisionIsCurrent(requestToken)) return;
      const outcome = MobKitFlowController.deployOutcome(document, plan, { execute: false });
      window.__mobkitFlowLastDocument = document;
      window.__mobkitFlowLastDeployPlanTrace = plan;
      persistCurrentOutcome(outcome);
      setValidationResults(outcome.validationRows);
      setStage(outcome.stage);
      applyApiOverlayPatch(MobKitFlowController.deployPlanTraceReadyTransition(document, plan));
    } catch (error) {
      if (requestToken !== null && !authoringRevisionIsCurrent(requestToken)) return;
      const outcome = MobKitFlowController.deployErrorOutcome(error, { execute: false, errorView: catalogs.errorView });
      setValidationResults(outcome.validationRows);
      applyApiOverlayPatch(MobKitFlowController.validationSheetOpenTransition());
      setStage(outcome.stage);
    } finally {
      setApiBusy(false);
    }
  };

  const renderCurrentSourceDocument = async (requestToken, projectedDocument = null) => {
    const document = projectedDocument || (await buildMobKitProjectedDocument()).document;
    if (!document) return null;
    const result = await MobKitFlowController.sourceDocument(document, currentDraftGuard());
    const projection = MobKitFlowController.sourceDocumentFromSourceResult(document, result, {
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
      const projected = await buildMobKitProjectedDocument();
      if (projected.stale || !projected.document) return;
      const document = projected.document;
      requestToken = beginSourceProjection();
      const nextSourceDocument = await renderCurrentSourceDocument(requestToken, document);
      if (!nextSourceDocument || !sourceProjectionIsCurrent(requestToken)) return;
      applySourceProjectionPatch(MobKitFlowController.sourceDrawerReadyTransition(nextSourceDocument));
    } catch (error) {
      if (requestToken !== null && !sourceProjectionIsCurrent(requestToken)) return;
      const outcome = MobKitFlowController.sourceErrorOutcome(error, { errorView: catalogs.errorView });
      setValidationResults(outcome.validationRows);
      applyApiOverlayPatch(MobKitFlowController.validationSheetOpenTransition());
      setStage(outcome.stage);
    } finally {
      setApiBusy(false);
    }
  };

  React.useEffect(() => {
    clearSourceProjection();
  }, [view, editorMode, clearSourceProjection]);

  const handleInlineSource = async (surface = "basic", sourceRequest = null) => {
    let requestToken = null;
    const toggle = MobKitFlowController.inlineSourceToggleTransition({
      open: inlineSourceOpen,
      currentSurface: inlineSourceSurface,
      targetSurface: surface,
    });
    if (!toggle.shouldOpen) {
      sourceProjectionVersion.current += 1;
      applySourceProjectionPatch(toggle.patch);
      return;
    }
    const requestedSourcePath = MobKitFlowController.inlineSourceRequestPath(sourceRequest, {
      sourceView: catalogs.sourceView,
      graphView: catalogs.graphView,
    });
    applySourceProjectionPatch(toggle.patch);
    setApiBusy(true);
    try {
      const projected = await buildMobKitProjectedDocument();
      if (projected.stale || !projected.document) return;
      const document = projected.document;
      requestToken = beginSourceProjection();
      applySourceProjectionPatch(MobKitFlowController.inlineSourcePendingTransition(surface));
      const nextSourceDocument = await renderCurrentSourceDocument(requestToken, document);
      if (!nextSourceDocument || !sourceProjectionIsCurrent(requestToken)) return;
      applySourceProjectionPatch(MobKitFlowController.inlineSourceReadyTransition({
        ...nextSourceDocument,
        ...(requestedSourcePath ? { sourcePath: requestedSourcePath } : {}),
      }));
    } catch (error) {
      if (requestToken !== null && !sourceProjectionIsCurrent(requestToken)) return;
      const outcome = MobKitFlowController.sourceErrorOutcome(error, { errorView: catalogs.errorView });
      setValidationResults(outcome.validationRows);
      applyApiOverlayPatch(MobKitFlowController.validationSheetOpenTransition());
      setStage(outcome.stage);
    } finally {
      applySourceProjectionPatch(MobKitFlowController.inlineSourceBusyTransition(false));
      setApiBusy(false);
    }
  };

  React.useEffect(() => {
    const openGraphSourceFromHash = () => {
      const canvasView = MobKitFlowController.graphCanvasViewState(catalogs.graphView);
      if (!canvasView.sourceFileActivationHash || window.location.hash !== canvasView.sourceFileActivationHash) return;
      window.history.replaceState(null, "", `${window.location.pathname}${window.location.search}`);
      handleInlineSource("graph", { sourcePath: canvasView.sourcePath || catalogs.sourceView.primarySourcePath });
    };
    window.addEventListener("hashchange", openGraphSourceFromHash);
    openGraphSourceFromHash();
    return () => window.removeEventListener("hashchange", openGraphSourceFromHash);
  }, [handleInlineSource, catalogs.graphView]);

  const handleValidate = async () => {
    let requestToken = null;
    setApiBusy(true);
    try {
      const projected = await buildMobKitProjectedDocument();
      if (projected.stale || !projected.document) return;
      const document = projected.document;
      requestToken = projected.requestToken;
      const result = await MobKitFlowController.validateDocument(document, currentDraftGuard());
      if (!authoringRevisionIsCurrent(requestToken)) return;
      const outcome = MobKitFlowController.validationOutcome(document, result);
      window.__mobkitFlowLastDocument = document;
      window.__mobkitFlowLastValidation = result;
      persistCurrentOutcome(outcome);
      setValidationResults(outcome.validationRows);
      setStage(outcome.stage);
    } catch (error) {
      if (requestToken !== null && !authoringRevisionIsCurrent(requestToken)) return;
      const outcome = MobKitFlowController.validationErrorOutcome(error, { errorView: catalogs.errorView });
      setValidationResults(outcome.validationRows);
      setStage(outcome.stage);
    } finally {
      if (requestToken === null || authoringRevisionIsCurrent(requestToken)) {
        applyApiOverlayPatch(MobKitFlowController.validationSheetOpenTransition());
      }
      setApiBusy(false);
    }
  };

  const handlePublish = async () => {
    let requestToken = null;
    setApiBusy(true);
    try {
      const projected = await buildMobKitProjectedDocument();
      if (projected.stale || !projected.document) return;
      const document = projected.document;
      requestToken = projected.requestToken;
      const result = await MobKitFlowController.exportDocument(document, currentDraftGuard());
      if (!authoringRevisionIsCurrent(requestToken)) return;
      const outcome = MobKitFlowController.exportOutcome(document, result);
      window.__mobkitFlowLastDocument = document;
      window.__mobkitFlowLastExport = result;
      persistCurrentOutcome(outcome);
      if (!window.__mobkitFlowDisableDownload) {
        downloadExportResult(result);
      }
      setValidationResults(outcome.validationRows);
      setStage(outcome.stage);
      applyApiOverlayPatch(MobKitFlowController.validationSheetCloseTransition());
    } catch (error) {
      if (requestToken !== null && !authoringRevisionIsCurrent(requestToken)) return;
      const outcome = MobKitFlowController.exportErrorOutcome(error, { errorView: catalogs.errorView });
      setValidationResults(outcome.validationRows);
      applyApiOverlayPatch(MobKitFlowController.validationSheetOpenTransition());
      setStage(outcome.stage);
    } finally {
      setApiBusy(false);
    }
  };

  const handleDeploy = async ({ execute }) => {
    let requestToken = null;
    setApiBusy(true);
    try {
      const projected = await buildMobKitProjectedDocument();
      if (projected.stale || !projected.document) return;
      const document = projected.document;
      requestToken = projected.requestToken;
      const result = await MobKitFlowController.deployDocument(document, { execute, ...currentDraftGuard() });
      const outcome = MobKitFlowController.deployOutcome(document, result, { execute });
      window.__mobkitFlowLastDocument = document;
      window.__mobkitFlowLastDeploy = result;
      // An execute:true deploy is an authoritative host side effect that
      // already ran: persist its outcome and fire the `deployed` host event
      // even if the user edited mid-run (authoring revision moved on).
      // Dropping it here would leave the registry and host listeners unaware
      // of a mob that actually deployed. The local validation-sheet UI
      // mutations stay gated on revision currency so a stale sheet does not
      // flash over newer edits.
      const revisionCurrent = authoringRevisionIsCurrent(requestToken);
      if (!revisionCurrent && !execute) return;
      persistCurrentOutcome(outcome);
      if (execute) emitHostEvent("deployed", { id: currentFlowId, stage: outcome.stage, ok: outcome.stage === "deployed" });
      if (!revisionCurrent) return;
      setValidationResults(outcome.validationRows);
      setStage(outcome.stage);
      applyApiOverlayPatch(MobKitFlowController.validationSheetOpenTransition());
    } catch (error) {
      if (requestToken !== null && !authoringRevisionIsCurrent(requestToken)) return;
      const outcome = MobKitFlowController.deployErrorOutcome(error, { execute, errorView: catalogs.errorView });
      setValidationResults(outcome.validationRows);
      applyApiOverlayPatch(MobKitFlowController.validationSheetOpenTransition());
      setStage(outcome.stage);
    } finally {
      setApiBusy(false);
    }
  };
  const handleDeployPlan = () => handleDeploy({ execute: false });
  const handleDeployRun = () => handleDeploy({ execute: true });
  const basicSourceToggle = MobKitFlowController.inlineSourceToggleButtonState({
    open: inlineSourceOpen,
    currentSurface: inlineSourceSurface,
    targetSurface: "basic",
    basicView: catalogs.basicView,
    sourceView: catalogs.sourceView,
  });

  const hydrateMobpackDocument = (result, options: any = {}) => {
    const activeContract = options.contract || contract;
    const hydration = MobKitFlowController.hydrateMobpackDocumentState(result, {
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
      applyApiOverlayPatch(MobKitFlowController.validationSheetOpenTransition());
      return;
    }
    const hydrationPersistence = MobKitFlowController.flowRegistryDocumentPersistence({
      currentFlowId: hydration.id,
      document: hydration.document,
      validation: hydration.validation,
      stage: hydration.stage,
    });
    if (hydrationPersistence.ok) {
      persistedDocumentSig.current = hydrationPersistence.signature;
    }
    beginDocumentHydration();
    setCurrentAuthoringDocument(hydration.document);
    hydratingDocumentRef.current = true;
    setCatalogs((current) => MobKitFlowController.catalogSkillRealmsPatch(current, hydration.skillRealms));
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
      setFlows(fs => MobKitFlowController.flowRegistryUpsertRowPatch(fs, hydration.registryRow));
    }
    setCurrentFlowId(hydration.id);
    setStage(hydration.stage);
    setValidationResults(hydration.validationRows);
    if (hydration.openEditor) setView("editor");
    const graphProjectionToken = currentAuthoringRevision();
    MobKitFlowController.graphProjectionDocument(hydration.document)
      .then((projectionResult) => {
        if (!authoringRevisionIsCurrent(graphProjectionToken)) return;
        const projection = MobKitFlowController.graphProjectionFromMobKitResult(projectionResult);
        if (!projection) return;
        hydratingDocumentRef.current = true;
        graphProjectionSig.current = MobKitFlowController.graphStructureSignature(projection.instances || [], projection.edges || [], {
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
      .catch((error) => {
        if (!authoringRevisionIsCurrent(graphProjectionToken)) return;
        showAuthoringFailure(error, authoringFailureHead("graph_projection"));
      });
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

  // Apply the deep-link boot intent once the authoring contract and the
  // library rows have hydrated: ?open=<id> opens that row into the FLOW
  // section (a missing row keeps the library up under the standard failure
  // sheet), ?intent=new opens the NEW MOB modal with an optional preselected
  // template. The default intent is the library itself, which is already
  // home.
  const bootIntentPending = React.useRef(BOOT_INTENT.kind !== "library");
  React.useEffect(() => {
    if (!bootIntentPending.current || !canCreateAuthoring) return;
    bootIntentPending.current = false;
    if (BOOT_INTENT.kind === "new") {
      setCreating(MobKitFlowController.newFlowInitialState({
        blankTemplate: catalogs.blankMobpack,
        template: BOOT_INTENT.template,
        templates,
      }));
      return;
    }
    const selection = MobKitFlowController.flowRegistrySelectionState(flows, BOOT_INTENT.id);
    if (openFlowRegistrySelection(selection)) return;
    const outcome = MobKitFlowController.bootIntentOpenFailureOutcome(BOOT_INTENT, { errorView: catalogs.errorView });
    setValidationResults(outcome.validationRows);
    setStage(outcome.stage);
    applyApiOverlayPatch(MobKitFlowController.validationSheetOpenTransition());
  }, [canCreateAuthoring, flows, templates, catalogs]);

  const handleImportFile = async (event) => {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) return;
    setApiBusy(true);
    try {
      const result = await MobKitFlowController.importDocument(await importParamsFromFile(file));
      window.__mobkitFlowLastImport = result;
      hydrateImportedDocument(result);
    } catch (error) {
      const outcome = MobKitFlowController.importErrorOutcome(error, { filename: file.name, errorView: catalogs.errorView });
      setValidationResults(outcome.validationRows);
      applyApiOverlayPatch(MobKitFlowController.validationSheetOpenTransition());
      setStage(outcome.stage);
    } finally {
      setApiBusy(false);
    }
  };

  const shellState = MobKitFlowController.topRailState({ contract, deploySettings, stage, view, mobOpen: !!currentFlowId, theme: t.theme, deployView: catalogs.deployView, capabilities });
  const switcherState = MobKitFlowController.mobSwitcherState(flows, currentFlowId, { deployView: catalogs.deployView });

  return (
    <div className={"app density--" + t.density + " inspector--" + t.inspectorLayout + " view--" + view}>
      <TopRail
        stage={stage}
        view={view}
        onNavigate={handleTopRailNavigation}
        currentFlowName={currentFlow?.name || "—"}
        switcherState={switcherState}
        onOpenMob={(id) => {
          const selection = MobKitFlowController.flowRegistrySelectionState(flows, id);
          openFlowRegistrySelection(selection);
        }}
        contract={contract}
        theme={t.theme}
        railState={shellState}
        onToggleTheme={handleThemeToggle}
        onValidate={handleValidate}
        onPublish={handlePublish}
        onDeployPlan={handleDeployPlan}
        onDeployRun={handleDeployRun}
        onImport={() => importInputRef.current?.click()}
        onDeployPlanTrace={handleDeployPlanTrace}
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

      {view === "library" && (
        <FlowsView
          flows={flows}
          currentFlowId={currentFlowId}
          onOpen={(id) => {
            const selection = MobKitFlowController.flowRegistrySelectionState(flows, id);
            openFlowRegistrySelection(selection);
          }}
          canCreate={canCreateAuthoring}
          flowRegistryView={catalogs.flowRegistryView}
          onNew={() => {
            if (!canCreateAuthoring) return;
            setCreating(MobKitFlowController.newFlowInitialState({ blankTemplate: catalogs.blankMobpack }));
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
            onOpenSourceFile={(sourceRequest) => handleInlineSource("graph", sourceRequest)}
            memberFocus={null}
            grid={catalogs.grid}
            contract={contract}
            graphView={catalogs.graphView}
            toolCatalog={catalogs.toolCatalog}
            applyAuthoringIntent={applyMobKitAuthoringOperation}
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
              editGraphNode={editGraphNode}
              editGraphEdge={editGraphEdge}
              deleteGraphNode={(id) => applyMobKitAuthoringOperation({
                intent: "graph.deleteNode",
                instanceId: id,
              })}
              deleteGraphEdge={(id) => applyMobKitAuthoringOperation({
                intent: "graph.deleteEdge",
                edgeId: id,
              })}
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
          sourceToggleLabel={basicSourceToggle.label}
          onCloseSource={clearSourceProjection}
          contract={contract}
          toolCatalog={catalogs.toolCatalog}
          sourceView={catalogs.sourceView}
          basicView={catalogs.basicView}
          launchView={catalogs.launchView}
          conditionView={catalogs.conditionView}
          applyAuthoringIntent={applyMobKitAuthoringOperation}
        />
      )}

      {view === "settings" && (
        <Tweaks
          t={t}
          setTweak={setTweak}
          deploySettings={deploySettings}
          mobSettings={mobSettings}
          members={studio.members}
          modelCatalog={catalogs.models}
          contract={contract}
          deployCommandPreview={deployCommandPreview}
          settingsView={catalogs.settingsView}
          applyAuthoringIntent={applyMobKitAuthoringOperation}
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
          applyAgentIntent={applyMobKitAuthoringOperation}
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
          templateOptions={MobKitFlowController.newFlowTemplateOptions(templates, {
            canCreateBlank: canCreateAuthoring,
            blankTemplate: catalogs.blankMobpack,
          })}
          newFlowView={catalogs.newFlowView}
          onCreate={async (spec) => {
            if (!canCreateAuthoring) return;
            setApiBusy(true);
            try {
              const result = await MobKitFlowController.createDocument(spec);
              const row = result?.row;
              if (!row?.document) return;
              setFlows(Array.isArray(result?.rows) ? result.rows : MobKitFlowController.flowRegistryUpsertRowPatch(flows, row));
              hydrateMobpackDocument(
                { document: row.document, validation: row.validation || null },
                {
                  id: row.id,
                  flowRow: row,
                  addToRegistry: false,
                  openEditor: true,
                },
              );
              emitHostEvent("created", { id: row.id, stage: row.stage, ok: true });
              setCreating(null);
            } catch (error) {
              const outcome = MobKitFlowController.importErrorOutcome(error, { filename: "mobkit/mobpacks/create", errorView: catalogs.errorView });
              setValidationResults(outcome.validationRows);
              applyApiOverlayPatch(MobKitFlowController.validationSheetOpenTransition());
              setStage(outcome.stage);
            } finally {
              setApiBusy(false);
            }
          }}
        />
      )}

      <DeployPlanTrace open={deployPlanOpen} onClose={() => applyApiOverlayPatch(MobKitFlowController.deployPlanTraceCloseTransition())} onActiveStep={setActiveStepId} runKey={deployPlanKey} document={deployPlanDocument} plan={deployPlanResult} deployView={catalogs.deployView} />
      <ValidateSheet open={validate} onClose={() => applyApiOverlayPatch(MobKitFlowController.validationSheetCloseTransition())} onPublish={handlePublish} onDeployPlan={handleDeployPlan} onDeployRun={handleDeployRun} results={validationResults} stage={stage} deployView={catalogs.deployView} capabilities={capabilities} />
      <SourceDrawer open={sourceOpen} onClose={clearSourceProjection} state={sourceDocument} sourceView={catalogs.sourceView} />
    </div>
  );
}

function rpcUrlFromShell() {
  const meta = document.querySelector('meta[name="mobkit-base-url"]');
  const base = (meta?.getAttribute("content") || "").trim().replace(/\/+$/, "");
  return `${base}/flow-editor/rpc`;
}

// Host integration: every successful create / registry save / execute deploy
// notifies the embedding host twice with the same controller-built payload
// ({ type, id, stage, ok }) — a window CustomEvent for same-document
// listeners and a parent postMessage for iframe hosts.
function emitHostEvent(kind, detail) {
  const payload = MobKitFlowController.hostEventPayload(kind, detail);
  if (!payload) return;
  window.dispatchEvent(new CustomEvent(payload.type, { detail: payload }));
  if (window.parent !== window) {
    window.parent.postMessage(payload, "*");
  }
}

function downloadExportResult(result) {
  const download = MobKitFlowController.exportDownloadPayload(result);
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
  return MobKitFlowController.importParamsFromDecodedFile({
    filename,
    mediaType,
    text: new TextDecoder("utf-8").decode(bytes),
    contentBase64: btoa(binary),
  });
}

function TopRail({ stage, view, onNavigate, currentFlowName, switcherState, onOpenMob, theme, railState, onToggleTheme, onValidate, onPublish, onDeployPlan, onDeployRun, onImport, onDeployPlanTrace, onYaml, contract, deploySettings }) {
  // <details> keeps itself open across re-renders; picking a mob (or "view
  // all") must collapse the switcher the same way a menu choice closes a menu.
  const closeSwitcher = (event) => {
    const details = event.currentTarget.closest("details");
    if (details) details.removeAttribute("open");
  };
  return (
    <header className="toprail">
      <div className="brand">
        <span className="dot" />
        <span>{railState.brandLabel}</span>
      </div>
      <nav className="viewtabs">
        {railState.sectionTabs.map((tab) => (
          <button key={tab.target} className={"viewtab" + (tab.current ? " is-current" : "")} onClick={() => onNavigate(tab.target)}>{tab.label}</button>
        ))}
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
        {railState.mobOpen && (
          <>
            <button className="crumb crumb--link" onClick={() => onNavigate("library")}>{railState.flowsCrumbLabel}</button>
            <span className="crumb crumb--sep">{railState.crumbSeparator}</span>
            {/* Keyed on the view so navigating sections remounts (closes) the dropdown. */}
            <details className="crumb-switcher" key={view}>
              <summary className="crumb is-current crumb-switcher__summary">{currentFlowName}</summary>
              <div className="crumb-switcher__panel">
                {switcherState.rows.map((row) => (
                  <button key={row.id} className={row.className} onClick={(event) => { closeSwitcher(event); onOpenMob(row.id); }}>{row.name}</button>
                ))}
                <button className="crumb-switcher__item crumb-switcher__item--all" onClick={(event) => { closeSwitcher(event); onNavigate("library"); }}>{switcherState.viewAllLabel}</button>
              </div>
            </details>
          </>
        )}
      </nav>
      <div className="actions">
        {railState.mobOpen && (
          <>
            <span className="stage" data-state={stage}><span className="glyph" />{stage}</span>
            <button className="btn btn--ghost btn--sm" onClick={onValidate}>{railState.validateLabel}</button>
            <button className="btn btn--primary btn--sm" disabled={railState.deployActionsDisabled} onClick={onPublish}>{railState.publishLabel}</button>
            <details className="actions-menu">
              <summary className="btn btn--ghost btn--sm actions-menu__summary">{railState.overflowLabel}</summary>
              <div className="actions-menu__panel">
                <button className="actions-menu__item" onClick={onDeployPlanTrace}>{railState.planTraceLabel}</button>
                <button className="actions-menu__item" onClick={onImport}>{railState.importLabel}</button>
                <button className="actions-menu__item" disabled={railState.deployActionsDisabled} onClick={onDeployPlan}>{railState.deployPlanLabel}</button>
                <button className="actions-menu__item actions-menu__item--primary" disabled={railState.deployRunDisabled} onClick={onDeployRun}>{railState.deployLabel}</button>
              </div>
            </details>
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

// ── Mob library view ──────────────────────────────────────────────
function FlowsView({ flows, currentFlowId, onOpen, onNew, canCreate, flowRegistryView = null }) {
  const registryState = MobKitFlowController.flowRegistryViewState(flows, currentFlowId, {
    canCreate,
    flowRegistryView,
    nowUnixMs: Date.now(),
  });
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
      {registryState.empty && (
        <div className="flows-view__empty">
          <div className="flows-view__empty-title">{registryState.empty.title}</div>
          <div className="flows-view__empty-text">{registryState.empty.text}</div>
        </div>
      )}
      {registryState.sections.map((section) => (
        <div key={section.key} className="flows-list">
          <div className="flows-list__section">
            <span className="flows-list__section-label">{section.label}</span>
            {section.hint && <span className="flows-list__section-hint">{section.hint}</span>}
          </div>
          <div className="flows-list__head">
            {registryState.columns.map(column => <span key={column.key}>{column.label}</span>)}
          </div>
          {section.rows.map(f => (
            <button key={f.id} className={f.className} onClick={() => onOpen(f.id)}>
              <span className="flows-list__name">{f.name}</span>
              <span className="flows-list__sub">{f.description}</span>
              <span className="flows-list__sub">{f.updated}</span>
              <span className="stage" data-state={f.stage}><span className="glyph" />{f.stage}</span>
            </button>
          ))}
        </div>
      ))}
    </div>
  );
}

// ── New Mob modal (single step: name + template) ─────────────────
function NewFlowModal({ state, setState, onCreate, templateOptions = [], newFlowView = null }) {
  const setField = (field, value) => setState((current) => MobKitFlowController.newFlowModalFieldPatch(current, field, value));
  const modalState = MobKitFlowController.newFlowModalState(state, templateOptions, newFlowView);

  return (
    <div className="modal-backdrop" onClick={() => setState(null)}>
      <div className="modal modal--new" onClick={e => e.stopPropagation()}>
        <div className="modal__head">
          <div className="inspector__eyebrow">{modalState.eyebrow}</div>
          <button className="btn btn--ghost btn--sm" onClick={() => setState(null)}>{modalState.closeLabel}</button>
        </div>
        <div className="modal__body">
          <div className="field">
            <label className="field__label">{modalState.nameLabel}</label>
            <input className="field__input" autoFocus placeholder={modalState.namePlaceholder} value={modalState.name} onChange={e => setField("name", e.target.value)} />
          </div>
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
        <div className="modal__foot">
          <span />
          <button
            className="btn btn--primary btn--sm"
            disabled={modalState.createDisabled}
            onClick={() => onCreate(MobKitFlowController.newFlowModalCreateSpec(modalState))}
          >{modalState.createLabel}</button>
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

// ── Settings section ──────────────────────────────────────────────
// Full-page settings surface for the open mob (the SETTINGS rail tab),
// reusing the generic Tweak* controls from @flow-editor-components. The
// floating panel shell is gone from this app; the components package
// still exports it for embedders.
function Tweaks({ t, setTweak, deploySettings, mobSettings, members = [], modelCatalog = [], contract, deployCommandPreview, settingsView = null, applyAuthoringIntent = null }) {
  const setDeployField = (field, value) => {
    if (!applyAuthoringIntent) return;
    applyAuthoringIntent({
      intent: "settings.updateDeployField",
      field,
      value,
    });
  };
  const setMobField = (field, value) => {
    if (!applyAuthoringIntent) return;
    applyAuthoringIntent({
      intent: "settings.updateMobField",
      field,
      value,
    });
  };
  const editRoleWiring = (operation) => {
    if (!applyAuthoringIntent) return;
    applyAuthoringIntent({
      intent: "settings.editRoleWiring",
      ...operation,
    });
  };
  const controlState = MobKitFlowController.tweaksControlState({
    deploySettings,
    mobSettings,
    members,
    modelCatalog,
    contract,
    settingsView,
  });
  return (
    <div className="settings-view">
      <div className="settings-view__groups">
        <section className="settings-view__group">
          <TweakSection label={controlState.canvasTitle}>
            <TweakRadio label={controlState.edgeStyleLabel} value={t.edgeStyle} onChange={v => setTweak("edgeStyle", v)}
              options={controlState.edgeStyleOptions} />
            <TweakRadio label={controlState.densityLabel} value={t.density} onChange={v => setTweak("density", v)}
              options={controlState.densityOptions} />
          </TweakSection>
        </section>
        <section className="settings-view__group">
          <TweakSection label={controlState.themeTitle}>
            <TweakRadio label={controlState.themeModeLabel} value={t.theme} onChange={v => setTweak("theme", v)}
              options={controlState.themeModeOptions} />
          </TweakSection>
        </section>
        <section className="settings-view__group">
          <TweakSection label={controlState.inspectorTitle}>
            <TweakRadio label={controlState.inspectorLayoutLabel} value={t.inspectorLayout} onChange={v => setTweak("inspectorLayout", v)}
              options={controlState.inspectorLayoutOptions} />
          </TweakSection>
        </section>
        <section className="settings-view__group">
          <TweakSection label={controlState.mobTitle}>
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
              onAction={editRoleWiring}
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
        </section>
        <section className="settings-view__group">
          <TweakSection label={controlState.deployTitle}>
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
        </section>
      </div>
    </div>
  );
}

function RoleWiringEditor({ value, profileOptions, settingsView, onAction }) {
  const wiringState = MobKitFlowController.mobRoleWiringEditorState(value, profileOptions, settingsView);
  const updateSource = (index, value) => {
    onAction && onAction({ action: "set_source", index, value });
  };
  const updateTarget = (index, value) => {
    onAction && onAction({ action: "set_target", index, value });
  };
  const removeRule = (index) => {
    onAction && onAction({ action: "delete", index });
  };
  const addRule = () => {
    onAction && onAction({ action: "add" });
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
  const advancedState = MobKitFlowController.advancedMobSettingsEditorState(value, settingsView);
  const [draft, setDraft] = React.useState(advancedState.text);
  const [error, setError] = React.useState("");
  React.useEffect(() => {
    setDraft(advancedState.text);
    setError("");
  }, [advancedState.text]);
  const commit = (next) => {
    setDraft(next);
    const result = MobKitFlowController.advancedMobSettingsDraftPatch(next, settingsView);
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
