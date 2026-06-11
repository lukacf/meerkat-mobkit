/* global window, fetch */
// MobKit Flow Editor controller plane.
// Keeps deployable document generation and API calls outside the visual JSX.

(function () {
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
