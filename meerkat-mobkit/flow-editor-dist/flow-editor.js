
/* data.js */

/* global window */
// MobKit Flow Editor boot constants only.
// Live mobpack state, models, tools, skills, and agent definitions hydrate from
// the MobKit schema RPC and live in the app/controller state planes.

const GRID = {
  cols: 5,    // initial; grows dynamically based on instances
  rows: 3,    // initial; grows dynamically based on instances
  cellW: 220,
  cellH: 158,
  gapX: 32,
  gapY: 24,
  padX: 56,
  padY: 56,
};
function cellXY(col, row) {
  return {
    x: GRID.padX + col * (GRID.cellW + GRID.gapX),
    y: GRID.padY + row * (GRID.cellH + GRID.gapY),
  };
}

const TEMPLATE_META = {
  name: "untitled-mob",
  repo: "mob.toml",
  version: "draft",
  triggers: { labels: [], default: false },
};

window.MOBKIT_BOOT = {
  GRID,
  cellXY,
  template: TEMPLATE_META,
};


/* controller.js */

/* global window, fetch */
// MobKit Flow Editor controller plane.
// Keeps deployable document generation and API calls outside the visual JSX.

(function () {
  const SCHEMA_VERSION = "0.1.0";
  const RPC_METHODS = {
    schema: "mobkit/mobpacks/schema",
    validate: "mobkit/mobpacks/validate",
    export: "mobkit/mobpacks/export",
    import: "mobkit/mobpacks/import",
    deployCommand: "mobkit/mobpacks/deploy_command",
    deploy: "mobkit/mobpacks/deploy",
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

  function addInlineSkillToRealms(realms, spec = {}) {
    const nextRealms = JSON.parse(JSON.stringify(realms || []));
    const label = String(spec.label || spec.id || "").trim();
    if (!label) throw new Error("Inline skill id or label is required.");
    const content = String(spec.content || "").trim();
    if (!content) throw new Error("Inline skill content is required.");
    const used = skillIdsFromRealms(nextRealms);
    const explicitId = String(spec.id || "").trim();
    const identityText = explicitId || label;
    if (!normalizeSkillId(identityText)) throw new Error("Inline skill id or label must contain letters or numbers.");
    const rawId = explicitId || (label.includes(".") ? label : `mob.${label}`);
    const baseId = normalizeSkillId(rawId);
    if (!baseId) throw new Error("Inline skill id or label must contain letters or numbers.");
    let id = baseId;
    let index = 2;
    while (used.has(id)) id = `${baseId}.${index++}`;
    let realm = nextRealms.find((candidate) => candidate?.id === "mobkit/editor-inline");
    if (!realm) {
      realm = {
        id: "mobkit/editor-inline",
        label: "This mobpack",
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
      desc: spec.desc || "Inline MobKit skill stored in this mobpack.",
    });
    return { id, skillRealms: nextRealms };
  }

  function memberToolAccessPatch(member, raw, toolCatalog) {
    const id = normalizeToolRef(raw, toolCatalog);
    if (!id) {
      return {
        ok: false,
        id: "",
        error: raw ? "Use a MobKit-listed runtime tool or configured MCP/Rust source." : "",
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

  function memberToolAccessCascadePatch({ memberId, members, flow, instances } = {}, raw, toolCatalog) {
    const list = Array.isArray(members) ? members : [];
    const member = list.find((candidate) => candidate?.id === memberId) || null;
    if (!member) return { ok: false, error: "member not found", id: "", patch: null, members: list, flow, instances };
    const access = memberToolAccessPatch(member, raw, toolCatalog);
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

  function memberToolAccessState(member, toolCatalog = []) {
    const catalog = Array.isArray(toolCatalog) ? toolCatalog.filter((tool) => tool?.id) : [];
    const metaById = new Map(catalog.map((tool) => [String(tool.id), tool]));
    const selectedTools = normalizeStringList(member?.tools);
    const selectedSet = new Set(selectedTools);
    const toolRow = (id) => {
      const meta = metaById.get(id) || null;
      return {
        id,
        name: id,
        description: meta?.desc || "—",
        meta,
        className: "tool-row",
        removeLabel: "×",
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
      title: "TOOL ACCESS",
      hint: "Authority is calculated from this allowlist. Reviewed once here.",
      rows: selectedTools.map(toolRow),
      addableRows: catalog
        .filter((tool) => !selectedSet.has(String(tool.id)))
        .map(addableRow),
      addSelectValue: "",
      addSelectPlaceholder: "+ add tool…",
      sourceLabel: "Configured tool source",
      sourcePlaceholder: "choose from MobKit tool catalog",
      addButtonLabel: "ADD",
    };
  }

  function stepToolScopeState({ member, selected, mode = "member", toolCatalog = [] } = {}) {
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
        ? (mode === "catalog" ? "not in MobKit tool catalog" : "not enabled on profile")
        : "";
      return {
        id,
        name: id,
        meta,
        unavailable,
        reason,
        className: `tool-row${unavailable ? " tool-row--invalid" : ""}`,
        description: unavailable ? reason : (meta?.desc || "MobKit tool"),
        removeLabel: "×",
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
        ? "select a member first"
        : (mode === "catalog" ? "+ block MobKit tool..." : "+ add profile tool..."),
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

  function memberInlineSkillPatch(member, realms, spec = {}) {
    const result = addInlineSkillToRealms(realms, spec);
    const skills = normalizeStringList(member?.skills);
    return {
      ...result,
      realmId: "mobkit/editor-inline",
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

  function memberInlineSkillCascadePatch({ memberId, members, skillRealms } = {}, spec = {}) {
    const list = Array.isArray(members) ? members : [];
    const member = list.find((candidate) => candidate?.id === memberId) || null;
    if (!member) return { ok: false, error: "member not found", id: "", patch: null, members: list, skillRealms };
    const result = memberInlineSkillPatch(member, skillRealms, spec);
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
    };
  }

  function memberSkillAccessState({ member, skillRealms, realmId = "", inlineOpen = false } = {}) {
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
          checkLabel: selected ? "✓" : "",
          name: id,
          desc: skill.desc || skill.path || skill.source || "MobKit skill",
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
        removeLabel: "×",
      }));
    const unavailableSelected = selectedSkillIds
      .filter((id) => !byId.has(id))
      .map((id) => ({
        id,
        className: "skill-chip is-invalid",
        label: id,
        removeLabel: "×",
      }));
    return {
      sectionTitle: "SKILLS",
      inlineToggleLabel: inlineOpen ? "CANCEL" : "+ INLINE",
      hint: "Selected skills are baked into the mobpack. Browse a realm to add more.",
      inlineLabelPlaceholder: "mob.skill-name",
      inlineContentRows: 4,
      inlineContentPlaceholder: "Skill instructions stored as [skills.<id>] content",
      inlineCreateHint: "Creates an inline skill definition in this mobpack.",
      inlineAddLabel: "ADD SKILL",
      inlineErrorFallback: "Could not create inline skill.",
      noRealmsMessage: "MobKit did not provide skill realms for this document.",
      realmLabel: "Realm",
      hasRealms: realms.length > 0,
      realmId: selectedRealm?.id || "",
      realmOptions: realms.map((realm) => ({
        id: realm.id,
        label: `${realm.label || realm.id}${realm.default ? " · default" : ""}`,
      })),
      skillRows,
      selectedOutsideRealm,
      unavailableSelected,
      unavailableHeading: "Unavailable in MobKit skill realms:",
      outsideRealmHeading: "Selected from other realms:",
    };
  }

  function agentListState({ members = [], instances = [], schemas = [], selection = null } = {}) {
    const sourceMembers = Array.isArray(members) ? members : [];
    const sourceInstances = Array.isArray(instances) ? instances : [];
    const sourceSchemas = Array.isArray(schemas) ? schemas : [];
    const memberRows = sourceMembers.map((member) => {
      const placedCount = sourceInstances.filter((instance) => instance?.memberId === member.id).length;
      const selected = selection?.kind === "agent" && selection.id === member.id;
      const placedLabel = placedCount === 0 ? "unplaced" : `×${placedCount}`;
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
        subLabel: `${member.role} · ${member.model}`,
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
      const fieldLabel = `${fieldCount} field${fieldCount === 1 ? "" : "s"}`;
      const usageLabel = `used by ${usedCount}`;
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
        subLabel: `${fieldLabel} · ${usageLabel}`,
      };
    });
    return {
      memberCount: memberRows.length,
      schemaCount: schemaRows.length,
      memberRows,
      schemaRows,
    };
  }

  function agentSelectionState({ selection = null, members = [], schemas = [] } = {}) {
    if (!selection) return { kind: "empty", member: null, schema: null, missing: false };
    if (selection.kind === "schema") {
      const schema = (Array.isArray(schemas) ? schemas : []).find((candidate) => candidate.id === selection.id) || null;
      return { kind: "schema", member: null, schema, missing: !schema };
    }
    if (selection.kind === "agent") {
      const member = (Array.isArray(members) ? members : []).find((candidate) => candidate.id === selection.id) || null;
      return { kind: "agent", member, schema: null, missing: !member };
    }
    return { kind: String(selection.kind || ""), member: null, schema: null, missing: true };
  }

  function agentEditorControlState({ member, instances = [], schemas = [], contract, deploySettings, modelCatalog = [] } = {}) {
    const placedAt = (Array.isArray(instances) ? instances : []).filter((instance) => instance?.memberId === member?.id);
    const placedCount = placedAt.length;
    const memberName = String(member?.name || member?.id || "agent");
    const schema = (Array.isArray(schemas) ? schemas : []).find((candidate) => candidate.id === member?.schema) || null;
    const profileBinding = typeof member?.profileBinding === "string"
      ? member.profileBinding
      : (member?.realmProfile ? "realm_profile" : "");
    const realmProfileRestriction = profileBindingRestriction(contract, "realm_profile");
    const bindingOptions = [
      { value: "", label: "missing profile binding", disabled: false, reason: "" },
      ...profileBindingOptions(contract, profileBinding),
    ];
    const runtimeMode = typeof member?.runtimeMode === "string" ? member.runtimeMode : "";
    const runtimeOptions = [
      { value: "", label: "missing runtime mode", disabled: false, reason: "" },
      ...runtimeModeOptions(contract, deploySettings, runtimeMode),
    ];
    const backendValue = String(member?.backend || "");
    const backendOptions = profileBackendOptions(contract, backendValue, true);
    const schemaOptions = [
      { value: "", label: "— none —", schema: null },
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
        requiredLabel: field.required ? "req" : "",
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
      idLine: `${member?.id || ""} · used in ${placedCount} instance${placedCount === 1 ? "" : "s"}`,
      deleteLabel: "DELETE",
      deleteNeedsConfirmation: placedCount > 0,
      deleteConfirmMessage: placedCount > 0
        ? `Delete agent "${memberName}"? It is placed in ${placedCount} cell${placedCount === 1 ? "" : "s"} - those nodes will be removed.`
        : "",
      usageTitle: `USED IN · ${placedCount}`,
      emptyUsageHint: "Not yet placed in any cell. Switch to Topology to add.",
      usageRows: placedAt.map((instance) => ({
        id: instance.id,
        cellLabel: `cell (${Number(instance.col || 0) + 1},${Number(instance.row || 0) + 1})`,
        laneLabel: instance.lane || "—",
        instance,
      })),
      identityTitle: "IDENTITY",
      profileBindingLabel: "Profile binding",
      realmProfileLabel: "Realm profile",
      realmProfilePlaceholder: "realm profile id",
      realmProfileImportHint: realmProfileRestriction.reason || "Realm profile refs are import-only for this editor. Mobpack archives must use inline profiles before validation/export.",
      realmProfileTitle: "REALM PROFILE",
      realmProfileReferenceLabel: member?.realmProfile || member?.role || member?.name || "",
      realmProfileReferenceHintBefore: "This imported member references",
      realmProfileReferenceHintAfter: realmProfileRestriction.reason
        ? `from a target realm. ${realmProfileRestriction.reason}`
        : "from a target realm. Convert it to an inline profile before validating or exporting a deployable mobpack.",
      modelLabel: "Model",
      runtimeModeLabel: "Runtime mode",
      backendLabel: "Backend",
      inlinePeerNotificationsLabel: "Inline peer notifications",
      inlinePeerNotificationsPlaceholder: "runtime default",
      systemPromptTitle: "SYSTEM PROMPT",
      applySkeletonLabel: "APPLY SKELETON",
      applySkeletonTitle: "Apply a MobKit profile prompt skeleton",
      systemPromptPlaceholder: "Describe the member mandate. This text is exported as the profile peer_description.",
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
      outputSchemaTitle: "OUTPUT SCHEMA",
      schemaPreviewRows,
      hasOutputSchema: !!schema,
      editSchemaLabel: "Edit schema →",
      editSchemaSelection: schema ? { kind: "schema", id: schema.id } : null,
      emptySchemaHint: "No structured output. Agent returns free-form text.",
      modelOptions,
    };
  }

  function agentDefinitionOptions(agentDefinitions = []) {
    const optionRows = (Array.isArray(agentDefinitions) ? agentDefinitions : [])
      .filter((definition) => definition?.id)
      .map((definition) => ({
        value: definition.id,
        label: definition.label || definition.role || definition.id,
        definition,
      }));
    return {
      hasDefinitions: optionRows.length > 0,
      optionRows,
    };
  }

  function agentDefinitionAddControlState(agentDefinitions = []) {
    const definitionState = agentDefinitionOptions(agentDefinitions);
    return {
      ...definitionState,
      controlClass: definitionState.hasDefinitions
        ? "agents-list__add agents-list__add--select"
        : "agents-list__add",
      disabled: !definitionState.hasDefinitions,
      title: definitionState.hasDefinitions
        ? "Create an agent from a MobKit profile-member definition."
        : "MobKit schema contract has not provided agent definitions yet.",
      unavailableLabel: "agents unavailable",
      placeholderOption: { value: "", label: "+ new agent..." },
      value: "",
    };
  }

  function schemaEditorControlState({ schema, members = [] } = {}) {
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
      eyebrow: "OUTPUT SCHEMA",
      descriptionTitle: "DESCRIPTION",
      descriptionPlaceholder: "What is this artifact and when is it emitted?",
      fieldsTitle: `FIELDS · ${fields.length}`,
      addFieldLabel: "+ field",
      headerLabels: {
        name: "NAME",
        type: "TYPE",
        required: "REQ",
        description: "DESCRIPTION",
        action: "",
      },
      fieldRows,
      emptyFieldsHint: "No fields yet. Click + field to start.",
      usedBy,
      usedCount: usedBy.length,
      usageLabel: `used by ${usedBy.length} agent${usedBy.length === 1 ? "" : "s"}`,
      usedByTitle: `USED BY · ${usedBy.length}`,
      emptyUsedByHint: "Not yet referenced by any agent.",
      deleteLabel: "DELETE",
      canDelete: usedBy.length === 0,
      deleteTitle: usedBy.length > 0 ? "Unassign from agents first" : "",
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

  function flowStepDeletePatch(flow, id) {
    const target = String(id || "").trim();
    const steps = flowStepRemoveFromTree(flow?.steps || [], target);
    const nextFlow = { ...(flow || {}), steps };
    return target ? reconcileDeletedFlowStepReferences(nextFlow, target) : nextFlow;
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
    const raw = String(rawFormat || "").trim();
    if (raw && !normalizeOutputFormat(raw)) return {};
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

  function renameSchemaDefinition({ schemas, members } = {}, oldId, newId) {
    const previousId = String(oldId || "").trim();
    const nextId = String(newId || "").trim();
    const sourceSchemas = Array.isArray(schemas) ? schemas : [];
    const sourceMembers = Array.isArray(members) ? members : [];
    if (!previousId || !nextId || previousId === nextId) {
      return { schemas: sourceSchemas, members: sourceMembers, renamed: false };
    }
    if (sourceSchemas.some((schema) => String(schema?.id || "").trim() === nextId)) {
      return {
        schemas: sourceSchemas,
        members: sourceMembers,
        renamed: false,
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
        renamed: false,
        reason: "unknown_schema_id",
      };
    }
    const nextMembers = sourceMembers.map((member) =>
      String(member?.schema || "").trim() === previousId
        ? { ...member, schema: nextId }
        : member
    );
    return { schemas: nextSchemas, members: nextMembers, renamed: true };
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
        if (runtimeMode && !runtimeModes.includes(runtimeMode)) write("runtimeMode", "");
        if (surface === "cli" && runtimeMode === "autonomous_host" && runtimeModes.includes("turn_driven")) {
          write("runtimeMode", "turn_driven");
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
      out.set(id, new Set((schema.fields || [])
        .map((field) => String(field?.name || "").trim())
        .filter(Boolean)));
    }
    return out;
  }

  function inputParamNameSet(flow) {
    const out = new Set();
    collectVisualSteps(flow?.steps || [], (step) => {
      if (step?.type !== "input") return;
      for (const param of step.inputParams || []) {
        const name = String(param?.name || "").trim();
        if (name) out.add(name);
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
      return inputFields.has(field);
    }
    const stepId = String(cond.stepId || cond.step_id || "").trim();
    if (!stepId) return true;
    return schemaHasField(schemaFields, stepSchemas.get(stepId), field);
  }

  function reconcileConditionAvailabilityInEdges(edges, stepSchemas, schemaFields, inputFields) {
    let changed = false;
    const next = (edges || []).map((edge) => {
      const condition = normalizedEdgeCondition(edge);
      const path = String(condition?.path || "").trim();
      const parts = path.split(".").filter(Boolean);
      let available = true;
      if (parts.length === 2 && parts[0] === "params") {
        available = inputFields.has(parts[1]);
      } else if (parts.length === 3 && parts[0] === "steps") {
        available = schemaHasField(schemaFields, stepSchemas.get(parts[1]), parts[2]);
      }
      if (available) return edge;
      changed = true;
      return { ...edge, cond: null, label: "" };
    });
    return changed ? next : edges;
  }

  function schemaHasField(schemaFields, schemaId, field) {
    const id = String(schemaId || "").trim();
    const name = String(field || "").trim();
    return !!id && !!name && schemaFields.get(id)?.has(name);
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

  function conditionValueControl(field, rawValue = "") {
    const type = String(field?.type || "").trim();
    const value = rawValue == null ? "" : String(rawValue);
    const optionRows = (values) => [
      { value: "", label: "—" },
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
    return { kind: "text", values: [], value, optionRows: [], placeholder: "value" };
  }

  function inputParamName(raw, fallback = "field") {
    return String(raw || fallback)
      .trim()
      .replace(/[^A-Za-z0-9_]+/g, "_")
      .replace(/^_+|_+$/g, "")
      .replace(/^[0-9]/, "_$&") || fallback;
  }

  function uniqueInputParamName(params, raw, currentId = null) {
    const base = inputParamName(raw, "param");
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

  function uniqueSchemaFieldName(fields, raw, currentId = null) {
    const base = schemaFieldName(raw, "field");
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

  function schemaFieldRowControlState(field, contract, overrides = {}) {
    const typeState = schemaLikeFieldTypeControlState(field, contract);
    return {
      namePlaceholder: overrides.namePlaceholder || "field_name",
      descriptionPlaceholder: "—",
      removeTitle: overrides.removeTitle || "Remove field",
      enumLabel: "VALUES",
      enumAddLabel: "+ value",
      enumAddValue: "value",
      enumValues: enumValuesForField(field),
      typeState,
    };
  }

  function inputParamFieldControlState(param, contract) {
    return schemaFieldRowControlState(param, contract, {
      namePlaceholder: "param_name",
      removeTitle: "Remove param",
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
      normalized.name = uniqueSchemaFieldName(fields, normalized.name, fieldId);
    }
    return { fields: fields.map((field) => field?.id === fieldId ? { ...field, ...normalized } : field) };
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

  function directMemberAddValidation(member, members = []) {
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
    if (profileBinding !== "inline") {
      return { ok: false, error: "direct member adds must use an inline deployable profileBinding" };
    }
    if (!runtimeMode) {
      return { ok: false, error: "member must include runtimeMode" };
    }
    if (!model) {
      return { ok: false, error: "inline member definitions must include a model" };
    }
    return { ok: true, error: "" };
  }

  function studioAddMemberPatch({ members } = {}, member) {
    const list = Array.isArray(members) ? members : [];
    const validation = directMemberAddValidation(member, list);
    if (!validation.ok) {
      return { ok: false, error: validation.error, members: list, member: null };
    }
    return { ok: true, error: "", members: [...list, member], member };
  }

  function studioUpdateMemberPatch({ members } = {}, id, patch = {}) {
    const target = String(id || "");
    const list = Array.isArray(members) ? members : [];
    const current = list.find((member) => member?.id === target) || null;
    if (!current) return { ok: false, error: "member not found", members: list };
    const nextMember = { ...current, ...(patch && typeof patch === "object" ? patch : {}) };
    const validation = memberUpdateValidation(current, nextMember, patch);
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

  function memberUpdateValidation(current, nextMember, patch = {}) {
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
    if (binding && binding !== "inline") {
      return { ok: false, error: "member updates must keep deployable inline profileBinding" };
    }
    if (!binding && (touched.has("profileBinding") || touched.has("profile_binding"))) {
      return { ok: false, error: "member updates must keep profileBinding explicit" };
    }
    if (!runtimeMode && (touched.has("runtimeMode") || touched.has("runtime_mode"))) {
      return { ok: false, error: "member updates must keep runtimeMode explicit" };
    }
    if ((touched.has("runtimeMode") || touched.has("runtime_mode")) && !knownMobKitRuntimeMode(runtimeMode)) {
      return { ok: false, error: "member updates must use a MobKit runtime_mode value" };
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

  function knownMobKitRuntimeMode(runtimeMode) {
    return runtimeMode === "turn_driven" || runtimeMode === "autonomous_host";
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
    return { edges: (edges || []).filter((edge) => edge?.id !== target) };
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

  function inputParamOptions(flow) {
    const input = (flow?.steps || []).find((step) => step.type === "input");
    const fields = inputParamsForStep(input);
    if (!fields.length) return [];
    return [{
      stepId: "params",
      namespace: "params",
      label: "Input params",
      fields,
    }];
  }

  function basicInputControlState(step, contract) {
    const params = inputParamsForStep(step);
    return {
      panelIcon: "▤",
      panelTitle: "Input",
      panelSub: "The task this mob is run with — its ingress",
      taskLabel: "Task",
      taskPlaceholder: "e.g. Fix the issue described below.",
      params,
      paramsTitle: `INPUT PARAMS · ${params.length}`,
      addParamLabel: "+ param",
      headerRows: [
        { key: "name", label: "NAME", className: "sb-col sb-col--name" },
        { key: "type", label: "TYPE", className: "sb-col sb-col--type" },
        { key: "required", label: "REQ", className: "sb-col sb-col--req" },
        { key: "description", label: "DESCRIPTION", className: "sb-col sb-col--desc" },
        { key: "actions", label: "", className: "sb-col sb-col--act" },
      ],
      emptyParamsParts: [
        { key: "prefix", text: "No params yet. Add one before branching on " },
        { key: "ref", text: "params.*", kind: "code" },
        { key: "suffix", text: "." },
      ],
      tips: [
        "Run with: rkat mob deploy <pack> \"<task>\" — or run_flow(input).",
        "Typed fields become the input schema the run is validated against.",
        "Event sources & schedules live outside the mobpack (e.g. fugue).",
      ],
    };
  }

  function basicConditionOptions(flow, targetId, members) {
    return [
      ...inputParamOptions(flow),
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

  function graphConditionOptions({ instances, members, schemas, edge, flow } = {}) {
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
        member: { name: "Input params" },
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
      normalized.name = uniqueInputParamName(source, normalized.name, id);
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
    const nextName = uniqueInputParamName(params, rawName, id);
    const next = (params || []).map((param) => param?.id === id ? { ...param, name: nextName } : param);
    return { name: nextName, patch: { inputParams: next, fields: inputParamSummary(next, contract) } };
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
      label: "Branch " + (branches.length + 1),
      steps: [],
    };
    if (step?.type !== "parallel") nextBranch.condition = "";
    return { branches: [...branches, nextBranch] };
  }

  function basicConditionLabel(cond, options = [], config = {}) {
    if (!cond || !cond.stepId || !cond.field) return "…";
    const option = (Array.isArray(options) ? options : []).find((candidate) => candidate.stepId === cond.stepId);
    const label = option?.label || option?.member?.name || cond.stepId;
    const op = cond.op || cond.operator || config.defaultOperator || "";
    return `${label}.${cond.field} ${op} ${conditionValueLiteral(cond.val ?? cond.value ?? "")}`;
  }

  function basicBranchConditionControlState({ branch, options = [], schemas = [], contract } = {}) {
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
      fieldPlaceholder: fields.length ? "— field —" : "(no schema)",
      defaultOperator,
      operatorValue,
      operatorOptions: conditionOperatorOptions(contract, operatorValue),
      previewLabel: basicConditionLabel(cond, sourceOptions, { defaultOperator }),
      hasConditionOptions: sourceOptions.length > 0,
    };
  }

  function basicBranchParallelControlState({ step, flow, members = [], contract } = {}) {
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
      panelTitle: isParallel ? "Parallel" : "Branch",
      panelSub: isParallel ? "fan_out to members, then fan_in and collect" : "Choose one downstream path by condition",
      controllerLabel: isParallel ? "Join member" : "Route member",
      controllerPlaceholderLabel: "— direct MobKit lanes —",
      controllerRole,
      memberOptions: sourceMembers.map((member) => ({
        value: member.id,
        label: `${member.name || member.role || member.id} · ${member.role || "profile"}`,
        member,
      })),
      emptyControllerHint: "Without a selected profile, MobKit conditions/parallel lanes attach directly to the first real member in each lane.",
      conditionOptions: basicConditionOptions(flow, step?.id, sourceMembers),
      branchConditionTitle: "Branch conditions",
      branchConditionIntro: "Read in order; the first match wins. Conditions read a member's structured output.",
      fallbackTitle: "Fallback",
      fallbackHint: "If none match, the flow follows the fallback path; else it stops.",
      addBranchLabel: isParallel ? "+ Add parallel branch" : "+ Add branch",
      dispatchLabel: "Dispatch mode",
      dispatchValue,
      dispatchOptions,
      selectedDispatch: dispatchOptions.find((option) => option.value === dispatchValue) || null,
      collectionLabel: "Collection policy (fan_in)",
      collectionValue,
      collectionOptions,
      selectedCollection: collectionOptions.find((option) => option.value === collectionValue) || null,
      showQuorum: collectionValue === "quorum",
      quorumLabel: "Quorum (N)",
      quorumPlaceholder: "required",
      dependencyLabel: "depends_on mode",
      dependencyValue,
      dependencyOptions,
      selectedDependency: dependencyOptions.find((option) => option.value === dependencyValue) || null,
    };
  }

  function basicForkCanvasState({ step, contract } = {}) {
    const isParallel = step?.type === "parallel";
    const collection = step?.collection || contractDefaultValue(contract, "collection_policy");
    const branches = Array.isArray(step?.branches) ? step.branches : [];
    const lanes = [
      ...branches.map((branch) => ({ id: branch.id, label: branch.label, steps: branch.steps || [] })),
      ...(isParallel ? [] : [{ id: "fallback", label: "Fallback", steps: step?.fallback || [] }]),
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

  function basicRepeatIterationLabel(step, members = []) {
    const iterationInput = typeof step?.iterationInput === "string" ? step.iterationInput.trim() : "";
    if (!iterationInput) return "runtime default";
    if (iterationInput === "carry") return "carries last output";
    if (iterationInput === "reuse") return "unsupported: re-use input task";
    const bodyStep = (Array.isArray(step?.steps) ? step.steps : []).find((candidate) => candidate?.id === iterationInput);
    const member = (Array.isArray(members) ? members : []).find((candidate) => candidate?.id === bodyStep?.role);
    return member ? `unsupported: feeds ${member.name}'s output` : `unsupported: ${iterationInput}`;
  }

  function basicRepeatCanvasState({ step, members = [], contract } = {}) {
    const defaultOperator = contractDefaultValue(contract, "condition_operator");
    const repeatUntilExpression = basicRepeatUntilExpression(step, members, { defaultOperator });
    return {
      repeatUntilExpression,
      conditionLabel: repeatUntilExpression || "…",
      maxIterationsLabel: step?.maxIterations ? `max ${step.maxIterations}` : "missing max_iterations",
      loopBackLabel: `↑ loop back · ${basicRepeatIterationLabel(step, members)}`,
      exitLabel: `↓ exit when ${repeatUntilExpression || "condition met"}`,
    };
  }

  function basicStepCardState({ step, members = [], contract } = {}) {
    const sourceMembers = Array.isArray(members) ? members : [];
    const member = step?.role ? sourceMembers.find((candidate) => candidate?.id === step.role) || null : null;
    if (step?.type === "input") {
      return {
        icon: "▤",
        iconTint: "member",
        title: "Input",
        desc: step?.task ? step.task : "the task this mob is run with",
        configured: true,
        isFlowCard: false,
      };
    }
    if (step?.type === "branch") {
      return {
        icon: "⑂",
        iconTint: "member",
        title: "Branch",
        desc: "Mob picks the first matching path",
        configured: true,
        isFlowCard: true,
      };
    }
    if (step?.type === "parallel") {
      const collection = step?.collection || contractDefaultValue(contract, "collection_policy") || "—";
      return {
        icon: "‖",
        iconTint: "member",
        title: "Parallel",
        desc: `fan-out → join · ${collection}`,
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
        title: "Repeat until",
        desc: repeatUntilExpression ? `until ${repeatUntilExpression}` : "loop body until condition",
        configured: true,
        isFlowCard: true,
      };
    }
    return {
      icon: "◆",
      iconTint: "accent",
      title: member ? member.name : "Select member",
      desc: step?.instruction || (member ? `${member.role} · ${member.model}` : ""),
      configured: !!step?.role,
      isFlowCard: false,
    };
  }

  function basicRepeatControlState({ step, members = [], schemas = [], contract } = {}) {
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
      { value: "", label: "runtime default", disabled: false, reason: "" },
      ...repeatIterationInputOptions(contract, iterationInputValue),
    ];
    return {
      panelIcon: "↻",
      panelTitle: "Repeat until",
      panelSub: "Loop the body, then evaluate the condition after each iteration",
      loopIdLabel: "loop_id",
      loopIdPlaceholder: "quality_loop",
      conditionTitle: "Until condition",
      conditionIntro: "Evaluated on a body member's structured output after each pass. The loop exits when it holds.",
      emptyBodyHint: "Add a member step inside the loop first — the condition reads its output schema.",
      memberPlaceholderLabel: "— member —",
      previewLabel: "until",
      previewFallback: "…",
      iterationInputLabel: "Iteration input — what each pass receives",
      maxIterationsLabel: "max_iterations",
      maxIterationsPlaceholder: "required",
      tips: [
        "The body is its own FrameSpec — add member steps inside the loop.",
        "The condition reads a member's typed output (e.g. reviewer.verdict == green).",
        "max_iterations bounds the loop so it always terminates.",
      ],
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
      fieldPlaceholder: condSchema ? "— field —" : "(no schema)",
      defaultOperator,
      operatorValue,
      operatorOptions: conditionOperatorOptions(contract, operatorValue),
      repeatUntilExpression,
      iterationInputValue,
      iterationInputOptions,
      selectedIterationInput: iterationInputOptions.find((option) => option.value === iterationInputValue) || null,
    };
  }

  function basicMemberStepControlState({ step, flow, members = [], contract } = {}) {
    const sourceMembers = Array.isArray(members) ? members : [];
    const memberById = new Map(sourceMembers.map((member) => [member.id, member]));
    const member = step?.role ? memberById.get(step.role) || null : null;
    const launchState = launchModeControlState(step, contract);
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
    const runtimeDefault = { value: "", label: "runtime default", disabled: false, reason: "" };
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
      panelTitle: member ? member.name : "Member step",
      panelSub: member ? `${member.role} · ${member.model}` : "Assign a member to run this step",
      memberFieldLabel: "Member (profile)",
      memberPlaceholderLabel: "— select member —",
      memberOptions: sourceMembers.map((candidate) => ({
        value: candidate.id,
        label: `${candidate.name} · ${candidate.role}`,
        member: candidate,
      })),
      launchState,
      launchSources,
      launchSourceOptions,
      firstLaunchSourceId: launchSourceOptions[0]?.value || "",
      instructionLabel: "message — instruction for this turn",
      instructionPlaceholder: "e.g. Run the focused tests and report failures.",
      dispatchLabel: "Dispatch mode",
      dispatchValue,
      dispatchOptions,
      selectedDispatch: dispatchOptions.find((option) => option.value === dispatchValue) || null,
      collectionLabel: "Collection policy",
      collectionValue,
      collectionOptions,
      selectedCollection: collectionOptions.find((option) => option.value === collectionValue) || null,
      quorumLabel: "Quorum",
      quorumPlaceholder: "required",
      timeoutLabel: "Timeout (ms)",
      timeoutPlaceholder: "runtime default",
      dependencyLabel: "depends_on mode",
      dependencyValue,
      dependencyOptions,
      selectedDependency: dependencyOptions.find((option) => option.value === dependencyValue) || null,
      outputFormatLabel: "Output format",
      outputValue,
      outputOptions,
      selectedOutput: outputOptions.find((option) => option.value === outputValue) || null,
      showQuorum: collectionValue === "quorum",
      allowedToolsLabel: "Allowed tools",
      allowedToolsEmptyLabel: "Runtime profile default",
      blockedToolsLabel: "Blocked tools",
      blockedToolsEmptyLabel: "No step-level blocks",
      schemaHint: member?.schema
        ? (() => {
          const tools = normalizeStringList(member.tools);
          const toolSummary = tools.join(", ") || "—";
          return {
            schema: member.schema,
            tools,
            toolSummary,
            parts: [
              { key: "prefix", text: "Emits " },
              { key: "schema", text: member.schema, kind: "code" },
              { key: "tools", text: ` · tools: ${toolSummary}` },
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
    return options.includeKind ? { kind: "cond", ...patch } : patch;
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
    return options.includeKind ? { kind: "cond", ...patch } : patch;
  }

  function graphEdgeKindPatch(edge, nextKind, options = {}) {
    const kind = String(nextKind || "").trim();
    if (kind !== "cond") {
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
      kind: "cond",
      ...graphEdgeConditionPatch(edge, options.conditionPatch || {}, {
        defaultOperator: options.defaultOperator,
        forceLabel: options.forceLabel,
      }),
    };
  }

  function graphEdgeFallbackPatch(edge, contract) {
    const kind = contractDefaultValue(contract, "graph_edge_kind");
    if (!kind) return null;
    return { kind, label: "fallback", cond: null };
  }

  function graphConnectionEdgeDraft({ from, to, edges, id, contract } = {}) {
    if (!from || !to || !from.id || !to.id || from.id === to.id) return null;
    if ((edges || []).some((edge) => edge.from === from.id && edge.to === to.id)) return null;

    const defaultKind = contractDefaultValue(contract, "graph_edge_kind");
    const fanoutKind = contractDefaultValue(contract, "graph_fanout_edge_kind");
    const conditionKind = contractDefaultValue(contract, "graph_condition_edge_kind");
    if (!defaultKind) return null;
    let kind = defaultKind;
    let label = "";

    if (to.isTerminal) {
      kind = defaultKind;
      label = "to " + String(to.label || "").toLowerCase();
    } else if (from.isGate && from.gateKind === "fork") {
      if (!fanoutKind) return null;
      kind = fanoutKind;
    } else if (to.isGate && to.gateKind === "join") {
      kind = defaultKind;
    } else if (to.col === from.col) {
      if (!fanoutKind) return null;
      kind = fanoutKind;
      label = "parallel";
    } else if (to.col < from.col) {
      if (!conditionKind) return null;
      kind = conditionKind;
      label = "rework";
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

  function graphTemplateInspectorState({ studio = {}, template = null, templateSeed = null } = {}) {
    const seed = templateSeed || { name: "untitled-mob", repo: "mob.toml", version: "draft", triggers: { labels: [], default: false } };
    const members = Array.isArray(studio.members) ? studio.members : [];
    const instances = Array.isArray(studio.instances) ? studio.instances : [];
    const edges = Array.isArray(studio.edges) ? studio.edges : [];
    const frames = Array.isArray(studio.frames) ? studio.frames : [];
    const labels = [template?.trigger || (Array.isArray(seed.triggers?.labels) ? seed.triggers.labels.join(", ") : "")];
    const placedMembers = new Set(instances.filter((instance) => instance?.memberId).map((instance) => instance.memberId)).size;
    return {
      name: template?.name || seed.name,
      repo: template?.repo || seed.repo,
      version: template?.version || seed.version,
      triggers: {
        labels,
        default: !!template?.defaultTrigger,
      },
      summaryRows: [
        { key: "members", label: "members", value: `${placedMembers} placed / ${members.length} in library` },
        { key: "instances", label: "instances", value: instances.filter((instance) => !instance?.isTerminal).length },
        { key: "terminals", label: "terminals", value: instances.filter((instance) => instance?.isTerminal).length },
        { key: "edges", label: "edges", value: edges.length },
        { key: "frames", label: "frames", value: frames.length },
      ],
    };
  }

  function graphInstanceControlState({ inst, instances = [], members = [], schemas = [] } = {}) {
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
      requiredLabel: field.required ? "req" : "",
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
      eyebrow: "INSTANCE",
      title: member ? member.name : "—",
      idLine: `${id} · cell (${col},${row})`,
      deleteLabel: "DELETE",
      memberTitle: member ? member.name : "—",
      memberRoleLabel: member ? `MEMBER · ${member.role}` : "",
      editMemberLabel: "EDIT MEMBER →",
      memberName: member?.name || "",
      memberSchemaLabel: member?.schema || "—",
      memberToolSummary,
      memberSummaryRows: [
        { key: "model", label: "model", value: member?.model || "—" },
        { key: "schema", label: "schema", value: member?.schema || "—" },
        { key: "tools", label: "tools", value: memberToolSummary },
      ],
      memberHint: "Editing the member updates every instance that uses it.",
      positionTitle: "POSITION",
      positionRows: [
        { key: "stage", label: "stage (col)", value: col },
        { key: "slot", label: "slot (row)", value: row },
      ],
      outputSchema,
      outputFields,
      outputTitle: `MEMBER OUTPUT · ${member?.schema || "—"}`,
      outputFieldRows,
      outputHint: "Defined on the member.",
      outputOpenMemberLabel: "Open member →",
      forkSourceOptions,
      firstForkSourceId: forkSourceOptions[0]?.value || "",
    };
  }

  function graphToolTagClass(toolId) {
    const id = String(toolId || "");
    if (id.startsWith("shell") || id === "git") return " is-shell";
    if (id.startsWith("mcp")) return " is-write";
    return "";
  }

  function graphNodeCanvasState({ inst, members = [], density = "" } = {}) {
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
        ariaLabel: isSourceFile ? "Open mob.toml read-only source editor" : undefined,
        sourceGlyph: isSourceFile ? "{ }" : "",
        roleLabel: isSourceFile ? "source file" : `terminal · ${inst.kind}`,
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

  function graphSourceFileNode({ instances = [] } = {}) {
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
      label: "mob.toml",
      col: minCol,
      row: minRow - 1,
    };
  }

  function graphCanvasInstances({ instances = [] } = {}) {
    const sourceInstances = Array.isArray(instances) ? instances : [];
    const sourceFileNode = graphSourceFileNode({ instances: sourceInstances });
    return sourceFileNode ? [sourceFileNode, ...sourceInstances] : sourceInstances;
  }

  function graphGateCanvasState({ inst, edges = [] } = {}) {
    const gateKind = String(inst?.gateKind || "");
    const glyph = gateKind === "fork" ? "‖"
      : gateKind === "join" ? "⋈"
        : gateKind === "branch" ? "⑂"
          : "•";
    let sublabel = inst?.label || gateKind;
    if (gateKind === "join" && inst?.collection === "quorum" && inst?.quorum) {
      const incoming = (Array.isArray(edges) ? edges : []).filter((edge) => edge.to === inst?.id).length;
      sublabel = `barrier · ${inst.quorum.n}/${incoming || inst.quorum.m}`;
    } else if (gateKind === "join" && inst?.collection) {
      sublabel = `join · ${inst.collection}`;
    }
    return { glyph, sublabel, gateKind };
  }

  function graphEdgeCanvasState({ edge, to, active = false, selected = false, edgeStyle = "" } = {}) {
    const kind = String(edge?.kind || "next").trim();
    const terminalTarget = !!to?.isTerminal;
    const labelText = String(edge?.label || edge?.kind || "");
    const isCondition = kind === "cond";
    const isFanout = kind === "fanout";
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

  function graphGateControlState(inst, { edges, members, contract } = {}) {
    const incoming = (edges || []).filter((edge) => edge.to === inst?.id);
    const outgoing = (edges || []).filter((edge) => edge.from === inst?.id);
    const defaultGateKind = contractDefaultValue(contract, "graph_gate_kind");
    const gateKind = String(inst?.gateKind || defaultGateKind || "").trim();
    const gateKindOptions = graphGateKindOptions(contract, gateKind);
    const collection = String(inst?.collection || (inst?.quorum?.n ? "quorum" : "")).trim();
    const collectionOptions = [
      { value: "", label: "runtime default", disabled: false, reason: "" },
      ...collectionPolicyOptions(contract, collection),
    ];
    const dispatch = String(inst?.dispatch || inst?.dispatchMode || "").trim();
    const dispatchOptions = [
      { value: "", label: "runtime default", disabled: false, reason: "" },
      ...dispatchModeOptions(contract, dispatch),
    ];
    const col = Number(inst?.col ?? 0);
    const row = Number(inst?.row ?? 0);
    return {
      incoming,
      outgoing,
      eyebrow: `GATE · ${gateKind}`,
      title: String(inst?.label || ""),
      idLine: `${inst?.id || ""} · cell (${col + 1},${row + 1})`,
      deleteLabel: "DELETE",
      labelTitle: "LABEL",
      kindTitle: "KIND",
      gateKind,
      gateKindOptions,
      selectedGateKind: gateKindOptions.find((option) => option.value === gateKind),
      collectionTitle: "COLLECTION POLICY",
      collection,
      collectionOptions,
      selectedCollection: collectionOptions.find((option) => option.value === collection),
      quorumIncomingLabel: `of ${incoming.length} incoming`,
      joinMemberLabel: "Join member",
      joinMemberPlaceholderOption: { value: "", label: "— select member —" },
      joinMemberHint: "MobKit uses this real profile to resolve non-all fan-in.",
      dispatchTitle: "DISPATCH MODE",
      dispatch,
      dispatchOptions,
      selectedDispatch: dispatchOptions.find((option) => option.value === dispatch),
      dispatchHint: "Exports as the MobKit parallel flow dispatch mode.",
      conditionsTitle: "CONDITIONS",
      emptyBranchHint: "add outgoing edges, then configure each as a typed condition or fallback",
      wiringTitle: "WIRING",
      incomingLabel: "incoming",
      outgoingLabel: "outgoing",
      firstMemberId: (members || []).find((member) => member?.id)?.id || "",
      memberOptions: (Array.isArray(members) ? members : [])
        .filter((member) => member?.id)
        .map((member) => ({
          value: member.id,
          label: `${member.name || member.id} · ${member.role || "profile"}`,
          member,
        })),
      incomingCount: incoming.length,
      outgoingCount: outgoing.length,
    };
  }

  function graphBranchConditionRows({ inst, edges = [], instances = [], members = [], schemas = [], flow, contract } = {}) {
    const sourceEdges = Array.isArray(edges) ? edges : [];
    const sourceInstances = Array.isArray(instances) ? instances : [];
    const sourceMembers = Array.isArray(members) ? members : [];
    const instanceById = new Map(sourceInstances.map((candidate) => [candidate.id, candidate]));
    const memberById = new Map(sourceMembers.map((candidate) => [candidate.id, candidate]));
    const defaultOperator = contractDefaultValue(contract, "condition_operator");
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
        });
        const condOwner = conditionOptions.find((option) => option.inst.id === condRef.instanceId) || null;
        const fields = condOwner?.fields || conditionOptions[0]?.fields || [];
        const condField = fields.find((field) => field.name === condRef.field) || null;
        const operatorValue = edge?.cond?.op || defaultOperator;
        return {
          edge,
          modeValue: edge?.kind === "cond" ? "cond" : "fallback",
          modeOptions: [
            { value: "cond", label: "condition" },
            { value: "fallback", label: "fallback" },
          ],
          targetPrefix: "→",
          target,
          targetLabel: target?.isTerminal
            ? target.label
            : (targetMember?.name || target?.label || "?"),
          condRef,
          conditionOptions,
          ownerOptions: conditionOptions.map((option) => ({
            value: option.inst.id,
            label: option.member.name,
            option,
          })),
          ownerValue: condRef.instanceId || conditionOptions[0]?.inst.id || "",
          firstOwnerId: conditionOptions[0]?.inst.id || "",
          fields,
          fieldOptions: fields.map((field) => ({
            value: field.name,
            label: `${field.name} · ${field.type}`,
            field,
          })),
          fieldValue: condRef.field || "",
          fieldPlaceholderOption: { value: "", label: "— field —" },
          condField,
          defaultOperator,
          operatorValue,
          operatorOptions: conditionOperatorOptions(contract, operatorValue),
          hasConditionOptions: conditionOptions.length > 0,
          noConditionOptionsHint: "add input params or an upstream schema field for this condition",
        };
      });
  }

  function graphTerminalControlState(inst, contract) {
    const defaultTerminalKind = contractDefaultValue(contract, "graph_terminal_kind");
    const terminalKind = String(inst?.kind || defaultTerminalKind || "").trim();
    const terminalKindOptions = graphTerminalKindOptions(contract, terminalKind);
    const id = String(inst?.id || "");
    const labelValue = String(inst?.label || "");
    const col = Number.isFinite(Number(inst?.col)) ? Number(inst.col) + 1 : 1;
    const row = Number.isFinite(Number(inst?.row)) ? Number(inst.row) + 1 : 1;
    return {
      eyebrow: `TERMINAL · ${terminalKind}`,
      title: labelValue,
      idLine: `${id} · cell (${col},${row})`,
      deleteLabel: "DELETE",
      labelTitle: "LABEL",
      labelValue,
      kindTitle: "KIND",
      terminalKind,
      terminalKindOptions,
      selectedTerminalKind: terminalKindOptions.find((option) => option.value === terminalKind) || null,
    };
  }

  function graphEdgeInspectorState({ edge, instances = [], members = [], schemas = [], flow, contract } = {}) {
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
    });
    const condOwner = conditionOptions.find((option) => option.inst.id === condRef.instanceId) || null;
    const fields = condOwner?.fields || conditionOptions[0]?.fields || [];
    const condField = fields.find((field) => field.name === condRef.field) || null;
    const defaultOperator = contractDefaultValue(contract, "condition_operator");
    const operatorValue = edge?.cond?.op || defaultOperator;
    const defaultEdgeKind = contractDefaultValue(contract, "graph_edge_kind");
    const edgeKind = String(edge?.kind || defaultEdgeKind || "").trim();
    const edgeKindOptions = graphEdgeKindOptions(contract, edgeKind);
    return {
      edge,
      fromInstance,
      toInstance,
      fromMember,
      toMember,
      eyebrow: `EDGE · ${edgeKind}`,
      title: `${fromMember?.name || fromInstance?.label || "—"} → ${toMember?.name || toInstance?.label || "—"}`,
      idLine: String(edge?.id || ""),
      deleteLabel: "DELETE",
      kindTitle: "KIND",
      labelTitle: "LABEL",
      conditionTitle: "CONDITION",
      noConditionOptionsHint: "Add an upstream agent with an output schema before configuring this edge.",
      ownerPlaceholderOption: { value: "", label: "— member —" },
      fromTitle: "FROM",
      toTitle: "TO",
      fromRows: [
        { key: "instance", label: "instance", value: fromInstance?.id || "" },
        { key: "member", label: "member", value: fromMember?.name || "—" },
        { key: "schema", label: "schema", value: fromMember?.schema || "—" },
      ],
      toRows: [
        { key: "instance", label: "instance", value: toInstance?.id || "" },
        { key: "member", label: "member", value: toMember?.name || (toInstance?.isTerminal ? "(terminal)" : "—") },
        { key: "schema", label: "schema", value: toMember?.schema || "—" },
      ],
      condRef,
      conditionOptions,
      condOwner,
      condField,
      ownerOptions: conditionOptions.map((option) => ({
        value: option.inst.id,
        label: option.member.name,
        option,
      })),
      ownerValue: condRef.instanceId || "",
      fields,
      fieldOptions: fields.map((field) => ({
        value: field.name,
        label: `${field.name} · ${field.type}`,
        field,
      })),
      fieldValue: condRef.field || "",
      fieldPlaceholder: condOwner ? "— field —" : "(no schema)",
      defaultOperator,
      operatorValue,
      operatorOptions: conditionOperatorOptions(contract, operatorValue),
      defaultEdgeKind,
      edgeKind,
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
    const count = Math.max(1, Number(incomingCount) || 0);
    return {
      collection: next,
      label: next ? `join · ${next}` : "join · missing collection",
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

  function buildDocument({ flow, studio, currentFlow, deploySettings }) {
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
      instances: instancesForDocument(documentFlow, members, studio?.instances || studio?.nodes || []),
      edges: edgesForDocument(documentFlow, members, studio?.edges || []),
      frames: framesForDocument(documentFlow, members, studio?.frames || []),
      schemas,
      skill_realms: skillRealmsForDocument(members, studio?.skillRealms),
      flow: documentFlow,
      launch_modes: launchModesFromFlow(documentFlow, members),
      deploy,
      deploy_command: deploy.command,
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

  function edgesForDocument(flow, members, existingEdges) {
    const projected = graphProjectionForFlow(flow, members).edges || [];
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
    const kind = String(edge?.kind || "next").trim() || "next";
    return from && to ? `${from}\n${to}\n${kind}` : "";
  }

  function instancesForDocument(flow, members, existingInstances) {
    const projected = graphProjectionForFlow(flow, members).instances || [];
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

  function graphProjectionForFlow(flow, members) {
    const projection = { instances: [], edges: [], frames: [] };
    const edgeId = () => `e${projection.edges.length + 1}`;

    function connectEdges(fromIds, toIds, kind = "next", label = "", extra = {}) {
      for (const from of fromIds || []) {
        for (const to of toIds || []) {
          if (!from || !to) continue;
          projection.edges.push({ id: edgeId(), from, to, kind, label, ...extra });
        }
      }
    }

    function emit(steps, startCol, row = 0, initialPrevExits = [], entryKind = "next", entryLabel = "", lane = "") {
      let col = startCol;
      let prevExits = initialPrevExits || [];
      let entries = [];
      let firstConnection = true;
      const rememberEntries = (ids) => {
        if (!entries.length) entries = (ids || []).filter(Boolean);
      };
      const connectPrev = (targets, extra = {}) => {
        const kind = firstConnection ? entryKind : "next";
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
          const collection = isBranch ? "" : collectionModeFromStepSource(step);
          projection.instances.push({
            id: gateId,
            isGate: true,
            gateKind: isBranch ? "branch" : "fork",
            label: isBranch ? "branch" : dispatch,
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
              ? [{ id: "fallback", label: "Fallback", steps: step.fallback }]
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
              isFallback ? "next" : isBranch ? "cond" : "fanout",
              isFallback ? "fallback" : isBranch ? (branch.condition || "") : "",
              isFallback ? "fallback" : "",
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
            label: isBranch ? "join · branch paths" : `join · ${collection || "missing collection"}`,
            collection,
            controllerRole: step.controllerRole || step.controllerMemberId || step.controlRole || "",
            quorum: !isBranch && collection === "quorum"
              ? { mode: "NofM", n: numberOrNull(step.quorum) || 2, m: Math.max(1, lanes.length) }
              : undefined,
            col: maxCol,
            row,
          });
          connectEdges(exits, [joinId], "next", "");
          projection.frames.push({
            id: `frame_${step.type}_${step.id}`,
            kind: isBranch ? "Branch" : "Parallel",
            colStart: gateCol,
            colEnd: maxCol,
            label: isBranch
              ? `BRANCH · ${lanes.length} path${lanes.length === 1 ? "" : "s"}`
              : parallelFrameLabel(dispatch, collection),
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
            firstConnection ? entryKind : "next",
            firstConnection ? entryLabel : "",
            lane,
          );
          rememberEntries(loopProjection.entries);
          firstConnection = false;
          const cond = repeatCondToGraphCond(step.cond, loopProjection.exits[0]);
          connectEdges(
            loopProjection.exits,
            loopProjection.entries,
            "cond",
            step.until ? `until ${step.until}` : "until condition",
            cond ? { cond } : {},
          );
          if (loopProjection.entries.length) {
            projection.frames.push({
              id: `frame_${step.id}`,
              kind: "RepeatUntil",
              colStart: frameStart,
              colEnd: Math.max(frameStart, loopProjection.nextCol - 1),
              label: repeatFrameLabel(step),
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

  function parallelFrameLabel(dispatch, collection) {
    const dispatchLabel = dispatch || "missing dispatch";
    const collectionLabel = collection || "missing collection";
    return `PARALLEL · ${dispatchLabel} · join ${collectionLabel}`;
  }

  function repeatFrameLabel(step) {
    const max = Number(step?.maxIterations ?? step?.max_iterations);
    return Number.isInteger(max) && max > 0
      ? `REPEAT-UNTIL · max ${max}`
      : "REPEAT-UNTIL · missing max_iterations";
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

  function framesForDocument(flow, members, existingFrames) {
    const projected = graphProjectionForFlow(flow, members).frames || [];
    const required = requiredFramesFromFlow(flow);
    const canonicalFrames = new Map();
    for (const frame of [...projected, ...required]) {
      if (frame?.id && !canonicalFrames.has(String(frame.id))) canonicalFrames.set(String(frame.id), frame);
    }
    const byId = new Map();
    for (const frame of existingFrames || []) {
      const id = String(frame?.id || "");
      const canonical = canonicalFrames.get(id);
      if (id && canonical) {
        byId.set(id, {
          ...canonical,
          colStart: frame.colStart ?? canonical.colStart,
          colEnd: frame.colEnd ?? canonical.colEnd,
          rowStart: frame.rowStart ?? canonical.rowStart,
          rowEnd: frame.rowEnd ?? canonical.rowEnd,
        });
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

  function requiredFramesFromFlow(flow) {
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
            label: `BRANCH · ${(step.branches || []).length + (Array.isArray(step.fallback) && step.fallback.length ? 1 : 0)} paths`,
          });
        } else if (step.type === "parallel") {
          const dispatch = dispatchModeFromStepSource(step);
          const collection = collectionModeFromStepSource(step);
          frames.push({
            id: `frame_parallel_${step.id}`,
            kind: "Parallel",
            colStart: 0,
            colEnd: 0,
            label: parallelFrameLabel(dispatch, collection),
          });
        } else if (step.type === "repeat") {
          frames.push({
            id: `frame_${step.id}`,
            kind: "RepeatUntil",
            colStart: 0,
            colEnd: 0,
            label: repeatFrameLabel(step),
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

  function graphStructureSignature(instances, edges) {
    return graphSignatureFor(instances, edges, { includeLayout: false });
  }

  function graphSignatureFor(instances, edges, { includeLayout }) {
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
        kind: edge.kind || "next",
        label: edge.label || "",
        cond: edge.cond || null,
      }))
      .sort((a, b) => a.id.localeCompare(b.id));
    return JSON.stringify({ nodes, links });
  }

  function graphToFlow({ instances, edges, members, previousFlow }) {
    const prior = previousFlow || {};
    const inputStep = (prior.steps || []).find((step) => step.type === "input") || {
      id: uniqueFlowStepId("input", prior),
      type: "input",
      task: "Run the mobpack flow.",
      fields: "",
      inputParams: [],
    };
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
      if ((edge.kind || "") !== "cond") return false;
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

  function flowStepForGraphGroup(nodes, edges, members, priorStepById) {
    if (nodes.length === 1) return memberStepFromInstance(nodes[0], members, priorStepById);
    const incoming = new Map();
    for (const node of nodes) {
      incoming.set(node.id, (edges || []).filter((edge) => edge.to === node.id));
    }
    const hasConditionalFanIn = nodes.some((node) => (incoming.get(node.id) || []).some((edge) => (edge.kind || "") === "cond"));
    if (hasConditionalFanIn) {
      const id = `branch_${nodes.map((node) => node.id).join("_")}`;
      const prior = priorStepById.get(id) || {};
      const dependsMode = dependencyModeFromStepSource(prior);
      const out = {
        id,
        type: "branch",
        controllerRole: prior.controllerRole || prior.controllerMemberId || prior.controlRole || "",
        branches: nodes.map((node, index) => {
          const edge = (incoming.get(node.id) || []).find((candidate) => (candidate.kind || "") === "cond");
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

  function graphSegmentsToFlowSteps({ instances, edges, members, priorStepById }) {
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
          && ((edge.kind || "") !== "cond" || String(edge.label || "").toLowerCase() === "fallback" || String(node.lane || "").toLowerCase() === "fallback");
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
                controllerRole: prior.controllerRole || prior.controllerMemberId || prior.controlRole || "",
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
      step: flowStepForGraphGroup(group.nodes, edges, members, priorStepById),
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

  function launchModeControlState(source, contract) {
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
    const launchOptions = launchModeOptions(contract, launchKind);
    const budgetOptions = budgetSplitPolicyOptions(contract, budgetSplitPolicy.kind);
    const defaultForkContext = contractDefaultValue(contract, "fork_context");
    const forkContextValue = normalizeForkContext(launchMode.context || defaultForkContext);
    const forkOptions = forkContextOptions(contract, forkContextValue);
    const fixedLimitValue = budgetSplitPolicy.limit || 4096;
    return {
      launchTitle: "Launch mode",
      graphLaunchTitle: "LAUNCH MODE · this position",
      resumeSessionLabel: "Bridge session",
      resumeSessionPlaceholder: "session id",
      forkSourceLabel: "Fork from",
      forkContextLabel: "Fork context",
      graphForkContextLabel: "Context",
      budgetPolicyLabel: "Budget split policy",
      fixedBudgetLabel: "Fixed token budget",
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

  function launchModeOptions(contract, currentKind) {
    const contractModes = Array.isArray(contract?.mob_definition?.launch_modes) && contract.mob_definition.launch_modes.length
      ? contract.mob_definition.launch_modes.map(canonicalLaunchModeKind)
      : [];
    const modes = [...contractModes];
    const currentSource = currentKind || contractDefaultValue(contract, "launch_mode");
    const current = currentSource ? canonicalLaunchModeKind(currentSource) : "";
    if (current && !modes.includes(current)) modes.push(current);
    const labels = {
      Fresh: "Fresh — empty context",
      Resume: "Resume — existing bridge session",
      Fork: "Fork — copy context from another step",
    };
    return modes.map((mode) => {
      const supported = contractModes.includes(mode);
      return {
        value: mode,
        label: labels[mode] || `${mode} — not in MobKit launch_modes`,
        disabled: !supported,
        reason: supported ? "" : "Unsupported by the MobKit launch_modes contract.",
      };
    });
  }

  function normalizeDispatchMode(mode) {
    return String(mode || "").trim();
  }

  function dispatchModeOptions(contract, currentMode) {
    const contractModes = Array.isArray(contract?.mob_definition?.dispatch_modes) && contract.mob_definition.dispatch_modes.length
      ? contract.mob_definition.dispatch_modes.map(String)
      : [];
    const modes = [...contractModes];
    const current = String(currentMode || contractDefaultValue(contract, "dispatch_mode") || "").trim();
    if (!modes.includes(current)) modes.push(current);
    const labels = {
      fan_out: "fan_out — broadcast to every lane",
      one_to_one: "one_to_one — pair inputs with lanes",
      fan_in: "fan_in — gather upstream outputs",
    };
    return modes.map((mode) => {
      const supported = contractModes.includes(mode);
      return {
        value: mode,
        label: labels[mode] || `${mode} — not in MobKit dispatch_modes`,
        disabled: !supported,
        reason: supported ? "" : "Unsupported by the MobKit dispatch_modes contract.",
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
    const contractModes = Array.isArray(contract?.mob_definition?.dependency_modes) && contract.mob_definition.dependency_modes.length
      ? contract.mob_definition.dependency_modes.map(String)
      : [];
    const modes = [...contractModes];
    const current = String(currentMode || contractDefaultValue(contract, "dependency_mode") || "").trim();
    if (!modes.includes(current)) modes.push(current);
    const labels = {
      all: "all — every upstream node",
      any: "any — any upstream node",
    };
    return modes.map((mode) => {
      const supported = contractModes.includes(mode);
      return {
        value: mode,
        label: labels[mode] || `${mode} — not in MobKit dependency_modes`,
        disabled: !supported,
        reason: supported ? "" : "Unsupported by the MobKit dependency_modes contract.",
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
    const contractPolicies = Array.isArray(contract?.mob_definition?.collection_policies) && contract.mob_definition.collection_policies.length
      ? contract.mob_definition.collection_policies.map(String)
      : [];
    const policies = [...contractPolicies];
    const current = String(currentPolicy || contractDefaultValue(contract, "collection_policy") || "").trim();
    if (!policies.includes(current)) policies.push(current);
    const labels = {
      all: "all — wait for every branch",
      any: "any — accept the first completed branch",
      quorum: "quorum — require N branches",
    };
    return policies.map((policy) => {
      const supported = contractPolicies.includes(policy);
      return {
        value: policy,
        label: labels[policy] || `${policy} — not in MobKit collection_policies`,
        disabled: !supported,
        reason: supported ? "" : "Unsupported by the MobKit collection_policies contract.",
      };
    });
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
    const rawKind = String(policy.kind || policy.type || "").trim().toLowerCase();
    if (!rawKind) return null;
    if (rawKind === "fixed") {
      const limit = numberOrNull(policy?.limit ?? policy?.value ?? policy?.tokens);
      return { kind: "Fixed", limit: limit && limit > 0 ? limit : 4096 };
    }
    if (rawKind === "proportional") return { kind: "Proportional" };
    if (rawKind === "remaining") return { kind: "Remaining" };
    return { kind: "Equal" };
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

  function budgetSplitPolicyOptions(contract, currentKind) {
    const contractPolicies = Array.isArray(contract?.mob_definition?.budget_split_policies) && contract.mob_definition.budget_split_policies.length
      ? contract.mob_definition.budget_split_policies.map(canonicalBudgetSplitPolicyKind)
      : [];
    const policies = [...contractPolicies];
    const currentSource = currentKind || contractDefaultValue(contract, "budget_split_policy");
    const current = currentSource ? canonicalBudgetSplitPolicyKind(currentSource) : "";
    if (current && !policies.includes(current)) policies.push(current);
    const labels = {
      Equal: "Equal — split remaining budget evenly",
      Proportional: "Proportional — MobKit proportional split",
      Remaining: "Remaining — grant all remaining budget",
      Fixed: "Fixed — token cap for this spawn",
    };
    return policies.map((policy) => {
      const supported = contractPolicies.includes(policy);
      return {
        value: policy,
        label: labels[policy] || `${policy} — not in MobKit budget_split_policies`,
        disabled: !supported,
        reason: supported ? "" : "Unsupported by the MobKit budget_split_policies contract.",
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
    return callRpc(RPC_METHODS.schema, {});
  }

  async function validateDocument(document) {
    return callRpc(RPC_METHODS.validate, { document });
  }

  async function exportDocument(document) {
    return callRpc(RPC_METHODS.export, { document });
  }

  async function deployDocument(document, options) {
    return callRpc(RPC_METHODS.deploy, { document, ...(options || {}) });
  }

  async function deployCommandPreview(settings, options) {
    const deploy = normalizeDeploySettings(settings);
    return callRpc(RPC_METHODS.deployCommand, {
      deploy,
      pack_path: options?.packPath || "<pack.mobpack>",
      prompt: options?.prompt || deploy.prompt || "<prompt>",
    });
  }

  async function importDocument(params) {
    return callRpc(RPC_METHODS.import, params || {});
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

  function modelCatalogFromSchema(schema) {
    return (schema?.models || [])
      .filter((model) => model && typeof model === "object" && model.id && model.label && (model.vendor || model.provider))
      .map((model) => ({
        id: String(model.id),
        label: String(model.label),
        vendor: String(model.vendor || model.provider),
        profile: model.profile || null,
      }));
  }

  function toolCatalogFromSchema(schema) {
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
      validationSource: "",
      contractMeta: {
        loaded: false,
        schemaVersion: "",
        mediaType: "",
        validationSource: "",
      },
      grid: boot.grid || null,
      cellXY: boot.cellXY || null,
      template: boot.template || null,
    };
  }

  function mobKitCatalogsFromSchema(schema, boot = {}) {
    const agentDefinitions = agentDefinitionsFromSchema(schema);
    return {
      models: modelCatalogFromSchema(schema),
      toolCatalog: toolCatalogFromSchema(schema),
      agentDefinitions,
      skillRealms: schemaSkillRealms(schema),
      blankMobpack: blankMobpackFromSchema(schema),
      deployDefaults: deployDefaultsFromSchema(schema),
      mobDefaults: mobDefaultsFromSchema(schema),
      mobDefinition: schema?.mob_definition || null,
      validationSource: schema?.validation_source || "",
      contractMeta: {
        loaded: true,
        schemaVersion: schema?.schema_version || "",
        mediaType: schema?.media_type || "",
        validationSource: schema?.validation_source || "",
      },
      grid: boot.grid || null,
      cellXY: boot.cellXY || null,
      template: boot.template || null,
    };
  }

  function schemaSkillRealms(schema) {
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
    const name = String(document?.name || document?.mob_id || "untitled-mob").trim() || "untitled-mob";
    return {
      name,
      steps: [{
        id: "input_1",
        type: "input",
        task: "",
        fields: "",
        inputParams: [],
      }],
    };
  }

  function graphProjectionForDocument(document, members) {
    const storedFrames = Array.isArray(document?.frames) ? document.frames : [];
    const hasStoredEditorGraph = storedFrames.length > 0;
    if (!hasStoredEditorGraph && document?.flow && Array.isArray(document.flow.steps)) {
      return graphProjectionForFlow(document.flow, members || []);
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
    const flow = flowFromHydratedDocument(document);
    const skillRealms = mergeSkillRealms(document.skill_realms, options.contractSkillRealms || []);
    const graphProjection = graphProjectionForDocument({ ...document, flow }, members);
    const hasDeploySettings = document.deploy && typeof document.deploy === "object" && !Array.isArray(document.deploy);
    const hasMobSettings = document.mob_settings && typeof document.mob_settings === "object" && !Array.isArray(document.mob_settings);
    const id = String(options.id || "f_imported");
    const validation = result?.validation || null;
    const validationRows = diagnosticsToRows(validation);
    const stage = validation?.ok ? "valid" : "draft";
    const flowName = document.name || document.mob_id || document.flow?.name || "imported-mob";
    const registryRow = flowRegistryRowFromDocument({
      id,
      document,
      validation,
      stage,
      sourceLabel: result?.source_label || "",
      source: result?.source || "",
      flowRow: options.flowRow || null,
      fallbackName: flowName,
      fallbackVersion: "imported",
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

  function runtimeModeOptions(contract, deploySettings, currentMode) {
    const modes = Array.isArray(contract?.mob_definition?.runtime_modes) && contract.mob_definition.runtime_modes.length
      ? contract.mob_definition.runtime_modes.map(String)
      : [];
    const current = String(currentMode || "");
    if (current && !modes.includes(current)) modes.push(current);
    const surface = String(deploySettings?.surface || contract?.deploy_settings?.defaults?.surface || "");
    const labels = {
      turn_driven: "turn_driven — explicit turn dispatch",
      autonomous_host: "autonomous_host — RPC keep-alive member loop",
    };
    return modes.map((mode) => {
      const cliBlocked = surface === "cli" && mode === "autonomous_host";
      return {
        value: mode,
        label: labels[mode] || mode,
        disabled: cliBlocked,
        reason: cliBlocked ? "RPC surface only; rkat mob deploy requires turn_driven profiles." : "",
      };
    });
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

  function profileBackendOptions(contract, currentBackend, includeDefault) {
    const options = simpleContractOptions(
      contract?.mob_definition?.profile_backends,
      currentBackend || "",
      { session: "session", external: "external" },
      "mob_definition.profile_backends"
    );
    if (!includeDefault) return options;
    return [{ value: "", label: "definition default", disabled: false, reason: "" }, ...options.filter(option => option.value)];
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
  } = {}) {
    const loadableFlowOptions = (Array.isArray(flows) ? flows : [])
      .filter((flow) => flow?.document)
      .map((flow) => ({
        value: flow.id,
        label: `${flow.name} · ${flow.stage || flow.source || "draft"}`,
      }));
    const profileOptions = [
      { value: "", label: "none" },
      ...(Array.isArray(members) ? members : []).map((member) => {
        const profile = profileName(member);
        return { value: profile, label: profile };
      }),
    ];
    const modelOptions = [
      { value: "", label: "default" },
      ...(Array.isArray(modelCatalog) ? modelCatalog : []).map((model) => ({
        value: model.id,
        label: `${model.label || model.id} · ${model.vendor || "provider"}`,
      })),
    ];
    return {
      panelTitle: "Tweaks",
      loadMobTitle: "Load mob",
      loadMobLabel: "Mobpack",
      canvasTitle: "Canvas",
      edgeStyleLabel: "Edges",
      edgeStyleOptions: [
        { value: "text", label: "Text" },
        { value: "icons", label: "Icons" },
        { value: "colored", label: "Color" },
      ],
      densityLabel: "Density",
      densityOptions: [
        { value: "compact", label: "Compact" },
        { value: "comfortable", label: "Comfy" },
      ],
      themeTitle: "Theme",
      themeModeLabel: "Mode",
      themeModeOptions: [
        { value: "light", label: "Light" },
        { value: "dark", label: "Dark" },
      ],
      mobTitle: "Mob",
      orchestratorLabel: "Orchestrator",
      autoWireLabel: "Auto wire",
      autoWireOptions: [
        { value: "no", label: "No" },
        { value: "yes", label: "Yes" },
      ],
      defaultBackendLabel: "Default backend",
      externalBaseLabel: "External base",
      externalBasePlaceholder: "http://127.0.0.1:9000",
      deployTitle: "Deploy",
      surfaceLabel: "Surface",
      trustLabel: "Trust",
      modelLabel: "Model",
      durationLabel: "Duration",
      durationPlaceholder: "30s",
      toolCallsLabel: "Tool calls",
      toolCallsMin: 0,
      toolCallsMax: 999,
      tokensLabel: "Tokens",
      tokensMin: 0,
      tokensMax: 200000,
      realmLabel: "Realm",
      realmOptions: [
        { value: "isolated", label: "Isolated" },
        { value: "shared", label: "Shared" },
      ],
      realmIdLabel: "Realm ID",
      realmIdPlaceholder: "realm id",
      backendLabel: "Backend",
      promptLabel: "Prompt",
      promptPlaceholder: "Deploy prompt",
      commandLabel: "Command",
      commandFallback: "--",
      inspectorTitle: "Inspector",
      inspectorLayoutLabel: "Layout",
      inspectorLayoutOptions: [
        { value: "right", label: "Right" },
        { value: "bottom", label: "Bottom" },
        { value: "modal", label: "Modal" },
      ],
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

  function forkContextOptions(contract, currentContext) {
    const currentSource = currentContext || contractDefaultValue(contract, "fork_context");
    const current = currentSource ? normalizeForkContext(currentSource) : "";
    return simpleContractOptions(
      contract?.mob_definition?.fork_contexts,
      current,
      {
        full_history: "full_history — entire transcript",
        last_messages: "last_messages — last N messages",
        FullHistory: "FullHistory — legacy alias for full_history",
      },
      "mob_definition.fork_contexts"
    );
  }

  function graphGateKindOptions(contract, currentKind) {
    return simpleContractOptions(
      contract?.mob_definition?.graph_gate_kinds,
      currentKind || contractDefaultValue(contract, "graph_gate_kind"),
      {
        branch: "branch — conditional split",
        fork: "fork — fan out in parallel",
        join: "join — wait for branches",
      },
      "mob_definition.graph_gate_kinds"
    );
  }

  function graphTerminalKindOptions(contract, currentKind) {
    return simpleContractOptions(
      contract?.mob_definition?.graph_terminal_kinds,
      currentKind || contractDefaultValue(contract, "graph_terminal_kind"),
      {
        success: "success — done",
        failed: "failed — blocked",
        human: "human — needs human",
      },
      "mob_definition.graph_terminal_kinds"
    );
  }

  function graphFrameKindOptions(contract, currentKind) {
    return simpleContractOptions(
      contract?.mob_definition?.graph_frame_kinds,
      currentKind || contractDefaultValue(contract, "graph_frame_kind"),
      {
        Branch: "Branch — conditional flow frame",
        Parallel: "Parallel — concurrent flow frame",
        RepeatUntil: "RepeatUntil — bounded loop frame",
      },
      "mob_definition.graph_frame_kinds"
    );
  }

  function graphEdgeKindOptions(contract, currentKind) {
    return simpleContractOptions(
      contract?.mob_definition?.graph_edge_kinds,
      currentKind || contractDefaultValue(contract, "graph_edge_kind"),
      {
        next: "next — sequential handoff",
        fanout: "fanout — parallel sibling",
        cond: "cond — guarded branch",
      },
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

  function editorFlowPrimitiveOptions(contract) {
    const stepTypes = Array.isArray(contract?.mob_definition?.editor_flow_step_types) && contract.mob_definition.editor_flow_step_types.length
      ? contract.mob_definition.editor_flow_step_types.map(String)
      : [];
    const metadata = {
      repeat: { id: "repeat", glyph: "↻", tint: "member", label: "Repeat until", sub: "Loop a body of steps until a condition holds (max_iterations)" },
      branch: { id: "branch", glyph: "⑂", tint: "member", label: "Branch", sub: "Pick one downstream path by condition (first match wins)" },
      parallel: { id: "parallel", glyph: "‖", tint: "member", label: "Parallel", sub: "fan_out to several members, then fan_in with a collection policy" },
    };
    return stepTypes
      .filter((type) => metadata[type])
      .map((type) => metadata[type]);
  }

  function graphControlNodes(contract) {
    const glyphs = { branch: "⑂", fork: "‖", join: "⋈" };
    const labels = { branch: "Branch gate", fork: "Parallel fork", join: "Join gate" };
    const metas = { branch: "conditional split", fork: "fan_out lanes", join: "fan_in barrier" };
    const paletteKinds = Array.isArray(contract?.mob_definition?.graph_palette_gate_kinds)
      ? contract.mob_definition.graph_palette_gate_kinds.map(String)
      : [];
    return graphGateKindOptions(contract, "")
      .filter((option) => !option.disabled && paletteKinds.includes(option.value))
      .map((option) => ({
        id: option.value,
        gateKind: option.value,
        glyph: glyphs[option.value] || "•",
        label: labels[option.value] || option.value,
        meta: metas[option.value] || "MobKit graph gate",
      }));
  }

  function graphAddNodeMenuState({ members = [], contract = null, query = "" } = {}) {
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
      ? graphControlNodes(contract)
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
      searchIcon: "⌕",
      searchPlaceholder: "Add a node…",
      closeLabel: "✕",
      closeTitle: "Close",
      agentsLabel: "Agents",
      controlsLabel: "Flow controls",
      emptyLabel: `No matches for “${q}”`,
      jumpLabel: "+ New agent in Agents →",
      memberRows,
      controlRows,
      hasMembers: memberRows.length > 0,
      hasControls: controlRows.length > 0,
      isEmpty: memberRows.length === 0 && controlRows.length === 0,
    };
  }

  function basicStepPickerState({ members = [], contract = null, query = "", isKickoff = false } = {}) {
    if (isKickoff) {
      return {
        mode: "kickoff",
        title: "Input",
        sub: "Every mob run starts from a single task input",
        kickoffHint: "This node is the mob's ingress — the task it's deployed/run with. Select it on the canvas to edit the task and any typed input fields.",
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
    const primitiveRows = editorFlowPrimitiveOptions(contract)
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
      title: "Add step",
      sub: "A flow node — a member turn or a flow primitive",
      searchIcon: "⌕",
      searchPlaceholder: "Search members & primitives…",
      membersLabel: "Mob members",
      flowLabel: "Flow",
      emptyMembersHint: "No members yet — define some in the Agents tab.",
      newBadgeLabel: "NEW",
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

  function graphControlShape({ gateKind, at, members, instances, edges, flow, contract } = {}) {
    const kind = String(gateKind || "").trim();
    if (kind !== "branch" && kind !== "fork") return null;
    const allowed = new Set(graphControlNodes(contract).map((node) => node.gateKind));
    if (!allowed.has(kind)) return null;
    const sourceMembers = Array.isArray(members) ? members : [];
    if (!at || sourceMembers.length === 0) return null;

    const launchKind = contractDefaultValue(contract, "launch_mode");
    const nextEdgeKind = contractDefaultValue(contract, "graph_edge_kind");
    if (!launchKind || !nextEdgeKind) return null;

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
    const collection = contractDefaultValue(contract, "collection_policy");
    if (!collection) return null;
    const dispatch = isBranch ? "" : contractDefaultValue(contract, "dispatch_mode");
    if (!isBranch && !dispatch) return null;

    const instancesOut = [
      {
        id: gateId,
        isGate: true,
        gateKind: kind,
        label: isBranch ? "branch" : dispatch,
        dispatch: isBranch ? undefined : dispatch,
        col: cells.gate.col,
        row: cells.gate.row,
      },
      {
        id: leftId,
        memberId: memberA.id,
        col: cells.laneA.col,
        row: cells.laneA.row,
        lane: isBranch ? "condition" : "lane 1",
        launchMode: { kind: launchKind },
      },
      {
        id: rightId,
        memberId: memberB.id,
        col: cells.laneB.col,
        row: cells.laneB.row,
        lane: isBranch ? "fallback" : "lane 2",
        launchMode: { kind: launchKind },
      },
      {
        id: joinId,
        isGate: true,
        gateKind: "join",
        label: isBranch ? "join · branch paths" : `join · ${collection}`,
        collection,
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
        { id: `e_${gateId}_${rightId}`, from: gateId, to: rightId, kind: nextEdgeKind, label: "fallback" },
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
        branches: [{ id: reserveFlowBranchId("br", branchIds), label: "Branch 1", condition: "", steps: [] }],
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
          { id: reserveFlowBranchId("br", branchIds), label: "Branch 1", steps: [] },
          { id: reserveFlowBranchId("br", branchIds), label: "Branch 2", steps: [] },
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

  function mobRoleWiringEditorState(value, profileOptions) {
    const options = Array.isArray(profileOptions) ? profileOptions : [];
    const wiring = normalizeRoleWiring(value);
    return {
      label: "Role wiring",
      countLabel: String(wiring.length),
      addLabel: "+ rule",
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

  function advancedMobSettingsEditorState(value) {
    return {
      label: "Advanced",
      text: JSON.stringify(value || {}, null, 2),
    };
  }

  function advancedMobSettingsDraftPatch(text) {
    try {
      const parsed = String(text || "").trim() ? JSON.parse(String(text)) : {};
      if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
        return { ok: false, error: "object required", value: null };
      }
      return { ok: true, error: "", value: normalizeMobSettings({ advanced: parsed }).advanced };
    } catch (err) {
      return { ok: false, error: err?.message || "invalid JSON", value: null };
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
      return validation.display_rows.map((row) => ({
        kind: row.kind || "warn",
        glyph: row.glyph || (row.kind === "crit" ? "!" : row.kind === "ok" ? "✓" : "△"),
        head: row.head || "",
        sub: row.sub || "",
        meta: row.meta || "",
      }));
    }
    return [];
  }

  function deployResultToRows(result) {
    if (Array.isArray(result?.display_rows)) {
      return result.display_rows.map((row) => ({
        kind: row.kind || "warn",
        glyph: row.glyph || (row.kind === "crit" ? "!" : row.kind === "ok" ? "✓" : "△"),
        head: row.head || "",
        sub: row.sub || "",
        meta: row.meta || "",
      }));
    }
    return [];
  }

  function validationSheetState(results, options = {}) {
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
      eyebrow: "VALIDATE · MobKit",
      title: `${counts.ok} passed · ${counts.warn} warnings · ${counts.crit} blocking`,
      publishLabel: "PUBLISH",
      deployPlanLabel: "DEPLOY PLAN",
      deployLabel: "DEPLOY",
      closeLabel: "×",
      actionsDisabled: counts.crit > 0 || stageBlocksActions,
    };
  }

  function deployPlanTraceState(document, plan) {
    const steps = Array.isArray(plan?.plan_trace) && plan.plan_trace.length
      ? plan.plan_trace
      : [{
        node: null,
        head: "DEPLOY TRACE UNAVAILABLE",
        body: "mobkit/mobpacks/deploy did not return plan_trace.",
      }];
    const title = document?.mob_id || document?.name || "mobkit_flow";
    const subtitle = plan?.command || "";
    const packLabel = plan?.pack_path || "";
    return {
      steps,
      eyebrow: "DEPLOY PLAN",
      title,
      subtitle,
      packLabel,
      firstLabel: "first",
      closeLabel: "×",
      stepLabel: "step",
      previousLabel: "‹",
      nextLabel: "›",
    };
  }

  function topRailState({ contract, deploySettings, stage, view, theme } = {}) {
    const inEditor = view === "editor";
    const contractState = contract?.error ? "api error" : contract ? "api ready" : "loading";
    const deployCommand = contract?.deploy_settings?.command || "";
    const deploySurface = deploySettings?.surface || contract?.deploy_settings?.surfaces?.[0] || "";
    const deployActionsDisabled = stage !== "valid";
    const nextTheme = theme === "dark" ? "light" : "dark";
    return {
      inEditor,
      brandLabel: "MobKit · Flow Editor",
      flowsTabLabel: "FLOWS",
      agentsTabLabel: "AGENTS",
      mobStatusTitle: "Active mob configuration",
      mobFileLabel: "mob.toml",
      contractState,
      deployPrefixLabel: "deploy:",
      deployCommand,
      deploySurface,
      flowsCrumbLabel: "flows",
      crumbSeparator: "/",
      planTraceLabel: "PLAN TRACE",
      importLabel: "IMPORT",
      validateLabel: "VALIDATE",
      publishLabel: "PUBLISH",
      deployPlanLabel: "DEPLOY PLAN",
      deployLabel: "DEPLOY",
      deployActionsDisabled,
      themeToggleTitle: `Switch to ${nextTheme} mode`,
      themeToggleLabel: nextTheme === "light" ? "☾ dark" : "☀ light",
      basicModeTitle: "Basic Editor",
      basicModeLabel: "Basic",
      graphModeTitle: "Graph Editor",
      graphModeLabel: "Graph",
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
    const publishedStage = options.publishedStage || "published";
    return {
      document,
      exportResult: result || null,
      validation,
      validationRows: diagnosticsToRows(validation),
      stage: validation?.ok ? publishedStage : "draft",
    };
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

  function errorMessage(error) {
    return error?.message || String(error || "");
  }

  function criticalErrorOutcome({ head, error, meta } = {}) {
    return {
      validationRows: [{
        kind: "crit",
        glyph: "!",
        head: String(head || "MobKit error"),
        sub: errorMessage(error),
        meta: String(meta || ""),
      }],
      stage: "draft",
    };
  }

  function deployErrorOutcome(error, options = {}) {
    return criticalErrorOutcome({
      head: options.execute ? "Deploy failed" : "Deploy plan failed",
      error,
      meta: "mobkit/mobpacks/deploy",
    });
  }

  function sourceErrorOutcome(error) {
    return criticalErrorOutcome({
      head: "Source render failed",
      error,
      meta: "mobkit/mobpacks/export",
    });
  }

  function validationErrorOutcome(error) {
    return criticalErrorOutcome({
      head: "MobKit API unavailable",
      error,
      meta: "/flow-editor/rpc",
    });
  }

  function exportErrorOutcome(error) {
    return criticalErrorOutcome({
      head: "Export failed",
      error,
      meta: "/flow-editor/rpc",
    });
  }

  function importErrorOutcome(error, options = {}) {
    return criticalErrorOutcome({
      head: "Import failed",
      error,
      meta: options.filename || "",
    });
  }

  function sourceDocumentFromExport(document, result) {
    const exportedToml = String(result?.mob_toml || "").trim();
    if (!exportedToml) throw new Error("mobkit/mobpacks/export did not return mob_toml");
    const renderedDocument = {
      ...(document && typeof document === "object" ? document : {}),
      mob_toml: result.mob_toml,
    };
    const validation = result?.validation || null;
    const stage = validation?.ok ? "valid" : "draft";
    return {
      document: renderedDocument,
      sourceDocument: {
        ...renderedDocument,
        validation,
        filename: result?.filename,
        media_type: result?.media_type,
        source: "mobkit/mobpacks/export",
      },
      validation,
      validationRows: diagnosticsToRows(validation),
      stage,
    };
  }

  function sourceEditorState(sourceDocument, options = {}) {
    const source = sourceDocument?.mob_toml || "";
    const sourceLabel = [
      sourceDocument?.source || "",
      sourceDocument?.filename || "",
      sourceDocument?.media_type || "",
    ].filter(Boolean).join(" · ");
    const validationSource = sourceDocument?.validation?.validation_source || "";
    const bodyClass = options.compact ? "bld-toml__body" : "source-drawer__body";
    return {
      source,
      drawerEyebrow: "SOURCE · mob.toml",
      inlineTitle: "mob.toml",
      sourceLabel,
      validationSource,
      bodyClass,
      showLoading: !!options.busy && !source,
      loadingText: "rendering mob.toml from mobkit/mobpacks/export...",
      copyLabel: "copy",
      closeLabel: "×",
      copyDisabled: !!options.busy || !source,
    };
  }

  function sampleFlowsFromSchema(schema) {
    return (schema?.sample_mobpacks || [])
      .filter((sample) => sample && typeof sample === "object" && sample.document)
      .map((sample) => {
        const source = typeof sample.source === "string" ? sample.source.trim() : "";
        if (!source) return null;
        const id = String(sample.id || "").trim();
        const name = String(sample.name || "").trim();
        if (!id || !name) return null;
        return {
          id,
          name,
          version: String(sample.version || sample.document?.schema_version || ""),
          stage: String(sample.stage || (sample.validation?.ok ? "valid" : "draft")),
          trigger: String(sample.trigger || source),
          source,
          document: sample.document,
          validation: sample.validation || null,
        };
      })
      .filter(Boolean);
  }

  function blankMobpackFromSchema(schema) {
    const blank = schema?.blank_mobpack;
    if (!blank || typeof blank !== "object" || !blank.document) return null;
    const source = typeof blank.source === "string" ? blank.source.trim() : "";
    const id = String(blank.id || "").trim();
    const name = String(blank.name || "").trim();
    if (!id || !name || !source) return null;
    return {
      id,
      name,
      version: String(blank.version || blank.document?.schema_version || ""),
      stage: String(blank.stage || (blank.validation?.ok ? "valid" : "draft")),
      trigger: String(blank.trigger || source),
      source,
      document: blank.document,
      validation: blank.validation || null,
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
    return {
      eyebrow: "FLOWS",
      title: `${list.length} flow${list.length === 1 ? "" : "s"}`,
      createLabel: "+ NEW FLOW",
      createDisabled: !options.canCreate,
      createTitle: options.canCreate ? "Create a MobKit mobpack" : "Waiting for MobKit schema",
      columns: [
        { key: "name", label: "NAME" },
        { key: "trigger", label: "TRIGGER" },
        { key: "version", label: "VERSION" },
        { key: "stage", label: "STAGE" },
      ],
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

  function flowRegistryRowFromDocument({
    id,
    document,
    validation = null,
    stage,
    trigger = "",
    source = "",
    sourceLabel = "",
    flowRow = null,
    fallbackName = "imported-mob",
    fallbackVersion = "draft",
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
      fallbackVersion: "draft",
    });
    return { id: rowId, document, row, template: template || null };
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
        ? String(blankTemplate.trigger || blankTemplate.source || "mobkit/blank-mobpack")
        : "Waiting for MobKit blank mobpack",
      tier: hasBlankDocument ? String(blankTemplate.stage || "draft") : "draft",
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
        tier: sample.validation?.ok ? "valid" : "draft",
        disabled: false,
      });
    }
    return options;
  }

  function newFlowModalState(state = {}, templateOptions = []) {
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
      eyebrow: `NEW FLOW · STEP ${step} OF 2`,
      closeLabel: "×",
      nameLabel: "Name",
      namePlaceholder: "docs-only",
      triggerLabel: "Trigger",
      triggerPlaceholder: "label · docs",
      startFromLabel: "Start from",
      backLabel: "← BACK",
      nextLabel: "NEXT →",
      createLabel: "CREATE",
      name,
      trigger,
      template,
      options,
      createDisabled: !!selectedTemplate?.disabled,
      nextDisabled: !name.trim(),
    };
  }

  function agentDefinitionsFromSchema(schema) {
    const definitions = Array.isArray(schema?.agent_definitions) ? schema.agent_definitions : [];
    return definitions
      .filter((template) => template && typeof template === "object")
      .filter((template) => String(template.definitionType || template.definition_type || "") === "mobkit/profile-member")
      .filter((template) => String(template.source || template.source_mobpack || template.sourceMobpack || "").trim())
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
          skills: Array.isArray(template.skills) ? [...template.skills] : [],
          tools: Array.isArray(template.tools) ? [...template.tools] : [],
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
          sourceDocumentPath: template.sourceDocumentPath || template.source_document_path || "",
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

  function memberFromAgentDefinition(definition, existingMembers = []) {
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
    };
  }

  function agentDefinitionAddPatch(definition, { members, schemas } = {}) {
    const existingMembers = Array.isArray(members) ? members : [];
    const existingSchemas = Array.isArray(schemas) ? schemas : [];
    const member = memberFromAgentDefinition(definition, existingMembers);
    const nextSchemas = mergeAgentDefinitionSchemas(existingSchemas, definition);
    return {
      member,
      members: [...existingMembers, member],
      schemas: nextSchemas,
      schemasChanged: nextSchemas !== existingSchemas,
    };
  }

  function agentDefinitionAddByIdPatch(agentDefinitions, definitionId, { members, schemas } = {}) {
    const id = String(definitionId || "").trim();
    const definition = (Array.isArray(agentDefinitions) ? agentDefinitions : []).find((candidate) => candidate?.id === id);
    if (!definition) {
      return {
        ok: false,
        member: null,
        members: Array.isArray(members) ? members : [],
        schemas: Array.isArray(schemas) ? schemas : [],
        schemasChanged: false,
        error: "unknown agent definition",
      };
    }
    return {
      ok: true,
      ...agentDefinitionAddPatch(definition, { members, schemas }),
    };
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
    const current = list.find((member) => String(member?.id || "").trim() === id) || null;
    if (!current) {
      return { ok: false, error: "member not found", members: list, flow, edges, patch: null };
    }
    const patch = memberSchemaPatch(rawSchema, schemas);
    if (!Object.prototype.hasOwnProperty.call(patch, "schema")) {
      return { ok: false, error: "unknown schema", members: list, flow, edges, patch: null };
    }
    const nextMember = { ...current, ...patch };
    const nextMembers = list.map((member) => String(member?.id || "").trim() === id ? nextMember : member);
    const reconciled = reconcileConditionFieldAvailability({
      flow,
      edges,
      members: nextMembers,
      instances,
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

  function memberProviderParamsEditorState(member) {
    return {
      label: "Provider params",
      text: member?.providerParams ? JSON.stringify(member.providerParams, null, 2) : "",
      placeholder: '{"thinking_budget":4096}',
      rows: 4,
      invalidJsonLabel: "invalid JSON",
    };
  }

  function memberProviderParamsPatch(rawText) {
    const text = String(rawText || "").trim();
    if (!text) return { ok: true, patch: { providerParams: null }, error: "" };
    try {
      const parsed = JSON.parse(text);
      const normalized = normalizeProviderParams(parsed);
      if (!normalized) {
        return { ok: false, patch: null, error: "provider_params must be a JSON object" };
      }
      return { ok: true, patch: { providerParams: normalized }, error: "" };
    } catch (err) {
      return { ok: false, patch: null, error: err?.message || "invalid JSON" };
    }
  }

  function normalizeProfileBackend(value) {
    const backend = String(value || "").trim();
    return backend === "session" || backend === "external" ? backend : "";
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
    const raw = String(value || "").trim().toLowerCase();
    if (raw === "text") return "text";
    if (raw === "json") return "json";
    return "";
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
    createFlowDraftFromSpec,
    flowDraftIdFromSpec,
    newFlowTemplateOptions,
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
    agentEditorControlState,
    agentDefinitionOptions,
    agentDefinitionAddControlState,
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
    basicStepPickerState,
    graphControlShape,
    graphMemberInstanceShape,
    flowStepTemplate,
    graphFirstConditionPatch,
    graphEdgeConditionOwnerPatch,
    graphEdgeConditionFieldPatch,
    graphEdgeConditionPatch,
    graphEdgeConditionOperatorPatch,
    graphEdgeConditionValuePatch,
    graphEdgeKindPatch,
    graphEdgeFallbackPatch,
    graphConnectionEdgeDraft,
    graphSelectionState,
    graphTemplateInspectorState,
    graphInstanceControlState,
    graphToolTagClass,
    graphSourceFileNode,
    graphCanvasInstances,
    graphNodeCanvasState,
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
    enumValueDraftPatch,
    enumValueCommitPatch,
    enumValueDeletePatch,
    enumValueAddPatch,
    schemaFieldAddPatch,
    schemaFieldUpdatePatch,
    schemaFieldDeletePatch,
    schemaFieldDeleteCascadePatch,
    studioAddMemberPatch,
    studioUpdateMemberPatch,
    studioDeleteMemberPatch,
    memberDeleteCascadePatch,
    studioAddInstancePatch,
    studioAppendInstancesPatch,
    studioUpdateInstancePatch,
    studioMoveInstancePatch,
    studioDeleteInstancePatch,
    studioAddEdgePatch,
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
    flowStepUpdatePatch,
    flowStepInsertPatch,
    flowStepDeletePatch,
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
    deployCommandPreview,
    callRpc,
    loadSchema,
    validateDocument,
    exportDocument,
    deployDocument,
    importDocument,
    importParamsFromDecodedFile,
    deploySettingsForUi,
    deployDefaultsFromSchema,
    modelCatalogFromSchema,
    toolCatalogFromSchema,
    blankMobpackFromSchema,
    emptyMobKitCatalogs,
    mobKitCatalogsFromSchema,
    schemaSkillRealms,
    mergeSkillRealms,
    runtimeModeOptions,
    diagnosticsToRows,
    deployResultToRows,
    validationSheetState,
    deployPlanTraceState,
    topRailState,
    validationOutcome,
    exportOutcome,
    deployOutcome,
    criticalErrorOutcome,
    deployErrorOutcome,
    sourceErrorOutcome,
    validationErrorOutcome,
    exportErrorOutcome,
    importErrorOutcome,
    sourceDocumentFromExport,
    sourceEditorState,
    sampleFlowsFromSchema,
    flowRegistryMarkDraftPatch,
    flowRegistryViewState,
    flowRegistrySelectionState,
    flowRegistryRowFromDocument,
    flowRegistryRememberDocumentPatch,
    flowRegistryDocumentPersistence,
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
    reconcileMemberSkillRefs,
    mobSettingsPatch,
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
    mobRoleWiringDeletePatch,
    mobRoleWiringAddPatch,
    advancedMobSettingsEditorState,
    advancedMobSettingsDraftPatch,
    cloneDocument,
    agentDefinitionsFromSchema,
    memberFromAgentDefinition,
    agentDefinitionAddPatch,
    agentDefinitionAddByIdPatch,
    schemaDefinitionsFromAgentDefinition,
    mergeAgentDefinitionSchemas,
  };
})();


/* tweaks-panel.jsx */

{
const __TWEAKS_STYLE = `
  .twk-panel{position:fixed;right:16px;bottom:16px;z-index:2147483646;width:280px;
    max-height:calc(100vh - 32px);display:flex;flex-direction:column;
    transform:scale(var(--dc-inv-zoom,1));transform-origin:bottom right;
    background:rgba(250,249,247,.78);color:#29261b;
    -webkit-backdrop-filter:blur(24px) saturate(160%);backdrop-filter:blur(24px) saturate(160%);
    border:.5px solid rgba(255,255,255,.6);border-radius:14px;
    box-shadow:0 1px 0 rgba(255,255,255,.5) inset,0 12px 40px rgba(0,0,0,.18);
    font:11.5px/1.4 ui-sans-serif,system-ui,-apple-system,sans-serif;overflow:hidden}
  .twk-hd{display:flex;align-items:center;justify-content:space-between;
    padding:10px 8px 10px 14px;cursor:move;user-select:none}
  .twk-hd b{font-size:12px;font-weight:600;letter-spacing:.01em}
  .twk-x{appearance:none;border:0;background:transparent;color:rgba(41,38,27,.55);
    width:22px;height:22px;border-radius:6px;cursor:default;font-size:13px;line-height:1}
  .twk-x:hover{background:rgba(0,0,0,.06);color:#29261b}
  .twk-body{padding:2px 14px 14px;display:flex;flex-direction:column;gap:10px;
    overflow-y:auto;overflow-x:hidden;min-height:0;
    scrollbar-width:thin;scrollbar-color:rgba(0,0,0,.15) transparent}
  .twk-body::-webkit-scrollbar{width:8px}
  .twk-body::-webkit-scrollbar-track{background:transparent;margin:2px}
  .twk-body::-webkit-scrollbar-thumb{background:rgba(0,0,0,.15);border-radius:4px;
    border:2px solid transparent;background-clip:content-box}
  .twk-body::-webkit-scrollbar-thumb:hover{background:rgba(0,0,0,.25);
    border:2px solid transparent;background-clip:content-box}
  .twk-row{display:flex;flex-direction:column;gap:5px}
  .twk-row-h{flex-direction:row;align-items:center;justify-content:space-between;gap:10px}
  .twk-lbl{display:flex;justify-content:space-between;align-items:baseline;
    color:rgba(41,38,27,.72)}
  .twk-lbl>span:first-child{font-weight:500}
  .twk-val{color:rgba(41,38,27,.5);font-variant-numeric:tabular-nums}

  .twk-sect{font-size:10px;font-weight:600;letter-spacing:.06em;text-transform:uppercase;
    color:rgba(41,38,27,.45);padding:10px 0 0}
  .twk-sect:first-child{padding-top:0}

  .twk-field{appearance:none;width:100%;height:26px;padding:0 8px;
    border:.5px solid rgba(0,0,0,.1);border-radius:7px;
    background:rgba(255,255,255,.6);color:inherit;font:inherit;outline:none}
  .twk-field:focus{border-color:rgba(0,0,0,.25);background:rgba(255,255,255,.85)}
  select.twk-field{padding-right:22px;
    background-image:url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='10' height='6' viewBox='0 0 10 6'><path fill='rgba(0,0,0,.5)' d='M0 0h10L5 6z'/></svg>");
    background-repeat:no-repeat;background-position:right 8px center}

  .twk-slider{appearance:none;-webkit-appearance:none;width:100%;height:4px;margin:6px 0;
    border-radius:999px;background:rgba(0,0,0,.12);outline:none}
  .twk-slider::-webkit-slider-thumb{-webkit-appearance:none;appearance:none;
    width:14px;height:14px;border-radius:50%;background:#fff;
    border:.5px solid rgba(0,0,0,.12);box-shadow:0 1px 3px rgba(0,0,0,.2);cursor:default}
  .twk-slider::-moz-range-thumb{width:14px;height:14px;border-radius:50%;
    background:#fff;border:.5px solid rgba(0,0,0,.12);box-shadow:0 1px 3px rgba(0,0,0,.2);cursor:default}

  .twk-seg{position:relative;display:flex;padding:2px;border-radius:8px;
    background:rgba(0,0,0,.06);user-select:none}
  .twk-seg-thumb{position:absolute;top:2px;bottom:2px;border-radius:6px;
    background:rgba(255,255,255,.9);box-shadow:0 1px 2px rgba(0,0,0,.12);
    transition:left .15s cubic-bezier(.3,.7,.4,1),width .15s}
  .twk-seg.dragging .twk-seg-thumb{transition:none}
  .twk-seg button{appearance:none;position:relative;z-index:1;flex:1;border:0;
    background:transparent;color:inherit;font:inherit;font-weight:500;min-height:22px;
    border-radius:6px;cursor:default;padding:4px 6px;line-height:1.2;
    overflow-wrap:anywhere}

  .twk-toggle{position:relative;width:32px;height:18px;border:0;border-radius:999px;
    background:rgba(0,0,0,.15);transition:background .15s;cursor:default;padding:0}
  .twk-toggle[data-on="1"]{background:#34c759}
  .twk-toggle i{position:absolute;top:2px;left:2px;width:14px;height:14px;border-radius:50%;
    background:#fff;box-shadow:0 1px 2px rgba(0,0,0,.25);transition:transform .15s}
  .twk-toggle[data-on="1"] i{transform:translateX(14px)}

  .twk-num{display:flex;align-items:center;height:26px;padding:0 0 0 8px;
    border:.5px solid rgba(0,0,0,.1);border-radius:7px;background:rgba(255,255,255,.6)}
  .twk-num-lbl{font-weight:500;color:rgba(41,38,27,.6);cursor:ew-resize;
    user-select:none;padding-right:8px}
  .twk-num input{flex:1;min-width:0;height:100%;border:0;background:transparent;
    font:inherit;font-variant-numeric:tabular-nums;text-align:right;padding:0 8px 0 0;
    outline:none;color:inherit;-moz-appearance:textfield}
  .twk-num input::-webkit-inner-spin-button,.twk-num input::-webkit-outer-spin-button{
    -webkit-appearance:none;margin:0}
  .twk-num-unit{padding-right:8px;color:rgba(41,38,27,.45)}

  .twk-btn{appearance:none;height:26px;padding:0 12px;border:0;border-radius:7px;
    background:rgba(0,0,0,.78);color:#fff;font:inherit;font-weight:500;cursor:default}
  .twk-btn:hover{background:rgba(0,0,0,.88)}
  .twk-btn.secondary{background:rgba(0,0,0,.06);color:inherit}
  .twk-btn.secondary:hover{background:rgba(0,0,0,.1)}

  .deploy-command{display:block;max-height:74px;overflow:auto;padding:7px 8px;
    border:.5px solid rgba(0,0,0,.1);border-radius:7px;background:rgba(255,255,255,.62);
    color:rgba(41,38,27,.72);font:10.5px/1.35 ui-monospace,SFMono-Regular,Menlo,monospace;
    white-space:pre-wrap;overflow-wrap:anywhere}

  .twk-swatch{appearance:none;-webkit-appearance:none;width:56px;height:22px;
    border:.5px solid rgba(0,0,0,.1);border-radius:6px;padding:0;cursor:default;
    background:transparent;flex-shrink:0}
  .twk-swatch::-webkit-color-swatch-wrapper{padding:0}
  .twk-swatch::-webkit-color-swatch{border:0;border-radius:5.5px}
  .twk-swatch::-moz-color-swatch{border:0;border-radius:5.5px}

  .twk-chips{display:flex;gap:6px}
  .twk-chip{position:relative;appearance:none;flex:1;min-width:0;height:46px;
    padding:0;border:0;border-radius:6px;overflow:hidden;cursor:default;
    box-shadow:0 0 0 .5px rgba(0,0,0,.12),0 1px 2px rgba(0,0,0,.06);
    transition:transform .12s cubic-bezier(.3,.7,.4,1),box-shadow .12s}
  .twk-chip:hover{transform:translateY(-1px);
    box-shadow:0 0 0 .5px rgba(0,0,0,.18),0 4px 10px rgba(0,0,0,.12)}
  .twk-chip[data-on="1"]{box-shadow:0 0 0 1.5px rgba(0,0,0,.85),
    0 2px 6px rgba(0,0,0,.15)}
  .twk-chip>span{position:absolute;top:0;bottom:0;right:0;width:34%;
    display:flex;flex-direction:column;box-shadow:-1px 0 0 rgba(0,0,0,.1)}
  .twk-chip>span>i{flex:1;box-shadow:0 -1px 0 rgba(0,0,0,.1)}
  .twk-chip>span>i:first-child{box-shadow:none}
  .twk-chip svg{position:absolute;top:6px;left:6px;width:13px;height:13px;
    filter:drop-shadow(0 1px 1px rgba(0,0,0,.3))}
`;
function useTweaks(defaults) {
  const [values, setValues] = React.useState(defaults);
  const setTweak = React.useCallback((keyOrEdits, val) => {
    const edits = typeof keyOrEdits === "object" && keyOrEdits !== null ? keyOrEdits : { [keyOrEdits]: val };
    setValues((prev) => ({ ...prev, ...edits }));
    window.parent.postMessage({ type: "__edit_mode_set_keys", edits }, "*");
    window.dispatchEvent(new CustomEvent("tweakchange", { detail: edits }));
  }, []);
  return [values, setTweak];
}
function TweaksPanel({ title = "Tweaks", noDeckControls = false, children }) {
  const [open, setOpen] = React.useState(false);
  const dragRef = React.useRef(null);
  const hasDeckStage = React.useMemo(
    () => typeof document !== "undefined" && !!document.querySelector("deck-stage"),
    []
  );
  const [railEnabled, setRailEnabled] = React.useState(
    () => hasDeckStage && !!document.querySelector("deck-stage")?._railEnabled
  );
  React.useEffect(() => {
    if (!hasDeckStage || railEnabled) return void 0;
    const onMsg = (e) => {
      if (e.data && e.data.type === "__omelette_rail_enabled") setRailEnabled(true);
    };
    window.addEventListener("message", onMsg);
    return () => window.removeEventListener("message", onMsg);
  }, [hasDeckStage, railEnabled]);
  const [railVisible, setRailVisible] = React.useState(() => {
    try {
      return localStorage.getItem("deck-stage.railVisible") !== "0";
    } catch (e) {
      return true;
    }
  });
  const toggleRail = (on) => {
    setRailVisible(on);
    window.postMessage({ type: "__deck_rail_visible", on }, "*");
  };
  const offsetRef = React.useRef({ x: 16, y: 16 });
  const PAD = 16;
  const clampToViewport = React.useCallback(() => {
    const panel = dragRef.current;
    if (!panel) return;
    const w = panel.offsetWidth, h = panel.offsetHeight;
    const maxRight = Math.max(PAD, window.innerWidth - w - PAD);
    const maxBottom = Math.max(PAD, window.innerHeight - h - PAD);
    offsetRef.current = {
      x: Math.min(maxRight, Math.max(PAD, offsetRef.current.x)),
      y: Math.min(maxBottom, Math.max(PAD, offsetRef.current.y))
    };
    panel.style.right = offsetRef.current.x + "px";
    panel.style.bottom = offsetRef.current.y + "px";
  }, []);
  React.useEffect(() => {
    if (!open) return;
    clampToViewport();
    if (typeof ResizeObserver === "undefined") {
      window.addEventListener("resize", clampToViewport);
      return () => window.removeEventListener("resize", clampToViewport);
    }
    const ro = new ResizeObserver(clampToViewport);
    ro.observe(document.documentElement);
    return () => ro.disconnect();
  }, [open, clampToViewport]);
  React.useEffect(() => {
    const onMsg = (e) => {
      const t = e?.data?.type;
      if (t === "__activate_edit_mode") setOpen(true);
      else if (t === "__deactivate_edit_mode") setOpen(false);
    };
    window.addEventListener("message", onMsg);
    window.parent.postMessage({ type: "__edit_mode_available" }, "*");
    return () => window.removeEventListener("message", onMsg);
  }, []);
  const dismiss = () => {
    setOpen(false);
    window.parent.postMessage({ type: "__edit_mode_dismissed" }, "*");
  };
  const onDragStart = (e) => {
    const panel = dragRef.current;
    if (!panel) return;
    const r = panel.getBoundingClientRect();
    const sx = e.clientX, sy = e.clientY;
    const startRight = window.innerWidth - r.right;
    const startBottom = window.innerHeight - r.bottom;
    const move = (ev) => {
      offsetRef.current = {
        x: startRight - (ev.clientX - sx),
        y: startBottom - (ev.clientY - sy)
      };
      clampToViewport();
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };
  if (!open) return null;
  return /* @__PURE__ */ React.createElement(React.Fragment, null, /* @__PURE__ */ React.createElement("style", null, __TWEAKS_STYLE), /* @__PURE__ */ React.createElement(
    "div",
    {
      ref: dragRef,
      className: "twk-panel",
      "data-noncommentable": "",
      style: { right: offsetRef.current.x, bottom: offsetRef.current.y }
    },
    /* @__PURE__ */ React.createElement("div", { className: "twk-hd", onMouseDown: onDragStart }, /* @__PURE__ */ React.createElement("b", null, title), /* @__PURE__ */ React.createElement(
      "button",
      {
        className: "twk-x",
        "aria-label": "Close tweaks",
        onMouseDown: (e) => e.stopPropagation(),
        onClick: dismiss
      },
      "\u2715"
    )),
    /* @__PURE__ */ React.createElement("div", { className: "twk-body" }, children, hasDeckStage && railEnabled && !noDeckControls && /* @__PURE__ */ React.createElement(TweakSection, { label: "Deck" }, /* @__PURE__ */ React.createElement(TweakToggle, { label: "Thumbnail rail", value: railVisible, onChange: toggleRail })))
  ));
}
function TweakSection({ label, children }) {
  return /* @__PURE__ */ React.createElement(React.Fragment, null, /* @__PURE__ */ React.createElement("div", { className: "twk-sect" }, label), children);
}
function TweakRow({ label, value, children, inline = false }) {
  return /* @__PURE__ */ React.createElement("div", { className: inline ? "twk-row twk-row-h" : "twk-row" }, /* @__PURE__ */ React.createElement("div", { className: "twk-lbl" }, /* @__PURE__ */ React.createElement("span", null, label), value != null && /* @__PURE__ */ React.createElement("span", { className: "twk-val" }, value)), children);
}
function TweakSlider({ label, value, min = 0, max = 100, step = 1, unit = "", onChange }) {
  return /* @__PURE__ */ React.createElement(TweakRow, { label, value: `${value}${unit}` }, /* @__PURE__ */ React.createElement(
    "input",
    {
      type: "range",
      className: "twk-slider",
      min,
      max,
      step,
      value,
      onChange: (e) => onChange(Number(e.target.value))
    }
  ));
}
function TweakToggle({ label, value, onChange }) {
  return /* @__PURE__ */ React.createElement("div", { className: "twk-row twk-row-h" }, /* @__PURE__ */ React.createElement("div", { className: "twk-lbl" }, /* @__PURE__ */ React.createElement("span", null, label)), /* @__PURE__ */ React.createElement(
    "button",
    {
      type: "button",
      className: "twk-toggle",
      "data-on": value ? "1" : "0",
      role: "switch",
      "aria-checked": !!value,
      onClick: () => onChange(!value)
    },
    /* @__PURE__ */ React.createElement("i", null)
  ));
}
function TweakRadio({ label, value, options, onChange }) {
  const trackRef = React.useRef(null);
  const [dragging, setDragging] = React.useState(false);
  const valueRef = React.useRef(value);
  valueRef.current = value;
  const labelLen = (o) => String(typeof o === "object" ? o.label : o).length;
  const maxLen = options.reduce((m, o) => Math.max(m, labelLen(o)), 0);
  const fitsAsSegments = maxLen <= ({ 2: 16, 3: 10 }[options.length] ?? 0);
  if (!fitsAsSegments) {
    const resolve = (s) => {
      const m = options.find((o) => String(typeof o === "object" ? o.value : o) === s);
      return m === void 0 ? s : typeof m === "object" ? m.value : m;
    };
    return /* @__PURE__ */ React.createElement(
      TweakSelect,
      {
        label,
        value,
        options,
        onChange: (s) => onChange(resolve(s))
      }
    );
  }
  const opts = options.map((o) => typeof o === "object" ? o : { value: o, label: o });
  const idx = Math.max(0, opts.findIndex((o) => o.value === value));
  const n = opts.length;
  const segAt = (clientX) => {
    const r = trackRef.current.getBoundingClientRect();
    const inner = r.width - 4;
    const i = Math.floor((clientX - r.left - 2) / inner * n);
    return opts[Math.max(0, Math.min(n - 1, i))].value;
  };
  const onPointerDown = (e) => {
    setDragging(true);
    const v0 = segAt(e.clientX);
    if (v0 !== valueRef.current) onChange(v0);
    const move = (ev) => {
      if (!trackRef.current) return;
      const v = segAt(ev.clientX);
      if (v !== valueRef.current) onChange(v);
    };
    const up = () => {
      setDragging(false);
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };
  return /* @__PURE__ */ React.createElement(TweakRow, { label }, /* @__PURE__ */ React.createElement(
    "div",
    {
      ref: trackRef,
      role: "radiogroup",
      onPointerDown,
      className: dragging ? "twk-seg dragging" : "twk-seg"
    },
    /* @__PURE__ */ React.createElement(
      "div",
      {
        className: "twk-seg-thumb",
        style: {
          left: `calc(2px + ${idx} * (100% - 4px) / ${n})`,
          width: `calc((100% - 4px) / ${n})`
        }
      }
    ),
    opts.map((o) => /* @__PURE__ */ React.createElement("button", { key: o.value, type: "button", role: "radio", "aria-checked": o.value === value }, o.label))
  ));
}
function TweakSelect({ label, value, options, onChange }) {
  return /* @__PURE__ */ React.createElement(TweakRow, { label }, /* @__PURE__ */ React.createElement("select", { className: "twk-field", value, onChange: (e) => onChange(e.target.value) }, options.map((o) => {
    const v = typeof o === "object" ? o.value : o;
    const l = typeof o === "object" ? o.label : o;
    const disabled = typeof o === "object" ? !!o.disabled : false;
    return /* @__PURE__ */ React.createElement("option", { key: v, value: v, disabled }, l);
  })));
}
function TweakText({ label, value, placeholder, onChange }) {
  return /* @__PURE__ */ React.createElement(TweakRow, { label }, /* @__PURE__ */ React.createElement(
    "input",
    {
      className: "twk-field",
      type: "text",
      value,
      placeholder,
      onChange: (e) => onChange(e.target.value)
    }
  ));
}
function TweakNumber({ label, value, min, max, step = 1, unit = "", onChange }) {
  const numericValue = Number.isFinite(Number(value)) ? Number(value) : 0;
  const clamp = (n) => {
    if (min != null && n < min) return min;
    if (max != null && n > max) return max;
    return n;
  };
  const startRef = React.useRef({ x: 0, val: 0 });
  const onScrubStart = (e) => {
    e.preventDefault();
    startRef.current = { x: e.clientX, val: numericValue };
    const decimals = (String(step).split(".")[1] || "").length;
    const move = (ev) => {
      const dx = ev.clientX - startRef.current.x;
      const raw = startRef.current.val + dx * step;
      const snapped = Math.round(raw / step) * step;
      onChange(clamp(Number(snapped.toFixed(decimals))));
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };
  return /* @__PURE__ */ React.createElement("div", { className: "twk-num" }, /* @__PURE__ */ React.createElement("span", { className: "twk-num-lbl", onPointerDown: onScrubStart }, label), /* @__PURE__ */ React.createElement(
    "input",
    {
      type: "number",
      value,
      min,
      max,
      step,
      onChange: (e) => onChange(e.target.value === "" ? null : clamp(Number(e.target.value)))
    }
  ), unit && /* @__PURE__ */ React.createElement("span", { className: "twk-num-unit" }, unit));
}
function __twkIsLight(hex) {
  const h = String(hex).replace("#", "");
  const x = h.length === 3 ? h.replace(/./g, (c) => c + c) : h.padEnd(6, "0");
  const n = parseInt(x.slice(0, 6), 16);
  if (Number.isNaN(n)) return true;
  const r = n >> 16 & 255, g = n >> 8 & 255, b = n & 255;
  return r * 299 + g * 587 + b * 114 > 148e3;
}
const __TwkCheck = ({ light }) => /* @__PURE__ */ React.createElement("svg", { viewBox: "0 0 14 14", "aria-hidden": "true" }, /* @__PURE__ */ React.createElement(
  "path",
  {
    d: "M3 7.2 5.8 10 11 4.2",
    fill: "none",
    strokeWidth: "2.2",
    strokeLinecap: "round",
    strokeLinejoin: "round",
    stroke: light ? "rgba(0,0,0,.78)" : "#fff"
  }
));
function TweakColor({ label, value, options, onChange }) {
  if (!options || !options.length) {
    return /* @__PURE__ */ React.createElement("div", { className: "twk-row twk-row-h" }, /* @__PURE__ */ React.createElement("div", { className: "twk-lbl" }, /* @__PURE__ */ React.createElement("span", null, label)), /* @__PURE__ */ React.createElement(
      "input",
      {
        type: "color",
        className: "twk-swatch",
        value,
        onChange: (e) => onChange(e.target.value)
      }
    ));
  }
  const key = (o) => String(JSON.stringify(o)).toLowerCase();
  const cur = key(value);
  return /* @__PURE__ */ React.createElement(TweakRow, { label }, /* @__PURE__ */ React.createElement("div", { className: "twk-chips", role: "radiogroup" }, options.map((o, i) => {
    const colors = Array.isArray(o) ? o : [o];
    const [hero, ...rest] = colors;
    const sup = rest.slice(0, 4);
    const on = key(o) === cur;
    return /* @__PURE__ */ React.createElement(
      "button",
      {
        key: i,
        type: "button",
        className: "twk-chip",
        role: "radio",
        "aria-checked": on,
        "data-on": on ? "1" : "0",
        "aria-label": colors.join(", "),
        title: colors.join(" \xB7 "),
        style: { background: hero },
        onClick: () => onChange(o)
      },
      sup.length > 0 && /* @__PURE__ */ React.createElement("span", null, sup.map((c, j) => /* @__PURE__ */ React.createElement("i", { key: j, style: { background: c } }))),
      on && /* @__PURE__ */ React.createElement(__TwkCheck, { light: __twkIsLight(hero) })
    );
  })));
}
function TweakButton({ label, onClick, secondary = false }) {
  return /* @__PURE__ */ React.createElement(
    "button",
    {
      type: "button",
      className: secondary ? "twk-btn secondary" : "twk-btn",
      onClick
    },
    label
  );
}
Object.assign(window, {
  useTweaks,
  TweaksPanel,
  TweakSection,
  TweakRow,
  TweakSlider,
  TweakToggle,
  TweakRadio,
  TweakSelect,
  TweakText,
  TweakNumber,
  TweakColor,
  TweakButton
});

}

/* graph.jsx */

{
const NODE_W = 200;
const NODE_H = 156;
function dynGrid(instances, gridBase) {
  let maxCol = gridBase.cols - 1;
  let maxRow = gridBase.rows - 1;
  for (const i of instances) {
    if (i.col > maxCol) maxCol = i.col;
    if (i.row > maxRow) maxRow = i.row;
  }
  return { ...gridBase, cols: maxCol + 2, rows: maxRow + 2 };
}
function useStudioState(initial, onDirty, authoring = {}) {
  const [members, setMembers] = React.useState(initial.members);
  const [instances, setInstances] = React.useState(initial.instances);
  const [edges, setEdges] = React.useState(initial.edges);
  const [frames, setFrames] = React.useState(initial.frames);
  const [schemas, setSchemas] = React.useState(initial.schemas);
  const [skillRealms, setSkillRealms] = React.useState(initial.skillRealms || []);
  const [history, setHistory] = React.useState([]);
  const [future, setFuture] = React.useState([]);
  const studioState = React.useCallback(() => ({
    members,
    instances,
    edges,
    frames,
    schemas,
    skillRealms
  }), [members, instances, edges, frames, schemas, skillRealms]);
  const applyStudioState = React.useCallback((state) => {
    setMembers(state.members);
    setInstances(state.instances);
    setEdges(state.edges);
    setFrames(state.frames);
    setSchemas(state.schemas);
    setSkillRealms(state.skillRealms || []);
  }, []);
  const snap = React.useCallback(() => {
    if (onDirty) onDirty();
    const next = window.MobKitFlowController.studioHistorySnapshotPatch({
      history,
      future,
      state: studioState()
    });
    setHistory(next.history);
    setFuture(next.future);
  }, [history, future, studioState, onDirty]);
  const undo = () => {
    const next = window.MobKitFlowController.studioUndoPatch({ history, future, state: studioState() });
    if (!next) return;
    if (onDirty) onDirty();
    setHistory(next.history);
    setFuture(next.future);
    applyStudioState(next.state);
  };
  const redo = () => {
    const next = window.MobKitFlowController.studioRedoPatch({ history, future, state: studioState() });
    if (!next) return;
    if (onDirty) onDirty();
    setHistory(next.history);
    setFuture(next.future);
    applyStudioState(next.state);
  };
  const addMember = (m) => {
    snap();
    const next = window.MobKitFlowController.studioAddMemberPatch({ members }, m);
    setMembers(next.members);
  };
  const updateMember = (id, patch) => {
    snap();
    const next = window.MobKitFlowController.studioUpdateMemberPatch({ members }, id, patch);
    setMembers(next.members);
  };
  const deleteMember = (id) => {
    snap();
    const next = window.MobKitFlowController.studioDeleteMemberPatch({ members, instances, edges }, id);
    setMembers(next.members);
    setInstances(next.instances);
    setEdges(next.edges);
  };
  const addInstance = (i) => {
    snap();
    const next = window.MobKitFlowController.studioAddInstancePatch({ instances, members }, i);
    setInstances(next.instances);
  };
  const updateInstance = (id, patch) => {
    snap();
    const next = window.MobKitFlowController.studioUpdateInstancePatch({ instances, members }, id, patch);
    setInstances(next.instances);
  };
  const deleteInstance = (id) => {
    snap();
    const next = window.MobKitFlowController.studioDeleteInstancePatch({ instances, edges }, id);
    setInstances(next.instances);
    setEdges(next.edges);
  };
  const addEdge = (e) => {
    snap();
    const next = window.MobKitFlowController.studioAddEdgePatch({ edges, instances }, e);
    setEdges(next.edges);
  };
  const updateEdge = (id, patch) => {
    snap();
    const next = window.MobKitFlowController.studioUpdateEdgePatch({ edges, instances }, id, patch);
    setEdges(next.edges);
  };
  const deleteEdge = (id) => {
    snap();
    const next = window.MobKitFlowController.studioDeleteEdgePatch({ edges }, id);
    setEdges(next.edges);
  };
  const addSchema = (s) => {
    snap();
    const next = window.MobKitFlowController.studioAddSchemaPatch({ schemas }, s);
    setSchemas(next.schemas);
  };
  const updateSchema = (id, patch) => {
    snap();
    const next = window.MobKitFlowController.studioUpdateSchemaPatch({ schemas }, id, patch);
    setSchemas(next.schemas);
  };
  const deleteSchema = (id) => {
    snap();
    const next = window.MobKitFlowController.studioDeleteSchemaPatch({
      schemas,
      members,
      flow: authoring.flow,
      edges,
      instances
    }, id);
    setSchemas(next.schemas);
    setMembers(next.members);
    if (next.flow !== authoring.flow && authoring.setFlow) authoring.setFlow(next.flow);
    if (next.edges) setEdges(next.edges);
  };
  const updateSkillRealms = (next) => {
    snap();
    setSkillRealms(Array.isArray(next) ? next : []);
  };
  return {
    members,
    instances,
    edges,
    frames,
    schemas,
    skillRealms,
    setMembers,
    setInstances,
    setEdges,
    setFrames,
    setSchemas,
    setSkillRealms,
    snap,
    undo,
    redo,
    canUndo: !!history.length,
    canRedo: !!future.length,
    addMember,
    updateMember,
    deleteMember,
    addInstance,
    updateInstance,
    deleteInstance,
    addEdge,
    updateEdge,
    deleteEdge,
    addSchema,
    updateSchema,
    deleteSchema,
    updateSkillRealms
  };
}
function cellXYFor(g, col, row) {
  return {
    x: g.padX + col * (g.cellW + g.gapX),
    y: g.padY + row * (g.cellH + g.gapY)
  };
}
function nodeBox(g, n) {
  const { x, y } = cellXYFor(g, n.col, n.row);
  if (n.isSourceFile) {
    const sw = 210, sh = 58;
    return { x: x + (g.cellW - sw) / 2, y: y + (g.cellH - sh) / 2, w: sw, h: sh };
  }
  if (n.isGate) {
    const gw = 156, gh = 56;
    return { x: x + (g.cellW - gw) / 2, y: y + (g.cellH - gh) / 2, w: gw, h: gh };
  }
  return {
    x: x + (g.cellW - NODE_W) / 2,
    y: y + (g.cellH - NODE_H) / 2,
    w: NODE_W,
    h: NODE_H
  };
}
function portOut(g, n) {
  const b = nodeBox(g, n);
  return { x: b.x + b.w, y: b.y + b.h / 2 };
}
function portIn(g, n) {
  const b = nodeBox(g, n);
  return { x: b.x, y: b.y + b.h / 2 };
}
function edgePath(a, b) {
  if (b.x < a.x - 20) {
    const dropY = Math.max(a.y, b.y) + 90;
    const dx2 = 60;
    return `M ${a.x} ${a.y} C ${a.x + dx2} ${a.y}, ${a.x + dx2} ${dropY}, ${a.x} ${dropY} L ${b.x} ${dropY} C ${b.x - dx2} ${dropY}, ${b.x - dx2} ${b.y}, ${b.x} ${b.y}`;
  }
  const dx = Math.max(40, (b.x - a.x) * 0.5);
  return `M ${a.x} ${a.y} C ${a.x + dx} ${a.y}, ${b.x - dx} ${b.y}, ${b.x} ${b.y}`;
}
function midpoint(a, b) {
  if (b.x < a.x - 20) return { x: (a.x + b.x) / 2, y: Math.max(a.y, b.y) + 90 };
  return { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 - 6 };
}
function occMap(insts) {
  const m = /* @__PURE__ */ new Map();
  for (const i of insts) m.set(i.col + ":" + i.row, i);
  return m;
}
function GraphEditor({ state, selection, selectInstance, selectEdge, clearSelection, activeStepId, edgeStyle, density, onRequestAdd, onOpenSourceFile, memberFocus, grid, contract }) {
  const hostRef = React.useRef(null);
  const [drag, setDrag] = React.useState(null);
  const [conn, setConn] = React.useState(null);
  const [hoverInId, setHoverInId] = React.useState(null);
  const [hoverCell, setHoverCell] = React.useState(null);
  const [view, setView] = React.useState({ scale: 1, tx: 0, ty: 0 });
  const viewRef = React.useRef(view);
  React.useEffect(() => {
    viewRef.current = view;
  }, [view]);
  const [panDrag, setPanDrag] = React.useState(null);
  const g = dynGrid(state.instances, grid);
  const totalW = g.padX * 2 + g.cols * g.cellW + (g.cols - 1) * g.gapX;
  const totalH = g.padY * 2 + g.rows * g.cellH + (g.rows - 1) * g.gapY;
  const occ = occMap(state.instances);
  const fitToBounds = React.useCallback(() => {
    const host = hostRef.current;
    if (!host) return;
    const r = host.getBoundingClientRect();
    const scale = Math.min(1, Math.min((r.width - 32) / totalW, (r.height - 32) / totalH));
    const tx = (r.width - totalW * scale) / 2;
    const ty = Math.max(8, (r.height - totalH * scale) / 2);
    setView({ scale, tx, ty });
  }, [totalW, totalH]);
  const didFit = React.useRef(false);
  React.useEffect(() => {
    if (didFit.current) return;
    if (hostRef.current?.offsetWidth > 0) {
      fitToBounds();
      didFit.current = true;
    } else {
      const id = setTimeout(() => {
        fitToBounds();
        didFit.current = true;
      }, 50);
      return () => clearTimeout(id);
    }
  }, [fitToBounds]);
  const screenToWorld = (sx, sy) => {
    const r = hostRef.current.getBoundingClientRect();
    const v = viewRef.current;
    return { x: (sx - r.left - v.tx) / v.scale, y: (sy - r.top - v.ty) / v.scale };
  };
  const zoomAt = (factor, sx, sy) => {
    const v = viewRef.current;
    const r = hostRef.current.getBoundingClientRect();
    const cx = sx - r.left;
    const cy = sy - r.top;
    const next = Math.max(0.3, Math.min(2.5, v.scale * factor));
    const k = next / v.scale;
    setView({
      scale: next,
      tx: cx - (cx - v.tx) * k,
      ty: cy - (cy - v.ty) * k
    });
  };
  const cellAt = (x, y) => {
    const col = Math.floor((x - g.padX + g.gapX / 2) / (g.cellW + g.gapX));
    const row = Math.floor((y - g.padY + g.gapY / 2) / (g.cellH + g.gapY));
    if (col < 0 || col >= g.cols || row < 0 || row >= g.rows) return null;
    return { col, row };
  };
  const onNodeDown = (e, inst) => {
    if (e.target.classList.contains("port")) return;
    e.stopPropagation();
    selectInstance(inst.id);
    const w = screenToWorld(e.clientX, e.clientY);
    const b = nodeBox(g, inst);
    setDrag({ instId: inst.id, dx: w.x - b.x, dy: w.y - b.y, origCol: inst.col, origRow: inst.row });
  };
  const onPortDown = (e, inst) => {
    e.stopPropagation();
    const p = portOut(g, inst);
    setConn({ from: p, fromId: inst.id, to: p });
  };
  const onHostMouseDown = (e) => {
    if (e.button !== 0 && e.button !== 1) return;
    const target = e.target;
    if (target === hostRef.current || target.classList?.contains("canvas")) {
      setPanDrag({ sx: e.clientX, sy: e.clientY, tx0: viewRef.current.tx, ty0: viewRef.current.ty });
      e.preventDefault();
    }
  };
  const onHostWheel = (e) => {
    if (!hostRef.current) return;
    if (e.ctrlKey || e.metaKey) {
      e.preventDefault();
      const factor = Math.exp(-e.deltaY * 15e-4);
      zoomAt(factor, e.clientX, e.clientY);
    } else {
      e.preventDefault();
      setView((v) => ({ ...v, tx: v.tx - e.deltaX, ty: v.ty - e.deltaY }));
    }
  };
  React.useEffect(() => {
    const el = hostRef.current;
    if (!el) return;
    const handler = (e) => onHostWheel(e);
    el.addEventListener("wheel", handler, { passive: false });
    return () => el.removeEventListener("wheel", handler);
  });
  React.useEffect(() => {
    const move = (e) => {
      if (panDrag) {
        setView((v) => ({ ...v, tx: panDrag.tx0 + (e.clientX - panDrag.sx), ty: panDrag.ty0 + (e.clientY - panDrag.sy) }));
      }
      if (drag) {
        const w = screenToWorld(e.clientX, e.clientY);
        const cx = w.x - drag.dx + NODE_W / 2;
        const cy = w.y - drag.dy + NODE_H / 2;
        const cell = cellAt(cx, cy);
        if (cell) setHoverCell(cell);
      }
      if (conn) {
        const w = screenToWorld(e.clientX, e.clientY);
        setConn((c) => ({ ...c, to: { x: w.x, y: w.y } }));
        const t = document.elementFromPoint(e.clientX, e.clientY);
        const closest = t?.closest?.("[data-inst-id]");
        if (closest && closest.dataset.instId !== conn.fromId) setHoverInId(closest.dataset.instId);
        else setHoverInId(null);
      }
    };
    const up = (e) => {
      if (drag) {
        const w = screenToWorld(e.clientX, e.clientY);
        const cx = w.x - drag.dx + NODE_W / 2;
        const cy = w.y - drag.dy + NODE_H / 2;
        const cell = cellAt(cx, cy);
        if (cell && (cell.col !== drag.origCol || cell.row !== drag.origRow)) {
          state.snap();
          const next = window.MobKitFlowController.studioMoveInstancePatch({
            instances: state.instances
          }, drag.instId, cell, {
            col: drag.origCol,
            row: drag.origRow
          });
          state.setInstances(next.instances);
        }
        setDrag(null);
        setHoverCell(null);
      }
      if (conn) {
        const t = document.elementFromPoint(e.clientX, e.clientY);
        const closest = t?.closest?.("[data-inst-id]");
        if (closest && closest.dataset.instId !== conn.fromId) {
          const toId = closest.dataset.instId;
          const fromI = state.instances.find((i) => i.id === conn.fromId);
          const toI = state.instances.find((i) => i.id === toId);
          const newEdge = window.MobKitFlowController.graphConnectionEdgeDraft({
            from: fromI,
            to: toI,
            edges: state.edges,
            contract
          });
          if (newEdge) {
            state.addEdge(newEdge);
            selectEdge(newEdge.id);
          }
        }
        setConn(null);
        setHoverInId(null);
      }
      if (panDrag) setPanDrag(null);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
    return () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
    };
  });
  const fit = view;
  const cells = [];
  for (let c = 0; c < g.cols; c++) {
    for (let r = 0; r < g.rows; r++) {
      const occupied = occ.has(c + ":" + r);
      const { x, y } = cellXYFor(g, c, r);
      cells.push(
        /* @__PURE__ */ React.createElement(
          "div",
          {
            key: `cell-${c}-${r}`,
            className: "cell" + (occupied ? " is-occupied" : "") + (hoverCell?.col === c && hoverCell?.row === r ? " is-hover" : ""),
            style: { left: x, top: y, width: g.cellW, height: g.cellH },
            onMouseDown: (e) => e.stopPropagation(),
            onClick: (e) => {
              e.stopPropagation();
              if (!occupied) onRequestAdd(c, r);
            }
          },
          !occupied && /* @__PURE__ */ React.createElement("div", { className: "cell__add" }, /* @__PURE__ */ React.createElement("span", { className: "cell__plus" }, "+"))
        )
      );
    }
  }
  const colHeads = [];
  for (let c = 0; c < g.cols; c++) {
    const { x } = cellXYFor(g, c, 0);
    colHeads.push(/* @__PURE__ */ React.createElement("div", { key: "col-" + c, className: "grid-head grid-head--col", style: { left: x, top: 28, width: g.cellW } }, String(c + 1).padStart(2, "0")));
  }
  const rowHeads = [];
  for (let r = 0; r < g.rows; r++) {
    const { y } = cellXYFor(g, 0, r);
    rowHeads.push(/* @__PURE__ */ React.createElement("div", { key: "row-" + r, className: "grid-head grid-head--row", style: { left: 14, top: y + g.cellH / 2 - 8 } }, String.fromCharCode(65 + r)));
  }
  const frameEls = state.frames.map((fr) => {
    const a = cellXYFor(g, fr.colStart, 0);
    const b = cellXYFor(g, fr.colEnd, g.rows - 1);
    const x = a.x - 14, y = a.y - 18;
    const w = b.x + g.cellW - x + 14;
    const h = b.y + g.cellH - y + 18;
    return /* @__PURE__ */ React.createElement(React.Fragment, { key: fr.id }, /* @__PURE__ */ React.createElement("div", { className: "frame", style: { left: x, top: y, width: w, height: h } }), /* @__PURE__ */ React.createElement("div", { className: "frame-label", style: { left: x + 12, top: y - 10 } }, fr.label));
  });
  const edgeEls = state.edges.map((edge) => {
    const fi = state.instances.find((i) => i.id === edge.from);
    const ti = state.instances.find((i) => i.id === edge.to);
    if (!fi || !ti) return null;
    const a = portOut(g, fi), b = portIn(g, ti);
    const d = edgePath(a, b);
    const mid = midpoint(a, b);
    const isActive = activeStepId === edge.from;
    const isSelected = selection.kind === "edge" && selection.id === edge.id;
    const edgeState = window.MobKitFlowController.graphEdgeCanvasState({
      edge,
      to: ti,
      active: isActive,
      selected: isSelected,
      edgeStyle
    });
    let labelEl;
    if (edgeState.mode === "icons") {
      labelEl = /* @__PURE__ */ React.createElement("g", { transform: `translate(${mid.x}, ${mid.y})` }, /* @__PURE__ */ React.createElement("rect", { x: -9, y: -9, width: 18, height: 16, className: "edge-label-bg" }), /* @__PURE__ */ React.createElement("text", { textAnchor: "middle", y: 4, className: edgeState.iconLabelClass }, edgeState.iconGlyph));
    } else if (edgeState.mode === "colored") {
      labelEl = /* @__PURE__ */ React.createElement("g", { transform: `translate(${mid.x}, ${mid.y})` }, /* @__PURE__ */ React.createElement("rect", { x: -edgeState.labelWidth / 2, y: -8, width: edgeState.labelWidth, height: 14, className: "edge-label-bg" }), /* @__PURE__ */ React.createElement("text", { textAnchor: "middle", y: 3, className: "edge-label", style: { fill: edgeState.labelFill } }, edgeState.labelText));
    } else {
      labelEl = /* @__PURE__ */ React.createElement("g", { transform: `translate(${mid.x}, ${mid.y})` }, /* @__PURE__ */ React.createElement("rect", { x: -edgeState.labelWidth / 2, y: -8, width: edgeState.labelWidth, height: 14, className: "edge-label-bg" }), /* @__PURE__ */ React.createElement("text", { textAnchor: "middle", y: 3, className: edgeState.textLabelClass }, edgeState.labelText));
    }
    return /* @__PURE__ */ React.createElement("g", { key: edge.id, className: "edge", onClick: (e) => {
      e.stopPropagation();
      selectEdge(edge.id);
    } }, /* @__PURE__ */ React.createElement("path", { d, className: "edge-hit" }), /* @__PURE__ */ React.createElement("path", { d, className: edgeState.lineClass, markerEnd: edgeState.markerEnd }), labelEl);
  });
  const canvasInstances = window.MobKitFlowController.graphCanvasInstances({ instances: state.instances });
  const nodeEls = canvasInstances.map((inst) => {
    if (inst.isGate) {
      return /* @__PURE__ */ React.createElement(
        GateView,
        {
          key: inst.id,
          g,
          inst,
          selected: selection.kind === "instance" && selection.id === inst.id,
          activeStep: activeStepId === inst.id,
          hoverIn: hoverInId === inst.id,
          onMouseDown: onNodeDown,
          onPortDown,
          state
        }
      );
    }
    return /* @__PURE__ */ React.createElement(
      NodeView,
      {
        key: inst.id,
        g,
        inst,
        nodeState: window.MobKitFlowController.graphNodeCanvasState({ inst, members: state.members, density }),
        selected: selection.kind === "instance" && selection.id === inst.id,
        memberHighlight: memberFocus && inst.memberId === memberFocus,
        memberDim: !!memberFocus && inst.memberId !== memberFocus && !inst.isTerminal,
        activeStep: activeStepId === inst.id,
        hoverIn: hoverInId === inst.id,
        onMouseDown: onNodeDown,
        onPortDown,
        onOpenSourceFile
      }
    );
  });
  return /* @__PURE__ */ React.createElement(
    "div",
    {
      ref: hostRef,
      className: "canvas-host" + (memberFocus ? " is-member-focus" : "") + (panDrag ? " is-panning" : ""),
      onMouseDown: onHostMouseDown,
      onClick: (e) => {
        if (e.target === hostRef.current || e.target.classList?.contains("canvas")) clearSelection();
      }
    },
    /* @__PURE__ */ React.createElement("div", { className: "canvas", style: { width: totalW, height: totalH, transform: `translate(${fit.tx}px, ${fit.ty}px) scale(${fit.scale})`, transformOrigin: "0 0" } }, colHeads, rowHeads, frameEls, cells, /* @__PURE__ */ React.createElement("svg", { className: "edges-svg", width: totalW, height: totalH }, /* @__PURE__ */ React.createElement("defs", null, /* @__PURE__ */ React.createElement("marker", { id: "arr", viewBox: "0 0 10 10", refX: "9", refY: "5", markerWidth: "7", markerHeight: "7", orient: "auto" }, /* @__PURE__ */ React.createElement("path", { d: "M 0 0 L 10 5 L 0 10 z", fill: "var(--ink)" })), /* @__PURE__ */ React.createElement("marker", { id: "arr-red", viewBox: "0 0 10 10", refX: "9", refY: "5", markerWidth: "7", markerHeight: "7", orient: "auto" }, /* @__PURE__ */ React.createElement("path", { d: "M 0 0 L 10 5 L 0 10 z", fill: "var(--danger)" })), /* @__PURE__ */ React.createElement("marker", { id: "arr-acc", viewBox: "0 0 10 10", refX: "9", refY: "5", markerWidth: "7", markerHeight: "7", orient: "auto" }, /* @__PURE__ */ React.createElement("path", { d: "M 0 0 L 10 5 L 0 10 z", fill: "var(--accent)" })), /* @__PURE__ */ React.createElement("marker", { id: "arr-dim", viewBox: "0 0 10 10", refX: "9", refY: "5", markerWidth: "7", markerHeight: "7", orient: "auto" }, /* @__PURE__ */ React.createElement("path", { d: "M 0 0 L 10 5 L 0 10 z", fill: "var(--subtle)" }))), edgeEls, conn && /* @__PURE__ */ React.createElement("path", { d: edgePath(conn.from, conn.to), className: "edge-line is-ghost", markerEnd: "url(#arr-acc)" })), nodeEls),
    /* @__PURE__ */ React.createElement("div", { className: "zoom-controls", onMouseDown: (e) => e.stopPropagation() }, /* @__PURE__ */ React.createElement("button", { className: "zoom-btn", title: "Zoom out", onClick: () => {
      const r = hostRef.current.getBoundingClientRect();
      zoomAt(1 / 1.2, r.left + r.width / 2, r.top + r.height / 2);
    } }, "\u2212"), /* @__PURE__ */ React.createElement("button", { className: "zoom-btn zoom-btn--pct", title: "Fit to view", onClick: fitToBounds }, Math.round(view.scale * 100), "%"), /* @__PURE__ */ React.createElement("button", { className: "zoom-btn", title: "Zoom in", onClick: () => {
      const r = hostRef.current.getBoundingClientRect();
      zoomAt(1.2, r.left + r.width / 2, r.top + r.height / 2);
    } }, "+"))
  );
}
function NodeView({ g, inst, nodeState, selected, memberHighlight, memberDim, activeStep, hoverIn, onMouseDown, onPortDown, onOpenSourceFile }) {
  const b = nodeBox(g, inst);
  if (nodeState.isTerminal) {
    const openSourceFile = (event) => {
      if (!nodeState.isSourceFile) return;
      event.stopPropagation();
      onOpenSourceFile?.(inst);
    };
    if (nodeState.isSourceFile) {
      return /* @__PURE__ */ React.createElement(
        "div",
        {
          "data-inst-id": inst.id,
          className: "node node--term node--source-file" + (selected ? " is-selected" : "") + (activeStep ? " is-active-step" : "") + (hoverIn ? " is-target" : ""),
          "data-kind": nodeState.dataKind,
          role: nodeState.role,
          tabIndex: nodeState.tabIndex,
          "aria-label": nodeState.ariaLabel,
          style: { left: b.x, top: b.y, width: b.w, height: b.h },
          onMouseDown: (e) => {
            e.stopPropagation();
          },
          onClick: openSourceFile,
          onKeyDown: (e) => {
            if (e.key !== "Enter" && e.key !== " ") return;
            e.preventDefault();
            openSourceFile(e);
          }
        },
        /* @__PURE__ */ React.createElement("span", { className: "source-file__glyph" }, nodeState.sourceGlyph),
        /* @__PURE__ */ React.createElement("span", { className: "source-file__label" }, nodeState.title)
      );
    }
    return /* @__PURE__ */ React.createElement(
      "div",
      {
        "data-inst-id": inst.id,
        className: "node node--term" + (nodeState.isSourceFile ? " node--source-file" : "") + (selected ? " is-selected" : "") + (activeStep ? " is-active-step" : "") + (hoverIn ? " is-target" : ""),
        "data-kind": nodeState.dataKind,
        role: nodeState.role,
        tabIndex: nodeState.tabIndex,
        "aria-label": nodeState.ariaLabel,
        style: { left: b.x, top: b.y, width: b.w, height: b.h },
        onMouseDown: (e) => {
          if (nodeState.isSourceFile) {
            e.stopPropagation();
            return;
          }
          onMouseDown(e, inst);
        },
        onClick: openSourceFile,
        onKeyDown: (e) => {
          if (!nodeState.isSourceFile || e.key !== "Enter" && e.key !== " ") return;
          e.preventDefault();
          openSourceFile(e);
        }
      },
      /* @__PURE__ */ React.createElement("div", { className: "node__head" }, /* @__PURE__ */ React.createElement("span", { className: "node__role" }, nodeState.roleLabel)),
      /* @__PURE__ */ React.createElement("div", { className: "node__body" }, /* @__PURE__ */ React.createElement("div", { className: "node__name" }, nodeState.title), /* @__PURE__ */ React.createElement("div", { className: "node__model" }, nodeState.subtitle))
    );
  }
  if (nodeState.hidden) return null;
  return /* @__PURE__ */ React.createElement(
    "div",
    {
      "data-inst-id": inst.id,
      className: "node" + (selected ? " is-selected" : "") + (memberHighlight ? " is-member-highlight" : "") + (memberDim ? " is-member-dim" : "") + (activeStep ? " is-active-step" : "") + (hoverIn ? " is-target" : "") + (nodeState.isCompact ? " is-compact" : ""),
      style: { left: b.x, top: b.y, width: b.w, height: b.h },
      onMouseDown: (e) => onMouseDown(e, inst)
    },
    /* @__PURE__ */ React.createElement("div", { className: "port port-out", onMouseDown: (e) => onPortDown(e, inst), title: "Drag to a member to connect" }),
    /* @__PURE__ */ React.createElement("div", { className: "node__head" }, /* @__PURE__ */ React.createElement("span", { className: "node__role" }, nodeState.roleLabel), /* @__PURE__ */ React.createElement("span", { className: "node__idx" }, nodeState.launchLabel)),
    /* @__PURE__ */ React.createElement("div", { className: "node__body" }, /* @__PURE__ */ React.createElement("div", { className: "node__name" }, nodeState.title), /* @__PURE__ */ React.createElement("div", { className: "node__model" }, nodeState.subtitle)),
    !nodeState.isCompact && /* @__PURE__ */ React.createElement("div", { className: "node__tools" }, nodeState.toolRows.map((row) => /* @__PURE__ */ React.createElement("span", { key: row.id, className: row.className }, row.id)), nodeState.overflowLabel && /* @__PURE__ */ React.createElement("span", { className: "tag" }, nodeState.overflowLabel))
  );
}
function GateView({ g, inst, selected, activeStep, hoverIn, onMouseDown, onPortDown, state }) {
  const b = nodeBox(g, inst);
  const gateState = window.MobKitFlowController.graphGateCanvasState({ inst, edges: state.edges });
  return /* @__PURE__ */ React.createElement(
    "div",
    {
      "data-inst-id": inst.id,
      className: "node node--gate gate--" + gateState.gateKind + (selected ? " is-selected" : "") + (activeStep ? " is-active-step" : "") + (hoverIn ? " is-target" : ""),
      style: { left: b.x, top: b.y, width: b.w, height: b.h },
      onMouseDown: (e) => onMouseDown(e, inst)
    },
    /* @__PURE__ */ React.createElement("div", { className: "port port-out", onMouseDown: (e) => onPortDown(e, inst), title: "Drag to a member to connect" }),
    /* @__PURE__ */ React.createElement("span", { className: "gate__glyph" }, gateState.glyph),
    /* @__PURE__ */ React.createElement("span", { className: "gate__label" }, gateState.sublabel)
  );
}
function computeFit(vw, vh, tw, th) {
  const scale = Math.min(1, Math.min((vw - 24) / tw, (vh - 24) / th));
  const left = (vw - tw * scale) / 2;
  const top = Math.max(8, (vh - th * scale) / 2);
  return { scale, left, top };
}
window.useStudioState = useStudioState;
window.GraphEditor = GraphEditor;

}

/* inspector.jsx */

{
function Inspector({ studio, selection, selectMember, selectInstance, clearSelection, template, templateSeed, flow, contract }) {
  const selectionState = window.MobKitFlowController.graphSelectionState({
    selection,
    instances: studio.instances,
    edges: studio.edges
  });
  if (selectionState.kind === "instance") {
    if (!selectionState.instance) return /* @__PURE__ */ React.createElement(TemplateInspector, { studio, template, templateSeed });
    return /* @__PURE__ */ React.createElement(InstanceInspector, { studio, flow, inst: selectionState.instance, selectMember, clearSelection, contract });
  }
  if (selectionState.kind === "edge") {
    if (!selectionState.edge) return /* @__PURE__ */ React.createElement(TemplateInspector, { studio, template, templateSeed });
    return /* @__PURE__ */ React.createElement(EdgeInspector, { studio, flow, edge: selectionState.edge, clearSelection, contract });
  }
  return /* @__PURE__ */ React.createElement(TemplateInspector, { studio, template, templateSeed });
}
function TemplateInspector({ studio, template, templateSeed }) {
  const templateState = window.MobKitFlowController.graphTemplateInspectorState({ studio, template, templateSeed });
  return /* @__PURE__ */ React.createElement(React.Fragment, null, /* @__PURE__ */ React.createElement("div", { className: "inspector__head" }, /* @__PURE__ */ React.createElement("div", { className: "inspector__eyebrow" }, "TEMPLATE"), /* @__PURE__ */ React.createElement("div", { className: "inspector__title" }, templateState.name), /* @__PURE__ */ React.createElement("div", { className: "inspector__id" }, templateState.repo, " \xB7 ", templateState.version)), /* @__PURE__ */ React.createElement("div", { className: "inspector__body" }, /* @__PURE__ */ React.createElement("div", { className: "section" }, /* @__PURE__ */ React.createElement("div", { className: "section__title" }, "SUMMARY"), /* @__PURE__ */ React.createElement("dl", { className: "kv" }, templateState.summaryRows.map((row) => /* @__PURE__ */ React.createElement(React.Fragment, { key: row.key }, /* @__PURE__ */ React.createElement("dt", null, row.label), /* @__PURE__ */ React.createElement("dd", null, row.value))))), /* @__PURE__ */ React.createElement("div", { className: "section" }, /* @__PURE__ */ React.createElement("div", { className: "section__title" }, "TRIGGERS"), /* @__PURE__ */ React.createElement("dl", { className: "kv" }, /* @__PURE__ */ React.createElement("dt", null, "labels"), /* @__PURE__ */ React.createElement("dd", null, templateState.triggers.labels.join(", ")), /* @__PURE__ */ React.createElement("dt", null, "default"), /* @__PURE__ */ React.createElement("dd", null, templateState.triggers.default ? "yes" : "no"))), /* @__PURE__ */ React.createElement("div", { className: "section section--hint" }, /* @__PURE__ */ React.createElement("div", { className: "hint__title" }, "QUICK START"), /* @__PURE__ */ React.createElement("div", { className: "hint__line" }, "Click a ", /* @__PURE__ */ React.createElement("strong", null, "library member"), " on the left to edit it."), /* @__PURE__ */ React.createElement("div", { className: "hint__line" }, "Click an ", /* @__PURE__ */ React.createElement("strong", null, "empty grid cell"), " to place a member."), /* @__PURE__ */ React.createElement("div", { className: "hint__line" }, "Drag a node's ", /* @__PURE__ */ React.createElement("strong", null, "right port"), " to wire it to another."), /* @__PURE__ */ React.createElement("div", { className: "hint__line" }, "\u232B deletes the selected instance or edge."))));
}
function GateInspector({ studio, flow, inst, clearSelection, contract }) {
  const change = (patch) => studio.updateInstance(inst.id, patch);
  const kind = inst.gateKind;
  const gateState = window.MobKitFlowController.graphGateControlState(inst, {
    edges: studio.edges,
    members: studio.members,
    contract
  });
  const branchRows = kind === "branch" ? window.MobKitFlowController.graphBranchConditionRows({
    inst,
    edges: studio.edges,
    instances: studio.instances,
    members: studio.members,
    schemas: studio.schemas,
    flow,
    contract
  }) : [];
  return /* @__PURE__ */ React.createElement(React.Fragment, null, /* @__PURE__ */ React.createElement("div", { className: "inspector__head" }, /* @__PURE__ */ React.createElement("div", { className: "row row--between" }, /* @__PURE__ */ React.createElement("div", null, /* @__PURE__ */ React.createElement("div", { className: "inspector__eyebrow" }, gateState.eyebrow), /* @__PURE__ */ React.createElement("div", { className: "inspector__title" }, gateState.title), /* @__PURE__ */ React.createElement("div", { className: "inspector__id" }, gateState.idLine)), /* @__PURE__ */ React.createElement("button", { className: "btn btn--ghost btn--sm", onClick: () => {
    studio.deleteInstance(inst.id);
    clearSelection();
  } }, gateState.deleteLabel))), /* @__PURE__ */ React.createElement("div", { className: "inspector__body" }, /* @__PURE__ */ React.createElement("div", { className: "section" }, /* @__PURE__ */ React.createElement("div", { className: "section__title" }, gateState.labelTitle), /* @__PURE__ */ React.createElement("input", { className: "field__input", value: inst.label, onChange: (e) => change(window.MobKitFlowController.graphInstanceLabelPatch(e.target.value)) })), /* @__PURE__ */ React.createElement("div", { className: "section" }, /* @__PURE__ */ React.createElement("div", { className: "section__title" }, gateState.kindTitle), /* @__PURE__ */ React.createElement("select", { className: "field__select", value: gateState.gateKind, onChange: (e) => change(window.MobKitFlowController.graphGateKindPatch(e.target.value, contract)) }, gateState.gateKindOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value, disabled: option.disabled }, option.label))), gateState.selectedGateKind?.reason && /* @__PURE__ */ React.createElement("div", { className: "kv__hint", style: { color: "var(--warn)" } }, gateState.selectedGateKind.reason)), kind === "join" && /* @__PURE__ */ React.createElement("div", { className: "section" }, /* @__PURE__ */ React.createElement("div", { className: "section__title" }, gateState.collectionTitle), /* @__PURE__ */ React.createElement("select", { className: "field__select", value: gateState.collection, onChange: (e) => {
    change(window.MobKitFlowController.graphJoinCollectionPatch(inst, e.target.value, {
      incomingCount: gateState.incoming.length,
      firstMemberId: gateState.firstMemberId,
      contract
    }));
  } }, gateState.collectionOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value, disabled: option.disabled }, option.label))), gateState.selectedCollection?.reason && /* @__PURE__ */ React.createElement("div", { className: "kv__hint", style: { color: "var(--warn)" } }, gateState.selectedCollection.reason), gateState.collection === "quorum" && /* @__PURE__ */ React.createElement("div", { className: "row", style: { marginTop: 8 } }, /* @__PURE__ */ React.createElement(
    "input",
    {
      className: "field__input field__input--num",
      type: "number",
      min: "1",
      value: inst.quorum?.n || gateState.incoming.length || 1,
      onChange: (e) => change(window.MobKitFlowController.graphJoinQuorumPatch(inst, e.target.value, gateState.incoming.length))
    }
  ), /* @__PURE__ */ React.createElement("span", { className: "kv__hint" }, gateState.quorumIncomingLabel)), gateState.collection && gateState.collection !== "all" && /* @__PURE__ */ React.createElement("div", { className: "field", style: { marginTop: 8 } }, /* @__PURE__ */ React.createElement("label", { className: "field__label" }, gateState.joinMemberLabel), /* @__PURE__ */ React.createElement("select", { className: "field__select", value: inst.controllerRole || "", onChange: (e) => change(window.MobKitFlowController.graphJoinControllerRolePatch(e.target.value, studio.members)) }, /* @__PURE__ */ React.createElement("option", { value: gateState.joinMemberPlaceholderOption.value }, gateState.joinMemberPlaceholderOption.label), gateState.memberOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value }, option.label))), /* @__PURE__ */ React.createElement("div", { className: "kv__hint" }, gateState.joinMemberHint))), kind === "fork" && /* @__PURE__ */ React.createElement("div", { className: "section" }, /* @__PURE__ */ React.createElement("div", { className: "section__title" }, gateState.dispatchTitle), /* @__PURE__ */ React.createElement("select", { className: "field__select", value: gateState.dispatch, onChange: (e) => change(window.MobKitFlowController.graphForkDispatchPatch(inst, e.target.value, contract)) }, gateState.dispatchOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value, disabled: option.disabled }, option.label))), gateState.selectedDispatch?.reason && /* @__PURE__ */ React.createElement("div", { className: "kv__hint", style: { color: "var(--warn)" } }, gateState.selectedDispatch.reason), /* @__PURE__ */ React.createElement("div", { className: "kv__hint" }, gateState.dispatchHint)), kind === "branch" && /* @__PURE__ */ React.createElement("div", { className: "section" }, /* @__PURE__ */ React.createElement("div", { className: "section__title" }, gateState.conditionsTitle), branchRows.length === 0 && /* @__PURE__ */ React.createElement("div", { className: "kv__hint" }, gateState.emptyBranchHint), branchRows.map((row) => {
    const e = row.edge;
    const setCondOwner = (instanceId) => studio.updateEdge(e.id, window.MobKitFlowController.graphEdgeConditionOwnerPatch(e, row.conditionOptions, instanceId, {
      defaultOperator: row.defaultOperator,
      forceLabel: true,
      includeKind: true
    }));
    const setCondField = (field) => studio.updateEdge(e.id, window.MobKitFlowController.graphEdgeConditionFieldPatch(e, row.conditionOptions, field, {
      defaultOperator: row.defaultOperator,
      forceLabel: true,
      includeKind: true
    }));
    return /* @__PURE__ */ React.createElement("div", { key: e.id, className: "branch-cond-row" }, /* @__PURE__ */ React.createElement("div", { className: "row row--gap" }, /* @__PURE__ */ React.createElement("select", { className: "field__select", value: e.kind === "cond" ? "cond" : "fallback", onChange: (ev) => {
      if (ev.target.value === "fallback") {
        const patch = window.MobKitFlowController.graphEdgeFallbackPatch(e, contract);
        if (patch) studio.updateEdge(e.id, patch);
      } else {
        setCondOwner(row.firstOwnerId);
      }
    } }, row.modeOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value }, option.label))), /* @__PURE__ */ React.createElement("span", { className: "kv__hint" }, row.targetPrefix, " ", row.targetLabel)), e.kind === "cond" && (!row.hasConditionOptions ? /* @__PURE__ */ React.createElement("div", { className: "kv__hint", style: { color: "var(--warn)" } }, row.noConditionOptionsHint) : /* @__PURE__ */ React.createElement("div", { className: "bld-cond", style: { marginTop: 8 } }, /* @__PURE__ */ React.createElement("select", { className: "field__select", value: row.ownerValue, onChange: (ev) => setCondOwner(ev.target.value) }, row.ownerOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value }, option.label))), /* @__PURE__ */ React.createElement("select", { className: "field__select", value: row.fieldValue, onChange: (ev) => setCondField(ev.target.value) }, /* @__PURE__ */ React.createElement("option", { value: row.fieldPlaceholderOption.value }, row.fieldPlaceholderOption.label), row.fieldOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value }, option.label))), /* @__PURE__ */ React.createElement("select", { className: "field__select bld-cond__op", value: row.operatorValue, onChange: (ev) => studio.updateEdge(e.id, window.MobKitFlowController.graphEdgeConditionOperatorPatch(e, ev.target.value, { defaultOperator: row.defaultOperator, contract })) }, row.operatorOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value, disabled: option.disabled }, option.label))), /* @__PURE__ */ React.createElement(GraphCondValue, { field: row.condField, value: e.cond?.val, onChange: (val) => studio.updateEdge(e.id, window.MobKitFlowController.graphEdgeConditionValuePatch(e, val, { defaultOperator: row.defaultOperator })) }))));
  })), /* @__PURE__ */ React.createElement("div", { className: "section" }, /* @__PURE__ */ React.createElement("div", { className: "section__title" }, gateState.wiringTitle), /* @__PURE__ */ React.createElement("dl", { className: "kv" }, /* @__PURE__ */ React.createElement("dt", null, gateState.incomingLabel), /* @__PURE__ */ React.createElement("dd", null, gateState.incomingCount), /* @__PURE__ */ React.createElement("dt", null, gateState.outgoingLabel), /* @__PURE__ */ React.createElement("dd", null, gateState.outgoingCount)))));
}
function InstanceInspector({ studio, flow, inst, selectMember, clearSelection, contract }) {
  const instanceState = window.MobKitFlowController.graphInstanceControlState({
    inst,
    instances: studio.instances,
    members: studio.members,
    schemas: studio.schemas
  });
  const member = instanceState.member;
  if (inst.isGate) {
    return /* @__PURE__ */ React.createElement(GateInspector, { studio, flow, inst, clearSelection, contract });
  }
  if (inst.isTerminal) {
    const terminalState = window.MobKitFlowController.graphTerminalControlState(inst, contract);
    return /* @__PURE__ */ React.createElement(React.Fragment, null, /* @__PURE__ */ React.createElement("div", { className: "inspector__head" }, /* @__PURE__ */ React.createElement("div", { className: "row row--between" }, /* @__PURE__ */ React.createElement("div", null, /* @__PURE__ */ React.createElement("div", { className: "inspector__eyebrow" }, terminalState.eyebrow), /* @__PURE__ */ React.createElement("div", { className: "inspector__title" }, terminalState.title), /* @__PURE__ */ React.createElement("div", { className: "inspector__id" }, terminalState.idLine)), /* @__PURE__ */ React.createElement("button", { className: "btn btn--ghost btn--sm", onClick: () => {
      studio.deleteInstance(inst.id);
      clearSelection();
    } }, terminalState.deleteLabel))), /* @__PURE__ */ React.createElement("div", { className: "inspector__body" }, /* @__PURE__ */ React.createElement("div", { className: "section" }, /* @__PURE__ */ React.createElement("div", { className: "section__title" }, terminalState.labelTitle), /* @__PURE__ */ React.createElement("input", { className: "field__input", value: terminalState.labelValue, onChange: (e) => studio.updateInstance(inst.id, window.MobKitFlowController.graphInstanceLabelPatch(e.target.value)) })), /* @__PURE__ */ React.createElement("div", { className: "section" }, /* @__PURE__ */ React.createElement("div", { className: "section__title" }, terminalState.kindTitle), /* @__PURE__ */ React.createElement("select", { className: "field__select", value: terminalState.terminalKind, onChange: (e) => studio.updateInstance(inst.id, window.MobKitFlowController.graphTerminalKindPatch(e.target.value, contract)) }, terminalState.terminalKindOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value, disabled: option.disabled }, option.label))), terminalState.selectedTerminalKind?.reason && /* @__PURE__ */ React.createElement("div", { className: "kv__hint", style: { color: "var(--warn)" } }, terminalState.selectedTerminalKind.reason))));
  }
  const launchState = window.MobKitFlowController.launchModeControlState(inst, contract);
  return /* @__PURE__ */ React.createElement(React.Fragment, null, /* @__PURE__ */ React.createElement("div", { className: "inspector__head" }, /* @__PURE__ */ React.createElement("div", { className: "row row--between" }, /* @__PURE__ */ React.createElement("div", null, /* @__PURE__ */ React.createElement("div", { className: "inspector__eyebrow" }, instanceState.eyebrow), /* @__PURE__ */ React.createElement("div", { className: "inspector__title" }, instanceState.title), /* @__PURE__ */ React.createElement("div", { className: "inspector__id" }, instanceState.idLine)), /* @__PURE__ */ React.createElement("button", { className: "btn btn--ghost btn--sm", onClick: () => {
    studio.deleteInstance(inst.id);
    clearSelection();
  } }, instanceState.deleteLabel))), /* @__PURE__ */ React.createElement("div", { className: "inspector__body" }, member && /* @__PURE__ */ React.createElement("div", { className: "section section--member-card" }, /* @__PURE__ */ React.createElement("div", { className: "member-card" }, /* @__PURE__ */ React.createElement("div", { className: "member-card__head" }, /* @__PURE__ */ React.createElement("span", { className: "member-card__role" }, instanceState.memberRoleLabel), /* @__PURE__ */ React.createElement("button", { className: "btn btn--ghost btn--sm", onClick: () => selectMember(instanceState.memberId) }, instanceState.editMemberLabel)), /* @__PURE__ */ React.createElement("div", { className: "member-card__name" }, instanceState.memberName), /* @__PURE__ */ React.createElement("dl", { className: "kv kv--small" }, instanceState.memberSummaryRows.map((row) => /* @__PURE__ */ React.createElement(React.Fragment, { key: row.key }, /* @__PURE__ */ React.createElement("dt", null, row.label), /* @__PURE__ */ React.createElement("dd", null, row.value)))), /* @__PURE__ */ React.createElement("div", { className: "member-card__hint" }, instanceState.memberHint))), /* @__PURE__ */ React.createElement("div", { className: "section" }, /* @__PURE__ */ React.createElement("div", { className: "section__title" }, launchState.graphLaunchTitle), /* @__PURE__ */ React.createElement(
    "select",
    {
      className: "field__select",
      value: launchState.launchKind,
      onChange: (e) => {
        studio.updateInstance(inst.id, window.MobKitFlowController.launchModeKindPatch(inst, e.target.value, contract, { firstForkSourceId: instanceState.firstForkSourceId }));
      }
    },
    launchState.launchOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value, disabled: option.disabled }, option.label))
  ), launchState.selectedLaunchMode?.reason && /* @__PURE__ */ React.createElement("div", { className: "hint__line", style: { color: "var(--warn)" } }, launchState.selectedLaunchMode.reason), launchState.launchKind === "Resume" && /* @__PURE__ */ React.createElement("div", { className: "field", style: { marginTop: 8 } }, /* @__PURE__ */ React.createElement("label", { className: "field__label" }, launchState.resumeSessionLabel), /* @__PURE__ */ React.createElement(
    "input",
    {
      className: "field__input",
      value: launchState.launchMode.sessionId || "",
      placeholder: launchState.resumeSessionPlaceholder,
      onChange: (e) => studio.updateInstance(inst.id, window.MobKitFlowController.launchModeSessionPatch(inst, e.target.value, contract))
    }
  )), launchState.launchKind === "Fork" && /* @__PURE__ */ React.createElement(React.Fragment, null, /* @__PURE__ */ React.createElement("div", { className: "field", style: { marginTop: 8 } }, /* @__PURE__ */ React.createElement("label", { className: "field__label" }, launchState.forkSourceLabel), /* @__PURE__ */ React.createElement("select", { className: "field__select", value: launchState.launchMode.from || "", onChange: (e) => studio.updateInstance(inst.id, window.MobKitFlowController.launchModeForkSourcePatch(inst, e.target.value, contract, { sourceOptions: instanceState.forkSourceOptions })) }, instanceState.forkSourceOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value }, option.label)))), /* @__PURE__ */ React.createElement("div", { className: "field" }, /* @__PURE__ */ React.createElement("label", { className: "field__label" }, launchState.graphForkContextLabel), /* @__PURE__ */ React.createElement("select", { className: "field__select", value: launchState.forkContextValue, onChange: (e) => studio.updateInstance(inst.id, window.MobKitFlowController.launchModeForkContextPatch(inst, e.target.value, contract)) }, launchState.forkContextOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value, disabled: option.disabled }, option.label)))), launchState.selectedForkContext?.reason && /* @__PURE__ */ React.createElement("div", { className: "hint__line", style: { color: "var(--warn)" } }, launchState.selectedForkContext.reason)), /* @__PURE__ */ React.createElement("div", { className: "field", style: { marginTop: 8 } }, /* @__PURE__ */ React.createElement("label", { className: "field__label" }, launchState.budgetPolicyLabel), /* @__PURE__ */ React.createElement("select", { className: "field__select", value: launchState.budgetSplitPolicy.kind, onChange: (e) => studio.updateInstance(inst.id, window.MobKitFlowController.launchBudgetKindPatch(inst, e.target.value, contract)) }, launchState.budgetOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value, disabled: option.disabled }, option.label))), launchState.selectedBudgetPolicy?.reason && /* @__PURE__ */ React.createElement("div", { className: "hint__line", style: { color: "var(--warn)" } }, launchState.selectedBudgetPolicy.reason)), launchState.budgetSplitPolicy.kind === "Fixed" && /* @__PURE__ */ React.createElement("div", { className: "field" }, /* @__PURE__ */ React.createElement("label", { className: "field__label" }, launchState.fixedBudgetLabel), /* @__PURE__ */ React.createElement("input", { className: "field__input", type: "number", min: "1", step: "1", value: launchState.fixedBudgetValue, onChange: (e) => studio.updateInstance(inst.id, window.MobKitFlowController.launchBudgetFixedLimitPatch(inst, e.target.value, contract)) }))), /* @__PURE__ */ React.createElement("div", { className: "section" }, /* @__PURE__ */ React.createElement("div", { className: "section__title" }, instanceState.positionTitle), /* @__PURE__ */ React.createElement("dl", { className: "kv kv--small" }, instanceState.positionRows.map((row) => /* @__PURE__ */ React.createElement(React.Fragment, { key: row.key }, /* @__PURE__ */ React.createElement("dt", null, row.label), /* @__PURE__ */ React.createElement("dd", null, row.value))))), member && /* @__PURE__ */ React.createElement("div", { className: "section" }, /* @__PURE__ */ React.createElement("div", { className: "section__title" }, instanceState.outputTitle), instanceState.outputSchema && /* @__PURE__ */ React.createElement("ul", { className: "schema-fields" }, instanceState.outputFieldRows.map((f) => /* @__PURE__ */ React.createElement("li", { key: f.id }, /* @__PURE__ */ React.createElement("span", { className: "sf__name" }, f.name), /* @__PURE__ */ React.createElement("span", { className: "sf__type" }, f.type), f.required && /* @__PURE__ */ React.createElement("span", { className: "sf__req" }, f.requiredLabel)))), /* @__PURE__ */ React.createElement("div", { className: "hint__line", style: { marginTop: 6 } }, instanceState.outputHint, " ", /* @__PURE__ */ React.createElement("button", { className: "link", onClick: () => selectMember(instanceState.memberId) }, instanceState.outputOpenMemberLabel)))));
}
function GraphCondValue({ field, value, onChange }) {
  const control = window.MobKitFlowController.conditionValueControl(field, value);
  if (control.kind === "enum") {
    return /* @__PURE__ */ React.createElement("select", { className: "field__select", value: control.value, onChange: (e) => onChange(e.target.value) }, control.optionRows.map((row) => /* @__PURE__ */ React.createElement("option", { key: row.value || "blank", value: row.value }, row.label)));
  }
  if (control.kind === "boolean") {
    return /* @__PURE__ */ React.createElement("select", { className: "field__select", value: control.value, onChange: (e) => onChange(e.target.value) }, control.optionRows.map((row) => /* @__PURE__ */ React.createElement("option", { key: row.value || "blank", value: row.value }, row.label)));
  }
  return /* @__PURE__ */ React.createElement("input", { className: "field__input", placeholder: control.placeholder, value: control.value, onChange: (e) => onChange(e.target.value) });
}
function EdgeInspector({ studio, flow, edge, clearSelection, contract }) {
  const edgeState = window.MobKitFlowController.graphEdgeInspectorState({
    edge,
    instances: studio.instances,
    members: studio.members,
    schemas: studio.schemas,
    flow,
    contract
  });
  const change = (patch) => studio.updateEdge(edge.id, patch);
  const setEdgeKind = (kind) => change(window.MobKitFlowController.graphEdgeKindPatch(edge, kind, {
    defaultOperator: edgeState.defaultOperator,
    conditionPatch: edgeState.conditionPatch,
    forceLabel: true
  }));
  const setCondOwner = (instanceId) => change(window.MobKitFlowController.graphEdgeConditionOwnerPatch(edge, edgeState.conditionOptions, instanceId, {
    defaultOperator: edgeState.defaultOperator,
    forceLabel: true
  }));
  const setCondField = (field) => change(window.MobKitFlowController.graphEdgeConditionFieldPatch(edge, edgeState.conditionOptions, field, {
    defaultOperator: edgeState.defaultOperator,
    forceLabel: true
  }));
  return /* @__PURE__ */ React.createElement(React.Fragment, null, /* @__PURE__ */ React.createElement("div", { className: "inspector__head" }, /* @__PURE__ */ React.createElement("div", { className: "row row--between" }, /* @__PURE__ */ React.createElement("div", null, /* @__PURE__ */ React.createElement("div", { className: "inspector__eyebrow" }, edgeState.eyebrow), /* @__PURE__ */ React.createElement("div", { className: "inspector__title" }, edgeState.title), /* @__PURE__ */ React.createElement("div", { className: "inspector__id" }, edgeState.idLine)), /* @__PURE__ */ React.createElement("button", { className: "btn btn--ghost btn--sm", onClick: () => {
    studio.deleteEdge(edge.id);
    clearSelection();
  } }, edgeState.deleteLabel))), /* @__PURE__ */ React.createElement("div", { className: "inspector__body" }, /* @__PURE__ */ React.createElement("div", { className: "section" }, /* @__PURE__ */ React.createElement("div", { className: "section__title" }, edgeState.kindTitle), /* @__PURE__ */ React.createElement("select", { className: "field__select", value: edgeState.edgeKind, onChange: (e) => setEdgeKind(e.target.value) }, edgeState.edgeKindOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value, disabled: option.disabled }, option.label))), edgeState.selectedEdgeKind?.reason && /* @__PURE__ */ React.createElement("div", { className: "hint__line", style: { color: "var(--warn)" } }, edgeState.selectedEdgeKind.reason)), /* @__PURE__ */ React.createElement("div", { className: "section" }, /* @__PURE__ */ React.createElement("div", { className: "section__title" }, edgeState.labelTitle), /* @__PURE__ */ React.createElement("input", { className: "field__input", value: edge.label || "", onChange: (e) => change(window.MobKitFlowController.graphEdgeLabelPatch(e.target.value)) })), edge.kind === "cond" && /* @__PURE__ */ React.createElement("div", { className: "section" }, /* @__PURE__ */ React.createElement("div", { className: "section__title" }, edgeState.conditionTitle), !edgeState.hasConditionOptions ? /* @__PURE__ */ React.createElement("div", { className: "hint__line", style: { color: "var(--warn)" } }, edgeState.noConditionOptionsHint) : /* @__PURE__ */ React.createElement("div", { className: "cond-row" }, /* @__PURE__ */ React.createElement("select", { className: "field__select", value: edgeState.ownerValue, onChange: (e) => setCondOwner(e.target.value) }, /* @__PURE__ */ React.createElement("option", { value: edgeState.ownerPlaceholderOption.value }, edgeState.ownerPlaceholderOption.label), edgeState.ownerOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value }, option.label))), /* @__PURE__ */ React.createElement("select", { className: "field__select", value: edgeState.fieldValue, disabled: !edgeState.condOwner, onChange: (e) => setCondField(e.target.value) }, /* @__PURE__ */ React.createElement("option", { value: "" }, edgeState.fieldPlaceholder), edgeState.fieldOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.field.id || option.value, value: option.value }, option.label))), /* @__PURE__ */ React.createElement("select", { className: "field__select", style: { width: 60 }, value: edgeState.operatorValue, onChange: (e) => change(window.MobKitFlowController.graphEdgeConditionOperatorPatch(edge, e.target.value, { defaultOperator: edgeState.defaultOperator, contract })) }, edgeState.operatorOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value, disabled: option.disabled }, option.label))), /* @__PURE__ */ React.createElement(GraphCondValue, { field: edgeState.condField, value: edge.cond?.val, onChange: (val) => change(window.MobKitFlowController.graphEdgeConditionValuePatch(edge, val, { defaultOperator: edgeState.defaultOperator })) }))), /* @__PURE__ */ React.createElement("div", { className: "section" }, /* @__PURE__ */ React.createElement("div", { className: "section__title" }, edgeState.fromTitle), /* @__PURE__ */ React.createElement("dl", { className: "kv" }, edgeState.fromRows.map((row) => /* @__PURE__ */ React.createElement(React.Fragment, { key: row.key }, /* @__PURE__ */ React.createElement("dt", null, row.label), /* @__PURE__ */ React.createElement("dd", null, row.value))))), /* @__PURE__ */ React.createElement("div", { className: "section" }, /* @__PURE__ */ React.createElement("div", { className: "section__title" }, edgeState.toTitle), /* @__PURE__ */ React.createElement("dl", { className: "kv" }, edgeState.toRows.map((row) => /* @__PURE__ */ React.createElement(React.Fragment, { key: row.key }, /* @__PURE__ */ React.createElement("dt", null, row.label), /* @__PURE__ */ React.createElement("dd", null, row.value)))))));
}
function AddNodeMenu({ at, members, contract, onPick, onClose, onJumpToAgents }) {
  const [q, setQ] = React.useState("");
  React.useEffect(() => {
    setQ("");
  }, [at]);
  if (!at) return null;
  const menuState = window.MobKitFlowController.graphAddNodeMenuState({ members, contract, query: q });
  return /* @__PURE__ */ React.createElement("div", { className: "add-menu", style: { left: at.x, top: at.y }, onClick: (e) => e.stopPropagation(), onMouseDown: (e) => e.stopPropagation() }, /* @__PURE__ */ React.createElement("div", { className: "add-menu__search" }, /* @__PURE__ */ React.createElement("span", { className: "add-menu__search-icon" }, menuState.searchIcon), /* @__PURE__ */ React.createElement(
    "input",
    {
      className: "add-menu__search-input",
      autoFocus: true,
      placeholder: menuState.searchPlaceholder,
      value: q,
      onChange: (e) => setQ(e.target.value),
      onKeyDown: (e) => {
        if (e.key === "Escape") onClose();
      }
    }
  ), /* @__PURE__ */ React.createElement("button", { className: "add-menu__x", onClick: onClose, title: menuState.closeTitle }, menuState.closeLabel)), /* @__PURE__ */ React.createElement("div", { className: "add-menu__scroll" }, menuState.hasMembers && /* @__PURE__ */ React.createElement("div", { className: "add-menu__label" }, menuState.agentsLabel), menuState.memberRows.map((row) => /* @__PURE__ */ React.createElement("button", { key: row.id, className: "add-menu__row", onClick: () => onPick(row.pick) }, /* @__PURE__ */ React.createElement("span", { className: "add-menu__dot", "data-role": row.role }), /* @__PURE__ */ React.createElement("span", { className: "add-menu__row-name" }, row.name), /* @__PURE__ */ React.createElement("span", { className: "add-menu__row-meta" }, row.model))), menuState.hasControls && /* @__PURE__ */ React.createElement("div", { className: "add-menu__label" }, menuState.controlsLabel), menuState.controlRows.map((row) => /* @__PURE__ */ React.createElement("button", { key: row.id, className: "add-menu__row", onClick: () => onPick(row.pick) }, /* @__PURE__ */ React.createElement("span", { className: "add-menu__glyph" }, row.glyph), /* @__PURE__ */ React.createElement("span", { className: "add-menu__row-name" }, row.label), /* @__PURE__ */ React.createElement("span", { className: "add-menu__row-meta" }, row.meta))), menuState.isEmpty && /* @__PURE__ */ React.createElement("div", { className: "add-menu__empty" }, menuState.emptyLabel)), onJumpToAgents && /* @__PURE__ */ React.createElement("button", { className: "add-menu__foot", onClick: () => onJumpToAgents(null) }, menuState.jumpLabel));
}
window.Inspector = Inspector;
window.AddNodeMenu = AddNodeMenu;

}

/* overlays.jsx */

{
function DrySim({ open, onClose, onActiveStep, runKey, document, plan }) {
  const traceState = React.useMemo(
    () => window.MobKitFlowController.deployPlanTraceState(document, plan),
    [document, plan]
  );
  const [idx, setIdx] = React.useState(0);
  const bodyRef = React.useRef(null);
  React.useEffect(() => {
    if (!open) return;
    setIdx(0);
  }, [open, runKey]);
  React.useEffect(() => {
    if (!open) {
      onActiveStep(null);
      return;
    }
    onActiveStep(traceState.steps[idx]?.node || null);
    if (bodyRef.current) {
      const el = bodyRef.current.querySelector(`[data-step="${idx}"]`);
      if (el) el.scrollIntoView({ block: "nearest", behavior: "smooth" });
    }
  }, [idx, open, traceState.steps]);
  if (!open) return null;
  return /* @__PURE__ */ React.createElement("div", { className: "drysim" }, /* @__PURE__ */ React.createElement("div", { className: "drysim__head" }, /* @__PURE__ */ React.createElement("div", null, /* @__PURE__ */ React.createElement("div", { className: "drysim__title" }, /* @__PURE__ */ React.createElement("span", { className: "accent" }, traceState.eyebrow), " \xB7 ", traceState.title), /* @__PURE__ */ React.createElement("div", { className: "drysim__sub" }, traceState.subtitle)), /* @__PURE__ */ React.createElement("div", { className: "row" }, /* @__PURE__ */ React.createElement("button", { className: "btn btn--sm", onClick: () => setIdx(0) }, traceState.firstLabel), /* @__PURE__ */ React.createElement("button", { className: "btn btn--ghost btn--sm", onClick: onClose }, traceState.closeLabel))), /* @__PURE__ */ React.createElement("div", { className: "drysim__body", ref: bodyRef }, traceState.steps.map((s, i) => /* @__PURE__ */ React.createElement(
    "div",
    {
      key: i,
      "data-step": i,
      className: "drysim__step" + (i === idx ? " is-current" : "") + (i > idx ? " is-pending" : "")
    },
    /* @__PURE__ */ React.createElement("div", { className: "g" }),
    /* @__PURE__ */ React.createElement("div", null, /* @__PURE__ */ React.createElement("div", { className: "head" }, s.head), /* @__PURE__ */ React.createElement("div", { className: "body" }, s.body))
  ))), /* @__PURE__ */ React.createElement("div", { className: "drysim__foot" }, /* @__PURE__ */ React.createElement("div", { className: "row row--between", style: { width: "100%" } }, /* @__PURE__ */ React.createElement("span", { className: "muted" }, traceState.packLabel ? `${traceState.packLabel} \xB7 ` : "", traceState.stepLabel, " ", idx + 1, " / ", traceState.steps.length), /* @__PURE__ */ React.createElement("div", { className: "row" }, /* @__PURE__ */ React.createElement("button", { className: "btn btn--sm", onClick: () => setIdx((i) => Math.max(0, i - 1)) }, traceState.previousLabel), /* @__PURE__ */ React.createElement("button", { className: "btn btn--sm", onClick: () => setIdx((i) => Math.min(traceState.steps.length - 1, i + 1)) }, traceState.nextLabel)))));
}
function ValidateSheet({ open, onClose, onPublish, onDeployPlan, onDeployRun, results, stage }) {
  if (!open) return null;
  const sheetState = window.MobKitFlowController.validationSheetState(results, { stage });
  return /* @__PURE__ */ React.createElement("div", { className: "validate" }, /* @__PURE__ */ React.createElement("div", { className: "validate__head" }, /* @__PURE__ */ React.createElement("div", null, /* @__PURE__ */ React.createElement("div", { className: "inspector__eyebrow" }, sheetState.eyebrow), /* @__PURE__ */ React.createElement("div", { className: "inspector__title" }, sheetState.title)), /* @__PURE__ */ React.createElement("div", { className: "row" }, /* @__PURE__ */ React.createElement("button", { className: "btn btn--primary btn--sm", onClick: onPublish, disabled: sheetState.actionsDisabled }, sheetState.publishLabel), /* @__PURE__ */ React.createElement("button", { className: "btn btn--ghost btn--sm", onClick: onDeployPlan, disabled: sheetState.actionsDisabled }, sheetState.deployPlanLabel), /* @__PURE__ */ React.createElement("button", { className: "btn btn--primary btn--sm", onClick: onDeployRun, disabled: sheetState.actionsDisabled }, sheetState.deployLabel), /* @__PURE__ */ React.createElement("button", { className: "btn btn--ghost btn--sm", onClick: onClose }, sheetState.closeLabel))), /* @__PURE__ */ React.createElement("div", { className: "validate__body" }, sheetState.rows.map((r, i) => /* @__PURE__ */ React.createElement("div", { key: i, className: "validate__row is-" + r.kind }, /* @__PURE__ */ React.createElement("span", { className: "glyph" }, r.glyph), /* @__PURE__ */ React.createElement("div", null, /* @__PURE__ */ React.createElement("div", { className: "head" }, r.head), /* @__PURE__ */ React.createElement("div", { className: "sub" }, r.sub)), /* @__PURE__ */ React.createElement("span", { className: "meta" }, r.meta)))));
}
function SourceCodePanel({ state, busy = false, compact = false }) {
  const editorState = window.MobKitFlowController.sourceEditorState(state, { busy, compact });
  if (editorState.showLoading) {
    return /* @__PURE__ */ React.createElement("pre", { className: editorState.bodyClass, role: "textbox", "aria-readonly": "true" }, editorState.loadingText);
  }
  return /* @__PURE__ */ React.createElement(
    "pre",
    {
      className: editorState.bodyClass,
      role: "textbox",
      "aria-readonly": "true",
      dangerouslySetInnerHTML: { __html: highlightToml(editorState.source) }
    }
  );
}
function SourceDrawer({ open, onClose, state }) {
  if (!open) return null;
  const editorState = window.MobKitFlowController.sourceEditorState(state);
  return /* @__PURE__ */ React.createElement("div", { className: "source-drawer" }, /* @__PURE__ */ React.createElement("div", { className: "source-drawer__head" }, /* @__PURE__ */ React.createElement("div", null, /* @__PURE__ */ React.createElement("div", { className: "inspector__eyebrow" }, editorState.drawerEyebrow), /* @__PURE__ */ React.createElement("div", { className: "inspector__id" }, editorState.sourceLabel), editorState.validationSource && /* @__PURE__ */ React.createElement("div", { className: "inspector__id" }, editorState.validationSource)), /* @__PURE__ */ React.createElement("div", { className: "row" }, /* @__PURE__ */ React.createElement("button", { className: "btn btn--sm", onClick: () => navigator.clipboard?.writeText(editorState.source) }, editorState.copyLabel), /* @__PURE__ */ React.createElement("button", { className: "btn btn--ghost btn--sm", onClick: onClose }, editorState.closeLabel))), /* @__PURE__ */ React.createElement(SourceCodePanel, { state }));
}
function InlineSourceEditor({ open, onClose, state, busy = false, surface = "basic" }) {
  if (!open) return null;
  const editorState = window.MobKitFlowController.sourceEditorState(state, { busy, compact: true });
  return /* @__PURE__ */ React.createElement("div", { className: "bld-toml bld-toml--" + surface, onMouseDown: (e) => e.stopPropagation() }, /* @__PURE__ */ React.createElement("div", { className: "bld-toml__head" }, /* @__PURE__ */ React.createElement("div", null, /* @__PURE__ */ React.createElement("div", null, editorState.inlineTitle), /* @__PURE__ */ React.createElement("div", { className: "bld-toml__hint" }, editorState.sourceLabel), editorState.validationSource && /* @__PURE__ */ React.createElement("div", { className: "bld-toml__hint" }, editorState.validationSource)), /* @__PURE__ */ React.createElement("div", { className: "row" }, /* @__PURE__ */ React.createElement("button", { className: "btn btn--sm", onClick: () => navigator.clipboard?.writeText(editorState.source), disabled: editorState.copyDisabled }, editorState.copyLabel), /* @__PURE__ */ React.createElement("button", { className: "btn btn--ghost btn--sm", onClick: onClose }, editorState.closeLabel))), /* @__PURE__ */ React.createElement(SourceCodePanel, { state, busy, compact: true }));
}
function highlightToml(s) {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/^(\s*#.*)$/gm, '<span class="toml-comment">$1</span>').replace(/^(\s*)(\[[^\]]+\])/gm, '$1<span class="toml-table">$2</span>').replace(/^(\s*)([A-Za-z_][\w-]*)(\s*=)/gm, '$1<span class="toml-key">$2</span>$3');
}
window.DrySim = DrySim;
window.ValidateSheet = ValidateSheet;
window.SourceDrawer = SourceDrawer;
window.InlineSourceEditor = InlineSourceEditor;

}

/* agents.jsx */

{
function AgentsView({ studio, agentSel, setAgentSel, contract, deploySettings, flow, setFlow, mobSettings, setMobSettings, toolCatalog = [], modelCatalog = [], agentDefinitions = [] }) {
  return /* @__PURE__ */ React.createElement("div", { className: "agents-view" }, /* @__PURE__ */ React.createElement(AgentsList, { studio, agentSel, setAgentSel, contract, agentDefinitions }), /* @__PURE__ */ React.createElement("div", { className: "agents-view__main" }, /* @__PURE__ */ React.createElement(AgentsMain, { studio, agentSel, setAgentSel, contract, deploySettings, flow, setFlow, mobSettings, setMobSettings, toolCatalog, modelCatalog })));
}
function AgentsList({ studio, agentSel, setAgentSel, contract, agentDefinitions }) {
  const listState = window.MobKitFlowController.agentListState({
    members: studio.members,
    instances: studio.instances,
    schemas: studio.schemas,
    selection: agentSel
  });
  return /* @__PURE__ */ React.createElement("aside", { className: "agents-list" }, /* @__PURE__ */ React.createElement("div", { className: "agents-list__head" }, /* @__PURE__ */ React.createElement("span", { className: "agents-list__title" }, "AGENTS"), /* @__PURE__ */ React.createElement("span", { className: "agents-list__count" }, listState.memberCount)), /* @__PURE__ */ React.createElement("div", { className: "agents-list__scroll" }, listState.memberRows.map((row) => {
    return /* @__PURE__ */ React.createElement(
      "button",
      {
        key: row.id,
        className: row.itemClass,
        onClick: () => setAgentSel({ kind: "agent", id: row.id })
      },
      /* @__PURE__ */ React.createElement("span", { className: "agents-list__bullet", "data-role": row.bulletRole }, "\u25CF"),
      /* @__PURE__ */ React.createElement("div", { className: "agents-list__col" }, /* @__PURE__ */ React.createElement("span", { className: "agents-list__name" }, row.name), /* @__PURE__ */ React.createElement("span", { className: "agents-list__sub" }, row.subLabel)),
      /* @__PURE__ */ React.createElement("span", { className: row.placedClass }, row.placedLabel)
    );
  }), /* @__PURE__ */ React.createElement(AddAgentControl, { studio, setAgentSel, agentDefinitions })), /* @__PURE__ */ React.createElement("div", { className: "agents-list__head agents-list__head--sub" }, /* @__PURE__ */ React.createElement("span", { className: "agents-list__title" }, "SCHEMAS"), /* @__PURE__ */ React.createElement("span", { className: "agents-list__count" }, listState.schemaCount)), /* @__PURE__ */ React.createElement("div", { className: "agents-list__scroll" }, listState.schemaRows.map((row) => {
    return /* @__PURE__ */ React.createElement(
      "button",
      {
        key: row.id,
        className: row.itemClass,
        onClick: () => setAgentSel({ kind: "schema", id: row.id })
      },
      /* @__PURE__ */ React.createElement("span", { className: "agents-list__bullet", "data-role": row.bulletRole }, "\u25A2"),
      /* @__PURE__ */ React.createElement("div", { className: "agents-list__col" }, /* @__PURE__ */ React.createElement("span", { className: "agents-list__name" }, row.id), /* @__PURE__ */ React.createElement("span", { className: "agents-list__sub" }, row.subLabel))
    );
  }), /* @__PURE__ */ React.createElement(
    "button",
    {
      className: "agents-list__add",
      onClick: () => {
        const result = window.MobKitFlowController.schemaDefinitionAddPatch(studio.schemas, contract);
        if (result.ok === false) return;
        if (studio.snap) studio.snap();
        studio.setSchemas(result.schemas);
        setAgentSel({ kind: "schema", id: result.schema.id });
      }
    },
    "+ new schema"
  )));
}
function AddAgentControl({ studio, setAgentSel, agentDefinitions = [] }) {
  const definitionState = window.MobKitFlowController.agentDefinitionAddControlState(agentDefinitions);
  const createFromDefinition = (definitionId) => {
    const result = window.MobKitFlowController.agentDefinitionAddByIdPatch(agentDefinitions, definitionId, {
      members: studio.members,
      schemas: studio.schemas
    });
    if (!result.ok) return;
    if (studio.snap) studio.snap();
    if (result.schemas !== studio.schemas) studio.setSchemas(result.schemas);
    studio.setMembers(result.members);
    setAgentSel({ kind: "agent", id: result.member.id });
  };
  if (!definitionState.hasDefinitions) {
    return /* @__PURE__ */ React.createElement(
      "button",
      {
        className: definitionState.controlClass,
        disabled: true,
        title: definitionState.title
      },
      definitionState.unavailableLabel
    );
  }
  return /* @__PURE__ */ React.createElement(
    "select",
    {
      className: definitionState.controlClass,
      value: definitionState.value,
      title: definitionState.title,
      onChange: (e) => {
        const id = e.target.value;
        if (!id) return;
        createFromDefinition(id);
        e.target.value = "";
      }
    },
    /* @__PURE__ */ React.createElement("option", { value: definitionState.placeholderOption.value }, definitionState.placeholderOption.label),
    definitionState.optionRows.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value }, option.label))
  );
}
function AgentsMain({ studio, agentSel, setAgentSel, contract, deploySettings, flow, setFlow, mobSettings, setMobSettings, toolCatalog, modelCatalog }) {
  const selectionState = window.MobKitFlowController.agentSelectionState({
    selection: agentSel,
    members: studio.members,
    schemas: studio.schemas
  });
  if (selectionState.kind === "empty") {
    return /* @__PURE__ */ React.createElement("div", { className: "agents-empty" }, /* @__PURE__ */ React.createElement("div", { className: "agents-empty__head" }, "AGENT LIBRARY"), /* @__PURE__ */ React.createElement("div", { className: "agents-empty__line" }, "Select an agent or schema on the left."), /* @__PURE__ */ React.createElement("div", { className: "agents-empty__line" }, "Agents are reusable across topologies. Edit one here and every placement updates."));
  }
  if (selectionState.kind === "schema") {
    if (!selectionState.schema) return /* @__PURE__ */ React.createElement("div", { className: "agents-empty" }, "Schema not found.");
    return /* @__PURE__ */ React.createElement(SchemaEditor, { studio, schema: selectionState.schema, setAgentSel, contract, flow, setFlow });
  }
  if (!selectionState.member) return /* @__PURE__ */ React.createElement("div", { className: "agents-empty" }, "Agent not found.");
  return /* @__PURE__ */ React.createElement(AgentEditor, { studio, member: selectionState.member, setAgentSel, contract, deploySettings, flow, setFlow, mobSettings, setMobSettings, toolCatalog, modelCatalog });
}
function AgentEditor({ studio, member, setAgentSel, contract, deploySettings, flow, setFlow, mobSettings, setMobSettings, toolCatalog = [], modelCatalog = [] }) {
  const change = (patch) => studio.updateMember(member.id, patch);
  const [toolDraft, setToolDraft] = React.useState("");
  const [toolDraftError, setToolDraftError] = React.useState("");
  const toolAccessState = window.MobKitFlowController.memberToolAccessState(member, toolCatalog);
  const editorState = window.MobKitFlowController.agentEditorControlState({
    member,
    instances: studio.instances,
    schemas: studio.schemas,
    contract,
    deploySettings,
    modelCatalog
  });
  const addToolAccess = (raw) => {
    const result = window.MobKitFlowController.memberToolAccessCascadePatch({
      memberId: member.id,
      members: studio.members,
      flow,
      instances: studio.instances
    }, raw, toolCatalog);
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
      instances: studio.instances
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
      schemas: studio.schemas
    }, rawSchema);
    if (!result.ok) return;
    if (studio.snap) studio.snap();
    studio.setMembers(result.members);
    if (result.flow !== flow && setFlow) setFlow(result.flow);
    if (result.edges !== studio.edges) studio.setEdges(result.edges);
  };
  return /* @__PURE__ */ React.createElement("div", { className: "agent-editor" }, /* @__PURE__ */ React.createElement("div", { className: "agent-editor__head" }, /* @__PURE__ */ React.createElement("div", { className: "row row--between" }, /* @__PURE__ */ React.createElement("div", null, /* @__PURE__ */ React.createElement("div", { className: "inspector__eyebrow" }, "AGENT \xB7 ", member.role), /* @__PURE__ */ React.createElement(
    "input",
    {
      className: "agent-editor__title-input",
      value: member.name,
      onChange: (e) => change(window.MobKitFlowController.memberNamePatch(e.target.value))
    }
  ), /* @__PURE__ */ React.createElement("div", { className: "inspector__id" }, editorState.idLine)), /* @__PURE__ */ React.createElement("button", { className: "btn btn--ghost btn--sm", onClick: () => {
    if (editorState.deleteNeedsConfirmation) {
      if (!confirm(editorState.deleteConfirmMessage)) return;
    }
    const result = window.MobKitFlowController.memberDeleteCascadePatch({
      memberId: member.id,
      members: studio.members,
      instances: studio.instances,
      edges: studio.edges,
      flow,
      mobSettings
    });
    if (!result.ok) return;
    if (studio.snap) studio.snap();
    studio.setMembers(result.members);
    studio.setInstances(result.instances);
    studio.setEdges(result.edges);
    if (setFlow) setFlow(result.flow);
    if (setMobSettings) setMobSettings(result.mobSettings);
    setAgentSel(null);
  } }, editorState.deleteLabel))), /* @__PURE__ */ React.createElement("div", { className: "agent-editor__body" }, /* @__PURE__ */ React.createElement("div", { className: "agent-editor__cols" }, /* @__PURE__ */ React.createElement("div", { className: "agent-editor__col" }, /* @__PURE__ */ React.createElement("div", { className: "section" }, /* @__PURE__ */ React.createElement("div", { className: "section__title" }, editorState.identityTitle), /* @__PURE__ */ React.createElement("div", { className: "field" }, /* @__PURE__ */ React.createElement("label", { className: "field__label" }, editorState.profileBindingLabel), /* @__PURE__ */ React.createElement(
    "select",
    {
      className: "field__select",
      value: editorState.profileBinding,
      onChange: (e) => change(window.MobKitFlowController.memberProfileBindingPatch(member, e.target.value, contract))
    },
    editorState.bindingOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value, disabled: option.disabled }, option.label))
  ), editorState.selectedBinding?.reason && /* @__PURE__ */ React.createElement("div", { className: "hint__line", style: { color: "var(--warn)" } }, editorState.selectedBinding.reason)), editorState.isRealmProfile ? /* @__PURE__ */ React.createElement("div", { className: "field" }, /* @__PURE__ */ React.createElement("label", { className: "field__label" }, editorState.realmProfileLabel), /* @__PURE__ */ React.createElement(
    "input",
    {
      className: "field__input field__input--mono",
      value: member.realmProfile || "",
      placeholder: editorState.realmProfilePlaceholder,
      onChange: (e) => change(window.MobKitFlowController.memberRealmProfilePatch(e.target.value))
    }
  ), /* @__PURE__ */ React.createElement("div", { className: "hint__line" }, editorState.realmProfileImportHint)) : /* @__PURE__ */ React.createElement(React.Fragment, null, /* @__PURE__ */ React.createElement("div", { className: "field" }, /* @__PURE__ */ React.createElement("label", { className: "field__label" }, editorState.modelLabel), /* @__PURE__ */ React.createElement("select", { className: "field__select", value: member.model, onChange: (e) => change(window.MobKitFlowController.memberModelPatch(e.target.value, modelCatalog)) }, editorState.modelOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value }, option.label)))), /* @__PURE__ */ React.createElement("div", { className: "field" }, /* @__PURE__ */ React.createElement("label", { className: "field__label" }, editorState.runtimeModeLabel), /* @__PURE__ */ React.createElement("select", { className: "field__select", value: editorState.runtimeMode, onChange: (e) => change(window.MobKitFlowController.memberRuntimeModePatch(e.target.value, contract, deploySettings)) }, editorState.runtimeOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value, disabled: option.disabled }, option.label))), editorState.selectedRuntime?.reason && /* @__PURE__ */ React.createElement("div", { className: "hint__line", style: { color: "var(--warn)" } }, editorState.selectedRuntime.reason)), /* @__PURE__ */ React.createElement("div", { className: "field" }, /* @__PURE__ */ React.createElement("label", { className: "field__label" }, editorState.backendLabel), /* @__PURE__ */ React.createElement("select", { className: "field__select", value: editorState.backendValue, onChange: (e) => change(window.MobKitFlowController.memberBackendPatch(e.target.value, contract)) }, editorState.backendOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value || "default", value: option.value, disabled: option.disabled }, option.label))), editorState.selectedBackend?.reason && /* @__PURE__ */ React.createElement("div", { className: "hint__line", style: { color: "var(--warn)" } }, editorState.selectedBackend.reason)), /* @__PURE__ */ React.createElement("div", { className: "field" }, /* @__PURE__ */ React.createElement("label", { className: "field__label" }, editorState.inlinePeerNotificationsLabel), /* @__PURE__ */ React.createElement(
    "input",
    {
      className: "field__input",
      type: "number",
      min: "-1",
      step: "1",
      value: member.maxInlinePeerNotifications ?? "",
      placeholder: editorState.inlinePeerNotificationsPlaceholder,
      onChange: (e) => change(window.MobKitFlowController.memberMaxInlinePeerNotificationsPatch(e.target.value))
    }
  )), /* @__PURE__ */ React.createElement(ProviderParamsEditor, { member, change }))), !editorState.isRealmProfile && /* @__PURE__ */ React.createElement("div", { className: "section" }, /* @__PURE__ */ React.createElement("div", { className: "section__title section__title--row" }, /* @__PURE__ */ React.createElement("span", null, editorState.systemPromptTitle), /* @__PURE__ */ React.createElement("button", { className: "ghost-btn", onClick: () => change(window.MobKitFlowController.memberSystemPromptPatch(window.MobKitFlowController.memberPromptSkeleton(member))), title: editorState.applySkeletonTitle }, editorState.applySkeletonLabel)), /* @__PURE__ */ React.createElement(
    "textarea",
    {
      className: "field__textarea",
      rows: 8,
      value: member.systemPrompt || "",
      onChange: (e) => change(window.MobKitFlowController.memberSystemPromptPatch(e.target.value)),
      placeholder: editorState.systemPromptPlaceholder
    }
  ))), /* @__PURE__ */ React.createElement("div", { className: "agent-editor__col" }, editorState.isRealmProfile ? /* @__PURE__ */ React.createElement("div", { className: "section" }, /* @__PURE__ */ React.createElement("div", { className: "section__title" }, editorState.realmProfileTitle), /* @__PURE__ */ React.createElement("div", { className: "hint__line" }, editorState.realmProfileReferenceHintBefore, " ", /* @__PURE__ */ React.createElement("code", null, editorState.realmProfileReferenceLabel), " ", editorState.realmProfileReferenceHintAfter)) : /* @__PURE__ */ React.createElement(React.Fragment, null, /* @__PURE__ */ React.createElement("div", { className: "section" }, /* @__PURE__ */ React.createElement("div", { className: "section__title" }, toolAccessState.title), /* @__PURE__ */ React.createElement("div", { className: "hint__line", style: { marginBottom: 8 } }, toolAccessState.hint), toolAccessState.rows.map((row) => {
    return /* @__PURE__ */ React.createElement("div", { key: row.id, className: row.className }, /* @__PURE__ */ React.createElement("div", null, /* @__PURE__ */ React.createElement("div", { className: "name" }, row.name), /* @__PURE__ */ React.createElement("div", { className: "auth" }, row.description)), /* @__PURE__ */ React.createElement("button", { onClick: () => removeToolAccess(row.id) }, row.removeLabel));
  }), /* @__PURE__ */ React.createElement(
    "select",
    {
      className: "field__select",
      value: toolAccessState.addSelectValue,
      onChange: (e) => {
        const id = e.target.value;
        if (!id) return;
        addToolAccess(id);
      }
    },
    /* @__PURE__ */ React.createElement("option", { value: toolAccessState.addSelectValue }, toolAccessState.addSelectPlaceholder),
    toolAccessState.addableRows.map((row) => /* @__PURE__ */ React.createElement("option", { key: row.id, value: row.value }, row.optionLabel))
  ), /* @__PURE__ */ React.createElement("div", { className: "field", style: { marginTop: 8 } }, /* @__PURE__ */ React.createElement("label", { className: "field__label" }, toolAccessState.sourceLabel), /* @__PURE__ */ React.createElement("div", { className: "row row--gap" }, /* @__PURE__ */ React.createElement(
    "input",
    {
      className: "field__input field__input--mono",
      value: toolDraft,
      placeholder: toolAccessState.sourcePlaceholder,
      onChange: (e) => {
        setToolDraft(e.target.value);
        setToolDraftError("");
      },
      onKeyDown: (e) => {
        if (e.key === "Enter") addToolAccess(toolDraft);
      }
    }
  ), /* @__PURE__ */ React.createElement("button", { className: "btn btn--ghost btn--sm", onClick: () => addToolAccess(toolDraft) }, toolAccessState.addButtonLabel)), toolDraftError && /* @__PURE__ */ React.createElement("div", { className: "hint__line" }, toolDraftError))), /* @__PURE__ */ React.createElement("div", { className: "section" }, /* @__PURE__ */ React.createElement("div", { className: "section__title" }, editorState.outputSchemaTitle), /* @__PURE__ */ React.createElement(
    "select",
    {
      className: "field__select",
      value: member.schema || "",
      onChange: (e) => changeSchema(e.target.value)
    },
    editorState.schemaOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value || "none", value: option.value }, option.label))
  ), editorState.hasOutputSchema ? /* @__PURE__ */ React.createElement(React.Fragment, null, /* @__PURE__ */ React.createElement("ul", { className: "schema-fields schema-fields--preview" }, editorState.schemaPreviewRows.map((f) => /* @__PURE__ */ React.createElement("li", { key: f.id }, /* @__PURE__ */ React.createElement("span", { className: "sf__name" }, f.name), /* @__PURE__ */ React.createElement("span", { className: "sf__type" }, f.type), f.required && /* @__PURE__ */ React.createElement("span", { className: "sf__req" }, f.requiredLabel)))), /* @__PURE__ */ React.createElement("button", { className: "link", onClick: () => setAgentSel(editorState.editSchemaSelection) }, editorState.editSchemaLabel)) : /* @__PURE__ */ React.createElement("div", { className: "hint__line", style: { marginTop: 6 } }, editorState.emptySchemaHint)), /* @__PURE__ */ React.createElement("div", { className: "section" }, /* @__PURE__ */ React.createElement(SkillAccess, { studio, member }))), /* @__PURE__ */ React.createElement("div", { className: "section" }, /* @__PURE__ */ React.createElement("div", { className: "section__title" }, editorState.usageTitle), editorState.placedCount === 0 && /* @__PURE__ */ React.createElement("div", { className: "hint__line" }, editorState.emptyUsageHint), editorState.usageRows.map((row) => /* @__PURE__ */ React.createElement("div", { key: row.id, className: "usage-row usage-row--ro" }, /* @__PURE__ */ React.createElement("span", { className: "usage-row__label" }, row.id), /* @__PURE__ */ React.createElement("span", { className: "usage-row__cell" }, row.cellLabel), /* @__PURE__ */ React.createElement("span", { className: "usage-row__lane" }, row.laneLabel))))))));
}
function SchemaEditor({ studio, schema, setAgentSel, contract, flow, setFlow }) {
  const change = (patch) => studio.updateSchema(schema.id, patch);
  const schemaState = window.MobKitFlowController.schemaEditorControlState({
    schema,
    members: studio.members
  });
  const reconcileFieldReferences = (oldName, newName) => {
    if (!window.MobKitFlowController?.reconcileSchemaFieldReferences) return;
    const result = window.MobKitFlowController.reconcileSchemaFieldReferences({
      flow,
      edges: studio.edges,
      members: studio.members,
      instances: studio.instances,
      schemaId: schema.id,
      oldName,
      newName
    });
    const flowChanged = result.flow !== flow;
    const edgesChanged = result.edges !== studio.edges;
    if (edgesChanged && studio.snap) studio.snap();
    if (flowChanged && setFlow) setFlow(result.flow);
    if (edgesChanged) studio.setEdges(result.edges);
  };
  const updateField = (fieldId, patch) => change(window.MobKitFlowController.schemaFieldUpdatePatch(schema, fieldId, patch, contract));
  const deleteField = (fieldId) => {
    const result = window.MobKitFlowController.schemaFieldDeleteCascadePatch({
      schema,
      schemas: studio.schemas,
      flow,
      edges: studio.edges,
      members: studio.members,
      instances: studio.instances
    }, fieldId);
    if (studio.snap) studio.snap();
    studio.setSchemas(result.schemas);
    if (result.flow !== flow && setFlow) setFlow(result.flow);
    if (result.edges !== studio.edges) studio.setEdges(result.edges);
  };
  const addField = () => {
    const result = window.MobKitFlowController.schemaFieldAddPatch(schema, contract);
    if (result.ok === false) return;
    change(result.patch);
  };
  const deleteSchema = () => {
    const result = window.MobKitFlowController.studioDeleteSchemaPatch({
      schemas: studio.schemas,
      members: studio.members,
      flow,
      edges: studio.edges,
      instances: studio.instances
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
      members: studio.members
    }, schema.id, newId);
    if (!result.renamed) return;
    if (studio.snap) studio.snap();
    studio.setSchemas(result.schemas);
    studio.setMembers(result.members);
    setAgentSel({ kind: "schema", id: String(newId || "").trim() });
  };
  return /* @__PURE__ */ React.createElement("div", { className: "agent-editor" }, /* @__PURE__ */ React.createElement("div", { className: "agent-editor__head" }, /* @__PURE__ */ React.createElement("div", { className: "row row--between" }, /* @__PURE__ */ React.createElement("div", null, /* @__PURE__ */ React.createElement("div", { className: "inspector__eyebrow" }, schemaState.eyebrow), /* @__PURE__ */ React.createElement(
    "input",
    {
      className: "agent-editor__title-input",
      defaultValue: schema.id,
      onBlur: (e) => renameSchema(e.target.value),
      onKeyDown: (e) => {
        if (e.key === "Enter") e.target.blur();
      }
    }
  ), /* @__PURE__ */ React.createElement("div", { className: "inspector__id" }, schemaState.usageLabel)), /* @__PURE__ */ React.createElement(
    "button",
    {
      className: "btn btn--ghost btn--sm",
      disabled: !schemaState.canDelete,
      title: schemaState.deleteTitle,
      onClick: deleteSchema
    },
    schemaState.deleteLabel
  ))), /* @__PURE__ */ React.createElement("div", { className: "agent-editor__body" }, /* @__PURE__ */ React.createElement("div", { className: "section" }, /* @__PURE__ */ React.createElement("div", { className: "section__title" }, schemaState.descriptionTitle), /* @__PURE__ */ React.createElement(
    "textarea",
    {
      className: "field__textarea",
      rows: 2,
      value: schema.description || "",
      placeholder: schemaState.descriptionPlaceholder,
      onChange: (e) => change(window.MobKitFlowController.schemaDescriptionPatch(e.target.value))
    }
  )), /* @__PURE__ */ React.createElement("div", { className: "section" }, /* @__PURE__ */ React.createElement("div", { className: "row row--between", style: { marginBottom: 6 } }, /* @__PURE__ */ React.createElement("div", { className: "section__title" }, schemaState.fieldsTitle), /* @__PURE__ */ React.createElement("button", { className: "btn btn--ghost btn--sm", onClick: addField }, schemaState.addFieldLabel)), /* @__PURE__ */ React.createElement("div", { className: "schema-builder" }, /* @__PURE__ */ React.createElement("div", { className: "schema-builder__header" }, /* @__PURE__ */ React.createElement("span", { className: "sb-col sb-col--name" }, schemaState.headerLabels.name), /* @__PURE__ */ React.createElement("span", { className: "sb-col sb-col--type" }, schemaState.headerLabels.type), /* @__PURE__ */ React.createElement("span", { className: "sb-col sb-col--req" }, schemaState.headerLabels.required), /* @__PURE__ */ React.createElement("span", { className: "sb-col sb-col--desc" }, schemaState.headerLabels.description), /* @__PURE__ */ React.createElement("span", { className: "sb-col sb-col--act" }, schemaState.headerLabels.action)), schemaState.fieldRows.map(({ field: f }) => /* @__PURE__ */ React.createElement(
    SchemaField,
    {
      key: f.id,
      field: f,
      normalizeName: (raw) => window.MobKitFlowController.uniqueSchemaFieldName(schema.fields, raw, f.id),
      onChange: (patch) => updateField(f.id, patch),
      onRename: (oldName, newName) => reconcileFieldReferences(oldName, newName),
      onDelete: () => deleteField(f.id),
      contract
    }
  )), schemaState.fieldRows.length === 0 && /* @__PURE__ */ React.createElement("div", { className: "schema-builder__empty" }, schemaState.emptyFieldsHint))), /* @__PURE__ */ React.createElement("div", { className: "section" }, /* @__PURE__ */ React.createElement("div", { className: "section__title" }, schemaState.usedByTitle), schemaState.usedCount === 0 && /* @__PURE__ */ React.createElement("div", { className: "hint__line" }, schemaState.emptyUsedByHint), schemaState.usedBy.map((row) => /* @__PURE__ */ React.createElement(
    "button",
    {
      key: row.id,
      className: "usage-row",
      onClick: () => setAgentSel(row.selection)
    },
    /* @__PURE__ */ React.createElement("span", { className: "usage-row__label" }, row.name),
    /* @__PURE__ */ React.createElement("span", { className: "usage-row__cell" }, row.role),
    /* @__PURE__ */ React.createElement("span", { className: "usage-row__lane" }, row.model)
  )))));
}
function SchemaField({ field, normalizeName, onChange, onRename, onDelete, contract }) {
  const nameBeforeEdit = React.useRef(field.name);
  const fieldState = window.MobKitFlowController.schemaFieldRowControlState(field, contract);
  const typeState = fieldState.typeState;
  const values = fieldState.enumValues;
  return /* @__PURE__ */ React.createElement("div", { className: "schema-field" }, /* @__PURE__ */ React.createElement(
    "input",
    {
      className: "sb-input sb-col--name",
      value: field.name,
      onFocus: () => {
        nameBeforeEdit.current = field.name;
      },
      onChange: (e) => onChange({ name: e.target.value }),
      onBlur: (e) => {
        const normalized = normalizeName(e.target.value);
        const previous = String(nameBeforeEdit.current || "").trim();
        onChange({ name: normalized });
        if (previous && previous !== normalized && onRename) onRename(previous, normalized);
      },
      placeholder: fieldState.namePlaceholder
    }
  ), /* @__PURE__ */ React.createElement(
    "select",
    {
      className: "sb-select sb-col--type",
      value: typeState.type,
      onChange: (e) => {
        onChange(window.MobKitFlowController.schemaLikeFieldTypePatch(field, e.target.value, contract));
      }
    },
    typeState.typeOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value, disabled: option.disabled }, option.label))
  ), typeState.selectedType?.reason && /* @__PURE__ */ React.createElement("div", { className: "hint__line", style: { color: "var(--warn)" } }, typeState.selectedType.reason), /* @__PURE__ */ React.createElement("label", { className: "sb-col--req sb-checkbox" }, /* @__PURE__ */ React.createElement(
    "input",
    {
      type: "checkbox",
      checked: !!field.required,
      onChange: (e) => onChange({ required: e.target.checked })
    }
  )), /* @__PURE__ */ React.createElement(
    "input",
    {
      className: "sb-input sb-col--desc",
      value: field.description || "",
      onChange: (e) => onChange({ description: e.target.value }),
      placeholder: fieldState.descriptionPlaceholder
    }
  ), /* @__PURE__ */ React.createElement("button", { className: "sb-del", onClick: onDelete, title: fieldState.removeTitle }, "\xD7"), field.type === "enum" && /* @__PURE__ */ React.createElement("div", { className: "sb-enum" }, /* @__PURE__ */ React.createElement("span", { className: "sb-enum__label" }, fieldState.enumLabel), /* @__PURE__ */ React.createElement("div", { className: "sb-enum__chips" }, values.map((v, i) => /* @__PURE__ */ React.createElement("span", { key: i, className: "chip" }, /* @__PURE__ */ React.createElement(
    "input",
    {
      className: "chip__input",
      value: v,
      onChange: (e) => onChange(window.MobKitFlowController.enumValueDraftPatch(field, i, e.target.value)),
      onBlur: (e) => onChange(window.MobKitFlowController.enumValueCommitPatch(field, i, e.target.value))
    }
  ), /* @__PURE__ */ React.createElement(
    "button",
    {
      className: "chip__x",
      onClick: () => onChange(window.MobKitFlowController.enumValueDeletePatch(field, i))
    },
    "\xD7"
  ))), /* @__PURE__ */ React.createElement(
    "button",
    {
      className: "chip chip--add",
      onClick: () => onChange(window.MobKitFlowController.enumValueAddPatch(field, fieldState.enumAddValue))
    },
    fieldState.enumAddLabel
  ))));
}
function ProviderParamsEditor({ member, change }) {
  const paramsState = window.MobKitFlowController.memberProviderParamsEditorState(member);
  const [draft, setDraft] = React.useState(paramsState.text);
  const [error, setError] = React.useState("");
  React.useEffect(() => {
    setDraft(paramsState.text);
    setError("");
  }, [member.id, paramsState.text]);
  const commit = (next) => {
    setDraft(next);
    const result = window.MobKitFlowController.memberProviderParamsPatch(next);
    if (!result.ok) {
      setError(result.error || paramsState.invalidJsonLabel);
      return;
    }
    setError("");
    change(result.patch);
  };
  return /* @__PURE__ */ React.createElement("div", { className: "field" }, /* @__PURE__ */ React.createElement("label", { className: "field__label" }, paramsState.label), /* @__PURE__ */ React.createElement(
    "textarea",
    {
      className: "field__textarea field__textarea--mono",
      rows: paramsState.rows,
      value: draft,
      placeholder: paramsState.placeholder,
      onChange: (e) => commit(e.target.value)
    }
  ), error && /* @__PURE__ */ React.createElement("div", { className: "hint__line", style: { color: "var(--danger)" } }, error));
}
function SkillAccess({ studio, member }) {
  const realms = studio.skillRealms || [];
  const initialSkillState = window.MobKitFlowController.memberSkillAccessState({ member, skillRealms: realms });
  const [realmId, setRealmId] = React.useState(initialSkillState.realmId);
  const [inlineOpen, setInlineOpen] = React.useState(false);
  const [inlineLabel, setInlineLabel] = React.useState("");
  const [inlineContent, setInlineContent] = React.useState("");
  const [inlineError, setInlineError] = React.useState("");
  const skillState = window.MobKitFlowController.memberSkillAccessState({ member, skillRealms: realms, realmId, inlineOpen });
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
      skillRealms: realms
    }, sid));
  };
  const removeSkill = (sid) => {
    applySkillCascade(window.MobKitFlowController.memberSkillRemoveCascadePatch({
      memberId: member.id,
      members: studio.members,
      skillRealms: realms
    }, sid));
  };
  const addInlineSkill = () => {
    try {
      const result = window.MobKitFlowController.memberInlineSkillCascadePatch({
        memberId: member.id,
        members: studio.members,
        skillRealms: realms
      }, {
        label: inlineLabel,
        content: inlineContent
      });
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
  return /* @__PURE__ */ React.createElement(React.Fragment, null, /* @__PURE__ */ React.createElement("div", { className: "section__title section__title--row" }, /* @__PURE__ */ React.createElement("span", null, skillState.sectionTitle), /* @__PURE__ */ React.createElement("button", { className: "ghost-btn", onClick: () => setInlineOpen((open) => !open) }, skillState.inlineToggleLabel)), /* @__PURE__ */ React.createElement("div", { className: "hint__line", style: { marginBottom: 8 } }, skillState.hint), inlineOpen && /* @__PURE__ */ React.createElement("div", { className: "inline-skill" }, /* @__PURE__ */ React.createElement(
    "input",
    {
      className: "field__input",
      value: inlineLabel,
      placeholder: skillState.inlineLabelPlaceholder,
      onChange: (e) => {
        setInlineLabel(e.target.value);
        setInlineError("");
      }
    }
  ), /* @__PURE__ */ React.createElement(
    "textarea",
    {
      className: "field__textarea field__textarea--mono",
      rows: skillState.inlineContentRows,
      value: inlineContent,
      placeholder: skillState.inlineContentPlaceholder,
      onChange: (e) => {
        setInlineContent(e.target.value);
        setInlineError("");
      }
    }
  ), /* @__PURE__ */ React.createElement("div", { className: "row row--between" }, /* @__PURE__ */ React.createElement("span", { className: "hint__line" }, skillState.inlineCreateHint), /* @__PURE__ */ React.createElement("button", { className: "btn btn--ghost btn--sm", onClick: addInlineSkill }, skillState.inlineAddLabel)), inlineError && /* @__PURE__ */ React.createElement("div", { className: "hint__line", style: { color: "var(--danger)" } }, inlineError)), !skillState.hasRealms ? /* @__PURE__ */ React.createElement("div", { className: "hint__line", style: { color: "var(--warn)" } }, skillState.noRealmsMessage) : /* @__PURE__ */ React.createElement(React.Fragment, null, /* @__PURE__ */ React.createElement("div", { className: "field" }, /* @__PURE__ */ React.createElement("label", { className: "field__label" }, skillState.realmLabel), /* @__PURE__ */ React.createElement("select", { className: "field__select", value: skillState.realmId, onChange: (e) => setRealmId(e.target.value) }, skillState.realmOptions.map((realm) => /* @__PURE__ */ React.createElement("option", { key: realm.id, value: realm.id }, realm.label)))), /* @__PURE__ */ React.createElement("div", { className: "skill-list" }, skillState.skillRows.map((row) => {
    return /* @__PURE__ */ React.createElement("button", { key: row.id, className: row.className, onClick: () => toggle(row.id) }, /* @__PURE__ */ React.createElement("span", { className: "skill-row__check" }, row.checkLabel), /* @__PURE__ */ React.createElement("span", { className: "skill-row__text" }, /* @__PURE__ */ React.createElement("span", { className: "skill-row__name" }, row.name), /* @__PURE__ */ React.createElement("span", { className: "skill-row__desc" }, row.desc)));
  }))), skillState.selectedOutsideRealm.length > 0 && /* @__PURE__ */ React.createElement("div", { className: "skill-other" }, /* @__PURE__ */ React.createElement("span", { className: "hint__line" }, skillState.outsideRealmHeading), skillState.selectedOutsideRealm.map((skill) => /* @__PURE__ */ React.createElement("span", { key: skill.id, className: skill.className, title: skill.title }, skill.label, /* @__PURE__ */ React.createElement("em", null, skill.detail), /* @__PURE__ */ React.createElement("button", { onClick: () => removeSkill(skill.id) }, skill.removeLabel)))), skillState.unavailableSelected.length > 0 && /* @__PURE__ */ React.createElement("div", { className: "skill-other" }, /* @__PURE__ */ React.createElement("span", { className: "hint__line", style: { color: "var(--warn)" } }, skillState.unavailableHeading), skillState.unavailableSelected.map((sid) => /* @__PURE__ */ React.createElement("span", { key: sid.id, className: sid.className }, sid.label, /* @__PURE__ */ React.createElement("button", { onClick: () => removeSkill(sid.id) }, sid.removeLabel)))));
}
window.AgentsView = AgentsView;

}

/* builder.jsx */

{
let _bid = 1;
const bid = (p) => p + "_" + (_bid++).toString(36);
function freshFlow() {
  _bid = 1;
  return { name: "untitled-mob", steps: [{ id: bid("s"), type: "input", task: "", fields: "", inputParams: [] }] };
}
function blankAuthoringFlow() {
  return freshFlow();
}
function CondValue({ field, value, onChange }) {
  const control = window.MobKitFlowController.conditionValueControl(field, value);
  if (control.kind === "enum") {
    return /* @__PURE__ */ React.createElement("select", { className: "field__select bld-cond__val", value: control.value, onChange: (e) => onChange(e.target.value) }, control.optionRows.map((row) => /* @__PURE__ */ React.createElement("option", { key: row.value || "blank", value: row.value }, row.label)));
  }
  if (control.kind === "boolean") {
    return /* @__PURE__ */ React.createElement("select", { className: "field__select bld-cond__val", value: control.value, onChange: (e) => onChange(e.target.value) }, control.optionRows.map((row) => /* @__PURE__ */ React.createElement("option", { key: row.value || "blank", value: row.value }, row.label)));
  }
  return /* @__PURE__ */ React.createElement("input", { className: "field__input bld-cond__val", placeholder: control.placeholder, value: control.value, onChange: (e) => onChange(e.target.value) });
}
function InputParamField({ param, normalizeName, onRename, onChange, onDelete, contract }) {
  const fieldState = window.MobKitFlowController.inputParamFieldControlState(param, contract);
  const values = fieldState.enumValues;
  const previousNameRef = React.useRef(null);
  const typeState = fieldState.typeState;
  return /* @__PURE__ */ React.createElement("div", { className: "schema-field" }, /* @__PURE__ */ React.createElement(
    "input",
    {
      className: "sb-input sb-col--name",
      value: param.name || "",
      onFocus: () => {
        previousNameRef.current = param.name || "";
      },
      onChange: (e) => onChange({ name: e.target.value }),
      onBlur: (e) => {
        const previousName = previousNameRef.current ?? param.name;
        previousNameRef.current = null;
        onRename?.(normalizeName(e.target.value), previousName);
      },
      placeholder: fieldState.namePlaceholder
    }
  ), /* @__PURE__ */ React.createElement(
    "select",
    {
      className: "sb-select sb-col--type",
      value: typeState.type,
      onChange: (e) => {
        onChange(window.MobKitFlowController.schemaLikeFieldTypePatch(param, e.target.value, contract));
      }
    },
    typeState.typeOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value, disabled: option.disabled }, option.label))
  ), typeState.selectedType?.reason && /* @__PURE__ */ React.createElement("div", { className: "hint__line", style: { color: "var(--warn)" } }, typeState.selectedType.reason), /* @__PURE__ */ React.createElement("label", { className: "sb-col--req sb-checkbox" }, /* @__PURE__ */ React.createElement("input", { type: "checkbox", checked: param.required !== false, onChange: (e) => onChange({ required: e.target.checked }) })), /* @__PURE__ */ React.createElement(
    "input",
    {
      className: "sb-input sb-col--desc",
      value: param.description || "",
      onChange: (e) => onChange({ description: e.target.value }),
      placeholder: fieldState.descriptionPlaceholder
    }
  ), /* @__PURE__ */ React.createElement("button", { className: "sb-del", onClick: onDelete, title: fieldState.removeTitle }, "\xD7"), param.type === "enum" && /* @__PURE__ */ React.createElement("div", { className: "sb-enum" }, /* @__PURE__ */ React.createElement("span", { className: "sb-enum__label" }, fieldState.enumLabel), /* @__PURE__ */ React.createElement("div", { className: "sb-enum__chips" }, values.map((value, index) => /* @__PURE__ */ React.createElement("span", { key: index, className: "chip" }, /* @__PURE__ */ React.createElement(
    "input",
    {
      className: "chip__input",
      value,
      onChange: (e) => onChange(window.MobKitFlowController.enumValueDraftPatch(param, index, e.target.value)),
      onBlur: (e) => onChange(window.MobKitFlowController.enumValueCommitPatch(param, index, e.target.value))
    }
  ), /* @__PURE__ */ React.createElement("button", { className: "chip__x", onClick: () => onChange(window.MobKitFlowController.enumValueDeletePatch(param, index)) }, "\xD7"))), /* @__PURE__ */ React.createElement("button", { className: "chip chip--add", onClick: () => onChange(window.MobKitFlowController.enumValueAddPatch(param, fieldState.enumAddValue)) }, fieldState.enumAddLabel))));
}
function BranchConditionEditor({ index, branch, options, schemas, onChange, contract }) {
  const conditionState = window.MobKitFlowController.basicBranchConditionControlState({
    branch,
    options,
    schemas,
    contract
  });
  return /* @__PURE__ */ React.createElement("div", { className: "bld-branch-card" }, /* @__PURE__ */ React.createElement("div", { className: "bld-branch-card__head" }, "Branch ", index + 1), !conditionState.hasConditionOptions ? /* @__PURE__ */ React.createElement("div", { className: "bld-hint", style: { color: "var(--warn)" } }, "Add an upstream member with an output schema before configuring this branch.") : /* @__PURE__ */ React.createElement(React.Fragment, null, /* @__PURE__ */ React.createElement("div", { className: "bld-cond" }, /* @__PURE__ */ React.createElement("select", { className: "field__select", value: conditionState.cond.stepId || "", onChange: (e) => onChange(window.MobKitFlowController.basicConditionSourcePatch(options, e.target.value, { includeNamespace: true })) }, /* @__PURE__ */ React.createElement("option", { value: "" }, "\u2014 source \u2014"), conditionState.sourceOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value }, option.label))), /* @__PURE__ */ React.createElement("select", { className: "field__select", value: conditionState.cond.field || "", onChange: (e) => onChange(window.MobKitFlowController.basicConditionFieldPatch(e.target.value, conditionState.fieldOptions)), disabled: !conditionState.fields.length }, /* @__PURE__ */ React.createElement("option", { value: "" }, conditionState.fieldPlaceholder), conditionState.fieldOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.field.id || option.value, value: option.value }, option.label))), /* @__PURE__ */ React.createElement("select", { className: "field__select bld-cond__op", value: conditionState.operatorValue, onChange: (e) => onChange(window.MobKitFlowController.basicConditionOperatorPatch(e.target.value, contract)) }, conditionState.operatorOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value, disabled: option.disabled }, option.label))), /* @__PURE__ */ React.createElement(CondValue, { field: conditionState.field, value: conditionState.cond.val, onChange: (v) => onChange(window.MobKitFlowController.basicConditionValuePatch(v)) })), /* @__PURE__ */ React.createElement("div", { className: "bld-cond__preview" }, "when ", /* @__PURE__ */ React.createElement("code", null, conditionState.previewLabel))));
}
function BuilderView({ studio, mode = "build", flow: flowProp, setFlow: setFlowProp, sel: selProp, setSel: setSelProp, onShowSource, sourceOpen = false, sourceDocument = null, sourceBusy = false, onCloseSource, contract, toolCatalog = [] }) {
  const members = studio?.members || [];
  const [flowLocal, setFlowLocal] = React.useState(freshFlow);
  const [selLocal, setSelLocal] = React.useState(null);
  const flow = flowProp || flowLocal;
  const setFlow = setFlowProp || setFlowLocal;
  const sel = selProp !== void 0 ? selProp : selLocal;
  const setSel = setSelProp || setSelLocal;
  const [picker, setPicker] = React.useState({ open: false });
  const [view, setView] = React.useState({ scale: 1, tx: 0, ty: 0 });
  const hostRef = React.useRef(null);
  const panRef = React.useRef(null);
  const isFlow = mode === "flow";
  const canvasView = Math.abs(view.ty) > 1200 ? { ...view, ty: 0 } : view;
  const update = (id, patch) => setFlow((f) => window.MobKitFlowController.flowStepUpdatePatch(f, id, patch, { members }));
  const reconcileInputParamReferences = (oldName, newName) => {
    if (!window.MobKitFlowController?.reconcileInputParamReferences) return;
    setFlow((current) => {
      const reconciled = window.MobKitFlowController.reconcileInputParamReferences({
        flow: current,
        edges: studio?.edges || [],
        oldName,
        newName
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
    setFlow((f) => window.MobKitFlowController.flowStepInsertPatch(f, laneRef, newStep, { members }));
    setSel(newStep.id);
    setPicker({ open: false });
  };
  const removeStep = (id) => {
    setFlow((f) => window.MobKitFlowController.flowStepDeletePatch(f, id));
    setSel(null);
    setPicker({ open: false });
  };
  const openPicker = (laneRef) => setPicker({ open: true, at: laneRef });
  const onWheel = (e) => {
    if (e.ctrlKey || e.metaKey) {
      e.preventDefault();
      const fz = Math.exp(-e.deltaY * 15e-4);
      setView((v) => {
        const r = hostRef.current.getBoundingClientRect();
        const cx = e.clientX - r.left, cy = e.clientY - r.top;
        const next = Math.max(0.4, Math.min(2, v.scale * fz));
        const k = next / v.scale;
        return { scale: next, tx: cx - (cx - v.tx) * k, ty: cy - (cy - v.ty) * k };
      });
    } else {
      e.preventDefault();
      setView((v) => ({ ...v, tx: v.tx - e.deltaX, ty: v.ty - e.deltaY }));
    }
  };
  React.useEffect(() => {
    const el = hostRef.current;
    if (!el) return;
    const h = (e) => onWheel(e);
    el.addEventListener("wheel", h, { passive: false });
    return () => el.removeEventListener("wheel", h);
  });
  const onHostDown = (e) => {
    if (e.target === hostRef.current || e.target.classList?.contains("bld-canvas")) {
      panRef.current = { sx: e.clientX, sy: e.clientY, tx: view.tx, ty: view.ty };
      const move = (ev) => setView((v) => ({ ...v, tx: panRef.current.tx + (ev.clientX - panRef.current.sx), ty: panRef.current.ty + (ev.clientY - panRef.current.sy) }));
      const up = () => {
        window.removeEventListener("mousemove", move);
        window.removeEventListener("mouseup", up);
      };
      window.addEventListener("mousemove", move);
      window.addEventListener("mouseup", up);
      setSel(null);
      setPicker({ open: false });
    }
  };
  return /* @__PURE__ */ React.createElement("div", { className: "builder" + (isFlow ? " builder--flow" : "") }, /* @__PURE__ */ React.createElement("div", { className: "bld-stage", ref: hostRef, onMouseDown: onHostDown }, /* @__PURE__ */ React.createElement("div", { className: "bld-canvas", style: { transform: `translate(calc(-50% + ${canvasView.tx}px), ${canvasView.ty}px) scale(${canvasView.scale})` } }, /* @__PURE__ */ React.createElement("div", { className: "bld-start" }, "START"), /* @__PURE__ */ React.createElement(
    Lane,
    {
      studio,
      mode,
      steps: flow.steps,
      laneRef: { lane: "main" },
      sel,
      contract,
      setSel: (id) => {
        setSel(id);
        setPicker({ open: false });
      },
      openPicker
    }
  )), /* @__PURE__ */ React.createElement("button", { className: "bld-toml-toggle", onMouseDown: (e) => e.stopPropagation(), onClick: () => onShowSource && onShowSource() }, "{ } mob.toml"), /* @__PURE__ */ React.createElement(
    InlineSourceEditor,
    {
      open: sourceOpen,
      onClose: () => onCloseSource && onCloseSource(),
      state: sourceDocument,
      busy: sourceBusy
    }
  ), /* @__PURE__ */ React.createElement("div", { className: "zoom-controls", onMouseDown: (e) => e.stopPropagation() }, /* @__PURE__ */ React.createElement("button", { className: "zoom-btn", onClick: () => setView((v) => ({ ...v, scale: Math.max(0.4, v.scale / 1.2) })) }, "\u2212"), /* @__PURE__ */ React.createElement("button", { className: "zoom-btn zoom-btn--pct", onClick: () => setView({ scale: 1, tx: 0, ty: 0 }) }, Math.round(view.scale * 100), "%"), /* @__PURE__ */ React.createElement("button", { className: "zoom-btn", onClick: () => setView((v) => ({ ...v, scale: Math.min(2, v.scale * 1.2) })) }, "+"))), /* @__PURE__ */ React.createElement("aside", { className: "bld-panel" }, picker.open ? /* @__PURE__ */ React.createElement(
    StepPicker,
    {
      members,
      isKickoff: picker.at?.lane === "main" && picker.at?.index === 0 && kickoffSlotEmpty(flow),
      contract,
      onPick: (pick) => insertAt(picker.at, pick),
      onClose: () => setPicker({ open: false })
    }
  ) : selStep ? /* @__PURE__ */ React.createElement(StepInspector, { studio, members, flow, step: selStep, update, onDelete: () => removeStep(selStep.id), contract, toolCatalog, onInputParamReferenceChange: reconcileInputParamReferences }) : /* @__PURE__ */ React.createElement(EmptyPanel, null)));
}
function Lane({ studio, mode, steps, laneRef, sel, setSel, openPicker, contract }) {
  return /* @__PURE__ */ React.createElement("div", { className: "bld-lane" }, steps.map((step, i) => /* @__PURE__ */ React.createElement(React.Fragment, { key: step.id }, /* @__PURE__ */ React.createElement(StepCard, { studio, step, index: i, selected: sel === step.id, onSelect: () => setSel(step.id), contract }), step.type === "branch" || step.type === "parallel" ? /* @__PURE__ */ React.createElement(React.Fragment, null, /* @__PURE__ */ React.createElement(Fork, { studio, mode, step, sel, setSel, openPicker, contract }), /* @__PURE__ */ React.createElement(InsertBtn, { mode, mid: i < steps.length - 1, onClick: () => openPicker({ ...laneRef, index: i + 1 }) })) : step.type === "repeat" ? /* @__PURE__ */ React.createElement(React.Fragment, null, /* @__PURE__ */ React.createElement(RepeatBody, { studio, mode, step, sel, setSel, openPicker, contract }), /* @__PURE__ */ React.createElement(InsertBtn, { mode, mid: i < steps.length - 1, onClick: () => openPicker({ ...laneRef, index: i + 1 }) })) : /* @__PURE__ */ React.createElement(InsertBtn, { mode, mid: i < steps.length - 1, onClick: () => openPicker({ ...laneRef, index: i + 1 }) }))), steps.length === 0 && /* @__PURE__ */ React.createElement(InsertBtn, { mode, onClick: () => openPicker({ ...laneRef, index: 0 }) }));
}
function Fork({ studio, mode, step, sel, setSel, openPicker, contract }) {
  const forkState = window.MobKitFlowController.basicForkCanvasState({ step, contract });
  return /* @__PURE__ */ React.createElement("div", { className: forkState.className }, /* @__PURE__ */ React.createElement("div", { className: "bld-fork__bar" }), forkState.showRail && /* @__PURE__ */ React.createElement("div", { className: "bld-fork__rail" }), /* @__PURE__ */ React.createElement("div", { className: "bld-fork__lanes" }, forkState.lanes.map((l) => /* @__PURE__ */ React.createElement("div", { className: "bld-fork__lane", key: l.id }, /* @__PURE__ */ React.createElement("div", { className: "bld-fork__drop" }), /* @__PURE__ */ React.createElement("div", { className: "bld-fork__label" }, l.label), /* @__PURE__ */ React.createElement("div", { className: "bld-fork__drop" }), l.steps.length === 0 ? /* @__PURE__ */ React.createElement(InsertBtn, { mode, onClick: () => openPicker({ lane: "branch", parentId: step.id, branchId: l.id, index: 0 }) }) : /* @__PURE__ */ React.createElement(Lane, { studio, mode, steps: l.steps, laneRef: { lane: "branch", parentId: step.id, branchId: l.id }, sel, setSel, openPicker, contract }), forkState.isParallel && /* @__PURE__ */ React.createElement("div", { className: "bld-fork__drop" })))), forkState.isParallel ? /* @__PURE__ */ React.createElement(React.Fragment, null, forkState.showRail && /* @__PURE__ */ React.createElement("div", { className: "bld-fork__rail bld-fork__rail--join" }), /* @__PURE__ */ React.createElement("div", { className: "bld-fork__bar" }), /* @__PURE__ */ React.createElement("div", { className: "bld-join" }, forkState.joinLabel)) : (
    // Branch paths reconverge to a single downstream column so the
    // following main-lane step connects cleanly (no diagonal jump).
    forkState.showRail && /* @__PURE__ */ React.createElement("div", { className: "bld-fork__rail bld-fork__rail--join" })
  ));
}
function RepeatBody({ studio, mode, step, sel, setSel, openPicker, contract }) {
  const repeatState = window.MobKitFlowController.basicRepeatCanvasState({ step, members: studio?.members || [], contract });
  return /* @__PURE__ */ React.createElement("div", { className: "bld-repeat" }, /* @__PURE__ */ React.createElement("div", { className: "bld-fork__bar" }), /* @__PURE__ */ React.createElement("div", { className: "bld-loop" }, /* @__PURE__ */ React.createElement("div", { className: "bld-loop__rail" }, /* @__PURE__ */ React.createElement("span", { className: "bld-loop__rail-glyph" }, "\u21BB")), /* @__PURE__ */ React.createElement("div", { className: "bld-loop__frame" }, /* @__PURE__ */ React.createElement("div", { className: "bld-loop__head" }, /* @__PURE__ */ React.createElement("span", { className: "bld-loop__badge" }, "LOOP"), /* @__PURE__ */ React.createElement("span", { className: "bld-loop__meta" }, "while ", /* @__PURE__ */ React.createElement("strong", null, "not"), " (", repeatState.conditionLabel, ") \xB7 ", repeatState.maxIterationsLabel)), step.steps.length === 0 ? /* @__PURE__ */ React.createElement(InsertBtn, { mode, onClick: () => openPicker({ lane: "branch", parentId: step.id, branchId: "body", index: 0 }) }) : /* @__PURE__ */ React.createElement(Lane, { studio, mode, steps: step.steps, laneRef: { lane: "branch", parentId: step.id, branchId: "body" }, sel, setSel, openPicker, contract }), /* @__PURE__ */ React.createElement("div", { className: "bld-loop__back" }, repeatState.loopBackLabel))), /* @__PURE__ */ React.createElement("div", { className: "bld-loop__exit" }, repeatState.exitLabel));
}
function StepCard({ studio, step, index, selected, onSelect, contract }) {
  const cardState = window.MobKitFlowController.basicStepCardState({ step, members: studio?.members || [], contract });
  return /* @__PURE__ */ React.createElement(
    "div",
    {
      className: "bld-card" + (selected ? " is-selected" : "") + (!cardState.configured ? " is-empty" : "") + (cardState.isFlowCard ? " bld-card--flow" : ""),
      onMouseDown: (e) => {
        e.stopPropagation();
        onSelect();
      }
    },
    /* @__PURE__ */ React.createElement("div", { className: "bld-card__head" }, /* @__PURE__ */ React.createElement("span", { className: "bld-card__index" }, index, "."), cardState.icon && /* @__PURE__ */ React.createElement("span", { className: "bld-card__icon tint--" + cardState.iconTint }, cardState.icon), /* @__PURE__ */ React.createElement("span", { className: "bld-card__title" }, cardState.title)),
    cardState.configured ? /* @__PURE__ */ React.createElement("div", { className: "bld-card__body" }, /* @__PURE__ */ React.createElement("span", { className: "bld-card__desc" }, cardState.desc)) : /* @__PURE__ */ React.createElement("div", { className: "bld-card__skeleton" }, /* @__PURE__ */ React.createElement("span", null), /* @__PURE__ */ React.createElement("span", null))
  );
}
function InsertBtn({ onClick, mid, mode }) {
  if (mode === "flow") {
    return /* @__PURE__ */ React.createElement("div", { className: "bld-insert bld-insert--conn" + (mid ? " bld-insert--mid" : "") }, /* @__PURE__ */ React.createElement("div", { className: "bld-insert__line" }), /* @__PURE__ */ React.createElement("span", { className: "bld-insert__dot" }), mid && /* @__PURE__ */ React.createElement("div", { className: "bld-insert__line" }));
  }
  return /* @__PURE__ */ React.createElement("div", { className: "bld-insert" + (mid ? " bld-insert--mid" : "") }, /* @__PURE__ */ React.createElement("div", { className: "bld-insert__line" }), /* @__PURE__ */ React.createElement("button", { className: "bld-insert__btn", onMouseDown: (e) => {
    e.stopPropagation();
    onClick();
  }, title: "Add step" }, "+"), mid && /* @__PURE__ */ React.createElement("div", { className: "bld-insert__line" }));
}
function StepPicker({ members, isKickoff, contract, onPick, onClose }) {
  const [q, setQ] = React.useState("");
  const pickerState = window.MobKitFlowController.basicStepPickerState({ members, contract, query: q, isKickoff });
  if (pickerState.mode === "kickoff") {
    return /* @__PURE__ */ React.createElement("div", { className: "bld-panel__inner" }, /* @__PURE__ */ React.createElement(PanelHead, { title: pickerState.title, sub: pickerState.sub, onClose }), /* @__PURE__ */ React.createElement("div", { className: "bld-hint" }, pickerState.kickoffHint));
  }
  return /* @__PURE__ */ React.createElement("div", { className: "bld-panel__inner" }, /* @__PURE__ */ React.createElement(PanelHead, { title: pickerState.title, sub: pickerState.sub, onClose }), /* @__PURE__ */ React.createElement("div", { className: "bld-search" }, /* @__PURE__ */ React.createElement("span", { className: "bld-search__icon" }, pickerState.searchIcon), /* @__PURE__ */ React.createElement("input", { className: "bld-search__input", placeholder: pickerState.searchPlaceholder, value: q, onChange: (e) => setQ(e.target.value), autoFocus: true })), /* @__PURE__ */ React.createElement("div", { className: "bld-opts__group" }, pickerState.membersLabel), /* @__PURE__ */ React.createElement("div", { className: "bld-opts" }, pickerState.memberRows.map((row) => /* @__PURE__ */ React.createElement("button", { key: row.id, className: "bld-opt", onClick: () => onPick(row.pick) }, /* @__PURE__ */ React.createElement("span", { className: "bld-opt__icon tint--" + row.iconTint }, row.icon), /* @__PURE__ */ React.createElement("span", { className: "bld-opt__text" }, /* @__PURE__ */ React.createElement("span", { className: "bld-opt__label" }, row.name), /* @__PURE__ */ React.createElement("span", { className: "bld-opt__sub" }, row.sub)))), !pickerState.hasConfiguredMembers && /* @__PURE__ */ React.createElement("div", { className: "bld-hint", style: { padding: "4px 8px" } }, pickerState.emptyMembersHint)), /* @__PURE__ */ React.createElement("div", { className: "bld-opts__group" }, pickerState.flowLabel), /* @__PURE__ */ React.createElement("div", { className: "bld-opts" }, pickerState.primitiveRows.map((row) => /* @__PURE__ */ React.createElement("button", { key: row.id, className: "bld-opt", onClick: () => onPick(row.pick) }, /* @__PURE__ */ React.createElement("span", { className: "bld-opt__icon tint--" + row.tint }, row.glyph), /* @__PURE__ */ React.createElement("span", { className: "bld-opt__text" }, /* @__PURE__ */ React.createElement("span", { className: "bld-opt__label" }, row.label, row.isNew && /* @__PURE__ */ React.createElement("span", { className: "bld-opt__new" }, pickerState.newBadgeLabel)), /* @__PURE__ */ React.createElement("span", { className: "bld-opt__sub" }, row.sub))))));
}
function StepInspector({ studio, members, flow, step, update, onDelete, contract, toolCatalog, onInputParamReferenceChange }) {
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
    return /* @__PURE__ */ React.createElement("div", { className: "bld-panel__inner" }, /* @__PURE__ */ React.createElement(PanelHead, { icon: inputState.panelIcon, iconTint: "member", title: inputState.panelTitle, sub: inputState.panelSub, onClose: onDelete, deleteMode: true }), /* @__PURE__ */ React.createElement(Field, { label: inputState.taskLabel }, /* @__PURE__ */ React.createElement("textarea", { className: "field__textarea", rows: 3, placeholder: inputState.taskPlaceholder, value: step.task || "", onChange: (e) => update(step.id, window.MobKitFlowController.flowStepTaskPatch(e.target.value)) })), /* @__PURE__ */ React.createElement("div", { className: "section" }, /* @__PURE__ */ React.createElement("div", { className: "row row--between", style: { marginBottom: 6 } }, /* @__PURE__ */ React.createElement("div", { className: "section__title" }, inputState.paramsTitle), /* @__PURE__ */ React.createElement("button", { className: "btn btn--ghost btn--sm", onClick: addParam }, inputState.addParamLabel)), /* @__PURE__ */ React.createElement("div", { className: "schema-builder" }, /* @__PURE__ */ React.createElement("div", { className: "schema-builder__header" }, inputState.headerRows.map((row) => /* @__PURE__ */ React.createElement("span", { key: row.key, className: row.className }, row.label))), params.map((param) => /* @__PURE__ */ React.createElement(
      InputParamField,
      {
        key: param.id,
        param,
        normalizeName: (raw) => window.MobKitFlowController.uniqueInputParamName(params, raw, param.id),
        onRename: (raw, previousName) => renameParam(param.id, raw, previousName),
        onChange: (patch) => updateParam(param.id, patch),
        onDelete: () => deleteParam(param.id),
        contract
      }
    )), params.length === 0 && /* @__PURE__ */ React.createElement("div", { className: "schema-builder__empty" }, inputState.emptyParamsParts.map((part) => part.kind === "code" ? /* @__PURE__ */ React.createElement("code", { key: part.key }, part.text) : /* @__PURE__ */ React.createElement(React.Fragment, { key: part.key }, part.text))))), /* @__PURE__ */ React.createElement(PanelTips, { items: inputState.tips }));
  }
  if (step.type === "branch" || step.type === "parallel") {
    const branchState = window.MobKitFlowController.basicBranchParallelControlState({
      step,
      flow,
      members: studio?.members || [],
      contract
    });
    const setBranchCondition = (branch, patch) => {
      update(step.id, window.MobKitFlowController.basicBranchConditionPatch(step, branch.id, patch, contract));
    };
    const addBranch = () => update(step.id, window.MobKitFlowController.basicBranchAddPatch(step, { flow }));
    return /* @__PURE__ */ React.createElement("div", { className: "bld-panel__inner" }, /* @__PURE__ */ React.createElement(PanelHead, { icon: branchState.panelIcon, iconTint: "member", title: branchState.panelTitle, sub: branchState.panelSub, onClose: onDelete, deleteMode: true }), /* @__PURE__ */ React.createElement(Field, { label: branchState.controllerLabel }, /* @__PURE__ */ React.createElement("select", { className: "field__select", value: branchState.controllerRole, onChange: (e) => update(step.id, window.MobKitFlowController.flowStepControllerRolePatch(e.target.value, members)) }, /* @__PURE__ */ React.createElement("option", { value: "" }, branchState.controllerPlaceholderLabel), branchState.memberOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value }, option.label)))), !branchState.controllerRole && /* @__PURE__ */ React.createElement("div", { className: "bld-hint", style: { marginTop: 8 } }, branchState.emptyControllerHint), !branchState.isParallel && /* @__PURE__ */ React.createElement(React.Fragment, null, /* @__PURE__ */ React.createElement("div", { className: "bld-section-label" }, branchState.branchConditionTitle), /* @__PURE__ */ React.createElement("div", { className: "bld-hint" }, branchState.branchConditionIntro), step.branches.map((b, i) => /* @__PURE__ */ React.createElement(
      BranchConditionEditor,
      {
        key: b.id,
        index: i,
        branch: b,
        options: branchState.conditionOptions,
        schemas: studio?.schemas || [],
        onChange: (patch) => setBranchCondition(b, patch),
        contract
      }
    )), /* @__PURE__ */ React.createElement("button", { className: "bld-add-row", onClick: addBranch }, branchState.addBranchLabel), /* @__PURE__ */ React.createElement("div", { className: "bld-branch-card bld-branch-card--fallback" }, /* @__PURE__ */ React.createElement("div", { className: "bld-branch-card__head" }, branchState.fallbackTitle), /* @__PURE__ */ React.createElement("div", { className: "bld-hint" }, branchState.fallbackHint))), branchState.isParallel && /* @__PURE__ */ React.createElement(React.Fragment, null, /* @__PURE__ */ React.createElement(Field, { label: branchState.dispatchLabel }, /* @__PURE__ */ React.createElement("select", { className: "field__select", value: branchState.dispatchValue, onChange: (e) => update(step.id, window.MobKitFlowController.flowStepParallelDispatchPatch(e.target.value, contract)) }, branchState.dispatchOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value, disabled: option.disabled }, option.label)))), branchState.selectedDispatch?.reason && /* @__PURE__ */ React.createElement("div", { className: "bld-hint", style: { color: "var(--warn)" } }, branchState.selectedDispatch.reason), /* @__PURE__ */ React.createElement(Field, { label: branchState.collectionLabel }, /* @__PURE__ */ React.createElement("select", { className: "field__select", value: branchState.collectionValue, onChange: (e) => update(step.id, window.MobKitFlowController.flowStepCollectionPatch(e.target.value, contract)) }, branchState.collectionOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value, disabled: option.disabled }, option.label)))), branchState.selectedCollection?.reason && /* @__PURE__ */ React.createElement("div", { className: "bld-hint", style: { color: "var(--warn)" } }, branchState.selectedCollection.reason), branchState.showQuorum && /* @__PURE__ */ React.createElement(Field, { label: branchState.quorumLabel }, /* @__PURE__ */ React.createElement("input", { className: "field__input", type: "number", min: "1", value: step.quorum ?? "", placeholder: branchState.quorumPlaceholder, onChange: (e) => update(step.id, window.MobKitFlowController.flowStepQuorumPatch(e.target.value)) })), /* @__PURE__ */ React.createElement("button", { className: "bld-add-row", onClick: addBranch }, branchState.addBranchLabel)), /* @__PURE__ */ React.createElement(Field, { label: branchState.dependencyLabel }, /* @__PURE__ */ React.createElement("select", { className: "field__select", value: branchState.dependencyValue, onChange: (e) => update(step.id, window.MobKitFlowController.flowStepDependencyModePatch(e.target.value, contract)) }, branchState.dependencyOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value, disabled: option.disabled }, option.label)))), branchState.selectedDependency?.reason && /* @__PURE__ */ React.createElement("div", { className: "bld-hint", style: { color: "var(--warn)" } }, branchState.selectedDependency.reason));
  }
  if (step.type === "repeat") {
    const repeatState = window.MobKitFlowController.basicRepeatControlState({
      step,
      members: studio?.members || [],
      schemas: studio?.schemas || [],
      contract
    });
    const setCond = (patch) => update(step.id, window.MobKitFlowController.flowStepRepeatConditionPatch(step, patch));
    return /* @__PURE__ */ React.createElement("div", { className: "bld-panel__inner" }, /* @__PURE__ */ React.createElement(PanelHead, { icon: repeatState.panelIcon, iconTint: "member", title: repeatState.panelTitle, sub: repeatState.panelSub, onClose: onDelete, deleteMode: true }), /* @__PURE__ */ React.createElement(Field, { label: repeatState.loopIdLabel }, /* @__PURE__ */ React.createElement("input", { className: "field__input field__input--mono", value: step.loopId || "", placeholder: repeatState.loopIdPlaceholder, onChange: (e) => update(step.id, window.MobKitFlowController.flowStepLoopIdPatch(e.target.value)) })), /* @__PURE__ */ React.createElement("div", { className: "bld-section-label", style: { marginTop: 16 } }, repeatState.conditionTitle), /* @__PURE__ */ React.createElement("div", { className: "bld-hint" }, repeatState.conditionIntro), !repeatState.hasBodyMembers ? /* @__PURE__ */ React.createElement("div", { className: "bld-hint", style: { marginTop: 10, color: "var(--warn)" } }, repeatState.emptyBodyHint) : /* @__PURE__ */ React.createElement("div", { className: "bld-cond" }, /* @__PURE__ */ React.createElement("select", { className: "field__select", value: repeatState.cond.stepId || "", onChange: (e) => setCond(window.MobKitFlowController.basicConditionSourcePatch(repeatState.bodyMembers, e.target.value)) }, /* @__PURE__ */ React.createElement("option", { value: "" }, repeatState.memberPlaceholderLabel), repeatState.bodyMemberOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value }, option.label))), /* @__PURE__ */ React.createElement("select", { className: "field__select", value: repeatState.cond.field || "", onChange: (e) => setCond(window.MobKitFlowController.basicConditionFieldPatch(e.target.value, repeatState.fieldOptions)), disabled: !repeatState.condSchema }, /* @__PURE__ */ React.createElement("option", { value: "" }, repeatState.fieldPlaceholder), repeatState.fieldOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.field.id || option.value, value: option.value }, option.label))), /* @__PURE__ */ React.createElement("select", { className: "field__select bld-cond__op", value: repeatState.operatorValue, onChange: (e) => setCond(window.MobKitFlowController.basicConditionOperatorPatch(e.target.value, contract)) }, repeatState.operatorOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value, disabled: option.disabled }, option.label))), /* @__PURE__ */ React.createElement(CondValue, { field: repeatState.condField, value: repeatState.cond.val, onChange: (v) => setCond(window.MobKitFlowController.basicConditionValuePatch(v)) })), /* @__PURE__ */ React.createElement("div", { className: "bld-cond__preview" }, repeatState.previewLabel, " ", /* @__PURE__ */ React.createElement("code", null, repeatState.repeatUntilExpression || repeatState.previewFallback)), /* @__PURE__ */ React.createElement(Field, { label: repeatState.iterationInputLabel }, /* @__PURE__ */ React.createElement("select", { className: "field__select", value: repeatState.iterationInputValue, onChange: (e) => update(step.id, window.MobKitFlowController.flowStepIterationInputPatch(e.target.value, contract)) }, repeatState.iterationInputOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value, disabled: option.disabled }, option.label)))), repeatState.selectedIterationInput?.reason && /* @__PURE__ */ React.createElement("div", { className: "bld-hint", style: { color: "var(--warn)" } }, repeatState.selectedIterationInput.reason), /* @__PURE__ */ React.createElement(Field, { label: repeatState.maxIterationsLabel }, /* @__PURE__ */ React.createElement("input", { className: "field__input", type: "number", min: "1", placeholder: repeatState.maxIterationsPlaceholder, value: step.maxIterations ?? "", onChange: (e) => update(step.id, window.MobKitFlowController.flowStepMaxIterationsPatch(e.target.value)) })), /* @__PURE__ */ React.createElement(PanelTips, { items: repeatState.tips }));
  }
  const memberStepState = window.MobKitFlowController.basicMemberStepControlState({
    step,
    flow,
    members,
    contract
  });
  const m = memberStepState.member;
  const launchState = memberStepState.launchState;
  return /* @__PURE__ */ React.createElement("div", { className: "bld-panel__inner" }, /* @__PURE__ */ React.createElement(PanelHead, { icon: "\u25C6", iconTint: "accent", title: memberStepState.panelTitle, sub: memberStepState.panelSub, onClose: onDelete, deleteMode: true }), /* @__PURE__ */ React.createElement(Field, { label: memberStepState.memberFieldLabel }, /* @__PURE__ */ React.createElement("select", { className: "field__select", value: step.role || "", onChange: (e) => update(step.id, window.MobKitFlowController.flowStepMemberRolePatch(e.target.value, members)) }, /* @__PURE__ */ React.createElement("option", { value: "" }, memberStepState.memberPlaceholderLabel), memberStepState.memberOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value }, option.label)))), /* @__PURE__ */ React.createElement(Field, { label: launchState.launchTitle }, /* @__PURE__ */ React.createElement("select", { className: "field__select", value: launchState.launchKind, onChange: (e) => {
    update(step.id, window.MobKitFlowController.launchModeKindPatch(step, e.target.value, contract, { firstForkSourceId: memberStepState.firstLaunchSourceId }));
  } }, launchState.launchOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value, disabled: option.disabled }, option.label)))), launchState.selectedLaunchMode?.reason && /* @__PURE__ */ React.createElement("div", { className: "bld-hint", style: { color: "var(--warn)" } }, launchState.selectedLaunchMode.reason), launchState.launchKind === "Resume" && /* @__PURE__ */ React.createElement(Field, { label: launchState.resumeSessionLabel }, /* @__PURE__ */ React.createElement("input", { className: "field__input", value: launchState.launchMode.sessionId || "", placeholder: launchState.resumeSessionPlaceholder, onChange: (e) => update(step.id, window.MobKitFlowController.launchModeSessionPatch(step, e.target.value, contract)) })), launchState.launchKind === "Fork" && /* @__PURE__ */ React.createElement(React.Fragment, null, /* @__PURE__ */ React.createElement(Field, { label: launchState.forkSourceLabel }, /* @__PURE__ */ React.createElement("select", { className: "field__select", value: launchState.launchMode.from || "", onChange: (e) => update(step.id, window.MobKitFlowController.launchModeForkSourcePatch(step, e.target.value, contract, { sourceOptions: memberStepState.launchSourceOptions })) }, memberStepState.launchSourceOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value }, option.label)))), /* @__PURE__ */ React.createElement(Field, { label: launchState.forkContextLabel }, /* @__PURE__ */ React.createElement("select", { className: "field__select", value: launchState.forkContextValue, onChange: (e) => update(step.id, window.MobKitFlowController.launchModeForkContextPatch(step, e.target.value, contract)) }, launchState.forkContextOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value, disabled: option.disabled }, option.label)))), launchState.selectedForkContext?.reason && /* @__PURE__ */ React.createElement("div", { className: "bld-hint", style: { color: "var(--warn)" } }, launchState.selectedForkContext.reason)), /* @__PURE__ */ React.createElement(Field, { label: launchState.budgetPolicyLabel }, /* @__PURE__ */ React.createElement("select", { className: "field__select", value: launchState.budgetSplitPolicy.kind, onChange: (e) => update(step.id, window.MobKitFlowController.launchBudgetKindPatch(step, e.target.value, contract)) }, launchState.budgetOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value, disabled: option.disabled }, option.label)))), launchState.selectedBudgetPolicy?.reason && /* @__PURE__ */ React.createElement("div", { className: "bld-hint", style: { color: "var(--warn)" } }, launchState.selectedBudgetPolicy.reason), launchState.budgetSplitPolicy.kind === "Fixed" && /* @__PURE__ */ React.createElement(Field, { label: launchState.fixedBudgetLabel }, /* @__PURE__ */ React.createElement("input", { className: "field__input", type: "number", min: "1", step: "1", value: launchState.fixedBudgetValue, onChange: (e) => update(step.id, window.MobKitFlowController.launchBudgetFixedLimitPatch(step, e.target.value, contract)) })), /* @__PURE__ */ React.createElement(Field, { label: memberStepState.instructionLabel }, /* @__PURE__ */ React.createElement("textarea", { className: "field__textarea", rows: 4, placeholder: memberStepState.instructionPlaceholder, value: step.instruction || "", onChange: (e) => update(step.id, window.MobKitFlowController.flowStepInstructionPatch(e.target.value)) })), /* @__PURE__ */ React.createElement(Field, { label: memberStepState.dispatchLabel }, /* @__PURE__ */ React.createElement("select", { className: "field__select", value: memberStepState.dispatchValue, onChange: (e) => update(step.id, window.MobKitFlowController.flowStepDispatchModePatch(e.target.value, contract)) }, memberStepState.dispatchOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value, disabled: option.disabled }, option.label)))), memberStepState.selectedDispatch?.reason && /* @__PURE__ */ React.createElement("div", { className: "bld-hint", style: { color: "var(--warn)" } }, memberStepState.selectedDispatch.reason), /* @__PURE__ */ React.createElement(Field, { label: memberStepState.collectionLabel }, /* @__PURE__ */ React.createElement("select", { className: "field__select", value: memberStepState.collectionValue, onChange: (e) => update(step.id, window.MobKitFlowController.flowStepCollectionPatch(e.target.value, contract)) }, memberStepState.collectionOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value, disabled: option.disabled }, option.label)))), memberStepState.selectedCollection?.reason && /* @__PURE__ */ React.createElement("div", { className: "bld-hint", style: { color: "var(--warn)" } }, memberStepState.selectedCollection.reason), memberStepState.showQuorum && /* @__PURE__ */ React.createElement(Field, { label: memberStepState.quorumLabel }, /* @__PURE__ */ React.createElement("input", { className: "field__input", type: "number", min: "1", step: "1", value: step.quorum ?? "", placeholder: memberStepState.quorumPlaceholder, onChange: (e) => update(step.id, window.MobKitFlowController.flowStepQuorumPatch(e.target.value)) })), /* @__PURE__ */ React.createElement(Field, { label: memberStepState.timeoutLabel }, /* @__PURE__ */ React.createElement("input", { className: "field__input", type: "number", min: "1", step: "1", placeholder: memberStepState.timeoutPlaceholder, value: step.timeoutMs ?? "", onChange: (e) => update(step.id, window.MobKitFlowController.flowStepTimeoutPatch(e.target.value)) })), /* @__PURE__ */ React.createElement(Field, { label: memberStepState.outputFormatLabel }, /* @__PURE__ */ React.createElement("select", { className: "field__select", value: memberStepState.outputValue, onChange: (e) => update(step.id, window.MobKitFlowController.flowStepOutputFormatPatch(e.target.value, contract)) }, memberStepState.outputOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value, disabled: option.disabled }, option.label)))), memberStepState.selectedOutput?.reason && /* @__PURE__ */ React.createElement("div", { className: "bld-hint", style: { color: "var(--warn)" } }, memberStepState.selectedOutput.reason), /* @__PURE__ */ React.createElement(
    ToolScopeEditor,
    {
      label: memberStepState.allowedToolsLabel,
      emptyLabel: memberStepState.allowedToolsEmptyLabel,
      member: m,
      selected: step.allowedTools || [],
      onChange: (tools) => update(step.id, window.MobKitFlowController.flowStepAllowedToolsPatch(tools, { member: m, toolCatalog })),
      mode: "member",
      toolCatalog
    }
  ), /* @__PURE__ */ React.createElement(
    ToolScopeEditor,
    {
      label: memberStepState.blockedToolsLabel,
      emptyLabel: memberStepState.blockedToolsEmptyLabel,
      member: m,
      selected: step.blockedTools || [],
      onChange: (tools) => update(step.id, window.MobKitFlowController.flowStepBlockedToolsPatch(tools, { toolCatalog })),
      mode: "catalog",
      toolCatalog
    }
  ), memberStepState.schemaHint && /* @__PURE__ */ React.createElement("div", { className: "bld-hint", style: { marginTop: 10 } }, memberStepState.schemaHint.parts.map((part) => part.kind === "code" ? /* @__PURE__ */ React.createElement("code", { key: part.key }, part.text) : /* @__PURE__ */ React.createElement(React.Fragment, { key: part.key }, part.text))), /* @__PURE__ */ React.createElement(Field, { label: memberStepState.dependencyLabel }, /* @__PURE__ */ React.createElement("select", { className: "field__select", value: memberStepState.dependencyValue, onChange: (e) => update(step.id, window.MobKitFlowController.flowStepDependencyModePatch(e.target.value, contract)) }, memberStepState.dependencyOptions.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value, disabled: option.disabled }, option.label)))), memberStepState.selectedDependency?.reason && /* @__PURE__ */ React.createElement("div", { className: "bld-hint", style: { color: "var(--warn)" } }, memberStepState.selectedDependency.reason));
}
function ToolScopeEditor({ label, emptyLabel, member, selected, onChange, mode = "member", toolCatalog = [] }) {
  const field = mode === "catalog" ? "blockedTools" : "allowedTools";
  const scope = window.MobKitFlowController.stepToolScopeState({ member, selected, mode, toolCatalog });
  const remove = (id) => {
    const result = window.MobKitFlowController.stepToolScopeRemovePatch(selected, id, { field });
    if (result.patch) onChange(result.patch[field] || []);
  };
  const add = (id) => {
    const result = window.MobKitFlowController.stepToolScopeAddPatch(selected, id, { member, mode, toolCatalog, field });
    if (result.patch) onChange(result.patch[field] || []);
  };
  return /* @__PURE__ */ React.createElement(Field, { label }, scope.selectedTools.length === 0 ? /* @__PURE__ */ React.createElement("div", { className: "bld-hint" }, emptyLabel) : scope.rows.map((row) => {
    return /* @__PURE__ */ React.createElement("div", { key: row.id, className: row.className }, /* @__PURE__ */ React.createElement("div", null, /* @__PURE__ */ React.createElement("div", { className: "name" }, row.name), /* @__PURE__ */ React.createElement("div", { className: "auth" }, row.description)), /* @__PURE__ */ React.createElement("button", { onClick: () => remove(row.id) }, row.removeLabel));
  }), /* @__PURE__ */ React.createElement("select", { className: "field__select", value: scope.addSelectValue, disabled: scope.disabled, onChange: (e) => {
    add(e.target.value);
    e.target.value = "";
  } }, /* @__PURE__ */ React.createElement("option", { value: scope.addSelectValue }, scope.addSelectPlaceholder), scope.addableRows.map((row) => /* @__PURE__ */ React.createElement("option", { key: row.id, value: row.value }, row.optionLabel))));
}
function PanelHead({ icon, iconTint, title, sub, onClose, deleteMode }) {
  return /* @__PURE__ */ React.createElement("div", { className: "bld-panel__head" }, /* @__PURE__ */ React.createElement("div", { className: "bld-panel__head-main" }, icon && /* @__PURE__ */ React.createElement("span", { className: "bld-panel__icon tint--" + (iconTint || "muted") }, icon), /* @__PURE__ */ React.createElement("div", null, /* @__PURE__ */ React.createElement("div", { className: "bld-panel__title" }, title), sub && /* @__PURE__ */ React.createElement("div", { className: "bld-panel__sub" }, sub))), /* @__PURE__ */ React.createElement("button", { className: "bld-panel__close", onClick: onClose, title: deleteMode ? "Delete step" : "Close" }, deleteMode ? "\u{1F5D1}" : "\u2715"));
}
function Field({ label, children }) {
  return /* @__PURE__ */ React.createElement("div", { className: "field", style: { marginTop: 14 } }, /* @__PURE__ */ React.createElement("label", { className: "field__label" }, label), children);
}
function PanelTips({ items }) {
  return /* @__PURE__ */ React.createElement("div", { className: "bld-tips" }, /* @__PURE__ */ React.createElement("div", { className: "bld-tips__head" }, "Tips"), /* @__PURE__ */ React.createElement("ul", null, items.map((t, i) => /* @__PURE__ */ React.createElement("li", { key: i }, t))));
}
function EmptyPanel() {
  return /* @__PURE__ */ React.createElement("div", { className: "bld-panel__inner bld-panel__empty" }, /* @__PURE__ */ React.createElement("div", { className: "bld-panel__title" }, "Build your mob flow"), /* @__PURE__ */ React.createElement("div", { className: "bld-panel__sub" }, "Pick a node to configure, or press ", /* @__PURE__ */ React.createElement("strong", null, "+"), " to add a member turn or flow primitive. The result is a ", /* @__PURE__ */ React.createElement("code", null, "mob.toml"), " flow."));
}
function kickoffSlotEmpty(flow) {
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
    for (const l of childLanes(s)) {
      const r = findStep(l.steps, id);
      if (r) return r;
    }
  }
  return null;
}
window.BuilderView = BuilderView;
window.blankAuthoringFlow = blankAuthoringFlow;

}

/* app.jsx */

const { useStudioState, GraphEditor, Inspector, AddNodeMenu, DrySim, ValidateSheet, SourceDrawer, InlineSourceEditor, useTweaks, TweaksPanel, TweakSection, TweakRadio, TweakSelect, TweakText, TweakNumber, AgentsView, BuilderView } = window;
const TWEAK_DEFAULTS = (
  /*EDITMODE-BEGIN*/
  {
    "edgeStyle": "text",
    "density": "comfortable",
    "theme": "light",
    "inspectorLayout": "right"
  }
);
const STARTER_FLOWS = [];
const CATALOG_BOOT = {
  grid: MOBKIT_BOOT.GRID,
  cellXY: MOBKIT_BOOT.cellXY,
  template: MOBKIT_BOOT.template
};
function App() {
  const [stage, setStage] = React.useState("draft");
  const [flow, setFlow] = React.useState(() => window.blankAuthoringFlow());
  const [stepSel, setStepSel] = React.useState(null);
  const [editorMode, setEditorMode] = React.useState("basic");
  const [view, setView] = React.useState("editor");
  const [flows, setFlows] = React.useState(STARTER_FLOWS);
  const [currentFlowId, setCurrentFlowId] = React.useState("");
  const [templates, setTemplates] = React.useState([]);
  const [creating, setCreating] = React.useState(null);
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
  const sourceProjectionIsCurrent = React.useCallback((requestToken) => requestToken === sourceProjectionVersion.current, []);
  const currentAuthoringRevision = React.useCallback(() => authoringRevision.current, []);
  const authoringRevisionIsCurrent = React.useCallback((requestToken) => requestToken === authoringRevision.current, []);
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
    skillRealms: []
  }, markDraft, {
    flow,
    setFlow: setAuthoringFlow
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
      prompt: deploySettings.prompt || "<prompt>"
    }).then((preview) => {
      if (!cancelled) {
        setDeployCommandPreview(preview?.command || "");
      }
    }).catch(() => {
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
    window.MobKitFlowController.loadSchema().then((schema) => {
      if (cancelled) return;
      const nextCatalogs = window.MobKitFlowController.mobKitCatalogsFromSchema(schema, CATALOG_BOOT);
      setCatalogs(nextCatalogs);
      setDeploySettings(nextCatalogs.deployDefaults);
      setMobSettings(nextCatalogs.mobDefaults);
      contractSkillRealms.current = nextCatalogs.skillRealms;
      studio.setSkillRealms(nextCatalogs.skillRealms);
      const sampleFlows = window.MobKitFlowController.sampleFlowsFromSchema(schema);
      if (sampleFlows.length) {
        setTemplates(sampleFlows);
        setFlows(sampleFlows);
        hydrateMobpackDocument({
          document: sampleFlows[0].document,
          validation: sampleFlows[0].validation
        }, {
          id: sampleFlows[0].id,
          flowRow: sampleFlows[0],
          addToRegistry: false,
          openEditor: view === "editor",
          deployDefaults: nextCatalogs.deployDefaults,
          mobDefaults: nextCatalogs.mobDefaults
        });
      }
      setContract(schema);
    }).catch((error) => {
      if (!cancelled) setContract({ error: error?.message || String(error) });
    });
    return () => {
      cancelled = true;
    };
  }, []);
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
    const { instances, edges, frames } = window.MobKitFlowController.graphProjectionForFlow(flow, studio.members);
    graphProjectionSig.current = window.MobKitFlowController.graphStructureSignature(instances, edges);
    studio.setInstances(instances);
    studio.setEdges(edges);
    studio.setFrames(frames || []);
  }, [flow, editorMode]);
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
      previousFlow: current
    }));
  }, [editorMode, studio.instances, studio.edges, studio.members, markDraft]);
  React.useEffect(() => {
    const previousMembers = previousMembersRef.current || [];
    setFlow((current) => {
      const pruned = window.MobKitFlowController?.reconcileFlowMemberSteps ? window.MobKitFlowController.reconcileFlowMemberSteps(current, studio.members) : current;
      const schemaSynced = window.MobKitFlowController?.reconcileFlowMemberSchemas ? window.MobKitFlowController.reconcileFlowMemberSchemas(pruned, studio.members) : pruned;
      const controlSynced = window.MobKitFlowController?.reconcileFlowControlRoles ? window.MobKitFlowController.reconcileFlowControlRoles(schemaSynced, studio.members) : schemaSynced;
      const launchSynced = window.MobKitFlowController?.reconcileFlowLaunchSources ? window.MobKitFlowController.reconcileFlowLaunchSources(controlSynced, studio.members) : controlSynced;
      return window.MobKitFlowController?.reconcileFlowStepToolScopes ? window.MobKitFlowController.reconcileFlowStepToolScopes(launchSynced, studio.members) : launchSynced;
    });
    if (window.MobKitFlowController?.reconcileGraphMemberInstances || window.MobKitFlowController?.reconcileGraphControlRoles || window.MobKitFlowController?.reconcileGraphLaunchSources || window.MobKitFlowController?.reconcileGraphStepToolScopes) {
      const memberSynced = window.MobKitFlowController?.reconcileGraphMemberInstances ? window.MobKitFlowController.reconcileGraphMemberInstances({ instances: studio.instances, edges: studio.edges }, studio.members) : { instances: studio.instances, edges: studio.edges };
      const controlSynced = window.MobKitFlowController?.reconcileGraphControlRoles ? window.MobKitFlowController.reconcileGraphControlRoles(memberSynced.instances, studio.members) : memberSynced.instances;
      const launchSynced = window.MobKitFlowController?.reconcileGraphLaunchSources ? window.MobKitFlowController.reconcileGraphLaunchSources(controlSynced, studio.members) : controlSynced;
      const toolSynced = window.MobKitFlowController?.reconcileGraphStepToolScopes ? window.MobKitFlowController.reconcileGraphStepToolScopes(launchSynced, studio.members) : launchSynced;
      if (memberSynced.edges !== studio.edges) studio.setEdges(memberSynced.edges);
      if (toolSynced !== studio.instances) studio.setInstances(toolSynced);
    }
    if (window.MobKitFlowController?.reconcileMobSettingsProfiles) {
      setMobSettings((current) => window.MobKitFlowController.reconcileMobSettingsProfiles(
        current,
        previousMembers,
        studio.members
      ));
    }
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
      schemas: studio.schemas
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
      { strictEmpty: !!catalogs.contractMeta.loaded }
    ));
  }, [studio.members, studio.skillRealms, catalogs.contractMeta.loaded]);
  React.useEffect(() => {
    if (!window.MobKitFlowController?.reconcileDeploySettingsWithContract) return;
    setDeploySettings((current) => window.MobKitFlowController.reconcileDeploySettingsWithContract(
      current,
      contract,
      catalogs.models,
      { strictEmptyModels: !!catalogs.contractMeta.loaded }
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
        strictEmptyTools: !!catalogs.contractMeta.loaded
      }
    ));
  }, [studio.members, contract, deploySettings, catalogs.models, catalogs.toolCatalog, catalogs.contractMeta.loaded]);
  React.useEffect(() => {
    if (!window.MobKitFlowController?.reconcileMobSettingsWithContract) return;
    setMobSettings((current) => window.MobKitFlowController.reconcileMobSettingsWithContract(
      current,
      contract
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
        if (selection.kind === "instance") {
          studio.deleteInstance(selection.id);
          clearSelection();
        } else if (selection.kind === "edge") {
          studio.deleteEdge(selection.id);
          clearSelection();
        }
      }
      if ((e.metaKey || e.ctrlKey) && e.key === "z") {
        e.preventDefault();
        if (e.shiftKey) studio.redo();
        else studio.undo();
      }
      if (e.key === "Escape") {
        clearSelection();
        setAddAt(null);
        setDrySim(false);
        setValidate(false);
        setSourceOpen(false);
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
        contract
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
        contract
      });
      if (inserted) {
        studio.snap();
        if (inserted.flow && inserted.flow !== flow) setAuthoringFlow(inserted.flow);
        studio.setInstances((current) => window.MobKitFlowController.studioAppendInstancesPatch({
          instances: current,
          members: studio.members
        }, inserted.instances).instances);
        studio.setEdges((current) => window.MobKitFlowController.studioAppendEdgesPatch({
          edges: current,
          instances: [...studio.instances, ...inserted.instances]
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
      previousFlow: flow
    });
  };
  const buildDocument = () => window.MobKitFlowController.buildDocument({
    flow: effectiveAuthoringFlow(),
    studio: { ...studio, mobSettings },
    currentFlow,
    deploySettings
  });
  const rememberCurrentDocument = (document2, validation, nextStage = stage) => {
    const persistence = window.MobKitFlowController.flowRegistryDocumentPersistence({
      currentFlowId,
      document: document2,
      validation,
      stage: nextStage
    });
    if (!persistence.ok || !persistence.rowPatch) return;
    persistedDocumentSig.current = persistence.signature;
    setFlows((rows) => window.MobKitFlowController.flowRegistryRememberDocumentPatch(rows, persistence.rowPatch));
  };
  React.useEffect(() => {
    if (!currentFlowId || !currentFlow) return;
    let document2;
    try {
      document2 = buildDocument();
    } catch {
      return;
    }
    const persistence = window.MobKitFlowController.flowRegistryDocumentPersistence({
      currentFlowId,
      document: document2,
      validation: null,
      stage,
      previousSignature: persistedDocumentSig.current,
      skipIfUnchanged: true
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
    mobSettings
  ]);
  const handleDrySim = async () => {
    const requestToken = currentAuthoringRevision();
    setApiBusy(true);
    try {
      const document2 = buildDocument();
      const plan = await window.MobKitFlowController.deployDocument(document2, { execute: false });
      if (!authoringRevisionIsCurrent(requestToken)) return;
      const outcome = window.MobKitFlowController.deployOutcome(document2, plan, { execute: false });
      window.__mobkitFlowLastDocument = document2;
      window.__mobkitFlowLastDeployPlanTrace = plan;
      setDrySimDocument(document2);
      setDrySimPlan(plan);
      rememberCurrentDocument(outcome.document, outcome.validation, outcome.stage);
      setValidationResults(outcome.validationRows);
      setStage(outcome.stage);
      setDrySim(true);
      setDrySimKey((k) => k + 1);
    } catch (error) {
      if (!authoringRevisionIsCurrent(requestToken)) return;
      const outcome = window.MobKitFlowController.deployErrorOutcome(error, { execute: false });
      setValidationResults(outcome.validationRows);
      setValidate(true);
      setStage(outcome.stage);
    } finally {
      setApiBusy(false);
    }
  };
  const exportCurrentSourceDocument = async (requestToken) => {
    const document2 = buildDocument();
    const result = await window.MobKitFlowController.exportDocument(document2);
    const projection = window.MobKitFlowController.sourceDocumentFromExport(document2, result);
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
      const outcome = window.MobKitFlowController.sourceErrorOutcome(error);
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
      const outcome = window.MobKitFlowController.sourceErrorOutcome(error);
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
      const document2 = buildDocument();
      const result = await window.MobKitFlowController.validateDocument(document2);
      if (!authoringRevisionIsCurrent(requestToken)) return;
      const outcome = window.MobKitFlowController.validationOutcome(document2, result);
      window.__mobkitFlowLastDocument = document2;
      window.__mobkitFlowLastValidation = result;
      rememberCurrentDocument(outcome.document, outcome.validation, outcome.stage);
      setValidationResults(outcome.validationRows);
      setStage(outcome.stage);
    } catch (error) {
      if (!authoringRevisionIsCurrent(requestToken)) return;
      const outcome = window.MobKitFlowController.validationErrorOutcome(error);
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
      const document2 = buildDocument();
      const result = await window.MobKitFlowController.exportDocument(document2);
      if (!authoringRevisionIsCurrent(requestToken)) return;
      const outcome = window.MobKitFlowController.exportOutcome(document2, result);
      window.__mobkitFlowLastDocument = document2;
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
      const outcome = window.MobKitFlowController.exportErrorOutcome(error);
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
      const document2 = buildDocument();
      const result = await window.MobKitFlowController.deployDocument(document2, { execute });
      if (!authoringRevisionIsCurrent(requestToken)) return;
      const outcome = window.MobKitFlowController.deployOutcome(document2, result, { execute });
      window.__mobkitFlowLastDocument = document2;
      window.__mobkitFlowLastDeploy = result;
      rememberCurrentDocument(outcome.document, outcome.validation, outcome.stage);
      setValidationResults(outcome.validationRows);
      setStage(outcome.stage);
      setValidate(true);
    } catch (error) {
      if (!authoringRevisionIsCurrent(requestToken)) return;
      const outcome = window.MobKitFlowController.deployErrorOutcome(error, { execute });
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
      addToRegistry: options.addToRegistry,
      openEditor: options.openEditor,
      flowRow: options.flowRow || null,
      deployDefaults: options.deployDefaults || catalogs.deployDefaults,
      mobDefaults: options.mobDefaults || catalogs.mobDefaults,
      contractSkillRealms: contractSkillRealms.current
    });
    const hydrationPersistence = window.MobKitFlowController.flowRegistryDocumentPersistence({
      currentFlowId: hydration.id,
      document: hydration.document,
      validation: hydration.validation,
      stage: hydration.stage
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
      setFlows((fs) => window.MobKitFlowController.flowRegistryUpsertRowPatch(fs, hydration.registryRow));
    }
    setCurrentFlowId(hydration.id);
    setStage(hydration.stage);
    setValidationResults(hydration.validationRows);
    if (hydration.openEditor) setView("editor");
  };
  const hydrateImportedDocument = (result) => {
    hydrateMobpackDocument(result, { id: "f_imported" });
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
      const outcome = window.MobKitFlowController.importErrorOutcome(error, { filename: file.name });
      setValidationResults(outcome.validationRows);
      setValidate(true);
      setStage(outcome.stage);
    } finally {
      setApiBusy(false);
    }
  };
  const shellState = window.MobKitFlowController.topRailState({ contract, deploySettings, stage, view, theme: t.theme });
  return /* @__PURE__ */ React.createElement("div", { className: "app density--" + t.density + " inspector--" + t.inspectorLayout + " view--" + view }, /* @__PURE__ */ React.createElement(
    TopRail,
    {
      studio,
      stage,
      view,
      setView,
      editorMode,
      setEditorMode,
      currentFlowName: currentFlow?.name || "\u2014",
      contract,
      theme: t.theme,
      railState: shellState,
      onToggleTheme: () => setTweak("theme", t.theme === "dark" ? "light" : "dark"),
      onValidate: handleValidate,
      onPublish: handlePublish,
      onDeployPlan: handleDeployPlan,
      onDeployRun: handleDeployRun,
      onImport: () => importInputRef.current?.click(),
      onDrySim: handleDrySim,
      onYaml: handleSource,
      deploySettings
    }
  ), /* @__PURE__ */ React.createElement(
    "input",
    {
      ref: importInputRef,
      type: "file",
      accept: ".mobpack,.json,.toml,application/json,application/gzip",
      style: { display: "none" },
      onChange: handleImportFile
    }
  ), view === "flows" && /* @__PURE__ */ React.createElement(
    FlowsView,
    {
      flows,
      currentFlowId,
      onOpen: (id) => {
        const selection2 = window.MobKitFlowController.flowRegistrySelectionState(flows, id);
        if (selection2.hydration) {
          hydrateMobpackDocument(selection2.hydration.result, selection2.hydration.options);
          return;
        }
        if (!selection2.fallback) return;
        setCurrentFlowId(selection2.fallback.currentFlowId);
        setStage(selection2.fallback.stage);
        setView(selection2.fallback.view);
      },
      canCreate: canCreateAuthoring,
      onNew: () => {
        if (!canCreateAuthoring) return;
        setCreating({ step: 1, name: "", trigger: "label \xB7 small-fix", template: "blank" });
      }
    }
  ), view === "editor" && /* @__PURE__ */ React.createElement(ModeToggle, { mode: editorMode, setMode: setEditorMode, railState: shellState }), view === "editor" && editorMode === "advanced" && /* @__PURE__ */ React.createElement("div", { className: "stage-area", onClick: (e) => {
    if (e.target === e.currentTarget) setAddAt(null);
  } }, /* @__PURE__ */ React.createElement(
    GraphEditor,
    {
      state: studio,
      selection,
      selectInstance,
      selectEdge,
      clearSelection,
      activeStepId,
      edgeStyle: t.edgeStyle,
      density: t.density,
      onRequestAdd: handleRequestAdd,
      onOpenSourceFile: () => handleInlineSource("graph"),
      memberFocus: null,
      grid: catalogs.grid,
      contract
    }
  ), /* @__PURE__ */ React.createElement(
    InlineSourceEditor,
    {
      open: inlineSourceOpen && inlineSourceSurface === "graph",
      onClose: clearSourceProjection,
      state: inlineSourceDocument,
      busy: inlineSourceBusy,
      surface: "graph"
    }
  ), /* @__PURE__ */ React.createElement(
    AddNodeMenu,
    {
      at: addAt,
      members: studio.members,
      contract,
      onPick: handlePick,
      onClose: () => setAddAt(null),
      onJumpToAgents: (id) => {
        setAddAt(null);
        setView("agents");
        setAgentSel({ kind: "agent", id });
      }
    }
  ), /* @__PURE__ */ React.createElement("aside", { className: "inspector" }, /* @__PURE__ */ React.createElement(
    Inspector,
    {
      studio,
      selection,
      flow,
      template: currentFlow,
      templateSeed: catalogs.template,
      contract,
      deploySettings,
      selectMember: (id) => {
        setView("agents");
        setAgentSel({ kind: "agent", id });
      },
      selectInstance,
      clearSelection
    }
  ))), view === "editor" && editorMode === "basic" && /* @__PURE__ */ React.createElement(
    BuilderView,
    {
      studio,
      mode: "build",
      flow,
      setFlow: setAuthoringFlow,
      sel: stepSel,
      setSel: setStepSel,
      onShowSource: () => handleInlineSource("basic"),
      sourceOpen: inlineSourceOpen && inlineSourceSurface === "basic",
      sourceDocument: inlineSourceDocument,
      sourceBusy: inlineSourceBusy,
      onCloseSource: clearSourceProjection,
      contract,
      toolCatalog: catalogs.toolCatalog
    }
  ), view === "agents" && /* @__PURE__ */ React.createElement(
    AgentsView,
    {
      studio,
      agentSel,
      setAgentSel,
      contract,
      deploySettings,
      flow,
      setFlow: setAuthoringFlow,
      mobSettings,
      setMobSettings: setAuthoringMobSettings,
      toolCatalog: catalogs.toolCatalog,
      modelCatalog: catalogs.models,
      agentDefinitions: catalogs.agentDefinitions
    }
  ), creating && /* @__PURE__ */ React.createElement(
    NewFlowModal,
    {
      state: creating,
      setState: setCreating,
      templateOptions: window.MobKitFlowController.newFlowTemplateOptions(templates, {
        canCreateBlank: canCreateAuthoring,
        blankTemplate: catalogs.blankMobpack
      }),
      onCreate: (spec) => {
        if (!canCreateAuthoring) return;
        const draft = window.MobKitFlowController.createFlowDraftFromSpec({
          spec,
          templates,
          existingRows: flows,
          blankTemplate: catalogs.blankMobpack,
          deploySettings,
          mobSettings
        });
        if (!draft?.document || !draft?.row) return;
        setFlows((fs) => window.MobKitFlowController.flowRegistryAppendRowPatch(fs, draft.row));
        hydrateMobpackDocument({ document: draft.document, validation: null }, {
          id: draft.id,
          flowRow: draft.row,
          addToRegistry: false
        });
        setCreating(null);
      }
    }
  ), /* @__PURE__ */ React.createElement(DrySim, { open: drySim, onClose: () => setDrySim(false), onActiveStep: setActiveStepId, runKey: drySimKey, document: drySimDocument, plan: drySimPlan }), /* @__PURE__ */ React.createElement(ValidateSheet, { open: validate, onClose: () => setValidate(false), onPublish: handlePublish, onDeployPlan: handleDeployPlan, onDeployRun: handleDeployRun, results: validationResults, stage }), /* @__PURE__ */ React.createElement(SourceDrawer, { open: sourceOpen, onClose: clearSourceProjection, state: sourceDocument }), /* @__PURE__ */ React.createElement(
    Tweaks,
    {
      t,
      setTweak,
      flows,
      currentFlowId,
      deploySettings,
      setDeploySettings: setAuthoringDeploySettings,
      mobSettings,
      setMobSettings: setAuthoringMobSettings,
      members: studio.members,
      modelCatalog: catalogs.models,
      contract,
      deployCommandPreview,
      onLoadFlow: (id) => {
        const selection2 = window.MobKitFlowController.flowRegistrySelectionState(flows, id);
        if (!selection2.hydration) return;
        hydrateMobpackDocument(selection2.hydration.result, selection2.hydration.options);
      }
    }
  ));
}
function rpcUrlFromShell() {
  const meta = document.querySelector('meta[name="mobkit-base-url"]');
  const base = (meta?.getAttribute("content") || "").trim().replace(/\/+$/, "");
  return `${base}/flow-editor/rpc`;
}
function downloadExportResult(result) {
  const bytes = Uint8Array.from(atob(result.content_base64), (char) => char.charCodeAt(0));
  const blob = new Blob([bytes], { type: result.media_type || "application/vnd.meerkat.mobpack" });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = result.filename || "mobkit-flow.mobpack";
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
    contentBase64: btoa(binary)
  });
}
function TopRail({ studio, stage, view, setView, editorMode, setEditorMode, currentFlowName, theme, railState, onToggleTheme, onValidate, onPublish, onDeployPlan, onDeployRun, onImport, onDrySim, onYaml, contract, deploySettings }) {
  return /* @__PURE__ */ React.createElement("header", { className: "toprail" }, /* @__PURE__ */ React.createElement("div", { className: "brand" }, /* @__PURE__ */ React.createElement("span", { className: "dot" }), /* @__PURE__ */ React.createElement("span", null, railState.brandLabel)), /* @__PURE__ */ React.createElement("nav", { className: "viewtabs" }, /* @__PURE__ */ React.createElement("button", { className: "viewtab" + (view === "flows" || view === "editor" ? " is-current" : ""), onClick: () => setView(view === "editor" ? "flows" : "editor") }, railState.flowsTabLabel), /* @__PURE__ */ React.createElement("button", { className: "viewtab" + (view === "agents" ? " is-current" : ""), onClick: () => setView("agents") }, railState.agentsTabLabel)), /* @__PURE__ */ React.createElement("div", { className: "mob-status", title: railState.mobStatusTitle }, /* @__PURE__ */ React.createElement("span", { className: "glyph" }), /* @__PURE__ */ React.createElement("span", { className: "name" }, railState.mobFileLabel), /* @__PURE__ */ React.createElement("span", { className: "env" }, "\xB7 ", railState.contractState)), /* @__PURE__ */ React.createElement("div", { className: "mob-status mob-status--env", title: railState.deployCommand }, /* @__PURE__ */ React.createElement("span", { className: "env" }, railState.deployPrefixLabel), /* @__PURE__ */ React.createElement("span", { className: "name" }, railState.deploySurface)), /* @__PURE__ */ React.createElement("nav", { className: "crumbs" }, railState.inEditor && /* @__PURE__ */ React.createElement(React.Fragment, null, /* @__PURE__ */ React.createElement("button", { className: "crumb crumb--link", onClick: () => setView("flows") }, railState.flowsCrumbLabel), /* @__PURE__ */ React.createElement("span", { className: "crumb crumb--sep" }, railState.crumbSeparator), /* @__PURE__ */ React.createElement("span", { className: "crumb is-current" }, currentFlowName))), /* @__PURE__ */ React.createElement("div", { className: "actions" }, railState.inEditor && /* @__PURE__ */ React.createElement(React.Fragment, null, /* @__PURE__ */ React.createElement("span", { className: "stage", "data-state": stage }, /* @__PURE__ */ React.createElement("span", { className: "glyph" }), stage), /* @__PURE__ */ React.createElement("button", { className: "btn btn--ghost btn--sm", onClick: onDrySim }, railState.planTraceLabel), /* @__PURE__ */ React.createElement("button", { className: "btn btn--ghost btn--sm", onClick: onImport }, railState.importLabel), /* @__PURE__ */ React.createElement("button", { className: "btn btn--ghost btn--sm", onClick: onValidate }, railState.validateLabel), /* @__PURE__ */ React.createElement("button", { className: "btn btn--primary btn--sm", disabled: railState.deployActionsDisabled, onClick: onPublish }, railState.publishLabel), /* @__PURE__ */ React.createElement("button", { className: "btn btn--ghost btn--sm", disabled: railState.deployActionsDisabled, onClick: onDeployPlan }, railState.deployPlanLabel), /* @__PURE__ */ React.createElement("button", { className: "btn btn--primary btn--sm", disabled: railState.deployActionsDisabled, onClick: onDeployRun }, railState.deployLabel)), /* @__PURE__ */ React.createElement(
    "button",
    {
      className: "btn btn--ghost btn--sm theme-toggle",
      onClick: onToggleTheme,
      title: railState.themeToggleTitle
    },
    railState.themeToggleLabel
  )));
}
function FlowsView({ flows, currentFlowId, onOpen, onNew, canCreate }) {
  const registryState = window.MobKitFlowController.flowRegistryViewState(flows, currentFlowId, { canCreate });
  return /* @__PURE__ */ React.createElement("div", { className: "flows-view" }, /* @__PURE__ */ React.createElement("div", { className: "flows-view__head" }, /* @__PURE__ */ React.createElement("div", null, /* @__PURE__ */ React.createElement("div", { className: "inspector__eyebrow" }, registryState.eyebrow), /* @__PURE__ */ React.createElement("div", { className: "flows-view__title" }, registryState.title)), /* @__PURE__ */ React.createElement(
    "button",
    {
      className: "btn btn--primary",
      disabled: registryState.createDisabled,
      title: registryState.createTitle,
      onClick: onNew
    },
    registryState.createLabel
  )), /* @__PURE__ */ React.createElement("div", { className: "flows-list" }, /* @__PURE__ */ React.createElement("div", { className: "flows-list__head" }, registryState.columns.map((column) => /* @__PURE__ */ React.createElement("span", { key: column.key }, column.label))), registryState.rows.map((f) => /* @__PURE__ */ React.createElement("button", { key: f.id, className: f.className, onClick: () => onOpen(f.id) }, /* @__PURE__ */ React.createElement("span", { className: "flows-list__name" }, f.name), /* @__PURE__ */ React.createElement("span", { className: "flows-list__sub" }, f.trigger), /* @__PURE__ */ React.createElement("span", { className: "flows-list__sub" }, f.version), /* @__PURE__ */ React.createElement("span", { className: "stage", "data-state": f.stage }, /* @__PURE__ */ React.createElement("span", { className: "glyph" }), f.stage)))));
}
function NewFlowModal({ state, setState, onCreate, templateOptions = [] }) {
  const set = (patch) => setState({ ...state, ...patch });
  const modalState = window.MobKitFlowController.newFlowModalState(state, templateOptions);
  return /* @__PURE__ */ React.createElement("div", { className: "modal-backdrop", onClick: () => setState(null) }, /* @__PURE__ */ React.createElement("div", { className: "modal modal--new", onClick: (e) => e.stopPropagation() }, /* @__PURE__ */ React.createElement("div", { className: "modal__head" }, /* @__PURE__ */ React.createElement("div", { className: "inspector__eyebrow" }, modalState.eyebrow), /* @__PURE__ */ React.createElement("button", { className: "btn btn--ghost btn--sm", onClick: () => setState(null) }, modalState.closeLabel)), modalState.step === 1 && /* @__PURE__ */ React.createElement("div", { className: "modal__body" }, /* @__PURE__ */ React.createElement("div", { className: "field" }, /* @__PURE__ */ React.createElement("label", { className: "field__label" }, modalState.nameLabel), /* @__PURE__ */ React.createElement("input", { className: "field__input", autoFocus: true, placeholder: modalState.namePlaceholder, value: modalState.name, onChange: (e) => set({ name: e.target.value }) })), /* @__PURE__ */ React.createElement("div", { className: "field" }, /* @__PURE__ */ React.createElement("label", { className: "field__label" }, modalState.triggerLabel), /* @__PURE__ */ React.createElement("input", { className: "field__input", placeholder: modalState.triggerPlaceholder, value: modalState.trigger, onChange: (e) => set({ trigger: e.target.value }) }))), modalState.step === 2 && /* @__PURE__ */ React.createElement("div", { className: "modal__body" }, /* @__PURE__ */ React.createElement("div", { className: "field__label" }, modalState.startFromLabel), /* @__PURE__ */ React.createElement("div", { className: "template-grid" }, modalState.options.map((opt) => /* @__PURE__ */ React.createElement("button", { key: opt.id, className: opt.className, disabled: opt.disabled, onClick: () => set({ template: opt.id }) }, /* @__PURE__ */ React.createElement("div", { className: "template-card__tier" }, opt.tier), /* @__PURE__ */ React.createElement("div", { className: "template-card__name" }, opt.label), /* @__PURE__ */ React.createElement("div", { className: "template-card__sub" }, opt.sub))))), /* @__PURE__ */ React.createElement("div", { className: "modal__foot" }, modalState.step > 1 ? /* @__PURE__ */ React.createElement("button", { className: "btn btn--ghost btn--sm", onClick: () => set({ step: modalState.step - 1 }) }, modalState.backLabel) : /* @__PURE__ */ React.createElement("span", null), modalState.step < 2 ? /* @__PURE__ */ React.createElement("button", { className: "btn btn--primary btn--sm", disabled: modalState.nextDisabled, onClick: () => set({ step: 2 }) }, modalState.nextLabel) : /* @__PURE__ */ React.createElement(
    "button",
    {
      className: "btn btn--primary btn--sm",
      disabled: modalState.createDisabled,
      onClick: () => onCreate({ name: modalState.name, trigger: modalState.trigger, template: modalState.template })
    },
    modalState.createLabel
  ))));
}
function ModeToggle({ mode, setMode, railState }) {
  return /* @__PURE__ */ React.createElement("div", { className: "modetoggle" }, /* @__PURE__ */ React.createElement("button", { className: "modetoggle__opt" + (mode === "basic" ? " is-active" : ""), onClick: () => setMode("basic"), title: railState.basicModeTitle }, /* @__PURE__ */ React.createElement("svg", { width: "13", height: "13", viewBox: "0 0 13 13", fill: "none", stroke: "currentColor", strokeWidth: "1.3" }, /* @__PURE__ */ React.createElement("rect", { x: "1.5", y: "2.2", width: "10", height: "2.2" }), /* @__PURE__ */ React.createElement("rect", { x: "1.5", y: "6.6", width: "10", height: "2.2" })), /* @__PURE__ */ React.createElement("span", null, railState.basicModeLabel)), /* @__PURE__ */ React.createElement("button", { className: "modetoggle__opt" + (mode === "advanced" ? " is-active" : ""), onClick: () => setMode("advanced"), title: railState.graphModeTitle }, /* @__PURE__ */ React.createElement("svg", { width: "13", height: "13", viewBox: "0 0 13 13", fill: "none", stroke: "currentColor", strokeWidth: "1.3" }, /* @__PURE__ */ React.createElement("rect", { x: "1", y: "4.5", width: "4", height: "4" }), /* @__PURE__ */ React.createElement("rect", { x: "8", y: "1", width: "4", height: "4" }), /* @__PURE__ */ React.createElement("rect", { x: "8", y: "8", width: "4", height: "4" }), /* @__PURE__ */ React.createElement("path", { d: "M5 6.5h1.6M6.6 6.5V3h1.4M6.6 6.5V10h1.4" })), /* @__PURE__ */ React.createElement("span", null, railState.graphModeLabel)));
}
function Tweaks({ t, setTweak, flows = [], currentFlowId, deploySettings, setDeploySettings, mobSettings, setMobSettings, members = [], modelCatalog = [], contract, deployCommandPreview, onLoadFlow }) {
  const setDeployField = (field, value) => setDeploySettings(
    (current) => window.MobKitFlowController.deploySettingsPatch(current, { [field]: value }, { contract, modelCatalog })
  );
  const setMobField = (field, value) => setMobSettings(
    (current) => window.MobKitFlowController.mobSettingsPatch(current, { [field]: value }, { contract })
  );
  const controlState = window.MobKitFlowController.tweaksControlState({
    flows,
    deploySettings,
    mobSettings,
    members,
    modelCatalog,
    contract
  });
  return /* @__PURE__ */ React.createElement(TweaksPanel, { title: controlState.panelTitle }, /* @__PURE__ */ React.createElement(TweakSection, { title: controlState.loadMobTitle }, /* @__PURE__ */ React.createElement(
    TweakSelect,
    {
      label: controlState.loadMobLabel,
      value: currentFlowId || "",
      options: controlState.loadableFlowOptions,
      onChange: (id) => {
        onLoadFlow && onLoadFlow(id);
      }
    }
  )), /* @__PURE__ */ React.createElement(TweakSection, { title: controlState.canvasTitle }, /* @__PURE__ */ React.createElement(
    TweakRadio,
    {
      label: controlState.edgeStyleLabel,
      value: t.edgeStyle,
      onChange: (v) => setTweak("edgeStyle", v),
      options: controlState.edgeStyleOptions
    }
  ), /* @__PURE__ */ React.createElement(
    TweakRadio,
    {
      label: controlState.densityLabel,
      value: t.density,
      onChange: (v) => setTweak("density", v),
      options: controlState.densityOptions
    }
  )), /* @__PURE__ */ React.createElement(TweakSection, { title: controlState.themeTitle }, /* @__PURE__ */ React.createElement(
    TweakRadio,
    {
      label: controlState.themeModeLabel,
      value: t.theme,
      onChange: (v) => setTweak("theme", v),
      options: controlState.themeModeOptions
    }
  )), /* @__PURE__ */ React.createElement(TweakSection, { title: controlState.mobTitle }, /* @__PURE__ */ React.createElement(
    TweakSelect,
    {
      label: controlState.orchestratorLabel,
      value: mobSettings.orchestrator || "",
      options: controlState.profileOptions,
      onChange: (v) => setMobField("orchestrator", v)
    }
  ), /* @__PURE__ */ React.createElement(
    TweakRadio,
    {
      label: controlState.autoWireLabel,
      value: mobSettings.autoWireOrchestrator ? "yes" : "no",
      onChange: (v) => setMobField("autoWireOrchestrator", v === "yes"),
      options: controlState.autoWireOptions
    }
  ), /* @__PURE__ */ React.createElement(
    RoleWiringEditor,
    {
      value: mobSettings.roleWiring || [],
      profileOptions: controlState.profileChoices,
      onChange: (roleWiring) => setMobField("roleWiring", roleWiring)
    }
  ), /* @__PURE__ */ React.createElement(
    TweakSelect,
    {
      label: controlState.defaultBackendLabel,
      value: mobSettings.backendDefault || "",
      onChange: (v) => setMobField("backendDefault", v),
      options: controlState.mobBackendOptions
    }
  ), (mobSettings.backendDefault === "external" || mobSettings.externalAddressBase) && /* @__PURE__ */ React.createElement(TweakText, { label: controlState.externalBaseLabel, value: mobSettings.externalAddressBase || "", placeholder: controlState.externalBasePlaceholder, onChange: (v) => setMobField("externalAddressBase", v) }), /* @__PURE__ */ React.createElement(
    AdvancedMobSettingsEditor,
    {
      value: mobSettings.advanced || {},
      onChange: (advanced) => setMobField("advanced", advanced)
    }
  )), /* @__PURE__ */ React.createElement(TweakSection, { title: controlState.deployTitle }, /* @__PURE__ */ React.createElement(
    TweakSelect,
    {
      label: controlState.surfaceLabel,
      value: deploySettings.surface,
      onChange: (v) => setDeployField("surface", v),
      options: controlState.surfaceOptions
    }
  ), /* @__PURE__ */ React.createElement(
    TweakSelect,
    {
      label: controlState.trustLabel,
      value: deploySettings.trustPolicy,
      onChange: (v) => setDeployField("trustPolicy", v),
      options: controlState.trustOptions
    }
  ), /* @__PURE__ */ React.createElement(
    TweakSelect,
    {
      label: controlState.modelLabel,
      value: deploySettings.model || "",
      options: controlState.modelOptions,
      onChange: (v) => setDeployField("model", v)
    }
  ), /* @__PURE__ */ React.createElement(TweakText, { label: controlState.durationLabel, value: deploySettings.maxDuration || "", placeholder: controlState.durationPlaceholder, onChange: (v) => setDeployField("maxDuration", v) }), /* @__PURE__ */ React.createElement(TweakNumber, { label: controlState.toolCallsLabel, value: deploySettings.maxToolCalls ?? "", min: controlState.toolCallsMin, max: controlState.toolCallsMax, onChange: (v) => setDeployField("maxToolCalls", v) }), /* @__PURE__ */ React.createElement(TweakNumber, { label: controlState.tokensLabel, value: deploySettings.maxTotalTokens ?? "", min: controlState.tokensMin, max: controlState.tokensMax, onChange: (v) => setDeployField("maxTotalTokens", v) }), /* @__PURE__ */ React.createElement(
    TweakRadio,
    {
      label: controlState.realmLabel,
      value: deploySettings.isolated ? "isolated" : "shared",
      onChange: (v) => setDeployField("isolated", v === "isolated"),
      options: controlState.realmOptions
    }
  ), !deploySettings.isolated && /* @__PURE__ */ React.createElement(TweakText, { label: controlState.realmIdLabel, value: deploySettings.realm || "", placeholder: controlState.realmIdPlaceholder, onChange: (v) => setDeployField("realm", v) }), /* @__PURE__ */ React.createElement(
    TweakSelect,
    {
      label: controlState.backendLabel,
      value: deploySettings.realmBackend || "",
      onChange: (v) => setDeployField("realmBackend", v),
      options: controlState.realmBackendOptions
    }
  ), /* @__PURE__ */ React.createElement(TweakText, { label: controlState.promptLabel, value: deploySettings.prompt || "", placeholder: controlState.promptPlaceholder, onChange: (v) => setDeployField("prompt", v) }), /* @__PURE__ */ React.createElement("div", { className: "twk-row" }, /* @__PURE__ */ React.createElement("div", { className: "twk-lbl" }, /* @__PURE__ */ React.createElement("span", null, controlState.commandLabel)), /* @__PURE__ */ React.createElement("code", { className: "deploy-command" }, deployCommandPreview || controlState.commandFallback))), /* @__PURE__ */ React.createElement(TweakSection, { title: controlState.inspectorTitle }, /* @__PURE__ */ React.createElement(
    TweakRadio,
    {
      label: controlState.inspectorLayoutLabel,
      value: t.inspectorLayout,
      onChange: (v) => setTweak("inspectorLayout", v),
      options: controlState.inspectorLayoutOptions
    }
  )));
}
function RoleWiringEditor({ value, profileOptions, onChange }) {
  const wiringState = window.MobKitFlowController.mobRoleWiringEditorState(value, profileOptions);
  const updateRule = (index, patch) => {
    onChange(window.MobKitFlowController.mobRoleWiringUpdatePatch(wiringState.wiring, index, patch, wiringState.options));
  };
  const removeRule = (index) => {
    onChange(window.MobKitFlowController.mobRoleWiringDeletePatch(wiringState.wiring, index));
  };
  const addRule = () => {
    onChange(window.MobKitFlowController.mobRoleWiringAddPatch(wiringState.wiring, wiringState.options));
  };
  return /* @__PURE__ */ React.createElement("div", { className: "twk-row" }, /* @__PURE__ */ React.createElement("div", { className: "twk-lbl" }, /* @__PURE__ */ React.createElement("span", null, wiringState.label), /* @__PURE__ */ React.createElement("span", null, wiringState.countLabel)), /* @__PURE__ */ React.createElement("div", { style: { display: "grid", gap: 6 } }, wiringState.wiring.map((rule, index) => /* @__PURE__ */ React.createElement("div", { key: `${rule.a}:${rule.b}:${index}`, style: { display: "grid", gridTemplateColumns: "1fr 1fr 26px", gap: 6 } }, /* @__PURE__ */ React.createElement("select", { className: "twk-field", value: rule.a, onChange: (e) => updateRule(index, { a: e.target.value }) }, wiringState.options.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value }, option.label))), /* @__PURE__ */ React.createElement("select", { className: "twk-field", value: rule.b, onChange: (e) => updateRule(index, { b: e.target.value }) }, wiringState.options.map((option) => /* @__PURE__ */ React.createElement("option", { key: option.value, value: option.value }, option.label))), /* @__PURE__ */ React.createElement("button", { className: "twk-field", style: { padding: 0 }, type: "button", onClick: () => removeRule(index) }, "\xD7"))), /* @__PURE__ */ React.createElement("button", { className: "twk-field", type: "button", disabled: wiringState.addDisabled, onClick: addRule }, wiringState.addLabel)));
}
function AdvancedMobSettingsEditor({ value, onChange }) {
  const advancedState = window.MobKitFlowController.advancedMobSettingsEditorState(value);
  const [draft, setDraft] = React.useState(advancedState.text);
  const [error, setError] = React.useState("");
  React.useEffect(() => {
    setDraft(advancedState.text);
    setError("");
  }, [advancedState.text]);
  const commit = (next) => {
    setDraft(next);
    const result = window.MobKitFlowController.advancedMobSettingsDraftPatch(next);
    setError(result.error || "");
    if (result.ok) onChange(result.value);
  };
  return /* @__PURE__ */ React.createElement("div", { className: "twk-row" }, /* @__PURE__ */ React.createElement("div", { className: "twk-lbl" }, /* @__PURE__ */ React.createElement("span", null, advancedState.label), error && /* @__PURE__ */ React.createElement("span", null, error)), /* @__PURE__ */ React.createElement(
    "textarea",
    {
      className: "twk-field",
      style: { height: 118, paddingTop: 7, fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace", resize: "vertical" },
      value: draft,
      onChange: (e) => commit(e.target.value)
    }
  ));
}
ReactDOM.createRoot(document.getElementById("root")).render(/* @__PURE__ */ React.createElement(App, null));
