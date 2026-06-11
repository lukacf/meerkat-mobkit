/* global window, fetch */
// MobKit Flow Editor controller plane.
// Keeps deployable document generation and API calls outside the visual JSX.

(function () {
  function agentListState({ members = [], instances = [], schemas = [], selection = null, agentView = null } = {}) {
    const sourceMembers = Array.isArray(members) ? members : [];
    const sourceInstances = Array.isArray(instances) ? instances : [];
    const sourceSchemas = Array.isArray(schemas) ? schemas : [];
    const view = agentViewForState(agentView);
    const memberRows = sourceMembers.map((member) => {
      const placedCount = sourceInstances.filter((instance) => instance?.memberId === member.id).length;
      const selected = selection?.kind === "agent" && selection.id === member.id;
      const placedLabel = placedCount === 0
        ? view.memberPlacedEmptyLabel
        : graphTemplateText(view.memberPlacedCountTemplate, { count: placedCount });
      const isUnplaced = placedCount === 0;
      return {
        id: member.id,
        name: member.name,
        role: member.role,
        model: member.model,
        member,
        selected,
        itemClass: `agents-list__item${selected ? " is-selected" : ""}`,
        bulletRole: member.role,
        bulletStyle: roleAccentStyle(member.role),
        subLabel: graphTemplateText(view.memberSubLabelTemplate, {
          role: member.role,
          model: member.model,
        }),
        placedCount,
        placedLabel,
        isUnplaced,
        placedClass: `agents-list__placed${isUnplaced ? " is-zero" : ""}`,
      };
    });
    const schemaRows = sourceSchemas.map((schema) => {
      const fieldCount = Array.isArray(schema.fields) ? schema.fields.length : 0;
      const usedCount = sourceMembers.filter((member) => member?.schema === schema.id).length;
      const selected = selection?.kind === "schema" && selection.id === schema.id;
      const fieldLabel = graphTemplateText(
        fieldCount === 1 ? view.schemaFieldSingularTemplate : view.schemaFieldPluralTemplate,
        { count: fieldCount },
      );
      const usageLabel = graphTemplateText(view.schemaUsageLabelTemplate, { count: usedCount });
      return {
        id: schema.id,
        schema,
        selected,
        itemClass: `agents-list__item${selected ? " is-selected" : ""}`,
        bulletRole: "schema",
        bulletStyle: roleAccentStyle("schema"),
        fieldCount,
        fieldLabel,
        usedCount,
        usageLabel,
        subLabel: [fieldLabel, usageLabel].filter(Boolean).join(view.sidebarSubLabelSeparator),
      };
    });
    return {
      agentsHeading: view.agentsHeading,
      schemasHeading: view.schemasHeading,
      addSchemaLabel: view.addSchemaLabel,
      authoringOperationUnavailableError: view.authoringOperationUnavailableError,
      schemaAddFallbackError: view.schemaAddFallbackError,
      emptyTitle: view.emptyTitle,
      emptyLines: view.emptyLines,
      missingSchemaLabel: view.missingSchemaLabel,
      missingAgentLabel: view.missingAgentLabel,
      memberCount: memberRows.length,
      schemaCount: schemaRows.length,
      memberRows,
      schemaRows,
    };
  }

  function basicEditorViewState(basicView) {
    const view = basicView && typeof basicView === "object" ? basicView : null;
    return {
      startLabel: String(view?.startLabel || ""),
      loopBadge: String(view?.loopBadge || ""),
      tipsTitle: String(view?.tipsTitle || ""),
      emptyPanelTitle: String(view?.emptyPanelTitle || ""),
      emptyPanelSubtitleParts: Array.isArray(view?.emptyPanelSubtitleParts)
        ? view.emptyPanelSubtitleParts
        : [],
      sourceToggleLabel: String(view?.sourceToggleLabel || ""),
      authoringOperationUnavailableError: String(view?.authoringOperationUnavailableError || ""),
      authoringOperationFallbackError: String(view?.authoringOperationFallbackError || ""),
      memberStepPanelTitleFallback: String(view?.memberStepPanelTitleFallback || ""),
      memberStepPanelSubFallback: String(view?.memberStepPanelSubFallback || ""),
      memberStepMemberLabel: String(view?.memberStepMemberLabel || ""),
      memberStepMemberPlaceholder: String(view?.memberStepMemberPlaceholder || ""),
      memberStepRuntimeDefaultLabel: String(view?.memberStepRuntimeDefaultLabel || ""),
      memberStepInstructionLabel: String(view?.memberStepInstructionLabel || ""),
      memberStepInstructionPlaceholder: String(view?.memberStepInstructionPlaceholder || ""),
      memberStepDispatchLabel: String(view?.memberStepDispatchLabel || ""),
      memberStepCollectionLabel: String(view?.memberStepCollectionLabel || ""),
      memberStepQuorumLabel: String(view?.memberStepQuorumLabel || ""),
      memberStepQuorumPlaceholder: String(view?.memberStepQuorumPlaceholder || ""),
      memberStepTimeoutLabel: String(view?.memberStepTimeoutLabel || ""),
      memberStepDependencyLabel: String(view?.memberStepDependencyLabel || ""),
      memberStepOutputFormatLabel: String(view?.memberStepOutputFormatLabel || ""),
      memberStepAllowedToolsLabel: String(view?.memberStepAllowedToolsLabel || ""),
      memberStepAllowedToolsEmptyLabel: String(view?.memberStepAllowedToolsEmptyLabel || ""),
      memberStepBlockedToolsLabel: String(view?.memberStepBlockedToolsLabel || ""),
      memberStepBlockedToolsEmptyLabel: String(view?.memberStepBlockedToolsEmptyLabel || ""),
      memberStepSchemaHintPrefix: String(view?.memberStepSchemaHintPrefix || ""),
      memberStepSchemaHintToolsPrefix: String(view?.memberStepSchemaHintToolsPrefix || ""),
      memberStepSchemaHintEmptyToolsLabel: String(view?.memberStepSchemaHintEmptyToolsLabel || ""),
      toolScopeNotInCatalogReason: String(view?.toolScopeNotInCatalogReason || ""),
      toolScopeNotEnabledReason: String(view?.toolScopeNotEnabledReason || ""),
      toolScopeToolDescriptionFallback: String(view?.toolScopeToolDescriptionFallback || ""),
      toolScopeRemoveLabel: String(view?.toolScopeRemoveLabel || ""),
      toolScopeSelectMemberPlaceholder: String(view?.toolScopeSelectMemberPlaceholder || ""),
      toolScopeBlockCatalogPlaceholder: String(view?.toolScopeBlockCatalogPlaceholder || ""),
      toolScopeAddProfilePlaceholder: String(view?.toolScopeAddProfilePlaceholder || ""),
      inputPanelIcon: String(view?.inputPanelIcon || ""),
      inputPanelTitle: String(view?.inputPanelTitle || ""),
      inputPanelSub: String(view?.inputPanelSub || ""),
      inputTaskLabel: String(view?.inputTaskLabel || ""),
      inputTaskPlaceholder: String(view?.inputTaskPlaceholder || ""),
      inputParamsTitlePrefix: String(view?.inputParamsTitlePrefix || ""),
      inputAddParamLabel: String(view?.inputAddParamLabel || ""),
      inputParamSourceLabel: String(view?.inputParamSourceLabel || ""),
      inputParamHeaderLabels: {
        name: String(view?.inputParamHeaderLabels?.name || ""),
        type: String(view?.inputParamHeaderLabels?.type || ""),
        required: String(view?.inputParamHeaderLabels?.required || ""),
        description: String(view?.inputParamHeaderLabels?.description || ""),
        action: String(view?.inputParamHeaderLabels?.action || ""),
      },
      inputParamNamePlaceholder: String(view?.inputParamNamePlaceholder || ""),
      inputParamDescriptionPlaceholder: String(view?.inputParamDescriptionPlaceholder || ""),
      inputParamRemoveTitle: String(view?.inputParamRemoveTitle || ""),
      inputParamEnumLabel: String(view?.inputParamEnumLabel || ""),
      inputParamEnumAddLabel: String(view?.inputParamEnumAddLabel || ""),
      inputParamEnumAddValue: String(view?.inputParamEnumAddValue || ""),
      inputEmptyParamsParts: Array.isArray(view?.inputEmptyParamsParts) ? view.inputEmptyParamsParts : [],
      inputTips: Array.isArray(view?.inputTips) ? view.inputTips : [],
      branchPanelTitle: String(view?.branchPanelTitle || ""),
      branchPanelSub: String(view?.branchPanelSub || ""),
      parallelPanelTitle: String(view?.parallelPanelTitle || ""),
      parallelPanelSub: String(view?.parallelPanelSub || ""),
      branchRouteMemberLabel: String(view?.branchRouteMemberLabel || ""),
      parallelJoinMemberLabel: String(view?.parallelJoinMemberLabel || ""),
      branchControllerPlaceholderLabel: String(view?.branchControllerPlaceholderLabel || ""),
      branchEmptyControllerHint: String(view?.branchEmptyControllerHint || ""),
      branchConditionTitle: String(view?.branchConditionTitle || ""),
      branchConditionIntro: String(view?.branchConditionIntro || ""),
      branchConditionRowTitlePrefix: String(view?.branchConditionRowTitlePrefix || ""),
      branchConditionEmptyHint: String(view?.branchConditionEmptyHint || ""),
      branchConditionSourcePlaceholder: String(view?.branchConditionSourcePlaceholder || ""),
      branchConditionFieldPlaceholder: String(view?.branchConditionFieldPlaceholder || ""),
      branchConditionNoSchemaLabel: String(view?.branchConditionNoSchemaLabel || ""),
      branchConditionPreviewPrefix: String(view?.branchConditionPreviewPrefix || ""),
      branchConditionPreviewFallback: String(view?.branchConditionPreviewFallback || ""),
      branchFallbackTitle: String(view?.branchFallbackTitle || ""),
      branchFallbackHint: String(view?.branchFallbackHint || ""),
      addBranchLabel: String(view?.addBranchLabel || ""),
      addParallelBranchLabel: String(view?.addParallelBranchLabel || ""),
      parallelDispatchLabel: String(view?.parallelDispatchLabel || ""),
      parallelCollectionLabel: String(view?.parallelCollectionLabel || ""),
      parallelQuorumLabel: String(view?.parallelQuorumLabel || ""),
      parallelQuorumPlaceholder: String(view?.parallelQuorumPlaceholder || ""),
      branchDependencyLabel: String(view?.branchDependencyLabel || ""),
      repeatPanelTitle: String(view?.repeatPanelTitle || ""),
      repeatPanelSub: String(view?.repeatPanelSub || ""),
      repeatLoopIdLabel: String(view?.repeatLoopIdLabel || ""),
      repeatLoopIdPlaceholder: String(view?.repeatLoopIdPlaceholder || ""),
      repeatConditionTitle: String(view?.repeatConditionTitle || ""),
      repeatConditionIntro: String(view?.repeatConditionIntro || ""),
      repeatEmptyBodyHint: String(view?.repeatEmptyBodyHint || ""),
      repeatMemberPlaceholderLabel: String(view?.repeatMemberPlaceholderLabel || ""),
      repeatConditionFieldPlaceholder: String(view?.repeatConditionFieldPlaceholder || ""),
      repeatConditionNoSchemaLabel: String(view?.repeatConditionNoSchemaLabel || ""),
      repeatPreviewLabel: String(view?.repeatPreviewLabel || ""),
      repeatPreviewFallback: String(view?.repeatPreviewFallback || ""),
      repeatIterationInputLabel: String(view?.repeatIterationInputLabel || ""),
      repeatMaxIterationsLabel: String(view?.repeatMaxIterationsLabel || ""),
      repeatMaxIterationsPlaceholder: String(view?.repeatMaxIterationsPlaceholder || ""),
      repeatTips: Array.isArray(view?.repeatTips) ? view.repeatTips : [],
      repeatCanvasWhileLabel: String(view?.repeatCanvasWhileLabel || ""),
      repeatCanvasNotLabel: String(view?.repeatCanvasNotLabel || ""),
      repeatCanvasMissingMaxIterationsLabel: String(view?.repeatCanvasMissingMaxIterationsLabel || ""),
      repeatCanvasMaxIterationsPrefix: String(view?.repeatCanvasMaxIterationsPrefix || ""),
      repeatCanvasLoopBackPrefix: String(view?.repeatCanvasLoopBackPrefix || ""),
      repeatCanvasExitPrefix: String(view?.repeatCanvasExitPrefix || ""),
      repeatCanvasExitFallback: String(view?.repeatCanvasExitFallback || ""),
      repeatIterationRuntimeDefaultLabel: String(view?.repeatIterationRuntimeDefaultLabel || ""),
      repeatIterationCarryLabel: String(view?.repeatIterationCarryLabel || ""),
      repeatIterationReuseUnsupportedLabel: String(view?.repeatIterationReuseUnsupportedLabel || ""),
      repeatIterationFeedsUnsupportedPrefix: String(view?.repeatIterationFeedsUnsupportedPrefix || ""),
      repeatIterationUnsupportedPrefix: String(view?.repeatIterationUnsupportedPrefix || ""),
      addStepTitle: String(view?.addStepTitle || ""),
      inputStepCardTitle: String(view?.inputStepCardTitle || ""),
      inputStepCardDescFallback: String(view?.inputStepCardDescFallback || ""),
      branchStepCardTitle: String(view?.branchStepCardTitle || ""),
      branchStepCardDesc: String(view?.branchStepCardDesc || ""),
      parallelStepCardTitle: String(view?.parallelStepCardTitle || ""),
      parallelStepCardDescPrefix: String(view?.parallelStepCardDescPrefix || ""),
      parallelStepCardCollectionFallback: String(view?.parallelStepCardCollectionFallback || ""),
      repeatStepCardTitle: String(view?.repeatStepCardTitle || ""),
      repeatStepCardDescPrefix: String(view?.repeatStepCardDescPrefix || ""),
      repeatStepCardDescFallback: String(view?.repeatStepCardDescFallback || ""),
      memberStepCardTitleFallback: String(view?.memberStepCardTitleFallback || ""),
      pickerKickoffTitle: String(view?.pickerKickoffTitle || ""),
      pickerKickoffSub: String(view?.pickerKickoffSub || ""),
      pickerKickoffHint: String(view?.pickerKickoffHint || ""),
      pickerTitle: String(view?.pickerTitle || ""),
      pickerSub: String(view?.pickerSub || ""),
      pickerSearchIcon: String(view?.pickerSearchIcon || ""),
      pickerSearchPlaceholder: String(view?.pickerSearchPlaceholder || ""),
      pickerMembersLabel: String(view?.pickerMembersLabel || ""),
      pickerFlowLabel: String(view?.pickerFlowLabel || ""),
      pickerEmptyMembersHint: String(view?.pickerEmptyMembersHint || ""),
      pickerNewBadgeLabel: String(view?.pickerNewBadgeLabel || ""),
      flowPrimitiveRows: Array.isArray(view?.flowPrimitiveRows) ? view.flowPrimitiveRows : [],
    };
  }

  function graphCanvasViewState(graphView) {
    const view = graphView && typeof graphView === "object" ? graphView : null;
    return {
      zoomOutTitle: String(view?.zoomOutTitle || ""),
      fitTitle: String(view?.fitTitle || ""),
      zoomInTitle: String(view?.zoomInTitle || ""),
      portDragTitle: String(view?.portDragTitle || ""),
      addNodeSearchIcon: String(view?.addNodeSearchIcon || ""),
      addNodeSearchPlaceholder: String(view?.addNodeSearchPlaceholder || ""),
      addNodeCloseLabel: String(view?.addNodeCloseLabel || ""),
      addNodeCloseTitle: String(view?.addNodeCloseTitle || ""),
      addNodeAgentsLabel: String(view?.addNodeAgentsLabel || ""),
      addNodeControlsLabel: String(view?.addNodeControlsLabel || ""),
      addNodeTerminalsLabel: String(view?.addNodeTerminalsLabel || ""),
      addNodeEmptyPrefix: String(view?.addNodeEmptyPrefix || ""),
      addNodeEmptySuffix: String(view?.addNodeEmptySuffix || ""),
      addNodeJumpLabel: String(view?.addNodeJumpLabel || ""),
      authoringOperationUnavailableError: String(view?.authoringOperationUnavailableError || ""),
      authoringOperationFallbackError: String(view?.authoringOperationFallbackError || ""),
      gatePaletteRows: Array.isArray(view?.gatePaletteRows) ? view.gatePaletteRows : [],
      terminalPaletteRows: Array.isArray(view?.terminalPaletteRows) ? view.terminalPaletteRows : [],
      gateKindLabels: view?.gateKindLabels && typeof view.gateKindLabels === "object" ? view.gateKindLabels : {},
      terminalKindLabels: view?.terminalKindLabels && typeof view.terminalKindLabels === "object" ? view.terminalKindLabels : {},
      frameKindLabels: view?.frameKindLabels && typeof view.frameKindLabels === "object" ? view.frameKindLabels : {},
      edgeKindLabels: view?.edgeKindLabels && typeof view.edgeKindLabels === "object" ? view.edgeKindLabels : {},
      inspectorDeleteLabel: String(view?.inspectorDeleteLabel || ""),
      inspectorLabelTitle: String(view?.inspectorLabelTitle || ""),
      inspectorKindTitle: String(view?.inspectorKindTitle || ""),
      inspectorRuntimeDefaultLabel: String(view?.inspectorRuntimeDefaultLabel || ""),
      instanceEyebrow: String(view?.instanceEyebrow || ""),
      instanceIdLineTemplate: String(view?.instanceIdLineTemplate || ""),
      instanceMemberRoleTemplate: String(view?.instanceMemberRoleTemplate || ""),
      instanceEditMemberLabel: String(view?.instanceEditMemberLabel || ""),
      instanceModelLabel: String(view?.instanceModelLabel || ""),
      instanceSchemaLabel: String(view?.instanceSchemaLabel || ""),
      instanceToolsLabel: String(view?.instanceToolsLabel || ""),
      instanceMemberHint: String(view?.instanceMemberHint || ""),
      instancePositionTitle: String(view?.instancePositionTitle || ""),
      instancePositionStageLabel: String(view?.instancePositionStageLabel || ""),
      instancePositionSlotLabel: String(view?.instancePositionSlotLabel || ""),
      instanceOutputTitleTemplate: String(view?.instanceOutputTitleTemplate || ""),
      instanceOutputRequiredLabel: String(view?.instanceOutputRequiredLabel || ""),
      instanceOutputHint: String(view?.instanceOutputHint || ""),
      instanceOutputOpenMemberLabel: String(view?.instanceOutputOpenMemberLabel || ""),
      gateEyebrowTemplate: String(view?.gateEyebrowTemplate || ""),
      gateIdLineTemplate: String(view?.gateIdLineTemplate || ""),
      gateQuorumIncomingTemplate: String(view?.gateQuorumIncomingTemplate || ""),
      gateMemberOptionTemplate: String(view?.gateMemberOptionTemplate || ""),
      terminalEyebrowTemplate: String(view?.terminalEyebrowTemplate || ""),
      terminalIdLineTemplate: String(view?.terminalIdLineTemplate || ""),
      terminalAuthoringLockedTitle: String(view?.terminalAuthoringLockedTitle || ""),
      terminalAuthoringLockedHint: String(view?.terminalAuthoringLockedHint || ""),
      edgeEyebrowTemplate: String(view?.edgeEyebrowTemplate || ""),
      edgeTitleTemplate: String(view?.edgeTitleTemplate || ""),
      edgeIdLineTemplate: String(view?.edgeIdLineTemplate || ""),
      edgeFieldPlaceholder: String(view?.edgeFieldPlaceholder || ""),
      edgeFieldNoSchemaPlaceholder: String(view?.edgeFieldNoSchemaPlaceholder || ""),
      gateCollectionTitle: String(view?.gateCollectionTitle || ""),
      gateJoinMemberLabel: String(view?.gateJoinMemberLabel || ""),
      gateJoinMemberPlaceholder: String(view?.gateJoinMemberPlaceholder || ""),
      gateJoinMemberHint: String(view?.gateJoinMemberHint || ""),
      gateDispatchTitle: String(view?.gateDispatchTitle || ""),
      gateDispatchHint: String(view?.gateDispatchHint || ""),
      gateConditionsTitle: String(view?.gateConditionsTitle || ""),
      gateEmptyBranchHint: String(view?.gateEmptyBranchHint || ""),
      gateWiringTitle: String(view?.gateWiringTitle || ""),
      gateIncomingLabel: String(view?.gateIncomingLabel || ""),
      gateOutgoingLabel: String(view?.gateOutgoingLabel || ""),
      branchConditionModeConditionLabel: String(view?.branchConditionModeConditionLabel || ""),
      branchConditionModeFallbackLabel: String(view?.branchConditionModeFallbackLabel || ""),
      branchConditionTargetPrefix: String(view?.branchConditionTargetPrefix || ""),
      graphConditionTargetMissingLabel: String(view?.graphConditionTargetMissingLabel || ""),
      graphConditionOwnerOptionTemplate: String(view?.graphConditionOwnerOptionTemplate || ""),
      graphConditionFieldOptionTemplate: String(view?.graphConditionFieldOptionTemplate || ""),
      graphInputParamSourceLabel: String(view?.graphInputParamSourceLabel || ""),
      sourceFileLabel: String(view?.sourceFileLabel || ""),
      sourceFileAriaLabel: String(view?.sourceFileAriaLabel || ""),
      sourceFileGlyph: String(view?.sourceFileGlyph || ""),
      sourceFileRoleLabel: String(view?.sourceFileRoleLabel || ""),
      sourceFileNodeId: String(view?.sourceFileNodeId || ""),
      sourceFileNodeKind: String(view?.sourceFileNodeKind || ""),
      sourceFileNodeColOffset: Number(view?.sourceFileNodeColOffset || 0),
      sourceFileNodeRowOffset: Number(view?.sourceFileNodeRowOffset || 0),
      sourceFileActivationHash: String(view?.sourceFileActivationHash || ""),
      sourceFileActivationSelector: String(view?.sourceFileActivationSelector || ""),
      branchConditionFieldPlaceholder: String(view?.branchConditionFieldPlaceholder || ""),
      branchConditionNoOptionsHint: String(view?.branchConditionNoOptionsHint || ""),
      edgeConditionTitle: String(view?.edgeConditionTitle || ""),
      edgeNoConditionOptionsHint: String(view?.edgeNoConditionOptionsHint || ""),
      edgeOwnerPlaceholder: String(view?.edgeOwnerPlaceholder || ""),
      edgeFromTitle: String(view?.edgeFromTitle || ""),
      edgeToTitle: String(view?.edgeToTitle || ""),
      edgeRowInstanceLabel: String(view?.edgeRowInstanceLabel || ""),
      edgeRowMemberLabel: String(view?.edgeRowMemberLabel || ""),
      edgeRowSchemaLabel: String(view?.edgeRowSchemaLabel || ""),
      edgeRowMissingValue: String(view?.edgeRowMissingValue || ""),
      edgeTerminalMemberValue: String(view?.edgeTerminalMemberValue || ""),
    };
  }

  function agentSelectionState({ selection = null, members = [], schemas = [], agentView = null } = {}) {
    const view = agentViewForState(agentView);
    const emptyState = {
      title: view.emptyTitle,
      lines: view.emptyLines,
    };
    const base = {
      emptyState,
      missingSchemaLabel: view.missingSchemaLabel,
      missingAgentLabel: view.missingAgentLabel,
    };
    if (!selection) return { ...base, kind: "empty", member: null, schema: null, missing: false };
    if (selection.kind === "schema") {
      const schema = (Array.isArray(schemas) ? schemas : []).find((candidate) => candidate.id === selection.id) || null;
      return { ...base, kind: "schema", member: null, schema, missing: !schema };
    }
    if (selection.kind === "agent") {
      const member = (Array.isArray(members) ? members : []).find((candidate) => candidate.id === selection.id) || null;
      return { ...base, kind: "agent", member, schema: null, missing: !member };
    }
    return { ...base, kind: String(selection.kind || ""), member: null, schema: null, missing: true };
  }

  function agentListSelectionProjection(kind, id) {
    const selectionKind = String(kind || "").trim();
    const selectionId = String(id || "").trim();
    if (!selectionId || (selectionKind !== "agent" && selectionKind !== "schema")) return null;
    return { kind: selectionKind, id: selectionId };
  }

  function agentDefaultSelectionProjection({
    selection = null,
    members = [],
    schemas = [],
    agentView = null,
  } = {}) {
    const current = agentSelectionState({ selection, members, schemas, agentView });
    if ((current.kind === "agent" || current.kind === "schema") && !current.missing) {
      return selection;
    }
    const firstMember = (Array.isArray(members) ? members : []).find((member) => member?.id);
    if (firstMember) return agentListSelectionProjection("agent", firstMember.id);
    const firstSchema = (Array.isArray(schemas) ? schemas : []).find((schema) => schema?.id);
    if (firstSchema) return agentListSelectionProjection("schema", firstSchema.id);
    return null;
  }

  function agentEditorControlState({ member, instances = [], schemas = [], contract, deploySettings, modelCatalog = [], agentView = null, agentDetailView = null } = {}) {
    const agentUiView = agentViewForState(agentView);
    const view = agentDetailViewForState(agentDetailView);
    const placedAt = (Array.isArray(instances) ? instances : []).filter((instance) => instance?.memberId === member?.id);
    const placedCount = placedAt.length;
    const memberName = String(member?.name || member?.id || "agent");
    const instanceNoun = placedCount === 1 ? view.instanceSingular : view.instancePlural;
    const cellNoun = placedCount === 1 ? view.cellSingular : view.cellPlural;
    const schema = (Array.isArray(schemas) ? schemas : []).find((candidate) => candidate.id === member?.schema) || null;
    const profileBinding = typeof member?.profileBinding === "string"
      ? member.profileBinding
      : (member?.realmProfile ? "realm_profile" : "");
    const realmProfileRestriction = profileBindingRestriction(contract, "realm_profile");
    const bindingOptions = [
      { value: "", label: view.missingProfileBindingLabel, disabled: false, reason: "" },
      ...profileBindingOptions(contract, profileBinding),
    ];
    const runtimeMode = typeof member?.runtimeMode === "string" ? member.runtimeMode : "";
    const runtimeOptions = [
      { value: "", label: view.missingRuntimeModeLabel, disabled: false, reason: "" },
      ...runtimeModeOptions(contract, deploySettings, runtimeMode),
    ];
    const backendValue = String(member?.backend || "");
    const backendOptions = profileBackendOptions(
      contract,
      backendValue,
      true,
      view.backendDefinitionDefaultLabel,
    );
    const schemaOptions = [
      { value: "", label: view.schemaNoneLabel, schema: null },
      ...(Array.isArray(schemas) ? schemas : [])
        .filter((candidate) => candidate?.id)
        .map((candidate) => ({ value: candidate.id, label: candidate.id, schema: candidate })),
    ];
    const schemaPreviewRows = (Array.isArray(schema?.fields) ? schema.fields : [])
      .map((field) => ({
        id: field.id,
        name: field.name,
        type: field.type,
        required: !!field.required,
        requiredLabel: field.required ? view.schemaRequiredLabel : "",
      }));
    const modelOptions = (Array.isArray(modelCatalog) ? modelCatalog : [])
      .filter((model) => model?.id)
      .map((model) => ({
        value: model.id,
        label: `${model.label || model.id}${model.vendor ? ` · ${model.vendor}` : ""}`,
        model,
      }));
    if (member?.model && !modelOptions.some((model) => model.value === member.model)) {
      modelOptions.push({ value: member.model, label: member.model, model: null });
    }
    const budgetSection = memberBudgetAffordanceState(member, contract, view);
    return {
      placedAt,
      placedCount,
      authoringOperationUnavailableError: agentUiView.authoringOperationUnavailableError,
      memberUpdateFallbackError: agentUiView.memberUpdateFallbackError,
      toolUpdateFallbackError: agentUiView.toolUpdateFallbackError,
      schemaAssignmentFallbackError: agentUiView.schemaAssignmentFallbackError,
      eyebrow: [view.agentEyebrowPrefix, member?.role || ""].filter(Boolean).join(" · "),
      idLine: `${member?.id || ""} · ${view.usedInLabel} ${placedCount} ${instanceNoun}`,
      deleteLabel: view.deleteLabel,
      deleteCancelLabel: view.deleteCancelLabel,
      deleteNeedsConfirmation: placedCount > 0,
      deleteConfirmMessage: placedCount > 0
        ? `${view.deleteConfirmIntro} "${memberName}"? ${view.deleteConfirmPlacedPrefix} ${placedCount} ${cellNoun} - ${view.deleteConfirmCellsSuffix}`
        : "",
      usageTitle: `${view.usageTitlePrefix} · ${placedCount}`,
      emptyUsageHint: view.emptyUsageHint,
      usageRows: placedAt.map((instance) => ({
        id: instance.id,
        cellLabel: `cell (${Number(instance.col || 0) + 1},${Number(instance.row || 0) + 1})`,
        laneLabel: instance.lane || "—",
        instance,
      })),
      identityTitle: view.identityTitle,
      profileBindingLabel: view.profileBindingLabel,
      realmProfileLabel: view.realmProfileLabel,
      realmProfilePlaceholder: view.realmProfilePlaceholder,
      realmProfileImportHint: realmProfileRestriction.reason || view.realmProfileImportHintFallback,
      realmProfileTitle: view.realmProfileTitle,
      realmProfileReferenceLabel: member?.realmProfile || member?.role || member?.name || "",
      realmProfileReferenceHintBefore: view.realmProfileReferenceHintBefore,
      realmProfileReferenceHintAfter: realmProfileRestriction.reason
        ? `from a target realm. ${realmProfileRestriction.reason}`
        : view.realmProfileReferenceHintAfterFallback,
      modelLabel: view.modelLabel,
      runtimeModeLabel: view.runtimeModeLabel,
      runtimeSectionTitle: view.runtimeSectionTitle,
      backendLabel: view.backendLabel,
      inlinePeerNotificationsLabel: view.inlinePeerNotificationsLabel,
      inlinePeerNotificationsPlaceholder: view.inlinePeerNotificationsPlaceholder,
      systemPromptTitle: view.systemPromptTitle,
      applySkeletonLabel: view.applySkeletonLabel,
      applySkeletonTitle: view.applySkeletonTitle,
      systemPromptPlaceholder: view.systemPromptPlaceholder,
      budgetSection,
      schema,
      profileBinding,
      bindingOptions,
      selectedBinding: bindingOptions.find((option) => option.value === profileBinding) || null,
      isRealmProfile: profileBinding === "realm_profile",
      runtimeMode,
      runtimeOptions,
      selectedRuntime: runtimeOptions.find((option) => option.value === runtimeMode) || null,
      backendValue,
      backendOptions,
      selectedBackend: backendOptions.find((option) => option.value === backendValue) || null,
      schemaOptions,
      outputSchemaTitle: view.outputSchemaTitle,
      schemaPreviewRows,
      hasOutputSchema: !!schema,
      editSchemaLabel: view.editSchemaLabel,
      editSchemaSelection: schema ? { kind: "schema", id: schema.id } : null,
      emptySchemaHint: view.emptySchemaHint,
      modelOptions,
      sourceProvenance: agentSourceProvenanceState(member, agentDetailView),
    };
  }

  function memberBudgetAffordanceState(member, contract, agentDetailView = null) {
    const view = agentDetailViewForState(agentDetailView);
    const policies = Array.isArray(contract?.mob_definition?.budget_split_policies)
      ? contract.mob_definition.budget_split_policies.map(canonicalBudgetSplitPolicyKind).filter(Boolean)
      : [];
    const authored = normalizeBudgetSplitPolicy(member?.budget || member?.budgetSplitPolicy || member?.budget_split_policy);
    const defaultKind = canonicalBudgetSplitPolicyKind(contractDefaultValue(contract, "budget_split_policy"));
    const selectedKind = authored?.kind || defaultKind || policies[0] || "";
    const allPolicies = [...new Set([...policies, selectedKind].filter(Boolean))];
    const options = allPolicies.map((kind) => ({
      value: kind,
      label: view.budgetSplitPolicyLabels[kind] || kind,
      disabled: false,
    }));
    return {
      title: view.budgetTitle,
      disabled: true,
      disabledReason: view.budgetDisabledReason,
      value: selectedKind,
      options,
      showWeight: authored?.kind === "Proportional",
      weightLabel: view.budgetWeightLabel,
      weightValue: authored?.weight || 1,
      showTokenCap: authored?.kind === "Fixed",
      tokenCapLabel: view.budgetTokenCapLabel,
      tokenCapValue: authored?.limit || authored?.value || 4096,
      contractLabel: view.budgetSplitPoliciesContractLabel,
    };
  }

  function agentDeleteConfirmationState(editorState, open = false) {
    const needsConfirmation = !!editorState?.deleteNeedsConfirmation;
    return {
      open: needsConfirmation && !!open,
      needsConfirmation,
      message: String(editorState?.deleteConfirmMessage || ""),
      confirmLabel: String(editorState?.deleteLabel || ""),
      cancelLabel: String(editorState?.deleteCancelLabel || ""),
    };
  }

  function sourceDefinitionRefRows(refs) {
    return normalizeAgentDefinitionRows(refs)
      .map((ref) => {
        const id = String(ref.id || "").trim();
        if (!id) return "";
        const source = String(ref.sourceMobpack || ref.source_mobpack || ref.source || "").trim();
        return source ? `${id} (${source})` : id;
      })
      .filter(Boolean);
  }

  function agentSourceProvenanceState(member, agentDetailView = null) {
    const view = agentDetailViewForState(agentDetailView);
    const source = member?.sourceDefinition && typeof member.sourceDefinition === "object"
      ? member.sourceDefinition
      : null;
    const toolRefs = sourceDefinitionRefRows(source?.toolDefinitions || source?.tool_definitions);
    const skillRefs = sourceDefinitionRefRows(source?.skillDefinitions || source?.skill_definitions);
    const rows = [];
    const push = (label, value) => {
      const text = String(value || "").trim();
      if (label && text) rows.push({ label, value: text });
    };
    push(view.sourceDefinitionLabel, source?.definitionId || source?.definition_id || "");
    push(view.sourceMobpackLabel, source?.sourceMobpackName || source?.source_mobpack_name || source?.sourceMobpack || source?.source_mobpack || "");
    push(view.sourceOriginLabel, source?.sourceOrigin || source?.source_origin || source?.source || "");
    push(view.sourceDocumentPathLabel, source?.sourceDocumentPath || source?.source_document_path || "");
    push(view.sourceSchemaPathLabel, source?.schemaSourceDocumentPath || source?.schema_source_document_path || "");
    push(view.sourceToolsLabel, toolRefs.join(", "));
    push(view.sourceSkillsLabel, skillRefs.join(", "));
    return {
      title: view.sourceTitle,
      emptyHint: view.sourceEmptyHint,
      hasRows: rows.length > 0,
      rows,
    };
  }

  function agentDefinitionOptions(agentDefinitions = []) {
    const definitions = (Array.isArray(agentDefinitions) ? agentDefinitions : [])
      .filter((definition) => definition?.id);
    const labelCounts = definitions.reduce((counts, definition) => {
      const label = String(definition.label || definition.role || definition.id);
      counts.set(label, (counts.get(label) || 0) + 1);
      return counts;
    }, new Map());
    const optionRows = definitions
      .map((definition) => {
        const label = String(definition.label || definition.role || definition.id);
        const sourceLabel = String(definition.sourceMobpackName || definition.sourceMobpack || "").trim();
        return {
          value: definition.id,
          label: labelCounts.get(label) > 1 && sourceLabel ? `${label} · ${sourceLabel}` : label,
          definition,
        };
      });
    return {
      hasDefinitions: optionRows.length > 0,
      optionRows,
    };
  }

  function agentDefinitionAddControlState(agentDefinitions = [], agentView = null) {
    const view = agentViewForState(agentView);
    const definitionState = agentDefinitionOptions(agentDefinitions);
    return {
      ...definitionState,
      controlClass: definitionState.hasDefinitions
        ? "agents-list__add agents-list__add--select"
        : "agents-list__add",
      disabled: !definitionState.hasDefinitions,
      title: definitionState.hasDefinitions
        ? view.addAgentTitle
        : view.addAgentUnavailableTitle,
      unavailableLabel: view.addAgentUnavailableLabel,
      authoringOperationUnavailableError: view.authoringOperationUnavailableError,
      placeholderOption: { value: "", label: view.addAgentPlaceholderLabel },
      value: "",
    };
  }

  function agentDefinitionAddErrorState(result = null, agentView = null) {
    const view = agentViewForState(agentView);
    const error = operationErrorText(result, "");
    const prefix = view.addAgentErrorPrefix
      ? `${view.addAgentErrorPrefix}${/\s$/.test(view.addAgentErrorPrefix) ? "" : " "}`
      : "";
    return {
      hasError: !!error,
      text: error ? `${prefix}${error}` : "",
      rawError: error,
    };
  }

  function agentDefinitionCatalogState(agentDefinitions = [], agentView = null) {
    const view = agentViewForState(agentView);
    const rows = (Array.isArray(agentDefinitions) ? agentDefinitions : [])
      .filter((definition) => definition?.id)
      .map((definition) => {
        const label = String(definition.label || definition.name || definition.role || definition.id).trim();
        const role = String(definition.role || "").trim();
        const source = [
          definition.sourceMobpackName || definition.source_mobpack_name || definition.sourceMobpack || definition.source_mobpack || "",
          definition.sourceOrigin || definition.source_origin || definition.source || "",
        ].map((value) => String(value || "").trim()).filter(Boolean).join(" · ");
        const tools = sourceDefinitionRefRows(definition.toolDefinitions || definition.tool_definitions);
        const skills = sourceDefinitionRefRows(definition.skillDefinitions || definition.skill_definitions);
        return {
          id: String(definition.id || "").trim(),
          title: label,
          role,
          sourceLabel: view.definitionCatalogSourceLabel,
          toolsLabel: view.definitionCatalogToolsLabel,
          skillsLabel: view.definitionCatalogSkillsLabel,
          source,
          tools: tools.join(", "),
          skills: skills.join(", "),
          definition,
        };
      });
    return {
      title: view.definitionCatalogTitle,
      empty: view.definitionCatalogEmpty,
      hasRows: rows.length > 0,
      rows,
    };
  }

  function memberSchemaChangeErrorState(result = null, fallback = "") {
    const error = operationErrorText(result, fallback);
    return {
      hasError: !!error,
      text: error,
      rawError: error,
    };
  }

  function schemaDefinitionAddErrorState(result = null, fallback = "") {
    return memberSchemaChangeErrorState(result, fallback);
  }

  function schemaFieldAddErrorState(result = null, fallback = "") {
    return memberSchemaChangeErrorState(result, fallback);
  }

  function inputParamAddErrorState(result = null, fallback = "") {
    return memberSchemaChangeErrorState(result, fallback);
  }

  function schemaEditorControlState({ schema, members = [], schemaView = null } = {}) {
    const view = schemaViewForState(schemaView);
    const fields = Array.isArray(schema?.fields) ? schema.fields : [];
    const usedBy = (Array.isArray(members) ? members : [])
      .filter((member) => member?.schema === schema?.id)
      .map((member) => ({
        id: member.id,
        name: member.name,
        role: member.role,
        model: member.model,
        selection: { kind: "agent", id: member.id },
        member,
      }));
    const fieldRows = fields.map((field) => ({
      id: field.id,
      field,
    }));
    return {
      eyebrow: view.eyebrow,
      descriptionTitle: view.descriptionTitle,
      descriptionPlaceholder: view.descriptionPlaceholder,
      fieldsTitle: graphTemplateText(view.fieldsTitleTemplate, {
        prefix: view.fieldsTitlePrefix,
        count: fields.length,
      }),
      addFieldLabel: view.addFieldLabel,
      authoringOperationUnavailableError: view.authoringOperationUnavailableError,
      schemaOperationFallbackError: view.schemaOperationFallbackError,
      fieldAddFallbackError: view.fieldAddFallbackError,
      headerLabels: view.headerLabels,
      fieldRows,
      emptyFieldsHint: view.emptyFieldsHint,
      usedBy,
      usedCount: usedBy.length,
      usageLabel: graphTemplateText(
        usedBy.length === 1 ? view.usageSingularTemplate : view.usagePluralTemplate,
        { count: usedBy.length },
      ),
      usedByTitle: graphTemplateText(view.usedByTitleTemplate, {
        prefix: view.usedByPrefix,
        count: usedBy.length,
      }),
      emptyUsedByHint: view.emptyUsedByHint,
      deleteLabel: view.deleteLabel,
      canDelete: usedBy.length === 0,
      deleteTitle: usedBy.length > 0 ? view.deleteBlockedTitle : "",
    };
  }

  function collectFlowMemberSteps(steps, out = []) {
    for (const step of steps || []) {
      if (step?.type === "member") out.push(step);
      for (const lane of childLanes(step || {})) collectFlowMemberSteps(lane.steps, out);
    }
    return out;
  }

  function flowStepUpdatePatch(flow, id, patch = {}, options = {}) {
    let accepted = false;
    const steps = flowStepMap(flow?.steps || [], id, (step) => {
      const nextStep = { ...step, ...(patch && typeof patch === "object" ? patch : {}) };
      const validation = flowStepValidation(nextStep, { flow, members: options.members, currentId: step.id });
      if (!validation.ok) return step;
      accepted = true;
      return nextStep;
    });
    if (!accepted) return flow || {};
    return { ...(flow || {}), steps };
  }

  function flowStepInsertPatch(flow, laneRef, newStep, options = {}) {
    const validation = flowStepValidation(newStep, { flow, members: options.members });
    if (!validation.ok) return flow || {};
    const steps = flowStepInsertIntoLane(flow?.steps || [], laneRef || {}, newStep);
    return { ...(flow || {}), steps };
  }

  function flowStepInsertTransition(flow, laneRef, newStep, options = {}) {
    const validation = flowStepValidation(newStep, { flow, members: options.members });
    if (!validation.ok) {
      return {
        ok: false,
        error: validation.error || "",
        flow: flow || {},
        selection: null,
        picker: { open: false },
      };
    }
    return {
      ok: true,
      error: "",
      flow: flowStepInsertPatch(flow, laneRef, newStep, options),
      selection: newStep.id,
      picker: { open: false },
    };
  }

  function flowStepDeletePatch(flow, id) {
    const target = String(id || "").trim();
    const steps = flowStepRemoveFromTree(flow?.steps || [], target);
    const nextFlow = { ...(flow || {}), steps };
    return target ? reconcileDeletedFlowStepReferences(nextFlow, target) : nextFlow;
  }

  function flowStepDeleteTransition(flow, id) {
    return {
      flow: flowStepDeletePatch(flow, id),
      selection: null,
      picker: { open: false },
    };
  }

  function basicStepPickerOpenTransition(laneRef) {
    return { picker: { open: true, at: laneRef || null } };
  }

  function basicStepPickerCloseTransition() {
    return { picker: { open: false } };
  }

  function basicCanvasClearTransition() {
    return { selection: null, picker: { open: false } };
  }

  function basicStepSelectionTransition(id) {
    const selection = String(id || "").trim() || null;
    return { selection, picker: { open: false } };
  }

  function flowStepTaskPatch(rawTask) {
    return { task: String(rawTask || "") };
  }

  function flowStepInstructionPatch(rawInstruction) {
    return { instruction: String(rawInstruction || "") };
  }

  function flowStepQuorumPatch(rawValue) {
    return { quorum: normalizePositiveInteger(rawValue) };
  }

  function flowStepTimeoutPatch(rawValue) {
    return { timeoutMs: normalizePositiveInteger(rawValue) };
  }

  function flowStepMaxIterationsPatch(rawValue) {
    return { maxIterations: normalizePositiveInteger(rawValue) };
  }

  function flowStepLoopIdPatch(rawLoopId) {
    return { loopId: String(rawLoopId || "").trim() };
  }

  function flowStepRepeatConditionPatch(step, patch = {}) {
    const currentCond = step?.cond && typeof step.cond === "object" && !Array.isArray(step.cond)
      ? step.cond
      : {};
    return { cond: { ...currentCond, ...patch } };
  }

  function basicConditionSourcePatch(conditionOptions, rawStepId, options = {}) {
    const stepId = String(rawStepId || "").trim();
    const rows = Array.isArray(conditionOptions) ? conditionOptions : [];
    if (stepId && !rows.some((candidate) => String(candidate?.stepId || "").trim() === stepId)) {
      return {};
    }
    const selected = rows.find((candidate) => String(candidate?.stepId || "").trim() === stepId);
    const patch = { stepId, field: "" };
    if (options.includeNamespace) {
      patch.namespace = String(selected?.namespace || options.defaultNamespace || "steps").trim();
    }
    return patch;
  }

  function basicConditionFieldPatch(rawField, fieldOptions) {
    const field = String(rawField || "").trim();
    const rows = Array.isArray(fieldOptions) ? fieldOptions : [];
    if (rows.length && field && !rows.some((option) => String(option?.value || option?.field?.name || "").trim() === field)) {
      return {};
    }
    return { field };
  }

  function basicConditionOperatorPatch(rawOperator, contract) {
    const op = String(rawOperator || "").trim();
    if (contract && op && !conditionOperatorOptions(contract, op).some((option) => option.value === op && !option.disabled)) {
      return {};
    }
    return { op };
  }

  function basicConditionValuePatch(rawValue) {
    return { val: rawValue ?? "" };
  }

  function flowStepIterationInputPatch(rawMode, contract) {
    const iterationInput = String(rawMode || "").trim();
    if (!optionValueAllowed(repeatIterationInputOptions(contract, iterationInput), iterationInput, { allowBlank: true })) return {};
    return { iterationInput };
  }

  function memberRoleAllowed(members, rawRole) {
    const role = String(rawRole || "").trim();
    if (!role) return true;
    return memberIdSet(members).has(role);
  }

  function flowStepControllerRolePatch(rawRole, members) {
    const controllerRole = String(rawRole || "").trim();
    return memberRoleAllowed(members, controllerRole) ? { controllerRole } : {};
  }

  function flowStepMemberRolePatch(rawRole, members) {
    const role = String(rawRole || "").trim();
    return memberRoleAllowed(members, role) ? { role } : {};
  }

  function flowStepDispatchModePatch(rawMode, contract) {
    const mode = String(rawMode || "").trim();
    return dispatchModeAllowed(contract, mode) ? { dispatchMode: mode } : {};
  }

  function flowStepParallelDispatchPatch(rawMode, contract) {
    const mode = String(rawMode || "").trim();
    return dispatchModeAllowed(contract, mode) ? { dispatch: mode } : {};
  }

  function flowStepCollectionPatch(rawPolicy, contract) {
    const policy = String(rawPolicy || "").trim();
    return collectionPolicyAllowed(contract, policy) ? { collection: policy } : {};
  }

  function flowStepDependencyModePatch(rawMode, contract) {
    const mode = String(rawMode || "").trim();
    return dependencyModeAllowed(contract, mode) ? { dependsMode: mode } : {};
  }

  function flowStepOutputFormatPatch(rawFormat, contract) {
    const format = normalizeOutputFormat(rawFormat);
    return outputFormatAllowed(contract, format) ? { outputFormat: format } : {};
  }

  function flowStepAllowedToolsPatch(tools, options = {}) {
    return { allowedTools: normalizeStepToolScopeList(tools, { ...options, mode: "member" }) };
  }

  function flowStepBlockedToolsPatch(tools, options = {}) {
    return { blockedTools: normalizeStepToolScopeList(tools, { ...options, mode: "catalog" }) };
  }

  function flowStepValidation(step, { flow, members, currentId = "" } = {}) {
    if (!step || typeof step !== "object") return { ok: false, error: "flow step must be an object" };
    const id = String(step.id || "").trim();
    if (!id) return { ok: false, error: "flow step must include id" };
    const target = String(currentId || "").trim();
    const ids = collectFlowStepIds(flow?.steps || []);
    if (ids.has(id) && (!target || id !== target)) {
      return { ok: false, error: "flow step id already exists" };
    }
    if (target && id !== target) {
      return { ok: false, error: "flow step id changes must use projection reconciliation" };
    }
    if (step.type === "member") {
      const role = String(step.role || "").trim();
      if (!role) return { ok: false, error: "member flow step must reference a member" };
      if (Array.isArray(members) && !memberIdSet(members).has(role)) {
        return { ok: false, error: "member flow step must reference an existing member" };
      }
    }
    return { ok: true, error: "" };
  }

  function flowStepById(steps, id) {
    const target = String(id || "").trim();
    if (!target) return null;
    for (const step of steps || []) {
      if (String(step?.id || "").trim() === target) return step;
      for (const lane of childLanes(step || {})) {
        const found = flowStepById(lane.steps || [], target);
        if (found) return found;
      }
    }
    return null;
  }

  function flowStepMap(steps, id, fn) {
    return (steps || []).map((step) => {
      if (step?.id === id) return fn(step);
      if (step?.type === "branch") {
        return {
          ...step,
          branches: (step.branches || []).map((branch) => ({ ...branch, steps: flowStepMap(branch.steps || [], id, fn) })),
          fallback: flowStepMap(step.fallback || [], id, fn),
        };
      }
      if (step?.type === "parallel") {
        return {
          ...step,
          branches: (step.branches || []).map((branch) => ({ ...branch, steps: flowStepMap(branch.steps || [], id, fn) })),
        };
      }
      if (step?.type === "repeat") return { ...step, steps: flowStepMap(step.steps || [], id, fn) };
      return step;
    });
  }

  function flowStepInsertIntoLane(steps, laneRef, newStep) {
    if (!newStep) return steps || [];
    if (laneRef?.lane === "main") {
      const idx = laneRef.index ?? (steps || []).length;
      return [...(steps || []).slice(0, idx), newStep, ...(steps || []).slice(idx)];
    }
    return (steps || []).map((step) => {
      if (step?.id !== laneRef?.parentId) {
        if (step?.type === "branch") {
          return {
            ...step,
            branches: (step.branches || []).map((branch) => ({ ...branch, steps: flowStepInsertIntoLane(branch.steps || [], laneRef, newStep) })),
            fallback: flowStepInsertIntoLane(step.fallback || [], laneRef, newStep),
          };
        }
        if (step?.type === "parallel") {
          return {
            ...step,
            branches: (step.branches || []).map((branch) => ({ ...branch, steps: flowStepInsertIntoLane(branch.steps || [], laneRef, newStep) })),
          };
        }
        if (step?.type === "repeat") return { ...step, steps: flowStepInsertIntoLane(step.steps || [], laneRef, newStep) };
        return step;
      }
      const at = (arr) => {
        const lane = arr || [];
        const idx = laneRef.index ?? lane.length;
        return [...lane.slice(0, idx), newStep, ...lane.slice(idx)];
      };
      if (laneRef.branchId === "body") return { ...step, steps: at(step.steps) };
      if (laneRef.branchId === "fallback") return { ...step, fallback: at(step.fallback) };
      return {
        ...step,
        branches: (step.branches || []).map((branch) => branch.id === laneRef.branchId ? { ...branch, steps: at(branch.steps) } : branch),
      };
    });
  }

  function flowStepRemoveFromTree(steps, id) {
    return (steps || []).filter((step) => step?.id !== id).map((step) => {
      if (step?.type === "branch") {
        return {
          ...step,
          branches: (step.branches || []).map((branch) => ({ ...branch, steps: flowStepRemoveFromTree(branch.steps || [], id) })),
          fallback: flowStepRemoveFromTree(step.fallback || [], id),
        };
      }
      if (step?.type === "parallel") {
        return {
          ...step,
          branches: (step.branches || []).map((branch) => ({ ...branch, steps: flowStepRemoveFromTree(branch.steps || [], id) })),
        };
      }
      if (step?.type === "repeat") return { ...step, steps: flowStepRemoveFromTree(step.steps || [], id) };
      return step;
    });
  }

  function emptyAuthoringFlowState() {
    return { name: "", steps: [] };
  }

  function directMemberAddValidation(member, members = [], contract = null) {
    if (!member || typeof member !== "object") {
      return { ok: false, error: "member must be an object" };
    }
    const id = String(member.id || "").trim();
    const role = String(member.role || member.name || "").trim();
    const profileBinding = String(member.profileBinding || member.profile_binding || "").trim();
    const runtimeMode = String(member.runtimeMode || member.runtime_mode || "").trim();
    const model = String(member.model || "").trim();
    if (!id || !role) {
      return { ok: false, error: "member must include id and role/name" };
    }
    if ((Array.isArray(members) ? members : []).some((candidate) => String(candidate?.id || "").trim() === id)) {
      return { ok: false, error: "member id already exists" };
    }
    if (!deployableInlineProfileBindingAllowed(contract)) {
      return { ok: false, error: "MobKit schema contract must allow deployable inline profileBinding" };
    }
    if (profileBinding !== "inline") {
      return { ok: false, error: "direct member adds must use an inline deployable profileBinding" };
    }
    if (!runtimeMode) {
      return { ok: false, error: "member must include runtimeMode" };
    }
    if (!contractStringValues(contract?.mob_definition?.runtime_modes).includes(runtimeMode)) {
      return { ok: false, error: "member runtimeMode must be allowed by mob_definition.runtime_modes" };
    }
    if (!model) {
      return { ok: false, error: "inline member definitions must include a model" };
    }
    return { ok: true, error: "" };
  }

  function studioAddMemberPatch({ members, contract } = {}, member) {
    const list = Array.isArray(members) ? members : [];
    const validation = directMemberAddValidation(member, list, contract);
    if (!validation.ok) {
      return { ok: false, error: validation.error, members: list, member: null };
    }
    return { ok: true, error: "", members: [...list, member], member };
  }

  function studioUpdateMemberPatch({ members, contract } = {}, id, patch = {}) {
    const target = String(id || "");
    const list = Array.isArray(members) ? members : [];
    const current = list.find((member) => member?.id === target) || null;
    if (!current) return { ok: false, error: "member not found", members: list };
    const nextMember = { ...current, ...(patch && typeof patch === "object" ? patch : {}) };
    const validation = memberUpdateValidation(current, nextMember, patch, contract);
    if (!validation.ok) {
      return { ok: false, error: validation.error, members: list };
    }
    return {
      ok: true,
      error: "",
      members: list.map((member) => member?.id === target ? nextMember : member),
      member: nextMember,
    };
  }

  function memberUpdateValidation(current, nextMember, patch = {}, contract = null) {
    const touched = patch && typeof patch === "object" ? new Set(Object.keys(patch)) : new Set();
    const exportCritical = [
      "id",
      "profileBinding",
      "profile_binding",
      "runtimeMode",
      "runtime_mode",
      "model",
      "realmProfile",
      "realm_profile",
      "backend",
      "schema",
      "tools",
      "skills",
      "providerParams",
      "provider_params",
      "maxInlinePeerNotifications",
      "max_inline_peer_notifications",
    ];
    if (!exportCritical.some((key) => touched.has(key))) return { ok: true, error: "" };
    if (touched.has("id") && String(nextMember.id || "").trim() !== String(current?.id || "").trim()) {
      return { ok: false, error: "member id changes must use projection reconciliation" };
    }
    const binding = String(nextMember.profileBinding || nextMember.profile_binding || "").trim();
    const runtimeMode = String(nextMember.runtimeMode || nextMember.runtime_mode || "").trim();
    const model = String(nextMember.model || "").trim();
    if ((touched.has("profileBinding") || touched.has("profile_binding") || touched.has("runtimeMode") || touched.has("runtime_mode"))
      && !deployableInlineProfileBindingAllowed(contract)) {
      return { ok: false, error: "MobKit schema contract must allow deployable inline profileBinding" };
    }
    if (binding && binding !== "inline") {
      return { ok: false, error: "member updates must keep deployable inline profileBinding" };
    }
    if (!binding && (touched.has("profileBinding") || touched.has("profile_binding"))) {
      return { ok: false, error: "member updates must keep profileBinding explicit" };
    }
    if (!runtimeMode && (touched.has("runtimeMode") || touched.has("runtime_mode"))) {
      return { ok: false, error: "member updates must keep runtimeMode explicit" };
    }
    if ((touched.has("runtimeMode") || touched.has("runtime_mode"))
      && !contractStringValues(contract?.mob_definition?.runtime_modes).includes(runtimeMode)) {
      return { ok: false, error: "member updates must use a mob_definition.runtime_modes value" };
    }
    if ((binding || current?.profileBinding === "inline" || current?.profile_binding === "inline") && !model && touched.has("model")) {
      return { ok: false, error: "inline member updates must keep a model" };
    }
    if (touched.has("schema") && typeof nextMember.schema !== "string") {
      return { ok: false, error: "member schema must be a string reference" };
    }
    if (touched.has("tools") && !stringListPatchValueIsValid(nextMember.tools)) {
      return { ok: false, error: "member tools must be an array of non-empty strings" };
    }
    if (touched.has("skills") && !stringListPatchValueIsValid(nextMember.skills)) {
      return { ok: false, error: "member skills must be an array of non-empty strings" };
    }
    if ((touched.has("providerParams") || touched.has("provider_params")) && !providerParamsPatchValueIsValid(nextMember.providerParams ?? nextMember.provider_params)) {
      return { ok: false, error: "member providerParams must be a JSON object" };
    }
    if ((touched.has("maxInlinePeerNotifications") || touched.has("max_inline_peer_notifications")) && !maxInlinePeerNotificationsPatchValueIsValid(nextMember.maxInlinePeerNotifications ?? nextMember.max_inline_peer_notifications)) {
      return { ok: false, error: "member maxInlinePeerNotifications must be an integer >= -1 or blank" };
    }
    return { ok: true, error: "" };
  }

  function deployableInlineProfileBindingAllowed(contract) {
    const bindings = contractStringValues(contract?.mob_definition?.profile_binding);
    const restriction = profileBindingRestriction(contract, "inline");
    return bindings.includes("inline") && restriction.deployable !== false;
  }

  function stringListPatchValueIsValid(value) {
    if (!Array.isArray(value)) return false;
    return value.every((item) => typeof item === "string" && !!item.trim());
  }

  function providerParamsPatchValueIsValid(value) {
    if (value === null || value === undefined) return true;
    return typeof value === "object" && !Array.isArray(value);
  }

  function maxInlinePeerNotificationsPatchValueIsValid(value) {
    if (value === null || value === undefined || value === "") return true;
    const number = typeof value === "number" ? value : Number(value);
    return Number.isInteger(number) && number >= -1;
  }

  function studioDeleteMemberPatch({ members, instances, edges } = {}, id) {
    const target = String(id || "");
    const nextMembers = (members || []).filter((member) => member?.id !== target);
    const nextInstances = (instances || []).filter((instance) => instance?.memberId !== target);
    const remainingInstanceIds = new Set(nextInstances.map((instance) => instance?.id).filter(Boolean));
    const nextEdges = (edges || []).filter((edge) => remainingInstanceIds.has(edge?.from) && remainingInstanceIds.has(edge?.to));
    return { members: nextMembers, instances: nextInstances, edges: nextEdges };
  }

  function memberUpdateCascadePatch({ memberId, members, flow, instances, edges, mobSettings, contract } = {}, patch = {}) {
    const sourceMembers = Array.isArray(members) ? members : [];
    const sourceInstances = Array.isArray(instances) ? instances : [];
    const sourceEdges = Array.isArray(edges) ? edges : [];
    const normalizedMobSettings = normalizeMobSettings(mobSettings);
    const updated = studioUpdateMemberPatch({ members: sourceMembers, contract }, memberId, patch);
    if (!updated.ok) {
      return {
        ok: false,
        error: updated.error || "",
        patch: null,
        member: null,
        members: sourceMembers,
        flow,
        instances: sourceInstances,
        edges: sourceEdges,
        mobSettings: normalizedMobSettings,
      };
    }
    const reconciled = reconcileAuthoringForMembers({
      flow,
      instances: sourceInstances,
      edges: sourceEdges,
      mobSettings: normalizedMobSettings,
      previousMembers: sourceMembers,
      members: updated.members,
    });
    return {
      ok: true,
      error: "",
      patch,
      member: updated.member,
      members: updated.members,
      flow: reconciled.flow,
      instances: reconciled.instances,
      edges: reconciled.edges,
      mobSettings: reconciled.mobSettings,
    };
  }

  function memberDeleteCascadePatch({ memberId, members, instances, edges, flow, mobSettings } = {}) {
    const target = String(memberId || "").trim();
    const sourceMembers = Array.isArray(members) ? members : [];
    const current = sourceMembers.find((member) => String(member?.id || "").trim() === target) || null;
    if (!current) {
      return {
        ok: false,
        error: "member not found",
        members: sourceMembers,
        instances: Array.isArray(instances) ? instances : [],
        edges: Array.isArray(edges) ? edges : [],
        flow,
        mobSettings: normalizeMobSettings(mobSettings),
        selection: null,
      };
    }
    const graphDeleted = studioDeleteMemberPatch({ members: sourceMembers, instances, edges }, target);
    let nextFlow = reconcileFlowMemberSteps(flow, graphDeleted.members);
    nextFlow = reconcileFlowMemberSchemas(nextFlow, graphDeleted.members);
    nextFlow = reconcileFlowControlRoles(nextFlow, graphDeleted.members);
    nextFlow = reconcileFlowLaunchSources(nextFlow, graphDeleted.members);
    nextFlow = reconcileFlowStepToolScopes(nextFlow, graphDeleted.members);
    let nextInstances = reconcileGraphControlRoles(graphDeleted.instances, graphDeleted.members);
    nextInstances = reconcileGraphLaunchSources(nextInstances, graphDeleted.members);
    nextInstances = reconcileGraphStepToolScopes(nextInstances, graphDeleted.members);
    const nextMobSettings = reconcileMobSettingsProfiles(mobSettings, sourceMembers, graphDeleted.members);
    return {
      ok: true,
      error: "",
      removed: current,
      members: graphDeleted.members,
      instances: nextInstances,
      edges: graphDeleted.edges,
      flow: nextFlow,
      mobSettings: nextMobSettings,
      selection: null,
    };
  }

  function graphInstanceValidation(instance, { instances, members, currentId = "" } = {}) {
    if (!instance || typeof instance !== "object") return { ok: false, error: "graph node must be an object" };
    const id = String(instance.id || "").trim();
    if (!id) return { ok: false, error: "graph node must include id" };
    const target = String(currentId || "").trim();
    const duplicate = (Array.isArray(instances) ? instances : []).some((candidate) => {
      const candidateId = String(candidate?.id || "").trim();
      return candidateId && candidateId === id && (!target || candidateId !== target);
    });
    if (duplicate) return { ok: false, error: "graph node id already exists" };
    const isControlNode = !!instance.isGate || !!instance.isTerminal || !!String(instance.gateKind || "").trim();
    const memberId = String(instance.memberId || "").trim();
    if (!isControlNode) {
      if (!memberId) return { ok: false, error: "member graph node must reference a member" };
      if (!memberIdSet(members).has(memberId)) return { ok: false, error: "member graph node must reference an existing member" };
    }
    return { ok: true, error: "" };
  }

  function studioUpdateInstancePatch({ instances, members } = {}, id, patch = {}) {
    const target = String(id || "");
    const list = Array.isArray(instances) ? instances : [];
    const current = list.find((instance) => instance?.id === target) || null;
    if (!current) return { ok: false, error: "graph node not found", instances: list };
    const nextInstance = { ...current, ...(patch && typeof patch === "object" ? patch : {}) };
    const validation = graphInstanceValidation(nextInstance, { instances: list, members, currentId: target });
    if (!validation.ok) return { ok: false, error: validation.error, instances: list };
    return {
      ok: true,
      error: "",
      instances: list.map((instance) => instance?.id === target ? nextInstance : instance),
      instance: nextInstance,
    };
  }

  function studioMoveInstancePatch({ instances } = {}, id, cell, originalCell = {}) {
    const target = String(id || "");
    const nextCol = Number.isInteger(cell?.col) ? cell.col : null;
    const nextRow = Number.isInteger(cell?.row) ? cell.row : null;
    if (!target || nextCol === null || nextRow === null) return { instances: instances || [] };
    const sourceInstances = instances || [];
    const moving = sourceInstances.find((instance) => instance?.id === target);
    if (!moving) return { instances: sourceInstances };
    const occupant = sourceInstances.find((instance) =>
      instance?.id !== target && instance?.col === nextCol && instance?.row === nextRow
    );
    const originalCol = Number.isInteger(originalCell?.col) ? originalCell.col : moving.col;
    const originalRow = Number.isInteger(originalCell?.row) ? originalCell.row : moving.row;
    return {
      instances: sourceInstances.map((instance) => {
        if (instance?.id === target) return { ...instance, col: nextCol, row: nextRow };
        if (occupant && instance?.id === occupant.id) return { ...instance, col: originalCol, row: originalRow };
        return instance;
      }),
    };
  }

  function studioDeleteInstancePatch({ instances, edges } = {}, id) {
    const target = String(id || "");
    const nextInstances = (instances || [])
      .filter((instance) => instance?.id !== target)
      .map((instance) => clearDeletedLaunchSource(instance, target));
    const nextEdges = clearDeletedGraphConditionEdges(
      (edges || []).filter((edge) => edge?.from !== target && edge?.to !== target),
      target,
    );
    return {
      instances: nextInstances,
      edges: nextEdges,
      selection: { kind: null, id: null },
    };
  }

  function clearDeletedGraphConditionEdges(edges, deletedId) {
    const target = String(deletedId || "").trim();
    if (!target) return edges || [];
    let changed = false;
    const next = (edges || []).map((edge) => {
      const condition = normalizedEdgeCondition(edge);
      const parts = String(condition?.path || "").split(".").filter(Boolean);
      if (parts.length !== 3 || parts[0] !== "steps" || parts[1] !== target) return edge;
      changed = true;
      return { ...edge, cond: null, label: "" };
    });
    return changed ? next : edges;
  }

  function graphEdgeValidation(edge, { instances, edges, currentId = "" } = {}) {
    if (!edge || typeof edge !== "object") return { ok: false, error: "edge must be an object" };
    const id = String(edge.id || "").trim();
    const from = String(edge.from || "").trim();
    const to = String(edge.to || "").trim();
    if (!id || !from || !to) return { ok: false, error: "edge must include id, from, and to" };
    if (from === to) return { ok: false, error: "edge endpoints must be different graph nodes" };
    const instanceIds = graphInstanceIdSet(instances);
    if (!instanceIds.has(from) || !instanceIds.has(to)) {
      return { ok: false, error: "edge endpoints must reference existing graph nodes" };
    }
    const target = String(currentId || "").trim();
    const duplicate = (Array.isArray(edges) ? edges : []).some((candidate) => {
      const candidateId = String(candidate?.id || "").trim();
      if (target && candidateId === target) return false;
      return candidateId === id || (candidate?.from === from && candidate?.to === to);
    });
    if (duplicate) return { ok: false, error: "edge already exists" };
    return { ok: true, error: "" };
  }

  function studioUpdateEdgePatch({ edges, instances } = {}, id, patch = {}) {
    const target = String(id || "");
    const list = Array.isArray(edges) ? edges : [];
    const current = list.find((edge) => edge?.id === target) || null;
    if (!current) return { ok: false, error: "edge not found", edges: list };
    const nextEdge = { ...current, ...(patch && typeof patch === "object" ? patch : {}) };
    const validation = graphEdgeValidation(nextEdge, { instances, edges: list, currentId: target });
    if (!validation.ok) return { ok: false, error: validation.error, edges: list };
    return {
      ok: true,
      error: "",
      edges: list.map((edge) => edge?.id === target ? nextEdge : edge),
      edge: nextEdge,
    };
  }

  function studioDeleteEdgePatch({ edges } = {}, id) {
    const target = String(id || "");
    return {
      edges: (edges || []).filter((edge) => edge?.id !== target),
      selection: { kind: null, id: null },
    };
  }

  function studioAddSchemaPatch({ schemas } = {}, schema) {
    const list = Array.isArray(schemas) ? schemas : [];
    const id = String(schema?.id || "").trim();
    if (!id) return { ok: false, error: "schema must include id", schemas: list, schema: null };
    if (list.some((candidate) => String(candidate?.id || "").trim() === id)) {
      return { ok: false, error: "schema id already exists", schemas: list, schema: null };
    }
    return { ok: true, error: "", schemas: [...list, schema], schema };
  }

  function studioUpdateSchemaPatch({ schemas } = {}, id, patch = {}) {
    const target = String(id || "");
    const list = Array.isArray(schemas) ? schemas : [];
    const current = list.find((schema) => schema?.id === target) || null;
    if (!current) return { ok: false, error: "schema not found", schemas: list };
    if (patch && typeof patch === "object" && Object.prototype.hasOwnProperty.call(patch, "id")) {
      const nextId = String(patch.id || "").trim();
      if (nextId !== target) {
        return { ok: false, error: "schema id changes must use renameSchemaDefinition", schemas: list };
      }
    }
    const nextSchema = { ...current, ...(patch && typeof patch === "object" ? patch : {}) };
    return {
      ok: true,
      error: "",
      schemas: list.map((schema) => schema?.id === target ? nextSchema : schema),
      schema: nextSchema,
    };
  }

  function studioDeleteSchemaPatch({ schemas, members, flow, edges, instances } = {}, id) {
    const target = String(id || "");
    const nextSchemas = (schemas || []).filter((schema) => schema?.id !== target);
    const nextMembers = (members || []).map((member) => member?.schema === target ? { ...member, schema: "" } : member);
    const result = {
      schemas: nextSchemas,
      members: nextMembers,
      selection: null,
    };
    if (flow || edges) {
      const reconciled = reconcileConditionFieldAvailability({
        flow,
        edges,
        members: nextMembers,
        instances,
        schemas: nextSchemas,
      });
      if (flow) result.flow = reconciled.flow;
      if (edges) result.edges = reconciled.edges;
    }
    return result;
  }

  function studioSnapshotState(state = {}) {
    return {
      members: Array.isArray(state.members) ? state.members : [],
      instances: Array.isArray(state.instances) ? state.instances : [],
      edges: Array.isArray(state.edges) ? state.edges : [],
      frames: Array.isArray(state.frames) ? state.frames : [],
      schemas: Array.isArray(state.schemas) ? state.schemas : [],
      skillRealms: Array.isArray(state.skillRealms) ? state.skillRealms : [],
    };
  }

  function studioHistorySnapshotPatch({ history, future, state } = {}) {
    const currentHistory = Array.isArray(history) ? history : [];
    return {
      history: [...currentHistory.slice(-30), studioSnapshotState(state)],
      future: [],
    };
  }

  function studioUndoPatch({ history, future, state } = {}) {
    const currentHistory = Array.isArray(history) ? history : [];
    if (!currentHistory.length) return null;
    const previous = studioSnapshotState(currentHistory[currentHistory.length - 1]);
    return {
      state: previous,
      history: currentHistory.slice(0, -1),
      future: [...(Array.isArray(future) ? future : []), studioSnapshotState(state)],
    };
  }

  function studioRedoPatch({ history, future, state } = {}) {
    const currentFuture = Array.isArray(future) ? future : [];
    if (!currentFuture.length) return null;
    const next = studioSnapshotState(currentFuture[currentFuture.length - 1]);
    return {
      state: next,
      history: [...(Array.isArray(history) ? history : []), studioSnapshotState(state)],
      future: currentFuture.slice(0, -1),
    };
  }

  function parseLegacyInputFields(text) {
    return String(text || "")
      .split(/\n/)
      .flatMap(splitLegacyInputFieldLine)
      .map((line) => line.trim())
      .filter(Boolean)
      .map((line, index) => {
        const match = /^([A-Za-z_][A-Za-z0-9_]*)\s*:\s*(enum\(([^)]+)\)|[A-Za-z0-9_[\]]+)/.exec(line);
        if (!match) return null;
        const enumValues = match[3]
          ? match[3].split("|").join(",").split(",").map((value) => value.trim()).filter(Boolean)
          : [];
        return {
          id: `p${index + 1}`,
          name: match[1],
          type: enumValues.length ? "enum" : match[2],
          required: true,
          description: "",
          enumValues,
        };
      })
      .filter(Boolean);
  }

  function splitLegacyInputFieldLine(line) {
    const out = [];
    let start = 0;
    let depth = 0;
    const raw = String(line || "");
    for (let index = 0; index < raw.length; index += 1) {
      const char = raw[index];
      if (char === "(") depth += 1;
      if (char === ")" && depth > 0) depth -= 1;
      if (char === "," && depth === 0) {
        out.push(raw.slice(start, index));
        start = index + 1;
      }
    }
    out.push(raw.slice(start));
    return out;
  }

  function inputParamsForStep(step) {
    if (Array.isArray(step?.inputParams)) return step.inputParams;
    return parseLegacyInputFields(step?.fields);
  }

  function inputParamSummary(params, contract) {
    const defaultType = contractDefaultValue(contract, "schema_field_type");
    return (params || [])
      .map((param) => `${param.name}: ${param.type || defaultType}${param.required ? "" : "?"}`)
      .join(", ");
  }

  function inputParamOptions(flow, basicView = null) {
    const input = (flow?.steps || []).find((step) => step.type === "input");
    const fields = inputParamsForStep(input);
    if (!fields.length) return [];
    const view = basicEditorViewState(basicView);
    return [{
      stepId: "params",
      namespace: "params",
      label: view.inputParamSourceLabel,
      fields,
    }];
  }

  function basicInputControlState(step, contract, basicView = null) {
    const params = inputParamsForStep(step);
    const view = basicEditorViewState(basicView);
    return {
      panelIcon: view.inputPanelIcon,
      panelTitle: view.inputPanelTitle,
      panelSub: view.inputPanelSub,
      taskLabel: view.inputTaskLabel,
      taskPlaceholder: view.inputTaskPlaceholder,
      params,
      paramsTitle: `${view.inputParamsTitlePrefix} · ${params.length}`,
      addParamLabel: view.inputAddParamLabel,
      headerRows: [
        { key: "name", label: view.inputParamHeaderLabels.name, className: "sb-col sb-col--name" },
        { key: "type", label: view.inputParamHeaderLabels.type, className: "sb-col sb-col--type" },
        { key: "required", label: view.inputParamHeaderLabels.required, className: "sb-col sb-col--req" },
        { key: "description", label: view.inputParamHeaderLabels.description, className: "sb-col sb-col--desc" },
        { key: "actions", label: view.inputParamHeaderLabels.action, className: "sb-col sb-col--act" },
      ],
      emptyParamsParts: view.inputEmptyParamsParts,
      tips: view.inputTips,
    };
  }

  function basicConditionOptions(flow, targetId, members, basicView = null) {
    return [
      ...inputParamOptions(flow, basicView),
      ...memberConditionOptionsBefore(flow?.steps || [], targetId, members).out,
    ];
  }

  function memberConditionOptionsBefore(steps, targetId, members, out = []) {
    const memberById = new Map((Array.isArray(members) ? members : [])
      .filter((member) => member?.id)
      .map((member) => [member.id, member]));
    return memberConditionOptionsBeforeWithMap(steps, targetId, memberById, out);
  }

  function memberConditionOptionsBeforeWithMap(steps, targetId, memberById, out = []) {
    const target = String(targetId || "");
    for (const step of steps || []) {
      if (step?.id === target) return { found: true, out };
      if (step?.type === "member" && step.role) {
        const member = memberById.get(step.role);
        if (member) {
          out.push({
            stepId: step.id,
            namespace: "steps",
            member,
            label: member.name || member.role || member.id || step.id,
            fields: [],
          });
        }
      }
      for (const lane of childLanes(step)) {
        const result = memberConditionOptionsBeforeWithMap(lane.steps, target, memberById, [...out]);
        if (result.found) return result;
      }
    }
    return { found: false, out };
  }

  function parseGraphConditionVar(value) {
    const text = String(value || "").trim();
    const params = /^params\.([A-Za-z0-9_.-]+)$/.exec(text);
    if (params) return { instanceId: "params", field: params[1], namespace: "params" };
    const match = /^steps\.([A-Za-z0-9_.-]+)\.([A-Za-z0-9_.-]+)$/.exec(text)
      || /^([A-Za-z0-9_.-]+)\.([A-Za-z0-9_.-]+)$/.exec(text);
    if (!match) return { instanceId: "", field: "", namespace: "" };
    return { instanceId: match[1], field: match[2], namespace: "steps" };
  }

  function graphConditionRefForEdge(edge) {
    const condition = normalizedEdgeCondition(edge);
    return parseGraphConditionVar(condition?.path || "");
  }

  function graphConditionOptions({ instances, members, schemas, edge, flow, graphView = null } = {}) {
    const view = graphCanvasViewState(graphView);
    const graphInstances = Array.isArray(instances) ? instances : [];
    const byId = new Map(graphInstances.map((inst) => [inst.id, inst]));
    const memberById = new Map((Array.isArray(members) ? members : []).map((member) => [member.id, member]));
    const schemaById = new Map((Array.isArray(schemas) ? schemas : []).map((schema) => [schema.id, schema]));
    const from = byId.get(edge?.from);
    const to = byId.get(edge?.to);
    const fromCol = Number(from?.col ?? 0);
    const toCol = Number(to?.col ?? fromCol);
    const limitCol = Math.max(fromCol, toCol);
    const condRef = graphConditionRefForEdge(edge);
    const params = inputParamsForStep((flow?.steps || []).find((step) => step.type === "input"));
    const paramFields = params.length ? params : (condRef.namespace === "params" && condRef.field
      ? [{ id: condRef.field, name: condRef.field, type: "string" }]
      : []);
    const options = graphInstances
      .filter((inst) => inst.memberId && !inst.isGate && !inst.isTerminal)
      .filter((inst) => inst.id !== to?.id)
      .filter((inst) => inst.id === from?.id || Number(inst.col ?? 0) <= limitCol)
      .map((inst) => {
        const member = memberById.get(inst.memberId);
        const schema = member?.schema ? schemaById.get(member.schema) || null : null;
        return { inst, member, schema, fields: schema?.fields || [] };
      })
      .filter((option) => option.member && option.fields.length > 0)
      .sort((a, b) => (Number(a.inst.col || 0) - Number(b.inst.col || 0)) || (Number(a.inst.row || 0) - Number(b.inst.row || 0)));
    if (paramFields.length) {
      options.unshift({
        inst: { id: "params" },
        member: { name: view.graphInputParamSourceLabel },
        schema: { id: "params" },
        fields: paramFields,
        isParams: true,
      });
    }
    return options;
  }

  function inputParamUpdatePatch(params, id, patch, contract) {
    const source = Array.isArray(params) ? params : [];
    const current = source.find((param) => param?.id === id) || null;
    if (!current) return { inputParams: source, fields: inputParamSummary(source, contract) };
    const normalized = normalizeSchemaLikeFieldPatch(current, patch, contract);
    if (Object.prototype.hasOwnProperty.call(normalized, "name")) {
      normalized.name = uniqueInputParamName(source, normalized.name, id, editorInputParamNameFallback(contract));
    }
    const next = source.map((param) => param?.id === id ? { ...param, ...normalized } : param);
    return { inputParams: next, fields: inputParamSummary(next, contract) };
  }

  function inputParamDeletePatch(params, id, contract) {
    const removed = (params || []).find((param) => param?.id === id) || null;
    const next = (params || []).filter((param) => param?.id !== id);
    return { removed, patch: { inputParams: next, fields: inputParamSummary(next, contract) } };
  }

  function inputParamRenamePatch(params, id, rawName, contract) {
    const nextName = uniqueInputParamName(params, rawName, id, editorInputParamNameFallback(contract));
    const next = (params || []).map((param) => param?.id === id ? { ...param, name: nextName } : param);
    return { name: nextName, patch: { inputParams: next, fields: inputParamSummary(next, contract) } };
  }

  function basicConditionFromText(text) {
    return parseEditorConditionText(text);
  }

  function basicConditionText(cond, options = {}) {
    if (!cond || !cond.stepId || !cond.field) return "";
    const op = cond.op || cond.operator || options.defaultOperator || "";
    if (!op) return "";
    if (cond.namespace === "params" || cond.stepId === "params") {
      return `params.${cond.field} ${op} ${conditionValueLiteral(cond.val ?? cond.value ?? "")}`;
    }
    return `steps.${cond.stepId}.${cond.field} ${op} ${conditionValueLiteral(cond.val ?? cond.value ?? "")}`;
  }

  function basicBranchConditionPatch(step, branchId, patch = {}, contract) {
    const branches = (step?.branches || []).map((branch) => {
      if (branch?.id !== branchId) return branch;
      const defaultOperator = contractDefaultValue(contract, "condition_operator");
      const cond = {
        ...(branch.cond || basicConditionFromText(branch.condition) || {}),
        ...patch,
      };
      return {
        ...branch,
        cond,
        condition: basicConditionText(cond, { defaultOperator }),
      };
    });
    return { branches };
  }

  function basicBranchAddPatch(step, options = {}) {
    const branches = Array.isArray(step?.branches) ? step.branches : [];
    const branchIds = collectFlowBranchIds(options.flow?.steps || []);
    for (const branch of branches) {
      const id = String(branch?.id || "").trim();
      if (id) branchIds.add(id);
    }
    const nextBranch = {
      id: reserveFlowBranchId("br", branchIds),
      label: basicBranchDefaultLabel(branches.length + 1, options.basicView),
      steps: [],
    };
    if (step?.type !== "parallel") nextBranch.condition = "";
    return { branches: [...branches, nextBranch] };
  }

  function basicConditionLabel(cond, options = [], config = {}) {
    if (!cond || !cond.stepId || !cond.field) return String(config.previewFallback || "");
    const option = (Array.isArray(options) ? options : []).find((candidate) => candidate.stepId === cond.stepId);
    const label = option?.label || option?.member?.name || cond.stepId;
    const op = cond.op || cond.operator || config.defaultOperator || "";
    return `${label}.${cond.field} ${op} ${conditionValueLiteral(cond.val ?? cond.value ?? "")}`;
  }

  function basicBranchConditionControlState({ branch, options = [], schemas = [], contract, basicView = null } = {}) {
    const view = basicEditorViewState(basicView);
    const sourceOptions = Array.isArray(options) ? options : [];
    const sourceSchemas = Array.isArray(schemas) ? schemas : [];
    const defaultOperator = contractDefaultValue(contract, "condition_operator");
    const cond = branch?.cond || basicConditionFromText(branch?.condition) || {};
    const selected = sourceOptions.find((option) => option.stepId === cond.stepId) || null;
    const fields = selected?.namespace === "params"
      ? selected.fields || []
      : ((sourceSchemas.find((candidate) => candidate.id === selected?.member?.schema)?.fields) || []);
    const field = fields.find((candidate) => candidate.name === cond.field) || null;
    const operatorValue = cond.op || defaultOperator;
    return {
      cond,
      selected,
      fields,
      field,
      sourceOptions: sourceOptions.map((option) => ({
        value: option.stepId,
        label: option.label || option.member?.name || option.stepId,
        option,
      })),
      fieldOptions: fields.map((candidate) => ({
        value: candidate.name,
        label: `${candidate.name} · ${candidate.type}`,
        field: candidate,
      })),
      rowTitle: `${view.branchConditionRowTitlePrefix} ${Number.isFinite(Number(branch?.index)) ? Number(branch.index) + 1 : ""}`.trim(),
      emptyHint: view.branchConditionEmptyHint,
      sourcePlaceholder: view.branchConditionSourcePlaceholder,
      fieldPlaceholder: fields.length ? view.branchConditionFieldPlaceholder : view.branchConditionNoSchemaLabel,
      defaultOperator,
      operatorValue,
      operatorOptions: conditionOperatorOptions(contract, operatorValue),
      previewPrefix: view.branchConditionPreviewPrefix,
      previewLabel: basicConditionLabel(cond, sourceOptions, {
        defaultOperator,
        previewFallback: view.branchConditionPreviewFallback,
      }),
      hasConditionOptions: sourceOptions.length > 0,
    };
  }

  function basicBranchParallelControlState({ step, flow, members = [], contract, basicView = null } = {}) {
    const view = basicEditorViewState(basicView);
    const sourceMembers = Array.isArray(members) ? members : [];
    const isParallel = step?.type === "parallel";
    const controllerRole = step?.controllerRole || "";
    const defaultDispatch = contractDefaultValue(contract, "dispatch_mode");
    const dispatchValue = step?.dispatch || defaultDispatch;
    const defaultCollection = contractDefaultValue(contract, "collection_policy");
    const collectionValue = step?.collection || defaultCollection;
    const defaultDependency = contractDefaultValue(contract, "dependency_mode");
    const dependencyValue = step?.dependsMode || defaultDependency;
    const dispatchOptions = dispatchModeOptions(contract, dispatchValue);
    const collectionOptions = collectionPolicyOptions(contract, collectionValue);
    const dependencyOptions = dependencyModeOptions(contract, dependencyValue);
    return {
      isParallel,
      panelIcon: isParallel ? "‖" : "⑂",
      panelTitle: isParallel ? view.parallelPanelTitle : view.branchPanelTitle,
      panelSub: isParallel ? view.parallelPanelSub : view.branchPanelSub,
      controllerLabel: isParallel ? view.parallelJoinMemberLabel : view.branchRouteMemberLabel,
      controllerPlaceholderLabel: view.branchControllerPlaceholderLabel,
      controllerRole,
      memberOptions: sourceMembers.map((member) => ({
        value: member.id,
        label: `${member.name || member.role || member.id} · ${member.role || "profile"}`,
        member,
      })),
      emptyControllerHint: view.branchEmptyControllerHint,
      conditionOptions: basicConditionOptions(flow, step?.id, sourceMembers, basicView),
      branchConditionTitle: view.branchConditionTitle,
      branchConditionIntro: view.branchConditionIntro,
      fallbackTitle: view.branchFallbackTitle,
      fallbackHint: view.branchFallbackHint,
      addBranchLabel: isParallel ? view.addParallelBranchLabel : view.addBranchLabel,
      dispatchLabel: view.parallelDispatchLabel,
      dispatchValue,
      dispatchOptions,
      selectedDispatch: dispatchOptions.find((option) => option.value === dispatchValue) || null,
      collectionLabel: view.parallelCollectionLabel,
      collectionValue,
      collectionOptions,
      selectedCollection: collectionOptions.find((option) => option.value === collectionValue) || null,
      showQuorum: collectionValue === "quorum",
      quorumLabel: view.parallelQuorumLabel,
      quorumPlaceholder: view.parallelQuorumPlaceholder,
      dependencyLabel: view.branchDependencyLabel,
      dependencyValue,
      dependencyOptions,
      selectedDependency: dependencyOptions.find((option) => option.value === dependencyValue) || null,
    };
  }

  function basicForkCanvasState({ step, contract, basicView = null } = {}) {
    const view = basicEditorViewState(basicView);
    const isParallel = step?.type === "parallel";
    const collection = step?.collection || contractDefaultValue(contract, "collection_policy");
    const branches = Array.isArray(step?.branches) ? step.branches : [];
    const lanes = [
      ...branches.map((branch) => ({ id: branch.id, label: branch.label, steps: branch.steps || [] })),
      ...(isParallel ? [] : [{ id: "fallback", label: view.branchFallbackTitle, steps: step?.fallback || [] }]),
    ];
    return {
      isParallel,
      className: "bld-fork" + (isParallel ? " bld-fork--parallel" : ""),
      lanes,
      showRail: lanes.length > 1,
      showJoin: isParallel,
      joinLabel: `⋈ join · ${collection || "—"}`,
    };
  }

  function basicRepeatIterationLabel(step, members = [], basicView = null) {
    const view = basicEditorViewState(basicView);
    const iterationInput = typeof step?.iterationInput === "string" ? step.iterationInput.trim() : "";
    if (!iterationInput) return view.repeatIterationRuntimeDefaultLabel;
    if (iterationInput === "carry") return view.repeatIterationCarryLabel;
    if (iterationInput === "reuse") return view.repeatIterationReuseUnsupportedLabel;
    const bodyStep = (Array.isArray(step?.steps) ? step.steps : []).find((candidate) => candidate?.id === iterationInput);
    const member = (Array.isArray(members) ? members : []).find((candidate) => candidate?.id === bodyStep?.role);
    return member
      ? `${view.repeatIterationFeedsUnsupportedPrefix}${member.name}'s output`
      : `${view.repeatIterationUnsupportedPrefix}${iterationInput}`;
  }

  function basicRepeatCanvasState({ step, members = [], contract, basicView = null } = {}) {
    const view = basicEditorViewState(basicView);
    const defaultOperator = contractDefaultValue(contract, "condition_operator");
    const repeatUntilExpression = basicRepeatUntilExpression(step, members, { defaultOperator });
    return {
      repeatUntilExpression,
      whileLabel: view.repeatCanvasWhileLabel,
      notLabel: view.repeatCanvasNotLabel,
      conditionLabel: repeatUntilExpression || view.repeatPreviewFallback,
      maxIterationsLabel: step?.maxIterations
        ? `${view.repeatCanvasMaxIterationsPrefix}${step.maxIterations}`
        : view.repeatCanvasMissingMaxIterationsLabel,
      loopBackLabel: `${view.repeatCanvasLoopBackPrefix}${basicRepeatIterationLabel(step, members, basicView)}`,
      exitLabel: `${view.repeatCanvasExitPrefix}${repeatUntilExpression || view.repeatCanvasExitFallback}`,
    };
  }

  function basicStepCardState({ step, members = [], contract, basicView = null } = {}) {
    const view = basicEditorViewState(basicView);
    const sourceMembers = Array.isArray(members) ? members : [];
    const member = step?.role ? sourceMembers.find((candidate) => candidate?.id === step.role) || null : null;
    if (step?.type === "input") {
      return {
        icon: "▤",
        iconTint: "member",
        title: view.inputStepCardTitle,
        desc: step?.task ? step.task : view.inputStepCardDescFallback,
        configured: true,
        isFlowCard: false,
      };
    }
    if (step?.type === "branch") {
      return {
        icon: "⑂",
        iconTint: "member",
        title: view.branchStepCardTitle,
        desc: view.branchStepCardDesc,
        configured: true,
        isFlowCard: true,
      };
    }
    if (step?.type === "parallel") {
      const collection = step?.collection || contractDefaultValue(contract, "collection_policy") || view.parallelStepCardCollectionFallback;
      return {
        icon: "‖",
        iconTint: "member",
        title: view.parallelStepCardTitle,
        desc: `${view.parallelStepCardDescPrefix}${collection}`,
        configured: true,
        isFlowCard: true,
      };
    }
    if (step?.type === "repeat") {
      const defaultOperator = contractDefaultValue(contract, "condition_operator");
      const repeatUntilExpression = basicRepeatUntilExpression(step, sourceMembers, { defaultOperator });
      return {
        icon: "↻",
        iconTint: "member",
        title: view.repeatStepCardTitle,
        desc: repeatUntilExpression
          ? `${view.repeatStepCardDescPrefix}${repeatUntilExpression}`
          : view.repeatStepCardDescFallback,
        configured: true,
        isFlowCard: true,
      };
    }
    return {
      icon: "◆",
      iconTint: "accent",
      title: member ? member.name : view.memberStepCardTitleFallback,
      desc: step?.instruction || (member ? `${member.role} · ${member.model}` : ""),
      configured: !!step?.role,
      isFlowCard: false,
    };
  }

  function basicRepeatControlState({ step, members = [], schemas = [], contract, basicView = null } = {}) {
    const view = basicEditorViewState(basicView);
    const sourceMembers = Array.isArray(members) ? members : [];
    const sourceSchemas = Array.isArray(schemas) ? schemas : [];
    const memberById = new Map(sourceMembers.map((member) => [member.id, member]));
    const bodyMembers = (Array.isArray(step?.steps) ? step.steps : [])
      .filter((candidate) => candidate?.type === "member" && candidate.role)
      .map((candidate) => ({
        stepId: candidate.id,
        member: memberById.get(candidate.role) || null,
      }))
      .filter((candidate) => candidate.member);
    const cond = step?.cond || {};
    const condMember = bodyMembers.find((candidate) => candidate.stepId === cond.stepId)?.member || null;
    const condSchema = condMember
      ? sourceSchemas.find((schema) => schema.id === condMember.schema) || null
      : null;
    const fields = condSchema?.fields || [];
    const condField = fields.find((field) => field.name === cond.field) || null;
    const defaultOperator = contractDefaultValue(contract, "condition_operator");
    const operatorValue = cond.op || defaultOperator;
    const repeatUntilExpression = basicRepeatUntilExpression(step, sourceMembers, { defaultOperator });
    const iterationInputValue = typeof step?.iterationInput === "string" ? step.iterationInput : "";
    const iterationInputOptions = [
      { value: "", label: view.repeatIterationRuntimeDefaultLabel, disabled: false, reason: "" },
      ...repeatIterationInputOptions(contract, iterationInputValue),
    ];
    return {
      panelIcon: "↻",
      panelTitle: view.repeatPanelTitle,
      panelSub: view.repeatPanelSub,
      loopIdLabel: view.repeatLoopIdLabel,
      loopIdPlaceholder: view.repeatLoopIdPlaceholder,
      conditionTitle: view.repeatConditionTitle,
      conditionIntro: view.repeatConditionIntro,
      emptyBodyHint: view.repeatEmptyBodyHint,
      memberPlaceholderLabel: view.repeatMemberPlaceholderLabel,
      previewLabel: view.repeatPreviewLabel,
      previewFallback: view.repeatPreviewFallback,
      iterationInputLabel: view.repeatIterationInputLabel,
      maxIterationsLabel: view.repeatMaxIterationsLabel,
      maxIterationsPlaceholder: view.repeatMaxIterationsPlaceholder,
      tips: view.repeatTips,
      bodyMembers,
      bodyMemberOptions: bodyMembers.map((candidate) => ({
        value: candidate.stepId,
        label: candidate.member.name,
        bodyMember: candidate,
      })),
      hasBodyMembers: bodyMembers.length > 0,
      cond,
      condMember,
      condSchema,
      fields,
      condField,
      fieldOptions: fields.map((field) => ({
        value: field.name,
        label: `${field.name} · ${field.type}`,
        field,
      })),
      fieldPlaceholder: condSchema ? view.repeatConditionFieldPlaceholder : view.repeatConditionNoSchemaLabel,
      defaultOperator,
      operatorValue,
      operatorOptions: conditionOperatorOptions(contract, operatorValue),
      repeatUntilExpression,
      iterationInputValue,
      iterationInputOptions,
      selectedIterationInput: iterationInputOptions.find((option) => option.value === iterationInputValue) || null,
    };
  }

  function basicMemberStepControlState({ step, flow, members = [], contract, basicView = null, launchView = null } = {}) {
    const view = basicEditorViewState(basicView);
    const sourceMembers = Array.isArray(members) ? members : [];
    const memberById = new Map(sourceMembers.map((member) => [member.id, member]));
    const member = step?.role ? memberById.get(step.role) || null : null;
    const launchState = launchModeControlState(step, contract, launchView);
    const launchSources = collectFlowMemberSteps(flow?.steps || [])
      .filter((candidate) => candidate.id !== step?.id && candidate.role);
    const launchSourceOptions = launchSources.map((source) => {
      const sourceMember = memberById.get(source.role) || null;
      return {
        value: source.id,
        label: `${sourceMember?.name || source.role} · ${source.id}`,
        step: source,
        member: sourceMember,
      };
    });
    const runtimeDefault = { value: "", label: view.memberStepRuntimeDefaultLabel, disabled: false, reason: "" };
    const dispatchValue = typeof step?.dispatchMode === "string" ? step.dispatchMode : "";
    const collectionValue = typeof step?.collection === "string" ? step.collection : "";
    const dependencyValue = typeof step?.dependsMode === "string" ? step.dependsMode : "";
    const outputValue = typeof step?.outputFormat === "string" ? step.outputFormat : "";
    const dispatchOptions = [runtimeDefault, ...dispatchModeOptions(contract, dispatchValue)];
    const collectionOptions = [runtimeDefault, ...collectionPolicyOptions(contract, collectionValue)];
    const dependencyOptions = [runtimeDefault, ...dependencyModeOptions(contract, dependencyValue)];
    const outputOptions = [runtimeDefault, ...outputFormatOptions(contract, outputValue)];
    return {
      member,
      panelTitle: member ? member.name : view.memberStepPanelTitleFallback,
      panelSub: member ? `${member.role} · ${member.model}` : view.memberStepPanelSubFallback,
      memberFieldLabel: view.memberStepMemberLabel,
      memberPlaceholderLabel: view.memberStepMemberPlaceholder,
      memberOptions: sourceMembers.map((candidate) => ({
        value: candidate.id,
        label: `${candidate.name} · ${candidate.role}`,
        member: candidate,
      })),
      launchState,
      launchSources,
      launchSourceOptions,
      firstLaunchSourceId: launchSourceOptions[0]?.value || "",
      instructionLabel: view.memberStepInstructionLabel,
      instructionPlaceholder: view.memberStepInstructionPlaceholder,
      dispatchLabel: view.memberStepDispatchLabel,
      dispatchValue,
      dispatchOptions,
      selectedDispatch: dispatchOptions.find((option) => option.value === dispatchValue) || null,
      collectionLabel: view.memberStepCollectionLabel,
      collectionValue,
      collectionOptions,
      selectedCollection: collectionOptions.find((option) => option.value === collectionValue) || null,
      quorumLabel: view.memberStepQuorumLabel,
      quorumPlaceholder: view.memberStepQuorumPlaceholder,
      timeoutLabel: view.memberStepTimeoutLabel,
      timeoutPlaceholder: view.memberStepRuntimeDefaultLabel,
      dependencyLabel: view.memberStepDependencyLabel,
      dependencyValue,
      dependencyOptions,
      selectedDependency: dependencyOptions.find((option) => option.value === dependencyValue) || null,
      outputFormatLabel: view.memberStepOutputFormatLabel,
      outputValue,
      outputOptions,
      selectedOutput: outputOptions.find((option) => option.value === outputValue) || null,
      showQuorum: collectionValue === "quorum",
      allowedToolsLabel: view.memberStepAllowedToolsLabel,
      allowedToolsEmptyLabel: view.memberStepAllowedToolsEmptyLabel,
      blockedToolsLabel: view.memberStepBlockedToolsLabel,
      blockedToolsEmptyLabel: view.memberStepBlockedToolsEmptyLabel,
      schemaHint: member?.schema
        ? (() => {
          const tools = normalizeStringList(member.tools);
          const toolSummary = tools.join(", ") || view.memberStepSchemaHintEmptyToolsLabel;
          return {
            schema: member.schema,
            tools,
            toolSummary,
            parts: [
              { key: "prefix", text: view.memberStepSchemaHintPrefix },
              { key: "schema", text: member.schema, kind: "code" },
              { key: "tools", text: `${view.memberStepSchemaHintToolsPrefix}${toolSummary}` },
            ],
          };
        })()
        : null,
    };
  }

  function basicRepeatUntilExpression(step, members = [], options = {}) {
    const cond = step?.cond;
    if (!cond || !cond.stepId || !cond.field) return step?.until || "";
    const bodyStep = (Array.isArray(step.steps) ? step.steps : []).find((candidate) => candidate?.id === cond.stepId);
    const member = (Array.isArray(members) ? members : []).find((candidate) => candidate?.id === bodyStep?.role);
    if (!member) return step?.until || "";
    const op = cond.op || cond.operator || options.defaultOperator || "";
    if (!op) return step?.until || "";
    return `${member.name || member.role || member.id}.${cond.field} ${op} ${conditionValueLiteral(cond.val ?? cond.value ?? "")}`;
  }

  function graphEdgeConditionPatch(edge, patch = {}, options = {}) {
    const previous = normalizedEdgeCondition(edge);
    const previousText = previous?.path ? conditionTextForPath(previous.path, previous) : "";
    const currentCond = edge?.cond && typeof edge.cond === "object" && !Array.isArray(edge.cond)
      ? edge.cond
      : {};
    const merged = { ...currentCond, ...patch };
    const normalized = normalizedEdgeCondition({ cond: merged });
    if (!normalized?.path) {
      return { cond: { ...merged, var: "" }, label: "" };
    }
    const cond = {
      var: normalized.path,
      op: normalized.op || options.defaultOperator || "",
      val: normalized.val ?? "",
    };
    const currentLabel = String(edge?.label || "");
    const shouldReplaceLabel = !!options.forceLabel
      || !currentLabel
      || (!!previousText && currentLabel === previousText);
    return {
      cond,
      label: shouldReplaceLabel ? conditionTextForPath(cond.var, cond) : edge.label,
    };
  }

  function graphEdgeConditionOperatorPatch(edge, rawOperator, options = {}) {
    const patch = basicConditionOperatorPatch(rawOperator, options.contract);
    if (!("op" in patch)) return {};
    return graphEdgeConditionPatch(edge, patch, options);
  }

  function graphEdgeConditionValuePatch(edge, rawValue, options = {}) {
    return graphEdgeConditionPatch(edge, basicConditionValuePatch(rawValue), options);
  }

  function graphConditionPathForOption(option, field) {
    const name = String(field || "").trim();
    if (!option || !name) return "";
    const instanceId = String(option?.inst?.id || "").trim();
    if (!instanceId) return "";
    if (option.isParams || instanceId === "params") return `params.${name}`;
    return `steps.${instanceId}.${name}`;
  }

  function graphFirstConditionPatch(edge, conditionOptions = [], options = {}) {
    const rows = Array.isArray(conditionOptions) ? conditionOptions : [];
    const condRef = graphConditionRefForEdge(edge);
    const preferredId = String(options.instanceId || condRef.instanceId || "").trim();
    const owner = rows.find((option) => option?.inst?.id === preferredId) || rows[0];
    const field = options.field !== undefined
      ? String(options.field || "").trim()
      : String(condRef.field || owner?.fields?.[0]?.name || "").trim();
    const path = graphConditionPathForOption(owner, field);
    return path
      ? { var: path, op: edge?.cond?.op || options.defaultOperator || "", val: edge?.cond?.val ?? "" }
      : { var: "" };
  }

  function graphConditionEdgeKindForPatch(options = {}) {
    return String(options.conditionKind || contractDefaultValue(options.contract, "graph_condition_edge_kind")).trim();
  }

  function graphEdgeConditionOwnerPatch(edge, conditionOptions = [], instanceId, options = {}) {
    const rows = Array.isArray(conditionOptions) ? conditionOptions : [];
    const id = String(instanceId || "").trim();
    if (id && !rows.some((option) => String(option?.inst?.id || "").trim() === id)) return {};
    const owner = rows.find((option) => option?.inst?.id === instanceId);
    const firstField = String(owner?.fields?.[0]?.name || "").trim();
    const conditionPatch = graphFirstConditionPatch(edge, rows, {
      instanceId,
      field: firstField,
      defaultOperator: options.defaultOperator,
    });
    const patch = graphEdgeConditionPatch(edge, conditionPatch, {
      defaultOperator: options.defaultOperator,
      forceLabel: options.forceLabel,
    });
    return options.includeKind ? { kind: graphConditionEdgeKindForPatch(options), ...patch } : patch;
  }

  function graphEdgeConditionFieldPatch(edge, conditionOptions = [], field, options = {}) {
    const rows = Array.isArray(conditionOptions) ? conditionOptions : [];
    const condRef = graphConditionRefForEdge(edge);
    const owner = rows.find((option) => option?.inst?.id === condRef.instanceId) || rows[0];
    const fieldName = String(field || "").trim();
    if (fieldName && !((owner?.fields || []).some((candidate) => String(candidate?.name || "").trim() === fieldName))) return {};
    const conditionPatch = graphFirstConditionPatch(edge, rows, {
      instanceId: owner?.inst?.id || "",
      field,
      defaultOperator: options.defaultOperator,
    });
    const patch = graphEdgeConditionPatch(edge, conditionPatch, {
      defaultOperator: options.defaultOperator,
      forceLabel: options.forceLabel,
    });
    return options.includeKind ? { kind: graphConditionEdgeKindForPatch(options), ...patch } : patch;
  }

  function graphEdgeKindPatch(edge, nextKind, options = {}) {
    const kind = String(nextKind || "").trim();
    const conditionKind = graphConditionEdgeKindForPatch(options);
    if (kind !== conditionKind) {
      const previous = normalizedEdgeCondition(edge);
      const previousText = previous?.path ? conditionTextForPath(previous.path, previous) : "";
      const currentLabel = String(edge?.label || "");
      return {
        kind,
        cond: null,
        label: previousText && currentLabel === previousText ? "" : currentLabel,
      };
    }
    return {
      kind: conditionKind,
      ...graphEdgeConditionPatch(edge, options.conditionPatch || {}, {
        defaultOperator: options.defaultOperator,
        forceLabel: options.forceLabel,
      }),
    };
  }

  function graphEdgeFallbackPatch(edge, contract) {
    const kind = contractDefaultValue(contract, "graph_edge_kind");
    const draft = editorGraphDraftContract(contract);
    if (!kind || !draft) return null;
    return { kind, label: draft.fallbackEdgeLabel, cond: null };
  }

  function graphBranchConditionModePatch(edge, mode, options = {}) {
    const value = String(mode || "").trim();
    if (value === "fallback") return graphEdgeFallbackPatch(edge, options.contract);
    const conditionKind = graphConditionEdgeKindForPatch(options);
    if (value !== conditionKind) return {};
    return graphEdgeConditionOwnerPatch(edge, options.conditionOptions, options.firstOwnerId, {
      defaultOperator: options.defaultOperator,
      forceLabel: true,
      includeKind: true,
      contract: options.contract,
      conditionKind,
    });
  }

  function graphSelectionState({ selection = {}, instances = [], edges = [] } = {}) {
    const kind = String(selection?.kind || "");
    if (kind === "instance") {
      const instance = (Array.isArray(instances) ? instances : []).find((candidate) => candidate.id === selection.id) || null;
      return { kind, instance, edge: null, missing: !instance };
    }
    if (kind === "edge") {
      const edge = (Array.isArray(edges) ? edges : []).find((candidate) => candidate.id === selection.id) || null;
      return { kind, instance: null, edge, missing: !edge };
    }
    return { kind: "", instance: null, edge: null, missing: false };
  }

  function graphSelectionProjection(kind, id) {
    const selectionKind = String(kind || "").trim();
    const selectionId = String(id || "").trim();
    if (!selectionId || (selectionKind !== "instance" && selectionKind !== "edge")) return { kind: null, id: null };
    return { kind: selectionKind, id: selectionId };
  }

  function graphTemplateInspectorState({ studio = {}, template = null, templateSeed = null, templateView = null } = {}) {
    const seed = templateSeed && typeof templateSeed === "object" ? templateSeed : {};
    const view = graphTemplateViewForState(templateView);
    const members = Array.isArray(studio.members) ? studio.members : [];
    const instances = Array.isArray(studio.instances) ? studio.instances : [];
    const edges = Array.isArray(studio.edges) ? studio.edges : [];
    const frames = Array.isArray(studio.frames) ? studio.frames : [];
    const triggerLabel = template?.trigger || (Array.isArray(seed.triggers?.labels) ? seed.triggers.labels.join(", ") : "");
    const labels = triggerLabel ? [triggerLabel] : [];
    const placedMembers = new Set(instances.filter((instance) => instance?.memberId).map((instance) => instance.memberId)).size;
    const memberSummary = view.summaryMembersValueTemplate
      .replaceAll("{placed}", String(placedMembers))
      .replaceAll("{total}", String(members.length));
    return {
      name: template?.name || seed.name || "",
      repo: template?.repo || seed.repo || "",
      version: template?.version || seed.version || "",
      templateEyebrow: view.templateEyebrow,
      summaryTitle: view.summaryTitle,
      triggersTitle: view.triggersTitle,
      quickStartTitle: view.quickStartTitle,
      quickStartRows: view.quickStartRows,
      triggers: {
        labels,
        default: !!template?.defaultTrigger,
      },
      triggerRows: [
        { key: "labels", label: view.triggerLabelsLabel, value: labels.join(", ") },
        {
          key: "default",
          label: view.triggerDefaultLabel,
          value: template?.defaultTrigger ? view.defaultYesLabel : view.defaultNoLabel,
        },
      ],
      summaryRows: [
        { key: "members", label: view.summaryMembersLabel, value: memberSummary },
        { key: "instances", label: view.summaryInstancesLabel, value: instances.filter((instance) => !instance?.isTerminal).length },
        { key: "terminals", label: view.summaryTerminalsLabel, value: instances.filter((instance) => instance?.isTerminal).length },
        { key: "edges", label: view.summaryEdgesLabel, value: edges.length },
        { key: "frames", label: view.summaryFramesLabel, value: frames.length },
      ],
    };
  }

  function graphInstanceControlState({ inst, instances = [], members = [], schemas = [], graphView = null } = {}) {
    const view = graphCanvasViewState(graphView);
    const sourceMembers = Array.isArray(members) ? members : [];
    const sourceInstances = Array.isArray(instances) ? instances : [];
    const member = inst?.memberId
      ? sourceMembers.find((candidate) => candidate.id === inst.memberId) || null
      : null;
    const id = String(inst?.id || "");
    const col = Number.isFinite(Number(inst?.col)) ? Number(inst.col) + 1 : 1;
    const row = Number.isFinite(Number(inst?.row)) ? Number(inst.row) + 1 : 1;
    const outputSchema = member?.schema
      ? (Array.isArray(schemas) ? schemas : []).find((schema) => schema.id === member.schema) || null
      : null;
    const outputFields = Array.isArray(outputSchema?.fields) ? outputSchema.fields : [];
    const outputFieldRows = outputFields.map((field) => ({
      id: field.id,
      name: field.name,
      type: field.type,
      required: !!field.required,
      requiredLabel: field.required ? view.instanceOutputRequiredLabel : "",
    }));
    const tools = normalizeStringList(member?.tools);
    const memberToolSummary = tools.length
      ? `${tools.length} · ${tools.slice(0, 3).join(", ")}${tools.length > 3 ? "…" : ""}`
      : "0";
    const forkSourceOptions = sourceInstances
      .filter((candidate) => !candidate?.isTerminal && candidate.id !== inst?.id)
      .map((candidate) => {
        const sourceMember = candidate?.memberId
          ? sourceMembers.find((memberCandidate) => memberCandidate.id === candidate.memberId) || null
          : null;
        return {
          value: candidate.id,
          label: `${sourceMember?.name || candidate.id} · ${candidate.id}`,
          instance: candidate,
          member: sourceMember,
        };
      });
    return {
      member,
      memberId: member?.id || "",
      eyebrow: view.instanceEyebrow,
      title: member ? member.name : view.edgeRowMissingValue,
      idLine: graphTemplateText(view.instanceIdLineTemplate, { id, col, row }),
      deleteLabel: view.inspectorDeleteLabel,
      memberTitle: member ? member.name : view.edgeRowMissingValue,
      memberRoleLabel: member ? graphTemplateText(view.instanceMemberRoleTemplate, { role: member.role || "" }) : "",
      editMemberLabel: view.instanceEditMemberLabel,
      memberName: member?.name || "",
      memberSchemaLabel: member?.schema || view.edgeRowMissingValue,
      memberToolSummary,
      memberSummaryRows: [
        { key: "model", label: view.instanceModelLabel, value: member?.model || view.edgeRowMissingValue },
        { key: "schema", label: view.instanceSchemaLabel, value: member?.schema || view.edgeRowMissingValue },
        { key: "tools", label: view.instanceToolsLabel, value: memberToolSummary },
      ],
      memberHint: view.instanceMemberHint,
      positionTitle: view.instancePositionTitle,
      positionRows: [
        { key: "stage", label: view.instancePositionStageLabel, value: col },
        { key: "slot", label: view.instancePositionSlotLabel, value: row },
      ],
      outputSchema,
      outputFields,
      outputTitle: graphTemplateText(view.instanceOutputTitleTemplate, { schema: member?.schema || view.edgeRowMissingValue }),
      outputFieldRows,
      outputHint: view.instanceOutputHint,
      outputOpenMemberLabel: view.instanceOutputOpenMemberLabel,
      forkSourceOptions,
      firstForkSourceId: forkSourceOptions[0]?.value || "",
    };
  }

  function graphTemplateText(template, values = {}) {
    let out = String(template || "");
    for (const [key, value] of Object.entries(values || {})) {
      out = out.replaceAll(`{${key}}`, String(value ?? ""));
    }
    return out;
  }

  function graphToolTagClass(toolId, toolCatalog = []) {
    const id = String(toolId || "");
    const tool = (Array.isArray(toolCatalog) ? toolCatalog : [])
      .find((candidate) => String(candidate?.id || "") === id) || null;
    const tagClass = String(tool?.tagClass || tool?.tag_class || tool?.raw?.tag_class || "").trim();
    return tagClass ? ` ${tagClass}` : "";
  }

  function graphGridState({ instances = [], gridBase = {} } = {}) {
    const baseCols = Math.max(1, Number(gridBase?.cols || 1));
    const baseRows = Math.max(1, Number(gridBase?.rows || 1));
    let maxCol = baseCols - 1;
    let maxRow = baseRows - 1;
    for (const instance of Array.isArray(instances) ? instances : []) {
      const col = Number(instance?.col);
      const row = Number(instance?.row);
      if (Number.isFinite(col) && col > maxCol) maxCol = col;
      if (Number.isFinite(row) && row > maxRow) maxRow = row;
    }
    const grid = {
      ...gridBase,
      cols: maxCol + 2,
      rows: maxRow + 2,
    };
    const totalW = Number(grid.padX || 0) * 2 +
      Number(grid.cols || 0) * Number(grid.cellW || 0) +
      Math.max(0, Number(grid.cols || 0) - 1) * Number(grid.gapX || 0);
    const totalH = Number(grid.padY || 0) * 2 +
      Number(grid.rows || 0) * Number(grid.cellH || 0) +
      Math.max(0, Number(grid.rows || 0) - 1) * Number(grid.gapY || 0);
    return { grid, totalW, totalH };
  }

  function graphCellXY(grid, col, row) {
    return {
      x: Number(grid?.padX || 0) + Number(col || 0) * (Number(grid?.cellW || 0) + Number(grid?.gapX || 0)),
      y: Number(grid?.padY || 0) + Number(row || 0) * (Number(grid?.cellH || 0) + Number(grid?.gapY || 0)),
    };
  }

  function graphNodeBox(grid, inst) {
    const { x, y } = graphCellXY(grid, inst?.col, inst?.row);
    if (inst?.isSourceFile) {
      const sw = 210;
      const sh = 58;
      return {
        x: x + (Number(grid?.cellW || 0) - sw) / 2,
        y: y + (Number(grid?.cellH || 0) - sh) / 2,
        w: sw,
        h: sh,
      };
    }
    if (inst?.isGate) {
      const gw = 156;
      const gh = 56;
      return {
        x: x + (Number(grid?.cellW || 0) - gw) / 2,
        y: y + (Number(grid?.cellH || 0) - gh) / 2,
        w: gw,
        h: gh,
      };
    }
    return {
      x: x + (Number(grid?.cellW || 0) - GRAPH_NODE_W) / 2,
      y: y + (Number(grid?.cellH || 0) - GRAPH_NODE_H) / 2,
      w: GRAPH_NODE_W,
      h: GRAPH_NODE_H,
    };
  }

  function graphPortOut(grid, inst) {
    const box = graphNodeBox(grid, inst);
    return { x: box.x + box.w, y: box.y + box.h / 2 };
  }

  function graphPortIn(grid, inst) {
    const box = graphNodeBox(grid, inst);
    return { x: box.x, y: box.y + box.h / 2 };
  }

  function graphEdgePath(a, b) {
    if (b.x < a.x - 20) {
      const dropY = Math.max(a.y, b.y) + 90;
      const dx = 60;
      return `M ${a.x} ${a.y} C ${a.x + dx} ${a.y}, ${a.x + dx} ${dropY}, ${a.x} ${dropY} L ${b.x} ${dropY} C ${b.x - dx} ${dropY}, ${b.x - dx} ${b.y}, ${b.x} ${b.y}`;
    }
    const dx = Math.max(40, (b.x - a.x) * 0.5);
    return `M ${a.x} ${a.y} C ${a.x + dx} ${a.y}, ${b.x - dx} ${b.y}, ${b.x} ${b.y}`;
  }

  function graphEdgeMidpoint(a, b) {
    if (b.x < a.x - 20) return { x: (a.x + b.x) / 2, y: Math.max(a.y, b.y) + 90 };
    return { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 - 6 };
  }

  function graphCellAt(grid, x, y) {
    const col = Math.floor((Number(x || 0) - Number(grid?.padX || 0) + Number(grid?.gapX || 0) / 2) / (Number(grid?.cellW || 0) + Number(grid?.gapX || 0)));
    const row = Math.floor((Number(y || 0) - Number(grid?.padY || 0) + Number(grid?.gapY || 0) / 2) / (Number(grid?.cellH || 0) + Number(grid?.gapY || 0)));
    if (col < 0 || col >= Number(grid?.cols || 0) || row < 0 || row >= Number(grid?.rows || 0)) return null;
    return { col, row };
  }

  function graphDragCellAt(grid, world, drag) {
    const cx = Number(world?.x || 0) - Number(drag?.dx || 0) + GRAPH_NODE_W / 2;
    const cy = Number(world?.y || 0) - Number(drag?.dy || 0) + GRAPH_NODE_H / 2;
    return graphCellAt(grid, cx, cy);
  }

  function graphCellCanvasRows({ grid, instances = [], hoverCell = null } = {}) {
    const occupied = new Set();
    for (const instance of Array.isArray(instances) ? instances : []) {
      occupied.add(`${instance?.col}:${instance?.row}`);
    }
    const cols = Math.max(0, Number(grid?.cols || 0));
    const rows = Math.max(0, Number(grid?.rows || 0));
    const out = [];
    for (let col = 0; col < cols; col++) {
      for (let row = 0; row < rows; row++) {
        const cellOccupied = occupied.has(`${col}:${row}`);
        const hovered = Number(hoverCell?.col) === col && Number(hoverCell?.row) === row;
        const { x, y } = graphCellXY(grid, col, row);
        out.push({
          key: `cell-${col}-${row}`,
          col,
          row,
          occupied: cellOccupied,
          addVisible: !cellOccupied,
          className: "cell" + (cellOccupied ? " is-occupied" : "") + (hovered ? " is-hover" : ""),
          style: { left: x, top: y, width: Number(grid?.cellW || 0), height: Number(grid?.cellH || 0) },
        });
      }
    }
    return out;
  }

  function graphGridHeaderCanvasRows({ grid } = {}) {
    const cols = Math.max(0, Number(grid?.cols || 0));
    const rows = Math.max(0, Number(grid?.rows || 0));
    const columns = [];
    const rowHeaders = [];
    for (let col = 0; col < cols; col++) {
      const { x } = graphCellXY(grid, col, 0);
      columns.push({
        key: `col-${col}`,
        label: String(col + 1).padStart(2, "0"),
        className: "grid-head grid-head--col",
        style: { left: x, top: 28, width: Number(grid?.cellW || 0) },
      });
    }
    for (let row = 0; row < rows; row++) {
      const { y } = graphCellXY(grid, 0, row);
      rowHeaders.push({
        key: `row-${row}`,
        label: String.fromCharCode(65 + row),
        className: "grid-head grid-head--row",
        style: { left: 14, top: y + Number(grid?.cellH || 0) / 2 - 8 },
      });
    }
    return { columns, rows: rowHeaders };
  }

  function graphNodeCanvasState({ inst, members = [], density = "", graphView = null, toolCatalog = [] } = {}) {
    const view = graphCanvasViewState(graphView);
    const isCompact = density === "compact";
    if (inst?.isTerminal) {
      return {
        hidden: false,
        isTerminal: true,
        isSourceFile: false,
        dataKind: inst.kind,
        role: undefined,
        tabIndex: undefined,
        ariaLabel: undefined,
        sourceGlyph: "",
        sourceActivationHash: "",
        sourceActivationSelector: "",
        roleLabel: `terminal · ${inst.kind}`,
        title: inst.label,
        subtitle: inst.kind,
      };
    }
    const member = inst?.memberId
      ? (Array.isArray(members) ? members : []).find((candidate) => candidate.id === inst.memberId) || null
      : null;
    if (!member) return { hidden: true, isTerminal: false, isCompact };
    const tools = normalizeStringList(member.tools);
    const visibleTools = tools.slice(0, isCompact ? 3 : 6);
    return {
      hidden: false,
      isTerminal: false,
      isCompact,
      roleLabel: member.role,
      launchLabel: inst?.launchMode?.kind?.toLowerCase() || "—",
      title: member.name,
      subtitle: member.model,
      toolRows: visibleTools.map((tool) => ({
        id: tool,
        className: "tag" + graphToolTagClass(tool, toolCatalog),
      })),
      overflowLabel: tools.length > visibleTools.length ? `+${tools.length - visibleTools.length}` : "",
    };
  }

  function graphFrameCanvasState({ frame, grid } = {}) {
    const cell = (col, row) => ({
      x: Number(grid?.padX || 0) + Number(col || 0) * (Number(grid?.cellW || 0) + Number(grid?.gapX || 0)),
      y: Number(grid?.padY || 0) + Number(row || 0) * (Number(grid?.cellH || 0) + Number(grid?.gapY || 0)),
    });
    const rows = Math.max(1, Number(grid?.rows || 1));
    const cellW = Number(grid?.cellW || 0);
    const cellH = Number(grid?.cellH || 0);
    const startCol = Number.isFinite(Number(frame?.colStart)) ? Number(frame.colStart) : 0;
    const endCol = Number.isFinite(Number(frame?.colEnd)) ? Number(frame.colEnd) : startCol;
    const start = cell(startCol, 0);
    const end = cell(endCol, rows - 1);
    const x = start.x - 14;
    const y = start.y - 18;
    const width = (end.x + cellW) - x + 14;
    const height = (end.y + cellH) - y + 18;
    return {
      id: String(frame?.id || ""),
      label: String(frame?.label || ""),
      frameStyle: { left: x, top: y, width, height },
      labelStyle: { left: x + 12, top: y - 10 },
    };
  }

  function graphSourceFileAdornment({ instances = [], graphView = null } = {}) {
    const view = graphCanvasViewState(graphView);
    if (!view.sourceFileNodeId || !view.sourceFileNodeKind || !view.sourceFileLabel) return null;
    const sourceInstances = graphCanvasInstances({ instances, graphView });
    const positioned = sourceInstances
      .filter((instance) => Number.isFinite(Number(instance?.col)) && Number.isFinite(Number(instance?.row)));
    const minCol = positioned.length
      ? Math.min(...positioned.map((instance) => Number(instance.col)))
      : 0;
    const minRow = positioned.length
      ? Math.min(...positioned.map((instance) => Number(instance.row)))
      : 0;
    return {
      id: view.sourceFileNodeId,
      isSourceFile: true,
      isGraphAdornment: true,
      adornmentKind: "source_file",
      kind: view.sourceFileNodeKind,
      label: view.sourceFileLabel,
      col: minCol + view.sourceFileNodeColOffset,
      row: minRow + view.sourceFileNodeRowOffset,
    };
  }

  function graphCanvasInstances({ instances = [], graphView = null } = {}) {
    const view = graphCanvasViewState(graphView);
    return (Array.isArray(instances) ? instances : [])
      .filter((instance) => {
        if (!instance || typeof instance !== "object") return false;
        if (instance.isGraphAdornment || instance.isSourceFile) return false;
        return String(instance.id || "") !== view.sourceFileNodeId;
      });
  }

  function graphCanvasAdornments({ instances = [], graphView = null } = {}) {
    const sourceFileAdornment = graphSourceFileAdornment({ instances, graphView });
    return sourceFileAdornment ? [sourceFileAdornment] : [];
  }

  function graphSourceFileAdornmentCanvasState({ adornment = null, graphView = null } = {}) {
    const view = graphCanvasViewState(graphView);
    return {
      hidden: !adornment,
      isSourceFile: true,
      role: "button",
      tabIndex: 0,
      dataKind: String(adornment?.kind || view.sourceFileNodeKind || ""),
      ariaLabel: view.sourceFileAriaLabel,
      sourceGlyph: view.sourceFileGlyph,
      sourceActivationHash: view.sourceFileActivationHash,
      sourceActivationSelector: view.sourceFileActivationSelector,
      roleLabel: view.sourceFileRoleLabel,
      title: String(adornment?.label || view.sourceFileLabel || ""),
    };
  }

  function graphGateCanvasState({ inst, edges = [], contract = null, graphView = null } = {}) {
    const gateKind = String(inst?.gateKind || "");
    const draft = editorGraphDraftContract(contract) || emptyGraphDraftContract();
    const view = graphCanvasViewState(graphView);
    const glyph = view.gatePaletteRows.find((row) => row.id === gateKind)?.glyph || "";
    let sublabel = inst?.label || gateKind;
    if (gateKind === "join" && inst?.collection === "quorum" && inst?.quorum) {
      const incoming = (Array.isArray(edges) ? edges : []).filter((edge) => edge.to === inst?.id).length;
      sublabel = `${draft.joinQuorumLabelPrefix}${inst.quorum.n}/${incoming || inst.quorum.m}`;
    } else if (gateKind === "join" && inst?.collection) {
      sublabel = `${draft.joinLabelPrefix}${inst.collection}`;
    }
    return { glyph, sublabel, gateKind };
  }

  function graphEdgeCanvasState({ edge, to, active = false, selected = false, edgeStyle = "", contract = null, graphView = null } = {}) {
    const kind = String(edge?.kind || "next").trim();
    const terminalTarget = !!to?.isTerminal;
    const view = graphCanvasViewState(graphView);
    const labelText = String(edge?.label || view.edgeKindLabels[kind] || "");
    const edgeKinds = graphProjectionEdgeKinds(contract);
    const isCondition = kind === edgeKinds.conditionKind;
    const isFanout = kind === edgeKinds.fanoutKind;
    const mode = edgeStyle === "icons" ? "icons" : edgeStyle === "colored" ? "colored" : "text";
    return {
      kind,
      mode,
      labelText,
      labelWidth: labelText.length * 6 + 12,
      iconGlyph: isCondition ? "?" : isFanout ? "‖" : terminalTarget ? "■" : "→",
      labelFill: isCondition ? "var(--danger)" : isFanout ? "var(--accent)" : terminalTarget ? "var(--muted)" : "var(--ok)",
      iconLabelClass: "edge-label" + (isCondition ? " is-cond" : ""),
      textLabelClass: "edge-label" + (isCondition ? " is-cond" : "") + (active ? " is-active" : "") + (selected ? " is-selected" : ""),
      lineClass: "edge-line" +
        (isCondition ? " is-cond" : "") +
        (isFanout ? " is-fanout" : "") +
        (terminalTarget ? " is-term" : "") +
        (active ? " is-active" : "") +
        (selected ? " is-selected" : ""),
      markerEnd: selected || active ? "url(#arr-acc)" :
        isCondition ? "url(#arr-red)" :
          isFanout ? "url(#arr-acc)" :
            terminalTarget ? "url(#arr-dim)" :
              "url(#arr)",
    };
  }

  function graphGateControlState(inst, { edges, members, contract, graphView = null } = {}) {
    const view = graphCanvasViewState(graphView);
    const incoming = (edges || []).filter((edge) => edge.to === inst?.id);
    const outgoing = (edges || []).filter((edge) => edge.from === inst?.id);
    const defaultGateKind = contractDefaultValue(contract, "graph_gate_kind");
    const gateKind = String(inst?.gateKind || defaultGateKind || "").trim();
    const gateKindOptions = graphGateKindOptions(contract, gateKind, graphView);
    const collection = String(inst?.collection || (inst?.quorum?.n ? "quorum" : "")).trim();
    const collectionOptions = [
      { value: "", label: view.inspectorRuntimeDefaultLabel, disabled: false, reason: "" },
      ...collectionPolicyOptions(contract, collection),
    ];
    const dispatch = String(inst?.dispatch || inst?.dispatchMode || "").trim();
    const dispatchOptions = [
      { value: "", label: view.inspectorRuntimeDefaultLabel, disabled: false, reason: "" },
      ...dispatchModeOptions(contract, dispatch),
    ];
    const col = Number(inst?.col ?? 0);
    const row = Number(inst?.row ?? 0);
    return {
      incoming,
      outgoing,
      eyebrow: graphTemplateText(view.gateEyebrowTemplate, { kind: gateKind }),
      title: String(inst?.label || ""),
      idLine: graphTemplateText(view.gateIdLineTemplate, { id: inst?.id || "", col: col + 1, row: row + 1 }),
      deleteLabel: view.inspectorDeleteLabel,
      labelTitle: view.inspectorLabelTitle,
      kindTitle: view.inspectorKindTitle,
      gateKind,
      gateKindOptions,
      selectedGateKind: gateKindOptions.find((option) => option.value === gateKind),
      collectionTitle: view.gateCollectionTitle,
      collection,
      collectionOptions,
      selectedCollection: collectionOptions.find((option) => option.value === collection),
      quorumIncomingLabel: graphTemplateText(view.gateQuorumIncomingTemplate, { count: incoming.length }),
      joinMemberLabel: view.gateJoinMemberLabel,
      joinMemberPlaceholderOption: { value: "", label: view.gateJoinMemberPlaceholder },
      joinMemberHint: view.gateJoinMemberHint,
      dispatchTitle: view.gateDispatchTitle,
      dispatch,
      dispatchOptions,
      selectedDispatch: dispatchOptions.find((option) => option.value === dispatch),
      dispatchHint: view.gateDispatchHint,
      conditionsTitle: view.gateConditionsTitle,
      emptyBranchHint: view.gateEmptyBranchHint,
      wiringTitle: view.gateWiringTitle,
      incomingLabel: view.gateIncomingLabel,
      outgoingLabel: view.gateOutgoingLabel,
      firstMemberId: (members || []).find((member) => member?.id)?.id || "",
      memberOptions: (Array.isArray(members) ? members : [])
        .filter((member) => member?.id)
        .map((member) => ({
          value: member.id,
          label: graphTemplateText(view.gateMemberOptionTemplate, {
            id: member.id,
            name: member.name || member.id,
            role: member.role || "profile",
          }),
          member,
        })),
      incomingCount: incoming.length,
      outgoingCount: outgoing.length,
    };
  }

  function graphBranchConditionRows({ inst, edges = [], instances = [], members = [], schemas = [], flow, contract, graphView = null } = {}) {
    const view = graphCanvasViewState(graphView);
    const sourceEdges = Array.isArray(edges) ? edges : [];
    const sourceInstances = Array.isArray(instances) ? instances : [];
    const sourceMembers = Array.isArray(members) ? members : [];
    const instanceById = new Map(sourceInstances.map((candidate) => [candidate.id, candidate]));
    const memberById = new Map(sourceMembers.map((candidate) => [candidate.id, candidate]));
    const defaultOperator = contractDefaultValue(contract, "condition_operator");
    const conditionKind = contractDefaultValue(contract, "graph_condition_edge_kind");
    return sourceEdges
      .filter((edge) => edge?.from === inst?.id)
      .map((edge) => {
        const target = instanceById.get(edge.to) || null;
        const targetMember = target?.memberId ? memberById.get(target.memberId) || null : null;
        const condRef = graphConditionRefForEdge(edge);
        const conditionOptions = graphConditionOptions({
          instances: sourceInstances,
          members: sourceMembers,
          schemas,
          edge,
          flow,
          graphView,
        });
        const condOwner = conditionOptions.find((option) => option.inst.id === condRef.instanceId) || null;
        const fields = condOwner?.fields || conditionOptions[0]?.fields || [];
        const condField = fields.find((field) => field.name === condRef.field) || null;
        const operatorValue = edge?.cond?.op || defaultOperator;
        const isCondition = !!conditionKind && edge?.kind === conditionKind;
        return {
          edge,
          isCondition,
          conditionEdgeKind: conditionKind,
          modeValue: isCondition ? conditionKind : "fallback",
          modeOptions: [
            ...(conditionKind ? [{ value: conditionKind, label: view.branchConditionModeConditionLabel }] : []),
            { value: "fallback", label: view.branchConditionModeFallbackLabel },
          ],
          targetPrefix: view.branchConditionTargetPrefix,
          target,
          targetLabel: target?.isTerminal
            ? target.label
            : (targetMember?.name || target?.label || view.graphConditionTargetMissingLabel),
          condRef,
          conditionOptions,
          ownerOptions: conditionOptions.map((option) => ({
            value: option.inst.id,
            label: graphTemplateText(view.graphConditionOwnerOptionTemplate, {
              id: option.inst.id,
              name: option.member.name,
            }),
            option,
          })),
          ownerValue: condRef.instanceId || conditionOptions[0]?.inst.id || "",
          firstOwnerId: conditionOptions[0]?.inst.id || "",
          fields,
          fieldOptions: fields.map((field) => ({
            value: field.name,
            label: graphTemplateText(view.graphConditionFieldOptionTemplate, {
              id: field.id || field.name,
              name: field.name,
              type: field.type,
            }),
            field,
          })),
          fieldValue: condRef.field || "",
          fieldPlaceholderOption: { value: "", label: view.branchConditionFieldPlaceholder },
          condField,
          defaultOperator,
          operatorValue,
          operatorOptions: conditionOperatorOptions(contract, operatorValue),
          hasConditionOptions: conditionOptions.length > 0,
          noConditionOptionsHint: view.branchConditionNoOptionsHint,
        };
      });
  }

  function graphTerminalControlState(inst, contract, graphView = null) {
    const view = graphCanvasViewState(graphView);
    const defaultTerminalKind = contractDefaultValue(contract, "graph_terminal_kind");
    const terminalKind = String(inst?.kind || defaultTerminalKind || "").trim();
    const terminalKindOptions = graphTerminalKindOptions(contract, terminalKind, graphView);
    const id = String(inst?.id || "");
    const labelValue = String(inst?.label || "");
    const col = Number.isFinite(Number(inst?.col)) ? Number(inst.col) + 1 : 1;
    const row = Number.isFinite(Number(inst?.row)) ? Number(inst.row) + 1 : 1;
    return {
      eyebrow: graphTemplateText(view.terminalEyebrowTemplate, { kind: terminalKind }),
      title: labelValue,
      idLine: graphTemplateText(view.terminalIdLineTemplate, { id, col, row }),
      deleteLabel: view.inspectorDeleteLabel,
      labelTitle: view.inspectorLabelTitle,
      labelValue,
      kindTitle: view.inspectorKindTitle,
      terminalKind,
      terminalKindOptions,
      selectedTerminalKind: terminalKindOptions.find((option) => option.value === terminalKind) || null,
      authoringLockedTitle: view.terminalAuthoringLockedTitle,
      authoringLockedHint: view.terminalAuthoringLockedHint,
      editable: false,
    };
  }

  function graphEdgeInspectorState({ edge, instances = [], members = [], schemas = [], flow, contract, graphView = null } = {}) {
    const view = graphCanvasViewState(graphView);
    const sourceInstances = Array.isArray(instances) ? instances : [];
    const sourceMembers = Array.isArray(members) ? members : [];
    const instanceById = new Map(sourceInstances.map((candidate) => [candidate.id, candidate]));
    const memberById = new Map(sourceMembers.map((candidate) => [candidate.id, candidate]));
    const fromInstance = instanceById.get(edge?.from) || null;
    const toInstance = instanceById.get(edge?.to) || null;
    const fromMember = fromInstance?.memberId ? memberById.get(fromInstance.memberId) || null : null;
    const toMember = toInstance?.memberId ? memberById.get(toInstance.memberId) || null : null;
    const condRef = graphConditionRefForEdge(edge);
    const conditionOptions = graphConditionOptions({
      instances: sourceInstances,
      members: sourceMembers,
      schemas,
      edge,
      flow,
      graphView,
    });
    const condOwner = conditionOptions.find((option) => option.inst.id === condRef.instanceId) || null;
    const fields = condOwner?.fields || conditionOptions[0]?.fields || [];
    const condField = fields.find((field) => field.name === condRef.field) || null;
    const defaultOperator = contractDefaultValue(contract, "condition_operator");
    const operatorValue = edge?.cond?.op || defaultOperator;
    const defaultEdgeKind = contractDefaultValue(contract, "graph_edge_kind");
    const conditionKind = contractDefaultValue(contract, "graph_condition_edge_kind");
    const edgeKind = String(edge?.kind || defaultEdgeKind || "").trim();
    const edgeKindOptions = graphEdgeKindOptions(contract, edgeKind, graphView);
    const isCondition = !!conditionKind && edgeKind === conditionKind;
    return {
      edge,
      fromInstance,
      toInstance,
      fromMember,
      toMember,
      eyebrow: graphTemplateText(view.edgeEyebrowTemplate, { kind: edgeKind }),
      title: graphTemplateText(view.edgeTitleTemplate, {
        from: fromMember?.name || fromInstance?.label || view.edgeRowMissingValue,
        to: toMember?.name || toInstance?.label || view.edgeRowMissingValue,
      }),
      idLine: graphTemplateText(view.edgeIdLineTemplate, { id: edge?.id || "" }),
      deleteLabel: view.inspectorDeleteLabel,
      kindTitle: view.inspectorKindTitle,
      labelTitle: view.inspectorLabelTitle,
      conditionTitle: view.edgeConditionTitle,
      noConditionOptionsHint: view.edgeNoConditionOptionsHint,
      ownerPlaceholderOption: { value: "", label: view.edgeOwnerPlaceholder },
      fromTitle: view.edgeFromTitle,
      toTitle: view.edgeToTitle,
      fromRows: [
        { key: "instance", label: view.edgeRowInstanceLabel, value: fromInstance?.id || "" },
        { key: "member", label: view.edgeRowMemberLabel, value: fromMember?.name || view.edgeRowMissingValue },
        { key: "schema", label: view.edgeRowSchemaLabel, value: fromMember?.schema || view.edgeRowMissingValue },
      ],
      toRows: [
        { key: "instance", label: view.edgeRowInstanceLabel, value: toInstance?.id || "" },
        { key: "member", label: view.edgeRowMemberLabel, value: toMember?.name || (toInstance?.isTerminal ? view.edgeTerminalMemberValue : view.edgeRowMissingValue) },
        { key: "schema", label: view.edgeRowSchemaLabel, value: toMember?.schema || view.edgeRowMissingValue },
      ],
      condRef,
      conditionOptions,
      condOwner,
      condField,
      ownerOptions: conditionOptions.map((option) => ({
        value: option.inst.id,
        label: graphTemplateText(view.graphConditionOwnerOptionTemplate, {
          id: option.inst.id,
          name: option.member.name,
        }),
        option,
      })),
      ownerValue: condRef.instanceId || "",
      fields,
      fieldOptions: fields.map((field) => ({
        value: field.name,
        label: graphTemplateText(view.graphConditionFieldOptionTemplate, {
          id: field.id || field.name,
          name: field.name,
          type: field.type,
        }),
        field,
      })),
      fieldValue: condRef.field || "",
      fieldPlaceholder: condOwner ? view.edgeFieldPlaceholder : view.edgeFieldNoSchemaPlaceholder,
      defaultOperator,
      operatorValue,
      operatorOptions: conditionOperatorOptions(contract, operatorValue),
      defaultEdgeKind,
      edgeKind,
      isCondition,
      conditionEdgeKind: conditionKind,
      edgeKindOptions,
      selectedEdgeKind: edgeKindOptions.find((option) => option.value === edgeKind) || null,
      conditionPatch: graphFirstConditionPatch(edge, conditionOptions, { defaultOperator }),
      hasConditionOptions: conditionOptions.length > 0,
    };
  }

  function graphGateKindAllowed(contract, kind) {
    return contractStringValues(contract?.mob_definition?.graph_gate_kinds).includes(String(kind || "").trim());
  }

  function graphTerminalKindAllowed(contract, kind) {
    return contractStringValues(contract?.mob_definition?.graph_terminal_kinds).includes(String(kind || "").trim());
  }

  function graphGateKindPatch(rawKind, contract) {
    const gateKind = String(rawKind || "").trim();
    return graphGateKindAllowed(contract, gateKind) ? { gateKind } : {};
  }

  function graphInstanceLabelPatch(rawLabel) {
    return { label: String(rawLabel || "") };
  }

  function graphEdgeLabelPatch(rawLabel) {
    return { label: String(rawLabel || "") };
  }

  function graphTerminalKindPatch(rawKind, contract) {
    const kind = String(rawKind || "").trim();
    return graphTerminalKindAllowed(contract, kind) ? { kind } : {};
  }

  function graphJoinCollectionPatch(inst, collection, { incomingCount = 0, firstMemberId = "", contract } = {}) {
    const next = String(collection || "").trim();
    if (!collectionPolicyAllowed(contract, next)) return {};
    const draft = editorGraphDraftContract(contract) || emptyGraphDraftContract();
    const count = Math.max(1, Number(incomingCount) || 0);
    return {
      collection: next,
      label: `${draft.joinLabelPrefix}${next || draft.parallelMissingCollectionLabel}`,
      quorum: next === "quorum"
        ? { ...(inst?.quorum || {}), n: inst?.quorum?.n || count, m: count }
        : null,
      controllerRole: next && next !== "all" ? (inst?.controllerRole || firstMemberId || "") : "",
    };
  }

  function graphJoinQuorumPatch(inst, n, incomingCount = 0) {
    return {
      quorum: {
        ...(inst?.quorum || {}),
        n: Number(n) || 1,
        m: Math.max(1, Number(incomingCount) || 0),
      },
    };
  }

  function graphJoinControllerRolePatch(rawRole, members) {
    const controllerRole = String(rawRole || "").trim();
    return memberRoleAllowed(members, controllerRole) ? { controllerRole } : {};
  }

  function graphForkDispatchPatch(_inst, dispatch, contract) {
    const next = String(dispatch || "").trim();
    if (!dispatchModeAllowed(contract, next)) return {};
    return { dispatch: next, label: next };
  }

  function buildDocument({ flow, studio, currentFlow, deploySettings, contract }) {
    const members = studio?.members || [];
    const schemas = studio?.schemas || [];
    const displayName = currentFlow?.name || flow?.mobName || flow?.name || "MobKit flow";
    const mobSettings = normalizeMobSettings(studio?.mobSettings);
    const deploy = normalizeDeploySettings(deploySettings);
    const documentFlow = flowForDocument(flow);
    return {
      schema_version: SCHEMA_VERSION,
      mob_id: slug(displayName, "mobkit_flow"),
      name: displayName,
      mob_settings: mobSettings,
      members,
      instances: instancesForDocument(documentFlow, members, studio?.instances || studio?.nodes || [], contract),
      edges: edgesForDocument(documentFlow, members, studio?.edges || [], contract),
      frames: framesForDocument(documentFlow, members, studio?.frames || [], contract),
      schemas,
      skill_realms: skillRealmsForDocument(members, studio?.skillRealms),
      flow: documentFlow,
      launch_modes: launchModesFromFlow(documentFlow, members),
      deploy,
      deploy_command: deploy.command,
    };
  }

  function authoringFlowForDocument({ editorMode, flow, instances, edges, members, contract } = {}) {
    return flow;
  }

  function authoringDocumentFromState({ editorMode, flow, studio, currentFlow, deploySettings, mobSettings, contract, modelCatalog, toolCatalog, contractLoaded = false } = {}) {
    const sourceStudio = studio && typeof studio === "object" ? studio : {};
    const effectiveFlow = authoringFlowForDocument({
      editorMode,
      flow,
      instances: sourceStudio.instances,
      edges: sourceStudio.edges,
      members: sourceStudio.members,
      contract,
    });
    const reconciled = reconcileAuthoringWithContract({
      members: sourceStudio.members,
      skillRealms: sourceStudio.skillRealms,
      schemas: sourceStudio.schemas,
      deploySettings,
      mobSettings,
      flow: effectiveFlow,
      instances: sourceStudio.instances,
      edges: sourceStudio.edges,
      contract,
      modelCatalog,
      toolCatalog,
      contractLoaded,
    });
    const document = buildDocument({
      flow: reconciled.flow,
      studio: {
        members: reconciled.members,
        schemas: sourceStudio.schemas,
        instances: reconciled.instances,
        edges: reconciled.edges,
        frames: sourceStudio.frames,
        skillRealms: sourceStudio.skillRealms,
        mobSettings: reconciled.mobSettings,
      },
      currentFlow,
      deploySettings: reconciled.deploySettings,
      contract,
    });
    return {
      flow: reconciled.flow,
      document,
      members: reconciled.members,
      instances: document.instances,
      edges: document.edges,
      frames: document.frames,
      deploySettings: reconciled.deploySettings,
      mobSettings: reconciled.mobSettings,
    };
  }

  function authoringProjectionApplyPlan(projection, current = {}) {
    if (!projection || typeof projection !== "object") return { ok: false };
    const studio = current?.studio && typeof current.studio === "object" ? current.studio : {};
    const members = Array.isArray(projection.members) ? projection.members : [];
    const skillRealms = Array.isArray(projection.skillRealms) ? projection.skillRealms : [];
    const schemas = Array.isArray(projection.schemas) ? projection.schemas : [];
    const instances = Array.isArray(projection.instances) ? projection.instances : [];
    const edges = Array.isArray(projection.edges) ? projection.edges : [];
    const frames = Array.isArray(projection.frames) ? projection.frames : [];
    const graphMembers = Array.isArray(projection.members) ? projection.members : (studio.members || []);
    const graphSignatureNext = projection.instances
      ? graphStructureSignature(instances, edges, { members: graphMembers, contract: current.contract })
      : "";
    const graphSignatureCurrent = projection.instances
      ? graphStructureSignature(studio.instances || [], studio.edges || [], { members: studio.members || [], contract: current.contract })
      : "";
    return {
      ok: true,
      flow: {
        changed: !jsonEquivalent(projection.flow, current.flow),
        value: projection.flow,
      },
      members: {
        changed: !jsonEquivalent(members, studio.members || []),
        value: members,
      },
      skillRealms: {
        changed: !jsonEquivalent(skillRealms, studio.skillRealms || []),
        value: skillRealms,
      },
      schemas: {
        changed: !jsonEquivalent(schemas, studio.schemas || []),
        value: schemas,
      },
      graph: {
        changed: !!projection.instances && graphSignatureNext !== graphSignatureCurrent,
        signature: graphSignatureNext,
        instances,
        edges,
      },
      frames: {
        changed: !jsonEquivalent(frames, studio.frames || []),
        value: frames,
      },
      deploySettings: {
        changed: !jsonEquivalent(projection.deploySettings, current.deploySettings),
        value: projection.deploySettings,
      },
      mobSettings: {
        changed: !jsonEquivalent(projection.mobSettings, current.mobSettings),
        value: projection.mobSettings,
      },
    };
  }

  function flowForDocument(flow) {
    const source = flow && typeof flow === "object" ? flow : {};
    return {
      ...source,
      steps: sanitizeFlowStepsForDocument(source.steps),
    };
  }

  function sanitizeFlowStepsForDocument(steps) {
    return (Array.isArray(steps) ? steps : []).map((step) => sanitizeFlowStepForDocument(step));
  }

  function sanitizeFlowStepForDocument(step) {
    if (!step || typeof step !== "object") return step;
    const next = { ...step };
    if (next.type === "member") {
      const dispatchMode = dispatchModeFromStepSource(next);
      const collection = collectionModeFromStepSource(next);
      const dependsMode = dependencyModeFromStepSource(next);
      const outputFormat = normalizeOutputFormat(next.outputFormat ?? next.output_format);
      delete next.dispatch;
      delete next.dispatchMode;
      delete next.dispatch_mode;
      delete next.collection;
      delete next.collectionPolicy;
      delete next.collection_policy;
      delete next.dependsMode;
      delete next.depends_mode;
      delete next.output_format;
      if (dispatchMode) next.dispatchMode = dispatchMode;
      if (collection) next.collection = collection;
      if (dependsMode) next.dependsMode = dependsMode;
      if (outputFormat) {
        next.outputFormat = outputFormat;
      } else {
        delete next.outputFormat;
      }
    }
    if (next.type === "repeat") {
      const iterationInput = String(next.iterationInput ?? next.iteration_input ?? "").trim();
      delete next.iteration_input;
      if (iterationInput) {
        next.iterationInput = iterationInput;
      } else {
        delete next.iterationInput;
      }
    }
    if (Array.isArray(next.steps)) next.steps = sanitizeFlowStepsForDocument(next.steps);
    if (Array.isArray(next.branches)) {
      next.branches = next.branches.map((branch) => ({
        ...branch,
        steps: sanitizeFlowStepsForDocument(branch?.steps),
      }));
    }
    if (Array.isArray(next.fallback)) next.fallback = sanitizeFlowStepsForDocument(next.fallback);
    return next;
  }

  function edgesForDocument(flow, members, existingEdges, contract) {
    const projected = graphProjectionForFlow(flow, members, contract).edges || [];
    const canonicalByKey = new Map();
    for (const edge of projected) {
      const normalized = normalizeGraphEdgeForDocument(edge);
      const key = graphEdgeKey(normalized);
      if (key && !canonicalByKey.has(key)) canonicalByKey.set(key, normalized);
    }
    const out = [];
    const seen = new Set();
    for (const edge of existingEdges || []) {
      const normalizedExisting = normalizeGraphEdgeForDocument(edge);
      const key = graphEdgeKey(normalizedExisting);
      const canonical = canonicalByKey.get(key);
      if (!canonical) continue;
      out.push({
        ...canonical,
        id: edge.id || canonical.id,
      });
      seen.add(key);
    }
    for (const edge of projected) {
      const normalized = normalizeGraphEdgeForDocument(edge);
      const key = graphEdgeKey(normalized);
      if (key && !seen.has(key)) {
        out.push(normalized);
        seen.add(key);
      }
    }
    return out;
  }

  function normalizeGraphEdgeForDocument(edge) {
      const condition = normalizedEdgeCondition(edge);
      if (!condition?.path) return edge;
      return {
        ...edge,
        cond: {
          var: condition.path,
          op: condition.op || "",
          val: condition.val === undefined || condition.val === null ? "" : String(condition.val),
        },
      };
  }

  function graphEdgeKey(edge) {
    const from = String(edge?.from || "").trim();
    const to = String(edge?.to || "").trim();
    const kind = String(edge?.kind || "").trim();
    return from && to && kind ? `${from}\n${to}\n${kind}` : "";
  }

  function instancesForDocument(flow, members, existingInstances, contract) {
    const projected = graphProjectionForFlow(flow, members, contract).instances || [];
    const canonicalById = new Map();
    for (const instance of projected) {
      if (instance?.id && !canonicalById.has(String(instance.id))) {
        canonicalById.set(String(instance.id), instance);
      }
    }
    const out = [];
    const seen = new Set();
    for (const instance of existingInstances || []) {
      const id = String(instance?.id || "");
      const canonical = canonicalById.get(id);
      if (!id || !canonical) continue;
      out.push(canonicalizeGraphInstance(instance, canonical));
      seen.add(id);
    }
    for (const instance of projected) {
      const id = String(instance?.id || "");
      if (id && !seen.has(id)) out.push(instance);
    }
    return out;
  }

  function canonicalizeGraphInstance(instance, canonical) {
    const merged = { ...canonical, ...instance };
    if (canonical.isGate) {
      return {
        ...merged,
        id: canonical.id,
        isGate: true,
        isTerminal: false,
        memberId: undefined,
        gateKind: canonical.gateKind,
        dispatch: canonical.dispatch,
        collection: canonical.collection,
        dependsMode: canonical.dependsMode,
        quorum: canonical.quorum,
        controllerRole: canonical.controllerRole,
      };
    }
    return {
      ...merged,
      id: canonical.id,
      memberId: canonical.memberId,
      isGate: false,
      isTerminal: false,
        launchMode: canonical.launchMode,
        dispatchMode: canonical.dispatchMode,
        collection: canonical.collection,
        dependsMode: canonical.dependsMode,
        quorum: canonical.quorum,
      timeoutMs: canonical.timeoutMs,
      allowedTools: canonical.allowedTools,
      blockedTools: canonical.blockedTools,
      outputFormat: canonical.outputFormat,
    };
  }

  function graphProjectionEdgeKinds(contract) {
    return {
      defaultKind: contractDefaultValue(contract, "graph_edge_kind"),
      conditionKind: contractDefaultValue(contract, "graph_condition_edge_kind"),
      fanoutKind: contractDefaultValue(contract, "graph_fanout_edge_kind"),
    };
  }

  function graphProjectionForFlow(flow, members, contract) {
    const edgeKinds = graphProjectionEdgeKinds(contract);
    const draft = editorGraphDraftContract(contract) || emptyGraphDraftContract();
    const projection = { instances: [], edges: [], frames: [] };
    const edgeId = () => `e${projection.edges.length + 1}`;

    function connectEdges(fromIds, toIds, kind = edgeKinds.defaultKind, label = "", extra = {}) {
      for (const from of fromIds || []) {
        for (const to of toIds || []) {
          if (!from || !to) continue;
          projection.edges.push({ id: edgeId(), from, to, kind, label, ...extra });
        }
      }
    }

    function emit(steps, startCol, row = 0, initialPrevExits = [], entryKind = edgeKinds.defaultKind, entryLabel = "", lane = "") {
      let col = startCol;
      let prevExits = initialPrevExits || [];
      let entries = [];
      let firstConnection = true;
      const rememberEntries = (ids) => {
        if (!entries.length) entries = (ids || []).filter(Boolean);
      };
      const connectPrev = (targets, extra = {}) => {
        const kind = firstConnection ? entryKind : edgeKinds.defaultKind;
        const label = firstConnection ? entryLabel : "";
        connectEdges(prevExits, targets, kind, label, extra);
        firstConnection = false;
      };

      for (const step of steps || []) {
        if (!step || step.type === "input") continue;
        if (step.type === "member") {
          const dispatchMode = dispatchModeFromStepSource(step);
          const collection = collectionModeFromStepSource(step);
          const dependsMode = dependencyModeFromStepSource(step);
          const outputFormat = normalizeOutputFormat(step.outputFormat ?? step.output_format);
          const instance = {
            id: step.id,
            memberId: step.role,
            col,
            row,
            lane,
            launchMode: launchModeFromAuthoringSource(step),
            quorum: numberOrNull(step.quorum ?? step.collectionQuorum),
            timeoutMs: normalizePositiveInteger(step.timeoutMs ?? step.timeout_ms),
            allowedTools: normalizeStringList(step.allowedTools || step.allowed_tools),
            blockedTools: normalizeStringList(step.blockedTools || step.blocked_tools),
          };
          if (dispatchMode) instance.dispatchMode = dispatchMode;
          if (collection) instance.collection = collection;
          if (dependsMode) instance.dependsMode = dependsMode;
          if (outputFormat) instance.outputFormat = outputFormat;
          projection.instances.push(instance);
          connectPrev([step.id]);
          rememberEntries([step.id]);
          prevExits = [step.id];
          col += 1;
        } else if (step.type === "branch" || step.type === "parallel") {
          const isBranch = step.type === "branch";
          const gateId = `g_${step.type}_${step.id}`;
          const joinId = `j_${step.type}_${step.id}`;
          const gateCol = col;
          const dispatch = isBranch ? "" : dispatchModeFromStepSource(step);
          const collection = isBranch ? "any" : collectionModeFromStepSource(step);
          projection.instances.push({
            id: gateId,
            isGate: true,
            gateKind: isBranch ? "branch" : "fork",
            label: isBranch ? draft.branchGateLabel : dispatch,
            dispatch: isBranch ? undefined : dispatch,
            dependsMode: dependencyModeFromStepSource(step),
            col: gateCol,
            row,
          });
          connectPrev([gateId]);
          rememberEntries([gateId]);
          const lanes = [
            ...(step.branches || []),
            ...(isBranch && Array.isArray(step.fallback) && step.fallback.length
              ? [{ id: "fallback", label: draft.branchFallbackLaneLabel, steps: step.fallback }]
              : []),
          ];
          const exits = [];
          let maxCol = gateCol + 1;
          lanes.forEach((branch, index) => {
            const isFallback = isBranch && branch.id === "fallback";
            const cond = isBranch && !isFallback
              ? editorCondToGraphCond(branch.cond) || conditionTextToGraphCond(branch.condition)
              : null;
            const laneProjection = emit(
              branch.steps || [],
              gateCol + 1,
              row + index,
              [gateId],
              isFallback ? edgeKinds.defaultKind : isBranch ? edgeKinds.conditionKind : edgeKinds.fanoutKind,
              isFallback ? draft.fallbackEdgeLabel : isBranch ? (branch.condition || "") : "",
              isFallback ? draft.branchFallbackLaneLabel : "",
            );
            if (cond) {
              for (const edge of projection.edges) {
                if (edge.from === gateId && (laneProjection.entries || []).includes(edge.to)) edge.cond = cond;
              }
            }
            exits.push(...laneProjection.exits);
            maxCol = Math.max(maxCol, laneProjection.nextCol);
          });
          projection.instances.push({
            id: joinId,
            isGate: true,
            gateKind: "join",
            label: isBranch ? draft.branchJoinLabel : `${draft.joinLabelPrefix}${collection || draft.parallelMissingCollectionLabel}`,
            collection,
            controllerRole: step.controllerRole || step.controllerMemberId || step.controlRole || "",
            quorum: !isBranch && collection === "quorum"
              ? { mode: "NofM", n: numberOrNull(step.quorum) || 2, m: Math.max(1, lanes.length) }
              : undefined,
            col: maxCol,
            row,
          });
          connectEdges(exits, [joinId], edgeKinds.defaultKind, "");
          projection.frames.push({
            id: `frame_${step.type}_${step.id}`,
            kind: isBranch ? "Branch" : "Parallel",
            colStart: gateCol,
            colEnd: maxCol,
            label: isBranch
              ? branchFrameLabel(lanes.length, draft)
              : parallelFrameLabel(dispatch, collection, draft),
          });
          prevExits = [joinId];
          col = maxCol + 1;
          firstConnection = false;
        } else if (step.type === "repeat") {
          const frameStart = col;
          const loopProjection = emit(
            step.steps || [],
            col,
            row,
            prevExits,
            firstConnection ? entryKind : edgeKinds.defaultKind,
            firstConnection ? entryLabel : "",
            lane,
          );
          rememberEntries(loopProjection.entries);
          firstConnection = false;
          const cond = repeatCondToGraphCond(step.cond, loopProjection.exits[0]);
          connectEdges(
            loopProjection.exits,
            loopProjection.entries,
            edgeKinds.conditionKind,
            repeatEdgeLabel(step, draft),
            cond ? { cond } : {},
          );
          if (loopProjection.entries.length) {
            projection.frames.push({
              id: `frame_${step.id}`,
              kind: "RepeatUntil",
              colStart: frameStart,
              colEnd: Math.max(frameStart, loopProjection.nextCol - 1),
              label: repeatFrameLabel(step, draft),
            });
          }
          col = loopProjection.nextCol;
          prevExits = loopProjection.exits;
        }
      }
      return { entries, exits: prevExits, nextCol: col };
    }

    emit(flow?.steps || [], 0);
    return projection;
  }

  function editorCondToGraphCond(cond) {
    if (!cond || !cond.field) return null;
    const path = cond.namespace === "params" || cond.stepId === "params"
      ? `params.${cond.field}`
      : `steps.${cond.stepId}.${cond.field}`;
    return { var: path, op: cond.op || "", val: String(cond.val ?? "") };
  }

  function dispatchModeFromStepSource(step) {
    const raw = step?.dispatch ?? step?.dispatchMode ?? step?.dispatch_mode;
    if (raw === null || raw === undefined || String(raw).trim() === "") return "";
    return normalizeDispatchMode(raw);
  }

  function dependencyModeFromStepSource(step) {
    const raw = step?.dependsMode ?? step?.depends_mode;
    if (raw === null || raw === undefined || String(raw).trim() === "") return "";
    return String(raw).trim();
  }

  function collectionModeFromStepSource(step) {
    const raw = step?.collection ?? step?.collectionPolicy ?? step?.collection_policy;
    if (raw === null || raw === undefined) return "";
    if (typeof raw === "object") {
      const type = String(raw.type || "").trim();
      return type ? normalizeCollectionMode(raw) : "";
    }
    if (String(raw).trim() === "") return "";
    return normalizeCollectionMode(raw);
  }

  function branchFrameLabel(pathCount, draft) {
    const count = Math.max(0, Number(pathCount) || 0);
    const suffix = count === 1 ? draft.branchFrameSingularSuffix : draft.branchFramePluralSuffix;
    return `${draft.branchFrameLabelPrefix}${count}${suffix}`;
  }

  function parallelFrameLabel(dispatch, collection, draft) {
    const dispatchLabel = dispatch || draft.parallelMissingDispatchLabel;
    const collectionLabel = collection || draft.parallelMissingCollectionLabel;
    return `${draft.parallelFrameLabelPrefix}${dispatchLabel}${draft.parallelFrameJoinInfix}${collectionLabel}`;
  }

  function repeatFrameLabel(step, draft) {
    const max = Number(step?.maxIterations ?? step?.max_iterations);
    return Number.isInteger(max) && max > 0
      ? `${draft.repeatFrameLabelPrefix}${draft.repeatMaxIterationsPrefix}${max}`
      : `${draft.repeatFrameLabelPrefix}${draft.repeatMissingMaxIterationsLabel}`;
  }

  function repeatEdgeLabel(step, draft) {
    return step?.until ? `${draft.repeatEdgeUntilPrefix}${step.until}` : draft.repeatEdgeUntilFallback;
  }

  function conditionTextToGraphCond(text) {
    const match = /([A-Za-z0-9_.-]+)\s*(==|>|<)\s*['"]?([^'"]+)['"]?/.exec(String(text || ""));
    return match ? { var: match[1], op: match[2], val: match[3] } : null;
  }

  function repeatCondToGraphCond(cond, fallbackStepId) {
    if (!cond || !cond.field) return null;
    return {
      var: `steps.${cond.stepId || fallbackStepId}.${cond.field}`,
      op: cond.op || "",
      val: String(cond.val ?? ""),
    };
  }

  function framesForDocument(flow, members, existingFrames, contract) {
    const draft = editorGraphDraftContract(contract) || emptyGraphDraftContract();
    const projected = graphProjectionForFlow(flow, members, contract).frames || [];
    const required = requiredFramesFromFlow(flow, draft);
    const canonicalFrames = new Map();
    for (const frame of [...projected, ...required]) {
      if (frame?.id && !canonicalFrames.has(String(frame.id))) canonicalFrames.set(String(frame.id), frame);
    }
    const byId = new Map();
    for (const frame of existingFrames || []) {
      const id = String(frame?.id || "");
      const canonical = canonicalFrames.get(id);
      if (id && canonical) {
        byId.set(id, canonical);
      }
    }
    for (const frame of projected) {
      if (frame?.id && !byId.has(String(frame.id))) byId.set(String(frame.id), frame);
    }
    for (const frame of required) {
      if (frame?.id && !byId.has(String(frame.id))) byId.set(String(frame.id), frame);
    }
    return Array.from(byId.values());
  }

  function requiredFramesFromFlow(flow, draft) {
    const frames = [];
    const visit = (steps) => {
      for (const step of steps || []) {
        if (!step?.id) continue;
        if (step.type === "branch") {
          frames.push({
            id: `frame_branch_${step.id}`,
            kind: "Branch",
            colStart: 0,
            colEnd: 0,
            label: branchFrameLabel((step.branches || []).length + (Array.isArray(step.fallback) && step.fallback.length ? 1 : 0), draft),
          });
        } else if (step.type === "parallel") {
          const dispatch = dispatchModeFromStepSource(step);
          const collection = collectionModeFromStepSource(step);
          frames.push({
            id: `frame_parallel_${step.id}`,
            kind: "Parallel",
            colStart: 0,
            colEnd: 0,
            label: parallelFrameLabel(dispatch, collection, draft),
          });
        } else if (step.type === "repeat") {
          frames.push({
            id: `frame_${step.id}`,
            kind: "RepeatUntil",
            colStart: 0,
            colEnd: 0,
            label: repeatFrameLabel(step, draft),
          });
        }
        if (Array.isArray(step.steps)) visit(step.steps);
        if (Array.isArray(step.branches)) {
          for (const branch of step.branches) visit(branch.steps || []);
        }
        if (Array.isArray(step.fallback)) visit(step.fallback);
      }
    };
    visit(flow?.steps || []);
    return frames;
  }

  function normalizeDeploySettings(settings) {
    const merged = { ...EMPTY_DEPLOY_SETTINGS, ...(settings || {}) };
    const surface = String(merged.surface || "").trim();
    const trustPolicy = String(merged.trustPolicy || merged.trust_policy || "").trim();
    const realmBackend = String(merged.realmBackend || merged.realm_backend || "").trim();
    return {
      command: String(merged.command || "").trim(),
      surface: surface === "rpc" || surface === "cli" ? surface : "",
      trust_policy: trustPolicy === "strict" || trustPolicy === "permissive" ? trustPolicy : "",
      model: String(merged.model || "").trim(),
      max_duration: String(merged.maxDuration || merged.max_duration || "").trim(),
      max_tool_calls: numberOrNull(merged.maxToolCalls ?? merged.max_tool_calls),
      max_total_tokens: numberOrNull(merged.maxTotalTokens ?? merged.max_total_tokens),
      isolated: merged.isolated === true,
      realm: String(merged.realm || "").trim(),
      instance: String(merged.instance || "").trim(),
      realm_backend: realmBackend === "sqlite" || realmBackend === "jsonl" ? realmBackend : "",
      context_root: String(merged.contextRoot || merged.context_root || "").trim(),
      state_root: String(merged.stateRoot || merged.state_root || "").trim(),
      user_config_root: String(merged.userConfigRoot || merged.user_config_root || "").trim(),
      prompt: String(merged.prompt || "").trim(),
    };
  }

  function graphSignature(instances, edges) {
    return graphSignatureFor(instances, edges, { includeLayout: true });
  }

  function graphStructureSignature(instances, edges, context = {}) {
    const options = Array.isArray(context) ? { members: context } : (context || {});
    return graphSignatureFor(instances, edges, {
      includeLayout: true,
      members: options.members,
      contract: options.contract,
    });
  }

  function graphSignatureFor(instances, edges, { includeLayout, members, contract }) {
    const nodes = (instances || [])
      .map((inst) => {
        const node = {
          id: inst.id,
          memberId: inst.memberId || null,
          isGate: !!inst.isGate,
          isTerminal: !!inst.isTerminal,
          gateKind: inst.gateKind || null,
          kind: inst.kind || null,
          label: inst.label || "",
          lane: inst.lane || "",
          launchMode: launchModeFromAuthoringSource(inst),
          collection: inst.collection || inst.collectionPolicy || inst.collection_policy || null,
          quorum: inst.quorum || null,
          controllerRole: inst.controllerRole || inst.controllerMemberId || inst.controlRole || null,
          dispatch: inst.dispatch || inst.dispatchMode || inst.dispatch_mode || null,
        };
        if (includeLayout) {
          node.col = Number(inst.col || 0);
          node.row = Number(inst.row || 0);
        }
        return node;
      })
      .sort((a, b) => a.id.localeCompare(b.id));
    const links = (edges || [])
      .map((edge) => ({
        id: edge.id,
        from: edge.from,
        to: edge.to,
        kind: edge.kind || "",
        label: edge.label || "",
        cond: edge.cond || null,
      }))
      .sort((a, b) => a.id.localeCompare(b.id));
    const projectionMembers = (members || [])
      .map((member) => ({
        id: member.id,
        name: member.name || "",
      }))
      .sort((a, b) => a.id.localeCompare(b.id));
    const draft = contract ? editorGraphDraftContract(contract) : null;
    const projectionContract = contract
      ? {
          edgeKinds: graphProjectionEdgeKinds(contract),
          fallbackEdgeLabel: draft?.fallbackEdgeLabel || "",
          branchFallbackLaneLabel: draft?.branchFallbackLaneLabel || "",
        }
      : null;
    return JSON.stringify({ nodes, links, members: projectionMembers, contract: projectionContract });
  }

  function graphIsConditionEdge(edge, edgeKinds) {
    return String(edge?.kind || "").trim() === edgeKinds.conditionKind;
  }

  function graphDraftLabelEquals(value, label) {
    const actual = String(value || "").trim().toLowerCase();
    const expected = String(label || "").trim().toLowerCase();
    return !!actual && !!expected && actual === expected;
  }

  function graphIsFallbackBranchLane(edge, node, edgeKinds, draft) {
    if (!graphIsConditionEdge(edge, edgeKinds)) return true;
    return graphDraftLabelEquals(edge?.label, draft?.fallbackEdgeLabel)
      || graphDraftLabelEquals(node?.lane, draft?.branchFallbackLaneLabel);
  }

  function graphToFlow({ instances, edges, members, previousFlow, contract }) {
    const edgeKinds = graphProjectionEdgeKinds(contract);
    const prior = previousFlow || {};
    const inputStep = (prior.steps || []).find((step) => step.type === "input") || inputStepDraft(contract, prior);
    const priorStepById = new Map();
    collectVisualSteps(prior.steps || [], (step) => {
      if (step?.id) priorStepById.set(step.id, step);
    });

    const instById = new Map((instances || []).map((inst) => [inst.id, inst]));
    const memberNodes = (instances || [])
      .filter((inst) => inst.memberId && !inst.isTerminal && !inst.isGate)
      .sort((a, b) => (Number(a.col || 0) - Number(b.col || 0)) || (Number(a.row || 0) - Number(b.row || 0)) || a.id.localeCompare(b.id));
    if (!memberNodes.length) return { ...prior, steps: [inputStep] };

    const backEdges = (edges || []).filter((edge) => {
      if (!graphIsConditionEdge(edge, edgeKinds)) return false;
      const from = instById.get(edge.from);
      const to = instById.get(edge.to);
      return from && to && Number(to.col || 0) <= Number(from.col || 0);
    });
    const forwardEdges = (edges || []).filter((edge) => !backEdges.includes(edge));
    const columnSteps = graphSegmentsToFlowSteps({
      instances,
      edges: forwardEdges,
      members: members || [],
      priorStepById,
      contract,
    });

    if (backEdges.length) {
      const back = backEdges
        .slice()
        .sort((a, b) => {
          const af = instById.get(a.from);
          const at = instById.get(a.to);
          const bf = instById.get(b.from);
          const bt = instById.get(b.to);
          const aw = Number(af?.col || 0) - Number(at?.col || 0);
          const bw = Number(bf?.col || 0) - Number(bt?.col || 0);
          return bw - aw;
        })[0];
      const from = instById.get(back.from);
      const to = instById.get(back.to);
      const firstCol = Number(to?.col || 0);
      const lastCol = Number(from?.col || 0);
      const before = columnSteps.filter((entry) => entry.col < firstCol).map((entry) => entry.step);
      const body = columnSteps.filter((entry) => entry.col >= firstCol && entry.col <= lastCol).map((entry) => entry.step);
      const after = columnSteps.filter((entry) => entry.col > lastCol).map((entry) => entry.step);
      if (body.length) {
        const previousRepeat = previousRepeatForBody(prior.steps || [], body);
        const repeat = {
          id: previousRepeat?.id || `loop_${to.id}_${from.id}`,
          type: "repeat",
          loopId: typeof previousRepeat?.loopId === "string" ? previousRepeat.loopId : "",
          maxIterations: previousRepeat && Object.prototype.hasOwnProperty.call(previousRepeat, "maxIterations")
            ? previousRepeat.maxIterations
            : null,
          iterationInput: typeof previousRepeat?.iterationInput === "string" ? previousRepeat.iterationInput : "",
          cond: repeatConditionFromEdge(back, from.id),
          steps: body,
        };
        return { ...prior, steps: [inputStep, ...before, repeat, ...after] };
      }
    }

    return { ...prior, steps: [inputStep, ...columnSteps.map((entry) => entry.step)] };
  }

  function previousRepeatForBody(steps, body) {
    const bodyIds = (body || []).map((step) => step?.id).filter(Boolean).join("|");
    let found = null;
    collectVisualSteps(steps || [], (step) => {
      if (found || step.type !== "repeat") return;
      const candidateIds = (step.steps || []).map((candidate) => candidate?.id).filter(Boolean).join("|");
      if (candidateIds === bodyIds) found = step;
    });
    return found;
  }

  function flowStepForGraphGroup(nodes, edges, members, priorStepById, edgeKinds) {
    if (nodes.length === 1) return memberStepFromInstance(nodes[0], members, priorStepById);
    const incoming = new Map();
    for (const node of nodes) {
      incoming.set(node.id, (edges || []).filter((edge) => edge.to === node.id));
    }
    const hasConditionalFanIn = nodes.some((node) => (incoming.get(node.id) || []).some((edge) => graphIsConditionEdge(edge, edgeKinds)));
    if (hasConditionalFanIn) {
      const id = `branch_${nodes.map((node) => node.id).join("_")}`;
      const prior = priorStepById.get(id) || {};
      const dependsMode = dependencyModeFromStepSource(prior);
      const out = {
        id,
        type: "branch",
        controllerRole: prior.controllerRole || prior.controllerMemberId || prior.controlRole || "",
        branches: nodes.map((node, index) => {
          const edge = (incoming.get(node.id) || []).find((candidate) => graphIsConditionEdge(candidate, edgeKinds));
          return {
            id: `br_${node.id}`,
            label: memberDisplayName(members, node.memberId) || `branch ${index + 1}`,
            condition: conditionTextFromEdge(edge, ""),
            cond: edgeConditionToEditorCond(edge),
            steps: [memberStepFromInstance(node, members, priorStepById)],
          };
        }),
        fallback: [],
      };
      if (dependsMode) out.dependsMode = dependsMode;
      return out;
    }
    const id = `parallel_${nodes.map((node) => node.id).join("_")}`;
    const prior = priorStepById.get(id) || {};
    const dependsMode = dependencyModeFromStepSource(prior);
    const out = {
      id,
      type: "parallel",
      controllerRole: prior.controllerRole || prior.controllerMemberId || prior.controlRole || "",
      dispatch: "",
      collection: "",
      branches: nodes.map((node, index) => ({
        id: `br_${node.id}`,
        label: memberDisplayName(members, node.memberId) || `lane ${index + 1}`,
        steps: [memberStepFromInstance(node, members, priorStepById)],
      })),
    };
    if (dependsMode) out.dependsMode = dependsMode;
    return out;
  }

  function graphControlDependsMode(gate, prior) {
    return dependencyModeFromStepSource(gate) || dependencyModeFromStepSource(prior);
  }

  function graphSegmentsToFlowSteps({ instances, edges, members, priorStepById, contract }) {
    const edgeKinds = graphProjectionEdgeKinds(contract);
    const draft = editorGraphDraftContract(contract) || emptyGraphDraftContract();
    const memberNodes = (instances || [])
      .filter((inst) => inst.memberId && !inst.isTerminal && !inst.isGate)
      .sort(compareGraphNodes);
    const gateNodes = (instances || []).filter((inst) => inst.isGate);
    const consumed = new Set();
    const segments = [];

    for (const gate of gateNodes.sort(compareGraphNodes)) {
      if (gate.gateKind !== "fork" && gate.gateKind !== "branch") continue;
      const branchStarts = outgoingEdges(edges, gate.id)
        .map((edge) => ({ edge, node: nodeById(instances, edge.to) }))
        .filter(({ node }) => node?.memberId);
      if (branchStarts.length < 2) continue;
      const join = findJoinForBranches(instances, edges, branchStarts.map(({ node }) => node.id));
      const lanes = branchStarts.map(({ edge, node }, index) => {
        const laneNodes = collectLaneToJoin(instances, edges, node.id, join?.id);
        laneNodes.forEach((laneNode) => consumed.add(laneNode.id));
        const isFallback = gate.gateKind === "branch"
          && graphIsFallbackBranchLane(edge, node, edgeKinds, draft);
        return {
          id: `br_${node.id}`,
          label: node.lane || memberDisplayName(members, node.memberId) || `Branch ${index + 1}`,
          isFallback,
          condition: gate.gateKind === "branch" ? conditionTextFromEdge(edge, "") : "",
          cond: gate.gateKind === "branch" ? edgeConditionToEditorCond(edge) : null,
          steps: laneNodes.map((laneNode) => memberStepFromInstance(laneNode, members, priorStepById)),
        };
      });
      const conditionalLanes = lanes.filter((lane) => !lane.isFallback);
      const fallbackSteps = lanes.filter((lane) => lane.isFallback).flatMap((lane) => lane.steps || []);
      segments.push({
        col: Number(gate.col || 0),
        spanEnd: Number(join?.col ?? gate.col ?? 0),
        step: gate.gateKind === "branch"
          ? (() => {
              const id = flowPrimitiveIdFromGate(gate, "branch");
              const prior = priorStepById.get(id) || {};
              const dependsMode = graphControlDependsMode(gate, prior);
              const out = {
                id,
                type: "branch",
                controllerRole: join?.controllerRole || join?.controllerMemberId || join?.controlRole || gate.controllerRole || gate.controllerMemberId || gate.controlRole || prior.controllerRole || prior.controllerMemberId || prior.controlRole || "",
                branches: conditionalLanes.map((lane) => ({
                  id: lane.id,
                  label: lane.label,
                  condition: lane.condition,
                  cond: lane.cond,
                  steps: lane.steps,
                })),
                fallback: fallbackSteps,
              };
              if (dependsMode) out.dependsMode = dependsMode;
              return out;
            })()
          : (() => {
              const id = flowPrimitiveIdFromGate(gate, "parallel");
              const prior = priorStepById.get(id) || {};
              const dependsMode = graphControlDependsMode(gate, prior);
              const out = {
                id,
                type: "parallel",
                controllerRole: join?.controllerRole || join?.controllerMemberId || join?.controlRole || prior.controllerRole || prior.controllerMemberId || prior.controlRole || "",
                dispatch: dispatchFromFork(gate, prior),
                collection: collectionFromJoin(join),
                quorum: join?.quorum?.n,
                branches: lanes.map((lane) => ({ id: lane.id, label: lane.label, steps: lane.steps })),
              };
              if (dependsMode) out.dependsMode = dependsMode;
              return out;
            })(),
      });
    }

    const groups = [];
    for (const inst of memberNodes) {
      if (consumed.has(inst.id)) continue;
      if (segments.some((segment) => Number(inst.col || 0) >= segment.col && Number(inst.col || 0) <= segment.spanEnd)) continue;
      const col = Number(inst.col || 0);
      let group = groups.find((entry) => entry.col === col);
      if (!group) {
        group = { col, nodes: [] };
        groups.push(group);
      }
      group.nodes.push(inst);
    }
    segments.push(...groups.map((group) => ({
      col: group.col,
      spanEnd: group.col,
      step: flowStepForGraphGroup(group.nodes, edges, members, priorStepById, edgeKinds),
    })));
    return segments.sort((a, b) => (a.col - b.col) || (a.spanEnd - b.spanEnd));
  }

  function compareGraphNodes(a, b) {
    return (Number(a.col || 0) - Number(b.col || 0)) || (Number(a.row || 0) - Number(b.row || 0)) || String(a.id).localeCompare(String(b.id));
  }

  function nodeById(instances, id) {
    return (instances || []).find((inst) => inst.id === id);
  }

  function outgoingEdges(edges, id) {
    return (edges || []).filter((edge) => edge.from === id);
  }

  function incomingEdges(edges, id) {
    return (edges || []).filter((edge) => edge.to === id);
  }

  function findJoinForBranches(instances, edges, branchStartIds) {
    const joins = (instances || []).filter((inst) => inst.isGate && inst.gateKind === "join").sort(compareGraphNodes);
    return joins.find((join) => {
      const incoming = incomingEdges(edges, join.id).map((edge) => edge.from);
      return branchStartIds.some((id) => incoming.includes(id) || laneReaches(instances, edges, id, join.id));
    }) || null;
  }

  function collectLaneToJoin(instances, edges, startId, joinId) {
    const out = [];
    let current = nodeById(instances, startId);
    const seen = new Set();
    while (current && current.id !== joinId && !seen.has(current.id)) {
      seen.add(current.id);
      if (current.memberId && !current.isGate && !current.isTerminal) out.push(current);
      const nextEdge = outgoingEdges(edges, current.id)
        .filter((edge) => edge.to !== joinId)
        .map((edge) => ({ edge, node: nodeById(instances, edge.to) }))
        .filter(({ node }) => node && !node.isTerminal)
        .sort((a, b) => compareGraphNodes(a.node, b.node))[0];
      if (!nextEdge) break;
      current = nextEdge.node;
    }
    return out;
  }

  function laneReaches(instances, edges, startId, targetId) {
    const queue = [startId];
    const seen = new Set();
    while (queue.length) {
      const id = queue.shift();
      if (id === targetId) return true;
      if (seen.has(id)) continue;
      seen.add(id);
      for (const edge of outgoingEdges(edges, id)) {
        const node = nodeById(instances, edge.to);
        if (node && !node.isTerminal) queue.push(node.id);
      }
    }
    return false;
  }

  function collectionFromJoin(join) {
    const rawCollection = join?.collection || join?.collectionPolicy || join?.collection_policy;
    if (rawCollection) return normalizeCollectionMode(rawCollection);
    if (join?.quorum?.mode === "NofM" || join?.quorum?.n) return "quorum";
    const label = String(join?.label || "").toLowerCase();
    if (label.includes("any")) return "any";
    return "";
  }

  function dispatchFromFork(gate, prior) {
    const raw = gate?.dispatch || gate?.dispatchMode || gate?.dispatch_mode || prior?.dispatch || prior?.dispatchMode || gate?.label || "";
    if (!String(raw || "").trim()) return "";
    return normalizeDispatchMode(raw);
  }

  function flowPrimitiveIdFromGate(gate, type) {
    const id = String(gate?.id || "").trim();
    const prefix = `g_${type}_`;
    if (id.startsWith(prefix) && id.length > prefix.length) return id.slice(prefix.length);
    return `${type}_${id || "flow"}`;
  }

  function memberStepFromInstance(inst, members, priorStepById) {
    const prior = priorStepById.get(inst.id) || {};
    const instruction = typeof prior.instruction === "string" ? prior.instruction : "";
    const collection = normalizeCollectionMode(inst.collection || inst.collectionPolicy || inst.collection_policy || prior.collection || prior.collectionPolicy || prior.collection_policy);
    const launchMode = launchModeFromAuthoringSource(inst, prior);
    const dispatchMode = normalizeDispatchMode(inst.dispatchMode || inst.dispatch_mode || prior.dispatchMode || prior.dispatch_mode);
    const dependsMode = dependencyModeFromStepSource(inst) || dependencyModeFromStepSource(prior);
    const outputFormat = normalizeOutputFormat(inst.outputFormat ?? inst.output_format ?? prior.outputFormat ?? prior.output_format);
    const out = {
      id: inst.id,
      type: "member",
      role: inst.memberId,
      instruction,
      launchMode,
      quorum: numberOrNull(inst.quorum ?? inst.collectionQuorum ?? prior.quorum ?? prior.collectionQuorum),
      timeoutMs: normalizePositiveInteger(inst.timeoutMs ?? inst.timeout_ms ?? prior.timeoutMs ?? prior.timeout_ms),
      allowedTools: normalizeStringList(inst.allowedTools || inst.allowed_tools || prior.allowedTools || prior.allowed_tools),
      blockedTools: normalizeStringList(inst.blockedTools || inst.blocked_tools || prior.blockedTools || prior.blocked_tools),
    };
    if (dispatchMode) out.dispatchMode = dispatchMode;
    if (collection) out.collection = collection;
    if (dependsMode) out.dependsMode = dependsMode;
    if (outputFormat) out.outputFormat = outputFormat;
    return out;
  }

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

  function deploySettingsForUi(deploy) {
    if (!deploy || typeof deploy !== "object") return { ...EMPTY_DEPLOY_SETTINGS };
    return {
      ...EMPTY_DEPLOY_SETTINGS,
      command: deploy.command || "",
      surface: deploy.surface || "",
      trustPolicy: deploy.trust_policy || deploy.trustPolicy || "",
      model: deploy.model || "",
      maxDuration: deploy.max_duration || deploy.maxDuration || "",
      maxToolCalls: deploy.max_tool_calls ?? deploy.maxToolCalls ?? null,
      maxTotalTokens: deploy.max_total_tokens ?? deploy.maxTotalTokens ?? null,
      isolated: deploy.isolated ?? false,
      realm: deploy.realm || "",
      instance: deploy.instance || "",
      realmBackend: deploy.realm_backend || deploy.realmBackend || "",
      contextRoot: deploy.context_root || deploy.contextRoot || "",
      stateRoot: deploy.state_root || deploy.stateRoot || "",
      userConfigRoot: deploy.user_config_root || deploy.userConfigRoot || "",
      prompt: deploy.prompt || "",
    };
  }

  function deployDefaultsFromSchema(schema) {
    return deploySettingsForUi(schema?.deploy_settings?.defaults);
  }

  function modelCatalogFromCatalogs(schema) {
    return (schema?.models || [])
      .filter((model) => model && typeof model === "object" && model.id && model.label && (model.vendor || model.provider))
      .map((model) => ({
        id: String(model.id),
        label: String(model.label),
        vendor: String(model.vendor || model.provider),
        ...(model.deployability ? { deployability: model.deployability } : {}),
        ...(model.provenance ? { provenance: model.provenance } : {}),
        profile: model.profile || null,
      }));
  }

  function toolCatalogFromCatalogs(schema) {
    return (Array.isArray(schema?.tool_catalog) ? schema.tool_catalog : [])
      .filter((tool) => tool && typeof tool === "object" && tool.id && tool.label && tool.desc && tool.kind && tool.source)
      .map((tool) => ({
        id: String(tool.id),
        label: String(tool.label),
        desc: String(tool.desc),
        kind: String(tool.kind),
        source: String(tool.source),
        tagClass: String(tool.tag_class || ""),
        raw: tool,
      }));
  }

  function emptyMobKitCatalogs(boot = {}) {
    return {
      models: [],
      toolCatalog: [],
      agentDefinitions: [],
      sampleAgentDefinitions: [],
      skillRealms: [],
      blankMobpack: null,
      catalogSnapshot: null,
      deployDefaults: deployDefaultsFromSchema(null),
      mobDefaults: mobDefaultsFromSchema(null),
      mobDefinition: null,
      sourceView: null,
      agentView: null,
      newFlowView: null,
      flowRegistryView: null,
      agentDetailView: null,
      agentAccessView: null,
      deployView: null,
      settingsView: null,
      launchView: null,
      schemaView: null,
      basicView: null,
      graphView: null,
      graphTemplateView: null,
      conditionView: null,
      errorView: null,
      authoringOperations: {},
      runtimeFlows: [],
      validationSource: "",
      contractMeta: {
        loaded: false,
        schemaVersion: "",
        mediaType: "",
        validationSource: "",
      },
      grid: boot.grid || null,
      cellXY: boot.cellXY || null,
      template: null,
    };
  }

  function mobKitCatalogsFromSchema(schema, boot = {}, catalogPayload = null) {
    const catalogSource = catalogPayload && typeof catalogPayload === "object" ? catalogPayload : {};
    const agentDefinitions = agentDefinitionsFromCatalogs(catalogSource);
    const sampleAgentDefinitions = sampleAgentDefinitionsFromCatalogs(catalogSource);
    const blankMobpack = blankMobpackFromCatalogs(catalogSource);
    return {
      models: modelCatalogFromCatalogs(catalogSource),
      toolCatalog: toolCatalogFromCatalogs(catalogSource),
      agentDefinitions,
      sampleAgentDefinitions,
      runtimeFlows: flowRegistryRowsFromBackend(catalogSource.runtime_flows),
      skillRealms: skillRealmsFromCatalogs(catalogSource),
      blankMobpack,
      catalogSnapshot: catalogSource.catalog_snapshot || null,
      deployDefaults: deployDefaultsFromSchema(schema),
      mobDefaults: mobDefaultsFromSchema(schema),
      mobDefinition: schema?.mob_definition || null,
      sourceView: sourceViewFromSchema(schema),
      agentView: agentViewFromSchema(schema),
      newFlowView: newFlowViewFromSchema(schema),
      flowRegistryView: flowRegistryViewFromSchema(schema),
      agentDetailView: agentDetailViewFromSchema(schema),
      agentAccessView: agentAccessViewFromSchema(schema),
      deployView: deployViewFromSchema(schema),
      settingsView: settingsViewFromSchema(schema),
      launchView: launchViewFromSchema(schema),
      schemaView: schemaViewFromSchema(schema),
      basicView: basicViewFromSchema(schema),
      graphView: graphViewFromSchema(schema),
      graphTemplateView: graphTemplateViewFromSchema(schema),
      conditionView: conditionViewFromSchema(schema),
      errorView: errorViewFromSchema(schema),
      authoringOperations: authoringOperationsFromSchema(schema),
      validationSource: schema?.validation_source || "",
      contractMeta: {
        loaded: true,
        schemaVersion: schema?.schema_version || "",
        mediaType: schema?.media_type || "",
        validationSource: schema?.validation_source || "",
      },
      grid: boot.grid || null,
      cellXY: boot.cellXY || null,
      template: graphTemplateSeedFromBlankMobpack(blankMobpack),
    };
  }

  function skillRealmsFromCatalogs(schema) {
    const skillRealms = schema?.skill_realms || [];
    return Array.isArray(skillRealms) ? skillRealms : [];
  }

  function mergeSkillRealms(documentRealms, contractRealms) {
    const merged = [];
    const seenSkillIds = new Set();
    for (const realm of [...(documentRealms || []), ...(contractRealms || [])]) {
      if (!realm || typeof realm !== "object") continue;
      const id = String(realm.id || realm.label || "").trim();
      if (!id) continue;
      const uniqueSkills = [];
      for (const skill of realm.skills || []) {
        const skillId = String(skill?.id || "").trim();
        if (!skillId || seenSkillIds.has(skillId)) continue;
        seenSkillIds.add(skillId);
        uniqueSkills.push(skill);
      }
      const existing = merged.find((candidate) => candidate.id === id);
      if (existing) {
        existing.skills = [...(existing.skills || []), ...uniqueSkills];
        continue;
      }
      if (!uniqueSkills.length) continue;
      merged.push({
        ...realm,
        id,
        skills: uniqueSkills,
        default: merged.length === 0 ? !!realm.default : false,
      });
    }
    return merged;
  }

  function catalogSkillRealmsPatch(catalogs, skillRealms) {
    return {
      ...(catalogs || {}),
      skillRealms: Array.isArray(skillRealms) ? skillRealms : [],
    };
  }

  function flowFromHydratedDocument(document) {
    if (document?.flow && typeof document.flow === "object" && Array.isArray(document.flow.steps)) {
      return document.flow;
    }
    return null;
  }

  function graphProjectionForDocument(document, members, contract) {
    const storedFrames = Array.isArray(document?.frames) ? document.frames : [];
    return {
      instances: Array.isArray(document?.instances) ? document.instances : [],
      edges: Array.isArray(document?.edges) ? document.edges : [],
      frames: storedFrames,
    };
  }

  function graphProjectionFromMobKitResult(result) {
    const source = result?.graph_projection || result?.graphProjection || result;
    if (!source || typeof source !== "object") return null;
    if (!Array.isArray(source.instances) || !Array.isArray(source.edges) || !Array.isArray(source.frames)) return null;
    return {
      instances: source.instances,
      edges: source.edges,
      frames: source.frames,
      source: String(source.source || ""),
      validation: source.validation || null,
    };
  }

  function hydrateMobpackDocumentState(result, options = {}) {
    const document = result?.document && typeof result.document === "object" ? result.document : {};
    const members = Array.isArray(document.members) ? document.members : [];
    const schemas = Array.isArray(document.schemas) ? document.schemas : [];
    const id = String(options.id || flowImportedIdFromDocument(document, result, options.existingRows)).trim();
    const flow = flowFromHydratedDocument(document);
    const errorView = errorViewForState(options.errorView);
    if (!flow) {
      return {
        ok: false,
        id,
        document,
        members,
        schemas,
        flow: null,
        skillRealms: mergeSkillRealms(document.skill_realms, options.contractSkillRealms || []),
        graphProjection: null,
        deploySettings: deploySettingsForUi(options.deployDefaults),
        mobSettings: mobSettingsForUi(options.mobDefaults),
        registryRow: null,
        addToRegistry: false,
        openEditor: false,
        validation: null,
        validationRows: [{
          kind: "crit",
          glyph: errorView.criticalGlyph,
          head: errorView.missingEditorFlowHead,
          sub: errorView.missingEditorFlowSub,
          meta: errorView.missingEditorFlowMeta,
        }],
        stage: "draft",
        error: errorView.missingEditorFlowMeta,
      };
    }
    const skillRealms = mergeSkillRealms(document.skill_realms, options.contractSkillRealms || []);
    const graphProjection = graphProjectionFromMobKitResult(result)
      || graphProjectionForDocument({ ...document, flow }, members, options.contract);
    const hasDeploySettings = document.deploy && typeof document.deploy === "object" && !Array.isArray(document.deploy);
    const hasMobSettings = document.mob_settings && typeof document.mob_settings === "object" && !Array.isArray(document.mob_settings);
    const validation = result?.validation || null;
    const validationRows = diagnosticsToRows(validation);
    const stage = validation?.ok ? "valid" : "draft";
    const registryRow = flowRegistryRowFromDocument({
      id,
      document,
      validation,
      stage,
      sourceLabel: result?.source_label || "",
      source: result?.source || "",
      flowRow: options.flowRow || null,
    });
    return {
      id,
      document,
      members,
      schemas,
      flow,
      skillRealms,
      graphProjection,
      deploySettings: deploySettingsForUi(hasDeploySettings ? document.deploy : options.deployDefaults),
      mobSettings: mobSettingsForUi(hasMobSettings ? document.mob_settings : options.mobDefaults),
      registryRow,
      addToRegistry: options.addToRegistry !== false,
      openEditor: options.openEditor !== false,
      validation,
      validationRows,
      stage,
    };
  }

  function authoringProjectionFromMobKitDocument(document, options = {}) {
    const source = document && typeof document === "object" ? document : {};
    const flow = flowFromHydratedDocument(source) || emptyAuthoringFlowState();
    return {
      document: source,
      flow,
      members: Array.isArray(source.members) ? source.members : [],
      schemas: Array.isArray(source.schemas) ? source.schemas : [],
      skillRealms: Array.isArray(source.skill_realms) ? source.skill_realms : [],
      instances: Array.isArray(source.instances) ? source.instances : [],
      edges: Array.isArray(source.edges) ? source.edges : [],
      frames: Array.isArray(source.frames) ? source.frames : [],
      deploySettings: deploySettingsForUi(source.deploy || options.deployDefaults),
      mobSettings: mobSettingsForUi(source.mob_settings || options.mobDefaults),
    };
  }

  function authoringProjectionFromOperationResult(result, options = {}) {
    const document = result?.document && typeof result.document === "object" ? result.document : null;
    if (!document) return null;
    const projection = authoringProjectionFromMobKitDocument(document, options);
    const graphProjection = graphProjectionFromMobKitResult(result);
    if (graphProjection) {
      projection.instances = graphProjection.instances;
      projection.edges = graphProjection.edges;
      projection.frames = graphProjection.frames;
    }
    return projection;
  }

  function flowImportedIdFromDocument(document, result = {}, existingRows = []) {
    const source = result?.source_name || result?.sourceName || result?.filename || result?.source;
    const name = document?.name || document?.mob_id || document?.flow?.name || source || "";
    if (!String(name || "").trim()) return "";
    return flowDraftIdFromSpec({
      name,
    }, existingRows);
  }

  function diagnosticsToRows(validation) {
    if (Array.isArray(validation?.display_rows)) {
      return apiDisplayRows(validation.display_rows);
    }
    return [];
  }

  function deployResultToRows(result) {
    if (Array.isArray(result?.display_rows)) {
      return apiDisplayRows(result.display_rows);
    }
    return [];
  }

  function apiDisplayRows(rows) {
    return (Array.isArray(rows) ? rows : [])
      .filter((row) => row && typeof row === "object")
      .map((row) => ({
        kind: String(row.kind || ""),
        glyph: String(row.glyph || ""),
        head: String(row.head || ""),
        sub: String(row.sub || ""),
        meta: String(row.meta || ""),
      }));
  }

  function validationSheetState(results, options = {}) {
    const view = deployViewForState(options.deployView);
    const rows = Array.isArray(results) ? results : [];
    const counts = rows.reduce((acc, row) => {
      const kind = row?.kind || "warn";
      if (kind === "ok") acc.ok += 1;
      else if (kind === "crit") acc.crit += 1;
      else acc.warn += 1;
      return acc;
    }, { ok: 0, warn: 0, crit: 0 });
    const stage = String(options.stage || "").trim();
    const stageBlocksActions = !!stage && stage !== "valid";
    const actionsDisabled = counts.crit > 0 || stageBlocksActions;
    const deployExecuteAllowed = options.capabilities?.authoring_capabilities?.deploy_execute_allowed !== false;
    return {
      rows,
      counts,
      eyebrow: view.validationEyebrow,
      title: `${counts.ok} ${view.validationPassedLabel} · ${counts.warn} ${view.validationWarningsLabel} · ${counts.crit} ${view.validationBlockingLabel}`,
      publishLabel: view.publishLabel,
      deployPlanLabel: view.deployPlanLabel,
      deployLabel: view.deployLabel,
      closeLabel: view.closeLabel,
      actionsDisabled,
      publishDisabled: actionsDisabled,
      deployPlanDisabled: actionsDisabled,
      deployRunDisabled: actionsDisabled || !deployExecuteAllowed,
    };
  }

  function deployPlanTraceState(document, plan, options = {}) {
    const view = deployViewForState(options.deployView);
    const steps = Array.isArray(plan?.plan_trace) && plan.plan_trace.length
      ? plan.plan_trace
      : [{
        node: null,
        head: view.planUnavailableHead,
        body: view.planUnavailableBody,
      }];
    const title = document?.mob_id || document?.name || "mobkit_flow";
    const subtitle = plan?.command || "";
    const packLabel = plan?.pack_path || "";
    return {
      steps,
      eyebrow: view.planEyebrow,
      title,
      subtitle,
      packLabel,
      firstLabel: view.planFirstLabel,
      closeLabel: view.closeLabel,
      stepLabel: view.planStepLabel,
      previousLabel: view.planPreviousLabel,
      nextLabel: view.planNextLabel,
    };
  }

  function topRailState({ contract, deploySettings, stage, view, theme, deployView, capabilities } = {}) {
    const shell = deployViewForState(deployView);
    const inEditor = view === "editor";
    const contractState = contract?.error ? shell.apiErrorLabel : contract ? shell.apiReadyLabel : shell.apiLoadingLabel;
    const deployCommand = contract?.deploy_settings?.command || "";
    const deploySurface = deploySettings?.surface || contract?.deploy_settings?.surfaces?.[0] || "";
    const deployActionsDisabled = stage !== "valid";
    const deployExecuteAllowed = capabilities?.authoring_capabilities?.deploy_execute_allowed !== false;
    const nextTheme = theme === "dark" ? "light" : "dark";
    return {
      inEditor,
      brandLabel: shell.brandLabel,
      flowsTabLabel: shell.flowsTabLabel,
      agentsTabLabel: shell.agentsTabLabel,
      mobStatusTitle: shell.mobStatusTitle,
      mobFileLabel: shell.mobFileLabel,
      contractState,
      deployPrefixLabel: shell.deployPrefixLabel,
      deployCommand,
      deploySurface,
      flowsCrumbLabel: shell.flowsCrumbLabel,
      crumbSeparator: shell.crumbSeparator,
      planTraceLabel: shell.planTraceLabel,
      importLabel: shell.importLabel,
      validateLabel: shell.validateLabel,
      publishLabel: shell.publishLabel,
      deployPlanLabel: shell.deployPlanLabel,
      deployLabel: shell.deployLabel,
      overflowLabel: shell.overflowLabel,
      settingsLabel: shell.settingsLabel,
      settingsTitle: shell.settingsTitle,
      deployActionsDisabled,
      deployRunDisabled: deployActionsDisabled || !deployExecuteAllowed,
      themeToggleTitle: `${shell.themeSwitchPrefix} ${nextTheme} ${shell.themeSwitchSuffix}`,
      themeToggleLabel: nextTheme === "light" ? shell.darkThemeLabel : shell.lightThemeLabel,
      basicModeTitle: shell.basicModeTitle,
      basicModeLabel: shell.basicModeLabel,
      graphModeTitle: shell.graphModeTitle,
      graphModeLabel: shell.graphModeLabel,
    };
  }

  function topRailNavigationTransition(currentView, target) {
    const view = String(currentView || "editor");
    switch (String(target || "")) {
      case "flows-tab":
        return { view: view === "editor" ? "flows" : "editor" };
      case "agents-tab":
        return { view: "agents" };
      case "flows-crumb":
        return { view: "flows" };
      default:
        return null;
    }
  }

  function editorModeTransition(target) {
    const editorMode = String(target || "");
    if (editorMode !== "basic" && editorMode !== "advanced") return null;
    return { editorMode };
  }

  function themeToggleTransition(currentTheme) {
    return {
      field: "theme",
      value: currentTheme === "dark" ? "light" : "dark",
    };
  }

  function validationOutcome(document, result) {
    const validation = result || null;
    return {
      document,
      validation,
      validationRows: diagnosticsToRows(validation),
      stage: validation?.ok ? "valid" : "draft",
    };
  }

  function exportOutcome(document, result, options = {}) {
    const validation = result?.validation || null;
    if (validation?.ok) {
      requireExportArchiveMetadata(result);
    }
    const publishedStage = options.publishedStage || "published";
    return {
      document,
      exportResult: result || null,
      validation,
      validationRows: diagnosticsToRows(validation),
      stage: validation?.ok ? publishedStage : "draft",
    };
  }

  function requireExportArchiveMetadata(result) {
    if (!String(result?.content_base64 || "").trim()) {
      throw new Error("mobkit/mobpacks/export did not return content_base64");
    }
    if (!String(result?.media_type || "").trim()) {
      throw new Error("mobkit/mobpacks/export did not return media_type");
    }
    if (!String(result?.filename || "").trim()) {
      throw new Error("mobkit/mobpacks/export did not return filename");
    }
  }

  function deployOutcome(document, result, options = {}) {
    const validation = result?.validation || null;
    const executing = options.execute === true;
    const deployOk =
      executing &&
      result?.executed === true &&
      result?.success === true &&
      result?.status_code === 0;
    return {
      document,
      deployResult: result || null,
      validation,
      validationRows: deployResultToRows(result),
      stage: validation?.ok && deployOk ? "deployed" : "draft",
    };
  }

  function validationSheetOpenTransition() {
    return { validate: true };
  }

  function validationSheetCloseTransition() {
    return { validate: false };
  }

  function deployPlanTraceReadyTransition(document, plan) {
    return {
      deployPlanOpen: true,
      deployPlanDocument: document || null,
      deployPlanResult: plan || null,
      incrementDeployPlanKey: true,
    };
  }

  function deployPlanTraceCloseTransition() {
    return { deployPlanOpen: false };
  }

  function apiOverlayClearTransition() {
    return {
      deployPlanOpen: false,
      validate: false,
    };
  }

  function errorMessage(error) {
    return error?.message || String(error || "");
  }

  function criticalErrorOutcome({ head, error, meta, errorView } = {}) {
    const view = errorViewForState(errorView);
    return {
      validationRows: [{
        kind: "crit",
        glyph: view.criticalGlyph,
        head: String(head || view.genericErrorHead),
        sub: errorMessage(error),
        meta: String(meta || ""),
      }],
      stage: "draft",
    };
  }

  function deployErrorOutcome(error, options = {}) {
    const view = errorViewForState(options.errorView);
    return criticalErrorOutcome({
      head: options.execute ? view.deployFailedHead : view.deployPlanFailedHead,
      error,
      meta: view.deployErrorMeta,
      errorView: view,
    });
  }

  function sourceErrorOutcome(error, options = {}) {
    const view = errorViewForState(options.errorView);
    return criticalErrorOutcome({
      head: view.sourceFailedHead,
      error,
      meta: view.sourceErrorMeta,
      errorView: view,
    });
  }

  function validationErrorOutcome(error, options = {}) {
    const view = errorViewForState(options.errorView);
    return criticalErrorOutcome({
      head: view.validationApiFailedHead,
      error,
      meta: view.rpcErrorMeta,
      errorView: view,
    });
  }

  function exportErrorOutcome(error, options = {}) {
    const view = errorViewForState(options.errorView);
    return criticalErrorOutcome({
      head: view.exportFailedHead,
      error,
      meta: view.rpcErrorMeta,
      errorView: view,
    });
  }

  function importErrorOutcome(error, options = {}) {
    const view = errorViewForState(options.errorView);
    return criticalErrorOutcome({
      head: view.importFailedHead,
      error,
      meta: options.filename || "",
      errorView: view,
    });
  }

  function sourceFileRequiresText(file) {
    const path = String(file?.path || "");
    const mediaType = String(file?.media_type || "");
    return /\.toml$/i.test(path)
      || /\.json$/i.test(path)
      || /^text\//i.test(mediaType)
      || mediaType === "application/json";
  }

  function validateSourceFileMetadata(apiSource, file, index) {
    const prefix = `${apiSource} source_files[${index}]`;
    if (!String(file?.path || "").trim()) throw new Error(`${prefix} did not return path`);
    if (!String(file?.media_type || "").trim()) throw new Error(`${prefix} did not return media_type`);
    if (!String(file?.content_base64 || "").trim()) throw new Error(`${prefix} did not return content_base64`);
    if (!String(file?.sha256 || "").trim()) throw new Error(`${prefix} did not return sha256`);
    const size = Number(file?.size_bytes);
    if (!Number.isFinite(size) || size < 0) throw new Error(`${prefix} did not return size_bytes`);
    if (sourceFileRequiresText(file) && typeof file?.text !== "string") {
      throw new Error(`${prefix} did not return text`);
    }
  }

  function sourceDocumentFromSourceResult(document, result, options = {}) {
    const apiSource = String(result?.source || "").trim();
    if (apiSource !== "mobkit/mobpacks/source") {
      throw new Error(`source preview expected mobkit/mobpacks/source but received ${apiSource}`);
    }
    const sourceView = sourceViewForState(null, options.sourceView);
    const primarySourcePath = sourceView.primarySourcePath;
    if (!primarySourcePath) throw new Error(`${apiSource} did not receive primary source path from MobKit schema`);
    const files = Array.isArray(result?.source_files) ? result.source_files : [];
    if (!files.length) throw new Error(`${apiSource} did not return source_files`);
    const primarySourceFile = files.find((file) => String(file?.path || "") === primarySourcePath);
    if (!primarySourceFile) throw new Error(`${apiSource} did not return primary source file ${primarySourcePath}`);
    const exportedSource = String(primarySourceFile.text || "").trim();
    if (!exportedSource) throw new Error(`${apiSource} did not return primary source text ${primarySourcePath}`);
    const filename = String(result?.filename || "").trim();
    if (!filename) throw new Error(`${apiSource} did not return filename`);
    const mediaType = String(result?.media_type || "").trim();
    if (!mediaType) throw new Error(`${apiSource} did not return media_type`);
    const sourceDigest = String(primarySourceFile.sha256 || "").trim();
    if (!sourceDigest) throw new Error(`${apiSource} did not return primary source sha256 ${primarySourcePath}`);
    files.forEach((file, index) => validateSourceFileMetadata(apiSource, file, index));
    const authoringDocument = document && typeof document === "object" ? document : {};
    const validation = result?.validation || null;
    const stage = validation?.ok ? "valid" : "draft";
    return {
      document: authoringDocument,
      sourceDocument: {
        ...authoringDocument,
        validation,
        filename,
        media_type: mediaType,
        sourcePath: primarySourceFile.path,
        sourceFile: primarySourceFile,
        sourceFiles: files,
        sourceDigest,
        source: apiSource,
        sourceView,
      },
      validation,
      validationRows: diagnosticsToRows(validation),
      stage,
    };
  }

  function exportDownloadPayload(result) {
    const contentBase64 = String(result?.content_base64 || "").trim();
    if (!contentBase64) throw new Error("mobkit/mobpacks/export did not return content_base64");
    const mediaType = String(result?.media_type || "").trim();
    if (!mediaType) throw new Error("mobkit/mobpacks/export did not return media_type");
    const filename = String(result?.filename || "").trim();
    if (!filename) throw new Error("mobkit/mobpacks/export did not return filename");
    return {
      contentBase64,
      mediaType,
      filename,
    };
  }

  function sourceProjectionClearTransition() {
    return {
      sourceOpen: false,
      sourceDocument: null,
      inlineSourceOpen: false,
      inlineSourceSurface: null,
      inlineSourceDocument: null,
      inlineSourceBusy: false,
    };
  }

  function sourceDrawerReadyTransition(sourceDocument) {
    return {
      sourceOpen: !!sourceDocument,
      sourceDocument: sourceDocument || null,
    };
  }

  function inlineSourcePendingTransition(surface = "basic") {
    return {
      inlineSourceOpen: true,
      inlineSourceSurface: String(surface || "basic"),
      inlineSourceBusy: true,
    };
  }

  function inlineSourceReadyTransition(sourceDocument) {
    return {
      inlineSourceDocument: sourceDocument || null,
      inlineSourceBusy: false,
    };
  }

  function inlineSourceBusyTransition(busy) {
    return { inlineSourceBusy: !!busy };
  }

  function inlineSourceToggleTransition({
    open = false,
    currentSurface = "",
    targetSurface = "basic",
  } = {}) {
    const target = String(targetSurface || "basic");
    const active = !!open && String(currentSurface || "") === target;
    return active
      ? { shouldOpen: false, patch: sourceProjectionClearTransition() }
      : { shouldOpen: true, patch: inlineSourcePendingTransition(target) };
  }

  function inlineSourceToggleButtonState({
    open = false,
    currentSurface = "",
    targetSurface = "basic",
    basicView = null,
    sourceView = null,
  } = {}) {
    const target = String(targetSurface || "basic");
    const active = !!open && String(currentSurface || "") === target;
    const basic = basicEditorViewState(basicView);
    const source = sourceViewForState(null, sourceView);
    return {
      active,
      label: active ? (source.closeLabel || basic.sourceToggleLabel) : basic.sourceToggleLabel,
    };
  }

  function inlineSourceRequestPath(request = null, options = {}) {
    const explicitPath = String(request?.sourcePath || request?.path || "").trim();
    if (explicitPath) return explicitPath;
    const graphView = graphCanvasViewState(options.graphView);
    const sourceView = sourceViewForState(null, options.sourceView);
    const requestedId = String(request?.id || "").trim();
    const requestedKind = String(request?.kind || "").trim();
    if (
      requestedId === graphView.sourceFileNodeId
      || requestedKind === graphView.sourceFileNodeKind
      || request?.isSourceFile
    ) {
      return sourceView.primarySourcePath || "mobkit/mob.toml";
    }
    return "";
  }

  function sourceFileForPath(sourceDocument, path) {
    const files = Array.isArray(sourceDocument?.sourceFiles) ? sourceDocument.sourceFiles : [];
    const selectedPath = String(path || sourceDocument?.sourcePath || sourceViewForState(sourceDocument).primarySourcePath || "").trim();
    return files.find((file) => String(file?.path || "") === selectedPath)
      || sourceDocument?.sourceFile
      || files[0]
      || null;
  }

  function sourceFileSelectionTransition(sourceDocument, path, currentPath = "") {
    const files = Array.isArray(sourceDocument?.sourceFiles) ? sourceDocument.sourceFiles : [];
    const requestedPath = String(path || "").trim();
    const requestedFile = files.find((file) => String(file?.path || "") === requestedPath) || null;
    if (requestedFile) return { sourcePath: String(requestedFile.path || "") };
    const currentFile = sourceFileForPath(sourceDocument, currentPath);
    return { sourcePath: String(currentFile?.path || "") };
  }

  function sourceFileContent(file) {
    return typeof file?.text === "string" ? file.text : "";
  }

  function sourceFileRows(sourceDocument, selectedPath) {
    const files = Array.isArray(sourceDocument?.sourceFiles) ? sourceDocument.sourceFiles : [];
    const activePath = String(selectedPath || sourceDocument?.sourcePath || "").trim();
    return files
      .filter((file) => String(file?.path || "").trim())
      .map((file) => {
        const path = String(file.path || "").trim();
        const size = Number(file.size_bytes || 0);
        const mediaType = String(file.media_type || "").trim();
        return {
          path,
          label: path,
          value: path,
          selected: path === activePath,
          className: `source-file-row${path === activePath ? " is-selected" : ""}`,
          meta: [mediaType, size > 0 ? `${size}b` : ""].filter(Boolean).join(" · "),
          file,
        };
      });
  }

  function highlightSourceFile(file) {
    const source = sourceFileContent(file);
    const path = String(file?.path || "");
    const mediaType = String(file?.media_type || "");
    if (/\.toml$/i.test(path) || mediaType === "text/toml") return highlightTomlSource(source);
    return escapeHtml(source);
  }

  function sourceEditorState(sourceDocument, options = {}) {
    const selectedFile = sourceFileForPath(sourceDocument, options.sourcePath);
    const source = selectedFile ? sourceFileContent(selectedFile) : "";
    const view = sourceViewForState(sourceDocument, options.sourceView);
    const sourcePath = String(selectedFile?.path || sourceDocument?.sourcePath || "").trim();
    const sourceLabel = [
      sourceDocument?.source || "",
      sourcePath,
      sourceDocument?.filename || "",
      sourceDocument?.media_type || "",
    ].filter(Boolean).join(" · ");
    const validationSource = sourceDocument?.validation?.validation_source || "";
    const bodyClass = options.compact ? "bld-toml__body" : "source-drawer__body";
    return {
      source,
      sourceHtml: selectedFile ? highlightSourceFile(selectedFile) : "",
      drawerEyebrow: view.drawerEyebrow,
      inlineTitle: view.inlineTitle,
      sourceLabel,
      validationSource,
      bodyClass,
      selectedPath: sourcePath,
      fileRows: sourceFileRows(sourceDocument, sourcePath),
      showLoading: !!options.busy && !source,
      loadingText: view.loadingText,
      copyLabel: view.copyLabel,
      closeLabel: view.closeLabel,
      copyDisabled: !!options.busy || !source,
    };
  }

  function highlightTomlSource(source) {
    return escapeHtml(String(source || ""))
      .replace(/^(\s*#.*)$/gm, '<span class="toml-comment">$1</span>')
      .replace(/^(\s*)(\[[^\]]+\])/gm, '$1<span class="toml-table">$2</span>')
      .replace(/^(\s*)([A-Za-z_][\w-]*)(\s*=)/gm, '$1<span class="toml-key">$2</span>$3');
  }

  function sourceViewFromSchema(schema) {
    const view = schema?.mob_definition?.editor_source_view;
    if (!view || typeof view !== "object") return null;
    const out = {
      drawerEyebrow: String(view.drawer_eyebrow || "").trim(),
      inlineTitle: String(view.inline_title || "").trim(),
      primarySourcePath: String(view.primary_source_path || "").trim(),
      loadingText: String(view.loading_text || "").trim(),
      copyLabel: String(view.copy_label || "").trim(),
      closeLabel: String(view.close_label || "").trim(),
    };
    return out.drawerEyebrow && out.inlineTitle && out.primarySourcePath && out.loadingText && out.copyLabel && out.closeLabel
      ? out
      : null;
  }

  function sourceViewForState(sourceDocument, sourceView) {
    const view = sourceView && typeof sourceView === "object"
      ? sourceView
      : sourceDocument?.sourceView;
    return {
      drawerEyebrow: String(view?.drawerEyebrow || ""),
      inlineTitle: String(view?.inlineTitle || ""),
      primarySourcePath: String(view?.primarySourcePath || ""),
      loadingText: String(view?.loadingText || ""),
      copyLabel: String(view?.copyLabel || ""),
      closeLabel: String(view?.closeLabel || ""),
    };
  }

  function sampleFlowsFromCatalogs(schema) {
    return (schema?.sample_mobpacks || [])
      .filter((sample) => sample && typeof sample === "object" && sample.document)
      .map((sample) => {
        const source = typeof sample.source === "string" ? sample.source.trim() : "";
        if (!source) return null;
        const id = String(sample.id || "").trim();
        const name = String(sample.name || "").trim();
        const stage = String(sample.stage || "").trim();
        if (!id || !name || !stage) return null;
        return {
          id,
          name,
          version: String(sample.version || sample.document?.schema_version || ""),
          stage,
          trigger: String(sample.trigger || source),
          source,
          document: sample.document,
          validation: sample.validation || null,
          ...(sample.deployability ? { deployability: sample.deployability } : {}),
          ...(sample.provenance ? { provenance: sample.provenance } : {}),
        };
      })
      .filter(Boolean);
  }

  function flowCatalogBootstrapState(catalogPayload, options = {}) {
    const sampleFlows = sampleFlowsFromCatalogs(catalogPayload);
    const registryFlows = flowRegistryRowsFromBackend(options.registryRows || options.registryResult?.rows);
    const runtimeFlows = flowRegistryRowsFromBackend(catalogPayload?.runtime_flows);
    const existingIds = new Set(runtimeFlows.map((row) => row.id));
    const flows = [
      ...runtimeFlows,
      ...registryFlows.filter((row) => !existingIds.has(row.id)),
    ];
    const first = flows[0] || null;
    return {
      templates: sampleFlows,
      flows,
      initialHydration: first
        ? {
          result: {
            document: first.document,
            validation: first.validation ?? null,
          },
          options: {
            id: first.id,
            flowRow: first,
            addToRegistry: false,
            openEditor: !!options.openEditor,
            deployDefaults: options.deployDefaults,
            mobDefaults: options.mobDefaults,
          },
        }
        : null,
    };
  }

  function flowRegistryRowsFromBackend(rows = []) {
    return (Array.isArray(rows) ? rows : [])
      .map((row) => {
        if (!row || typeof row !== "object" || !row.document) return null;
        return flowRegistryRowFromDocument({
          id: row.id,
          document: row.document,
          validation: row.validation ?? null,
          stage: row.stage,
          trigger: row.trigger,
          source: row.source,
          flowRow: row,
        });
      })
      .filter(Boolean);
  }

  function blankMobpackFromCatalogs(schema) {
    const blank = schema?.blank_mobpack;
    if (!blank || typeof blank !== "object" || !blank.document) return null;
    const source = typeof blank.source === "string" ? blank.source.trim() : "";
    const id = String(blank.id || "").trim();
    const name = String(blank.name || "").trim();
    const stage = String(blank.stage || "").trim();
    if (!id || !name || !source || !stage) return null;
    return {
      id,
      name,
      version: String(blank.version || blank.document?.schema_version || ""),
      stage,
      trigger: String(blank.trigger || source),
      source,
      document: blank.document,
      validation: blank.validation || null,
      ...(blank.deployability ? { deployability: blank.deployability } : {}),
      ...(blank.provenance ? { provenance: blank.provenance } : {}),
    };
  }

  function graphTemplateSeedFromBlankMobpack(blankMobpack) {
    if (!blankMobpack || typeof blankMobpack !== "object") return null;
    const name = String(blankMobpack.name || "").trim();
    const repo = String(blankMobpack.source || "").trim();
    const version = String(blankMobpack.version || "").trim();
    const trigger = String(blankMobpack.trigger || "").trim();
    if (!name || !repo || !version) return null;
    return {
      name,
      repo,
      version,
      triggers: {
        labels: trigger ? [trigger] : [],
        default: false,
      },
    };
  }

  function flowRegistryMarkDraftPatch(rows, currentFlowId) {
    const list = Array.isArray(rows) ? rows : [];
    if (!currentFlowId) return list;
    let changed = false;
    const next = list.map((row) => {
      if (!row || row.id !== currentFlowId) return row;
      if (row.stage === "draft" && row.validation == null) return row;
      changed = true;
      return { ...row, stage: "draft", validation: null };
    });
    return changed ? next : list;
  }

  function flowRegistryViewState(rows, currentFlowId, options = {}) {
    const list = Array.isArray(rows) ? rows : [];
    const view = flowRegistryViewForState(options.flowRegistryView);
    const suffix = list.length === 1 ? view.titleSingularSuffix : view.titlePluralSuffix;
    return {
      eyebrow: view.eyebrow,
      title: `${list.length} ${suffix}`.trim(),
      createLabel: view.createLabel,
      createDisabled: !options.canCreate,
      createTitle: options.canCreate ? view.createReadyTitle : view.createUnavailableTitle,
      columns: view.columns,
      rows: list.map((row) => {
        const id = String(row?.id || "");
        const stage = String(row?.stage || "draft");
        return {
          id,
          className: "flows-list__row" + (id && id === currentFlowId ? " is-current" : ""),
          name: String(row?.name || ""),
          trigger: String(row?.trigger || ""),
          version: String(row?.version || ""),
          stage,
        };
      }),
    };
  }

  function flowRegistrySelectionState(rows, id) {
    const selectedId = String(id || "");
    const row = (Array.isArray(rows) ? rows : []).find((candidate) => candidate?.id === selectedId) || null;
    if (!row) {
      return {
        found: false,
        id: selectedId,
        row: null,
        hasDocument: false,
        hydration: null,
        fallback: null,
      };
    }
    if (row.document && typeof row.document === "object") {
      return {
      found: true,
      id: selectedId,
      row,
      hasDocument: true,
      hydration: {
        result: {
          document: row.document,
          validation: row.validation ?? null,
        },
        options: {
          id: selectedId,
          flowRow: row,
          addToRegistry: false,
        },
      },
      fallback: null,
    };
    }
    return {
      found: true,
      id: selectedId,
      row,
      hasDocument: false,
      hydration: null,
      fallback: null,
      error: "missing_registry_document",
    };
  }

  function flowRegistryRowFromDocument({
    id,
    document,
    validation = null,
    stage,
    trigger = "",
    source = "",
    sourceLabel = "",
    flowRow = null,
    fallbackName = "",
    fallbackVersion = "",
  } = {}) {
    const rowId = String(id || "").trim();
    if (!rowId || !document || typeof document !== "object") return null;
    const name = String(
      flowRow?.name ||
      document.name ||
      document.flow?.name ||
      document.mob_id ||
      fallbackName
    );
    const version = String(flowRow?.version || document.schema_version || fallbackVersion);
    return {
      id: rowId,
      name,
      version,
      stage: String(stage || (validation?.ok ? "valid" : "draft")),
      trigger: String(flowRow?.trigger || trigger || sourceLabel || ""),
      source: String(flowRow?.source || source || ""),
      document,
      validation: validation ?? null,
      ...(flowRow?.registry_source ? { registry_source: String(flowRow.registry_source) } : {}),
      ...(flowRow?.document_kind ? { document_kind: String(flowRow.document_kind) } : {}),
      ...(flowRow?.runtime_projection === true ? { runtime_projection: true } : {}),
      ...(flowRow?.runtime_mob_id ? { runtime_mob_id: String(flowRow.runtime_mob_id) } : {}),
      ...(flowRow?.runtime_flow_id ? { runtime_flow_id: String(flowRow.runtime_flow_id) } : {}),
      ...(flowRow?.deployability ? { deployability: flowRow.deployability } : {}),
      ...(flowRow?.provenance ? { provenance: flowRow.provenance } : {}),
    };
  }

  function flowRegistryRowIsRuntimeProjection(row) {
    return row?.runtime_projection === true
      || row?.document_kind === "runtime_projection"
      || row?.source === "mobkit/runtime/flow_projection"
      || row?.registry_source === "mobkit/runtime/flow_projection";
  }

  function flowRegistryRememberDocumentPatch(rows, {
    currentFlowId,
    document,
    validation = null,
    stage = "draft",
  } = {}) {
    const list = Array.isArray(rows) ? rows : [];
    if (!currentFlowId || !document || typeof document !== "object") return list;
    let changed = false;
    const next = list.map((row) => {
      if (!row || row.id !== currentFlowId) return row;
      changed = true;
      return {
        ...row,
        name: document.name || document.flow?.name || row.name,
        version: document.schema_version || row.version,
        stage,
        document,
        validation: validation ?? null,
      };
    });
    return changed ? next : list;
  }

  function flowRegistryRowRevision(row) {
    const value = row?.revision ?? row?.draft_revision;
    const revision = Number(value);
    return Number.isFinite(revision) && revision >= 0 ? revision : null;
  }

  function flowRegistryRowEtag(row) {
    const value = row?.draft_etag ?? row?.etag;
    return value ? String(value) : "";
  }

  function flowRegistryDraftGuard(row, currentFlowId = "") {
    const id = String(row?.id || currentFlowId || "").trim();
    const expectedRevision = flowRegistryRowRevision(row);
    const expectedEtag = flowRegistryRowEtag(row);
    if (!id || expectedRevision === null) return {};
    return {
      id,
      expected_revision: expectedRevision,
      ...(expectedEtag ? { expected_etag: expectedEtag } : {}),
    };
  }

  function flowRegistryDocumentPersistence({
    currentFlowId,
    document,
    validation = null,
    stage = "draft",
    previousSignature = "",
    skipIfUnchanged = false,
    expectedRevision = null,
    expectedEtag = "",
  } = {}) {
    if (!currentFlowId || !document || typeof document !== "object") {
      return {
        ok: false,
        changed: false,
        signature: String(previousSignature || ""),
        rowPatch: null,
      };
    }
    const signature = `${currentFlowId}\n${JSON.stringify(document)}`;
    if (skipIfUnchanged && signature === previousSignature) {
      return {
        ok: true,
        changed: false,
        signature,
        rowPatch: null,
      };
    }
    const nextStage = validation == null && stage === "published" ? "draft" : stage;
    return {
      ok: true,
      changed: true,
      signature,
      rowPatch: {
        currentFlowId,
        document,
        validation,
        stage: nextStage,
        ...(expectedRevision !== null && expectedRevision !== undefined ? { expectedRevision } : {}),
        ...(expectedEtag ? { expectedEtag } : {}),
      },
    };
  }

  function flowRegistryPersistDocumentProjection(rows, options = {}) {
    const sourceRows = Array.isArray(rows) ? rows : [];
    const currentRow = sourceRows.find((row) => row?.id === options.currentFlowId) || null;
    if (flowRegistryRowIsRuntimeProjection(currentRow)) {
      return {
        ok: false,
        changed: false,
        reason: "runtime_projection_read_only",
        signature: String(options.previousSignature || ""),
        rowPatch: null,
        rows: sourceRows,
      };
    }
    const persistence = flowRegistryDocumentPersistence({
      expectedRevision: flowRegistryRowRevision(currentRow),
      expectedEtag: flowRegistryRowEtag(currentRow),
      ...options,
    });
    if (!persistence.ok || !persistence.rowPatch) {
      return {
        ...persistence,
        rows: sourceRows,
      };
    }
    return {
      ...persistence,
      rows: flowRegistryRememberDocumentPatch(sourceRows, persistence.rowPatch),
    };
  }

  function flowRegistryPersistOutcomeProjection(rows, { currentFlowId, outcome, previousSignature = "", skipIfUnchanged = false } = {}) {
    const sourceOutcome = outcome && typeof outcome === "object" ? outcome : {};
    const persistence = flowRegistryPersistDocumentProjection(rows, {
      currentFlowId,
      document: sourceOutcome.document,
      validation: sourceOutcome.validation,
      stage: sourceOutcome.stage,
      previousSignature,
      skipIfUnchanged,
    });
    return {
      ...sourceOutcome,
      persistence,
      rows: persistence.rows,
      signature: persistence.signature,
      changed: persistence.changed,
      ok: persistence.ok,
    };
  }

  function flowRegistryAppendRowPatch(rows, row) {
    const list = Array.isArray(rows) ? rows : [];
    if (!row || typeof row !== "object" || !row.id) return list;
    return [...list, row];
  }

  function flowRegistryUpsertRowPatch(rows, row) {
    const list = Array.isArray(rows) ? rows : [];
    if (!row || typeof row !== "object" || !row.id) return list;
    let replaced = false;
    const next = list.map((candidate) => {
      if (!candidate || candidate.id !== row.id) return candidate;
      replaced = true;
      return row;
    });
    return replaced ? next : [...list, row];
  }

  function flowDraftIdFromSpec(spec, existingRows = []) {
    const draftSpec = spec && typeof spec === "object" ? spec : {};
    const base = slug(draftSpec.name || draftSpec.template || "mobkit_flow", "mobkit_flow");
    const prefix = base.startsWith("f_") ? base : `f_${base}`;
    const used = new Set((Array.isArray(existingRows) ? existingRows : [])
      .map((row) => String(row?.id || "").trim())
      .filter(Boolean));
    if (!used.has(prefix)) return prefix;
    let index = 2;
    while (used.has(`${prefix}_${index}`)) index += 1;
    return `${prefix}_${index}`;
  }

  function newFlowTemplateOptions(templates = [], { canCreateBlank = false, blankTemplate = null } = {}) {
    const hasBlankDocument = !!blankTemplate?.document;
    const options = [{
      id: "blank",
      label: hasBlankDocument ? String(blankTemplate.name || "") : "Blank",
      sub: hasBlankDocument
        ? String(blankTemplate.trigger || blankTemplate.source || "")
        : "Waiting for MobKit blank mobpack",
      tier: hasBlankDocument ? String(blankTemplate.stage || "") : "",
      disabled: !canCreateBlank || !hasBlankDocument,
    }];
    for (const sample of Array.isArray(templates) ? templates : []) {
      if (!sample || typeof sample !== "object") continue;
      const id = String(sample.id || "").trim();
      const label = String(sample.name || "").trim();
      if (!id || !label) continue;
      options.push({
        id,
        label,
        sub: String(sample.trigger || sample.source || ""),
        tier: String(sample.stage || ""),
        disabled: false,
      });
    }
    return options;
  }

  function newFlowInitialState({ blankTemplate = null } = {}) {
    const hasBlankDocument = !!blankTemplate?.document;
    return {
      step: 1,
      name: "",
      trigger: hasBlankDocument ? String(blankTemplate.trigger || "") : "",
      template: hasBlankDocument ? String(blankTemplate.id || "") : "",
    };
  }

  function newFlowModalState(state = {}, templateOptions = [], newFlowView = null) {
    const view = newFlowViewForState(newFlowView);
    const step = Number(state.step || 1);
    const name = String(state.name || "");
    const trigger = String(state.trigger || "");
    const template = String(state.template || "");
    const options = (Array.isArray(templateOptions) ? templateOptions : []).map((option) => {
      const id = String(option?.id || "");
      return {
        id,
        label: String(option?.label || ""),
        sub: String(option?.sub || ""),
        tier: String(option?.tier || ""),
        disabled: !!option?.disabled,
        className: "template-card" + (id && id === template ? " is-selected" : ""),
      };
    });
    const selectedTemplate = options.find((option) => option.id === template) || null;
    return {
      step,
      eyebrow: view.eyebrowTemplate.replace("{step}", String(step)),
      closeLabel: view.closeLabel,
      nameLabel: view.nameLabel,
      namePlaceholder: view.namePlaceholder,
      triggerLabel: view.triggerLabel,
      triggerPlaceholder: view.triggerPlaceholder,
      startFromLabel: view.startFromLabel,
      backLabel: view.backLabel,
      nextLabel: view.nextLabel,
      createLabel: view.createLabel,
      name,
      trigger,
      template,
      options,
      createDisabled: !selectedTemplate || !!selectedTemplate.disabled,
      nextDisabled: !name.trim(),
    };
  }

  function newFlowModalPatch(state = {}, patch = {}) {
    const source = state && typeof state === "object" ? state : {};
    const rawPatch = patch && typeof patch === "object" ? patch : {};
    const next = { ...source, ...rawPatch };
    const step = Number(next.step || 1);
    next.step = step === 2 ? 2 : 1;
    next.name = String(next.name || "");
    next.trigger = String(next.trigger || "");
    next.template = String(next.template || "");
    return next;
  }

  function newFlowModalFieldPatch(state = {}, field, value) {
    const key = String(field || "").trim();
    if (!key) return newFlowModalPatch(state);
    if (!["name", "trigger", "template"].includes(key)) return newFlowModalPatch(state);
    return newFlowModalPatch(state, { [key]: value });
  }

  function newFlowModalStepPatch(state = {}, step) {
    return newFlowModalPatch(state, { step });
  }

  function newFlowModalCreateSpec(state = {}) {
    const source = newFlowModalPatch(state);
    return {
      name: source.name,
      trigger: source.trigger,
      template: source.template,
    };
  }

  function agentDefinitionsFromCatalogs(schema) {
    const definitions = Array.isArray(schema?.agent_definitions) ? schema.agent_definitions : [];
    return normalizeAgentDefinitionsFromCatalog(definitions);
  }

  function sampleAgentDefinitionsFromCatalogs(schema) {
    const definitions = Array.isArray(schema?.sample_agent_definitions) ? schema.sample_agent_definitions : [];
    return normalizeAgentDefinitionsFromCatalog(definitions);
  }

  function normalizeAgentDefinitionsFromCatalog(definitions) {
    return definitions
      .filter((template) => template && typeof template === "object")
      .filter((template) => String(template.definitionType || template.definition_type || "") === "mobkit/profile-member")
      .filter((template) => String(template.source || "").trim())
      .filter((template) => String(template.sourceMobpack || template.source_mobpack || "").trim())
      .filter((template) => String(template.sourceOrigin || template.source_origin || "").trim())
      .filter((template) => String(template.profileBinding || template.profile_binding || "").trim())
      .filter((template) => String(template.runtimeMode || template.runtime_mode || "").trim())
      .filter((template) => String(template.model || "").trim())
      .map((template) => {
        const id = String(template.id || "").trim();
        const role = String(template.role || "").trim();
        const name = String(template.name || template.label || "").trim();
        const model = String(template.model || "").trim();
        const definitionKind = String(template.definitionKind || template.definition_kind || "").trim();
        const sourceKind = String(template.sourceKind || template.source_kind || "").trim();
        if (!id || !role || !name) return null;
        return {
          id,
          role,
          label: String(template.label || name),
          name,
          model,
          schema: String(template.schema || ""),
          schemaDefinition: normalizeAgentSchemaDefinition(template.schemaDefinition || template.schema_definition),
          schemaSourceDocumentPath: String(template.schemaSourceDocumentPath || template.schema_source_document_path || ""),
          skills: Array.isArray(template.skills) ? [...template.skills] : [],
          skillDefinitions: normalizeAgentDefinitionRows(template.skillDefinitions || template.skill_definitions),
          tools: Array.isArray(template.tools) ? [...template.tools] : [],
          toolDefinitions: normalizeAgentDefinitionRows(template.toolDefinitions || template.tool_definitions),
          profileBinding: String(template.profileBinding || template.profile_binding || ""),
          realmProfile: String(template.realmProfile || template.realm_profile || ""),
          runtimeMode: String(template.runtimeMode || template.runtime_mode || ""),
          externalAddressable: !!template.externalAddressable,
          backend: normalizeProfileBackend(template.backend),
          maxInlinePeerNotifications: normalizeMaxInlinePeerNotifications(template.maxInlinePeerNotifications ?? template.max_inline_peer_notifications),
          systemPrompt: String(template.systemPrompt || template.system_prompt || ""),
          providerParams: normalizeProviderParams(template.providerParams || template.provider_params),
          definitionType: String(template.definitionType || template.definition_type),
          ...(definitionKind ? { definitionKind } : {}),
          ...(sourceKind ? { sourceKind } : {}),
          source: template.source || "",
          sourceMobpack: template.sourceMobpack || template.source_mobpack || "",
          sourceMobpackName: template.sourceMobpackName || template.source_mobpack_name || "",
          sourceOrigin: template.sourceOrigin || template.source_origin || "",
          sourceDocumentPath: template.sourceDocumentPath || template.source_document_path || "",
          ...(template.deployability ? { deployability: template.deployability } : {}),
          ...(template.provenance ? { provenance: template.provenance } : {}),
        };
      })
      .filter(Boolean);
  }

  function normalizeAgentSchemaDefinition(value) {
    if (!value || typeof value !== "object" || Array.isArray(value)) return null;
    const id = String(value.id || "").trim();
    const fields = Array.isArray(value.fields) ? value.fields : [];
    if (!id || !fields.length) return null;
    return JSON.parse(JSON.stringify(value));
  }

  function normalizeAgentDefinitionRows(value) {
    if (!Array.isArray(value)) return [];
    return value
      .filter((row) => row && typeof row === "object" && !Array.isArray(row))
      .map((row) => JSON.parse(JSON.stringify(row)));
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
