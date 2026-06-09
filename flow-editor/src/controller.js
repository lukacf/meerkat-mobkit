/* global window, fetch */
// MobKit Flow Editor controller plane.
// Keeps deployable document generation and API calls outside the visual JSX.

(function () {
  const SCHEMA_VERSION = "0.1.0";
  const RPC_METHODS = {
    schema: "mobkit/mobpacks/schema",
    catalogs: "mobkit/mobpacks/catalogs",
    validate: "mobkit/mobpacks/validate",
    source: "mobkit/mobpacks/source",
    export: "mobkit/mobpacks/export",
    import: "mobkit/mobpacks/import",
    deployCommand: "mobkit/mobpacks/deploy_command",
    deploy: "mobkit/mobpacks/deploy",
  };
  const SCHEMA_COMMAND_KEYS = {
    schema: "schema",
    catalogs: "catalogs",
    validate: "validate",
    source: "source",
    export: "export",
    import: "import",
    deployCommand: "deploy_command",
    deploy: "deploy_rpc",
  };
  const EMPTY_DEPLOY_SETTINGS = {
    command: "",
    surface: "",
    trustPolicy: "",
    model: "",
    maxDuration: "",
    maxToolCalls: null,
    maxTotalTokens: null,
    isolated: false,
    realm: "",
    instance: "",
    realmBackend: "",
    contextRoot: "",
    stateRoot: "",
    userConfigRoot: "",
    prompt: "",
  };
  const EMPTY_MOB_SETTINGS = {
    orchestrator: "",
    autoWireOrchestrator: false,
    roleWiring: [],
    backendDefault: "",
    externalAddressBase: "",
    advanced: {
      topology: null,
      supervisor: null,
      limits: null,
      spawnPolicy: null,
      eventRouter: null,
    },
  };
  const MOB_SETTINGS_PATCH_KEYS = new Set(Object.keys(EMPTY_MOB_SETTINGS));
  const controllerConfig = {
    rpcUrl: "/flow-editor/rpc",
    rpcMethods: { ...RPC_METHODS },
  };

  function configure(options) {
    const rpcUrl = String(options?.rpcUrl || "").trim();
    if (rpcUrl) {
      controllerConfig.rpcUrl = rpcUrl;
    }
  }

  function rpcPath() {
    return controllerConfig.rpcUrl || "/flow-editor/rpc";
  }

  function rpcMethod(name) {
    return controllerConfig.rpcMethods?.[name] || RPC_METHODS[name] || "";
  }

  function authoringRpcMethodsFromSchema(schema) {
    const commands = schema?.commands;
    if (!commands || typeof commands !== "object") return {};
    const out = {};
    for (const [name, commandKey] of Object.entries(SCHEMA_COMMAND_KEYS)) {
      const value = String(commands[commandKey] || "").trim();
      if (value) out[name] = value;
    }
    return out;
  }

  function configureAuthoringMethodsFromSchema(schema) {
    const methods = authoringRpcMethodsFromSchema(schema);
    controllerConfig.rpcMethods = { ...RPC_METHODS, ...methods };
    return { ...controllerConfig.rpcMethods };
  }

  let requestId = 0;
  async function callRpc(method, params) {
    const response = await fetch(rpcPath(), {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: ++requestId,
        method,
        params: params || {},
      }),
    });
    if (!response.ok) {
      throw new Error(`MobKit API ${response.status}`);
    }
    const payload = await response.json();
    if (payload.error) {
      throw new Error(payload.error.message || "MobKit API error");
    }
    return payload.result;
  }

  function slug(value, fallback) {
    const out = String(value || fallback || "mobpack")
      .toLowerCase()
      .replace(/[^a-z0-9_ -]+/g, "")
      .trim()
      .replace(/[\s-]+/g, "_")
      .replace(/^_+|_+$/g, "");
    return out || fallback || "mobpack";
  }

  function profileName(member) {
    return slug(member?.name || member?.role || member?.id || "member", "member");
  }

  function normalizeToolRef(raw, catalog) {
    const id = String(raw || "").trim();
    if (!id) return "";
    const entries = Array.isArray(catalog) ? catalog : [];
    if ((entries || []).some((tool) => tool.id === id)) return id;
    return "";
  }

  function normalizeSkillId(raw) {
    return String(raw || "")
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9_.-]+/g, ".")
      .replace(/^[._-]+|[._-]+$/g, "")
      .replace(/\.{2,}/g, ".");
  }

  function skillIdsFromRealms(realms) {
    const ids = new Set();
    for (const realm of realms || []) {
      for (const skill of realm?.skills || []) {
        if (skill?.id) ids.add(String(skill.id));
      }
    }
    return ids;
  }

  function addInlineSkillToRealms(realms, spec = {}, accessView = null) {
    const view = agentAccessViewForState(accessView);
    const nextRealms = JSON.parse(JSON.stringify(realms || []));
    const label = String(spec.label || spec.id || "").trim();
    if (!label) throw new Error(view.skillInlineMissingLabelError);
    const content = String(spec.content || "").trim();
    if (!content) throw new Error(view.skillInlineMissingContentError);
    const used = skillIdsFromRealms(nextRealms);
    const explicitId = String(spec.id || "").trim();
    const identityText = explicitId || label;
    if (!normalizeSkillId(identityText)) throw new Error(view.skillInlineInvalidIdError);
    const rawId = explicitId || (label.includes(".") ? label : `mob.${label}`);
    const baseId = normalizeSkillId(rawId);
    if (!baseId) throw new Error(view.skillInlineInvalidIdError);
    let id = baseId;
    let index = 2;
    while (used.has(id)) id = `${baseId}.${index++}`;
    let realm = nextRealms.find((candidate) => candidate?.id === view.inlineSkillRealmId);
    if (!realm) {
      realm = {
        id: view.inlineSkillRealmId,
        label: view.inlineSkillRealmLabel,
        source: "editor",
        default: nextRealms.length === 0,
        skills: [],
      };
      nextRealms.unshift(realm);
    }
    if (!Array.isArray(realm.skills)) realm.skills = [];
    realm.skills.push({
      id,
      label,
      source: "inline",
      content,
      desc: spec.desc || view.inlineSkillDefaultDescription,
    });
    return { id, skillRealms: nextRealms };
  }

  function memberToolAccessPatch(member, raw, toolCatalog, accessView = null) {
    const view = agentAccessViewForState(accessView);
    const id = normalizeToolRef(raw, toolCatalog);
    if (!id) {
      return {
        ok: false,
        id: "",
        error: raw ? view.toolInvalidError : "",
        patch: null,
      };
    }
    const tools = normalizeStringList(member?.tools);
    if (tools.includes(id)) {
      return { ok: true, id, alreadySelected: true, patch: null };
    }
    return { ok: true, id, alreadySelected: false, patch: { tools: [...tools, id] } };
  }

  function memberToolRemovePatch(member, toolId) {
    const id = String(toolId || "").trim();
    if (!id) return { ok: false, id: "", patch: null };
    const tools = normalizeStringList(member?.tools);
    return { ok: true, id, patch: { tools: tools.filter((candidate) => candidate !== id) } };
  }

  function memberToolAccessCascadePatch({ memberId, members, flow, instances } = {}, raw, toolCatalog, accessView = null) {
    const list = Array.isArray(members) ? members : [];
    const member = list.find((candidate) => candidate?.id === memberId) || null;
    if (!member) return { ok: false, error: "member not found", id: "", patch: null, members: list, flow, instances };
    const access = memberToolAccessPatch(member, raw, toolCatalog, accessView);
    if (!access.ok) return { ...access, members: list, flow, instances };
    if (!access.patch) return { ...access, members: list, member, flow, instances };
    const updated = studioUpdateMemberPatch({ members: list }, member.id, access.patch);
    if (!updated.ok) {
      return { ...access, ok: false, error: updated.error || access.error || "", members: list, flow, instances };
    }
    return {
      ...access,
      members: updated.members,
      member: updated.member,
      flow: reconcileFlowStepToolScopes(flow, updated.members),
      instances: reconcileGraphStepToolScopes(instances, updated.members),
    };
  }

  function memberToolRemoveCascadePatch({ memberId, members, flow, instances } = {}, toolId) {
    const list = Array.isArray(members) ? members : [];
    const member = list.find((candidate) => candidate?.id === memberId) || null;
    if (!member) return { ok: false, error: "member not found", id: "", patch: null, members: list, flow, instances };
    const removal = memberToolRemovePatch(member, toolId);
    if (!removal.ok) return { ...removal, members: list, flow, instances };
    const updated = studioUpdateMemberPatch({ members: list }, member.id, removal.patch || {});
    if (!updated.ok) {
      return { ...removal, ok: false, error: updated.error || "", members: list, flow, instances };
    }
    return {
      ...removal,
      members: updated.members,
      member: updated.member,
      flow: reconcileFlowStepToolScopes(flow, updated.members),
      instances: reconcileGraphStepToolScopes(instances, updated.members),
    };
  }

  function memberToolAccessState(member, toolCatalog = [], accessView = null) {
    const view = agentAccessViewForState(accessView);
    const catalog = Array.isArray(toolCatalog) ? toolCatalog.filter((tool) => tool?.id) : [];
    const metaById = new Map(catalog.map((tool) => [String(tool.id), tool]));
    const selectedTools = normalizeStringList(member?.tools);
    const selectedSet = new Set(selectedTools);
    const catalogSet = new Set(catalog.map((tool) => String(tool.id)));
    const toolRow = (id) => {
      const meta = metaById.get(id) || null;
      const unavailable = !catalogSet.has(id);
      return {
        id,
        name: id,
        unavailable,
        reason: unavailable ? view.toolInvalidError : "",
        description: unavailable ? view.toolMissingDescription : (meta?.desc || view.toolMissingDescription),
        meta,
        className: `tool-row${unavailable ? " tool-row--invalid" : ""}`,
        removeLabel: view.toolRemoveLabel,
      };
    };
    const addableRow = (tool) => {
      const id = String(tool.id);
      const label = tool.label || id;
      const desc = tool.desc || id;
      return {
        id,
        value: id,
        label,
        description: desc,
        optionLabel: `${label} — ${desc}`,
        meta: tool,
      };
    };
    return {
      selectedTools,
      title: view.toolTitle,
      hint: view.toolHint,
      rows: selectedTools.map(toolRow),
      addableRows: catalog
        .filter((tool) => !selectedSet.has(String(tool.id)))
        .map(addableRow),
      addSelectValue: "",
      addSelectPlaceholder: view.toolAddSelectPlaceholder,
      sourceLabel: view.toolSourceLabel,
      sourcePlaceholder: view.toolSourcePlaceholder,
      addButtonLabel: view.toolAddButtonLabel,
    };
  }

  function stepToolScopeState({ member, selected, mode = "member", toolCatalog = [], basicView = null } = {}) {
    const view = basicEditorViewState(basicView);
    const catalog = Array.isArray(toolCatalog) ? toolCatalog.filter((tool) => tool?.id) : [];
    const catalogIds = Array.from(new Set(catalog.map((tool) => String(tool.id).trim()).filter(Boolean)));
    const selectedTools = Array.from(new Set(normalizeStringList(selected)));
    const memberToolIds = validMemberToolIds(member, catalogIds);
    const validToolIds = mode === "catalog" ? catalogIds : memberToolIds;
    const validToolSet = new Set(validToolIds);
    const addable = validToolIds.filter((id) => !selectedTools.includes(id));
    const metaById = new Map(catalog.map((tool) => [String(tool.id), tool]));
    const rowFor = (id) => {
      const meta = metaById.get(id) || null;
      const unavailable = !validToolSet.has(id);
      const reason = unavailable
        ? (mode === "catalog" ? view.toolScopeNotInCatalogReason : view.toolScopeNotEnabledReason)
        : "";
      return {
        id,
        name: id,
        meta,
        unavailable,
        reason,
        className: `tool-row${unavailable ? " tool-row--invalid" : ""}`,
        description: unavailable ? reason : (meta?.desc || view.toolScopeToolDescriptionFallback),
        removeLabel: view.toolScopeRemoveLabel,
      };
    };
    const optionFor = (id) => {
      const meta = metaById.get(id) || null;
      const label = meta?.label || id;
      const desc = meta?.desc || id;
      return {
        id,
        value: id,
        label,
        description: desc,
        optionLabel: `${label} — ${desc}`,
        meta,
      };
    };
    return {
      selectedTools,
      addable,
      addableRows: addable.map(optionFor),
      rows: selectedTools.map(rowFor),
      addSelectValue: "",
      addSelectPlaceholder: mode === "member" && !member
        ? view.toolScopeSelectMemberPlaceholder
        : (mode === "catalog" ? view.toolScopeBlockCatalogPlaceholder : view.toolScopeAddProfilePlaceholder),
      disabled: (mode === "member" && !member) || addable.length === 0,
    };
  }

  function validMemberToolIds(member, catalogIds) {
    const catalogIdSet = new Set(catalogIds || []);
    return Array.from(new Set(normalizeStringList(member?.tools).filter((id) => catalogIdSet.has(id))));
  }

  function validStepToolSet({ member, mode = "member", toolCatalog = [] } = {}) {
    const catalog = Array.isArray(toolCatalog) ? toolCatalog.filter((tool) => tool?.id) : [];
    const catalogIds = Array.from(new Set(catalog.map((tool) => String(tool.id).trim()).filter(Boolean)));
    if (mode === "catalog") return new Set(catalogIds);
    return new Set(validMemberToolIds(member, catalogIds));
  }

  function normalizeStepToolScopeList(tools, options = {}) {
    const validTools = validStepToolSet(options);
    return Array.from(new Set(normalizeStringList(tools).filter((tool) => validTools.has(tool))));
  }

  function stepToolScopeAddPatch(selected, raw, options = {}) {
    const id = String(raw || "").trim();
    const field = options.field || "allowedTools";
    if (!id) return { ok: false, id: "", patch: null };
    const state = stepToolScopeState({ ...options, selected });
    if (!state.addable.includes(id)) {
      return { ok: false, id, patch: null };
    }
    return {
      ok: true,
      id,
      patch: { [field]: [...state.selectedTools, id] },
    };
  }

  function stepToolScopeRemovePatch(selected, raw, options = {}) {
    const id = String(raw || "").trim();
    const field = options.field || "allowedTools";
    if (!id) return { ok: false, id: "", patch: null };
    const selectedTools = Array.from(new Set(normalizeStringList(selected)));
    return {
      ok: true,
      id,
      patch: { [field]: selectedTools.filter((candidate) => candidate !== id) },
    };
  }

  function memberSkillTogglePatch(member, skillId, skillRealms = []) {
    const id = String(skillId || "").trim();
    if (!id) return { ok: false, id: "", patch: null };
    const skills = normalizeStringList(member?.skills);
    const selected = skills.includes(id);
    if (!selected && !skillIdsFromRealms(skillRealms).has(id)) {
      return { ok: false, id, patch: null };
    }
    return {
      ok: true,
      id,
      selected: !selected,
      patch: { skills: selected ? skills.filter((candidate) => candidate !== id) : [...skills, id] },
    };
  }

  function memberSkillRemovePatch(member, skillId) {
    const id = String(skillId || "").trim();
    if (!id) return { ok: false, id: "", patch: null };
    const skills = normalizeStringList(member?.skills);
    return { ok: true, id, patch: { skills: skills.filter((candidate) => candidate !== id) } };
  }

  function memberInlineSkillPatch(member, realms, spec = {}, accessView = null) {
    const view = agentAccessViewForState(accessView);
    const result = addInlineSkillToRealms(realms, spec, accessView);
    const skills = normalizeStringList(member?.skills);
    return {
      ...result,
      realmId: view.inlineSkillRealmId,
      patch: { skills: skills.includes(result.id) ? skills : [...skills, result.id] },
    };
  }

  function memberSkillToggleCascadePatch({ memberId, members, skillRealms } = {}, skillId) {
    const list = Array.isArray(members) ? members : [];
    const member = list.find((candidate) => candidate?.id === memberId) || null;
    if (!member) return { ok: false, error: "member not found", id: "", patch: null, members: list, skillRealms };
    const result = memberSkillTogglePatch(member, skillId, skillRealms);
    if (!result.ok) return { ...result, members: list, skillRealms };
    const updated = studioUpdateMemberPatch({ members: list }, member.id, result.patch || {});
    if (!updated.ok) {
      return { ...result, ok: false, error: updated.error || "", members: list, skillRealms };
    }
    return { ...result, members: updated.members, member: updated.member, skillRealms };
  }

  function memberSkillRemoveCascadePatch({ memberId, members, skillRealms } = {}, skillId) {
    const list = Array.isArray(members) ? members : [];
    const member = list.find((candidate) => candidate?.id === memberId) || null;
    if (!member) return { ok: false, error: "member not found", id: "", patch: null, members: list, skillRealms };
    const result = memberSkillRemovePatch(member, skillId);
    if (!result.ok) return { ...result, members: list, skillRealms };
    const updated = studioUpdateMemberPatch({ members: list }, member.id, result.patch || {});
    if (!updated.ok) {
      return { ...result, ok: false, error: updated.error || "", members: list, skillRealms };
    }
    return { ...result, members: updated.members, member: updated.member, skillRealms };
  }

  function memberInlineSkillCascadePatch({ memberId, members, skillRealms } = {}, spec = {}, accessView = null) {
    const list = Array.isArray(members) ? members : [];
    const member = list.find((candidate) => candidate?.id === memberId) || null;
    if (!member) return { ok: false, error: "member not found", id: "", patch: null, members: list, skillRealms };
    const result = memberInlineSkillPatch(member, skillRealms, spec, accessView);
    const updated = studioUpdateMemberPatch({ members: list }, member.id, result.patch || {});
    if (!updated.ok) {
      return { ...result, ok: false, error: updated.error || "", members: list, skillRealms };
    }
    return {
      ...result,
      ok: true,
      members: updated.members,
      member: updated.member,
      skillRealms: result.skillRealms,
      inlineForm: {
        realmId: result.realmId,
        label: "",
        content: "",
        error: "",
        open: false,
      },
    };
  }

  function memberSkillAccessState({ member, skillRealms, realmId = "", inlineOpen = false, accessView = null } = {}) {
    const view = agentAccessViewForState(accessView);
    const realms = Array.isArray(skillRealms) ? skillRealms.filter((realm) => realm?.id) : [];
    const defaultRealm = realms.find((realm) => realm.default) || realms[0] || null;
    const selectedRealm = realms.find((realm) => realm.id === realmId) || defaultRealm;
    const selectedSkillIds = normalizeStringList(member?.skills);
    const selectedSet = new Set(selectedSkillIds);
    const byId = new Map();
    for (const sourceRealm of realms) {
      for (const skill of sourceRealm.skills || []) {
        const id = String(skill?.id || "").trim();
        if (!id || byId.has(id)) continue;
        byId.set(id, { ...skill, id, realm: sourceRealm });
      }
    }
    const skillRows = (selectedRealm?.skills || [])
      .filter((skill) => String(skill?.id || "").trim())
      .map((skill) => {
        const id = String(skill.id).trim();
        const selected = selectedSet.has(id);
        return {
          id,
          selected,
          className: `skill-row${selected ? " is-on" : ""}`,
          checkLabel: selected ? view.skillSelectedCheckLabel : "",
          name: id,
          desc: skill.desc || skill.path || skill.source || view.skillDefaultDescription,
          skill,
        };
      });
    const selectedOutsideRealm = selectedSkillIds
      .map((id) => byId.get(id))
      .filter((skill) => skill && skill.realm?.id !== selectedRealm?.id)
      .map((skill) => ({
        id: skill.id,
        realmId: skill.realm?.id || "",
        realmLabel: skill.realm?.label || skill.realm?.id || "",
        className: "skill-chip",
        title: skill.realm?.label || skill.realm?.id || "",
        label: skill.id,
        detail: skill.realm?.label || skill.realm?.id || "",
        removeLabel: view.skillRemoveLabel,
      }));
    const unavailableSelected = selectedSkillIds
      .filter((id) => !byId.has(id))
      .map((id) => ({
        id,
        className: "skill-chip is-invalid",
        label: id,
        removeLabel: view.skillRemoveLabel,
      }));
    return {
      sectionTitle: view.skillSectionTitle,
      inlineToggleLabel: inlineOpen ? view.skillInlineCancelLabel : view.skillInlineOpenLabel,
      hint: view.skillHint,
      inlineLabelPlaceholder: view.skillInlineLabelPlaceholder,
      inlineContentRows: view.skillInlineContentRows,
      inlineContentPlaceholder: view.skillInlineContentPlaceholder,
      inlineCreateHint: view.skillInlineCreateHint,
      inlineAddLabel: view.skillInlineAddLabel,
      inlineErrorFallback: view.skillInlineErrorFallback,
      noRealmsMessage: view.skillNoRealmsMessage,
      realmLabel: view.skillRealmLabel,
      hasRealms: realms.length > 0,
      realmId: selectedRealm?.id || "",
      realmOptions: realms.map((realm) => ({
        id: realm.id,
        label: `${realm.label || realm.id}${realm.default ? view.skillDefaultRealmSuffix : ""}`,
      })),
      skillRows,
      selectedOutsideRealm,
      unavailableSelected,
      unavailableHeading: view.skillUnavailableHeading,
      outsideRealmHeading: view.skillOutsideRealmHeading,
    };
  }

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

  function agentViewFromSchema(schema) {
    const view = schema?.mob_definition?.editor_agent_view;
    if (!view || typeof view !== "object") return null;
    const out = {
      agentsHeading: String(view.agents_heading || "").trim(),
      schemasHeading: String(view.schemas_heading || "").trim(),
      addSchemaLabel: String(view.add_schema_label || "").trim(),
      addAgentTitle: String(view.add_agent_title || "").trim(),
      addAgentUnavailableTitle: String(view.add_agent_unavailable_title || "").trim(),
      addAgentUnavailableLabel: String(view.add_agent_unavailable_label || "").trim(),
      addAgentPlaceholderLabel: String(view.add_agent_placeholder_label || "").trim(),
      addAgentErrorPrefix: String(view.add_agent_error_prefix || "").trim(),
      definitionCatalogTitle: String(view.definition_catalog_title || "").trim(),
      definitionCatalogEmpty: String(view.definition_catalog_empty || "").trim(),
      definitionCatalogSourceLabel: String(view.definition_catalog_source_label || "").trim(),
      definitionCatalogToolsLabel: String(view.definition_catalog_tools_label || "").trim(),
      definitionCatalogSkillsLabel: String(view.definition_catalog_skills_label || "").trim(),
      memberSubLabelTemplate: String(view.member_sub_label_template || "").trim(),
      memberPlacedEmptyLabel: String(view.member_placed_empty_label || "").trim(),
      memberPlacedCountTemplate: String(view.member_placed_count_template || "").trim(),
      schemaFieldSingularTemplate: String(view.schema_field_singular_template || "").trim(),
      schemaFieldPluralTemplate: String(view.schema_field_plural_template || "").trim(),
      schemaUsageLabelTemplate: String(view.schema_usage_label_template || "").trim(),
      sidebarSubLabelSeparator: String(view.sidebar_sub_label_separator || ""),
      emptyTitle: String(view.empty_title || "").trim(),
      emptyLines: Array.isArray(view.empty_lines)
        ? view.empty_lines.map((line) => String(line || "").trim()).filter(Boolean)
        : [],
      missingSchemaLabel: String(view.missing_schema_label || "").trim(),
      missingAgentLabel: String(view.missing_agent_label || "").trim(),
    };
    return out.agentsHeading && out.schemasHeading && out.addSchemaLabel
      && out.addAgentTitle && out.addAgentUnavailableTitle
      && out.addAgentUnavailableLabel && out.addAgentPlaceholderLabel
      && out.definitionCatalogTitle && out.definitionCatalogEmpty
      && out.definitionCatalogSourceLabel && out.definitionCatalogToolsLabel && out.definitionCatalogSkillsLabel
      && out.memberSubLabelTemplate && out.memberPlacedEmptyLabel && out.memberPlacedCountTemplate
      && out.schemaFieldSingularTemplate && out.schemaFieldPluralTemplate && out.schemaUsageLabelTemplate
      && out.sidebarSubLabelSeparator
      && out.emptyTitle && out.emptyLines.length && out.missingSchemaLabel && out.missingAgentLabel
      ? out
      : null;
  }

  function agentViewForState(agentView) {
    const view = agentView && typeof agentView === "object" ? agentView : null;
    return {
      agentsHeading: String(view?.agentsHeading || ""),
      schemasHeading: String(view?.schemasHeading || ""),
      addSchemaLabel: String(view?.addSchemaLabel || ""),
      addAgentTitle: String(view?.addAgentTitle || ""),
      addAgentUnavailableTitle: String(view?.addAgentUnavailableTitle || ""),
      addAgentUnavailableLabel: String(view?.addAgentUnavailableLabel || ""),
      addAgentPlaceholderLabel: String(view?.addAgentPlaceholderLabel || ""),
      addAgentErrorPrefix: String(view?.addAgentErrorPrefix || ""),
      definitionCatalogTitle: String(view?.definitionCatalogTitle || ""),
      definitionCatalogEmpty: String(view?.definitionCatalogEmpty || ""),
      definitionCatalogSourceLabel: String(view?.definitionCatalogSourceLabel || ""),
      definitionCatalogToolsLabel: String(view?.definitionCatalogToolsLabel || ""),
      definitionCatalogSkillsLabel: String(view?.definitionCatalogSkillsLabel || ""),
      memberSubLabelTemplate: String(view?.memberSubLabelTemplate || ""),
      memberPlacedEmptyLabel: String(view?.memberPlacedEmptyLabel || ""),
      memberPlacedCountTemplate: String(view?.memberPlacedCountTemplate || ""),
      schemaFieldSingularTemplate: String(view?.schemaFieldSingularTemplate || ""),
      schemaFieldPluralTemplate: String(view?.schemaFieldPluralTemplate || ""),
      schemaUsageLabelTemplate: String(view?.schemaUsageLabelTemplate || ""),
      sidebarSubLabelSeparator: String(view?.sidebarSubLabelSeparator || ""),
      emptyTitle: String(view?.emptyTitle || ""),
      emptyLines: Array.isArray(view?.emptyLines) ? view.emptyLines : [],
      missingSchemaLabel: String(view?.missingSchemaLabel || ""),
      missingAgentLabel: String(view?.missingAgentLabel || ""),
    };
  }

  function newFlowViewFromSchema(schema) {
    const view = schema?.mob_definition?.editor_new_flow_view;
    if (!view || typeof view !== "object") return null;
    const out = {
      eyebrowTemplate: String(view.eyebrow_template || "").trim(),
      closeLabel: String(view.close_label || "").trim(),
      nameLabel: String(view.name_label || "").trim(),
      namePlaceholder: String(view.name_placeholder || "").trim(),
      triggerLabel: String(view.trigger_label || "").trim(),
      triggerPlaceholder: String(view.trigger_placeholder || "").trim(),
      startFromLabel: String(view.start_from_label || "").trim(),
      backLabel: String(view.back_label || "").trim(),
      nextLabel: String(view.next_label || "").trim(),
      createLabel: String(view.create_label || "").trim(),
    };
    return out.eyebrowTemplate && out.closeLabel && out.nameLabel && out.namePlaceholder
      && out.triggerLabel && out.triggerPlaceholder && out.startFromLabel && out.backLabel
      && out.nextLabel && out.createLabel
      ? out
      : null;
  }

  function newFlowViewForState(newFlowView) {
    const view = newFlowView && typeof newFlowView === "object" ? newFlowView : null;
    return {
      eyebrowTemplate: String(view?.eyebrowTemplate || ""),
      closeLabel: String(view?.closeLabel || ""),
      nameLabel: String(view?.nameLabel || ""),
      namePlaceholder: String(view?.namePlaceholder || ""),
      triggerLabel: String(view?.triggerLabel || ""),
      triggerPlaceholder: String(view?.triggerPlaceholder || ""),
      startFromLabel: String(view?.startFromLabel || ""),
      backLabel: String(view?.backLabel || ""),
      nextLabel: String(view?.nextLabel || ""),
      createLabel: String(view?.createLabel || ""),
    };
  }

  function flowRegistryViewFromSchema(schema) {
    const view = schema?.mob_definition?.editor_flow_registry_view;
    if (!view || typeof view !== "object") return null;
    const columns = Array.isArray(view.columns)
      ? view.columns.map((column) => ({
        key: String(column?.key || "").trim(),
        label: String(column?.label || "").trim(),
      })).filter((column) => column.key && column.label)
      : [];
    const out = {
      eyebrow: String(view.eyebrow || "").trim(),
      titleSingularSuffix: String(view.title_singular_suffix || "").trim(),
      titlePluralSuffix: String(view.title_plural_suffix || "").trim(),
      createLabel: String(view.create_label || "").trim(),
      createReadyTitle: String(view.create_ready_title || "").trim(),
      createUnavailableTitle: String(view.create_unavailable_title || "").trim(),
      columns,
    };
    return out.eyebrow && out.titleSingularSuffix && out.titlePluralSuffix
      && out.createLabel && out.createReadyTitle && out.createUnavailableTitle
      && out.columns.length === 4
      ? out
      : null;
  }

  function flowRegistryViewForState(flowRegistryView) {
    const view = flowRegistryView && typeof flowRegistryView === "object" ? flowRegistryView : null;
    return {
      eyebrow: String(view?.eyebrow || ""),
      titleSingularSuffix: String(view?.titleSingularSuffix || ""),
      titlePluralSuffix: String(view?.titlePluralSuffix || ""),
      createLabel: String(view?.createLabel || ""),
      createReadyTitle: String(view?.createReadyTitle || ""),
      createUnavailableTitle: String(view?.createUnavailableTitle || ""),
      columns: Array.isArray(view?.columns)
        ? view.columns.map((column) => ({
          key: String(column?.key || ""),
          label: String(column?.label || ""),
        })).filter((column) => column.key && column.label)
        : [],
    };
  }

  function schemaViewFromSchema(schema) {
    const view = schema?.mob_definition?.editor_schema_view;
    if (!view || typeof view !== "object") return null;
    const headers = view.header_labels && typeof view.header_labels === "object" ? view.header_labels : {};
    const out = {
      eyebrow: String(view.eyebrow || "").trim(),
      descriptionTitle: String(view.description_title || "").trim(),
      descriptionPlaceholder: String(view.description_placeholder || "").trim(),
      fieldsTitlePrefix: String(view.fields_title_prefix || "").trim(),
      fieldsTitleTemplate: String(view.fields_title_template || "").trim(),
      addFieldLabel: String(view.add_field_label || "").trim(),
      headerLabels: {
        name: String(headers.name || "").trim(),
        type: String(headers.type || "").trim(),
        required: String(headers.required || "").trim(),
        description: String(headers.description || "").trim(),
        action: String(headers.action || "").trim(),
      },
      emptyFieldsHint: String(view.empty_fields_hint || "").trim(),
      usedByPrefix: String(view.used_by_prefix || "").trim(),
      usedByTitleTemplate: String(view.used_by_title_template || "").trim(),
      usageSingularTemplate: String(view.usage_singular_template || "").trim(),
      usagePluralTemplate: String(view.usage_plural_template || "").trim(),
      emptyUsedByHint: String(view.empty_used_by_hint || "").trim(),
      deleteLabel: String(view.delete_label || "").trim(),
      deleteBlockedTitle: String(view.delete_blocked_title || "").trim(),
      fieldNamePlaceholder: String(view.field_name_placeholder || "").trim(),
      fieldDescriptionPlaceholder: String(view.field_description_placeholder || "").trim(),
      fieldRemoveTitle: String(view.field_remove_title || "").trim(),
      fieldEnumLabel: String(view.field_enum_label || "").trim(),
      fieldEnumAddLabel: String(view.field_enum_add_label || "").trim(),
      fieldEnumAddValue: String(view.field_enum_add_value || "").trim(),
    };
    return out.eyebrow && out.descriptionTitle && out.fieldsTitlePrefix && out.fieldsTitleTemplate && out.addFieldLabel
      && out.headerLabels.name && out.headerLabels.type && out.headerLabels.required && out.headerLabels.description
      && out.emptyFieldsHint && out.usedByPrefix && out.usedByTitleTemplate
      && out.usageSingularTemplate && out.usagePluralTemplate && out.emptyUsedByHint && out.deleteLabel && out.deleteBlockedTitle
      && out.fieldNamePlaceholder && out.fieldDescriptionPlaceholder && out.fieldRemoveTitle
      && out.fieldEnumLabel && out.fieldEnumAddLabel && out.fieldEnumAddValue
      ? out
      : null;
  }

  function schemaViewForState(schemaView) {
    const view = schemaView && typeof schemaView === "object" ? schemaView : null;
    return {
      eyebrow: String(view?.eyebrow || ""),
      descriptionTitle: String(view?.descriptionTitle || ""),
      descriptionPlaceholder: String(view?.descriptionPlaceholder || ""),
      fieldsTitlePrefix: String(view?.fieldsTitlePrefix || ""),
      fieldsTitleTemplate: String(view?.fieldsTitleTemplate || ""),
      addFieldLabel: String(view?.addFieldLabel || ""),
      headerLabels: {
        name: String(view?.headerLabels?.name || ""),
        type: String(view?.headerLabels?.type || ""),
        required: String(view?.headerLabels?.required || ""),
        description: String(view?.headerLabels?.description || ""),
        action: String(view?.headerLabels?.action || ""),
      },
      emptyFieldsHint: String(view?.emptyFieldsHint || ""),
      usedByPrefix: String(view?.usedByPrefix || ""),
      usedByTitleTemplate: String(view?.usedByTitleTemplate || ""),
      usageSingularTemplate: String(view?.usageSingularTemplate || ""),
      usagePluralTemplate: String(view?.usagePluralTemplate || ""),
      emptyUsedByHint: String(view?.emptyUsedByHint || ""),
      deleteLabel: String(view?.deleteLabel || ""),
      deleteBlockedTitle: String(view?.deleteBlockedTitle || ""),
      fieldNamePlaceholder: String(view?.fieldNamePlaceholder || ""),
      fieldDescriptionPlaceholder: String(view?.fieldDescriptionPlaceholder || ""),
      fieldRemoveTitle: String(view?.fieldRemoveTitle || ""),
      fieldEnumLabel: String(view?.fieldEnumLabel || ""),
      fieldEnumAddLabel: String(view?.fieldEnumAddLabel || ""),
      fieldEnumAddValue: String(view?.fieldEnumAddValue || ""),
    };
  }

  function agentDetailViewFromSchema(schema) {
    const view = schema?.mob_definition?.editor_agent_detail_view;
    if (!view || typeof view !== "object") return null;
    const out = {
      usedInLabel: String(view.used_in_label || "").trim(),
      instanceSingular: String(view.instance_singular || "").trim(),
      instancePlural: String(view.instance_plural || "").trim(),
      deleteLabel: String(view.delete_label || "").trim(),
      deleteConfirmIntro: String(view.delete_confirm_intro || "").trim(),
      deleteConfirmPlacedPrefix: String(view.delete_confirm_placed_prefix || "").trim(),
      deleteCancelLabel: String(view.delete_cancel_label || "").trim(),
      cellSingular: String(view.cell_singular || "").trim(),
      cellPlural: String(view.cell_plural || "").trim(),
      deleteConfirmCellsSuffix: String(view.delete_confirm_cells_suffix || "").trim(),
      usageTitlePrefix: String(view.usage_title_prefix || "").trim(),
      emptyUsageHint: String(view.empty_usage_hint || "").trim(),
      agentEyebrowPrefix: String(view.agent_eyebrow_prefix || "").trim(),
      identityTitle: String(view.identity_title || "").trim(),
      profileBindingLabel: String(view.profile_binding_label || "").trim(),
      missingProfileBindingLabel: String(view.missing_profile_binding_label || "").trim(),
      realmProfileLabel: String(view.realm_profile_label || "").trim(),
      realmProfilePlaceholder: String(view.realm_profile_placeholder || "").trim(),
      realmProfileImportHintFallback: String(view.realm_profile_import_hint_fallback || "").trim(),
      realmProfileTitle: String(view.realm_profile_title || "").trim(),
      realmProfileReferenceHintBefore: String(view.realm_profile_reference_hint_before || "").trim(),
      realmProfileReferenceHintAfterFallback: String(view.realm_profile_reference_hint_after_fallback || "").trim(),
      modelLabel: String(view.model_label || "").trim(),
      runtimeModeLabel: String(view.runtime_mode_label || "").trim(),
      missingRuntimeModeLabel: String(view.missing_runtime_mode_label || "").trim(),
      backendLabel: String(view.backend_label || "").trim(),
      backendDefinitionDefaultLabel: String(view.backend_definition_default_label || "").trim(),
      inlinePeerNotificationsLabel: String(view.inline_peer_notifications_label || "").trim(),
      inlinePeerNotificationsPlaceholder: String(view.inline_peer_notifications_placeholder || "").trim(),
      providerParamsLabel: String(view.provider_params_label || "").trim(),
      providerParamsPlaceholder: String(view.provider_params_placeholder || "").trim(),
      providerParamsRows: Number(view.provider_params_rows || 0),
      providerParamsInvalidJsonLabel: String(view.provider_params_invalid_json_label || "").trim(),
      providerParamsObjectRequiredError: String(view.provider_params_object_required_error || "").trim(),
      systemPromptTitle: String(view.system_prompt_title || "").trim(),
      applySkeletonLabel: String(view.apply_skeleton_label || "").trim(),
      applySkeletonTitle: String(view.apply_skeleton_title || "").trim(),
      systemPromptPlaceholder: String(view.system_prompt_placeholder || "").trim(),
      outputSchemaTitle: String(view.output_schema_title || "").trim(),
      schemaNoneLabel: String(view.schema_none_label || "").trim(),
      schemaRequiredLabel: String(view.schema_required_label || "").trim(),
      editSchemaLabel: String(view.edit_schema_label || "").trim(),
      emptySchemaHint: String(view.empty_schema_hint || "").trim(),
      sourceTitle: String(view.source_title || "").trim(),
      sourceEmptyHint: String(view.source_empty_hint || "").trim(),
      sourceDefinitionLabel: String(view.source_definition_label || "").trim(),
      sourceMobpackLabel: String(view.source_mobpack_label || "").trim(),
      sourceOriginLabel: String(view.source_origin_label || "").trim(),
      sourceDocumentPathLabel: String(view.source_document_path_label || "").trim(),
      sourceSchemaPathLabel: String(view.source_schema_path_label || "").trim(),
      sourceToolsLabel: String(view.source_tools_label || "").trim(),
      sourceSkillsLabel: String(view.source_skills_label || "").trim(),
    };
    return out.usedInLabel && out.instanceSingular && out.instancePlural && out.deleteLabel
      && out.deleteConfirmIntro && out.deleteConfirmPlacedPrefix && out.deleteCancelLabel && out.cellSingular && out.cellPlural
      && out.deleteConfirmCellsSuffix && out.usageTitlePrefix
      && out.emptyUsageHint && out.agentEyebrowPrefix && out.identityTitle && out.profileBindingLabel && out.missingProfileBindingLabel
      && out.realmProfileLabel && out.realmProfilePlaceholder && out.realmProfileImportHintFallback
      && out.realmProfileTitle && out.realmProfileReferenceHintBefore && out.realmProfileReferenceHintAfterFallback
      && out.modelLabel && out.runtimeModeLabel && out.missingRuntimeModeLabel && out.backendLabel
      && out.backendDefinitionDefaultLabel
      && out.inlinePeerNotificationsLabel && out.inlinePeerNotificationsPlaceholder
      && out.providerParamsLabel && out.providerParamsPlaceholder && Number.isFinite(out.providerParamsRows) && out.providerParamsRows > 0
      && out.providerParamsInvalidJsonLabel && out.providerParamsObjectRequiredError
      && out.systemPromptTitle && out.applySkeletonLabel && out.applySkeletonTitle && out.systemPromptPlaceholder
      && out.outputSchemaTitle && out.schemaNoneLabel && out.schemaRequiredLabel && out.editSchemaLabel && out.emptySchemaHint
      && out.sourceTitle && out.sourceEmptyHint && out.sourceDefinitionLabel && out.sourceMobpackLabel
      && out.sourceOriginLabel && out.sourceDocumentPathLabel && out.sourceSchemaPathLabel
      && out.sourceToolsLabel && out.sourceSkillsLabel
      ? out
      : null;
  }

  function agentDetailViewForState(agentDetailView) {
    const view = agentDetailView && typeof agentDetailView === "object" ? agentDetailView : null;
    return {
      usedInLabel: String(view?.usedInLabel || ""),
      instanceSingular: String(view?.instanceSingular || ""),
      instancePlural: String(view?.instancePlural || ""),
      deleteLabel: String(view?.deleteLabel || ""),
      deleteConfirmIntro: String(view?.deleteConfirmIntro || ""),
      deleteConfirmPlacedPrefix: String(view?.deleteConfirmPlacedPrefix || ""),
      deleteCancelLabel: String(view?.deleteCancelLabel || ""),
      cellSingular: String(view?.cellSingular || ""),
      cellPlural: String(view?.cellPlural || ""),
      deleteConfirmCellsSuffix: String(view?.deleteConfirmCellsSuffix || ""),
      usageTitlePrefix: String(view?.usageTitlePrefix || ""),
      emptyUsageHint: String(view?.emptyUsageHint || ""),
      agentEyebrowPrefix: String(view?.agentEyebrowPrefix || ""),
      identityTitle: String(view?.identityTitle || ""),
      profileBindingLabel: String(view?.profileBindingLabel || ""),
      missingProfileBindingLabel: String(view?.missingProfileBindingLabel || ""),
      realmProfileLabel: String(view?.realmProfileLabel || ""),
      realmProfilePlaceholder: String(view?.realmProfilePlaceholder || ""),
      realmProfileImportHintFallback: String(view?.realmProfileImportHintFallback || ""),
      realmProfileTitle: String(view?.realmProfileTitle || ""),
      realmProfileReferenceHintBefore: String(view?.realmProfileReferenceHintBefore || ""),
      realmProfileReferenceHintAfterFallback: String(view?.realmProfileReferenceHintAfterFallback || ""),
      modelLabel: String(view?.modelLabel || ""),
      runtimeModeLabel: String(view?.runtimeModeLabel || ""),
      missingRuntimeModeLabel: String(view?.missingRuntimeModeLabel || ""),
      backendLabel: String(view?.backendLabel || ""),
      backendDefinitionDefaultLabel: String(view?.backendDefinitionDefaultLabel || ""),
      inlinePeerNotificationsLabel: String(view?.inlinePeerNotificationsLabel || ""),
      inlinePeerNotificationsPlaceholder: String(view?.inlinePeerNotificationsPlaceholder || ""),
      providerParamsLabel: String(view?.providerParamsLabel || ""),
      providerParamsPlaceholder: String(view?.providerParamsPlaceholder || ""),
      providerParamsRows: Number(view?.providerParamsRows || 0),
      providerParamsInvalidJsonLabel: String(view?.providerParamsInvalidJsonLabel || ""),
      providerParamsObjectRequiredError: String(view?.providerParamsObjectRequiredError || ""),
      systemPromptTitle: String(view?.systemPromptTitle || ""),
      applySkeletonLabel: String(view?.applySkeletonLabel || ""),
      applySkeletonTitle: String(view?.applySkeletonTitle || ""),
      systemPromptPlaceholder: String(view?.systemPromptPlaceholder || ""),
      outputSchemaTitle: String(view?.outputSchemaTitle || ""),
      schemaNoneLabel: String(view?.schemaNoneLabel || ""),
      schemaRequiredLabel: String(view?.schemaRequiredLabel || ""),
      editSchemaLabel: String(view?.editSchemaLabel || ""),
      emptySchemaHint: String(view?.emptySchemaHint || ""),
      sourceTitle: String(view?.sourceTitle || ""),
      sourceEmptyHint: String(view?.sourceEmptyHint || ""),
      sourceDefinitionLabel: String(view?.sourceDefinitionLabel || ""),
      sourceMobpackLabel: String(view?.sourceMobpackLabel || ""),
      sourceOriginLabel: String(view?.sourceOriginLabel || ""),
      sourceDocumentPathLabel: String(view?.sourceDocumentPathLabel || ""),
      sourceSchemaPathLabel: String(view?.sourceSchemaPathLabel || ""),
      sourceToolsLabel: String(view?.sourceToolsLabel || ""),
      sourceSkillsLabel: String(view?.sourceSkillsLabel || ""),
    };
  }

  function agentAccessViewFromSchema(schema) {
    const view = schema?.mob_definition?.editor_agent_access_view;
    if (!view || typeof view !== "object") return null;
    const out = {
      toolInvalidError: String(view.tool_invalid_error || "").trim(),
      toolTitle: String(view.tool_title || "").trim(),
      toolHint: String(view.tool_hint || "").trim(),
      toolMissingDescription: String(view.tool_missing_description || "").trim(),
      toolRemoveLabel: String(view.tool_remove_label || "").trim(),
      toolAddSelectPlaceholder: String(view.tool_add_select_placeholder || "").trim(),
      toolSourceLabel: String(view.tool_source_label || "").trim(),
      toolSourcePlaceholder: String(view.tool_source_placeholder || "").trim(),
      toolAddButtonLabel: String(view.tool_add_button_label || "").trim(),
      inlineSkillRealmId: String(view.inline_skill_realm_id || "").trim(),
      inlineSkillRealmLabel: String(view.inline_skill_realm_label || "").trim(),
      inlineSkillDefaultDescription: String(view.inline_skill_default_description || "").trim(),
      skillDefaultDescription: String(view.skill_default_description || "").trim(),
      skillSelectedCheckLabel: String(view.skill_selected_check_label || "").trim(),
      skillRemoveLabel: String(view.skill_remove_label || "").trim(),
      skillSectionTitle: String(view.skill_section_title || "").trim(),
      skillInlineCancelLabel: String(view.skill_inline_cancel_label || "").trim(),
      skillInlineOpenLabel: String(view.skill_inline_open_label || "").trim(),
      skillHint: String(view.skill_hint || "").trim(),
      skillInlineLabelPlaceholder: String(view.skill_inline_label_placeholder || "").trim(),
      skillInlineContentRows: Number(view.skill_inline_content_rows || 0),
      skillInlineContentPlaceholder: String(view.skill_inline_content_placeholder || "").trim(),
      skillInlineCreateHint: String(view.skill_inline_create_hint || "").trim(),
      skillInlineAddLabel: String(view.skill_inline_add_label || "").trim(),
      skillInlineErrorFallback: String(view.skill_inline_error_fallback || "").trim(),
      skillInlineMissingLabelError: String(view.skill_inline_missing_label_error || "").trim(),
      skillInlineMissingContentError: String(view.skill_inline_missing_content_error || "").trim(),
      skillInlineInvalidIdError: String(view.skill_inline_invalid_id_error || "").trim(),
      skillNoRealmsMessage: String(view.skill_no_realms_message || "").trim(),
      skillRealmLabel: String(view.skill_realm_label || "").trim(),
      skillDefaultRealmSuffix: String(view.skill_default_realm_suffix || ""),
      skillUnavailableHeading: String(view.skill_unavailable_heading || "").trim(),
      skillOutsideRealmHeading: String(view.skill_outside_realm_heading || "").trim(),
    };
    return Object.entries(out).every(([key, value]) => key === "skillInlineContentRows" ? Number.isFinite(value) && value > 0 : !!value)
      ? out
      : null;
  }

  function agentAccessViewForState(agentAccessView) {
    const view = agentAccessView && typeof agentAccessView === "object" ? agentAccessView : null;
    return {
      toolInvalidError: String(view?.toolInvalidError || ""),
      toolTitle: String(view?.toolTitle || ""),
      toolHint: String(view?.toolHint || ""),
      toolMissingDescription: String(view?.toolMissingDescription || ""),
      toolRemoveLabel: String(view?.toolRemoveLabel || ""),
      toolAddSelectPlaceholder: String(view?.toolAddSelectPlaceholder || ""),
      toolSourceLabel: String(view?.toolSourceLabel || ""),
      toolSourcePlaceholder: String(view?.toolSourcePlaceholder || ""),
      toolAddButtonLabel: String(view?.toolAddButtonLabel || ""),
      inlineSkillRealmId: String(view?.inlineSkillRealmId || ""),
      inlineSkillRealmLabel: String(view?.inlineSkillRealmLabel || ""),
      inlineSkillDefaultDescription: String(view?.inlineSkillDefaultDescription || ""),
      skillDefaultDescription: String(view?.skillDefaultDescription || ""),
      skillSelectedCheckLabel: String(view?.skillSelectedCheckLabel || ""),
      skillRemoveLabel: String(view?.skillRemoveLabel || ""),
      skillSectionTitle: String(view?.skillSectionTitle || ""),
      skillInlineCancelLabel: String(view?.skillInlineCancelLabel || ""),
      skillInlineOpenLabel: String(view?.skillInlineOpenLabel || ""),
      skillHint: String(view?.skillHint || ""),
      skillInlineLabelPlaceholder: String(view?.skillInlineLabelPlaceholder || ""),
      skillInlineContentRows: Number(view?.skillInlineContentRows || 0),
      skillInlineContentPlaceholder: String(view?.skillInlineContentPlaceholder || ""),
      skillInlineCreateHint: String(view?.skillInlineCreateHint || ""),
      skillInlineAddLabel: String(view?.skillInlineAddLabel || ""),
      skillInlineErrorFallback: String(view?.skillInlineErrorFallback || ""),
      skillInlineMissingLabelError: String(view?.skillInlineMissingLabelError || ""),
      skillInlineMissingContentError: String(view?.skillInlineMissingContentError || ""),
      skillInlineInvalidIdError: String(view?.skillInlineInvalidIdError || ""),
      skillNoRealmsMessage: String(view?.skillNoRealmsMessage || ""),
      skillRealmLabel: String(view?.skillRealmLabel || ""),
      skillDefaultRealmSuffix: String(view?.skillDefaultRealmSuffix || ""),
      skillUnavailableHeading: String(view?.skillUnavailableHeading || ""),
      skillOutsideRealmHeading: String(view?.skillOutsideRealmHeading || ""),
    };
  }

  function deployViewFromSchema(schema) {
    const view = schema?.mob_definition?.editor_deploy_view;
    if (!view || typeof view !== "object") return null;
    const out = {
      brandLabel: String(view.brand_label || "").trim(),
      flowsTabLabel: String(view.flows_tab_label || "").trim(),
      agentsTabLabel: String(view.agents_tab_label || "").trim(),
      mobStatusTitle: String(view.mob_status_title || "").trim(),
      mobFileLabel: String(view.mob_file_label || "").trim(),
      apiErrorLabel: String(view.api_error_label || "").trim(),
      apiReadyLabel: String(view.api_ready_label || "").trim(),
      apiLoadingLabel: String(view.api_loading_label || "").trim(),
      deployPrefixLabel: String(view.deploy_prefix_label || "").trim(),
      flowsCrumbLabel: String(view.flows_crumb_label || "").trim(),
      crumbSeparator: String(view.crumb_separator || "").trim(),
      planTraceLabel: String(view.plan_trace_label || "").trim(),
      importLabel: String(view.import_label || "").trim(),
      validateLabel: String(view.validate_label || "").trim(),
      publishLabel: String(view.publish_label || "").trim(),
      deployPlanLabel: String(view.deploy_plan_label || "").trim(),
      deployLabel: String(view.deploy_label || "").trim(),
      themeSwitchPrefix: String(view.theme_switch_prefix || "").trim(),
      themeSwitchSuffix: String(view.theme_switch_suffix || "").trim(),
      darkThemeLabel: String(view.dark_theme_label || "").trim(),
      lightThemeLabel: String(view.light_theme_label || "").trim(),
      basicModeTitle: String(view.basic_mode_title || "").trim(),
      basicModeLabel: String(view.basic_mode_label || "").trim(),
      graphModeTitle: String(view.graph_mode_title || "").trim(),
      graphModeLabel: String(view.graph_mode_label || "").trim(),
      validationEyebrow: String(view.validation_eyebrow || "").trim(),
      validationPassedLabel: String(view.validation_passed_label || "").trim(),
      validationWarningsLabel: String(view.validation_warnings_label || "").trim(),
      validationBlockingLabel: String(view.validation_blocking_label || "").trim(),
      closeLabel: String(view.close_label || "").trim(),
      planEyebrow: String(view.plan_eyebrow || "").trim(),
      planUnavailableHead: String(view.plan_unavailable_head || "").trim(),
      planUnavailableBody: String(view.plan_unavailable_body || "").trim(),
      planFirstLabel: String(view.plan_first_label || "").trim(),
      planStepLabel: String(view.plan_step_label || "").trim(),
      planPreviousLabel: String(view.plan_previous_label || "").trim(),
      planNextLabel: String(view.plan_next_label || "").trim(),
    };
    return Object.values(out).every(Boolean) ? out : null;
  }

  function deployViewForState(deployView) {
    const view = deployView && typeof deployView === "object" ? deployView : null;
    return {
      brandLabel: String(view?.brandLabel || ""),
      flowsTabLabel: String(view?.flowsTabLabel || ""),
      agentsTabLabel: String(view?.agentsTabLabel || ""),
      mobStatusTitle: String(view?.mobStatusTitle || ""),
      mobFileLabel: String(view?.mobFileLabel || ""),
      apiErrorLabel: String(view?.apiErrorLabel || ""),
      apiReadyLabel: String(view?.apiReadyLabel || ""),
      apiLoadingLabel: String(view?.apiLoadingLabel || ""),
      deployPrefixLabel: String(view?.deployPrefixLabel || ""),
      flowsCrumbLabel: String(view?.flowsCrumbLabel || ""),
      crumbSeparator: String(view?.crumbSeparator || ""),
      planTraceLabel: String(view?.planTraceLabel || ""),
      importLabel: String(view?.importLabel || ""),
      validateLabel: String(view?.validateLabel || ""),
      publishLabel: String(view?.publishLabel || ""),
      deployPlanLabel: String(view?.deployPlanLabel || ""),
      deployLabel: String(view?.deployLabel || ""),
      themeSwitchPrefix: String(view?.themeSwitchPrefix || ""),
      themeSwitchSuffix: String(view?.themeSwitchSuffix || ""),
      darkThemeLabel: String(view?.darkThemeLabel || ""),
      lightThemeLabel: String(view?.lightThemeLabel || ""),
      basicModeTitle: String(view?.basicModeTitle || ""),
      basicModeLabel: String(view?.basicModeLabel || ""),
      graphModeTitle: String(view?.graphModeTitle || ""),
      graphModeLabel: String(view?.graphModeLabel || ""),
      validationEyebrow: String(view?.validationEyebrow || ""),
      validationPassedLabel: String(view?.validationPassedLabel || ""),
      validationWarningsLabel: String(view?.validationWarningsLabel || ""),
      validationBlockingLabel: String(view?.validationBlockingLabel || ""),
      closeLabel: String(view?.closeLabel || ""),
      planEyebrow: String(view?.planEyebrow || ""),
      planUnavailableHead: String(view?.planUnavailableHead || ""),
      planUnavailableBody: String(view?.planUnavailableBody || ""),
      planFirstLabel: String(view?.planFirstLabel || ""),
      planStepLabel: String(view?.planStepLabel || ""),
      planPreviousLabel: String(view?.planPreviousLabel || ""),
      planNextLabel: String(view?.planNextLabel || ""),
    };
  }

  function settingsViewOptionsFromSchema(value) {
    return Array.isArray(value)
      ? value
        .map((option) => ({
          value: String(option?.value || "").trim(),
          label: String(option?.label || "").trim(),
        }))
        .filter((option) => option.value && option.label)
      : [];
  }

  function settingsViewFromSchema(schema) {
    const view = schema?.mob_definition?.editor_settings_view;
    if (!view || typeof view !== "object") return null;
    const out = {
      panelTitle: String(view.panel_title || "").trim(),
      panelCloseLabel: String(view.panel_close_label || "").trim(),
      loadMobTitle: String(view.load_mob_title || "").trim(),
      loadMobLabel: String(view.load_mob_label || "").trim(),
      flowStageFallback: String(view.flow_stage_fallback || "").trim(),
      optionSeparator: String(view.option_separator || ""),
      canvasTitle: String(view.canvas_title || "").trim(),
      edgeStyleLabel: String(view.edge_style_label || "").trim(),
      edgeStyleOptions: settingsViewOptionsFromSchema(view.edge_style_options),
      densityLabel: String(view.density_label || "").trim(),
      densityOptions: settingsViewOptionsFromSchema(view.density_options),
      themeTitle: String(view.theme_title || "").trim(),
      themeModeLabel: String(view.theme_mode_label || "").trim(),
      themeModeOptions: settingsViewOptionsFromSchema(view.theme_mode_options),
      mobTitle: String(view.mob_title || "").trim(),
      orchestratorLabel: String(view.orchestrator_label || "").trim(),
      profileNoneLabel: String(view.profile_none_label || "").trim(),
      autoWireLabel: String(view.auto_wire_label || "").trim(),
      autoWireOptions: settingsViewOptionsFromSchema(view.auto_wire_options),
      roleWiringLabel: String(view.role_wiring_label || "").trim(),
      roleWiringAddLabel: String(view.role_wiring_add_label || "").trim(),
      defaultBackendLabel: String(view.default_backend_label || "").trim(),
      externalBaseLabel: String(view.external_base_label || "").trim(),
      externalBasePlaceholder: String(view.external_base_placeholder || "").trim(),
      advancedLabel: String(view.advanced_label || "").trim(),
      advancedObjectRequiredError: String(view.advanced_object_required_error || "").trim(),
      advancedInvalidJsonError: String(view.advanced_invalid_json_error || "").trim(),
      deployTitle: String(view.deploy_title || "").trim(),
      surfaceLabel: String(view.surface_label || "").trim(),
      trustLabel: String(view.trust_label || "").trim(),
      modelLabel: String(view.model_label || "").trim(),
      modelDefaultLabel: String(view.model_default_label || "").trim(),
      modelVendorFallback: String(view.model_vendor_fallback || "").trim(),
      durationLabel: String(view.duration_label || "").trim(),
      durationPlaceholder: String(view.duration_placeholder || "").trim(),
      toolCallsLabel: String(view.tool_calls_label || "").trim(),
      toolCallsMin: Number(view.tool_calls_min),
      toolCallsMax: Number(view.tool_calls_max),
      tokensLabel: String(view.tokens_label || "").trim(),
      tokensMin: Number(view.tokens_min),
      tokensMax: Number(view.tokens_max),
      realmLabel: String(view.realm_label || "").trim(),
      realmOptions: settingsViewOptionsFromSchema(view.realm_options),
      realmIdLabel: String(view.realm_id_label || "").trim(),
      realmIdPlaceholder: String(view.realm_id_placeholder || "").trim(),
      backendLabel: String(view.backend_label || "").trim(),
      promptLabel: String(view.prompt_label || "").trim(),
      promptPlaceholder: String(view.prompt_placeholder || "").trim(),
      commandLabel: String(view.command_label || "").trim(),
      commandFallback: String(view.command_fallback || "").trim(),
      inspectorTitle: String(view.inspector_title || "").trim(),
      inspectorLayoutLabel: String(view.inspector_layout_label || "").trim(),
      inspectorLayoutOptions: settingsViewOptionsFromSchema(view.inspector_layout_options),
    };
    const numericOk = [out.toolCallsMin, out.toolCallsMax, out.tokensMin, out.tokensMax].every(Number.isFinite);
    const optionsOk = out.edgeStyleOptions.length && out.densityOptions.length && out.themeModeOptions.length
      && out.autoWireOptions.length && out.realmOptions.length && out.inspectorLayoutOptions.length;
    const stringsOk = Object.entries(out).every(([key, value]) => {
      if (Array.isArray(value) || typeof value === "number") return true;
      return key === "optionSeparator" ? value.length > 0 : !!value;
    });
    return numericOk && optionsOk && stringsOk ? out : null;
  }

  function settingsViewForState(settingsView) {
    const view = settingsView && typeof settingsView === "object" ? settingsView : null;
    return {
      panelTitle: String(view?.panelTitle || ""),
      panelCloseLabel: String(view?.panelCloseLabel || ""),
      loadMobTitle: String(view?.loadMobTitle || ""),
      loadMobLabel: String(view?.loadMobLabel || ""),
      flowStageFallback: String(view?.flowStageFallback || ""),
      optionSeparator: String(view?.optionSeparator || ""),
      canvasTitle: String(view?.canvasTitle || ""),
      edgeStyleLabel: String(view?.edgeStyleLabel || ""),
      edgeStyleOptions: Array.isArray(view?.edgeStyleOptions) ? view.edgeStyleOptions : [],
      densityLabel: String(view?.densityLabel || ""),
      densityOptions: Array.isArray(view?.densityOptions) ? view.densityOptions : [],
      themeTitle: String(view?.themeTitle || ""),
      themeModeLabel: String(view?.themeModeLabel || ""),
      themeModeOptions: Array.isArray(view?.themeModeOptions) ? view.themeModeOptions : [],
      mobTitle: String(view?.mobTitle || ""),
      orchestratorLabel: String(view?.orchestratorLabel || ""),
      profileNoneLabel: String(view?.profileNoneLabel || ""),
      autoWireLabel: String(view?.autoWireLabel || ""),
      autoWireOptions: Array.isArray(view?.autoWireOptions) ? view.autoWireOptions : [],
      roleWiringLabel: String(view?.roleWiringLabel || ""),
      roleWiringAddLabel: String(view?.roleWiringAddLabel || ""),
      defaultBackendLabel: String(view?.defaultBackendLabel || ""),
      externalBaseLabel: String(view?.externalBaseLabel || ""),
      externalBasePlaceholder: String(view?.externalBasePlaceholder || ""),
      advancedLabel: String(view?.advancedLabel || ""),
      advancedObjectRequiredError: String(view?.advancedObjectRequiredError || ""),
      advancedInvalidJsonError: String(view?.advancedInvalidJsonError || ""),
      deployTitle: String(view?.deployTitle || ""),
      surfaceLabel: String(view?.surfaceLabel || ""),
      trustLabel: String(view?.trustLabel || ""),
      modelLabel: String(view?.modelLabel || ""),
      modelDefaultLabel: String(view?.modelDefaultLabel || ""),
      modelVendorFallback: String(view?.modelVendorFallback || ""),
      durationLabel: String(view?.durationLabel || ""),
      durationPlaceholder: String(view?.durationPlaceholder || ""),
      toolCallsLabel: String(view?.toolCallsLabel || ""),
      toolCallsMin: Number(view?.toolCallsMin ?? NaN),
      toolCallsMax: Number(view?.toolCallsMax ?? NaN),
      tokensLabel: String(view?.tokensLabel || ""),
      tokensMin: Number(view?.tokensMin ?? NaN),
      tokensMax: Number(view?.tokensMax ?? NaN),
      realmLabel: String(view?.realmLabel || ""),
      realmOptions: Array.isArray(view?.realmOptions) ? view.realmOptions : [],
      realmIdLabel: String(view?.realmIdLabel || ""),
      realmIdPlaceholder: String(view?.realmIdPlaceholder || ""),
      backendLabel: String(view?.backendLabel || ""),
      promptLabel: String(view?.promptLabel || ""),
      promptPlaceholder: String(view?.promptPlaceholder || ""),
      commandLabel: String(view?.commandLabel || ""),
      commandFallback: String(view?.commandFallback || ""),
      inspectorTitle: String(view?.inspectorTitle || ""),
      inspectorLayoutLabel: String(view?.inspectorLayoutLabel || ""),
      inspectorLayoutOptions: Array.isArray(view?.inspectorLayoutOptions) ? view.inspectorLayoutOptions : [],
    };
  }

  function basicViewFromSchema(schema) {
    const view = schema?.mob_definition?.editor_basic_view;
    if (!view || typeof view !== "object") return null;
    const out = {
      startLabel: String(view.start_label || "").trim(),
      loopBadge: String(view.loop_badge || "").trim(),
      tipsTitle: String(view.tips_title || "").trim(),
      emptyPanelTitle: String(view.empty_panel_title || "").trim(),
      emptyPanelSubtitleParts: basicViewPartsFromSchema(view.empty_panel_subtitle_parts),
      sourceToggleLabel: String(view.source_toggle_label || "").trim(),
      memberStepPanelTitleFallback: String(view.member_step_panel_title_fallback || "").trim(),
      memberStepPanelSubFallback: String(view.member_step_panel_sub_fallback || "").trim(),
      memberStepMemberLabel: String(view.member_step_member_label || "").trim(),
      memberStepMemberPlaceholder: String(view.member_step_member_placeholder || "").trim(),
      memberStepRuntimeDefaultLabel: String(view.member_step_runtime_default_label || "").trim(),
      memberStepInstructionLabel: String(view.member_step_instruction_label || "").trim(),
      memberStepInstructionPlaceholder: String(view.member_step_instruction_placeholder || "").trim(),
      memberStepDispatchLabel: String(view.member_step_dispatch_label || "").trim(),
      memberStepCollectionLabel: String(view.member_step_collection_label || "").trim(),
      memberStepQuorumLabel: String(view.member_step_quorum_label || "").trim(),
      memberStepQuorumPlaceholder: String(view.member_step_quorum_placeholder || "").trim(),
      memberStepTimeoutLabel: String(view.member_step_timeout_label || "").trim(),
      memberStepDependencyLabel: String(view.member_step_dependency_label || "").trim(),
      memberStepOutputFormatLabel: String(view.member_step_output_format_label || "").trim(),
      memberStepAllowedToolsLabel: String(view.member_step_allowed_tools_label || "").trim(),
      memberStepAllowedToolsEmptyLabel: String(view.member_step_allowed_tools_empty_label || "").trim(),
      memberStepBlockedToolsLabel: String(view.member_step_blocked_tools_label || "").trim(),
      memberStepBlockedToolsEmptyLabel: String(view.member_step_blocked_tools_empty_label || "").trim(),
      memberStepSchemaHintPrefix: String(view.member_step_schema_hint_prefix || ""),
      memberStepSchemaHintToolsPrefix: String(view.member_step_schema_hint_tools_prefix || ""),
      memberStepSchemaHintEmptyToolsLabel: String(view.member_step_schema_hint_empty_tools_label || "").trim(),
      toolScopeNotInCatalogReason: String(view.tool_scope_not_in_catalog_reason || "").trim(),
      toolScopeNotEnabledReason: String(view.tool_scope_not_enabled_reason || "").trim(),
      toolScopeToolDescriptionFallback: String(view.tool_scope_tool_description_fallback || "").trim(),
      toolScopeRemoveLabel: String(view.tool_scope_remove_label || "").trim(),
      toolScopeSelectMemberPlaceholder: String(view.tool_scope_select_member_placeholder || "").trim(),
      toolScopeBlockCatalogPlaceholder: String(view.tool_scope_block_catalog_placeholder || "").trim(),
      toolScopeAddProfilePlaceholder: String(view.tool_scope_add_profile_placeholder || "").trim(),
      inputPanelIcon: String(view.input_panel_icon || "").trim(),
      inputPanelTitle: String(view.input_panel_title || "").trim(),
      inputPanelSub: String(view.input_panel_sub || "").trim(),
      inputTaskLabel: String(view.input_task_label || "").trim(),
      inputTaskPlaceholder: String(view.input_task_placeholder || "").trim(),
      inputParamsTitlePrefix: String(view.input_params_title_prefix || "").trim(),
      inputAddParamLabel: String(view.input_add_param_label || "").trim(),
      inputParamSourceLabel: String(view.input_param_source_label || "").trim(),
      inputParamHeaderLabels: {
        name: String(view.input_param_header_labels?.name || "").trim(),
        type: String(view.input_param_header_labels?.type || "").trim(),
        required: String(view.input_param_header_labels?.required || "").trim(),
        description: String(view.input_param_header_labels?.description || "").trim(),
        action: String(view.input_param_header_labels?.action || ""),
      },
      inputParamNamePlaceholder: String(view.input_param_name_placeholder || "").trim(),
      inputParamDescriptionPlaceholder: String(view.input_param_description_placeholder || "").trim(),
      inputParamRemoveTitle: String(view.input_param_remove_title || "").trim(),
      inputParamEnumLabel: String(view.input_param_enum_label || "").trim(),
      inputParamEnumAddLabel: String(view.input_param_enum_add_label || "").trim(),
      inputParamEnumAddValue: String(view.input_param_enum_add_value || "").trim(),
      inputEmptyParamsParts: basicViewPartsFromSchema(view.input_empty_params_parts),
      inputTips: Array.isArray(view.input_tips)
        ? view.input_tips.map((tip) => String(tip || "").trim()).filter(Boolean)
        : [],
      branchPanelTitle: String(view.branch_panel_title || "").trim(),
      branchPanelSub: String(view.branch_panel_sub || "").trim(),
      parallelPanelTitle: String(view.parallel_panel_title || "").trim(),
      parallelPanelSub: String(view.parallel_panel_sub || "").trim(),
      branchRouteMemberLabel: String(view.branch_route_member_label || "").trim(),
      parallelJoinMemberLabel: String(view.parallel_join_member_label || "").trim(),
      branchControllerPlaceholderLabel: String(view.branch_controller_placeholder_label || "").trim(),
      branchEmptyControllerHint: String(view.branch_empty_controller_hint || "").trim(),
      branchConditionTitle: String(view.branch_condition_title || "").trim(),
      branchConditionIntro: String(view.branch_condition_intro || "").trim(),
      branchConditionRowTitlePrefix: String(view.branch_condition_row_title_prefix || "").trim(),
      branchConditionEmptyHint: String(view.branch_condition_empty_hint || "").trim(),
      branchConditionSourcePlaceholder: String(view.branch_condition_source_placeholder || "").trim(),
      branchConditionFieldPlaceholder: String(view.branch_condition_field_placeholder || "").trim(),
      branchConditionNoSchemaLabel: String(view.branch_condition_no_schema_label || "").trim(),
      branchConditionPreviewPrefix: String(view.branch_condition_preview_prefix || "").trim(),
      branchConditionPreviewFallback: String(view.branch_condition_preview_fallback || "").trim(),
      branchFallbackTitle: String(view.branch_fallback_title || "").trim(),
      branchFallbackHint: String(view.branch_fallback_hint || "").trim(),
      addBranchLabel: String(view.add_branch_label || "").trim(),
      addParallelBranchLabel: String(view.add_parallel_branch_label || "").trim(),
      parallelDispatchLabel: String(view.parallel_dispatch_label || "").trim(),
      parallelCollectionLabel: String(view.parallel_collection_label || "").trim(),
      parallelQuorumLabel: String(view.parallel_quorum_label || "").trim(),
      parallelQuorumPlaceholder: String(view.parallel_quorum_placeholder || "").trim(),
      branchDependencyLabel: String(view.branch_dependency_label || "").trim(),
      repeatPanelTitle: String(view.repeat_panel_title || "").trim(),
      repeatPanelSub: String(view.repeat_panel_sub || "").trim(),
      repeatLoopIdLabel: String(view.repeat_loop_id_label || "").trim(),
      repeatLoopIdPlaceholder: String(view.repeat_loop_id_placeholder || "").trim(),
      repeatConditionTitle: String(view.repeat_condition_title || "").trim(),
      repeatConditionIntro: String(view.repeat_condition_intro || "").trim(),
      repeatEmptyBodyHint: String(view.repeat_empty_body_hint || "").trim(),
      repeatMemberPlaceholderLabel: String(view.repeat_member_placeholder_label || "").trim(),
      repeatConditionFieldPlaceholder: String(view.repeat_condition_field_placeholder || "").trim(),
      repeatConditionNoSchemaLabel: String(view.repeat_condition_no_schema_label || "").trim(),
      repeatPreviewLabel: String(view.repeat_preview_label || "").trim(),
      repeatPreviewFallback: String(view.repeat_preview_fallback || "").trim(),
      repeatIterationInputLabel: String(view.repeat_iteration_input_label || "").trim(),
      repeatMaxIterationsLabel: String(view.repeat_max_iterations_label || "").trim(),
      repeatMaxIterationsPlaceholder: String(view.repeat_max_iterations_placeholder || "").trim(),
      repeatTips: Array.isArray(view.repeat_tips)
        ? view.repeat_tips.map((tip) => String(tip || "").trim()).filter(Boolean)
        : [],
      repeatCanvasWhileLabel: String(view.repeat_canvas_while_label || "").trim(),
      repeatCanvasNotLabel: String(view.repeat_canvas_not_label || "").trim(),
      repeatCanvasMissingMaxIterationsLabel: String(view.repeat_canvas_missing_max_iterations_label || "").trim(),
      repeatCanvasMaxIterationsPrefix: String(view.repeat_canvas_max_iterations_prefix || ""),
      repeatCanvasLoopBackPrefix: String(view.repeat_canvas_loop_back_prefix || ""),
      repeatCanvasExitPrefix: String(view.repeat_canvas_exit_prefix || ""),
      repeatCanvasExitFallback: String(view.repeat_canvas_exit_fallback || "").trim(),
      repeatIterationRuntimeDefaultLabel: String(view.repeat_iteration_runtime_default_label || "").trim(),
      repeatIterationCarryLabel: String(view.repeat_iteration_carry_label || "").trim(),
      repeatIterationReuseUnsupportedLabel: String(view.repeat_iteration_reuse_unsupported_label || "").trim(),
      repeatIterationFeedsUnsupportedPrefix: String(view.repeat_iteration_feeds_unsupported_prefix || ""),
      repeatIterationUnsupportedPrefix: String(view.repeat_iteration_unsupported_prefix || ""),
      addStepTitle: String(view.add_step_title || "").trim(),
      inputStepCardTitle: String(view.input_step_card_title || "").trim(),
      inputStepCardDescFallback: String(view.input_step_card_desc_fallback || "").trim(),
      branchStepCardTitle: String(view.branch_step_card_title || "").trim(),
      branchStepCardDesc: String(view.branch_step_card_desc || "").trim(),
      parallelStepCardTitle: String(view.parallel_step_card_title || "").trim(),
      parallelStepCardDescPrefix: String(view.parallel_step_card_desc_prefix || ""),
      parallelStepCardCollectionFallback: String(view.parallel_step_card_collection_fallback || "").trim(),
      repeatStepCardTitle: String(view.repeat_step_card_title || "").trim(),
      repeatStepCardDescPrefix: String(view.repeat_step_card_desc_prefix || ""),
      repeatStepCardDescFallback: String(view.repeat_step_card_desc_fallback || "").trim(),
      memberStepCardTitleFallback: String(view.member_step_card_title_fallback || "").trim(),
      pickerKickoffTitle: String(view.picker_kickoff_title || "").trim(),
      pickerKickoffSub: String(view.picker_kickoff_sub || "").trim(),
      pickerKickoffHint: String(view.picker_kickoff_hint || "").trim(),
      pickerTitle: String(view.picker_title || "").trim(),
      pickerSub: String(view.picker_sub || "").trim(),
      pickerSearchIcon: String(view.picker_search_icon || "").trim(),
      pickerSearchPlaceholder: String(view.picker_search_placeholder || "").trim(),
      pickerMembersLabel: String(view.picker_members_label || "").trim(),
      pickerFlowLabel: String(view.picker_flow_label || "").trim(),
      pickerEmptyMembersHint: String(view.picker_empty_members_hint || "").trim(),
      pickerNewBadgeLabel: String(view.picker_new_badge_label || "").trim(),
      flowPrimitiveRows: basicFlowPrimitiveRowsFromSchema(view.flow_primitive_rows),
    };
    return Object.entries(out).every(([key, value]) => {
      if (key === "inputParamHeaderLabels") {
        return value.name && value.type && value.required && value.description;
      }
      return Array.isArray(value) ? value.length : !!value;
    })
      ? out
      : null;
  }

  function basicViewPartsFromSchema(parts) {
    if (!Array.isArray(parts)) return [];
    return parts
      .map((part, index) => {
        if (!part || typeof part !== "object") return null;
        const kind = String(part.kind || "text").trim();
        const text = String(part.text || "");
        if (!text) return null;
        return {
          key: String(part.key || `${kind}-${index}`),
          kind: kind === "code" || kind === "strong" ? kind : "text",
          text,
        };
      })
      .filter(Boolean);
  }

  function basicFlowPrimitiveRowsFromSchema(rows) {
    if (!Array.isArray(rows)) return [];
    return rows
      .map((row) => {
        if (!row || typeof row !== "object") return null;
        const id = String(row.id || "").trim();
        const glyph = String(row.glyph || "").trim();
        const tint = String(row.tint || "").trim();
        const label = String(row.label || "").trim();
        const sub = String(row.sub || "").trim();
        if (!id || !glyph || !tint || !label || !sub) return null;
        return { id, glyph, tint, label, sub, isNew: Boolean(row.is_new) };
      })
      .filter(Boolean);
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

  function viewStringMapFromSchema(value) {
    if (!value || typeof value !== "object" || Array.isArray(value)) return {};
    return Object.fromEntries(
      Object.entries(value)
        .map(([key, label]) => [String(key || "").trim(), String(label || "").trim()])
        .filter(([key, label]) => key && label),
    );
  }

  function launchViewFromSchema(schema) {
    const view = schema?.mob_definition?.editor_launch_view;
    if (!view || typeof view !== "object") return null;
    const out = {
      launchTitle: String(view.launch_title || "").trim(),
      graphLaunchTitle: String(view.graph_launch_title || "").trim(),
      resumeSessionLabel: String(view.resume_session_label || "").trim(),
      resumeSessionPlaceholder: String(view.resume_session_placeholder || "").trim(),
      forkSourceLabel: String(view.fork_source_label || "").trim(),
      forkContextLabel: String(view.fork_context_label || "").trim(),
      graphForkContextLabel: String(view.graph_fork_context_label || "").trim(),
      budgetPolicyLabel: String(view.budget_policy_label || "").trim(),
      fixedBudgetLabel: String(view.fixed_budget_label || "").trim(),
      fixedBudgetDefaultValue: Number(view.fixed_budget_default_value),
      unsupportedLabelSeparator: String(view.unsupported_label_separator || ""),
      unsupportedReasonPrefix: String(view.unsupported_reason_prefix || ""),
      unsupportedReasonSuffix: String(view.unsupported_reason_suffix || ""),
      launchModesContractLabel: String(view.launch_modes_contract_label || "").trim(),
      forkContextsContractLabel: String(view.fork_contexts_contract_label || "").trim(),
      budgetSplitPoliciesContractLabel: String(view.budget_split_policies_contract_label || "").trim(),
      launchModeLabels: viewStringMapFromSchema(view.launch_mode_labels),
      forkContextLabels: viewStringMapFromSchema(view.fork_context_labels),
      budgetSplitPolicyLabels: viewStringMapFromSchema(view.budget_split_policy_labels),
    };
    const stringsOk = Object.entries(out).every(([key, value]) => {
      if (typeof value === "number") return Number.isFinite(value) && value > 0;
      if (value && typeof value === "object") return Object.keys(value).length > 0;
      return !!value;
    });
    return stringsOk ? out : null;
  }

  function launchViewForState(launchView) {
    const view = launchView && typeof launchView === "object" ? launchView : null;
    return {
      launchTitle: String(view?.launchTitle || ""),
      graphLaunchTitle: String(view?.graphLaunchTitle || ""),
      resumeSessionLabel: String(view?.resumeSessionLabel || ""),
      resumeSessionPlaceholder: String(view?.resumeSessionPlaceholder || ""),
      forkSourceLabel: String(view?.forkSourceLabel || ""),
      forkContextLabel: String(view?.forkContextLabel || ""),
      graphForkContextLabel: String(view?.graphForkContextLabel || ""),
      budgetPolicyLabel: String(view?.budgetPolicyLabel || ""),
      fixedBudgetLabel: String(view?.fixedBudgetLabel || ""),
      fixedBudgetDefaultValue: Number(view?.fixedBudgetDefaultValue || 0),
      unsupportedLabelSeparator: String(view?.unsupportedLabelSeparator || ""),
      unsupportedReasonPrefix: String(view?.unsupportedReasonPrefix || ""),
      unsupportedReasonSuffix: String(view?.unsupportedReasonSuffix || ""),
      launchModesContractLabel: String(view?.launchModesContractLabel || ""),
      forkContextsContractLabel: String(view?.forkContextsContractLabel || ""),
      budgetSplitPoliciesContractLabel: String(view?.budgetSplitPoliciesContractLabel || ""),
      launchModeLabels: view?.launchModeLabels && typeof view.launchModeLabels === "object" ? view.launchModeLabels : {},
      forkContextLabels: view?.forkContextLabels && typeof view.forkContextLabels === "object" ? view.forkContextLabels : {},
      budgetSplitPolicyLabels: view?.budgetSplitPolicyLabels && typeof view.budgetSplitPolicyLabels === "object" ? view.budgetSplitPolicyLabels : {},
    };
  }

  function graphTemplateViewFromSchema(schema) {
    const view = schema?.mob_definition?.editor_graph_template_view;
    if (!view || typeof view !== "object") return null;
    const out = {
      templateEyebrow: String(view.template_eyebrow || "").trim(),
      summaryTitle: String(view.summary_title || "").trim(),
      triggersTitle: String(view.triggers_title || "").trim(),
      triggerLabelsLabel: String(view.trigger_labels_label || "").trim(),
      triggerDefaultLabel: String(view.trigger_default_label || "").trim(),
      defaultYesLabel: String(view.default_yes_label || "").trim(),
      defaultNoLabel: String(view.default_no_label || "").trim(),
      summaryMembersLabel: String(view.summary_members_label || "").trim(),
      summaryInstancesLabel: String(view.summary_instances_label || "").trim(),
      summaryTerminalsLabel: String(view.summary_terminals_label || "").trim(),
      summaryEdgesLabel: String(view.summary_edges_label || "").trim(),
      summaryFramesLabel: String(view.summary_frames_label || "").trim(),
      summaryMembersValueTemplate: String(view.summary_members_value_template || "").trim(),
      quickStartTitle: String(view.quick_start_title || "").trim(),
      quickStartRows: graphTemplateQuickStartRowsFromSchema(view.quick_start_rows),
    };
    return out.templateEyebrow && out.summaryTitle && out.triggersTitle && out.triggerLabelsLabel
      && out.triggerDefaultLabel && out.defaultYesLabel && out.defaultNoLabel
      && out.summaryMembersLabel && out.summaryInstancesLabel && out.summaryTerminalsLabel
      && out.summaryEdgesLabel && out.summaryFramesLabel && out.summaryMembersValueTemplate
      && out.quickStartTitle && out.quickStartRows.length
      ? out
      : null;
  }

  function graphTemplateQuickStartRowsFromSchema(rows) {
    if (!Array.isArray(rows)) return [];
    return rows
      .map((row, rowIndex) => ({
        key: `quick-start-${rowIndex}`,
        parts: basicViewPartsFromSchema(row),
      }))
      .filter((row) => row.parts.length);
  }

  function graphTemplateViewForState(templateView) {
    const view = templateView && typeof templateView === "object" ? templateView : null;
    return {
      templateEyebrow: String(view?.templateEyebrow || ""),
      summaryTitle: String(view?.summaryTitle || ""),
      triggersTitle: String(view?.triggersTitle || ""),
      triggerLabelsLabel: String(view?.triggerLabelsLabel || ""),
      triggerDefaultLabel: String(view?.triggerDefaultLabel || ""),
      defaultYesLabel: String(view?.defaultYesLabel || ""),
      defaultNoLabel: String(view?.defaultNoLabel || ""),
      summaryMembersLabel: String(view?.summaryMembersLabel || ""),
      summaryInstancesLabel: String(view?.summaryInstancesLabel || ""),
      summaryTerminalsLabel: String(view?.summaryTerminalsLabel || ""),
      summaryEdgesLabel: String(view?.summaryEdgesLabel || ""),
      summaryFramesLabel: String(view?.summaryFramesLabel || ""),
      summaryMembersValueTemplate: String(view?.summaryMembersValueTemplate || ""),
      quickStartTitle: String(view?.quickStartTitle || ""),
      quickStartRows: Array.isArray(view?.quickStartRows) ? view.quickStartRows : [],
    };
  }

  function graphViewFromSchema(schema) {
    const view = schema?.mob_definition?.editor_graph_view;
    if (!view || typeof view !== "object") return null;
    const out = {
      zoomOutTitle: String(view.zoom_out_title || "").trim(),
      fitTitle: String(view.fit_title || "").trim(),
      zoomInTitle: String(view.zoom_in_title || "").trim(),
      portDragTitle: String(view.port_drag_title || "").trim(),
      addNodeSearchIcon: String(view.add_node_search_icon || "").trim(),
      addNodeSearchPlaceholder: String(view.add_node_search_placeholder || "").trim(),
      addNodeCloseLabel: String(view.add_node_close_label || "").trim(),
      addNodeCloseTitle: String(view.add_node_close_title || "").trim(),
      addNodeAgentsLabel: String(view.add_node_agents_label || "").trim(),
      addNodeControlsLabel: String(view.add_node_controls_label || "").trim(),
      addNodeEmptyPrefix: String(view.add_node_empty_prefix || ""),
      addNodeEmptySuffix: String(view.add_node_empty_suffix || ""),
      addNodeJumpLabel: String(view.add_node_jump_label || "").trim(),
      gatePaletteRows: graphGatePaletteRowsFromSchema(view.gate_palette_rows),
      gateKindLabels: viewStringMapFromSchema(view.graph_gate_kind_labels),
      terminalKindLabels: viewStringMapFromSchema(view.graph_terminal_kind_labels),
      frameKindLabels: viewStringMapFromSchema(view.graph_frame_kind_labels),
      edgeKindLabels: viewStringMapFromSchema(view.graph_edge_kind_labels),
      inspectorDeleteLabel: String(view.inspector_delete_label || "").trim(),
      inspectorLabelTitle: String(view.inspector_label_title || "").trim(),
      inspectorKindTitle: String(view.inspector_kind_title || "").trim(),
      inspectorRuntimeDefaultLabel: String(view.inspector_runtime_default_label || "").trim(),
      instanceEyebrow: String(view.instance_eyebrow || "").trim(),
      instanceIdLineTemplate: String(view.instance_id_line_template || "").trim(),
      instanceMemberRoleTemplate: String(view.instance_member_role_template || "").trim(),
      instanceEditMemberLabel: String(view.instance_edit_member_label || "").trim(),
      instanceModelLabel: String(view.instance_model_label || "").trim(),
      instanceSchemaLabel: String(view.instance_schema_label || "").trim(),
      instanceToolsLabel: String(view.instance_tools_label || "").trim(),
      instanceMemberHint: String(view.instance_member_hint || "").trim(),
      instancePositionTitle: String(view.instance_position_title || "").trim(),
      instancePositionStageLabel: String(view.instance_position_stage_label || "").trim(),
      instancePositionSlotLabel: String(view.instance_position_slot_label || "").trim(),
      instanceOutputTitleTemplate: String(view.instance_output_title_template || "").trim(),
      instanceOutputRequiredLabel: String(view.instance_output_required_label || "").trim(),
      instanceOutputHint: String(view.instance_output_hint || "").trim(),
      instanceOutputOpenMemberLabel: String(view.instance_output_open_member_label || "").trim(),
      gateEyebrowTemplate: String(view.gate_eyebrow_template || "").trim(),
      gateIdLineTemplate: String(view.gate_id_line_template || "").trim(),
      gateQuorumIncomingTemplate: String(view.gate_quorum_incoming_template || "").trim(),
      gateMemberOptionTemplate: String(view.gate_member_option_template || "").trim(),
      terminalEyebrowTemplate: String(view.terminal_eyebrow_template || "").trim(),
      terminalIdLineTemplate: String(view.terminal_id_line_template || "").trim(),
      edgeEyebrowTemplate: String(view.edge_eyebrow_template || "").trim(),
      edgeTitleTemplate: String(view.edge_title_template || "").trim(),
      edgeIdLineTemplate: String(view.edge_id_line_template || "").trim(),
      edgeFieldPlaceholder: String(view.edge_field_placeholder || "").trim(),
      edgeFieldNoSchemaPlaceholder: String(view.edge_field_no_schema_placeholder || "").trim(),
      gateCollectionTitle: String(view.gate_collection_title || "").trim(),
      gateJoinMemberLabel: String(view.gate_join_member_label || "").trim(),
      gateJoinMemberPlaceholder: String(view.gate_join_member_placeholder || "").trim(),
      gateJoinMemberHint: String(view.gate_join_member_hint || "").trim(),
      gateDispatchTitle: String(view.gate_dispatch_title || "").trim(),
      gateDispatchHint: String(view.gate_dispatch_hint || "").trim(),
      gateConditionsTitle: String(view.gate_conditions_title || "").trim(),
      gateEmptyBranchHint: String(view.gate_empty_branch_hint || "").trim(),
      gateWiringTitle: String(view.gate_wiring_title || "").trim(),
      gateIncomingLabel: String(view.gate_incoming_label || "").trim(),
      gateOutgoingLabel: String(view.gate_outgoing_label || "").trim(),
      branchConditionModeConditionLabel: String(view.branch_condition_mode_condition_label || "").trim(),
      branchConditionModeFallbackLabel: String(view.branch_condition_mode_fallback_label || "").trim(),
      branchConditionTargetPrefix: String(view.branch_condition_target_prefix || "").trim(),
      graphConditionTargetMissingLabel: String(view.graph_condition_target_missing_label || "").trim(),
      graphConditionOwnerOptionTemplate: String(view.graph_condition_owner_option_template || "").trim(),
      graphConditionFieldOptionTemplate: String(view.graph_condition_field_option_template || "").trim(),
      graphInputParamSourceLabel: String(view.branch_input_param_source_label || "").trim(),
      sourceFileLabel: String(view.source_file_label || "").trim(),
      sourceFileAriaLabel: String(view.source_file_aria_label || "").trim(),
      sourceFileGlyph: String(view.source_file_glyph || "").trim(),
      sourceFileRoleLabel: String(view.source_file_role_label || "").trim(),
      branchConditionFieldPlaceholder: String(view.branch_condition_field_placeholder || "").trim(),
      branchConditionNoOptionsHint: String(view.branch_condition_no_options_hint || "").trim(),
      edgeConditionTitle: String(view.edge_condition_title || "").trim(),
      edgeNoConditionOptionsHint: String(view.edge_no_condition_options_hint || "").trim(),
      edgeOwnerPlaceholder: String(view.edge_owner_placeholder || "").trim(),
      edgeFromTitle: String(view.edge_from_title || "").trim(),
      edgeToTitle: String(view.edge_to_title || "").trim(),
      edgeRowInstanceLabel: String(view.edge_row_instance_label || "").trim(),
      edgeRowMemberLabel: String(view.edge_row_member_label || "").trim(),
      edgeRowSchemaLabel: String(view.edge_row_schema_label || "").trim(),
      edgeRowMissingValue: String(view.edge_row_missing_value || "").trim(),
      edgeTerminalMemberValue: String(view.edge_terminal_member_value || "").trim(),
    };
    return out.zoomOutTitle && out.fitTitle && out.zoomInTitle && out.portDragTitle
      && out.addNodeSearchIcon && out.addNodeSearchPlaceholder && out.addNodeCloseLabel
      && out.addNodeCloseTitle && out.addNodeAgentsLabel && out.addNodeControlsLabel
      && out.addNodeEmptyPrefix && out.addNodeEmptySuffix && out.addNodeJumpLabel
      && out.gatePaletteRows.length
      && Object.keys(out.gateKindLabels).length
      && Object.keys(out.terminalKindLabels).length
      && Object.keys(out.frameKindLabels).length
      && Object.keys(out.edgeKindLabels).length
      && out.inspectorDeleteLabel && out.inspectorLabelTitle && out.inspectorKindTitle
      && out.inspectorRuntimeDefaultLabel && out.instanceEyebrow && out.instanceIdLineTemplate
      && out.instanceMemberRoleTemplate && out.instanceEditMemberLabel && out.instanceModelLabel
      && out.instanceSchemaLabel && out.instanceToolsLabel && out.instanceMemberHint
      && out.instancePositionTitle && out.instancePositionStageLabel && out.instancePositionSlotLabel
      && out.instanceOutputTitleTemplate && out.instanceOutputRequiredLabel && out.instanceOutputHint
      && out.instanceOutputOpenMemberLabel && out.gateEyebrowTemplate && out.gateIdLineTemplate
      && out.gateQuorumIncomingTemplate && out.gateMemberOptionTemplate
      && out.terminalEyebrowTemplate && out.terminalIdLineTemplate && out.edgeEyebrowTemplate
      && out.edgeTitleTemplate && out.edgeIdLineTemplate && out.edgeFieldPlaceholder
      && out.edgeFieldNoSchemaPlaceholder
      && out.gateCollectionTitle
      && out.gateJoinMemberLabel && out.gateJoinMemberPlaceholder && out.gateJoinMemberHint
      && out.gateDispatchTitle && out.gateDispatchHint && out.gateConditionsTitle
      && out.gateEmptyBranchHint && out.gateWiringTitle && out.gateIncomingLabel
      && out.gateOutgoingLabel && out.branchConditionModeConditionLabel
      && out.branchConditionModeFallbackLabel && out.branchConditionTargetPrefix
      && out.graphConditionTargetMissingLabel && out.graphConditionOwnerOptionTemplate
      && out.graphConditionFieldOptionTemplate
      && out.graphInputParamSourceLabel && out.sourceFileLabel
      && out.sourceFileAriaLabel && out.sourceFileGlyph && out.sourceFileRoleLabel
      && out.branchConditionFieldPlaceholder && out.branchConditionNoOptionsHint
      && out.edgeConditionTitle && out.edgeNoConditionOptionsHint && out.edgeOwnerPlaceholder
      && out.edgeFromTitle && out.edgeToTitle && out.edgeRowInstanceLabel
      && out.edgeRowMemberLabel && out.edgeRowSchemaLabel && out.edgeRowMissingValue
      && out.edgeTerminalMemberValue
      ? out
      : null;
  }

  function graphGatePaletteRowsFromSchema(rows) {
    if (!Array.isArray(rows)) return [];
    return rows
      .map((row) => {
        if (!row || typeof row !== "object") return null;
        const id = String(row.id || "").trim();
        const glyph = String(row.glyph || "").trim();
        const label = String(row.label || "").trim();
        const meta = String(row.meta || "").trim();
        if (!id || !glyph || !label || !meta) return null;
        return { id, glyph, label, meta };
      })
      .filter(Boolean);
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
      addNodeEmptyPrefix: String(view?.addNodeEmptyPrefix || ""),
      addNodeEmptySuffix: String(view?.addNodeEmptySuffix || ""),
      addNodeJumpLabel: String(view?.addNodeJumpLabel || ""),
      gatePaletteRows: Array.isArray(view?.gatePaletteRows) ? view.gatePaletteRows : [],
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

  function agentEditorControlState({ member, instances = [], schemas = [], contract, deploySettings, modelCatalog = [], agentDetailView = null } = {}) {
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
    return {
      placedAt,
      placedCount,
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
      backendLabel: view.backendLabel,
      inlinePeerNotificationsLabel: view.inlinePeerNotificationsLabel,
      inlinePeerNotificationsPlaceholder: view.inlinePeerNotificationsPlaceholder,
      systemPromptTitle: view.systemPromptTitle,
      applySkeletonLabel: view.applySkeletonLabel,
      applySkeletonTitle: view.applySkeletonTitle,
      systemPromptPlaceholder: view.systemPromptPlaceholder,
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
      placeholderOption: { value: "", label: view.addAgentPlaceholderLabel },
      value: "",
    };
  }

  function agentDefinitionAddErrorState(result = null, agentView = null) {
    const view = agentViewForState(agentView);
    const error = String(result?.error || "").trim();
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

  function memberSchemaChangeErrorState(result = null) {
    const error = String(result?.error || "").trim();
    return {
      hasError: !!error,
      text: error,
      rawError: error,
    };
  }

  function schemaDefinitionAddErrorState(result = null) {
    return memberSchemaChangeErrorState(result);
  }

  function schemaFieldAddErrorState(result = null) {
    return memberSchemaChangeErrorState(result);
  }

  function inputParamAddErrorState(result = null) {
    return memberSchemaChangeErrorState(result);
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

  function childLanes(step) {
    if (!step) return [];
    if (step.type === "repeat") return [{ id: "body", steps: step.steps || [] }];
    if (step.type === "branch") {
      return [
        ...(step.branches || []).map((branch) => ({ id: branch.id, steps: branch.steps || [] })),
        { id: "fallback", steps: step.fallback || [] },
      ];
    }
    if (step.type === "parallel") {
      return (step.branches || []).map((branch) => ({ id: branch.id, steps: branch.steps || [] }));
    }
    return [];
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

  function collectFlowStepIds(steps, out = new Set()) {
    for (const step of steps || []) {
      const id = String(step?.id || "").trim();
      if (id) out.add(id);
      for (const lane of childLanes(step || {})) collectFlowStepIds(lane.steps, out);
    }
    return out;
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

  function reconcileDeletedFlowStepReferences(flow, deletedId) {
    if (!flow || typeof flow !== "object") return flow;
    const target = String(deletedId || "").trim();
    if (!target) return flow;
    const steps = reconcileDeletedFlowStepReferencesInSteps(flow.steps || [], target);
    return steps === flow.steps ? flow : { ...flow, steps };
  }

  function reconcileDeletedFlowStepReferencesInSteps(steps, deletedId) {
    let changed = false;
    const next = (steps || []).map((step) => {
      const reconciled = reconcileDeletedFlowStepReferencesInStep(step, deletedId);
      if (reconciled !== step) changed = true;
      return reconciled;
    });
    return changed ? next : steps;
  }

  function reconcileDeletedFlowStepReferencesInStep(step, deletedId) {
    if (!step || typeof step !== "object") return step;
    let next = clearDeletedLaunchSource(step, deletedId);
    if (step.type === "repeat") {
      const cond = clearDeletedStepCondition(step.cond, deletedId);
      const until = clearDeletedStepConditionText(step.until, deletedId, cond);
      const nested = reconcileDeletedFlowStepReferencesInSteps(step.steps || [], deletedId);
      if (cond !== step.cond || until !== step.until || nested !== step.steps) {
        next = { ...next, cond, until, steps: nested };
      }
    }
    if (step.type === "branch") {
      let changed = false;
      const branches = (step.branches || []).map((branch) => {
        const cond = clearDeletedStepCondition(branch.cond, deletedId);
        const condition = clearDeletedStepConditionText(branch.condition, deletedId, cond);
        const branchSteps = reconcileDeletedFlowStepReferencesInSteps(branch.steps || [], deletedId);
        if (cond === branch.cond && condition === branch.condition && branchSteps === branch.steps) return branch;
        changed = true;
        return { ...branch, cond, condition, steps: branchSteps };
      });
      const fallback = Array.isArray(step.fallback)
        ? reconcileDeletedFlowStepReferencesInSteps(step.fallback, deletedId)
        : step.fallback;
      if (fallback !== step.fallback) changed = true;
      if (changed) next = { ...next, branches, fallback };
    }
    if (step.type === "parallel") {
      let changed = false;
      const branches = (step.branches || []).map((branch) => {
        const branchSteps = reconcileDeletedFlowStepReferencesInSteps(branch.steps || [], deletedId);
        if (branchSteps === branch.steps) return branch;
        changed = true;
        return { ...branch, steps: branchSteps };
      });
      if (changed) next = { ...next, branches };
    }
    return next;
  }

  function clearDeletedLaunchSource(source, deletedId) {
    const mode = launchModeFromAuthoringSource(source);
    if (!mode || mode.kind !== "Fork" || String(mode.from || "").trim() !== deletedId) return source;
    return {
      ...source,
      launchMode: freshLaunchModePreservingBudget(mode),
    };
  }

  function freshLaunchModePreservingBudget(mode) {
    const budgetSplitPolicy = mode?.budgetSplitPolicy;
    return budgetSplitPolicy ? { kind: "Fresh", budgetSplitPolicy } : { kind: "Fresh" };
  }

  function clearDeletedStepCondition(cond, deletedId) {
    if (!cond || typeof cond !== "object") return cond;
    const stepId = String(cond.stepId || cond.step_id || "").trim();
    if (stepId !== deletedId) return cond;
    return {};
  }

  function clearDeletedStepConditionText(text, deletedId, preferredCond) {
    if (preferredCond !== undefined) {
      if (preferredCond && preferredCond.field) return editorConditionText(preferredCond);
      return "";
    }
    if (!text) return text;
    const parsed = parseEditorConditionText(text);
    if (!parsed || parsed.namespace === "params" || parsed.stepId !== deletedId) return text;
    return "";
  }

  function reconcileFlowMemberSchemas(flow, members) {
    if (!flow || typeof flow !== "object") return flow;
    const memberById = new Map((members || []).map((member) => [member.id, member]));
    const steps = reconcileMemberSchemasInSteps(flow.steps || [], memberById);
    return steps === flow.steps ? flow : { ...flow, steps };
  }

  function renameSchemaDefinition({ schemas, members, flow } = {}, oldId, newId) {
    const previousId = String(oldId || "").trim();
    const nextId = String(newId || "").trim();
    const sourceSchemas = Array.isArray(schemas) ? schemas : [];
    const sourceMembers = Array.isArray(members) ? members : [];
    if (!previousId || !nextId || previousId === nextId) {
      return { schemas: sourceSchemas, members: sourceMembers, flow, renamed: false, selection: null };
    }
    if (sourceSchemas.some((schema) => String(schema?.id || "").trim() === nextId)) {
      return {
        schemas: sourceSchemas,
        members: sourceMembers,
        flow,
        renamed: false,
        selection: null,
        reason: "duplicate_schema_id",
      };
    }
    let found = false;
    const nextSchemas = sourceSchemas.map((schema) => {
      if (String(schema?.id || "").trim() !== previousId) return schema;
      found = true;
      return { ...schema, id: nextId };
    });
    if (!found) {
      return {
        schemas: sourceSchemas,
        members: sourceMembers,
        flow,
        renamed: false,
        selection: null,
        reason: "unknown_schema_id",
      };
    }
    const nextMembers = sourceMembers.map((member) =>
      String(member?.schema || "").trim() === previousId
        ? { ...member, schema: nextId }
        : member
    );
    return {
      schemas: nextSchemas,
      members: nextMembers,
      flow: reconcileFlowMemberSchemas(flow, nextMembers),
      renamed: true,
      selection: { kind: "schema", id: nextId },
    };
  }

  function reconcileFlowMemberSteps(flow, members) {
    if (!flow || typeof flow !== "object") return flow;
    const memberIds = memberIdSet(members);
    const steps = pruneMissingMemberSteps(flow.steps || [], memberIds);
    return steps === flow.steps ? flow : { ...flow, steps };
  }

  function pruneMissingMemberSteps(steps, memberIds) {
    let changed = false;
    const next = [];
    for (const step of steps || []) {
      const pruned = pruneMissingMemberStep(step, memberIds);
      if (!pruned) {
        changed = true;
        continue;
      }
      if (pruned !== step) changed = true;
      next.push(pruned);
    }
    return changed ? next : steps;
  }

  function pruneMissingMemberStep(step, memberIds) {
    if (!step || typeof step !== "object") return step;
    if (step.type === "member") {
      const role = String(step.role || "").trim();
      return role && memberIds.has(role) ? step : null;
    }
    if (step.type === "repeat") {
      const steps = pruneMissingMemberSteps(step.steps || [], memberIds);
      if (!steps.length) return null;
      return steps === step.steps ? step : { ...step, steps };
    }
    if (step.type === "branch" || step.type === "parallel") {
      let changed = false;
      const branches = (step.branches || []).flatMap((branch) => {
        const branchSteps = pruneMissingMemberSteps(branch?.steps || [], memberIds);
        if (branchSteps !== branch.steps) changed = true;
        if (!branchSteps.length) {
          changed = true;
          return [];
        }
        return branchSteps === branch.steps ? [branch] : [{ ...branch, steps: branchSteps }];
      });
      const fallback = Array.isArray(step.fallback)
        ? pruneMissingMemberSteps(step.fallback, memberIds)
        : step.fallback;
      if (fallback !== step.fallback) changed = true;
      const hasFallback = Array.isArray(fallback) && fallback.length > 0;
      if (!branches.length && !hasFallback) return null;
      return changed ? { ...step, branches, fallback } : step;
    }
    return step;
  }

  function reconcileFlowControlRoles(flow, members) {
    if (!flow || typeof flow !== "object") return flow;
    const memberIds = memberIdSet(members);
    const steps = reconcileControlRolesInSteps(flow.steps || [], memberIds);
    return steps === flow.steps ? flow : { ...flow, steps };
  }

  function reconcileGraphControlRoles(instances, members) {
    const memberIds = memberIdSet(members);
    let changed = false;
    const next = (instances || []).map((instance) => {
      const reconciled = reconcileControlRoleObject(instance, memberIds);
      if (reconciled !== instance) changed = true;
      return reconciled;
    });
    return changed ? next : instances;
  }

  function reconcileGraphMemberInstances({ instances, edges }, members) {
    const sourceInstances = Array.isArray(instances) ? instances : [];
    const sourceEdges = Array.isArray(edges) ? edges : [];
    const memberIds = memberIdSet(members);
    const keptIds = new Set();
    let instancesChanged = false;
    const nextInstances = [];
    for (const instance of sourceInstances) {
      const memberId = String(instance?.memberId || "").trim();
      const keep = !memberId || memberIds.has(memberId);
      if (!keep) {
        instancesChanged = true;
        continue;
      }
      const id = String(instance?.id || "").trim();
      if (id) keptIds.add(id);
      nextInstances.push(instance);
    }
    let edgesChanged = false;
    const nextEdges = sourceEdges.filter((edge) => {
      const from = String(edge?.from || "").trim();
      const to = String(edge?.to || "").trim();
      const keep = (!from || keptIds.has(from)) && (!to || keptIds.has(to));
      if (!keep) edgesChanged = true;
      return keep;
    });
    return {
      instances: instancesChanged ? nextInstances : instances,
      edges: edgesChanged ? nextEdges : edges,
    };
  }

  function reconcileFlowLaunchSources(flow, members) {
    if (!flow || typeof flow !== "object") return flow;
    const allowedSources = flowLaunchSourceSet(flow, members);
    const steps = reconcileLaunchSourcesInSteps(flow.steps || [], allowedSources);
    return steps === flow.steps ? flow : { ...flow, steps };
  }

  function reconcileGraphLaunchSources(instances, members) {
    const allowedSources = graphLaunchSourceSet(instances, members);
    let changed = false;
    const next = (instances || []).map((instance) => {
      const reconciled = reconcileLaunchSourceObject(instance, allowedSources);
      if (reconciled !== instance) changed = true;
      return reconciled;
    });
    return changed ? next : instances;
  }

  function reconcileFlowStepToolScopes(flow, members) {
    if (!flow || typeof flow !== "object") return flow;
    const memberTools = memberToolIndex(members);
    const steps = reconcileToolScopesInSteps(flow.steps || [], (step) => memberTools.get(step?.role) || new Set());
    return steps === flow.steps ? flow : { ...flow, steps };
  }

  function reconcileGraphStepToolScopes(instances, members) {
    const memberTools = memberToolIndex(members);
    let changed = false;
    const next = (instances || []).map((instance) => {
      if (!instance?.memberId) return instance;
      const reconciled = reconcileToolScopeObject(instance, memberTools.get(instance.memberId) || new Set());
      if (reconciled !== instance) changed = true;
      return reconciled;
    });
    return changed ? next : instances;
  }

  function reconcileAuthoringForMembers({ flow, instances, edges, mobSettings, previousMembers, members } = {}) {
    const nextMembers = Array.isArray(members) ? members : [];
    let nextFlow = reconcileFlowMemberSteps(flow, nextMembers);
    nextFlow = reconcileFlowMemberSchemas(nextFlow, nextMembers);
    nextFlow = reconcileFlowControlRoles(nextFlow, nextMembers);
    nextFlow = reconcileFlowLaunchSources(nextFlow, nextMembers);
    nextFlow = reconcileFlowStepToolScopes(nextFlow, nextMembers);
    const memberSynced = reconcileGraphMemberInstances({ instances, edges }, nextMembers);
    let nextInstances = reconcileGraphControlRoles(memberSynced.instances, nextMembers);
    nextInstances = reconcileGraphLaunchSources(nextInstances, nextMembers);
    nextInstances = reconcileGraphStepToolScopes(nextInstances, nextMembers);
    return {
      flow: nextFlow,
      instances: nextInstances,
      edges: memberSynced.edges,
      mobSettings: reconcileMobSettingsProfiles(mobSettings, previousMembers, nextMembers),
    };
  }

  function reconcileMemberSkillRefs(members, skillRealms, options = {}) {
    const knownSkills = skillIdSet(skillRealms);
    if (knownSkills.size === 0 && !options.strictEmpty) return members;
    let changed = false;
    const next = (members || []).map((member) => {
      if (!member || typeof member !== "object") return member;
      const skills = normalizeStringList(member.skills).filter((skill) => knownSkills.has(skill));
      if (JSON.stringify(skills) === JSON.stringify(normalizeStringList(member.skills))) return member;
      changed = true;
      return { ...member, skills };
    });
    return changed ? next : members;
  }

  function reconcileMemberSchemaRefs(members, schemas, options = {}) {
    const knownSchemas = new Set((Array.isArray(schemas) ? schemas : [])
      .map((schema) => String(schema?.id || "").trim())
      .filter(Boolean));
    if (knownSchemas.size === 0 && !options.strictEmpty) return members;
    let changed = false;
    const next = (members || []).map((member) => {
      if (!member || typeof member !== "object") return member;
      const schema = String(member.schema || "").trim();
      if (!schema || knownSchemas.has(schema)) return member;
      changed = true;
      return { ...member, schema: "" };
    });
    return changed ? next : members;
  }

  function reconcileDeploySettingsWithContract(settings, contract, modelCatalog, options = {}) {
    const source = deploySettingsForUi(settings);
    let next = source;
    const write = (key, value) => {
      if (next[key] === value) return;
      if (next === source) next = { ...source };
      next[key] = value;
    };
    const command = String(contract?.deploy_settings?.command || "").trim();
    if (command) write("command", command);
    reconcileStringField(next, write, "surface", contract?.deploy_settings?.surfaces);
    reconcileStringField(next, write, "trustPolicy", contract?.deploy_settings?.trust_policies);
    reconcileStringField(next, write, "realmBackend", contract?.deploy_settings?.realm_backends);
    const modelIds = (modelCatalog || [])
      .map((model) => String(model?.id || "").trim())
      .filter(Boolean);
    if ((modelIds.length || options.strictEmptyModels) && source.model && !modelIds.includes(source.model)) {
      write("model", "");
    }
    return next === source ? settings : next;
  }

  function deploySettingsPatch(settings, patch, options = {}) {
    const source = deploySettingsForUi(settings);
    const rawPatch = patch && typeof patch === "object" ? patch : {};
    const next = { ...source };
    const command = String(options.contract?.deploy_settings?.command || "").trim();
    const modelIds = (options.modelCatalog || [])
      .map((model) => String(model?.id || "").trim())
      .filter(Boolean);
    for (const [key, value] of Object.entries(rawPatch)) {
      if (key === "command" && command) {
        if (String(value || "").trim() === command) next.command = command;
        continue;
      }
      if (key === "surface" && !contractValueAllowed(options.contract?.deploy_settings?.surfaces, value)) continue;
      if (key === "trustPolicy" && !contractValueAllowed(options.contract?.deploy_settings?.trust_policies, value)) continue;
      if (key === "realmBackend" && !contractValueAllowed(options.contract?.deploy_settings?.realm_backends, value)) continue;
      if (key === "model" && !catalogValueAllowed(modelIds, value)) continue;
      next[key] = value;
    }
    return deploySettingsForUi(next);
  }

  function deploySettingsFieldPatch(settings, field, value, options = {}) {
    const key = String(field || "").trim();
    if (!key) return deploySettingsForUi(settings);
    return deploySettingsPatch(settings, { [key]: value }, options);
  }

  function mobSettingsPatch(settings, patch, options = {}) {
    const source = normalizeMobSettings(settings);
    const rawPatch = patch && typeof patch === "object" ? patch : {};
    const next = { ...source };
    for (const [key, value] of Object.entries(rawPatch)) {
      if (!MOB_SETTINGS_PATCH_KEYS.has(key)) continue;
      if (key === "backendDefault" && !contractValueAllowed(options.contract?.mob_definition?.profile_backends, value, { allowBlank: true })) continue;
      next[key] = value;
    }
    return normalizeMobSettings(next);
  }

  function mobSettingsFieldPatch(settings, field, value, options = {}) {
    const key = String(field || "").trim();
    if (!key) return normalizeMobSettings(settings);
    return mobSettingsPatch(settings, { [key]: value }, options);
  }

  function reconcileMembersWithContract(members, contract, deploySettings, modelCatalog, toolCatalog, options = {}) {
    const source = Array.isArray(members) ? members : [];
    const runtimeModes = contractStringValues(contract?.mob_definition?.runtime_modes);
    const profileBindings = contractStringValues(contract?.mob_definition?.profile_binding);
    const profileBackends = contractStringValues(contract?.mob_definition?.profile_backends);
    const modelIds = (modelCatalog || [])
      .map((model) => String(model?.id || "").trim())
      .filter(Boolean);
    const toolIds = (toolCatalog || [])
      .map((tool) => String(tool?.id || "").trim())
      .filter(Boolean);
    if (!runtimeModes.length && !profileBindings.length && !profileBackends.length && !modelIds.length && !toolIds.length && !options.strictEmptyModels && !options.strictEmptyTools) {
      return members;
    }
    const surface = String(deploySettings?.surface || "").trim();
    let changed = false;
    const next = source.map((member) => {
      if (!member || typeof member !== "object") return member;
      let out = member;
      const write = (key, value) => {
        if (out[key] === value) return;
        if (out === member) out = { ...member };
        out[key] = value;
      };
      if (profileBindings.length) {
        const binding = String(member.profileBinding || member.profile_binding || "").trim();
        if (binding && !profileBindings.includes(binding)) write("profileBinding", "");
      }
      if (runtimeModes.length) {
        const runtimeMode = String(member.runtimeMode || member.runtime_mode || "").trim();
        const knownRuntimeMode = runtimeMode && runtimeModes.includes(runtimeMode);
        if (runtimeMode && !knownRuntimeMode) write("runtimeMode", "");
        if (knownRuntimeMode && !runtimeModeDeploySurfaceAllowed(contract, surface, runtimeMode)) {
          const replacement = firstDeploySurfaceRuntimeMode(contract, surface);
          if (replacement) write("runtimeMode", replacement);
          else write("runtimeMode", "");
        }
      }
      if (profileBackends.length) {
        const backend = String(member.backend || "").trim();
        if (backend && !profileBackends.includes(backend)) write("backend", "");
      }
      if (modelIds.length || options.strictEmptyModels) {
        const model = String(member.model || "").trim();
        if (model && !modelIds.includes(model)) write("model", "");
      }
      if (toolIds.length || options.strictEmptyTools) {
        const allowedTools = new Set(toolIds);
        const tools = normalizeStringList(member.tools).filter((tool) => allowedTools.has(tool));
        if (JSON.stringify(tools) !== JSON.stringify(normalizeStringList(member.tools))) write("tools", tools);
      }
      if (out !== member) changed = true;
      return out;
    });
    return changed ? next : members;
  }

  function reconcileMobSettingsWithContract(settings, contract) {
    const source = mobSettingsForUi(settings);
    const backends = contractStringValues(contract?.mob_definition?.profile_backends);
    if (!backends.length) return settings;
    const normalizedChanged = JSON.stringify(source) !== JSON.stringify(settings || {});
    const backendDefault = String(source.backendDefault || "").trim();
    if (!backendDefault || backends.includes(backendDefault)) return normalizedChanged ? source : settings;
    return { ...source, backendDefault: "" };
  }

  function reconcileAuthoringWithContract({
    members,
    skillRealms,
    schemas,
    deploySettings,
    mobSettings,
    flow,
    instances,
    edges,
    contract,
    modelCatalog,
    toolCatalog,
    contractLoaded = false,
  } = {}) {
    const strictEmpty = !!contractLoaded;
    let nextMembers = reconcileMemberSkillRefs(
      members,
      skillRealms,
      { strictEmpty },
    );
    nextMembers = reconcileMemberSchemaRefs(
      nextMembers,
      schemas,
      { strictEmpty },
    );
    const nextDeploySettings = reconcileDeploySettingsWithContract(
      deploySettings,
      contract,
      modelCatalog,
      { strictEmptyModels: strictEmpty },
    );
    nextMembers = reconcileMembersWithContract(
      nextMembers,
      contract,
      nextDeploySettings,
      modelCatalog,
      toolCatalog,
      {
        strictEmptyModels: strictEmpty,
        strictEmptyTools: strictEmpty,
      },
    );
    const authoring = reconcileAuthoringForMembers({
      flow,
      instances,
      edges,
      mobSettings,
      previousMembers: members,
      members: nextMembers,
    });
    const nextMobSettings = reconcileMobSettingsWithContract(authoring.mobSettings, contract);
    return {
      members: nextMembers,
      deploySettings: nextDeploySettings,
      flow: authoring.flow,
      instances: authoring.instances,
      edges: authoring.edges,
      mobSettings: nextMobSettings,
      changed: nextMembers !== members
        || nextDeploySettings !== deploySettings
        || authoring.flow !== flow
        || authoring.instances !== instances
        || authoring.edges !== edges
        || nextMobSettings !== mobSettings,
    };
  }

  function reconcileStringField(source, write, key, values) {
    const allowed = contractStringValues(values);
    if (!allowed.length) return;
    const value = String(source[key] || "").trim();
    if (value && !allowed.includes(value)) write(key, "");
  }

  function contractValueAllowed(values, raw, { allowBlank = false } = {}) {
    const value = String(raw || "").trim();
    if (!value) return allowBlank;
    const allowed = contractStringValues(values);
    return allowed.length ? allowed.includes(value) : true;
  }

  function catalogValueAllowed(values, raw, { allowBlank = true } = {}) {
    const value = String(raw || "").trim();
    if (!value) return allowBlank;
    const allowed = Array.isArray(values)
      ? values.map((candidate) => String(candidate || "").trim()).filter(Boolean)
      : [];
    return allowed.length ? allowed.includes(value) : true;
  }

  function optionValueAllowed(options, raw, { allowBlank = false } = {}) {
    const value = String(raw || "").trim();
    if (!value) return allowBlank;
    const enabled = (Array.isArray(options) ? options : [])
      .filter((option) => option && option.disabled !== true)
      .map((option) => String(option.value || "").trim())
      .filter(Boolean);
    return enabled.length ? enabled.includes(value) : true;
  }

  function skillIdSet(skillRealms) {
    const ids = new Set();
    for (const realm of skillRealms || []) {
      for (const skill of realm?.skills || []) {
        const id = String(skill?.id || "").trim();
        if (id) ids.add(id);
      }
    }
    return ids;
  }

  function skillRealmsForDocument(members, skillRealms) {
    const selected = new Set();
    for (const member of members || []) {
      for (const skill of member?.skills || []) {
        const id = String(skill || "").trim();
        if (id) selected.add(id);
      }
    }
    if (!selected.size) return [];

    const out = [];
    const seen = new Set();
    for (const realm of skillRealms || []) {
      if (!realm || typeof realm !== "object") continue;
      const skills = [];
      for (const skill of realm.skills || []) {
        const id = String(skill?.id || "").trim();
        if (!id || !selected.has(id) || seen.has(id)) continue;
        seen.add(id);
        skills.push(skill);
      }
      if (!skills.length) continue;
      out.push({
        ...realm,
        skills,
        default: out.length === 0 ? !!realm.default : false,
      });
    }
    return out;
  }

  function memberToolIndex(members) {
    return new Map((members || [])
      .filter((member) => member?.id)
      .map((member) => [member.id, new Set(normalizeStringList(member.tools))]));
  }

  function reconcileToolScopesInSteps(steps, allowedForStep) {
    let changed = false;
    const next = (steps || []).map((step) => {
      const reconciled = reconcileToolScopesInStep(step, allowedForStep);
      if (reconciled !== step) changed = true;
      return reconciled;
    });
    return changed ? next : steps;
  }

  function reconcileToolScopesInStep(step, allowedForStep) {
    if (!step || typeof step !== "object") return step;
    let next = step;
    if (step.type === "member") {
      next = reconcileToolScopeObject(step, allowedForStep(step));
    }
    if (step.type === "repeat") {
      const nested = reconcileToolScopesInSteps(step.steps || [], allowedForStep);
      if (nested !== step.steps) next = { ...next, steps: nested };
    }
    if (step.type === "branch" || step.type === "parallel") {
      let changed = false;
      const branches = (step.branches || []).map((branch) => {
        const branchSteps = reconcileToolScopesInSteps(branch.steps || [], allowedForStep);
        if (branchSteps === branch.steps) return branch;
        changed = true;
        return { ...branch, steps: branchSteps };
      });
      const fallback = Array.isArray(step.fallback)
        ? reconcileToolScopesInSteps(step.fallback, allowedForStep)
        : step.fallback;
      if (fallback !== step.fallback) changed = true;
      if (changed) next = { ...next, branches, fallback };
    }
    return next;
  }

  function reconcileToolScopeObject(source, allowedTools) {
    if (!source || typeof source !== "object") return source;
    const allowed = allowedTools || new Set();
    const allowedToolsList = normalizeStringList(source.allowedTools || source.allowed_tools)
      .filter((tool) => allowed.has(tool));
    const blockedToolsList = normalizeStringList(source.blockedTools || source.blocked_tools)
      .filter((tool) => allowed.has(tool));
    const currentAllowed = normalizeStringList(source.allowedTools || source.allowed_tools);
    const currentBlocked = normalizeStringList(source.blockedTools || source.blocked_tools);
    if (
      JSON.stringify(allowedToolsList) === JSON.stringify(currentAllowed)
      && JSON.stringify(blockedToolsList) === JSON.stringify(currentBlocked)
      && !("allowed_tools" in source)
      && !("blocked_tools" in source)
    ) {
      return source;
    }
    const { allowed_tools: _allowedToolsSnake, blocked_tools: _blockedToolsSnake, ...rest } = source;
    return {
      ...rest,
      allowedTools: allowedToolsList,
      blockedTools: blockedToolsList,
    };
  }

  function memberIdSet(members) {
    return new Set((members || []).map((member) => String(member?.id || "").trim()).filter(Boolean));
  }

  function flowLaunchSourceSet(flow, members) {
    const ids = memberIdSet(members);
    collectVisualSteps(flow?.steps || [], (step) => {
      if (step?.type === "member" && step.id) ids.add(String(step.id));
    });
    return ids;
  }

  function graphLaunchSourceSet(instances, members) {
    const ids = memberIdSet(members);
    for (const instance of instances || []) {
      if (instance?.id && instance.memberId && !instance.isGate && !instance.isTerminal) {
        ids.add(String(instance.id));
      }
    }
    return ids;
  }

  function reconcileLaunchSourcesInSteps(steps, allowedSources) {
    let changed = false;
    const next = (steps || []).map((step) => {
      const reconciled = reconcileLaunchSourcesInStep(step, allowedSources);
      if (reconciled !== step) changed = true;
      return reconciled;
    });
    return changed ? next : steps;
  }

  function reconcileLaunchSourcesInStep(step, allowedSources) {
    if (!step || typeof step !== "object") return step;
    let next = reconcileLaunchSourceObject(step, allowedSources);
    if (step.type === "repeat") {
      const nested = reconcileLaunchSourcesInSteps(step.steps || [], allowedSources);
      if (nested !== step.steps) next = { ...next, steps: nested };
    }
    if (step.type === "branch" || step.type === "parallel") {
      let changed = false;
      const branches = (step.branches || []).map((branch) => {
        const branchSteps = reconcileLaunchSourcesInSteps(branch.steps || [], allowedSources);
        if (branchSteps === branch.steps) return branch;
        changed = true;
        return { ...branch, steps: branchSteps };
      });
      const fallback = Array.isArray(step.fallback)
        ? reconcileLaunchSourcesInSteps(step.fallback, allowedSources)
        : step.fallback;
      if (fallback !== step.fallback) changed = true;
      if (changed) next = { ...next, branches, fallback };
    }
    return next;
  }

  function reconcileLaunchSourceObject(source, allowedSources) {
    if (!source || typeof source !== "object") return source;
    const mode = launchModeFromAuthoringSource(source);
    if (!mode) return source;
    if (mode.kind !== "Fork" || !mode.from || allowedSources.has(mode.from)) return source;
    return {
      ...source,
      launchMode: freshLaunchModePreservingBudget(mode),
    };
  }

  function reconcileControlRolesInSteps(steps, memberIds) {
    let changed = false;
    const next = (steps || []).map((step) => {
      const reconciled = reconcileControlRolesInStep(step, memberIds);
      if (reconciled !== step) changed = true;
      return reconciled;
    });
    return changed ? next : steps;
  }

  function reconcileControlRolesInStep(step, memberIds) {
    if (!step || typeof step !== "object") return step;
    let next = reconcileControlRoleObject(step, memberIds);
    if (step.type === "repeat") {
      const nested = reconcileControlRolesInSteps(step.steps || [], memberIds);
      if (nested !== step.steps) next = { ...next, steps: nested };
    }
    if (step.type === "branch" || step.type === "parallel") {
      let changed = false;
      const branches = (step.branches || []).map((branch) => {
        const branchSteps = reconcileControlRolesInSteps(branch.steps || [], memberIds);
        if (branchSteps === branch.steps) return branch;
        changed = true;
        return { ...branch, steps: branchSteps };
      });
      const fallback = Array.isArray(step.fallback)
        ? reconcileControlRolesInSteps(step.fallback, memberIds)
        : step.fallback;
      if (fallback !== step.fallback) changed = true;
      if (changed) next = { ...next, branches, fallback };
    }
    return next;
  }

  function reconcileControlRoleObject(source, memberIds) {
    if (!source || typeof source !== "object") return source;
    const controllerRole = String(source.controllerRole || source.controllerMemberId || source.controlRole || source.joinRole || "").trim();
    if (!controllerRole || memberIds.has(controllerRole)) return source;
    const { controllerRole: _controllerRole, controllerMemberId: _controllerMemberId, controlRole: _controlRole, joinRole: _joinRole, ...rest } = source;
    return rest;
  }

  function reconcileMobSettingsProfiles(settings, previousMembers, members) {
    const normalized = normalizeMobSettings(settings);
    const previousById = new Map((previousMembers || [])
      .filter((member) => member?.id)
      .map((member) => [member.id, profileName(member)]));
    const currentProfiles = new Set((members || []).map(profileName).filter(Boolean));
    const renameByProfile = new Map();
    for (const member of members || []) {
      if (!member?.id) continue;
      const previous = previousById.get(member.id);
      const current = profileName(member);
      if (previous && current && previous !== current) renameByProfile.set(previous, current);
    }
    const rewriteProfile = (value) => {
      const raw = String(value || "").trim();
      if (!raw) return "";
      const renamed = renameByProfile.get(raw) || raw;
      return currentProfiles.has(renamed) ? renamed : "";
    };
    const orchestrator = rewriteProfile(normalized.orchestrator);
    const roleWiring = [];
    const seen = new Set();
    for (const rule of normalized.roleWiring || []) {
      const a = rewriteProfile(rule.a);
      const b = rewriteProfile(rule.b);
      if (!a || !b) continue;
      const key = `${a}\u0000${b}`;
      if (seen.has(key)) continue;
      seen.add(key);
      roleWiring.push({ a, b });
    }
    if (
      orchestrator === normalized.orchestrator
      && JSON.stringify(roleWiring) === JSON.stringify(normalized.roleWiring || [])
    ) {
      return settings;
    }
    return {
      ...normalized,
      orchestrator,
      roleWiring,
    };
  }

  function reconcileSchemaFieldReferences({ flow, edges, members, instances, schemaId, oldName, newName }) {
    const schema = String(schemaId || "").trim();
    const oldField = String(oldName || "").trim();
    const nextField = String(newName || "").trim();
    if (!schema || !oldField || oldField === nextField) {
      return { flow, edges };
    }
    const memberSchemaById = new Map((members || []).map((member) => [
      member.id,
      String(member.schema || "").trim(),
    ]));
    const flowStepSchemas = flowStepSchemaIndex(flow?.steps || [], memberSchemaById);
    const graphStepSchemas = new Map((instances || [])
      .filter((instance) => instance?.id && instance?.memberId)
      .map((instance) => [instance.id, memberSchemaById.get(instance.memberId) || ""]));
    const reconciledFlow = reconcileSchemaFieldReferencesInFlow(flow, flowStepSchemas, schema, oldField, nextField);
    const reconciledEdges = reconcileSchemaFieldReferencesInEdges(edges, graphStepSchemas, schema, oldField, nextField);
    return {
      flow: reconciledFlow,
      edges: reconciledEdges,
    };
  }

  function reconcileInputParamReferences({ flow, edges, oldName, newName }) {
    const oldField = String(oldName || "").trim();
    const nextField = String(newName || "").trim();
    if (!oldField || oldField === nextField) {
      return { flow, edges };
    }
    return {
      flow: reconcileInputParamReferencesInFlow(flow, oldField, nextField),
      edges: reconcileInputParamReferencesInEdges(edges, oldField, nextField),
    };
  }

  function reconcileInputParamReferencesInFlow(flow, oldName, newName) {
    if (!flow || typeof flow !== "object") return flow;
    const steps = reconcileInputParamReferencesInSteps(flow.steps || [], oldName, newName);
    return steps === flow.steps ? flow : { ...flow, steps };
  }

  function reconcileInputParamReferencesInSteps(steps, oldName, newName) {
    let changed = false;
    const next = (steps || []).map((step) => {
      const reconciled = reconcileInputParamReferencesInStep(step, oldName, newName);
      if (reconciled !== step) changed = true;
      return reconciled;
    });
    return changed ? next : steps;
  }

  function reconcileInputParamReferencesInStep(step, oldName, newName) {
    if (!step || typeof step !== "object") return step;
    if (step.type === "repeat") {
      const cond = rewriteInputParamCondition(step.cond, oldName, newName);
      const until = rewriteInputParamConditionText(step.until, oldName, newName, cond);
      const nested = reconcileInputParamReferencesInSteps(step.steps || [], oldName, newName);
      if (cond === step.cond && until === step.until && nested === step.steps) return step;
      return { ...step, cond, until, steps: nested };
    }
    if (step.type === "branch") {
      let changed = false;
      const branches = (step.branches || []).map((branch) => {
        const cond = rewriteInputParamCondition(branch.cond, oldName, newName);
        const condition = rewriteInputParamConditionText(branch.condition, oldName, newName, cond);
        const branchSteps = reconcileInputParamReferencesInSteps(branch.steps || [], oldName, newName);
        if (cond === branch.cond && condition === branch.condition && branchSteps === branch.steps) return branch;
        changed = true;
        return { ...branch, cond, condition, steps: branchSteps };
      });
      const fallback = Array.isArray(step.fallback)
        ? reconcileInputParamReferencesInSteps(step.fallback, oldName, newName)
        : step.fallback;
      if (fallback !== step.fallback) changed = true;
      return changed ? { ...step, branches, fallback } : step;
    }
    if (step.type === "parallel") {
      let changed = false;
      const branches = (step.branches || []).map((branch) => {
        const branchSteps = reconcileInputParamReferencesInSteps(branch.steps || [], oldName, newName);
        if (branchSteps === branch.steps) return branch;
        changed = true;
        return { ...branch, steps: branchSteps };
      });
      return changed ? { ...step, branches } : step;
    }
    return step;
  }

  function rewriteInputParamCondition(cond, oldName, newName) {
    if (!cond || typeof cond !== "object") return cond;
    const isParam = cond.namespace === "params" || cond.stepId === "params" || cond.step_id === "params";
    if (!isParam || String(cond.field || "").trim() !== oldName) return cond;
    if (!newName) return {};
    return {
      ...cond,
      namespace: "params",
      stepId: "params",
      field: newName,
    };
  }

  function rewriteInputParamConditionText(text, oldName, newName, preferredCond) {
    if (!text) return text;
    if (preferredCond && preferredCond.field) return editorConditionText(preferredCond);
    const parsed = parseEditorConditionText(text);
    if (!parsed || parsed.namespace !== "params" || parsed.field !== oldName) return text;
    if (!newName) return "";
    return editorConditionText({ ...parsed, field: newName });
  }

  function reconcileInputParamReferencesInEdges(edges, oldName, newName) {
    let changed = false;
    const next = (edges || []).map((edge) => {
      const condition = normalizedEdgeCondition(edge);
      const path = String(condition?.path || "").trim();
      const parts = path.split(".").filter(Boolean);
      if (parts.length !== 2 || parts[0] !== "params" || parts[1] !== oldName) return edge;
      changed = true;
      if (!newName) {
        return { ...edge, cond: null, label: "" };
      }
      const nextPath = `params.${newName}`;
      const nextCond = { var: nextPath, op: condition.op || "", val: condition.val ?? "" };
      return {
        ...edge,
        cond: nextCond,
        label: edge.label && edge.label === conditionTextForPath(path, condition)
          ? conditionTextForPath(nextPath, nextCond)
          : edge.label,
      };
    });
    return changed ? next : edges;
  }

  function reconcileConditionFieldAvailability({ flow, edges, members, instances, schemas }) {
    const schemaFields = schemaFieldNameIndex(schemas);
    const inputFields = inputParamNameSet(flow);
    const memberSchemaById = new Map((members || []).map((member) => [
      member.id,
      String(member.schema || "").trim(),
    ]));
    const flowStepSchemas = flowStepSchemaIndex(flow?.steps || [], memberSchemaById);
    const graphStepSchemas = new Map((instances || [])
      .filter((instance) => instance?.id && instance?.memberId)
      .map((instance) => [instance.id, memberSchemaById.get(instance.memberId) || ""]));
    return {
      flow: reconcileConditionAvailabilityInFlow(flow, flowStepSchemas, schemaFields, inputFields),
      edges: reconcileConditionAvailabilityInEdges(edges, graphStepSchemas, schemaFields, inputFields),
    };
  }

  function schemaFieldNameIndex(schemas) {
    const out = new Map();
    for (const schema of schemas || []) {
      const id = String(schema?.id || "").trim();
      if (!id) continue;
      const fields = new Map();
      for (const field of schema.fields || []) {
        const name = String(field?.name || "").trim();
        if (name) fields.set(name, field);
      }
      out.set(id, fields);
    }
    return out;
  }

  function inputParamNameSet(flow) {
    const out = new Map();
    collectVisualSteps(flow?.steps || [], (step) => {
      if (step?.type !== "input") return;
      for (const param of step.inputParams || []) {
        const name = String(param?.name || "").trim();
        if (name) out.set(name, param);
      }
    });
    return out;
  }

  function reconcileConditionAvailabilityInFlow(flow, stepSchemas, schemaFields, inputFields) {
    if (!flow || typeof flow !== "object") return flow;
    const steps = reconcileConditionAvailabilityInSteps(flow.steps || [], stepSchemas, schemaFields, inputFields);
    return steps === flow.steps ? flow : { ...flow, steps };
  }

  function reconcileConditionAvailabilityInSteps(steps, stepSchemas, schemaFields, inputFields) {
    let changed = false;
    const next = (steps || []).map((step) => {
      const reconciled = reconcileConditionAvailabilityInStep(step, stepSchemas, schemaFields, inputFields);
      if (reconciled !== step) changed = true;
      return reconciled;
    });
    return changed ? next : steps;
  }

  function reconcileConditionAvailabilityInStep(step, stepSchemas, schemaFields, inputFields) {
    if (!step || typeof step !== "object") return step;
    if (step.type === "repeat") {
      const hadCond = !!step.cond;
      const cond = clearUnavailableEditorCondition(step.cond, stepSchemas, schemaFields, inputFields);
      const until = clearUnavailableConditionText(step.until, stepSchemas, schemaFields, inputFields, hadCond ? cond : undefined);
      const nested = reconcileConditionAvailabilityInSteps(step.steps || [], stepSchemas, schemaFields, inputFields);
      if (cond === step.cond && until === step.until && nested === step.steps) return step;
      return { ...step, cond, until, steps: nested };
    }
    if (step.type === "branch") {
      let changed = false;
      const branches = (step.branches || []).map((branch) => {
        const hadCond = !!branch.cond;
        const cond = clearUnavailableEditorCondition(branch.cond, stepSchemas, schemaFields, inputFields);
        const condition = clearUnavailableConditionText(branch.condition, stepSchemas, schemaFields, inputFields, hadCond ? cond : undefined);
        const branchSteps = reconcileConditionAvailabilityInSteps(branch.steps || [], stepSchemas, schemaFields, inputFields);
        if (cond === branch.cond && condition === branch.condition && branchSteps === branch.steps) return branch;
        changed = true;
        return { ...branch, cond, condition, steps: branchSteps };
      });
      const fallback = Array.isArray(step.fallback)
        ? reconcileConditionAvailabilityInSteps(step.fallback, stepSchemas, schemaFields, inputFields)
        : step.fallback;
      if (fallback !== step.fallback) changed = true;
      return changed ? { ...step, branches, fallback } : step;
    }
    if (step.type === "parallel") {
      let changed = false;
      const branches = (step.branches || []).map((branch) => {
        const branchSteps = reconcileConditionAvailabilityInSteps(branch.steps || [], stepSchemas, schemaFields, inputFields);
        if (branchSteps === branch.steps) return branch;
        changed = true;
        return { ...branch, steps: branchSteps };
      });
      return changed ? { ...step, branches } : step;
    }
    return step;
  }

  function clearUnavailableEditorCondition(cond, stepSchemas, schemaFields, inputFields) {
    if (!cond || typeof cond !== "object") return cond;
    return editorConditionFieldAvailable(cond, stepSchemas, schemaFields, inputFields) ? cond : {};
  }

  function clearUnavailableConditionText(text, stepSchemas, schemaFields, inputFields, preferredCond) {
    if (preferredCond !== undefined) {
      if (preferredCond && preferredCond.field) return editorConditionText(preferredCond);
      return "";
    }
    if (!text) return text;
    const parsed = parseEditorConditionText(text);
    if (!parsed) return text;
    return editorConditionFieldAvailable(parsed, stepSchemas, schemaFields, inputFields) ? text : "";
  }

  function editorConditionFieldAvailable(cond, stepSchemas, schemaFields, inputFields) {
    if (!cond || typeof cond !== "object") return true;
    const field = String(cond.field || "").trim();
    if (!field) return true;
    if (cond.namespace === "params" || cond.stepId === "params" || cond.step_id === "params") {
      return conditionFieldValueAvailable(inputFields.get(field), cond);
    }
    const stepId = String(cond.stepId || cond.step_id || "").trim();
    if (!stepId) return true;
    return conditionFieldValueAvailable(schemaFieldForCondition(schemaFields, stepSchemas.get(stepId), field), cond);
  }

  function reconcileConditionAvailabilityInEdges(edges, stepSchemas, schemaFields, inputFields) {
    let changed = false;
    const next = (edges || []).map((edge) => {
      const condition = normalizedEdgeCondition(edge);
      const path = String(condition?.path || "").trim();
      const parts = path.split(".").filter(Boolean);
      let available = true;
      if (parts.length === 2 && parts[0] === "params") {
        available = conditionFieldValueAvailable(inputFields.get(parts[1]), condition);
      } else if (parts.length === 3 && parts[0] === "steps") {
        available = conditionFieldValueAvailable(schemaFieldForCondition(schemaFields, stepSchemas.get(parts[1]), parts[2]), condition);
      }
      if (available) return edge;
      changed = true;
      return { ...edge, cond: null, label: "" };
    });
    return changed ? next : edges;
  }

  function schemaHasField(schemaFields, schemaId, field) {
    return !!schemaFieldForCondition(schemaFields, schemaId, field);
  }

  function schemaFieldForCondition(schemaFields, schemaId, field) {
    const id = String(schemaId || "").trim();
    const name = String(field || "").trim();
    if (!id || !name) return null;
    const fields = schemaFields.get(id);
    if (!fields) return null;
    if (fields instanceof Map) return fields.get(name) || null;
    return fields.has?.(name) ? { name } : null;
  }

  function conditionFieldValueAvailable(field, cond) {
    if (!field) return false;
    const type = String(field.type || "").trim();
    if (type !== "enum") return true;
    const values = enumValuesForField(field).map(String);
    if (!values.length) return true;
    const raw = cond?.val ?? cond?.value;
    if (raw == null || String(raw).trim() === "") return true;
    return values.includes(String(raw));
  }

  function flowStepSchemaIndex(steps, memberSchemaById, out = new Map()) {
    for (const step of steps || []) {
      if (!step || typeof step !== "object") continue;
      if (step.type === "member" && step.id) {
        out.set(step.id, memberSchemaById.get(step.role) || String(step.schema || "").trim());
      }
      if (step.type === "repeat") flowStepSchemaIndex(step.steps || [], memberSchemaById, out);
      if (step.type === "branch" || step.type === "parallel") {
        for (const branch of step.branches || []) flowStepSchemaIndex(branch.steps || [], memberSchemaById, out);
        flowStepSchemaIndex(step.fallback || [], memberSchemaById, out);
      }
    }
    return out;
  }

  function reconcileSchemaFieldReferencesInFlow(flow, stepSchemas, schemaId, oldName, newName) {
    if (!flow || typeof flow !== "object") return flow;
    const steps = reconcileSchemaFieldReferencesInSteps(flow.steps || [], stepSchemas, schemaId, oldName, newName);
    return steps === flow.steps ? flow : { ...flow, steps };
  }

  function reconcileSchemaFieldReferencesInSteps(steps, stepSchemas, schemaId, oldName, newName) {
    let changed = false;
    const next = (steps || []).map((step) => {
      const reconciled = reconcileSchemaFieldReferencesInStep(step, stepSchemas, schemaId, oldName, newName);
      if (reconciled !== step) changed = true;
      return reconciled;
    });
    return changed ? next : steps;
  }

  function reconcileSchemaFieldReferencesInStep(step, stepSchemas, schemaId, oldName, newName) {
    if (!step || typeof step !== "object") return step;
    if (step.type === "repeat") {
      const cond = rewriteEditorCondition(step.cond, stepSchemas, schemaId, oldName, newName);
      const until = rewriteConditionTextReference(step.until, stepSchemas, schemaId, oldName, newName);
      const nested = reconcileSchemaFieldReferencesInSteps(step.steps || [], stepSchemas, schemaId, oldName, newName);
      if (cond === step.cond && until === step.until && nested === step.steps) return step;
      return {
        ...step,
        cond,
        until,
        steps: nested,
      };
    }
    if (step.type === "branch") {
      let changed = false;
      const branches = (step.branches || []).map((branch) => {
        const cond = rewriteEditorCondition(branch.cond, stepSchemas, schemaId, oldName, newName);
        const condition = rewriteConditionTextReference(branch.condition, stepSchemas, schemaId, oldName, newName, cond);
        const branchSteps = reconcileSchemaFieldReferencesInSteps(branch.steps || [], stepSchemas, schemaId, oldName, newName);
        if (cond === branch.cond && condition === branch.condition && branchSteps === branch.steps) return branch;
        changed = true;
        return { ...branch, cond, condition, steps: branchSteps };
      });
      const fallback = Array.isArray(step.fallback)
        ? reconcileSchemaFieldReferencesInSteps(step.fallback, stepSchemas, schemaId, oldName, newName)
        : step.fallback;
      if (fallback !== step.fallback) changed = true;
      return changed ? { ...step, branches, fallback } : step;
    }
    if (step.type === "parallel") {
      let changed = false;
      const branches = (step.branches || []).map((branch) => {
        const branchSteps = reconcileSchemaFieldReferencesInSteps(branch.steps || [], stepSchemas, schemaId, oldName, newName);
        if (branchSteps === branch.steps) return branch;
        changed = true;
        return { ...branch, steps: branchSteps };
      });
      return changed ? { ...step, branches } : step;
    }
    return step;
  }

  function rewriteEditorCondition(cond, stepSchemas, schemaId, oldName, newName) {
    if (!cond || typeof cond !== "object") return cond;
    if (cond.namespace === "params" || cond.stepId === "params") return cond;
    const stepId = String(cond.stepId || cond.step_id || "").trim();
    if (!stepId || stepSchemas.get(stepId) !== schemaId || String(cond.field || "").trim() !== oldName) return cond;
    if (!newName) return {};
    const next = { ...cond, field: newName || "", val: newName ? (cond.val ?? "") : "" };
    return next;
  }

  function rewriteConditionTextReference(text, stepSchemas, schemaId, oldName, newName, preferredCond) {
    if (!text) return text;
    if (preferredCond && preferredCond.field) return editorConditionText(preferredCond);
    const parsed = parseEditorConditionText(text);
    if (!parsed || parsed.namespace === "params") return text;
    if (stepSchemas.get(parsed.stepId) !== schemaId || parsed.field !== oldName) return text;
    if (!newName) return "";
    return editorConditionText({ ...parsed, field: newName });
  }

  function parseEditorConditionText(text) {
    const raw = String(text || "").trim();
    const params = /^params\.([A-Za-z0-9_.-]+)\s*(==|>|<)\s*(.+)$/.exec(raw);
    if (params) {
      return {
        namespace: "params",
        stepId: "params",
        field: params[1],
        op: params[2],
        val: params[3].trim().replace(/^["']|["']$/g, ""),
      };
    }
    const steps = /^steps\.([A-Za-z0-9_.-]+)\.([A-Za-z0-9_.-]+)\s*(==|>|<)\s*(.+)$/.exec(raw);
    if (!steps) return null;
    return {
      namespace: "steps",
      stepId: steps[1],
      field: steps[2],
      op: steps[3],
      val: steps[4].trim().replace(/^["']|["']$/g, ""),
    };
  }

  function editorConditionText(cond) {
    if (!cond || !cond.stepId || !cond.field) return "";
    const op = cond.op || cond.operator || "";
    if (!op) return "";
    const val = cond.val ?? cond.value ?? "";
    if (cond.namespace === "params" || cond.stepId === "params") {
      return `params.${cond.field} ${op} ${JSON.stringify(String(val))}`;
    }
    return `steps.${cond.stepId}.${cond.field} ${op} ${JSON.stringify(String(val))}`;
  }

  function conditionValueLiteral(value) {
    return /^(true|false|-?\d+(\.\d+)?)$/.test(String(value ?? ""))
      ? String(value ?? "")
      : JSON.stringify(String(value ?? ""));
  }

  function conditionViewFromSchema(schema) {
    const view = schema?.mob_definition?.editor_condition_view;
    if (!view || typeof view !== "object") return null;
    const out = {
      emptyValueLabel: String(view.empty_value_label || "").trim(),
      textValuePlaceholder: String(view.text_value_placeholder || "").trim(),
    };
    return Object.values(out).every(Boolean) ? out : null;
  }

  function conditionViewForState(conditionView) {
    const view = conditionView && typeof conditionView === "object" ? conditionView : null;
    return {
      emptyValueLabel: String(view?.emptyValueLabel || ""),
      textValuePlaceholder: String(view?.textValuePlaceholder || ""),
    };
  }

  function errorViewFromSchema(schema) {
    const view = schema?.mob_definition?.editor_error_view;
    if (!view || typeof view !== "object") return null;
    const out = {
      criticalGlyph: String(view.critical_glyph || "").trim(),
      genericErrorHead: String(view.generic_error_head || "").trim(),
      deployFailedHead: String(view.deploy_failed_head || "").trim(),
      deployPlanFailedHead: String(view.deploy_plan_failed_head || "").trim(),
      deployErrorMeta: String(view.deploy_error_meta || "").trim(),
      sourceFailedHead: String(view.source_failed_head || "").trim(),
      sourceErrorMeta: String(view.source_error_meta || "").trim(),
      validationApiFailedHead: String(view.validation_api_failed_head || "").trim(),
      rpcErrorMeta: String(view.rpc_error_meta || "").trim(),
      exportFailedHead: String(view.export_failed_head || "").trim(),
      importFailedHead: String(view.import_failed_head || "").trim(),
      missingEditorFlowHead: String(view.missing_editor_flow_head || "").trim(),
      missingEditorFlowSub: String(view.missing_editor_flow_sub || "").trim(),
      missingEditorFlowMeta: String(view.missing_editor_flow_meta || "").trim(),
    };
    return Object.values(out).every(Boolean) ? out : null;
  }

  function errorViewForState(errorView) {
    const view = errorView && typeof errorView === "object" ? errorView : null;
    return {
      criticalGlyph: String(view?.criticalGlyph || ""),
      genericErrorHead: String(view?.genericErrorHead || ""),
      deployFailedHead: String(view?.deployFailedHead || ""),
      deployPlanFailedHead: String(view?.deployPlanFailedHead || ""),
      deployErrorMeta: String(view?.deployErrorMeta || ""),
      sourceFailedHead: String(view?.sourceFailedHead || ""),
      sourceErrorMeta: String(view?.sourceErrorMeta || ""),
      validationApiFailedHead: String(view?.validationApiFailedHead || ""),
      rpcErrorMeta: String(view?.rpcErrorMeta || ""),
      exportFailedHead: String(view?.exportFailedHead || ""),
      importFailedHead: String(view?.importFailedHead || ""),
      missingEditorFlowHead: String(view?.missingEditorFlowHead || ""),
      missingEditorFlowSub: String(view?.missingEditorFlowSub || ""),
      missingEditorFlowMeta: String(view?.missingEditorFlowMeta || ""),
    };
  }

  function conditionValueControl(field, rawValue = "", conditionView = null) {
    const view = conditionViewForState(conditionView);
    const type = String(field?.type || "").trim();
    const value = rawValue == null ? "" : String(rawValue);
    const optionRows = (values) => [
      { value: "", label: view.emptyValueLabel },
      ...values.map((candidate) => ({ value: candidate, label: candidate })),
    ];
    if (type === "enum" && Array.isArray(field?.enumValues) && field.enumValues.length) {
      const values = field.enumValues.map(String);
      return { kind: "enum", values, value, optionRows: optionRows(values), placeholder: "" };
    }
    if (type === "boolean" || type === "bool") {
      const values = ["true", "false"];
      return { kind: "boolean", values, value, optionRows: optionRows(values), placeholder: "" };
    }
    return { kind: "text", values: [], value, optionRows: [], placeholder: view.textValuePlaceholder };
  }

  function inputParamName(raw, fallback = "field") {
    return String(raw || fallback)
      .trim()
      .replace(/[^A-Za-z0-9_]+/g, "_")
      .replace(/^_+|_+$/g, "")
      .replace(/^[0-9]/, "_$&") || fallback;
  }

  function uniqueInputParamName(params, raw, currentId = null, fallback = "param") {
    const base = inputParamName(raw, fallback);
    const taken = new Set((params || [])
      .filter((param) => param?.id !== currentId)
      .map((param) => String(param?.name || "").trim())
      .filter(Boolean));
    if (!taken.has(base)) return base;
    let i = 2;
    while (taken.has(`${base}_${i}`)) i += 1;
    return `${base}_${i}`;
  }

  function schemaFieldName(raw, fallback = "field") {
    return inputParamName(raw, fallback);
  }

  function uniqueSchemaFieldName(fields, raw, currentId = null, fallback = "field") {
    const base = schemaFieldName(raw, fallback);
    const taken = new Set((fields || [])
      .filter((field) => field?.id !== currentId)
      .map((field) => String(field?.name || "").trim())
      .filter(Boolean));
    if (!taken.has(base)) return base;
    let i = 2;
    while (taken.has(`${base}_${i}`)) i += 1;
    return `${base}_${i}`;
  }

  function schemaDefinitionAddPatch(existingSchemas, contract) {
    const schemas = Array.isArray(existingSchemas) ? existingSchemas : [];
    const draft = editorSchemaDraftContract(contract);
    if (!draft) {
      return {
        ok: false,
        error: "MobKit schema is missing mob_definition.editor_schema_draft",
        schemas,
      };
    }
    let n = 1;
    while (schemas.some((schema) => schema?.id === `${draft.schemaIdPrefix}${n}`)) n += 1;
    const schema = {
      id: `${draft.schemaIdPrefix}${n}`,
      description: "",
      fields: [{
        id: "f1",
        name: uniqueSchemaFieldName([], draft.initialField.name),
        type: draft.schemaFieldType,
        required: draft.initialField.required,
        description: draft.initialField.description,
        enumValues: draft.initialField.enumValues,
      }],
    };
    return { schema, schemas: [...schemas, schema] };
  }

  function schemaDefinitionAddTransition(existingSchemas, contract) {
    const result = schemaDefinitionAddPatch(existingSchemas, contract);
    if (result.ok === false) return result;
    return {
      ...result,
      ok: true,
      selection: { kind: "schema", id: result.schema.id },
    };
  }

  function schemaDescriptionPatch(rawDescription) {
    return { description: String(rawDescription || "") };
  }

  function enumValuesForField(field) {
    return Array.isArray(field?.enumValues) ? field.enumValues : [];
  }

  function uniqueEnumValue(values, raw, index = null) {
    const base = String(raw || "value").trim() || "value";
    const taken = new Set((Array.isArray(values) ? values : [])
      .filter((_, i) => i !== index)
      .map((value) => String(value || "").trim())
      .filter(Boolean));
    if (!taken.has(base)) return base;
    let i = 2;
    while (taken.has(`${base}_${i}`)) i += 1;
    return `${base}_${i}`;
  }

  function schemaFieldTypeAllowedSet(contract) {
    return new Set(contractStringValues(contract?.mob_definition?.editor_schema_field_types));
  }

  function schemaLikeFieldTypePatch(field, rawType, contract) {
    const type = String(rawType || "").trim();
    const allowedTypes = schemaFieldTypeAllowedSet(contract);
    if (!type || !allowedTypes.has(type)) {
      return {};
    }
    const values = enumValuesForField(field);
    return {
      type,
      enumValues: type === "enum" ? (values.length ? values : ["value"]) : [],
    };
  }

  function schemaLikeFieldRequiredPatch(rawValue) {
    return { required: !!rawValue };
  }

  function schemaLikeFieldDescriptionPatch(rawValue) {
    return { description: String(rawValue ?? "") };
  }

  function normalizeSchemaLikeFieldPatch(current, patch = {}, contract) {
    const source = current && typeof current === "object" ? current : {};
    const rawPatch = patch && typeof patch === "object" ? patch : {};
    let nextPatch = { ...rawPatch };
    if ("type" in nextPatch) {
      const typePatch = schemaLikeFieldTypePatch(source, nextPatch.type, contract);
      delete nextPatch.type;
      delete nextPatch.enumValues;
      nextPatch = { ...nextPatch, ...typePatch };
    }
    if ("enumValues" in nextPatch) {
      const type = String(nextPatch.type || source.type || "").trim();
      nextPatch.enumValues = type === "enum"
        ? enumValuesForField(nextPatch).map((value) => String(value || "").trim()).filter(Boolean)
        : [];
    }
    return nextPatch;
  }

  function schemaLikeFieldTypeControlState(field, contract) {
    const defaultType = contractDefaultValue(contract, "schema_field_type");
    const type = String(field?.type || defaultType || "").trim();
    const typeOptions = schemaFieldTypeOptions(contract, type);
    return {
      type,
      typeOptions,
      selectedType: typeOptions.find((option) => option.value === type) || null,
    };
  }

  function schemaFieldRowControlState(field, contract, schemaView = null, overrides = {}) {
    const view = schemaViewForState(schemaView);
    const typeState = schemaLikeFieldTypeControlState(field, contract);
    return {
      namePlaceholder: overrides.namePlaceholder || view.fieldNamePlaceholder,
      descriptionPlaceholder: overrides.descriptionPlaceholder || view.fieldDescriptionPlaceholder,
      removeTitle: overrides.removeTitle || view.fieldRemoveTitle,
      enumLabel: overrides.enumLabel || view.fieldEnumLabel,
      enumAddLabel: overrides.enumAddLabel || view.fieldEnumAddLabel,
      enumAddValue: overrides.enumAddValue || view.fieldEnumAddValue,
      enumValues: enumValuesForField(field),
      typeState,
    };
  }

  function inputParamFieldControlState(param, contract, basicView = null) {
    const view = basicEditorViewState(basicView);
    return schemaFieldRowControlState(param, contract, null, {
      namePlaceholder: view.inputParamNamePlaceholder,
      descriptionPlaceholder: view.inputParamDescriptionPlaceholder,
      removeTitle: view.inputParamRemoveTitle,
      enumLabel: view.inputParamEnumLabel,
      enumAddLabel: view.inputParamEnumAddLabel,
      enumAddValue: view.inputParamEnumAddValue,
    });
  }

  function enumValueDraftPatch(field, index, rawValue) {
    const values = enumValuesForField(field);
    const i = Number(index);
    if (!Number.isInteger(i) || i < 0 || i >= values.length) return { enumValues: values };
    const next = [...values];
    next[i] = String(rawValue ?? "");
    return { enumValues: next };
  }

  function enumValueCommitPatch(field, index, rawValue) {
    const values = enumValuesForField(field);
    const i = Number(index);
    if (!Number.isInteger(i) || i < 0 || i >= values.length) return { enumValues: values };
    const next = [...values];
    next[i] = uniqueEnumValue(values, rawValue, i);
    return { enumValues: next };
  }

  function enumValueDeletePatch(field, index) {
    const values = enumValuesForField(field);
    const i = Number(index);
    if (!Number.isInteger(i) || i < 0 || i >= values.length) return { enumValues: values };
    return { enumValues: values.filter((_, j) => j !== i) };
  }

  function enumValueAddPatch(field, rawValue = "value") {
    const values = enumValuesForField(field);
    return { enumValues: [...values, uniqueEnumValue(values, rawValue)] };
  }

  function schemaFieldUpdatePatch(schema, fieldId, patch = {}, contract) {
    const fields = Array.isArray(schema?.fields) ? schema.fields : [];
    const current = fields.find((field) => field?.id === fieldId) || null;
    if (!current) return { fields };
    const normalized = normalizeSchemaLikeFieldPatch(current, patch, contract);
    if (Object.prototype.hasOwnProperty.call(normalized, "name")) {
      normalized.name = uniqueSchemaFieldName(fields, normalized.name, fieldId, editorSchemaFieldNameFallback(contract));
    }
    return { fields: fields.map((field) => field?.id === fieldId ? { ...field, ...normalized } : field) };
  }

  function schemaFieldUpdateCascadePatch({ schema, schemas, flow, edges, members, instances } = {}, fieldId, patch = {}, contract) {
    const currentSchemaId = String(schema?.id || "").trim();
    const updatePatch = schemaFieldUpdatePatch(schema, fieldId, patch, contract);
    const nextSchema = { ...(schema || {}), ...updatePatch };
    const list = Array.isArray(schemas) ? schemas : [];
    const nextSchemas = currentSchemaId
      ? list.map((candidate) => candidate?.id === currentSchemaId ? nextSchema : candidate)
      : list;
    const reconciled = reconcileConditionFieldAvailability({
      flow,
      edges,
      members,
      instances,
      schemas: nextSchemas,
    });
    return {
      patch: updatePatch,
      schema: nextSchema,
      schemas: nextSchemas,
      flow: reconciled.flow,
      edges: reconciled.edges,
    };
  }

  function schemaFieldRenameCascadePatch({ schema, schemas, flow, edges, members, instances } = {}, fieldId, rawName, oldName, contract) {
    const currentSchemaId = String(schema?.id || "").trim();
    const updatePatch = schemaFieldUpdatePatch(schema, fieldId, { name: rawName }, contract);
    const nextSchema = { ...(schema || {}), ...updatePatch };
    const list = Array.isArray(schemas) ? schemas : [];
    const nextSchemas = currentSchemaId
      ? list.map((candidate) => candidate?.id === currentSchemaId ? nextSchema : candidate)
      : list;
    const nextField = (nextSchema.fields || []).find((field) => field?.id === fieldId) || null;
    const previousName = String(oldName || "").trim();
    const nextName = String(nextField?.name || "").trim();
    const reconciled = previousName && previousName !== nextName
      ? reconcileSchemaFieldReferences({
        flow,
        edges,
        members,
        instances,
        schemaId: currentSchemaId,
        oldName: previousName,
        newName: nextName,
      })
      : { flow, edges };
    return {
      patch: updatePatch,
      schema: nextSchema,
      schemas: nextSchemas,
      flow: reconciled.flow,
      edges: reconciled.edges,
    };
  }

  function schemaFieldDeletePatch(schema, fieldId) {
    const fields = Array.isArray(schema?.fields) ? schema.fields : [];
    const removed = fields.find((field) => field?.id === fieldId) || null;
    return { removed, patch: { fields: fields.filter((field) => field?.id !== fieldId) } };
  }

  function schemaFieldDeleteCascadePatch({ schema, schemas, flow, edges, members, instances } = {}, fieldId) {
    const deleteResult = schemaFieldDeletePatch(schema, fieldId);
    const currentSchemaId = String(schema?.id || "").trim();
    if (!currentSchemaId) {
      return {
        removed: deleteResult.removed,
        patch: deleteResult.patch,
        schemas: Array.isArray(schemas) ? schemas : [],
        flow,
        edges,
      };
    }
    const list = Array.isArray(schemas) ? schemas : [];
    const nextSchema = { ...(schema || {}), ...deleteResult.patch };
    const nextSchemas = list.map((candidate) => candidate?.id === currentSchemaId ? nextSchema : candidate);
    const removedName = String(deleteResult.removed?.name || "").trim();
    const reconciled = removedName
      ? reconcileSchemaFieldReferences({
        flow,
        edges,
        members,
        instances,
        schemaId: currentSchemaId,
        oldName: removedName,
        newName: "",
      })
      : { flow, edges };
    return {
      removed: deleteResult.removed,
      patch: deleteResult.patch,
      schema: nextSchema,
      schemas: nextSchemas,
      flow: reconciled.flow,
      edges: reconciled.edges,
    };
  }

  function schemaFieldAddPatch(schema, contract) {
    const fields = Array.isArray(schema?.fields) ? schema.fields : [];
    const draft = editorSchemaDraftContract(contract);
    if (!draft) {
      return {
        ok: false,
        error: "MobKit schema is missing mob_definition.editor_schema_draft",
        patch: { fields },
      };
    }
    const nextNumber = Math.max(0, ...fields.map((field) => Number(String(field?.id || "f0").slice(1)) || 0)) + 1;
    const field = {
      id: `f${nextNumber}`,
      name: uniqueSchemaFieldName(fields, draft.addedField.name),
      type: draft.schemaFieldType,
      required: draft.addedField.required,
      description: draft.addedField.description,
      enumValues: draft.addedField.enumValues,
    };
    return { field, patch: { fields: [...fields, field] } };
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

  function studioAddInstancePatch({ instances, members } = {}, instance) {
    const list = Array.isArray(instances) ? instances : [];
    const validation = graphInstanceValidation(instance, { instances: list, members });
    if (!validation.ok) return { ok: false, error: validation.error, instances: list, instance: null };
    return { ok: true, error: "", instances: [...list, instance], instance };
  }

  function studioAppendInstancesPatch({ instances, members } = {}, nextInstances = []) {
    let list = Array.isArray(instances) ? instances : [];
    for (const instance of Array.isArray(nextInstances) ? nextInstances : []) {
      const validation = graphInstanceValidation(instance, { instances: list, members });
      if (!validation.ok) continue;
      list = [...list, instance];
    }
    return { instances: list };
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

  function graphInstanceIdSet(instances) {
    return new Set((Array.isArray(instances) ? instances : [])
      .map((instance) => String(instance?.id || "").trim())
      .filter(Boolean));
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

  function studioAddEdgePatch({ edges, instances } = {}, edge) {
    const list = Array.isArray(edges) ? edges : [];
    const validation = graphEdgeValidation(edge, { instances, edges: list });
    if (!validation.ok) return { ok: false, error: validation.error, edges: list, edge: null };
    return { ok: true, error: "", edges: [...list, edge], edge };
  }

  function graphConnectionAddPatch({ fromId, toId, instances, edges, contract } = {}) {
    const from = String(fromId || "").trim();
    const to = String(toId || "").trim();
    const sourceInstances = Array.isArray(instances) ? instances : [];
    const sourceEdges = Array.isArray(edges) ? edges : [];
    if (!from || !to || from === to) {
      return { ok: false, error: "edge endpoints must be different graph nodes", edges: sourceEdges, edge: null, selectId: "" };
    }
    const fromInstance = sourceInstances.find((instance) => String(instance?.id || "") === from) || null;
    const toInstance = sourceInstances.find((instance) => String(instance?.id || "") === to) || null;
    if (!fromInstance || !toInstance) {
      return { ok: false, error: "edge endpoints must reference existing graph nodes", edges: sourceEdges, edge: null, selectId: "" };
    }
    const draft = graphConnectionEdgeDraft({
      from: fromInstance,
      to: toInstance,
      edges: sourceEdges,
      contract,
    });
    if (!draft) return { ok: false, error: "edge draft unavailable", edges: sourceEdges, edge: null, selectId: "" };
    const patch = studioAddEdgePatch({ edges: sourceEdges, instances: sourceInstances }, draft);
    return {
      ...patch,
      selectId: patch.ok && patch.edge ? patch.edge.id : "",
    };
  }

  function studioAppendEdgesPatch({ edges, instances } = {}, nextEdges = []) {
    let list = Array.isArray(edges) ? edges : [];
    for (const edge of Array.isArray(nextEdges) ? nextEdges : []) {
      const validation = graphEdgeValidation(edge, { instances, edges: list });
      if (!validation.ok) continue;
      list = [...list, edge];
    }
    return {
      edges: list,
    };
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

  function inputParamUpdateCascadePatch({ flow, edges, members, instances, schemas } = {}, stepId, paramId, patch, contract) {
    const step = flowStepById(flow?.steps || [], stepId);
    const params = inputParamsForStep(step || {});
    const updatePatch = inputParamUpdatePatch(params, paramId, patch, contract);
    const updatedFlow = flowStepUpdatePatch(flow, stepId, updatePatch);
    const reconciled = reconcileConditionFieldAvailability({
      flow: updatedFlow,
      edges,
      members,
      instances,
      schemas,
    });
    return {
      patch: updatePatch,
      flow: reconciled.flow,
      edges: reconciled.edges,
    };
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

  function inputParamRenameCascadePatch({ flow, edges } = {}, stepId, paramId, rawName, previousName, contract) {
    const step = flowStepById(flow?.steps || [], stepId);
    const params = inputParamsForStep(step || {});
    const oldName = String(previousName || params.find((param) => param?.id === paramId)?.name || "").trim();
    const renamed = inputParamRenamePatch(params, paramId, rawName, contract);
    const updatedFlow = flowStepUpdatePatch(flow, stepId, renamed.patch);
    const reconciled = oldName && oldName !== renamed.name
      ? reconcileInputParamReferences({
        flow: updatedFlow,
        edges,
        oldName,
        newName: renamed.name,
      })
      : { flow: updatedFlow, edges };
    return {
      ...renamed,
      flow: reconciled.flow,
      edges: reconciled.edges,
    };
  }

  function inputParamDeleteCascadePatch({ flow, edges } = {}, stepId, paramId, contract) {
    const step = flowStepById(flow?.steps || [], stepId);
    const params = inputParamsForStep(step || {});
    const deleted = inputParamDeletePatch(params, paramId, contract);
    const updatedFlow = flowStepUpdatePatch(flow, stepId, deleted.patch);
    const oldName = String(deleted.removed?.name || "").trim();
    const reconciled = oldName
      ? reconcileInputParamReferences({
        flow: updatedFlow,
        edges,
        oldName,
        newName: "",
      })
      : { flow: updatedFlow, edges };
    return {
      ...deleted,
      flow: reconciled.flow,
      edges: reconciled.edges,
    };
  }

  function inputParamAddPatch(params, contract) {
    const current = Array.isArray(params) ? params : [];
    const draft = editorInputParamDraftContract(contract);
    if (!draft) {
      return {
        ok: false,
        error: "MobKit schema is missing mob_definition.editor_input_param_draft",
        patch: { inputParams: current, fields: inputParamSummary(current, contract) },
      };
    }
    const nextNumber = Math.max(0, ...current.map((param) => Number(String(param?.id || "p0").slice(1)) || 0)) + 1;
    const param = {
      id: `p${nextNumber}`,
      name: uniqueInputParamName(current, draft.addedField.name),
      type: draft.schemaFieldType,
      required: draft.addedField.required,
      description: draft.addedField.description,
      enumValues: draft.addedField.enumValues,
    };
    const next = [...current, param];
    return { param, patch: { inputParams: next, fields: inputParamSummary(next, contract) } };
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

  function basicBranchDefaultLabel(index, basicView = null) {
    const view = basicEditorViewState(basicView);
    const prefix = view.branchConditionRowTitlePrefix;
    return [prefix, String(index || 1)].filter(Boolean).join(" ");
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

  function reconcileSchemaFieldReferencesInEdges(edges, stepSchemas, schemaId, oldName, newName) {
    let changed = false;
    const next = (edges || []).map((edge) => {
      const condition = normalizedEdgeCondition(edge);
      const path = String(condition?.path || "").trim();
      const parts = path.split(".").filter(Boolean);
      if (parts.length !== 3 || parts[0] !== "steps") return edge;
      const stepId = parts[1];
      const field = parts[2];
      if (stepSchemas.get(stepId) !== schemaId || field !== oldName) return edge;
      changed = true;
      if (!newName) {
        return { ...edge, cond: null, label: "" };
      }
      const nextPath = `steps.${stepId}.${newName}`;
      const nextCond = { var: nextPath, op: condition.op || "", val: condition.val ?? "" };
      return {
        ...edge,
        cond: nextCond,
        label: edge.label && edge.label === conditionTextForPath(path, condition)
          ? conditionTextForPath(nextPath, nextCond)
          : edge.label,
      };
    });
    return changed ? next : edges;
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

  function graphConnectionEdgeDraft({ from, to, edges, id, contract } = {}) {
    if (!from || !to || !from.id || !to.id || from.id === to.id) return null;
    if ((edges || []).some((edge) => edge.from === from.id && edge.to === to.id)) return null;

    const draft = editorGraphDraftContract(contract);
    const defaultKind = contractDefaultValue(contract, "graph_edge_kind");
    const fanoutKind = contractDefaultValue(contract, "graph_fanout_edge_kind");
    const conditionKind = contractDefaultValue(contract, "graph_condition_edge_kind");
    if (!defaultKind || !draft) return null;
    let kind = defaultKind;
    let label = "";

    if (to.isTerminal) {
      kind = defaultKind;
      label = draft.terminalEdgeLabelPrefix + String(to.label || "").toLowerCase();
    } else if (from.isGate && from.gateKind === "fork") {
      if (!fanoutKind) return null;
      kind = fanoutKind;
    } else if (to.isGate && to.gateKind === "join") {
      kind = defaultKind;
    } else if (to.col === from.col) {
      if (!fanoutKind) return null;
      kind = fanoutKind;
      label = draft.parallelEdgeLabel;
    } else if (to.col < from.col) {
      if (!conditionKind) return null;
      kind = conditionKind;
      label = draft.reworkEdgeLabel;
    }

    return {
      id: id || uniqueGraphEdgeId(from.id, to.id, edges),
      from: from.id,
      to: to.id,
      kind,
      label,
    };
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

  function graphToolTagClass(toolId) {
    const id = String(toolId || "");
    if (id.startsWith("shell") || id === "git") return " is-shell";
    if (id.startsWith("mcp")) return " is-write";
    return "";
  }

  const GRAPH_NODE_W = 200;
  const GRAPH_NODE_H = 156;

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

  function graphNodeCanvasState({ inst, members = [], density = "", graphView = null } = {}) {
    const view = graphCanvasViewState(graphView);
    const isCompact = density === "compact";
    if (inst?.isTerminal) {
      const isSourceFile = !!inst.isSourceFile || /mob\.toml/i.test([inst.id, inst.label, inst.kind].filter(Boolean).join(" "));
      return {
        hidden: false,
        isTerminal: true,
        isSourceFile,
        dataKind: inst.kind,
        role: isSourceFile ? "button" : undefined,
        tabIndex: isSourceFile ? 0 : undefined,
        ariaLabel: isSourceFile ? view.sourceFileAriaLabel : undefined,
        sourceGlyph: isSourceFile ? view.sourceFileGlyph : "",
        roleLabel: isSourceFile ? view.sourceFileRoleLabel : `terminal · ${inst.kind}`,
        title: inst.label,
        subtitle: isSourceFile ? "" : inst.kind,
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
        className: "tag" + graphToolTagClass(tool),
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

  function graphSourceFileNode({ instances = [], graphView = null } = {}) {
    const view = graphCanvasViewState(graphView);
    const sourceInstances = Array.isArray(instances) ? instances : [];
    if (sourceInstances.some((instance) => instance?.isSourceFile || /mob\.toml/i.test([instance?.id, instance?.label, instance?.kind].filter(Boolean).join(" ")))) {
      return null;
    }
    const positioned = sourceInstances
      .filter((instance) => Number.isFinite(Number(instance?.col)) && Number.isFinite(Number(instance?.row)));
    const minCol = positioned.length
      ? Math.min(...positioned.map((instance) => Number(instance.col)))
      : 0;
    const minRow = positioned.length
      ? Math.min(...positioned.map((instance) => Number(instance.row)))
      : 0;
    return {
      id: "source_mob_toml",
      isTerminal: true,
      isSourceFile: true,
      isGraphAdornment: true,
      kind: "source",
      label: view.sourceFileLabel,
      col: minCol,
      row: minRow - 1,
    };
  }

  function graphCanvasInstances({ instances = [], graphView = null } = {}) {
    const sourceInstances = Array.isArray(instances) ? instances : [];
    const sourceFileNode = graphSourceFileNode({ instances: sourceInstances, graphView });
    return sourceFileNode ? [sourceFileNode, ...sourceInstances] : sourceInstances;
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

  function conditionTextForPath(path, condition) {
    const op = condition.op || "";
    return op ? `${path} ${op} ${JSON.stringify(String(condition.val ?? ""))}` : "";
  }

  function reconcileMemberSchemasInSteps(steps, memberById) {
    let changed = false;
    const next = (steps || []).map((step) => {
      const reconciled = reconcileMemberSchemaInStep(step, memberById);
      if (reconciled !== step) changed = true;
      return reconciled;
    });
    return changed ? next : steps;
  }

  function reconcileMemberSchemaInStep(step, memberById) {
    if (!step || typeof step !== "object") return step;
    if (step.type === "member") {
      const member = memberById.get(step.role);
      if (!member) return step;
      const memberSchema = String(member.schema || "").trim();
      const stepSchema = String(step.schema || "").trim();
      const expected = String(step.expectedSchemaRef || step.expected_schema_ref || "").trim();
      if (!memberSchema) {
        if (!stepSchema && !expected && !("expected_schema_ref" in step)) return step;
        const { schema, expectedSchemaRef, expected_schema_ref, ...rest } = step;
        return rest;
      }
      let next = step;
      if (stepSchema !== memberSchema) {
        next = { ...next, schema: memberSchema };
        if (expected && stepSchema && expected === `schemas/${stepSchema}.json`) {
          next.expectedSchemaRef = `schemas/${memberSchema}.json`;
        }
      }
      if ("expected_schema_ref" in next) {
        const { expected_schema_ref, ...rest } = next;
        next = rest;
      }
      return next;
    }
    if (step.type === "repeat") {
      const nested = reconcileMemberSchemasInSteps(step.steps || [], memberById);
      return nested === step.steps ? step : { ...step, steps: nested };
    }
    if (step.type === "branch" || step.type === "parallel") {
      let changed = false;
      const branches = (step.branches || []).map((branch) => {
        const branchSteps = reconcileMemberSchemasInSteps(branch.steps || [], memberById);
        if (branchSteps !== branch.steps) changed = true;
        return branchSteps === branch.steps ? branch : { ...branch, steps: branchSteps };
      });
      const fallback = Array.isArray(step.fallback)
        ? reconcileMemberSchemasInSteps(step.fallback, memberById)
        : step.fallback;
      if (fallback !== step.fallback) changed = true;
      return changed ? { ...step, branches, fallback } : step;
    }
    return step;
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
    if (String(editorMode || "") !== "advanced") return flow;
    return graphToFlow({
      instances,
      edges,
      members,
      previousFlow: flow,
      contract,
    });
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

  function jsonEquivalent(a, b) {
    return JSON.stringify(a) === JSON.stringify(b);
  }

  function authoringProjectionApplyPlan(projection, current = {}) {
    if (!projection || typeof projection !== "object") return { ok: false };
    const studio = current?.studio && typeof current.studio === "object" ? current.studio : {};
    const members = Array.isArray(projection.members) ? projection.members : [];
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

  function findMember(members, id) {
    return (members || []).find((member) => member.id === id) || null;
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

  function numberOrNull(value) {
    if (value === "" || value === null || value === undefined) return null;
    const n = Number(value);
    return Number.isFinite(n) ? n : null;
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

  function hasAuthoringLaunchMode(source) {
    return !!source && typeof source === "object" && ("launchMode" in source || "launch_mode" in source);
  }

  function launchModeFromAuthoringSource(source, fallback) {
    const raw = hasAuthoringLaunchMode(source)
      ? (source.launchMode ?? source.launch_mode)
      : (hasAuthoringLaunchMode(fallback) ? (fallback.launchMode ?? fallback.launch_mode) : null);
    if (!raw || typeof raw !== "object" || !String(raw.kind || "").trim()) return null;
    return normalizeLaunchMode(raw);
  }

  function memberDisplayName(members, id) {
    return ((members || []).find((member) => member.id === id) || {}).name || id;
  }

  function normalizeLaunchMode(mode) {
    if (!mode || typeof mode !== "object") return null;
    const kind = canonicalLaunchModeKind(mode.kind);
    if (!kind) return null;
    const rawBudgetSplitPolicy = mode.budgetSplitPolicy ?? mode.budget_split_policy ?? mode.budget;
    const budgetSplitPolicy = rawBudgetSplitPolicy
      ? normalizeBudgetSplitPolicy(rawBudgetSplitPolicy)
      : null;
    const budgetPatch = budgetSplitPolicy ? { budgetSplitPolicy } : {};
    if (kind === "Resume") {
      return {
        kind: "Resume",
        sessionId: String(mode.sessionId || mode.session_id || mode.bridgeSessionId || mode.bridge_session_id || "").trim(),
        ...budgetPatch,
      };
    }
    if (kind === "Fork") {
      return {
        kind: "Fork",
        from: String(mode.from || mode.sourceMemberId || mode.source_member_id || "").trim(),
        context: normalizeForkContext(mode.context || mode.forkContext || mode.fork_context),
        ...budgetPatch,
      };
    }
    return { kind, ...budgetPatch };
  }

  function launchModeControlState(source, contract, launchView = null) {
    const view = launchViewForState(launchView);
    const authoredLaunchMode = source && typeof source === "object"
      ? (source.launchMode ?? source.launch_mode)
      : null;
    const defaultLaunchMode = contractDefaultValue(contract, "launch_mode");
    const launchMode = authoredLaunchMode && typeof authoredLaunchMode === "object"
      ? authoredLaunchMode
      : { kind: defaultLaunchMode };
    const launchKind = canonicalLaunchModeKind(launchMode.kind || defaultLaunchMode);
    const authoredBudgetSplitPolicy = normalizeBudgetSplitPolicy(
      launchMode.budgetSplitPolicy || launchMode.budget_split_policy,
    );
    const defaultBudgetSplitKind = contractDefaultValue(contract, "budget_split_policy");
    const budgetSplitPolicy = authoredBudgetSplitPolicy
      || normalizeBudgetSplitPolicy(defaultBudgetSplitKind ? { kind: defaultBudgetSplitKind } : null)
      || { kind: "" };
    const budgetLaunchPatch = authoredBudgetSplitPolicy ? { budgetSplitPolicy: authoredBudgetSplitPolicy } : {};
    const launchOptions = launchModeOptions(contract, launchKind, view);
    const budgetOptions = budgetSplitPolicyOptions(contract, budgetSplitPolicy.kind, view);
    const defaultForkContext = contractDefaultValue(contract, "fork_context");
    const forkContextValue = normalizeForkContext(launchMode.context || defaultForkContext);
    const forkOptions = forkContextOptions(contract, forkContextValue, view);
    const fixedLimitValue = budgetSplitPolicy.limit || view.fixedBudgetDefaultValue;
    return {
      launchTitle: view.launchTitle,
      graphLaunchTitle: view.graphLaunchTitle,
      resumeSessionLabel: view.resumeSessionLabel,
      resumeSessionPlaceholder: view.resumeSessionPlaceholder,
      forkSourceLabel: view.forkSourceLabel,
      forkContextLabel: view.forkContextLabel,
      graphForkContextLabel: view.graphForkContextLabel,
      budgetPolicyLabel: view.budgetPolicyLabel,
      fixedBudgetLabel: view.fixedBudgetLabel,
      fixedBudgetValue: fixedLimitValue,
      launchMode,
      launchKind,
      defaultLaunchMode,
      launchOptions,
      selectedLaunchMode: launchOptions.find((option) => option.value === launchKind),
      authoredBudgetSplitPolicy,
      budgetSplitPolicy,
      budgetLaunchPatch,
      budgetOptions,
      selectedBudgetPolicy: budgetOptions.find((option) => option.value === budgetSplitPolicy.kind),
      defaultForkContext,
      forkContextValue,
      forkContextOptions: forkOptions,
      selectedForkContext: forkOptions.find((option) => option.value === forkContextValue),
    };
  }

  function launchModeKindPatch(source, kind, contract, options = {}) {
    const state = launchModeControlState(source, contract);
    const nextKind = canonicalLaunchModeKind(kind);
    if (!launchModeKindAllowed(contract, nextKind)) return {};
    if (nextKind === "Fork") {
      return {
        launchMode: {
          ...state.launchMode,
          kind: "Fork",
          from: options.firstForkSourceId || state.launchMode.from || "",
          context: state.launchMode.context || state.defaultForkContext,
          ...state.budgetLaunchPatch,
        },
      };
    }
    if (nextKind === "Resume") {
      return {
        launchMode: {
          ...state.launchMode,
          kind: "Resume",
          sessionId: state.launchMode.sessionId || "",
          ...state.budgetLaunchPatch,
        },
      };
    }
    return { launchMode: { kind: nextKind, ...state.budgetLaunchPatch } };
  }

  function launchModeMergePatch(source, patch, contract) {
    const state = launchModeControlState(source, contract);
    const nextPatch = patch && typeof patch === "object" ? { ...patch } : {};
    if ("kind" in nextPatch) {
      const kind = canonicalLaunchModeKind(nextPatch.kind);
      if (!launchModeKindAllowed(contract, kind)) return {};
      nextPatch.kind = kind;
    }
    if ("context" in nextPatch) {
      const context = normalizeForkContext(nextPatch.context);
      if (!forkContextAllowed(contract, context)) return {};
      nextPatch.context = context;
    }
    return { launchMode: { ...state.launchMode, ...nextPatch } };
  }

  function launchModeSessionPatch(source, sessionId, contract) {
    return launchModeMergePatch(source, { sessionId: String(sessionId || "") }, contract);
  }

  function launchSourceAllowed(sourceOptions, from) {
    const value = String(from || "").trim();
    if (!value) return true;
    return (Array.isArray(sourceOptions) ? sourceOptions : [])
      .some((option) => String(option?.value || option?.id || "").trim() === value);
  }

  function launchModeForkSourcePatch(source, from, contract, options = {}) {
    const value = String(from || "").trim();
    if (!launchSourceAllowed(options.sourceOptions, value)) return {};
    return launchModeMergePatch(source, { from: value }, contract);
  }

  function launchModeForkContextPatch(source, context, contract) {
    return launchModeMergePatch(source, { context }, contract);
  }

  function launchModeBudgetPatch(source, patch, contract) {
    const state = launchModeControlState(source, contract);
    if (patch && typeof patch === "object" && "kind" in patch) {
      const requestedKind = canonicalBudgetSplitPolicyKind(patch.kind);
      if (!budgetSplitPolicyAllowed(contract, requestedKind)) return {};
    }
    const nextPolicy = normalizeBudgetSplitPolicy({ ...state.budgetSplitPolicy, ...patch });
    if (!nextPolicy || !budgetSplitPolicyAllowed(contract, nextPolicy.kind)) return {};
    return {
      launchMode: {
        ...state.launchMode,
        budgetSplitPolicy: nextPolicy,
      },
    };
  }

  function launchBudgetKindPatch(source, kind, contract) {
    return launchModeBudgetPatch(source, { kind: canonicalBudgetSplitPolicyKind(kind) }, contract);
  }

  function launchBudgetFixedLimitPatch(source, limit, contract) {
    return launchModeBudgetPatch(source, { kind: "Fixed", limit }, contract);
  }

  function canonicalLaunchModeKind(value) {
    const raw = String(value || "").trim();
    if (!raw) return "";
    const lower = raw.toLowerCase();
    if (lower === "resume") return "Resume";
    if (lower === "fork") return "Fork";
    if (lower === "fresh") return "Fresh";
    return raw;
  }

  function launchModeKindAllowed(contract, kind) {
    const canonicalKind = canonicalLaunchModeKind(kind);
    if (!canonicalKind) return false;
    const contractModes = Array.isArray(contract?.mob_definition?.launch_modes)
      ? contract.mob_definition.launch_modes.map(canonicalLaunchModeKind)
      : [];
    return contractModes.includes(canonicalKind);
  }

  function normalizeForkContext(value) {
    const raw = String(value || "").trim();
    if (!raw) return "";
    if (raw === "FullHistory") return "full_history";
    if (raw === "LastMessages") return "last_messages";
    return raw;
  }

  function forkContextAllowed(contract, context) {
    const normalized = normalizeForkContext(context);
    if (!normalized) return false;
    const contexts = Array.isArray(contract?.mob_definition?.fork_contexts)
      ? contract.mob_definition.fork_contexts.map(normalizeForkContext)
      : [];
    return contexts.includes(normalized);
  }

  function launchOptionLabel(labels, value, view, contractLabel) {
    return labels?.[value] || `${value}${view.unsupportedLabelSeparator}${contractLabel}`;
  }

  function launchUnsupportedReason(view, contractLabel) {
    return `${view.unsupportedReasonPrefix}${contractLabel}${view.unsupportedReasonSuffix}`;
  }

  function launchModeOptions(contract, currentKind, launchView = null) {
    const view = launchViewForState(launchView);
    const contractModes = Array.isArray(contract?.mob_definition?.launch_modes) && contract.mob_definition.launch_modes.length
      ? contract.mob_definition.launch_modes.map(canonicalLaunchModeKind)
      : [];
    const modes = [...contractModes];
    const currentSource = currentKind || contractDefaultValue(contract, "launch_mode");
    const current = currentSource ? canonicalLaunchModeKind(currentSource) : "";
    if (current && !modes.includes(current)) modes.push(current);
    return modes.map((mode) => {
      const supported = contractModes.includes(mode);
      return {
        value: mode,
        label: launchOptionLabel(view.launchModeLabels, mode, view, view.launchModesContractLabel),
        disabled: !supported,
        reason: supported ? "" : launchUnsupportedReason(view, view.launchModesContractLabel),
      };
    });
  }

  function normalizeDispatchMode(mode) {
    return String(mode || "").trim();
  }

  function dispatchModeOptions(contract, currentMode) {
    const contractLabel = "dispatch_modes";
    const contractModes = Array.isArray(contract?.mob_definition?.dispatch_modes) && contract.mob_definition.dispatch_modes.length
      ? contract.mob_definition.dispatch_modes.map(String)
      : [];
    const modes = [...contractModes];
    const current = String(currentMode || contractDefaultValue(contract, "dispatch_mode") || "").trim();
    if (!modes.includes(current)) modes.push(current);
    const labels = viewStringMapFromSchema(contract?.mob_definition?.dispatch_mode_labels);
    return modes.map((mode) => {
      const supported = contractModes.includes(mode);
      return {
        value: mode,
        label: labels[mode] || mobDefinitionUnsupportedOptionLabel(contract, mode, contractLabel),
        disabled: !supported,
        reason: supported ? "" : mobDefinitionUnsupportedOptionReason(contract, contractLabel),
      };
    });
  }

  function dispatchModeAllowed(contract, mode) {
    const value = String(mode || "").trim();
    if (!value) return true;
    const contractModes = Array.isArray(contract?.mob_definition?.dispatch_modes)
      ? contract.mob_definition.dispatch_modes.map(String)
      : [];
    return contractModes.includes(value);
  }

  function normalizeCollectionMode(policy) {
    const raw = typeof policy === "object" && policy
      ? String(policy.type || "").trim()
      : String(policy || "").trim();
    return raw;
  }

  function dependencyModeOptions(contract, currentMode) {
    const contractLabel = "dependency_modes";
    const contractModes = Array.isArray(contract?.mob_definition?.dependency_modes) && contract.mob_definition.dependency_modes.length
      ? contract.mob_definition.dependency_modes.map(String)
      : [];
    const modes = [...contractModes];
    const current = String(currentMode || contractDefaultValue(contract, "dependency_mode") || "").trim();
    if (!modes.includes(current)) modes.push(current);
    const labels = viewStringMapFromSchema(contract?.mob_definition?.dependency_mode_labels);
    return modes.map((mode) => {
      const supported = contractModes.includes(mode);
      return {
        value: mode,
        label: labels[mode] || mobDefinitionUnsupportedOptionLabel(contract, mode, contractLabel),
        disabled: !supported,
        reason: supported ? "" : mobDefinitionUnsupportedOptionReason(contract, contractLabel),
      };
    });
  }

  function dependencyModeAllowed(contract, mode) {
    const value = String(mode || "").trim();
    if (!value) return true;
    const contractModes = Array.isArray(contract?.mob_definition?.dependency_modes)
      ? contract.mob_definition.dependency_modes.map(String)
      : [];
    return contractModes.includes(value);
  }

  function collectionPolicyOptions(contract, currentPolicy) {
    const contractLabel = "collection_policies";
    const contractPolicies = Array.isArray(contract?.mob_definition?.collection_policies) && contract.mob_definition.collection_policies.length
      ? contract.mob_definition.collection_policies.map(String)
      : [];
    const policies = [...contractPolicies];
    const current = String(currentPolicy || contractDefaultValue(contract, "collection_policy") || "").trim();
    if (!policies.includes(current)) policies.push(current);
    const labels = viewStringMapFromSchema(contract?.mob_definition?.collection_policy_labels);
    return policies.map((policy) => {
      const supported = contractPolicies.includes(policy);
      return {
        value: policy,
        label: labels[policy] || mobDefinitionUnsupportedOptionLabel(contract, policy, contractLabel),
        disabled: !supported,
        reason: supported ? "" : mobDefinitionUnsupportedOptionReason(contract, contractLabel),
      };
    });
  }

  function mobDefinitionUnsupportedOptionLabel(contract, value, contractLabel) {
    const separator = String(contract?.mob_definition?.option_unsupported_label_separator || " ");
    return `${value}${separator}${contractLabel}`;
  }

  function mobDefinitionUnsupportedOptionReason(contract, contractLabel) {
    const prefix = String(contract?.mob_definition?.option_unsupported_reason_prefix || "");
    const suffix = String(contract?.mob_definition?.option_unsupported_reason_suffix || "");
    return `${prefix}${contractLabel}${suffix}`;
  }

  function collectionPolicyAllowed(contract, policy) {
    const value = String(policy || "").trim();
    if (!value) return true;
    const contractPolicies = Array.isArray(contract?.mob_definition?.collection_policies)
      ? contract.mob_definition.collection_policies.map(String)
      : [];
    return contractPolicies.includes(value);
  }

  function normalizeBudgetSplitPolicy(policy) {
    if (!policy || typeof policy !== "object") return null;
    const rawKind = String(policy.kind || policy.type || "").trim();
    if (!rawKind) return null;
    const kind = canonicalBudgetSplitPolicyKind(rawKind);
    if (kind === "Fixed") {
      const limit = numberOrNull(policy?.limit ?? policy?.value ?? policy?.tokens);
      return { kind: "Fixed", limit: limit && limit > 0 ? limit : 4096 };
    }
    return { kind };
  }

  function canonicalBudgetSplitPolicyKind(value) {
    const raw = String(value || "").trim();
    if (!raw) return "";
    const lower = raw.toLowerCase();
    if (lower === "fixed") return "Fixed";
    if (lower === "proportional") return "Proportional";
    if (lower === "remaining") return "Remaining";
    if (lower === "equal") return "Equal";
    return raw;
  }

  function budgetSplitPolicyAllowed(contract, kind) {
    const canonicalKind = canonicalBudgetSplitPolicyKind(kind);
    if (!canonicalKind) return false;
    const policies = Array.isArray(contract?.mob_definition?.budget_split_policies)
      ? contract.mob_definition.budget_split_policies.map(canonicalBudgetSplitPolicyKind)
      : [];
    return policies.includes(canonicalKind);
  }

  function budgetSplitPolicyOptions(contract, currentKind, launchView = null) {
    const view = launchViewForState(launchView);
    const contractPolicies = Array.isArray(contract?.mob_definition?.budget_split_policies) && contract.mob_definition.budget_split_policies.length
      ? contract.mob_definition.budget_split_policies.map(canonicalBudgetSplitPolicyKind)
      : [];
    const policies = [...contractPolicies];
    const currentSource = currentKind || contractDefaultValue(contract, "budget_split_policy");
    const current = currentSource ? canonicalBudgetSplitPolicyKind(currentSource) : "";
    if (current && !policies.includes(current)) policies.push(current);
    return policies.map((policy) => {
      const supported = contractPolicies.includes(policy);
      return {
        value: policy,
        label: launchOptionLabel(view.budgetSplitPolicyLabels, policy, view, view.budgetSplitPoliciesContractLabel),
        disabled: !supported,
        reason: supported ? "" : launchUnsupportedReason(view, view.budgetSplitPoliciesContractLabel),
      };
    });
  }

  function mobKitBudgetSplitPolicy(policy) {
    const normalized = normalizeBudgetSplitPolicy(policy);
    if (!normalized) return null;
    if (normalized.kind === "Fixed") return { type: "fixed", value: normalized.limit || 4096 };
    return { type: normalized.kind.replace(/[A-Z]/g, (ch, index) => `${index ? "_" : ""}${ch.toLowerCase()}`) };
  }

  function launchModesFromFlow(flow, members) {
    const out = [];
    collectVisualSteps(flow?.steps || [], (step) => {
      if (step.type !== "member") return;
      const member = findMember(members, step.role);
      const launchMode = launchModeFromAuthoringSource(step);
      const row = {
        step_id: step.id,
        member_id: step.role || "",
        profile: profileName(member || { id: step.role }),
        launch_mode: launchMode,
      };
      if (launchMode?.budgetSplitPolicy) {
        const budgetSplitPolicy = mobKitBudgetSplitPolicy(launchMode.budgetSplitPolicy);
        if (budgetSplitPolicy) row.budget_split_policy = budgetSplitPolicy;
      }
      out.push(row);
    });
    return out;
  }

  function conditionTextFromEdge(edge, fallback) {
    if (!edge) return fallback;
    const condition = normalizedEdgeCondition(edge);
    if (condition?.path) {
      if (condition.val === undefined || condition.val === null || String(condition.val).trim() === "") return fallback;
      const value = condition.val;
      if (!condition.op) return fallback;
      return `${condition.path} ${condition.op} ${JSON.stringify(value)}`;
    }
    return edge.label || fallback;
  }

  function edgeConditionToEditorCond(edge) {
    const condition = normalizedEdgeCondition(edge);
    const rawVar = String(condition?.path || "").trim();
    const op = String(condition?.op || "").trim();
    const val = condition?.val === undefined || condition?.val === null ? "" : String(condition.val);
    if (!rawVar || !op || !val) return null;
    const parts = rawVar.split(".").filter(Boolean);
    if (parts.length === 2 && parts[0] === "params") {
      return {
        namespace: "params",
        stepId: "params",
        field: parts[1],
        op,
        val,
      };
    }
    if (parts.length === 3 && parts[0] === "steps") {
      return {
        namespace: "steps",
        stepId: parts[1],
        field: parts[2],
        op,
        val,
      };
    }
    return null;
  }

  function repeatConditionFromEdge(edge, stepId) {
    const condition = normalizedEdgeCondition(edge);
    if (condition?.path) {
      const parts = String(condition.path).split(".").filter(Boolean);
      const field = parts.pop() || "";
      const op = String(condition.op || "").trim();
      const val = condition.val === undefined || condition.val === null ? "" : String(condition.val).trim();
      if (!field || !op || !val) return null;
      return { stepId, field, op, val };
    }
    const label = String(edge?.label || "");
    const match = /([A-Za-z0-9_.-]+)\s*(==|>|<)\s*['"]?([^'"]+)['"]?/.exec(label);
    if (match) {
      return { stepId, field: match[1].split(".").pop(), op: match[2], val: match[3] };
    }
    return null;
  }

  function normalizedEdgeCondition(edge) {
    const cond = edge?.cond || {};
    const op = String(cond.op || cond.operator || "").trim();
    const val = cond.val ?? cond.value;
    if (cond.var || cond.path || cond.source) {
      return {
        path: String(cond.var || cond.path || cond.source || "").trim(),
        op,
        val,
      };
    }
    const namespace = String(cond.namespace || "").trim();
    const stepId = String(cond.stepId || cond.step_id || "").trim();
    const field = String(cond.field || "").trim();
    if (field && (namespace === "params" || stepId === "params")) {
      return { path: `params.${field}`, op, val };
    }
    if (field && stepId) {
      return { path: `steps.${stepId}.${field}`, op, val };
    }
    return null;
  }

  function collectVisualSteps(steps, visit) {
    for (const step of steps || []) {
      visit(step);
      for (const lane of childLanes(step)) collectVisualSteps(lane.steps, visit);
    }
  }

  async function loadSchema() {
    return callRpc(rpcMethod("schema"), {});
  }

  async function loadCatalogs() {
    return callRpc(rpcMethod("catalogs"), {});
  }

  async function validateDocument(document) {
    return callRpc(rpcMethod("validate"), { document });
  }

  async function sourceDocument(document) {
    return callRpc(rpcMethod("source"), { document });
  }

  async function exportDocument(document) {
    return callRpc(rpcMethod("export"), { document });
  }

  async function deployDocument(document, options) {
    return callRpc(rpcMethod("deploy"), { document, ...(options || {}) });
  }

  async function deployCommandPreview(settings, options) {
    const deploy = normalizeDeploySettings(settings);
    return callRpc(rpcMethod("deployCommand"), {
      deploy,
      pack_path: options?.packPath || "<pack.mobpack>",
      prompt: options?.prompt || deploy.prompt || "<prompt>",
    });
  }

  async function deployCommandPreviewForDocument(document, options = {}) {
    const sourceDocument = document && typeof document === "object" ? document : {};
    const deploy = normalizeDeploySettings(sourceDocument.deploy || options.deploySettings);
    const prompt = String(options.prompt || deploy.prompt || "").trim();
    const request = {
      document: {
        ...sourceDocument,
        deploy,
      },
    };
    if (String(options.packPath || "").trim()) request.pack_path = String(options.packPath).trim();
    if (prompt) request.prompt = prompt;
    return callRpc(rpcMethod("deployCommand"), request);
  }

  async function importDocument(params) {
    return callRpc(rpcMethod("import"), params || {});
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
        raw: tool,
      }));
  }

  function emptyMobKitCatalogs(boot = {}) {
    return {
      models: [],
      toolCatalog: [],
      agentDefinitions: [],
      skillRealms: [],
      blankMobpack: null,
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
    const blankMobpack = blankMobpackFromCatalogs(catalogSource);
    return {
      models: modelCatalogFromCatalogs(catalogSource),
      toolCatalog: toolCatalogFromCatalogs(catalogSource),
      agentDefinitions,
      skillRealms: skillRealmsFromCatalogs(catalogSource),
      blankMobpack,
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
    const hasStoredEditorGraph = storedFrames.length > 0;
    if (!hasStoredEditorGraph && document?.flow && Array.isArray(document.flow.steps)) {
      return graphProjectionForFlow(document.flow, members || [], contract);
    }
    return {
      instances: Array.isArray(document?.instances) ? document.instances : [],
      edges: Array.isArray(document?.edges) ? document.edges : [],
      frames: storedFrames,
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
    const graphProjection = graphProjectionForDocument({ ...document, flow }, members, options.contract);
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

  function flowImportedIdFromDocument(document, result = {}, existingRows = []) {
    const source = result?.source_name || result?.sourceName || result?.filename || result?.source;
    const name = document?.name || document?.mob_id || document?.flow?.name || source || "";
    if (!String(name || "").trim()) return "";
    return flowDraftIdFromSpec({
      name,
    }, existingRows);
  }

  function runtimeModeOptions(contract, deploySettings, currentMode) {
    const modes = Array.isArray(contract?.mob_definition?.runtime_modes) && contract.mob_definition.runtime_modes.length
      ? contract.mob_definition.runtime_modes.map(String)
      : [];
    const current = String(currentMode || "");
    if (current && !modes.includes(current)) modes.push(current);
    const surface = String(deploySettings?.surface || contract?.deploy_settings?.defaults?.surface || "");
    const labels = viewStringMapFromSchema(contract?.mob_definition?.runtime_mode_labels);
    return modes.map((mode) => {
      const surfaceBlocked = !runtimeModeDeploySurfaceAllowed(contract, surface, mode);
      return {
        value: mode,
        label: labels[mode] || `${mode}`,
        disabled: surfaceBlocked,
        reason: surfaceBlocked ? runtimeModeDeploySurfaceReason(contract, surface, mode) : "",
      };
    });
  }

  function deployRuntimeCompatibility(contract, surface) {
    const compatibility = contract?.mob_definition?.deploy_runtime_mode_compatibility;
    if (!compatibility || typeof compatibility !== "object") return null;
    const surfaceKey = String(surface || contract?.deploy_settings?.defaults?.surface || "").trim();
    const surfaceContract = compatibility[surfaceKey];
    return surfaceContract && typeof surfaceContract === "object" ? surfaceContract : null;
  }

  function deploySurfaceRuntimeModes(contract, surface) {
    return contractStringValues(deployRuntimeCompatibility(contract, surface)?.allowed);
  }

  function runtimeModeDeploySurfaceAllowed(contract, surface, mode) {
    const value = String(mode || "").trim();
    if (!value) return true;
    const allowed = deploySurfaceRuntimeModes(contract, surface);
    return allowed.length ? allowed.includes(value) : true;
  }

  function runtimeModeDeploySurfaceReason(contract, surface, mode) {
    const value = String(mode || "").trim();
    const blocked = deployRuntimeCompatibility(contract, surface)?.blocked;
    const reason = blocked && typeof blocked === "object" ? blocked[value] : "";
    return String(reason || "Unsupported by this MobKit deploy surface.");
  }

  function firstDeploySurfaceRuntimeMode(contract, surface) {
    return deploySurfaceRuntimeModes(contract, surface)[0] || "";
  }

  function simpleContractOptions(values, currentValue, labels, contractName) {
    const contractValues = Array.isArray(values)
      ? values.map((value) => String(value || "").trim()).filter(Boolean)
      : [];
    const options = contractValues.length ? [...contractValues] : [];
    const current = String(currentValue || "").trim();
    if (current && !options.includes(current)) options.push(current);
    return options.map((value) => {
      const known = contractValues.includes(value);
      return {
        value,
        label: labels?.[value] || value,
        disabled: !known,
        reason: known ? "" : `Unsupported by the MobKit ${contractName} contract.`,
      };
    });
  }

  function profileBindingRestriction(contract, binding) {
    const restrictions = contract?.mob_definition?.profile_binding_restrictions;
    const value = restrictions && typeof restrictions === "object" ? restrictions[binding] : null;
    return value && typeof value === "object" ? value : {};
  }

  function deploySurfaceOptions(contract, currentSurface) {
    return simpleContractOptions(
      contract?.deploy_settings?.surfaces,
      currentSurface || "",
      { cli: "cli", rpc: "rpc" },
      "deploy_settings.surfaces"
    );
  }

  function trustPolicyOptions(contract, currentPolicy) {
    return simpleContractOptions(
      contract?.deploy_settings?.trust_policies,
      currentPolicy || "",
      { permissive: "permissive", strict: "strict" },
      "deploy_settings.trust_policies"
    );
  }

  function realmBackendOptions(contract, currentBackend) {
    return simpleContractOptions(
      contract?.deploy_settings?.realm_backends,
      currentBackend || "",
      { jsonl: "jsonl", sqlite: "sqlite" },
      "deploy_settings.realm_backends"
    );
  }

  function profileBackendOptions(contract, currentBackend, includeDefault, defaultLabel = "") {
    const options = simpleContractOptions(
      contract?.mob_definition?.profile_backends,
      currentBackend || "",
      { session: "session", external: "external" },
      "mob_definition.profile_backends"
    );
    if (!includeDefault) return options;
    return [{ value: "", label: String(defaultLabel || ""), disabled: false, reason: "" }, ...options.filter(option => option.value)];
  }

  function profileBindingOptions(contract, currentBinding) {
    return simpleContractOptions(
      contract?.mob_definition?.profile_binding,
      currentBinding || "",
      {
        inline: "inline — define profile in this mobpack",
        realm_profile: "realm_profile",
      },
      "mob_definition.profile_binding"
    ).map((option) => {
      const restriction = profileBindingRestriction(contract, option.value);
      const deployable = restriction.deployable;
      return {
        ...option,
        label: String(restriction.label || option.label || option.value),
        disabled: option.disabled || deployable === false,
        reason: String(restriction.reason || option.reason || ""),
      };
    });
  }

  function mobBackendDefaultOptions(contract, currentBackend) {
    return simpleContractOptions(
      contract?.mob_definition?.profile_backends,
      currentBackend || "",
      { session: "session", external: "external" },
      "mob_definition.mob_settings.backendDefault"
    );
  }

  function tweaksControlState({
    flows = [],
    deploySettings = {},
    mobSettings = {},
    members = [],
    modelCatalog = [],
    contract = null,
    settingsView = null,
  } = {}) {
    const view = settingsViewForState(settingsView);
    const loadableFlowOptions = (Array.isArray(flows) ? flows : [])
      .filter((flow) => flow?.document)
      .map((flow) => ({
        value: flow.id,
        label: `${flow.name}${view.optionSeparator}${flow.stage || flow.source || view.flowStageFallback}`,
      }));
    const profileOptions = [
      { value: "", label: view.profileNoneLabel },
      ...(Array.isArray(members) ? members : []).map((member) => {
        const profile = profileName(member);
        return { value: profile, label: profile };
      }),
    ];
    const modelOptions = [
      { value: "", label: view.modelDefaultLabel },
      ...(Array.isArray(modelCatalog) ? modelCatalog : []).map((model) => ({
        value: model.id,
        label: `${model.label || model.id}${view.optionSeparator}${model.vendor || view.modelVendorFallback}`,
      })),
    ];
    return {
      panelTitle: view.panelTitle,
      panelCloseLabel: view.panelCloseLabel,
      loadMobTitle: view.loadMobTitle,
      loadMobLabel: view.loadMobLabel,
      canvasTitle: view.canvasTitle,
      edgeStyleLabel: view.edgeStyleLabel,
      edgeStyleOptions: view.edgeStyleOptions,
      densityLabel: view.densityLabel,
      densityOptions: view.densityOptions,
      themeTitle: view.themeTitle,
      themeModeLabel: view.themeModeLabel,
      themeModeOptions: view.themeModeOptions,
      mobTitle: view.mobTitle,
      orchestratorLabel: view.orchestratorLabel,
      autoWireLabel: view.autoWireLabel,
      autoWireOptions: view.autoWireOptions,
      roleWiringLabel: view.roleWiringLabel,
      roleWiringAddLabel: view.roleWiringAddLabel,
      defaultBackendLabel: view.defaultBackendLabel,
      externalBaseLabel: view.externalBaseLabel,
      externalBasePlaceholder: view.externalBasePlaceholder,
      advancedLabel: view.advancedLabel,
      advancedObjectRequiredError: view.advancedObjectRequiredError,
      advancedInvalidJsonError: view.advancedInvalidJsonError,
      deployTitle: view.deployTitle,
      surfaceLabel: view.surfaceLabel,
      trustLabel: view.trustLabel,
      modelLabel: view.modelLabel,
      durationLabel: view.durationLabel,
      durationPlaceholder: view.durationPlaceholder,
      toolCallsLabel: view.toolCallsLabel,
      toolCallsMin: view.toolCallsMin,
      toolCallsMax: view.toolCallsMax,
      tokensLabel: view.tokensLabel,
      tokensMin: view.tokensMin,
      tokensMax: view.tokensMax,
      realmLabel: view.realmLabel,
      realmOptions: view.realmOptions,
      realmIdLabel: view.realmIdLabel,
      realmIdPlaceholder: view.realmIdPlaceholder,
      backendLabel: view.backendLabel,
      promptLabel: view.promptLabel,
      promptPlaceholder: view.promptPlaceholder,
      commandLabel: view.commandLabel,
      commandFallback: view.commandFallback,
      inspectorTitle: view.inspectorTitle,
      inspectorLayoutLabel: view.inspectorLayoutLabel,
      inspectorLayoutOptions: view.inspectorLayoutOptions,
      loadableFlowOptions,
      profileOptions,
      profileChoices: profileOptions.filter((option) => option.value),
      mobBackendOptions: mobBackendDefaultOptions(contract, mobSettings.backendDefault || ""),
      surfaceOptions: deploySurfaceOptions(contract, deploySettings.surface || ""),
      trustOptions: trustPolicyOptions(contract, deploySettings.trustPolicy || ""),
      realmBackendOptions: realmBackendOptions(contract, deploySettings.realmBackend || ""),
      modelOptions,
    };
  }

  function schemaFieldTypeOptions(contract, currentType) {
    return simpleContractOptions(
      contract?.mob_definition?.editor_schema_field_types,
      currentType || contractDefaultValue(contract, "schema_field_type"),
      {
        string: "string",
        "string[]": "string[] — list",
        number: "number",
        float: "float",
        int: "int",
        integer: "integer",
        boolean: "boolean",
        bool: "bool",
        enum: "enum — fixed choices",
        bytes: "bytes — binary blob",
        object: "object — nested",
      },
      "mob_definition.editor_schema_field_types"
    );
  }

  function conditionOperatorOptions(contract, currentOperator) {
    return simpleContractOptions(
      contract?.mob_definition?.condition_operators,
      currentOperator || contractDefaultValue(contract, "condition_operator"),
      { "==": "==", ">": ">", "<": "<" },
      "mob_definition.condition_operators"
    );
  }

  function forkContextOptions(contract, currentContext, launchView = null) {
    const view = launchViewForState(launchView);
    const contractValues = Array.isArray(contract?.mob_definition?.fork_contexts)
      ? contract.mob_definition.fork_contexts.map((value) => normalizeForkContext(value)).filter(Boolean)
      : [];
    const options = contractValues.length ? [...contractValues] : [];
    const currentSource = currentContext || contractDefaultValue(contract, "fork_context");
    const current = currentSource ? normalizeForkContext(currentSource) : "";
    if (current && !options.includes(current)) options.push(current);
    return options.map((value) => {
      const supported = contractValues.includes(value);
      return {
        value,
        label: launchOptionLabel(view.forkContextLabels, value, view, view.forkContextsContractLabel),
        disabled: !supported,
        reason: supported ? "" : launchUnsupportedReason(view, view.forkContextsContractLabel),
      };
    });
  }

  function graphGateKindOptions(contract, currentKind, graphView = null) {
    const view = graphCanvasViewState(graphView);
    return simpleContractOptions(
      contract?.mob_definition?.graph_gate_kinds,
      currentKind || contractDefaultValue(contract, "graph_gate_kind"),
      view.gateKindLabels,
      "mob_definition.graph_gate_kinds"
    );
  }

  function graphTerminalKindOptions(contract, currentKind, graphView = null) {
    const view = graphCanvasViewState(graphView);
    return simpleContractOptions(
      contract?.mob_definition?.graph_terminal_kinds,
      currentKind || contractDefaultValue(contract, "graph_terminal_kind"),
      view.terminalKindLabels,
      "mob_definition.graph_terminal_kinds"
    );
  }

  function graphFrameKindOptions(contract, currentKind, graphView = null) {
    const view = graphCanvasViewState(graphView);
    return simpleContractOptions(
      contract?.mob_definition?.graph_frame_kinds,
      currentKind || contractDefaultValue(contract, "graph_frame_kind"),
      view.frameKindLabels,
      "mob_definition.graph_frame_kinds"
    );
  }

  function graphEdgeKindOptions(contract, currentKind, graphView = null) {
    const view = graphCanvasViewState(graphView);
    return simpleContractOptions(
      contract?.mob_definition?.graph_edge_kinds,
      currentKind || contractDefaultValue(contract, "graph_edge_kind"),
      view.edgeKindLabels,
      "mob_definition.graph_edge_kinds"
    );
  }

  function repeatIterationInputOptions(contract, currentMode) {
    return simpleContractOptions(
      contract?.mob_definition?.repeat_iteration_inputs,
      currentMode || contractDefaultValue(contract, "repeat_iteration_input"),
      {
        carry: "Carry — last body step's output feeds the next pass",
      },
      "mob_definition.repeat_iteration_inputs"
    );
  }

  function editorFlowPrimitiveOptions(contract, basicView = null) {
    const view = basicEditorViewState(basicView);
    const stepTypes = Array.isArray(contract?.mob_definition?.editor_flow_step_types) && contract.mob_definition.editor_flow_step_types.length
      ? contract.mob_definition.editor_flow_step_types.map(String)
      : [];
    const metadata = Object.fromEntries((view.flowPrimitiveRows || []).map((row) => [row.id, row]));
    return stepTypes
      .filter((type) => metadata[type])
      .map((type) => metadata[type]);
  }

  function graphControlNodes(contract, graphView = null) {
    const view = graphCanvasViewState(graphView);
    const metadata = Object.fromEntries((view.gatePaletteRows || []).map((row) => [row.id, row]));
    const paletteKinds = Array.isArray(contract?.mob_definition?.graph_palette_gate_kinds)
      ? contract.mob_definition.graph_palette_gate_kinds.map(String)
      : [];
    return graphGateKindOptions(contract, "")
      .filter((option) => !option.disabled && paletteKinds.includes(option.value) && metadata[option.value])
      .map((option) => ({
        id: option.value,
        gateKind: option.value,
        glyph: metadata[option.value].glyph,
        label: metadata[option.value].label,
        meta: metadata[option.value].meta,
      }));
  }

  function graphAddNodeMenuState({ members = [], contract = null, query = "", graphView = null } = {}) {
    const view = graphCanvasViewState(graphView);
    const q = String(query || "");
    const ql = q.trim().toLowerCase();
    const memberRows = (Array.isArray(members) ? members : [])
      .filter((member) => {
        if (!ql) return true;
        return [
          member?.name,
          member?.role,
          member?.model,
        ].map((part) => String(part || "")).join(" ").toLowerCase().includes(ql);
      })
      .map((member) => ({
        id: String(member.id || ""),
        role: String(member.role || ""),
        name: String(member.name || ""),
        model: String(member.model || ""),
        pick: { kind: "memberInstance", memberId: member.id },
      }))
      .filter((row) => row.id);
    const controls = (Array.isArray(members) && members.length)
      ? graphControlNodes(contract, graphView)
      : [];
    const controlRows = controls
      .filter((node) => {
        if (!ql) return true;
        return [
          node?.label,
          node?.meta,
          node?.gateKind,
        ].map((part) => String(part || "")).join(" ").toLowerCase().includes(ql);
      })
      .map((node) => ({
        id: String(node.id || ""),
        gateKind: String(node.gateKind || ""),
        glyph: String(node.glyph || ""),
        label: String(node.label || ""),
        meta: String(node.meta || ""),
        pick: { kind: "gate", gateKind: node.gateKind },
      }))
      .filter((row) => row.id);
    return {
      searchIcon: view.addNodeSearchIcon,
      searchPlaceholder: view.addNodeSearchPlaceholder,
      closeLabel: view.addNodeCloseLabel,
      closeTitle: view.addNodeCloseTitle,
      agentsLabel: view.addNodeAgentsLabel,
      controlsLabel: view.addNodeControlsLabel,
      emptyLabel: `${view.addNodeEmptyPrefix}${q}${view.addNodeEmptySuffix}`,
      jumpLabel: view.addNodeJumpLabel,
      memberRows,
      controlRows,
      hasMembers: memberRows.length > 0,
      hasControls: controlRows.length > 0,
      isEmpty: memberRows.length === 0 && controlRows.length === 0,
    };
  }

  function graphAddMenuOpenProjection({ col, row, grid } = {}) {
    const cell = graphCellXY(grid, col, row);
    return {
      addAt: {
        col,
        row,
        x: cell.x + Number(grid?.cellW || 0) * 0.5 - 130,
        y: 90,
      },
    };
  }

  function graphAddMenuCloseProjection() {
    return { addAt: null };
  }

  function basicStepPickerState({ members = [], contract = null, query = "", isKickoff = false, basicView = null } = {}) {
    const view = basicEditorViewState(basicView);
    if (isKickoff) {
      return {
        mode: "kickoff",
        title: view.pickerKickoffTitle,
        sub: view.pickerKickoffSub,
        kickoffHint: view.pickerKickoffHint,
      };
    }
    const q = String(query || "");
    const ql = q.trim().toLowerCase();
    const memberRows = (Array.isArray(members) ? members : [])
      .filter((member) => {
        if (!ql) return true;
        return [
          member?.name,
          member?.role,
        ].map((part) => String(part || "")).join(" ").toLowerCase().includes(ql);
      })
      .map((member) => ({
        id: String(member.id || ""),
        name: String(member.name || ""),
        role: String(member.role || ""),
        model: String(member.model || ""),
        schema: String(member.schema || ""),
        icon: "◆",
        iconTint: "accent",
        sub: [
          member?.role,
          member?.model,
          member?.schema,
        ].map((part) => String(part || "").trim()).filter(Boolean).join(" · "),
        pick: { kind: "member", id: member.id },
      }))
      .filter((row) => row.id);
    const primitiveRows = editorFlowPrimitiveOptions(contract, basicView)
      .filter((primitive) => {
        if (!ql) return true;
        return [
          primitive?.label,
          primitive?.sub,
        ].map((part) => String(part || "")).join(" ").toLowerCase().includes(ql);
      })
      .map((primitive) => ({
        id: String(primitive.id || ""),
        glyph: String(primitive.glyph || ""),
        tint: String(primitive.tint || ""),
        label: String(primitive.label || ""),
        sub: String(primitive.sub || ""),
        isNew: Boolean(primitive.isNew),
        pick: { kind: primitive.id },
      }))
      .filter((row) => row.id);
    return {
      mode: "picker",
      title: view.pickerTitle,
      sub: view.pickerSub,
      searchIcon: view.pickerSearchIcon,
      searchPlaceholder: view.pickerSearchPlaceholder,
      membersLabel: view.pickerMembersLabel,
      flowLabel: view.pickerFlowLabel,
      emptyMembersHint: view.pickerEmptyMembersHint,
      newBadgeLabel: view.pickerNewBadgeLabel,
      memberRows,
      primitiveRows,
      hasConfiguredMembers: Array.isArray(members) && members.length > 0,
    };
  }

  function firstSupportedOption(options, preferred = []) {
    const list = Array.isArray(options) ? options : [];
    for (const value of preferred) {
      const option = list.find((candidate) => candidate.value === value && !candidate.disabled);
      if (option) return option.value;
    }
    return list.find((option) => !option.disabled)?.value || "";
  }

  function contractStringValues(values) {
    return Array.isArray(values)
      ? values.map((value) => String(value || "").trim()).filter(Boolean)
      : [];
  }

  function firstContractValue(values, preferred = []) {
    const list = contractStringValues(values);
    for (const value of preferred) {
      if (list.includes(value)) return value;
    }
    return list[0] || "";
  }

  function contractDefaultRaw(contract, name) {
    return String(contract?.mob_definition?.defaults?.[name] || "").trim();
  }

  function contractDefaultFromList(contract, name, values, normalizer) {
    const raw = contractDefaultRaw(contract, name);
    if (!raw) return "";
    const normalized = normalizer ? normalizer(raw) : raw;
    const allowed = new Set(contractStringValues(values).map((value) => normalizer ? normalizer(value) : value));
    return allowed.has(normalized) ? normalized : "";
  }

  function contractDefaultValue(contract, name) {
    const mob = contract?.mob_definition || {};
    switch (name) {
      case "launch_mode":
        return contractDefaultFromList(contract, "launch_mode", mob.launch_modes, canonicalLaunchModeKind);
      case "dispatch_mode":
        return contractDefaultFromList(contract, "dispatch_mode", mob.dispatch_modes);
      case "collection_policy":
        return contractDefaultFromList(contract, "collection_policy", mob.collection_policies);
      case "dependency_mode":
        return contractDefaultFromList(contract, "dependency_mode", mob.dependency_modes);
      case "condition_operator":
        return contractDefaultFromList(contract, "condition_operator", mob.condition_operators);
      case "fork_context":
        return contractDefaultFromList(contract, "fork_context", mob.fork_contexts, normalizeForkContext);
      case "budget_split_policy":
        return contractDefaultFromList(contract, "budget_split_policy", mob.budget_split_policies, canonicalBudgetSplitPolicyKind);
      case "graph_gate_kind":
        return contractDefaultFromList(contract, "graph_gate_kind", mob.graph_gate_kinds);
      case "graph_edge_kind":
        return contractDefaultFromList(contract, "graph_edge_kind", mob.graph_edge_kinds);
      case "graph_condition_edge_kind":
        return contractDefaultFromList(contract, "graph_condition_edge_kind", mob.graph_edge_kinds);
      case "graph_fanout_edge_kind":
        return contractDefaultFromList(contract, "graph_fanout_edge_kind", mob.graph_edge_kinds);
      case "graph_terminal_kind":
        return contractDefaultFromList(contract, "graph_terminal_kind", mob.graph_terminal_kinds);
      case "schema_field_type":
        return contractDefaultFromList(contract, "schema_field_type", mob.editor_schema_field_types);
      case "branch_param_type":
        return contractDefaultFromList(contract, "branch_param_type", mob.editor_schema_field_types);
      case "repeat_iteration_input":
        return contractDefaultFromList(contract, "repeat_iteration_input", mob.repeat_iteration_inputs);
      case "step_output_format":
        return contractDefaultFromList(contract, "step_output_format", mob.step_output_formats);
      case "runtime_mode":
        return contractDefaultFromList(contract, "runtime_mode", mob.runtime_modes);
      default:
        return "";
    }
  }

  function editorSchemaDraftField(rawField) {
    if (!rawField || typeof rawField !== "object") return null;
    const name = schemaFieldName(rawField.name, "");
    if (!name) return null;
    return {
      name,
      required: rawField.required === true,
      description: String(rawField.description || ""),
      enumValues: Array.isArray(rawField.enumValues)
        ? rawField.enumValues.map((value) => String(value || "").trim()).filter(Boolean)
        : [],
    };
  }

  function editorSchemaDraftContract(contract) {
    const draft = contract?.mob_definition?.editor_schema_draft;
    if (!draft || typeof draft !== "object") return null;
    const schemaIdPrefix = String(draft.schema_id_prefix || "").trim();
    const schemaFieldType = contractDefaultValue(contract, "schema_field_type");
    const initialField = editorSchemaDraftField(draft.initial_field);
    const addedField = editorSchemaDraftField(draft.added_field);
    if (!schemaIdPrefix || !schemaFieldType || !initialField || !addedField) return null;
    return { schemaIdPrefix, schemaFieldType, initialField, addedField };
  }

  function editorInputParamDraftContract(contract) {
    const draft = contract?.mob_definition?.editor_input_param_draft;
    if (!draft || typeof draft !== "object") return null;
    const schemaFieldType = contractDefaultValue(contract, "schema_field_type");
    const addedField = editorSchemaDraftField(draft.added_field);
    if (!schemaFieldType || !addedField) return null;
    return { schemaFieldType, addedField };
  }

  function editorInputStepDraftContract(contract) {
    const draft = contract?.mob_definition?.editor_input_step_draft;
    const step = draft?.default_step;
    if (!step || typeof step !== "object") return null;
    const idPrefix = String(step.id || "").trim();
    if (!idPrefix) return null;
    return {
      idPrefix,
      task: String(step.task || ""),
      fields: String(step.fields || ""),
      inputParams: Array.isArray(step.inputParams) ? JSON.parse(JSON.stringify(step.inputParams)) : [],
    };
  }

  function inputStepDraft(contract, flow) {
    const draft = editorInputStepDraftContract(contract);
    return {
      id: uniqueFlowStepId(draft?.idPrefix || "input", flow),
      type: "input",
      task: draft?.task || "",
      fields: draft?.fields || "",
      inputParams: Array.isArray(draft?.inputParams) ? JSON.parse(JSON.stringify(draft.inputParams)) : [],
    };
  }

  function editorSchemaFieldNameFallback(contract) {
    const draft = editorSchemaDraftContract(contract);
    return draft?.addedField?.name || draft?.initialField?.name || "field";
  }

  function editorInputParamNameFallback(contract) {
    return editorInputParamDraftContract(contract)?.addedField?.name || "param";
  }

  function editorGraphDraftContract(contract) {
    const draft = contract?.mob_definition?.editor_graph_draft;
    if (!draft || typeof draft !== "object") return null;
    const parallelLaneLabels = Array.isArray(draft.parallel_lane_labels)
      ? draft.parallel_lane_labels.map((label) => String(label || "").trim()).filter(Boolean)
      : [];
    const out = {
      branchGateLabel: String(draft.branch_gate_label || "").trim(),
      branchConditionLaneLabel: String(draft.branch_condition_lane_label || "").trim(),
      branchFallbackLaneLabel: String(draft.branch_fallback_lane_label || "").trim(),
      branchJoinLabel: String(draft.branch_join_label || "").trim(),
      fallbackEdgeLabel: String(draft.fallback_edge_label || "").trim(),
      parallelLaneLabels,
      parallelEdgeLabel: String(draft.parallel_edge_label || "").trim(),
      reworkEdgeLabel: String(draft.rework_edge_label || "").trim(),
      terminalEdgeLabelPrefix: String(draft.terminal_edge_label_prefix || ""),
      joinLabelPrefix: String(draft.join_label_prefix || ""),
      joinQuorumLabelPrefix: String(draft.join_quorum_label_prefix || ""),
      branchFrameLabelPrefix: String(draft.branch_frame_label_prefix || ""),
      branchFrameSingularSuffix: String(draft.branch_frame_singular_suffix || ""),
      branchFramePluralSuffix: String(draft.branch_frame_plural_suffix || ""),
      parallelFrameLabelPrefix: String(draft.parallel_frame_label_prefix || ""),
      parallelFrameJoinInfix: String(draft.parallel_frame_join_infix || ""),
      parallelMissingDispatchLabel: String(draft.parallel_missing_dispatch_label || "").trim(),
      parallelMissingCollectionLabel: String(draft.parallel_missing_collection_label || "").trim(),
      repeatFrameLabelPrefix: String(draft.repeat_frame_label_prefix || ""),
      repeatMaxIterationsPrefix: String(draft.repeat_max_iterations_prefix || ""),
      repeatMissingMaxIterationsLabel: String(draft.repeat_missing_max_iterations_label || "").trim(),
      repeatEdgeUntilPrefix: String(draft.repeat_edge_until_prefix || ""),
      repeatEdgeUntilFallback: String(draft.repeat_edge_until_fallback || "").trim(),
    };
    if (!out.branchGateLabel || !out.branchConditionLaneLabel || !out.branchFallbackLaneLabel
      || !out.branchJoinLabel || !out.fallbackEdgeLabel || out.parallelLaneLabels.length < 2
      || !out.parallelEdgeLabel || !out.reworkEdgeLabel || !out.terminalEdgeLabelPrefix
      || !out.joinLabelPrefix || !out.joinQuorumLabelPrefix || !out.branchFrameLabelPrefix || !out.branchFrameSingularSuffix
      || !out.branchFramePluralSuffix || !out.parallelFrameLabelPrefix || !out.parallelFrameJoinInfix
      || !out.parallelMissingDispatchLabel || !out.parallelMissingCollectionLabel
      || !out.repeatFrameLabelPrefix || !out.repeatMaxIterationsPrefix
      || !out.repeatMissingMaxIterationsLabel || !out.repeatEdgeUntilPrefix
      || !out.repeatEdgeUntilFallback) {
      return null;
    }
    return out;
  }

  function emptyGraphDraftContract() {
    return {
      branchGateLabel: "",
      branchConditionLaneLabel: "",
      branchFallbackLaneLabel: "",
      branchJoinLabel: "",
      fallbackEdgeLabel: "",
      parallelLaneLabels: [],
      parallelEdgeLabel: "",
      reworkEdgeLabel: "",
      terminalEdgeLabelPrefix: "",
      joinLabelPrefix: "",
      joinQuorumLabelPrefix: "",
      branchFrameLabelPrefix: "",
      branchFrameSingularSuffix: "",
      branchFramePluralSuffix: "",
      parallelFrameLabelPrefix: "",
      parallelFrameJoinInfix: "",
      parallelMissingDispatchLabel: "",
      parallelMissingCollectionLabel: "",
      repeatFrameLabelPrefix: "",
      repeatMaxIterationsPrefix: "",
      repeatMissingMaxIterationsLabel: "",
      repeatEdgeUntilPrefix: "",
      repeatEdgeUntilFallback: "",
    };
  }

  function graphControlShape({ gateKind, at, members, instances, edges, flow, contract, graphView = null } = {}) {
    const kind = String(gateKind || "").trim();
    if (kind !== "branch" && kind !== "fork") return null;
    const allowed = new Set(graphControlNodes(contract, graphView).map((node) => node.gateKind));
    if (!allowed.has(kind)) return null;
    const sourceMembers = Array.isArray(members) ? members : [];
    if (!at || sourceMembers.length === 0) return null;

    const launchKind = contractDefaultValue(contract, "launch_mode");
    const nextEdgeKind = contractDefaultValue(contract, "graph_edge_kind");
    const draft = editorGraphDraftContract(contract);
    if (!launchKind || !nextEdgeKind || !draft) return null;

    const sourceInstances = Array.isArray(instances) ? instances : [];
    const sourceEdges = Array.isArray(edges) ? edges : [];
    const cells = allocateGraphControlCells(sourceInstances, at);
    const suffix = uniqueGraphControlSuffix(kind, sourceInstances, sourceEdges);
    const memberA = sourceMembers[0];
    const memberB = sourceMembers[1] || sourceMembers[0];
    const isBranch = kind === "branch";
    const gateId = isBranch ? `g_branch_${suffix}` : `g_parallel_${suffix}`;
    const leftId = `${gateId}_a`;
    const rightId = `${gateId}_b`;
    const joinId = isBranch ? `j_branch_${suffix}` : `j_parallel_${suffix}`;
    const collection = isBranch ? "any" : contractDefaultValue(contract, "collection_policy");
    if (!collection) return null;
    const dispatch = isBranch ? "" : contractDefaultValue(contract, "dispatch_mode");
    if (!isBranch && !dispatch) return null;

    const instancesOut = [
      {
        id: gateId,
        isGate: true,
        gateKind: kind,
        label: isBranch ? draft.branchGateLabel : dispatch,
        dispatch: isBranch ? undefined : dispatch,
        col: cells.gate.col,
        row: cells.gate.row,
      },
      {
        id: leftId,
        memberId: memberA.id,
        col: cells.laneA.col,
        row: cells.laneA.row,
        lane: isBranch ? draft.branchConditionLaneLabel : draft.parallelLaneLabels[0],
        launchMode: { kind: launchKind },
      },
      {
        id: rightId,
        memberId: memberB.id,
        col: cells.laneB.col,
        row: cells.laneB.row,
        lane: isBranch ? draft.branchFallbackLaneLabel : draft.parallelLaneLabels[1],
        launchMode: { kind: launchKind },
      },
      {
        id: joinId,
        isGate: true,
        gateKind: "join",
        label: isBranch ? draft.branchJoinLabel : `${draft.joinLabelPrefix}${collection}`,
        collection,
        controllerRole: isBranch ? memberA.id : "",
        col: cells.join.col,
        row: cells.join.row,
      },
    ];

    let edgesOut;
    if (isBranch) {
      const condEdgeKind = contractDefaultValue(contract, "graph_condition_edge_kind");
      if (!condEdgeKind) return null;
      edgesOut = [
        {
          id: `e_${gateId}_${leftId}`,
          from: gateId,
          to: leftId,
          kind: condEdgeKind,
          label: "",
          cond: null,
        },
        { id: `e_${gateId}_${rightId}`, from: gateId, to: rightId, kind: nextEdgeKind, label: draft.fallbackEdgeLabel },
        { id: `e_${leftId}_${joinId}`, from: leftId, to: joinId, kind: nextEdgeKind, label: "" },
        { id: `e_${rightId}_${joinId}`, from: rightId, to: joinId, kind: nextEdgeKind, label: "" },
      ];
    } else {
      const fanoutEdgeKind = contractDefaultValue(contract, "graph_fanout_edge_kind");
      if (!fanoutEdgeKind) return null;
      edgesOut = [
        { id: `e_${gateId}_${leftId}`, from: gateId, to: leftId, kind: fanoutEdgeKind, label: "" },
        { id: `e_${gateId}_${rightId}`, from: gateId, to: rightId, kind: fanoutEdgeKind, label: "" },
        { id: `e_${leftId}_${joinId}`, from: leftId, to: joinId, kind: nextEdgeKind, label: "" },
        { id: `e_${rightId}_${joinId}`, from: rightId, to: joinId, kind: nextEdgeKind, label: "" },
      ];
    }

    return {
      selectId: gateId,
      flow,
      instances: instancesOut,
      edges: edgesOut,
    };
  }

  function graphMemberInstanceShape({ memberId, at, instances, contract } = {}) {
    const id = String(memberId || "").trim();
    if (!id || !at) return null;
    const launchKind = contractDefaultValue(contract, "launch_mode");
    if (!launchKind) return null;
    return {
      id: uniqueGraphInstanceId(`i_${slug(id, "member")}`, instances),
      memberId: id,
      col: at.col,
      row: at.row,
      launchMode: { kind: launchKind },
      lane: "",
    };
  }

  function graphQuickInsertResult(result = {}) {
    return {
      ok: false,
      error: "",
      flow: result.flow,
      instances: Array.isArray(result.instances) ? result.instances : [],
      edges: Array.isArray(result.edges) ? result.edges : [],
      selectId: "",
      snap: false,
      addAt: null,
      ...result,
    };
  }

  function graphQuickInsertProjection({ pick, at, members, instances, edges, flow, contract, graphView = null } = {}) {
    const sourceInstances = Array.isArray(instances) ? instances : [];
    const sourceEdges = Array.isArray(edges) ? edges : [];
    const sourceFlow = flow;
    const kind = String(pick?.kind || "").trim();
    if (!pick || !at) {
      return graphQuickInsertResult({ flow: sourceFlow, instances: sourceInstances, edges: sourceEdges });
    }
    if (kind === "memberInstance") {
      const instance = graphMemberInstanceShape({
        memberId: pick.memberId,
        at,
        instances: sourceInstances,
        contract,
      });
      const next = studioAddInstancePatch({ instances: sourceInstances, members }, instance);
      if (!next.ok) {
        return graphQuickInsertResult({ error: next.error, flow: sourceFlow, instances: sourceInstances, edges: sourceEdges });
      }
      return graphQuickInsertResult({ ok: true, flow: sourceFlow, instances: next.instances, edges: sourceEdges, selectId: next.instance?.id || "", snap: true });
    }
    if (kind === "gate") {
      const inserted = graphControlShape({
        gateKind: pick.gateKind,
        at,
        members,
        instances: sourceInstances,
        edges: sourceEdges,
        flow: sourceFlow,
        contract,
        graphView,
      });
      if (!inserted) {
        return graphQuickInsertResult({ flow: sourceFlow, instances: sourceInstances, edges: sourceEdges });
      }
      const instancesPatch = studioAppendInstancesPatch({ instances: sourceInstances, members }, inserted.instances);
      const edgesPatch = studioAppendEdgesPatch({ edges: sourceEdges, instances: instancesPatch.instances }, inserted.edges);
      return graphQuickInsertResult({
        ok: true,
        flow: inserted.flow || sourceFlow,
        instances: instancesPatch.instances,
        edges: edgesPatch.edges,
        selectId: inserted.selectId || "",
        snap: true,
      });
    }
    return graphQuickInsertResult({ flow: sourceFlow, instances: sourceInstances, edges: sourceEdges });
  }

  function agentNavigationProjection(memberId = null) {
    const id = String(memberId || "").trim();
    return {
      view: "agents",
      addAt: null,
      selection: id ? { kind: "agent", id } : null,
    };
  }

  function flowStepTemplate(pick, contract, options = {}) {
    const kind = String(pick?.kind || "").trim();
    const id = uniqueFlowStepId("s", options.flow);
    const branchIds = collectFlowBranchIds(options.flow?.steps || []);
    const dependencyMode = contractDefaultValue(contract, "dependency_mode");
    if (!dependencyMode) return null;
    const stepTypes = contractStringValues(contract?.mob_definition?.editor_flow_step_types);
    if (kind === "member") {
      return {
        id,
        type: "member",
        role: String(pick?.id || "").trim(),
        instruction: "",
        dependsMode: dependencyMode,
      };
    }
    if (!stepTypes.includes(kind)) return null;
    if (kind === "branch") {
      return {
        id,
        type: "branch",
        controllerRole: "",
        branches: [{ id: reserveFlowBranchId("br", branchIds), label: basicBranchDefaultLabel(1, options.basicView), condition: "", steps: [] }],
        fallback: [],
        dependsMode: dependencyMode,
      };
    }
    if (kind === "parallel") {
      const dispatch = contractDefaultValue(contract, "dispatch_mode");
      const collection = contractDefaultValue(contract, "collection_policy");
      if (!dispatch || !collection) return null;
      return {
        id,
        type: "parallel",
        controllerRole: "",
        dispatch,
        collection,
        branches: [
          { id: reserveFlowBranchId("br", branchIds), label: basicBranchDefaultLabel(1, options.basicView), steps: [] },
          { id: reserveFlowBranchId("br", branchIds), label: basicBranchDefaultLabel(2, options.basicView), steps: [] },
        ],
        dependsMode: dependencyMode,
      };
    }
    if (kind === "repeat") {
      return { id, type: "repeat", loopId: "", until: "", maxIterations: null, iterationInput: "", steps: [] };
    }
    return null;
  }

  function uniqueFlowStepId(prefix, flow) {
    const stem = slug(prefix, "s");
    const base = `${stem}_1`;
    const used = collectFlowStepIds(flow?.steps || []);
    if (!used.has(base)) return base;
    let index = 2;
    while (used.has(`${stem}_${index}`)) index += 1;
    return `${stem}_${index}`;
  }

  function reserveFlowBranchId(prefix, used) {
    const stem = slug(prefix, "br");
    const base = `${stem}_1`;
    const ids = used instanceof Set ? used : new Set();
    if (!ids.has(base)) {
      ids.add(base);
      return base;
    }
    let index = 2;
    while (ids.has(`${stem}_${index}`)) index += 1;
    const id = `${stem}_${index}`;
    ids.add(id);
    return id;
  }

  function collectFlowBranchIds(steps, out = new Set()) {
    for (const step of steps || []) {
      if (step?.type === "branch" || step?.type === "parallel") {
        for (const branch of step.branches || []) {
          const id = String(branch?.id || "").trim();
          if (id) out.add(id);
          collectFlowBranchIds(branch?.steps || [], out);
        }
      }
      if (step?.type === "branch") collectFlowBranchIds(step.fallback || [], out);
      if (step?.type === "repeat") collectFlowBranchIds(step.steps || [], out);
    }
    return out;
  }

  function uniqueGraphControlSuffix(kind, instances = [], edges = []) {
    const prefix = kind === "branch" ? "branch" : "parallel";
    const instanceIds = graphInstanceIdSet(instances);
    const edgeIds = graphEdgeIdSet(edges);
    let index = 1;
    while (true) {
      const suffix = String(index);
      const gateId = `g_${prefix}_${suffix}`;
      const leftId = `${gateId}_a`;
      const rightId = `${gateId}_b`;
      const joinId = `j_${prefix}_${suffix}`;
      const nodeIds = [gateId, leftId, rightId, joinId];
      const proposedEdgeIds = [
        `e_${gateId}_${leftId}`,
        `e_${gateId}_${rightId}`,
        `e_${leftId}_${joinId}`,
        `e_${rightId}_${joinId}`,
      ];
      if (
        nodeIds.every((id) => !instanceIds.has(id))
        && proposedEdgeIds.every((id) => !edgeIds.has(id))
      ) {
        return suffix;
      }
      index += 1;
    }
  }

  function uniqueGraphInstanceId(prefix, instances = []) {
    const base = slug(prefix, "i_member");
    const withPrefix = base.startsWith("i_") ? base : `i_${base}`;
    const used = graphInstanceIdSet(instances);
    if (!used.has(withPrefix)) return withPrefix;
    let index = 2;
    while (used.has(`${withPrefix}_${index}`)) index += 1;
    return `${withPrefix}_${index}`;
  }

  function graphInstanceIdSet(instances = []) {
    return new Set((Array.isArray(instances) ? instances : [])
      .map((instance) => String(instance?.id || "").trim())
      .filter(Boolean));
  }

  function graphEdgeIdSet(edges = []) {
    return new Set((Array.isArray(edges) ? edges : [])
      .map((edge) => String(edge?.id || "").trim())
      .filter(Boolean));
  }

  function uniqueGraphEdgeId(fromId, toId, edges = []) {
    const base = `e_${slug(fromId, "from")}_${slug(toId, "to")}`;
    const used = graphEdgeIdSet(edges);
    if (!used.has(base)) return base;
    let index = 2;
    while (used.has(`${base}_${index}`)) index += 1;
    return `${base}_${index}`;
  }

  function allocateGraphControlCells(instances, at) {
    const occupied = new Set((instances || []).map(inst => `${inst.col}:${inst.row}`));
    for (let row = at.row; row < at.row + 24; row += 1) {
      const candidate = {
        gate: { col: at.col, row },
        laneA: { col: at.col + 1, row },
        laneB: { col: at.col + 1, row: row + 1 },
        join: { col: at.col + 2, row },
      };
      if (Object.values(candidate).every(cell => !occupied.has(`${cell.col}:${cell.row}`))) {
        return candidate;
      }
    }
    return {
      gate: { col: at.col, row: at.row },
      laneA: { col: at.col + 1, row: at.row },
      laneB: { col: at.col + 1, row: at.row + 1 },
      join: { col: at.col + 2, row: at.row },
    };
  }

  function outputFormatOptions(contract, currentFormat) {
    return simpleContractOptions(
      contract?.mob_definition?.step_output_formats,
      currentFormat || contractDefaultValue(contract, "step_output_format"),
      {
        json: "json — parse terminal output as JSON",
        text: "text — preserve terminal text",
      },
      "mob_definition.step_output_formats"
    );
  }

  function outputFormatAllowed(contract, format) {
    const value = normalizeOutputFormat(format);
    if (!value) return true;
    const formats = Array.isArray(contract?.mob_definition?.step_output_formats)
      ? contract.mob_definition.step_output_formats.map(normalizeOutputFormat)
      : [];
    return formats.includes(value);
  }

  function normalizeMobSettings(settings) {
    const source = settings && typeof settings === "object" ? settings : {};
    const advancedSource = source.advanced && typeof source.advanced === "object" ? source.advanced : {};
    const roleWiring = normalizeRoleWiring(source.roleWiring || source.role_wiring);
    return {
      ...EMPTY_MOB_SETTINGS,
      orchestrator: String(source.orchestrator || source.orchestratorProfile || source.orchestrator_profile || "").trim(),
      autoWireOrchestrator: !!(source.autoWireOrchestrator ?? source.auto_wire_orchestrator),
      roleWiring,
      backendDefault: normalizeProfileBackend(source.backendDefault || source.backend_default || source.backend?.default) || "",
      externalAddressBase: String(source.externalAddressBase || source.external_address_base || source.backend?.external?.address_base || "").trim(),
      advanced: {
        topology: normalizeOptionalObject(advancedSource.topology || source.topology),
        supervisor: normalizeOptionalObject(advancedSource.supervisor || source.supervisor),
        limits: normalizeOptionalObject(advancedSource.limits || source.limits),
        spawnPolicy: normalizeOptionalObject(advancedSource.spawnPolicy || advancedSource.spawn_policy || source.spawnPolicy || source.spawn_policy),
        eventRouter: normalizeOptionalObject(advancedSource.eventRouter || advancedSource.event_router || source.eventRouter || source.event_router),
      },
    };
  }

  function normalizeRoleWiring(value) {
    if (!Array.isArray(value)) return [];
    return value
      .map((rule) => ({
        a: String(rule?.a || "").trim(),
        b: String(rule?.b || "").trim(),
      }))
      .filter((rule) => rule.a && rule.b);
  }

  function mobRoleWiringEditorState(value, profileOptions, settingsView = null) {
    const view = settingsViewForState(settingsView);
    const options = Array.isArray(profileOptions) ? profileOptions : [];
    const wiring = normalizeRoleWiring(value);
    return {
      label: view.roleWiringLabel,
      countLabel: String(wiring.length),
      addLabel: view.roleWiringAddLabel,
      addDisabled: !options.length,
      options,
      wiring,
    };
  }

  function roleWiringOptionValues(profileOptions) {
    return (Array.isArray(profileOptions) ? profileOptions : [])
      .map((option) => String(option?.value || option || "").trim())
      .filter(Boolean);
  }

  function normalizeRoleWiringForOptions(wiring, profileOptions) {
    const allowed = new Set(roleWiringOptionValues(profileOptions));
    if (!allowed.size) return [];
    return normalizeRoleWiring(wiring).filter((rule) => allowed.has(rule.a) && allowed.has(rule.b));
  }

  function mobRoleWiringUpdatePatch(wiring, index, patch, profileOptions) {
    const rules = normalizeRoleWiring(wiring);
    const ruleIndex = Number(index);
    if (!Number.isInteger(ruleIndex) || ruleIndex < 0 || ruleIndex >= rules.length) return rules;
    return normalizeRoleWiringForOptions(
      rules.map((rule, i) => i === ruleIndex ? { ...rule, ...(patch || {}) } : rule),
      profileOptions,
    );
  }

  function mobRoleWiringSourcePatch(wiring, index, rawValue, profileOptions) {
    return mobRoleWiringUpdatePatch(wiring, index, { a: String(rawValue || "").trim() }, profileOptions);
  }

  function mobRoleWiringTargetPatch(wiring, index, rawValue, profileOptions) {
    return mobRoleWiringUpdatePatch(wiring, index, { b: String(rawValue || "").trim() }, profileOptions);
  }

  function mobRoleWiringDeletePatch(wiring, index) {
    const rules = normalizeRoleWiring(wiring);
    const ruleIndex = Number(index);
    if (!Number.isInteger(ruleIndex) || ruleIndex < 0 || ruleIndex >= rules.length) return rules;
    return rules.filter((_, i) => i !== ruleIndex);
  }

  function mobRoleWiringAddPatch(wiring, profileOptions) {
    const rules = normalizeRoleWiring(wiring);
    const options = roleWiringOptionValues(profileOptions);
    if (!options.length) return rules;
    return normalizeRoleWiring([
      ...rules,
      { a: options[0], b: options[1] || options[0] },
    ]);
  }

  function advancedMobSettingsEditorState(value, settingsView = null) {
    const view = settingsViewForState(settingsView);
    return {
      label: view.advancedLabel,
      text: JSON.stringify(value || {}, null, 2),
    };
  }

  function advancedMobSettingsDraftPatch(text, settingsView = null) {
    const view = settingsViewForState(settingsView);
    try {
      const parsed = String(text || "").trim() ? JSON.parse(String(text)) : {};
      if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
        return { ok: false, error: view.advancedObjectRequiredError, value: null };
      }
      return { ok: true, error: "", value: normalizeMobSettings({ advanced: parsed }).advanced };
    } catch (err) {
      return { ok: false, error: err?.message || view.advancedInvalidJsonError, value: null };
    }
  }

  function mobSettingsForUi(settings) {
    return normalizeMobSettings(settings);
  }

  function mobDefaultsFromSchema(schema) {
    return mobSettingsForUi(schema?.mob_definition?.mob_settings?.defaults);
  }

  function normalizeOptionalObject(value) {
    if (!value || typeof value !== "object" || Array.isArray(value)) return null;
    return JSON.parse(JSON.stringify(value));
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
    return {
      rows,
      counts,
      eyebrow: view.validationEyebrow,
      title: `${counts.ok} ${view.validationPassedLabel} · ${counts.warn} ${view.validationWarningsLabel} · ${counts.crit} ${view.validationBlockingLabel}`,
      publishLabel: view.publishLabel,
      deployPlanLabel: view.deployPlanLabel,
      deployLabel: view.deployLabel,
      closeLabel: view.closeLabel,
      actionsDisabled: counts.crit > 0 || stageBlocksActions,
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

  function topRailState({ contract, deploySettings, stage, view, theme, deployView } = {}) {
    const shell = deployViewForState(deployView);
    const inEditor = view === "editor";
    const contractState = contract?.error ? shell.apiErrorLabel : contract ? shell.apiReadyLabel : shell.apiLoadingLabel;
    const deployCommand = contract?.deploy_settings?.command || "";
    const deploySurface = deploySettings?.surface || contract?.deploy_settings?.surfaces?.[0] || "";
    const deployActionsDisabled = stage !== "valid";
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
      deployActionsDisabled,
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
    const deployOk = !options.execute || result?.success !== false;
    return {
      document,
      deployResult: result || null,
      validation,
      validationRows: deployResultToRows(result),
      stage: validation?.ok && deployOk ? "valid" : "draft",
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
      drySim: true,
      drySimDocument: document || null,
      drySimPlan: plan || null,
      incrementDrySimKey: true,
    };
  }

  function deployPlanTraceCloseTransition() {
    return { drySim: false };
  }

  function apiOverlayClearTransition() {
    return {
      drySim: false,
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
    const files = Array.isArray(result?.source_files) ? result.source_files : [];
    if (!files.length) throw new Error(`${apiSource} did not return source_files`);
    const mobTomlFile = files.find((file) => String(file?.path || "") === "mobkit/mob.toml");
    if (!mobTomlFile) throw new Error(`${apiSource} did not return mobkit/mob.toml source file`);
    const exportedToml = String(mobTomlFile.text || "").trim();
    if (!exportedToml) throw new Error(`${apiSource} did not return mobkit/mob.toml text`);
    const filename = String(result?.filename || "").trim();
    if (!filename) throw new Error(`${apiSource} did not return filename`);
    const mediaType = String(result?.media_type || "").trim();
    if (!mediaType) throw new Error(`${apiSource} did not return media_type`);
    const sourceDigest = String(mobTomlFile.sha256 || "").trim();
    if (!sourceDigest) throw new Error(`${apiSource} did not return mobkit/mob.toml sha256`);
    files.forEach((file, index) => validateSourceFileMetadata(apiSource, file, index));
    const sourceView = sourceViewForState(null, options.sourceView);
    const renderedDocument = {
      ...(document && typeof document === "object" ? document : {}),
      mob_toml: mobTomlFile.text,
    };
    const validation = result?.validation || null;
    const stage = validation?.ok ? "valid" : "draft";
    return {
      document: renderedDocument,
      sourceDocument: {
        ...renderedDocument,
        validation,
        filename,
        media_type: mediaType,
        sourcePath: mobTomlFile.path,
        sourceFile: mobTomlFile,
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

  function sourceFileForPath(sourceDocument, path) {
    const files = Array.isArray(sourceDocument?.sourceFiles) ? sourceDocument.sourceFiles : [];
    const selectedPath = String(path || sourceDocument?.sourcePath || "mobkit/mob.toml").trim();
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
    const source = selectedFile ? sourceFileContent(selectedFile) : String(sourceDocument?.mob_toml || "");
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
      sourceHtml: selectedFile ? highlightSourceFile(selectedFile) : highlightTomlSource(source),
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

  function escapeHtml(source) {
    return String(source || "")
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;");
  }

  function sourceViewFromSchema(schema) {
    const view = schema?.mob_definition?.editor_source_view;
    if (!view || typeof view !== "object") return null;
    const out = {
      drawerEyebrow: String(view.drawer_eyebrow || "").trim(),
      inlineTitle: String(view.inline_title || "").trim(),
      loadingText: String(view.loading_text || "").trim(),
      copyLabel: String(view.copy_label || "").trim(),
      closeLabel: String(view.close_label || "").trim(),
    };
    return out.drawerEyebrow && out.inlineTitle && out.loadingText && out.copyLabel && out.closeLabel
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
        };
      })
      .filter(Boolean);
  }

  function flowCatalogBootstrapState(catalogPayload, options = {}) {
    const sampleFlows = sampleFlowsFromCatalogs(catalogPayload);
    const first = sampleFlows[0] || null;
    return {
      templates: sampleFlows,
      flows: sampleFlows,
      initialHydration: first
        ? {
          result: {
            document: first.document,
            validation: first.validation,
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
      fallback: {
        currentFlowId: selectedId,
        stage: row.stage || "draft",
        view: "editor",
      },
    };
  }

  function flowRegistryFallbackOpenTransition(selection) {
    const fallback = selection?.fallback || null;
    if (!fallback) return null;
    return {
      currentFlowId: String(fallback.currentFlowId || ""),
      stage: fallback.stage || "draft",
      view: fallback.view || "editor",
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
    };
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

  function flowRegistryDocumentPersistence({
    currentFlowId,
    document,
    validation = null,
    stage = "draft",
    previousSignature = "",
    skipIfUnchanged = false,
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
      },
    };
  }

  function flowRegistryPersistDocumentProjection(rows, options = {}) {
    const sourceRows = Array.isArray(rows) ? rows : [];
    const persistence = flowRegistryDocumentPersistence(options);
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

  function cloneDocument(document, options = {}) {
    const next = JSON.parse(JSON.stringify(document || {}));
    const name = String(options.name || next.name || next.flow?.name || next.mob_id || "mobkit_flow").trim();
    if (name) {
      next.name = name;
      next.mob_id = slug(name, next.mob_id || "mobkit_flow");
      if (next.flow && typeof next.flow === "object") next.flow.name = name;
      delete next.mob_toml;
    }
    return next;
  }

  function createFlowDraftFromSpec({
    id,
    spec,
    templates,
    blankTemplate,
    deploySettings,
    mobSettings,
    existingRows,
  } = {}) {
    const rowId = String(id || flowDraftIdFromSpec(spec, existingRows)).trim();
    if (!rowId) return null;
    const draftSpec = spec && typeof spec === "object" ? spec : {};
    const template = draftSpec.template === "blank" && blankTemplate?.document
      ? blankTemplate
      : (Array.isArray(templates) ? templates : [])
        .find((candidate) => candidate?.id === draftSpec.template);
    const trigger = String(draftSpec.trigger || "");
    let source = "";
    let document;
    if (template?.document) {
      source = String(template.source || "");
      document = cloneDocument(template.document, {
        name: draftSpec.name || template.name,
      });
    } else {
      return null;
    }
    const row = flowRegistryRowFromDocument({
      id: rowId,
      document,
      stage: "draft",
      trigger,
      source,
      validation: null,
    });
    return { id: rowId, document, row, template: template || null };
  }

  function flowRegistryCreateDraftProjection(rows, options = {}) {
    const sourceRows = Array.isArray(rows) ? rows : [];
    const draft = createFlowDraftFromSpec({
      ...options,
      existingRows: options.existingRows || sourceRows,
    });
    if (!draft?.document || !draft?.row) {
      return {
        ok: false,
        draft: null,
        rows: sourceRows,
        hydration: null,
      };
    }
    return {
      ok: true,
      draft,
      rows: flowRegistryAppendRowPatch(sourceRows, draft.row),
      hydration: {
        result: { document: draft.document, validation: null },
        options: {
          id: draft.id,
          flowRow: draft.row,
          addToRegistry: false,
        },
      },
    };
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
          source: template.source || "",
          sourceMobpack: template.sourceMobpack || template.source_mobpack || "",
          sourceMobpackName: template.sourceMobpackName || template.source_mobpack_name || "",
          sourceOrigin: template.sourceOrigin || template.source_origin || "",
          sourceDocumentPath: template.sourceDocumentPath || template.source_document_path || "",
        };
      })
      .filter(Boolean);
  }

  function validateAgentDefinitionCatalogRefs(source, options = {}) {
    if (Array.isArray(options.modelCatalog)) {
      const modelIds = new Set(options.modelCatalog
        .map((model) => String(model?.id || "").trim())
        .filter(Boolean));
      const model = String(source?.model || "").trim();
      if (model && !modelIds.has(model)) {
        throw new Error(`MobKit agent definition references unavailable model: ${model}`);
      }
    }
    if (options.contract) {
      const profileBinding = String(source?.profileBinding || "").trim();
      if (!optionValueAllowed(profileBindingOptions(options.contract, profileBinding), profileBinding, { allowBlank: false })) {
        throw new Error(`MobKit agent definition references unsupported profile binding: ${profileBinding}`);
      }
      const runtimeMode = String(source?.runtimeMode || "").trim();
      if (!contractValueAllowed(options.contract?.mob_definition?.runtime_modes, runtimeMode, { allowBlank: false })
        || !optionValueAllowed(runtimeModeOptions(options.contract, options.deploySettings, runtimeMode), runtimeMode, { allowBlank: false })) {
        throw new Error(`MobKit agent definition references unsupported runtime mode: ${runtimeMode}`);
      }
      const backend = normalizeProfileBackend(source?.backend);
      if (backend && !optionValueAllowed(profileBackendOptions(options.contract, backend, false), backend, { allowBlank: false })) {
        throw new Error(`MobKit agent definition references unsupported backend: ${backend}`);
      }
    }
    if (Array.isArray(options.toolCatalog)) {
      const toolIds = new Set(options.toolCatalog
        .map((tool) => String(tool?.id || "").trim())
        .filter(Boolean));
      const missingTools = normalizeStringList(source?.tools)
        .filter((id) => !toolIds.has(id));
      if (missingTools.length) {
        throw new Error(`MobKit agent definition references unavailable tool(s): ${missingTools.join(", ")}`);
      }
    }
    if (Array.isArray(options.skillRealms)) {
      const skillIds = skillIdsFromRealms(options.skillRealms);
      const missingSkills = normalizeStringList(source?.skills)
        .filter((id) => !skillIds.has(id));
      if (missingSkills.length) {
        throw new Error(`MobKit agent definition references unavailable skill(s): ${missingSkills.join(", ")}`);
      }
    }
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

  function schemaDefinitionsFromAgentDefinition(definition) {
    const schema = normalizeAgentSchemaDefinition(definition?.schemaDefinition || definition?.schema_definition);
    return schema ? [schema] : [];
  }

  function mergeAgentDefinitionSchemas(existingSchemas, definition) {
    const schemas = Array.isArray(existingSchemas) ? existingSchemas : [];
    const incoming = schemaDefinitionsFromAgentDefinition(definition);
    if (!incoming.length) return schemas;

    let changed = false;
    const incomingById = new Map(incoming.map((schema) => [schema.id, schema]));
    const merged = schemas.map((schema) => {
      const replacement = incomingById.get(schema?.id);
      if (!replacement) return schema;
      incomingById.delete(schema.id);
      if (JSON.stringify(schema) === JSON.stringify(replacement)) return schema;
      changed = true;
      return replacement;
    });
    for (const schema of incomingById.values()) {
      changed = true;
      merged.push(schema);
    }
    return changed ? merged : schemas;
  }

  function validateAgentDefinitionSchemaRef(source, schemas = []) {
    const schemaId = String(source?.schema || "").trim();
    if (!schemaId) return;
    const available = new Set((Array.isArray(schemas) ? schemas : [])
      .map((schema) => String(schema?.id || "").trim())
      .filter(Boolean));
    if (!available.has(schemaId)) {
      throw new Error(`MobKit agent definition references unavailable schema: ${schemaId}`);
    }
  }

  function memberFromAgentDefinition(definition, existingMembers = [], options = {}) {
    const source = definition;
    if (!source) {
      throw new Error("MobKit agent definitions are unavailable; cannot create a member without the schema contract.");
    }
    if (source.definitionType !== "mobkit/profile-member") {
      throw new Error("MobKit agent definition is not a profile-member contract.");
    }
    if (!String(source.source || source.sourceMobpack || "").trim()) {
      throw new Error("MobKit agent definition is missing its source contract.");
    }
    if (!String(source.sourceMobpack || "").trim()) {
      throw new Error("MobKit agent definition is missing its sourceMobpack contract.");
    }
    if (!String(source.sourceOrigin || "").trim()) {
      throw new Error("MobKit agent definition is missing its sourceOrigin contract.");
    }
    if (!String(source.id || "").trim()) {
      throw new Error("MobKit agent definition is missing its id contract.");
    }
    const role = String(source.role || "").trim();
    if (!role) {
      throw new Error("MobKit agent definition is missing its role contract.");
    }
    const displayName = String(source.name || source.label || "").trim();
    if (!displayName) {
      throw new Error("MobKit agent definition is missing its name contract.");
    }
    if (!String(source.profileBinding || "").trim()) {
      throw new Error("MobKit agent definition is missing its profileBinding contract.");
    }
    if (!String(source.runtimeMode || "").trim()) {
      throw new Error("MobKit agent definition is missing its runtimeMode contract.");
    }
    const model = String(source.model || "").trim();
    if (!model) {
      throw new Error("MobKit agent definition is missing its model contract.");
    }
    validateAgentDefinitionCatalogRefs(source, options);
    const baseRole = slug(role, "member").replace(/-/g, "_");
    let id = `m_${baseRole}`;
    let index = 2;
    const used = new Set((existingMembers || []).map((member) => member.id));
    while (used.has(id)) id = `m_${baseRole}_${index++}`;
    const name = uniqueMemberName(displayName, existingMembers);
    return {
      id,
      name,
      role,
      model,
      schema: source.schema || "",
      skills: Array.isArray(source.skills) ? [...source.skills] : [],
      tools: Array.isArray(source.tools) ? [...source.tools] : [],
      profileBinding: source.profileBinding,
      realmProfile: source.realmProfile || "",
      runtimeMode: source.runtimeMode,
      externalAddressable: !!source.externalAddressable,
      backend: normalizeProfileBackend(source.backend),
      maxInlinePeerNotifications: normalizeMaxInlinePeerNotifications(source.maxInlinePeerNotifications ?? source.max_inline_peer_notifications),
      systemPrompt: source.systemPrompt || "",
      providerParams: normalizeProviderParams(source.providerParams || source.provider_params),
      sourceDefinition: {
        definitionType: source.definitionType,
        definitionId: source.id,
        source: source.source,
        sourceMobpack: source.sourceMobpack,
        sourceMobpackName: source.sourceMobpackName || "",
        sourceOrigin: source.sourceOrigin,
        sourceDocumentPath: source.sourceDocumentPath || "",
        schemaSourceDocumentPath: source.schemaSourceDocumentPath || "",
        toolDefinitions: normalizeAgentDefinitionRows(source.toolDefinitions || source.tool_definitions),
        skillDefinitions: normalizeAgentDefinitionRows(source.skillDefinitions || source.skill_definitions),
      },
    };
  }

  function agentDefinitionAddPatch(definition, { members, schemas, contract, deploySettings, modelCatalog, toolCatalog, skillRealms } = {}) {
    const existingMembers = Array.isArray(members) ? members : [];
    const existingSchemas = Array.isArray(schemas) ? schemas : [];
    const nextSchemas = mergeAgentDefinitionSchemas(existingSchemas, definition);
    validateAgentDefinitionSchemaRef(definition, nextSchemas);
    const member = memberFromAgentDefinition(definition, existingMembers, { contract, deploySettings, modelCatalog, toolCatalog, skillRealms });
    return {
      member,
      members: [...existingMembers, member],
      schemas: nextSchemas,
      schemasChanged: nextSchemas !== existingSchemas,
      selection: { kind: "agent", id: member.id },
    };
  }

  function agentDefinitionAddByIdPatch(agentDefinitions, definitionId, { members, schemas, contract, deploySettings, modelCatalog, toolCatalog, skillRealms } = {}) {
    const id = String(definitionId || "").trim();
    const definition = (Array.isArray(agentDefinitions) ? agentDefinitions : []).find((candidate) => candidate?.id === id);
    if (!definition) {
      return {
        ok: false,
        member: null,
        members: Array.isArray(members) ? members : [],
        schemas: Array.isArray(schemas) ? schemas : [],
        schemasChanged: false,
        selection: null,
        error: "unknown agent definition",
      };
    }
    try {
      return {
        ok: true,
        ...agentDefinitionAddPatch(definition, { members, schemas, contract, deploySettings, modelCatalog, toolCatalog, skillRealms }),
      };
    } catch (error) {
      return {
        ok: false,
        member: null,
        members: Array.isArray(members) ? members : [],
        schemas: Array.isArray(schemas) ? schemas : [],
        schemasChanged: false,
        selection: null,
        error: error?.message || String(error),
      };
    }
  }

  function memberPromptSkeleton(member) {
    const notes = String(member?.systemPrompt || "").trim();
    const name = member?.name || "this agent";
    const intent = notes || `Act as the ${member?.role || "member"} of the mob.`;
    const lines = [
      `You are ${name}, a member of a Meerkat mob.`,
      "",
      "## Mandate",
      intent.replace(/\s+/g, " "),
      "",
      "## Operating rules",
      "- Read the shared mob workpad and prior members' output before acting.",
      "- Do exactly what this step requires — no more, no less.",
      member?.schema ? `- Emit a ${member.schema} as your structured output.` : "- Return a concise, well-structured result.",
      "- Hand off cleanly: state what you did and what the next member needs.",
    ];
    return lines.join("\n");
  }

  function memberNamePatch(rawName) {
    return { name: String(rawName || "") };
  }

  function memberRealmProfilePatch(rawProfile) {
    return { realmProfile: String(rawProfile || "").trim() };
  }

  function memberSystemPromptPatch(rawPrompt) {
    return { systemPrompt: String(rawPrompt || "") };
  }

  function memberProfileBindingPatch(member, rawBinding, contract) {
    const binding = String(rawBinding || "").trim();
    if (!optionValueAllowed(profileBindingOptions(contract, binding), binding)) return {};
    return {
      profileBinding: binding,
      realmProfile: binding === "realm_profile"
        ? String(member?.realmProfile || member?.role || member?.name || "")
        : "",
    };
  }

  function memberRuntimeModePatch(rawMode, contract, deploySettings) {
    const runtimeMode = String(rawMode || "").trim();
    if (!optionValueAllowed(runtimeModeOptions(contract, deploySettings, runtimeMode), runtimeMode)) return {};
    return { runtimeMode };
  }

  function memberModelPatch(rawModel, modelCatalog) {
    const model = String(rawModel || "").trim();
    const ids = (modelCatalog || []).map((entry) => String(entry?.id || "").trim()).filter(Boolean);
    if (!catalogValueAllowed(ids, model, { allowBlank: false })) return {};
    return { model };
  }

  function memberSchemaPatch(rawSchema, schemas) {
    const schema = String(rawSchema || "").trim();
    if (Array.isArray(schemas)) {
      const ids = schemas.map((entry) => String(entry?.id || "").trim()).filter(Boolean);
      if (schema && !ids.includes(schema)) return {};
    }
    return { schema };
  }

  function memberSchemaCascadePatch({ memberId, members, flow, edges, instances, schemas } = {}, rawSchema) {
    const id = String(memberId || "").trim();
    const list = Array.isArray(members) ? members : [];
    const sourceInstances = Array.isArray(instances) ? instances : [];
    const current = list.find((member) => String(member?.id || "").trim() === id) || null;
    if (!current) {
      return { ok: false, error: "member not found", members: list, flow, edges, instances: sourceInstances, patch: null };
    }
    const patch = memberSchemaPatch(rawSchema, schemas);
    if (!Object.prototype.hasOwnProperty.call(patch, "schema")) {
      return { ok: false, error: "unknown schema", members: list, flow, edges, instances: sourceInstances, patch: null };
    }
    const nextMember = { ...current, ...patch };
    const nextMembers = list.map((member) => String(member?.id || "").trim() === id ? nextMember : member);
    const reconciled = reconcileConditionFieldAvailability({
      flow,
      edges,
      members: nextMembers,
      instances: sourceInstances,
      schemas,
    });
    return {
      ok: true,
      error: "",
      patch,
      member: nextMember,
      members: nextMembers,
      flow: reconciled.flow,
      edges: reconciled.edges,
      instances: sourceInstances,
    };
  }

  function memberBackendPatch(rawBackend, contract) {
    const backend = String(rawBackend || "").trim();
    if (!optionValueAllowed(profileBackendOptions(contract, backend, true), backend, { allowBlank: true })) return {};
    return { backend };
  }

  function memberMaxInlinePeerNotificationsPatch(rawValue) {
    return { maxInlinePeerNotifications: normalizeMaxInlinePeerNotifications(rawValue) };
  }

  function memberProviderParamsEditorState(member, agentDetailView = null) {
    const view = agentDetailViewForState(agentDetailView);
    return {
      label: view.providerParamsLabel,
      text: member?.providerParams ? JSON.stringify(member.providerParams, null, 2) : "",
      placeholder: view.providerParamsPlaceholder,
      rows: view.providerParamsRows,
      invalidJsonLabel: view.providerParamsInvalidJsonLabel,
    };
  }

  function memberProviderParamsPatch(rawText, agentDetailView = null) {
    const view = agentDetailViewForState(agentDetailView);
    const text = String(rawText || "").trim();
    if (!text) return { ok: true, patch: { providerParams: null }, error: "" };
    try {
      const parsed = JSON.parse(text);
      const normalized = normalizeProviderParams(parsed);
      if (!normalized) {
        return { ok: false, patch: null, error: view.providerParamsObjectRequiredError };
      }
      return { ok: true, patch: { providerParams: normalized }, error: "" };
    } catch (err) {
      return { ok: false, patch: null, error: err?.message || view.providerParamsInvalidJsonLabel };
    }
  }

  function normalizeProfileBackend(value) {
    return String(value || "").trim();
  }

  function normalizeMaxInlinePeerNotifications(value) {
    if (value === null || value === undefined || value === "") return null;
    const number = typeof value === "number" ? value : Number(value);
    if (!Number.isInteger(number) || number < -1) return null;
    return number;
  }

  function normalizePositiveInteger(value) {
    if (value === null || value === undefined || value === "") return null;
    const number = typeof value === "number" ? value : Number(value);
    if (!Number.isInteger(number) || number <= 0) return null;
    return number;
  }

  function normalizeStringList(value) {
    const source = Array.isArray(value)
      ? value
      : String(value || "").split(",");
    return source
      .map((item) => String(item || "").trim())
      .filter(Boolean);
  }

  function normalizeOutputFormat(value) {
    return String(value || "").trim();
  }

  function normalizeProviderParams(value) {
    if (!value || typeof value !== "object" || Array.isArray(value)) return null;
    return JSON.parse(JSON.stringify(value));
  }

  function uniqueMemberName(name, members) {
    const base = String(name || "member").trim() || "member";
    const used = new Set((members || []).map((member) => member.name));
    if (!used.has(base)) return base;
    let index = 2;
    while (used.has(`${base}-${index}`)) index += 1;
    return `${base}-${index}`;
  }

  window.MobKitFlowController = {
    SCHEMA_VERSION,
    RPC_METHODS,
    configure,
    buildDocument,
    authoringFlowForDocument,
    authoringDocumentFromState,
    authoringProjectionApplyPlan,
    createFlowDraftFromSpec,
    flowRegistryCreateDraftProjection,
    flowDraftIdFromSpec,
    newFlowTemplateOptions,
    newFlowInitialState,
    newFlowModalState,
    graphSignature,
    graphStructureSignature,
    graphProjectionForFlow,
    graphProjectionForDocument,
    flowFromHydratedDocument,
    hydrateMobpackDocumentState,
    graphToFlow,
    profileName,
    normalizeToolRef,
    addInlineSkillToRealms,
    memberToolAccessPatch,
    memberToolRemovePatch,
    memberToolAccessCascadePatch,
    memberToolRemoveCascadePatch,
    memberToolAccessState,
    stepToolScopeState,
    stepToolScopeAddPatch,
    stepToolScopeRemovePatch,
    memberSkillTogglePatch,
    memberSkillRemovePatch,
    memberInlineSkillPatch,
    memberSkillToggleCascadePatch,
    memberSkillRemoveCascadePatch,
    memberInlineSkillCascadePatch,
    memberSkillAccessState,
    agentListState,
    agentSelectionState,
    agentListSelectionProjection,
    agentEditorControlState,
    agentSourceProvenanceState,
    agentDefinitionOptions,
    agentDefinitionAddControlState,
    agentDefinitionAddErrorState,
    memberSchemaChangeErrorState,
    schemaDefinitionAddErrorState,
    schemaDefinitionAddTransition,
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
    graphControlShape,
    graphMemberInstanceShape,
    graphQuickInsertProjection,
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
    graphConnectionEdgeDraft,
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
    graphSourceFileNode,
    graphCanvasInstances,
    graphNodeCanvasState,
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
    schemaDefinitionAddPatch,
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
    schemaFieldAddPatch,
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
    studioAddInstancePatch,
    studioAppendInstancesPatch,
    studioUpdateInstancePatch,
    studioMoveInstancePatch,
    studioDeleteInstancePatch,
    studioAddEdgePatch,
    graphConnectionAddPatch,
    studioAppendEdgesPatch,
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
    inputParamUpdateCascadePatch,
    inputParamDeletePatch,
    inputParamRenamePatch,
    inputParamRenameCascadePatch,
    inputParamDeleteCascadePatch,
    inputParamAddPatch,
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
    deployCommandPreview,
    deployCommandPreviewForDocument,
    callRpc,
    loadSchema,
    loadCatalogs,
    authoringRpcMethodsFromSchema,
    configureAuthoringMethodsFromSchema,
    validateDocument,
    sourceDocument,
    exportDocument,
    deployDocument,
    importDocument,
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
    sourceEditorState,
    sourceFileSelectionTransition,
    sampleFlowsFromCatalogs,
    flowCatalogBootstrapState,
    newFlowModalPatch,
    newFlowModalFieldPatch,
    newFlowModalStepPatch,
    newFlowModalCreateSpec,
    flowRegistryMarkDraftPatch,
    flowRegistryViewState,
    flowRegistrySelectionState,
    flowRegistryFallbackOpenTransition,
    flowRegistryRowFromDocument,
    flowImportedIdFromDocument,
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
    cloneDocument,
    agentDefinitionsFromCatalogs,
    agentDefinitionCatalogState,
    agentDeleteConfirmationState,
    memberFromAgentDefinition,
    agentDefinitionAddPatch,
    agentDefinitionAddByIdPatch,
    schemaDefinitionsFromAgentDefinition,
    mergeAgentDefinitionSchemas,
  };
})();
