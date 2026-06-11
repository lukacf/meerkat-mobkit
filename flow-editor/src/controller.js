/* global window, fetch */
// MobKit Flow Editor controller plane.
// Keeps deployable document generation and API calls outside the visual JSX.

(function () {
  async function loadSchema(options = {}) {
    return callRpc(rpcMethod("schema"), {}, options);
  }

  async function loadCapabilities(options = {}) {
    return callRpc("mobkit/capabilities", {}, options);
  }

  async function loadCatalogs(options = {}) {
    return callRpc(rpcMethod("catalogs"), {}, options);
  }

  async function validateDocument(document, options = {}) {
    const { signal, rkatValidate, rkat_validate, ...requestOptions } = options || {};
    return callRpc(rpcMethod("validate"), {
      document,
      rkat_validate: rkatValidate ?? rkat_validate ?? true,
      ...requestOptions,
    }, { signal });
  }

  async function sourceDocument(document, options = {}) {
    const { signal, ...requestOptions } = options || {};
    return callRpc(rpcMethod("source"), { document, ...requestOptions }, { signal });
  }

  async function exportDocument(document, options = {}) {
    const { signal, ...requestOptions } = options || {};
    return callRpc(rpcMethod("export"), { document, ...requestOptions }, { signal });
  }

  async function deployDocument(document, options = {}) {
    const { signal, ...requestOptions } = options || {};
    return callRpc(rpcMethod("deploy"), { document, ...requestOptions }, { signal });
  }

  async function deployCommandPreviewForDocument(document, options = {}) {
    const { signal, packPath, prompt: optionPrompt, deploySettings, ...requestOptions } = options || {};
    const sourceDocument = document && typeof document === "object" ? document : {};
    const deploy = normalizeDeploySettings(sourceDocument.deploy || deploySettings);
    const prompt = String(optionPrompt || deploy.prompt || "").trim();
    const request = {
      document: {
        ...sourceDocument,
        deploy,
      },
      ...requestOptions,
    };
    if (String(packPath || "").trim()) request.pack_path = String(packPath).trim();
    if (prompt) request.prompt = prompt;
    return callRpc(rpcMethod("deployCommand"), request, { signal });
  }

  async function importDocument(params, options = {}) {
    return callRpc(rpcMethod("import"), params || {}, options);
  }

  async function listDocuments(params = {}, options = {}) {
    return callRpc(rpcMethod("list"), params || {}, options);
  }

  async function getDocument(id, params = {}, options = {}) {
    return callRpc(rpcMethod("get"), { ...(params || {}), id }, options);
  }

  async function createDocument(spec = {}, options = {}) {
    return callRpc(rpcMethod("create"), spec || {}, options);
  }

  // MobKit-owned history steps over the draft store: the server restores a
  // snapshot it recorded itself, so the browser never authors restore state.
  async function undoDocument(params = {}, options = {}) {
    return historyStepDocument("undo", params, options);
  }

  async function redoDocument(params = {}, options = {}) {
    return historyStepDocument("redo", params, options);
  }

  async function historyStepDocument(direction, params = {}, options = {}) {
    const { signal } = options || {};
    const request = { id: String(params.id || "").trim() };
    const expectedRevision = params.expected_revision ?? params.expectedRevision;
    if (expectedRevision !== undefined && expectedRevision !== null && expectedRevision !== "") {
      request.expected_revision = Number(expectedRevision);
    }
    const expectedEtag = String(params.expected_etag ?? params.expectedEtag ?? "").trim();
    if (expectedEtag) request.expected_etag = expectedEtag;
    return callRpc(rpcMethod(direction), request, { signal });
  }

  async function saveDocument(row = {}, options = {}) {
    if (flowRegistryRowIsRuntimeProjection(row)) {
      return {
        ok: false,
        error: "runtime_projection_read_only",
        row: null,
        reason: "Runtime flow projections must be forked into a MobKit draft before saving.",
      };
    }
    const document = row.document;
    const request = {
      id: row.id || row.currentFlowId,
      document,
      validation: row.validation ?? null,
      stage: row.stage,
      trigger: row.trigger,
      source: row.source,
    };
    const expectedRevision = row.expectedRevision ?? row.expected_revision ?? row.baseRevision ?? row.base_revision ?? row.revision ?? row.draft_revision;
    if (expectedRevision !== undefined && expectedRevision !== null && expectedRevision !== "") {
      request.expected_revision = Number(expectedRevision);
    }
    const expectedEtag = row.expectedEtag ?? row.expected_etag ?? row.draft_etag ?? row.etag;
    if (expectedEtag) {
      request.expected_etag = String(expectedEtag);
    }
    return callRpc(rpcMethod("save"), request, options);
  }

  async function deleteDocument(id, params = {}, options = {}) {
    return callRpc(rpcMethod("delete"), { ...(params || {}), id }, options);
  }

  async function applyAuthoringOperationDocument(document, operation, options = {}) {
    const {
      signal,
      catalogSnapshot,
      catalog_snapshot,
      expectedCatalogSnapshotId,
      expected_catalog_snapshot_id,
      ...requestOptions
    } = options || {};
    const expectedSnapshotId = String(
      expectedCatalogSnapshotId
      ?? expected_catalog_snapshot_id
      ?? catalogSnapshot?.id
      ?? catalog_snapshot?.id
      ?? catalogSnapshot
      ?? catalog_snapshot
      ?? "",
    ).trim();
    return callRpc(rpcMethod("applyOperation"), {
      document,
      operation,
      ...(expectedSnapshotId ? { expected_catalog_snapshot_id: expectedSnapshotId } : {}),
      ...requestOptions,
    }, { signal });
  }

  function isDraftGuardConflictError(error) {
    const message = String(error?.message || error || "");
    return message.includes("draft revision conflict") || message.includes("draft etag conflict");
  }

  function createAuthoringOperationRunner(options = {}) {
    const hooks = options && typeof options === "object" ? options : {};
    let queue = Promise.resolve();
    const runOperation = async (operation, enqueuedRevision) => {
      if (hooks.isRevisionCurrent && !hooks.isRevisionCurrent(enqueuedRevision)) {
        return {
          ok: false,
          error: hooks.getStaleError?.() || "MobKit authoring operation result is stale",
        };
      }
      const translatedOperation = authoringOperationFromIntent(operation);
      const availability = authoringOperationAvailability(
        hooks.getAuthoringOperations?.() || hooks.authoringOperations || {},
        translatedOperation?.type,
      );
      if (!availability.supported) return { ok: false, error: availability.error };
      const requestToken = hooks.getCurrentRevision?.();
      let document;
      try {
        document = hooks.getCurrentDocument?.();
      } catch (error) {
        return { ok: false, error: error?.message || String(error) };
      }
      let result;
      try {
        result = await applyAuthoringOperationDocument(document, translatedOperation, {
          ...(hooks.getDraftGuard?.() || {}),
          catalogSnapshot: hooks.getCatalogSnapshot?.(),
        });
      } catch (error) {
        if (!isDraftGuardConflictError(error)) throw error;
        // Our own autosave raced this operation and bumped the draft store
        // revision. The submitted document is still the freshest authoring
        // state, so retry once without the optimistic store guard; save-time
        // concurrency control is unaffected.
        result = await applyAuthoringOperationDocument(document, translatedOperation, {
          catalogSnapshot: hooks.getCatalogSnapshot?.(),
        });
      }
      if (hooks.isRevisionCurrent && !hooks.isRevisionCurrent(requestToken)) {
        return {
          ok: false,
          error: hooks.getStaleError?.() || "MobKit authoring operation result is stale",
        };
      }
      const projection = authoringProjectionFromOperationResult(result, hooks.getProjectionDefaults?.() || {});
      if (!projection) {
        return {
          ok: false,
          error: hooks.getMissingDocumentError?.() || "MobKit authoring operation did not return a document",
        };
      }
      hooks.beginProjectionSync?.();
      hooks.applyProjection?.(projection);
      hooks.markDraft?.();
      return result;
    };
    return (operation) => {
      const enqueuedRevision = hooks.getCurrentRevision?.();
      const run = queue.catch(() => null).then(() => runOperation(operation, enqueuedRevision));
      queue = run.catch(() => null);
      return run;
    };
  }

  async function graphProjectionDocument(document, options = {}) {
    const { signal, ...requestOptions } = options || {};
    return callRpc(rpcMethod("graphProjection"), { document, ...requestOptions }, { signal });
  }

  async function graphToFlowDocument(document, options = {}) {
    const { signal, ...requestOptions } = options || {};
    return callRpc(rpcMethod("graphToFlow"), { document, ...requestOptions }, { signal });
  }

  function importParamsFromDecodedFile(input = {}) {
    const {
      filename = "",
      mediaType = "",
      kind = "",
      text = "",
      parsedJson,
      contentBase64 = "",
    } = input;
    const sourceMeta = {
      source_name: String(filename || ""),
      source_media_type: String(mediaType || ""),
    };
    const filenameText = String(filename || "");
    const mediaTypeText = String(mediaType || "");
    const sourceKind = String(kind || inferDecodedFileKind(filenameText, mediaTypeText)).toLowerCase();
    if (sourceKind === "toml") {
      return { ...sourceMeta, mob_toml: String(text || "") };
    }
    if (sourceKind === "json") {
      const parsed = Object.prototype.hasOwnProperty.call(input, "parsedJson")
        ? parsedJson
        : parseDecodedJsonImport(text, filenameText);
      return parsed && typeof parsed === "object" && !Array.isArray(parsed)
        ? { ...parsed, ...sourceMeta }
        : { ...sourceMeta, document: parsed };
    }
    return { ...sourceMeta, content_base64: String(contentBase64 || "") };
  }

  function inferDecodedFileKind(filename, mediaType) {
    const name = String(filename || "");
    const type = String(mediaType || "").toLowerCase();
    if (/\.toml$/i.test(name) || type.includes("toml")) return "toml";
    if (/\.json$/i.test(name) || type.includes("json")) return "json";
    return "binary";
  }

  function parseDecodedJsonImport(text, filename = "") {
    try {
      return JSON.parse(String(text || ""));
    } catch (error) {
      const label = String(filename || "JSON import");
      throw new Error(`${label} is not valid JSON: ${error?.message || error}`);
    }
  }

  const MobKitFlowController = {
    SCHEMA_VERSION,
    RPC_METHODS,
    configure,
    authoringOperationsFromSchema,
    authoringOperationAvailability,
    operationErrorText,
    authoringProjectionApplyPlan,
    flowDraftIdFromSpec,
    newFlowTemplateOptions,
    newFlowInitialState,
    newFlowModalState,
    graphSignature,
    graphStructureSignature,
    graphProjectionForFlow,
    graphProjectionForDocument,
    graphProjectionFromMobKitResult,
    authoringProjectionFromMobKitDocument,
    authoringProjectionFromOperationResult,
    flowFromHydratedDocument,
    hydrateMobpackDocumentState,
    graphToFlow,
    profileName,
    normalizeToolRef,
    addInlineSkillToRealms,
    memberToolAccessPatch,
    memberToolRemovePatch,
    memberToolAccessState,
    stepToolScopeState,
    stepToolScopeAddPatch,
    stepToolScopeRemovePatch,
    memberSkillTogglePatch,
    memberSkillRemovePatch,
    memberInlineSkillPatch,
    memberSkillAccessState,
    agentListState,
    agentSelectionState,
    agentListSelectionProjection,
    agentDefaultSelectionProjection,
    agentEditorControlState,
    agentSourceProvenanceState,
    agentDefinitionOptions,
    agentDefinitionAddControlState,
    agentDefinitionAddErrorState,
    memberSchemaChangeErrorState,
    schemaDefinitionAddErrorState,
    schemaFieldAddErrorState,
    inputParamAddErrorState,
    basicEditorViewState,
    schemaEditorControlState,
    memberPromptSkeleton,
    memberNamePatch,
    memberRealmProfilePatch,
    memberSystemPromptPatch,
    memberProfileBindingPatch,
    memberRuntimeModePatch,
    memberModelPatch,
    memberSchemaPatch,
    memberSchemaCascadePatch,
    memberBackendPatch,
    memberMaxInlinePeerNotificationsPatch,
    memberProviderParamsEditorState,
    memberProviderParamsPatch,
    skillRealmsForDocument,
    catalogSkillRealmsPatch,
    normalizeProviderParams,
    normalizeMobSettings,
    mobSettingsForUi,
    mobDefaultsFromSchema,
    normalizeBudgetSplitPolicy,
    launchModeControlState,
    launchModeKindPatch,
    launchModeMergePatch,
    launchModeSessionPatch,
    launchModeForkSourcePatch,
    launchModeForkContextPatch,
    launchModeBudgetPatch,
    launchBudgetKindPatch,
    launchBudgetFixedLimitPatch,
    launchModeOptions,
    budgetSplitPolicyOptions,
    dispatchModeOptions,
    dependencyModeOptions,
    collectionPolicyOptions,
    deploySurfaceOptions,
    trustPolicyOptions,
    realmBackendOptions,
    profileBackendOptions,
    profileBindingOptions,
    mobBackendDefaultOptions,
    tweaksControlState,
    schemaFieldTypeOptions,
    conditionOperatorOptions,
    forkContextOptions,
    graphGateKindOptions,
    graphTerminalKindOptions,
    graphFrameKindOptions,
    graphEdgeKindOptions,
    repeatIterationInputOptions,
    editorFlowPrimitiveOptions,
    graphControlNodes,
    graphAddNodeMenuState,
    graphAddMenuOpenProjection,
    graphAddMenuCloseProjection,
    basicStepPickerState,
    agentNavigationProjection,
    flowStepTemplate,
    graphFirstConditionPatch,
    graphEdgeConditionOwnerPatch,
    graphEdgeConditionFieldPatch,
    graphEdgeConditionPatch,
    graphEdgeConditionOperatorPatch,
    graphEdgeConditionValuePatch,
    graphEdgeKindPatch,
    graphBranchConditionModePatch,
    graphEdgeFallbackPatch,
    graphSelectionState,
    graphSelectionProjection,
    graphTemplateInspectorState,
    graphInstanceControlState,
    graphToolTagClass,
    graphGridState,
    graphCellXY,
    graphNodeBox,
    graphPortOut,
    graphPortIn,
    graphEdgePath,
    graphEdgeMidpoint,
    graphCellAt,
    graphDragCellAt,
    graphCellCanvasRows,
    graphGridHeaderCanvasRows,
    graphSourceFileAdornment,
    graphCanvasInstances,
    graphCanvasAdornments,
    graphNodeCanvasState,
    graphSourceFileAdornmentCanvasState,
    graphFrameCanvasState,
    graphGateCanvasState,
    graphEdgeCanvasState,
    graphGateControlState,
    graphBranchConditionRows,
    graphTerminalControlState,
    graphEdgeInspectorState,
    graphGateKindPatch,
    graphInstanceLabelPatch,
    graphEdgeLabelPatch,
    graphTerminalKindPatch,
    graphJoinCollectionPatch,
    graphJoinQuorumPatch,
    graphJoinControllerRolePatch,
    graphForkDispatchPatch,
    conditionValueLiteral,
    conditionValueControl,
    inputParamName,
    uniqueInputParamName,
    schemaFieldName,
    uniqueSchemaFieldName,
    schemaDescriptionPatch,
    schemaLikeFieldTypeControlState,
    schemaFieldRowControlState,
    inputParamFieldControlState,
    schemaLikeFieldTypePatch,
    schemaLikeFieldRequiredPatch,
    schemaLikeFieldDescriptionPatch,
    enumValueDraftPatch,
    enumValueCommitPatch,
    enumValueDeletePatch,
    enumValueAddPatch,
    schemaFieldUpdatePatch,
    schemaFieldUpdateCascadePatch,
    schemaFieldRenameCascadePatch,
    schemaFieldDeletePatch,
    schemaFieldDeleteCascadePatch,
    studioAddMemberPatch,
    studioUpdateMemberPatch,
    memberUpdateCascadePatch,
    studioDeleteMemberPatch,
    memberDeleteCascadePatch,
    studioUpdateInstancePatch,
    studioMoveInstancePatch,
    studioDeleteInstancePatch,
    studioUpdateEdgePatch,
    studioDeleteEdgePatch,
    studioAddSchemaPatch,
    studioUpdateSchemaPatch,
    studioDeleteSchemaPatch,
    studioSnapshotState,
    studioHistorySnapshotPatch,
    studioUndoPatch,
    studioRedoPatch,
    emptyAuthoringFlowState,
    flowStepUpdatePatch,
    flowStepInsertPatch,
    flowStepInsertTransition,
    flowStepDeletePatch,
    flowStepDeleteTransition,
    basicStepPickerOpenTransition,
    basicStepPickerCloseTransition,
    basicCanvasClearTransition,
    basicStepSelectionTransition,
    flowStepTaskPatch,
    flowStepInstructionPatch,
    flowStepQuorumPatch,
    flowStepTimeoutPatch,
    flowStepMaxIterationsPatch,
    flowStepLoopIdPatch,
    flowStepRepeatConditionPatch,
    flowStepIterationInputPatch,
    flowStepControllerRolePatch,
    flowStepMemberRolePatch,
    flowStepDispatchModePatch,
    flowStepParallelDispatchPatch,
    flowStepCollectionPatch,
    flowStepDependencyModePatch,
    flowStepOutputFormatPatch,
    flowStepAllowedToolsPatch,
    flowStepBlockedToolsPatch,
    parseLegacyInputFields,
    inputParamsForStep,
    inputParamSummary,
    inputParamOptions,
    basicInputControlState,
    basicConditionOptions,
    inputParamUpdatePatch,
    inputParamDeletePatch,
    inputParamRenamePatch,
    parseGraphConditionVar,
    graphConditionRefForEdge,
    graphConditionOptions,
    basicConditionFromText,
    basicConditionText,
    basicConditionSourcePatch,
    basicConditionFieldPatch,
    basicConditionOperatorPatch,
    basicConditionValuePatch,
    basicBranchConditionPatch,
    basicBranchAddPatch,
    basicConditionLabel,
    basicBranchConditionControlState,
    basicBranchParallelControlState,
    basicForkCanvasState,
    basicRepeatIterationLabel,
    basicRepeatCanvasState,
    basicStepCardState,
    basicRepeatControlState,
    basicMemberStepControlState,
    basicRepeatUntilExpression,
    contractDefaultValue,
    outputFormatOptions,
    normalizeDeploySettings,
    deploySettingsPatch,
    deploySettingsFieldPatch,
    deployCommandPreviewForDocument,
    callRpc,
    loadSchema,
    loadCapabilities,
    loadCatalogs,
    authoringRpcMethodsFromSchema,
    configureAuthoringMethodsFromSchema,
    authoringOperationFromIntent,
    inlineSkillRealmIdFromOperationResult,
    validateDocument,
    sourceDocument,
    exportDocument,
    deployDocument,
    importDocument,
    listDocuments,
    getDocument,
    createDocument,
    saveDocument,
    deleteDocument,
    applyAuthoringOperationDocument,
    createAuthoringOperationRunner,
    graphProjectionDocument,
    graphToFlowDocument,
    importParamsFromDecodedFile,
    deploySettingsForUi,
    deployDefaultsFromSchema,
    modelCatalogFromCatalogs,
    toolCatalogFromCatalogs,
    blankMobpackFromCatalogs,
    emptyMobKitCatalogs,
    mobKitCatalogsFromSchema,
    skillRealmsFromCatalogs,
    mergeSkillRealms,
    graphCanvasViewState,
    runtimeModeOptions,
    diagnosticsToRows,
    deployResultToRows,
    validationSheetState,
    deployPlanTraceState,
    topRailState,
    topRailNavigationTransition,
    editorModeTransition,
    themeToggleTransition,
    validationOutcome,
    exportOutcome,
    deployOutcome,
    validationSheetOpenTransition,
    validationSheetCloseTransition,
    deployPlanTraceReadyTransition,
    deployPlanTraceCloseTransition,
    apiOverlayClearTransition,
    criticalErrorOutcome,
    deployErrorOutcome,
    sourceErrorOutcome,
    validationErrorOutcome,
    exportErrorOutcome,
    importErrorOutcome,
    sourceDocumentFromSourceResult,
    exportDownloadPayload,
    sourceProjectionClearTransition,
    sourceDrawerReadyTransition,
    inlineSourcePendingTransition,
    inlineSourceReadyTransition,
    inlineSourceBusyTransition,
    inlineSourceToggleTransition,
    inlineSourceToggleButtonState,
    inlineSourceRequestPath,
    sourceEditorState,
    sourceFileSelectionTransition,
    sampleFlowsFromCatalogs,
    flowCatalogBootstrapState,
    flowRegistryRowsFromBackend,
    sampleAgentDefinitionsFromCatalogs,
    newFlowModalPatch,
    newFlowModalFieldPatch,
    newFlowModalStepPatch,
    newFlowModalCreateSpec,
    flowRegistryMarkDraftPatch,
    flowRegistryViewState,
    flowRegistrySelectionState,
    flowRegistryRowFromDocument,
    flowRegistryRowIsRuntimeProjection,
    flowImportedIdFromDocument,
    flowRegistryDraftGuard,
    isDraftGuardConflictError,
    undoDocument,
    redoDocument,
    flowRegistryRememberDocumentPatch,
    flowRegistryDocumentPersistence,
    flowRegistryPersistDocumentProjection,
    flowRegistryPersistOutcomeProjection,
    flowRegistryAppendRowPatch,
    flowRegistryUpsertRowPatch,
    renameSchemaDefinition,
    reconcileFlowMemberSteps,
    reconcileFlowMemberSchemas,
    reconcileGraphMemberInstances,
    reconcileFlowControlRoles,
    reconcileGraphControlRoles,
    reconcileFlowLaunchSources,
    reconcileGraphLaunchSources,
    reconcileFlowStepToolScopes,
    reconcileGraphStepToolScopes,
    reconcileAuthoringForMembers,
    reconcileAuthoringWithContract,
    reconcileMemberSkillRefs,
    mobSettingsPatch,
    mobSettingsFieldPatch,
    reconcileDeploySettingsWithContract,
    reconcileMembersWithContract,
    reconcileMobSettingsWithContract,
    reconcileMobSettingsProfiles,
    reconcileSchemaFieldReferences,
    reconcileInputParamReferences,
    reconcileConditionFieldAvailability,
    normalizeRoleWiring,
    mobRoleWiringEditorState,
    mobRoleWiringUpdatePatch,
    mobRoleWiringSourcePatch,
    mobRoleWiringTargetPatch,
    mobRoleWiringDeletePatch,
    mobRoleWiringAddPatch,
    advancedMobSettingsEditorState,
    advancedMobSettingsDraftPatch,
    agentDefinitionsFromCatalogs,
    agentDefinitionCatalogState,
    agentDeleteConfirmationState,
    memberBudgetAffordanceState,
  };

  if (window.__MOBKIT_FLOW_CONTROLLER_TEST__) {
    Object.assign(MobKitFlowController, {
      buildDocument,
      authoringFlowForDocument,
      authoringDocumentFromState,
    });
  }

  window.MobKitFlowController = MobKitFlowController;
})();
