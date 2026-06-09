const assert = require("node:assert/strict");

global.window = {};

require("../src/controller.js");

const controller = global.window.MobKitFlowController;
assert.deepEqual(controller.emptyAuthoringFlowState(), {
  name: "",
  steps: [],
});
const TEST_DEPLOY_VIEW_SCHEMA = {
  brand_label: "MobKit · Flow Editor",
  flows_tab_label: "FLOWS",
  agents_tab_label: "AGENTS",
  mob_status_title: "Active mob configuration",
  mob_file_label: "mob.toml",
  api_error_label: "api error",
  api_ready_label: "api ready",
  api_loading_label: "loading",
  deploy_prefix_label: "deploy:",
  flows_crumb_label: "flows",
  crumb_separator: "/",
  plan_trace_label: "PLAN TRACE",
  import_label: "IMPORT",
  validate_label: "VALIDATE",
  publish_label: "PUBLISH",
  deploy_plan_label: "DEPLOY PLAN",
  deploy_label: "DEPLOY",
  theme_switch_prefix: "Switch to",
  theme_switch_suffix: "mode",
  dark_theme_label: "☾ dark",
  light_theme_label: "☀ light",
  basic_mode_title: "Basic Editor",
  basic_mode_label: "Basic",
  graph_mode_title: "Graph Editor",
  graph_mode_label: "Graph",
  validation_eyebrow: "VALIDATE · MobKit",
  validation_passed_label: "passed",
  validation_warnings_label: "warnings",
  validation_blocking_label: "blocking",
  close_label: "×",
  plan_eyebrow: "DEPLOY PLAN",
  plan_unavailable_head: "DEPLOY TRACE UNAVAILABLE",
  plan_unavailable_body: "mobkit/mobpacks/deploy did not return plan_trace.",
  plan_first_label: "first",
  plan_step_label: "step",
  plan_previous_label: "‹",
  plan_next_label: "›",
};
const TEST_DEPLOY_VIEW = {
  brandLabel: "MobKit · Flow Editor",
  flowsTabLabel: "FLOWS",
  agentsTabLabel: "AGENTS",
  mobStatusTitle: "Active mob configuration",
  mobFileLabel: "mob.toml",
  apiErrorLabel: "api error",
  apiReadyLabel: "api ready",
  apiLoadingLabel: "loading",
  deployPrefixLabel: "deploy:",
  flowsCrumbLabel: "flows",
  crumbSeparator: "/",
  planTraceLabel: "PLAN TRACE",
  importLabel: "IMPORT",
  validateLabel: "VALIDATE",
  publishLabel: "PUBLISH",
  deployPlanLabel: "DEPLOY PLAN",
  deployLabel: "DEPLOY",
  themeSwitchPrefix: "Switch to",
  themeSwitchSuffix: "mode",
  darkThemeLabel: "☾ dark",
  lightThemeLabel: "☀ light",
  basicModeTitle: "Basic Editor",
  basicModeLabel: "Basic",
  graphModeTitle: "Graph Editor",
  graphModeLabel: "Graph",
  validationEyebrow: "VALIDATE · MobKit",
  validationPassedLabel: "passed",
  validationWarningsLabel: "warnings",
  validationBlockingLabel: "blocking",
  closeLabel: "×",
  planEyebrow: "DEPLOY PLAN",
  planUnavailableHead: "DEPLOY TRACE UNAVAILABLE",
  planUnavailableBody: "mobkit/mobpacks/deploy did not return plan_trace.",
  planFirstLabel: "first",
  planStepLabel: "step",
  planPreviousLabel: "‹",
  planNextLabel: "›",
};
const TEST_AGENT_ACCESS_VIEW_SCHEMA = {
  tool_invalid_error: "Use a MobKit-listed runtime tool or configured MCP/Rust source.",
  tool_title: "TOOL ACCESS",
  tool_hint: "Authority is calculated from this allowlist. Reviewed once here.",
  tool_missing_description: "—",
  tool_remove_label: "×",
  tool_add_select_placeholder: "+ add tool…",
  tool_source_label: "Configured tool source",
  tool_source_placeholder: "choose from MobKit tool catalog",
  tool_add_button_label: "ADD",
  inline_skill_realm_id: "mobkit/editor-inline",
  inline_skill_realm_label: "This mobpack",
  inline_skill_default_description: "Inline MobKit skill stored in this mobpack.",
  skill_default_description: "MobKit skill",
  skill_selected_check_label: "✓",
  skill_remove_label: "×",
  skill_section_title: "SKILLS",
  skill_inline_cancel_label: "CANCEL",
  skill_inline_open_label: "+ INLINE",
  skill_hint: "Selected skills are baked into the mobpack. Browse a realm to add more.",
  skill_inline_label_placeholder: "mob.skill-name",
  skill_inline_content_rows: 4,
  skill_inline_content_placeholder: "Skill instructions stored as [skills.<id>] content",
  skill_inline_create_hint: "Creates an inline skill definition in this mobpack.",
  skill_inline_add_label: "ADD SKILL",
  skill_inline_error_fallback: "Could not create inline skill.",
  skill_inline_missing_label_error: "Inline skill id or label is required.",
  skill_inline_missing_content_error: "Inline skill content is required.",
  skill_inline_invalid_id_error: "Inline skill id or label must contain letters or numbers.",
  skill_no_realms_message: "MobKit did not provide skill realms for this document.",
  skill_realm_label: "Realm",
  skill_default_realm_suffix: " · default",
  skill_unavailable_heading: "Unavailable in MobKit skill realms:",
  skill_outside_realm_heading: "Selected from other realms:",
};
const TEST_AGENT_ACCESS_VIEW = {
  toolInvalidError: "Use a MobKit-listed runtime tool or configured MCP/Rust source.",
  toolTitle: "TOOL ACCESS",
  toolHint: "Authority is calculated from this allowlist. Reviewed once here.",
  toolMissingDescription: "—",
  toolRemoveLabel: "×",
  toolAddSelectPlaceholder: "+ add tool…",
  toolSourceLabel: "Configured tool source",
  toolSourcePlaceholder: "choose from MobKit tool catalog",
  toolAddButtonLabel: "ADD",
  inlineSkillRealmId: "mobkit/editor-inline",
  inlineSkillRealmLabel: "This mobpack",
  inlineSkillDefaultDescription: "Inline MobKit skill stored in this mobpack.",
  skillDefaultDescription: "MobKit skill",
  skillSelectedCheckLabel: "✓",
  skillRemoveLabel: "×",
  skillSectionTitle: "SKILLS",
  skillInlineCancelLabel: "CANCEL",
  skillInlineOpenLabel: "+ INLINE",
  skillHint: "Selected skills are baked into the mobpack. Browse a realm to add more.",
  skillInlineLabelPlaceholder: "mob.skill-name",
  skillInlineContentRows: 4,
  skillInlineContentPlaceholder: "Skill instructions stored as [skills.<id>] content",
  skillInlineCreateHint: "Creates an inline skill definition in this mobpack.",
  skillInlineAddLabel: "ADD SKILL",
  skillInlineErrorFallback: "Could not create inline skill.",
  skillInlineMissingLabelError: "Inline skill id or label is required.",
  skillInlineMissingContentError: "Inline skill content is required.",
  skillInlineInvalidIdError: "Inline skill id or label must contain letters or numbers.",
  skillNoRealmsMessage: "MobKit did not provide skill realms for this document.",
  skillRealmLabel: "Realm",
  skillDefaultRealmSuffix: " · default",
  skillUnavailableHeading: "Unavailable in MobKit skill realms:",
  skillOutsideRealmHeading: "Selected from other realms:",
};
const TEST_SETTINGS_VIEW_SCHEMA = {
  panel_title: "Tweaks",
  load_mob_title: "Load mob",
  load_mob_label: "Mobpack",
  flow_stage_fallback: "draft",
  option_separator: " · ",
  canvas_title: "Canvas",
  edge_style_label: "Edges",
  edge_style_options: [{ value: "text", label: "Text" }, { value: "icons", label: "Icons" }, { value: "colored", label: "Color" }],
  density_label: "Density",
  density_options: [{ value: "compact", label: "Compact" }, { value: "comfortable", label: "Comfy" }],
  theme_title: "Theme",
  theme_mode_label: "Mode",
  theme_mode_options: [{ value: "light", label: "Light" }, { value: "dark", label: "Dark" }],
  mob_title: "Mob",
  orchestrator_label: "Orchestrator",
  profile_none_label: "none",
  auto_wire_label: "Auto wire",
  auto_wire_options: [{ value: "no", label: "No" }, { value: "yes", label: "Yes" }],
  role_wiring_label: "Role wiring",
  role_wiring_add_label: "+ rule",
  default_backend_label: "Default backend",
  external_base_label: "External base",
  external_base_placeholder: "http://127.0.0.1:9000",
  advanced_label: "Advanced",
  advanced_object_required_error: "object required",
  advanced_invalid_json_error: "invalid JSON",
  deploy_title: "Deploy",
  surface_label: "Surface",
  trust_label: "Trust",
  model_label: "Model",
  model_default_label: "default",
  model_vendor_fallback: "provider",
  duration_label: "Duration",
  duration_placeholder: "30s",
  tool_calls_label: "Tool calls",
  tool_calls_min: 0,
  tool_calls_max: 999,
  tokens_label: "Tokens",
  tokens_min: 0,
  tokens_max: 200000,
  realm_label: "Realm",
  realm_options: [{ value: "isolated", label: "Isolated" }, { value: "shared", label: "Shared" }],
  realm_id_label: "Realm ID",
  realm_id_placeholder: "realm id",
  backend_label: "Backend",
  prompt_label: "Prompt",
  prompt_placeholder: "Deploy prompt",
  command_label: "Command",
  command_fallback: "--",
  inspector_title: "Inspector",
  inspector_layout_label: "Layout",
  inspector_layout_options: [{ value: "right", label: "Right" }, { value: "bottom", label: "Bottom" }, { value: "modal", label: "Modal" }],
};
const TEST_SETTINGS_VIEW = {
  panelTitle: "Tweaks",
  loadMobTitle: "Load mob",
  loadMobLabel: "Mobpack",
  flowStageFallback: "draft",
  optionSeparator: " · ",
  canvasTitle: "Canvas",
  edgeStyleLabel: "Edges",
  edgeStyleOptions: [{ value: "text", label: "Text" }, { value: "icons", label: "Icons" }, { value: "colored", label: "Color" }],
  densityLabel: "Density",
  densityOptions: [{ value: "compact", label: "Compact" }, { value: "comfortable", label: "Comfy" }],
  themeTitle: "Theme",
  themeModeLabel: "Mode",
  themeModeOptions: [{ value: "light", label: "Light" }, { value: "dark", label: "Dark" }],
  mobTitle: "Mob",
  orchestratorLabel: "Orchestrator",
  profileNoneLabel: "none",
  autoWireLabel: "Auto wire",
  autoWireOptions: [{ value: "no", label: "No" }, { value: "yes", label: "Yes" }],
  roleWiringLabel: "Role wiring",
  roleWiringAddLabel: "+ rule",
  defaultBackendLabel: "Default backend",
  externalBaseLabel: "External base",
  externalBasePlaceholder: "http://127.0.0.1:9000",
  advancedLabel: "Advanced",
  advancedObjectRequiredError: "object required",
  advancedInvalidJsonError: "invalid JSON",
  deployTitle: "Deploy",
  surfaceLabel: "Surface",
  trustLabel: "Trust",
  modelLabel: "Model",
  modelDefaultLabel: "default",
  modelVendorFallback: "provider",
  durationLabel: "Duration",
  durationPlaceholder: "30s",
  toolCallsLabel: "Tool calls",
  toolCallsMin: 0,
  toolCallsMax: 999,
  tokensLabel: "Tokens",
  tokensMin: 0,
  tokensMax: 200000,
  realmLabel: "Realm",
  realmOptions: [{ value: "isolated", label: "Isolated" }, { value: "shared", label: "Shared" }],
  realmIdLabel: "Realm ID",
  realmIdPlaceholder: "realm id",
  backendLabel: "Backend",
  promptLabel: "Prompt",
  promptPlaceholder: "Deploy prompt",
  commandLabel: "Command",
  commandFallback: "--",
  inspectorTitle: "Inspector",
  inspectorLayoutLabel: "Layout",
  inspectorLayoutOptions: [{ value: "right", label: "Right" }, { value: "bottom", label: "Bottom" }, { value: "modal", label: "Modal" }],
};
const TEST_LAUNCH_VIEW_SCHEMA = {
  launch_title: "Launch mode",
  graph_launch_title: "LAUNCH MODE · this position",
  resume_session_label: "Bridge session",
  resume_session_placeholder: "session id",
  fork_source_label: "Fork from",
  fork_context_label: "Fork context",
  graph_fork_context_label: "Context",
  budget_policy_label: "Budget split policy",
  fixed_budget_label: "Fixed token budget",
  fixed_budget_default_value: 4096,
  unsupported_label_separator: " — not in MobKit ",
  unsupported_reason_prefix: "Unsupported by the MobKit ",
  unsupported_reason_suffix: " contract.",
  launch_modes_contract_label: "launch_modes",
  fork_contexts_contract_label: "mob_definition.fork_contexts",
  budget_split_policies_contract_label: "budget_split_policies",
  launch_mode_labels: {
    Fresh: "Fresh — empty context",
    Resume: "Resume — existing bridge session",
    Fork: "Fork — copy context from another step",
  },
  fork_context_labels: {
    full_history: "full_history — entire transcript",
    last_messages: "last_messages — last N messages",
    FullHistory: "FullHistory — legacy alias for full_history",
  },
  budget_split_policy_labels: {
    Equal: "Equal — split remaining budget evenly",
    Proportional: "Proportional — MobKit proportional split",
    Remaining: "Remaining — grant all remaining budget",
    Fixed: "Fixed — token cap for this spawn",
  },
};
const TEST_LAUNCH_VIEW = {
  launchTitle: "Launch mode",
  graphLaunchTitle: "LAUNCH MODE · this position",
  resumeSessionLabel: "Bridge session",
  resumeSessionPlaceholder: "session id",
  forkSourceLabel: "Fork from",
  forkContextLabel: "Fork context",
  graphForkContextLabel: "Context",
  budgetPolicyLabel: "Budget split policy",
  fixedBudgetLabel: "Fixed token budget",
  fixedBudgetDefaultValue: 4096,
  unsupportedLabelSeparator: " — not in MobKit ",
  unsupportedReasonPrefix: "Unsupported by the MobKit ",
  unsupportedReasonSuffix: " contract.",
  launchModesContractLabel: "launch_modes",
  forkContextsContractLabel: "mob_definition.fork_contexts",
  budgetSplitPoliciesContractLabel: "budget_split_policies",
  launchModeLabels: TEST_LAUNCH_VIEW_SCHEMA.launch_mode_labels,
  forkContextLabels: TEST_LAUNCH_VIEW_SCHEMA.fork_context_labels,
  budgetSplitPolicyLabels: TEST_LAUNCH_VIEW_SCHEMA.budget_split_policy_labels,
};
const TEST_CONDITION_VIEW_SCHEMA = {
  empty_value_label: "—",
  text_value_placeholder: "value",
};
const TEST_CONDITION_VIEW = {
  emptyValueLabel: "—",
  textValuePlaceholder: "value",
};
const TEST_ERROR_VIEW_SCHEMA = {
  critical_glyph: "!",
  generic_error_head: "MobKit error",
  deploy_failed_head: "Deploy failed",
  deploy_plan_failed_head: "Deploy plan failed",
  deploy_error_meta: "mobkit/mobpacks/deploy",
  source_failed_head: "Source render failed",
  source_error_meta: "mobkit/mobpacks/export",
  validation_api_failed_head: "MobKit API unavailable",
  rpc_error_meta: "/flow-editor/rpc",
  export_failed_head: "Export failed",
  import_failed_head: "Import failed",
  missing_editor_flow_head: "Imported mobpack is missing a MobKit editor flow",
  missing_editor_flow_sub: "mobkit/mobpacks/import did not return document.flow.steps",
  missing_editor_flow_meta: "missing_editor_flow",
};
const TEST_ERROR_VIEW = {
  criticalGlyph: "!",
  genericErrorHead: "MobKit error",
  deployFailedHead: "Deploy failed",
  deployPlanFailedHead: "Deploy plan failed",
  deployErrorMeta: "mobkit/mobpacks/deploy",
  sourceFailedHead: "Source render failed",
  sourceErrorMeta: "mobkit/mobpacks/export",
  validationApiFailedHead: "MobKit API unavailable",
  rpcErrorMeta: "/flow-editor/rpc",
  exportFailedHead: "Export failed",
  importFailedHead: "Import failed",
  missingEditorFlowHead: "Imported mobpack is missing a MobKit editor flow",
  missingEditorFlowSub: "mobkit/mobpacks/import did not return document.flow.steps",
  missingEditorFlowMeta: "missing_editor_flow",
};
const TEST_NEW_FLOW_VIEW_SCHEMA = {
  eyebrow_template: "CREATE MOB · STEP {step} / 2",
  close_label: "close",
  name_label: "Mob name",
  name_placeholder: "sample-mob",
  trigger_label: "Mob trigger",
  trigger_placeholder: "label · task",
  start_from_label: "Template",
  back_label: "BACK",
  next_label: "NEXT",
  create_label: "CREATE MOB",
};
const TEST_NEW_FLOW_VIEW = {
  eyebrowTemplate: "CREATE MOB · STEP {step} / 2",
  closeLabel: "close",
  nameLabel: "Mob name",
  namePlaceholder: "sample-mob",
  triggerLabel: "Mob trigger",
  triggerPlaceholder: "label · task",
  startFromLabel: "Template",
  backLabel: "BACK",
  nextLabel: "NEXT",
  createLabel: "CREATE MOB",
};
const TEST_FLOW_REGISTRY_VIEW_SCHEMA = {
  eyebrow: "MOBS",
  title_singular_suffix: "mob",
  title_plural_suffix: "mobs",
  create_label: "+ CREATE MOB",
  create_ready_title: "Create a deployable MobKit mobpack",
  create_unavailable_title: "MobKit authoring contract unavailable",
  columns: [
    { key: "name", label: "MOB" },
    { key: "trigger", label: "TRIGGER" },
    { key: "version", label: "PACK" },
    { key: "stage", label: "STATE" },
  ],
};
const TEST_FLOW_REGISTRY_VIEW = {
  eyebrow: "MOBS",
  titleSingularSuffix: "mob",
  titlePluralSuffix: "mobs",
  createLabel: "+ CREATE MOB",
  createReadyTitle: "Create a deployable MobKit mobpack",
  createUnavailableTitle: "MobKit authoring contract unavailable",
  columns: [
    { key: "name", label: "MOB" },
    { key: "trigger", label: "TRIGGER" },
    { key: "version", label: "PACK" },
    { key: "stage", label: "STATE" },
  ],
};
const TEST_SCHEMA = {
  deploy_settings: {
    command: "rkat mob deploy",
    surfaces: ["cli"],
    defaults: {
      command: "rkat mob deploy",
      surface: "cli",
      trust_policy: "permissive",
      model: "",
      max_duration: "30s",
      max_tool_calls: 0,
      max_total_tokens: 64,
      isolated: true,
      realm: "",
      instance: "",
      realm_backend: "jsonl",
      context_root: "",
      state_root: "",
      user_config_root: "",
      prompt: "Reply with exactly OK.",
    },
  },
  mob_definition: {
    editor_deploy_view: TEST_DEPLOY_VIEW_SCHEMA,
    editor_agent_access_view: TEST_AGENT_ACCESS_VIEW_SCHEMA,
    editor_settings_view: TEST_SETTINGS_VIEW_SCHEMA,
    editor_launch_view: TEST_LAUNCH_VIEW_SCHEMA,
    editor_condition_view: TEST_CONDITION_VIEW_SCHEMA,
    editor_error_view: TEST_ERROR_VIEW_SCHEMA,
    editor_new_flow_view: TEST_NEW_FLOW_VIEW_SCHEMA,
    editor_flow_registry_view: TEST_FLOW_REGISTRY_VIEW_SCHEMA,
    mob_settings: {
      defaults: {
        orchestrator: "",
        autoWireOrchestrator: false,
        roleWiring: [],
        backendDefault: "session",
        externalAddressBase: "",
        advanced: {
          topology: null,
          supervisor: null,
          limits: null,
          spawnPolicy: null,
          eventRouter: null,
        },
      },
    },
  },
};
const testDeploySettings = () => controller.deployDefaultsFromSchema(TEST_SCHEMA);

assert.deepEqual(controller.deployDefaultsFromSchema(null), {
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
});
assert.equal(controller.normalizeDeploySettings(controller.deployDefaultsFromSchema(null)).command, "");
assert.equal(controller.mobDefaultsFromSchema(null).backendDefault, "");
assert.equal(controller.normalizeBudgetSplitPolicy(null), null);
assert.equal(controller.normalizeBudgetSplitPolicy(undefined), null);
assert.deepEqual(controller.topRailState({
  contract: null,
  deploySettings: controller.deployDefaultsFromSchema(null),
  stage: "draft",
  view: "flows",
  theme: "light",
  deployView: TEST_DEPLOY_VIEW,
}), {
  inEditor: false,
  brandLabel: "MobKit · Flow Editor",
  flowsTabLabel: "FLOWS",
  agentsTabLabel: "AGENTS",
  mobStatusTitle: "Active mob configuration",
  mobFileLabel: "mob.toml",
  contractState: "loading",
  deployPrefixLabel: "deploy:",
  deployCommand: "",
  deploySurface: "",
  flowsCrumbLabel: "flows",
  crumbSeparator: "/",
  planTraceLabel: "PLAN TRACE",
  importLabel: "IMPORT",
  validateLabel: "VALIDATE",
  publishLabel: "PUBLISH",
  deployPlanLabel: "DEPLOY PLAN",
  deployLabel: "DEPLOY",
  deployActionsDisabled: true,
  themeToggleTitle: "Switch to dark mode",
  themeToggleLabel: "☀ light",
  basicModeTitle: "Basic Editor",
  basicModeLabel: "Basic",
  graphModeTitle: "Graph Editor",
  graphModeLabel: "Graph",
});
assert.deepEqual(controller.topRailState({
  contract: TEST_SCHEMA,
  deploySettings: testDeploySettings(),
  stage: "valid",
  view: "editor",
  theme: "dark",
  deployView: TEST_DEPLOY_VIEW,
}), {
  inEditor: true,
  brandLabel: "MobKit · Flow Editor",
  flowsTabLabel: "FLOWS",
  agentsTabLabel: "AGENTS",
  mobStatusTitle: "Active mob configuration",
  mobFileLabel: "mob.toml",
  contractState: "api ready",
  deployPrefixLabel: "deploy:",
  deployCommand: "rkat mob deploy",
  deploySurface: "cli",
  flowsCrumbLabel: "flows",
  crumbSeparator: "/",
  planTraceLabel: "PLAN TRACE",
  importLabel: "IMPORT",
  validateLabel: "VALIDATE",
  publishLabel: "PUBLISH",
  deployPlanLabel: "DEPLOY PLAN",
  deployLabel: "DEPLOY",
  deployActionsDisabled: false,
  themeToggleTitle: "Switch to light mode",
  themeToggleLabel: "☾ dark",
  basicModeTitle: "Basic Editor",
  basicModeLabel: "Basic",
  graphModeTitle: "Graph Editor",
  graphModeLabel: "Graph",
});
assert.equal(controller.topRailState({
  contract: { error: "schema unavailable", deploy_settings: { command: "rkat mob deploy", surfaces: ["cli"] } },
  deploySettings: { surface: "" },
  stage: "published",
  view: "editor",
  deployView: TEST_DEPLOY_VIEW,
}).contractState, "api error");

assert.equal(controller.buildBlankDocument, undefined, "blank mobpack documents must come from MobKit schema, not a local builder");

const schemaBlankMobpack = controller.blankMobpackFromSchema({
  blank_mobpack: {
    id: "blank",
    name: "Blank",
    source: "mobkit/blank-mobpack",
    trigger: "label · small-fix",
    version: "0.1.0",
    stage: "valid",
    document: {
      schema_version: "0.1.0",
      mob_id: "blank_mob",
      name: "blank-mob",
      flow: { name: "blank-mob", steps: [{ id: "work", type: "member", role: "m_worker" }] },
    },
    validation: { ok: true },
  },
});
assert.equal(schemaBlankMobpack.id, "blank");
assert.equal(schemaBlankMobpack.source, "mobkit/blank-mobpack");
assert.equal(controller.blankMobpackFromSchema({
  blank_mobpack: {
    source: "mobkit/blank-mobpack",
    document: { mob_id: "blank_mob" },
  },
}), null);
assert.equal(controller.blankMobpackFromSchema({
  blank_mobpack: {
    id: "blank",
    document: { mob_id: "blank_mob" },
  },
}), null);

assert.deepEqual(controller.newFlowTemplateOptions([
  { id: "docs", name: "Docs", trigger: "label · docs", validation: { ok: true } },
  { id: "needs_source", name: "Needs Source", source: "mobkit://samples/needs-source", validation: { ok: false } },
  { id: "", name: "Missing Id", validation: { ok: true } },
  { id: "missing_name", name: "", validation: { ok: true } },
], { canCreateBlank: false, blankTemplate: schemaBlankMobpack }), [
  {
    id: "blank",
    label: "Blank",
    sub: "label · small-fix",
    tier: "valid",
    disabled: true,
  },
  {
    id: "docs",
    label: "Docs",
    sub: "label · docs",
    tier: "valid",
    disabled: false,
  },
  {
    id: "needs_source",
    label: "Needs Source",
    sub: "mobkit://samples/needs-source",
    tier: "draft",
    disabled: false,
  },
]);
assert.equal(controller.newFlowTemplateOptions([], { canCreateBlank: true })[0].disabled, true);
assert.equal(controller.newFlowTemplateOptions([], { canCreateBlank: true, blankTemplate: schemaBlankMobpack })[0].disabled, false);
assert.deepEqual(controller.newFlowInitialState({ blankTemplate: schemaBlankMobpack }), {
  step: 1,
  name: "",
  trigger: "label · small-fix",
  template: "blank",
});
assert.deepEqual(controller.newFlowInitialState({ blankTemplate: { id: "missing-document", trigger: "ignored" } }), {
  step: 1,
  name: "",
  trigger: "",
  template: "",
});
assert.deepEqual(controller.newFlowModalState({
  step: 2,
  name: "New Mob",
  trigger: "label · docs",
  template: "docs",
}, controller.newFlowTemplateOptions([
  { id: "docs", name: "Docs", trigger: "label · docs", validation: { ok: true } },
], { canCreateBlank: false, blankTemplate: schemaBlankMobpack }), TEST_NEW_FLOW_VIEW), {
  step: 2,
  eyebrow: "CREATE MOB · STEP 2 / 2",
  closeLabel: "close",
  nameLabel: "Mob name",
  namePlaceholder: "sample-mob",
  triggerLabel: "Mob trigger",
  triggerPlaceholder: "label · task",
  startFromLabel: "Template",
  backLabel: "BACK",
  nextLabel: "NEXT",
  createLabel: "CREATE MOB",
  name: "New Mob",
  trigger: "label · docs",
  template: "docs",
  options: [
    {
      id: "blank",
      label: "Blank",
      sub: "label · small-fix",
      tier: "valid",
      disabled: true,
      className: "template-card",
    },
    {
      id: "docs",
      label: "Docs",
      sub: "label · docs",
      tier: "valid",
      disabled: false,
      className: "template-card is-selected",
    },
  ],
  createDisabled: false,
  nextDisabled: false,
});
assert.equal(controller.newFlowModalState({ step: 1, name: "   ", template: "blank" }, [
  { id: "blank", disabled: true },
]).nextDisabled, true);
assert.equal(controller.newFlowModalState({ step: 2, name: "Draft", template: "blank" }, [
  { id: "blank", disabled: true },
]).createDisabled, true);
assert.deepEqual(controller.flowRegistryViewState([
  { id: "f_existing", name: "Existing", trigger: "label · docs", version: "0.1", stage: "valid" },
  { id: "f_draft", name: "Draft", trigger: "", version: "", stage: "" },
], "f_existing", { canCreate: false, flowRegistryView: TEST_FLOW_REGISTRY_VIEW }), {
  eyebrow: "MOBS",
  title: "2 mobs",
  createLabel: "+ CREATE MOB",
  createDisabled: true,
  createTitle: "MobKit authoring contract unavailable",
  columns: [
    { key: "name", label: "MOB" },
    { key: "trigger", label: "TRIGGER" },
    { key: "version", label: "PACK" },
    { key: "stage", label: "STATE" },
  ],
  rows: [
    {
      id: "f_existing",
      className: "flows-list__row is-current",
      name: "Existing",
      trigger: "label · docs",
      version: "0.1",
      stage: "valid",
    },
    {
      id: "f_draft",
      className: "flows-list__row",
      name: "Draft",
      trigger: "",
      version: "",
      stage: "draft",
    },
  ],
});
assert.deepEqual(controller.flowRegistryViewState([{ id: "only", name: "One" }], "", { canCreate: true, flowRegistryView: TEST_FLOW_REGISTRY_VIEW }), {
  eyebrow: "MOBS",
  title: "1 mob",
  createLabel: "+ CREATE MOB",
  createDisabled: false,
  createTitle: "Create a deployable MobKit mobpack",
  columns: [
    { key: "name", label: "MOB" },
    { key: "trigger", label: "TRIGGER" },
    { key: "version", label: "PACK" },
    { key: "stage", label: "STATE" },
  ],
  rows: [{
    id: "only",
    className: "flows-list__row",
    name: "One",
    trigger: "",
    version: "",
    stage: "draft",
  }],
});

const templateDraft = controller.createFlowDraftFromSpec({
  id: "f_template",
  spec: { name: "Renamed Template", trigger: "label · docs", template: "sample_docs" },
  templates: [{
    id: "sample_docs",
    name: "Docs Sample",
    source: "mobkit://samples/docs",
    document: {
      name: "Docs Sample",
      mob_id: "docs_sample",
      schema_version: "1.0",
      flow: { name: "Docs Sample", steps: [{ id: "input_1", type: "input", task: "", fields: "", inputParams: [] }] },
      members: [{ id: "writer", name: "Writer", role: "writer", profileBinding: "inline", runtimeMode: "turn_driven" }],
      mob_toml: "[stale]",
    },
  }],
  deploySettings: testDeploySettings(),
  mobSettings: controller.mobDefaultsFromSchema(TEST_SCHEMA),
});
assert.equal(templateDraft.id, "f_template");
assert.equal(templateDraft.document.name, "Renamed Template");
assert.equal(templateDraft.document.mob_id, "renamed_template");
assert.equal(templateDraft.document.flow.name, "Renamed Template");
assert.equal(templateDraft.document.mob_toml, undefined);
assert.equal(templateDraft.row.id, "f_template");
assert.equal(templateDraft.row.name, "Renamed Template");
assert.equal(templateDraft.row.stage, "draft");
assert.equal(templateDraft.row.trigger, "label · docs");
assert.equal(templateDraft.row.source, "mobkit://samples/docs");
assert.equal(templateDraft.row.document, templateDraft.document);

const blankDraft = controller.createFlowDraftFromSpec({
  id: "f_blank",
  spec: { name: "Blank Created", trigger: "label · blank", template: "blank" },
  templates: [],
  blankTemplate: schemaBlankMobpack,
  deploySettings: testDeploySettings(),
  mobSettings: controller.mobDefaultsFromSchema(TEST_SCHEMA),
});
assert.equal(blankDraft.document.name, "Blank Created");
assert.equal(blankDraft.document.mob_id, "blank_created");
assert.equal(blankDraft.document.flow.name, "Blank Created");
assert.equal(blankDraft.document.flow.steps[0].type, "member");
assert.equal(blankDraft.row.name, "Blank Created");
assert.equal(blankDraft.row.trigger, "label · blank");
assert.equal(blankDraft.row.source, "mobkit/blank-mobpack");

const generatedBlankDraft = controller.createFlowDraftFromSpec({
  spec: { name: "Blank Created", trigger: "label · generated", template: "blank" },
  templates: [],
  existingRows: [{ id: "f_blank_created" }],
  blankTemplate: schemaBlankMobpack,
  deploySettings: testDeploySettings(),
  mobSettings: controller.mobDefaultsFromSchema(TEST_SCHEMA),
});
assert.equal(controller.flowDraftIdFromSpec({ name: "Blank Created" }, [{ id: "f_blank_created" }]), "f_blank_created_2");
assert.equal(generatedBlankDraft.id, "f_blank_created_2");
assert.equal(generatedBlankDraft.row.id, "f_blank_created_2");
assert.equal(generatedBlankDraft.row.trigger, "label · generated");

const createdDraftProjection = controller.flowRegistryCreateDraftProjection([{ id: "f_existing" }], {
  spec: { name: "Projected Draft", trigger: "label · projected", template: "blank" },
  templates: [],
  blankTemplate: schemaBlankMobpack,
  deploySettings: testDeploySettings(),
  mobSettings: controller.mobDefaultsFromSchema(TEST_SCHEMA),
});
assert.equal(createdDraftProjection.ok, true);
assert.equal(createdDraftProjection.draft.id, "f_projected_draft");
assert.equal(createdDraftProjection.rows.length, 2);
assert.equal(createdDraftProjection.rows[1].id, "f_projected_draft");
assert.equal(createdDraftProjection.hydration.result.document, createdDraftProjection.draft.document);
assert.equal(createdDraftProjection.hydration.options.id, "f_projected_draft");
assert.equal(createdDraftProjection.hydration.options.flowRow, createdDraftProjection.draft.row);
assert.equal(createdDraftProjection.hydration.options.addToRegistry, false);

const missingDraftProjection = controller.flowRegistryCreateDraftProjection([{ id: "f_existing" }], {
  spec: { name: "Missing", template: "blank" },
  templates: [],
});
assert.equal(missingDraftProjection.ok, false);
assert.equal(missingDraftProjection.rows.length, 1);
assert.equal(missingDraftProjection.hydration, null);

assert.equal(controller.createFlowDraftFromSpec({
  id: "f_blank_missing",
  spec: { name: "Blank Missing", template: "blank" },
  templates: [],
}), null);
assert.equal(controller.createFlowDraftFromSpec({ id: "", spec: {} }), null);
assert.deepEqual(controller.authoringRpcMethodsFromSchema({
  commands: {
    schema: "mobkit/mobpacks/schema",
    catalogs: "mobkit/editor/catalogs",
    validate: "mobkit/editor/validate",
    export: "mobkit/editor/export",
    import: "mobkit/editor/import",
    deploy_command: "mobkit/editor/deploy_command",
    deploy_rpc: "mobkit/editor/deploy",
  },
}), {
  schema: "mobkit/mobpacks/schema",
  catalogs: "mobkit/editor/catalogs",
  validate: "mobkit/editor/validate",
  export: "mobkit/editor/export",
  import: "mobkit/editor/import",
  deployCommand: "mobkit/editor/deploy_command",
  deploy: "mobkit/editor/deploy",
});
assert.deepEqual(controller.authoringRpcMethodsFromSchema({ commands: { catalogs: "" } }), {});

assert.deepEqual(controller.modelCatalogFromSchema({
  models: [
    { id: "missing-label", vendor: "openai" },
    { id: "missing-vendor", label: "Missing Vendor" },
    { id: "openai/gpt-5.5", label: "GPT-5.5", vendor: "openai", profile: { temperature: 0 } },
  ],
}), [{
  id: "openai/gpt-5.5",
  label: "GPT-5.5",
  vendor: "openai",
  profile: { temperature: 0 },
}]);

assert.deepEqual(controller.toolCatalogFromSchema({
  tool_config: [{
    id: "compat-only",
    label: "Compatibility Only",
    desc: "Must not hydrate from compatibility aliases.",
    kind: "runtime",
    source: "meerkat_mob::ToolConfig",
  }],
}), []);

assert.deepEqual(controller.toolCatalogFromSchema({
  tool_catalog: [
    { id: "missing-desc", label: "Missing Desc", kind: "runtime", source: "meerkat_mob::ToolConfig" },
    { id: "missing-source", label: "Missing Source", desc: "No source", kind: "runtime" },
    { id: "builtins", label: "builtins", desc: "Built-ins", kind: "runtime", source: "meerkat_mob::ToolConfig" },
  ],
}), [{
  id: "builtins",
  label: "builtins",
  desc: "Built-ins",
  kind: "runtime",
  source: "meerkat_mob::ToolConfig",
  raw: { id: "builtins", label: "builtins", desc: "Built-ins", kind: "runtime", source: "meerkat_mob::ToolConfig" },
}]);

const catalogBoot = { grid: { cellW: 10 }, cellXY: () => ({ x: 0, y: 0 }), template: { col: 1 } };
const emptyCatalogs = controller.emptyMobKitCatalogs(catalogBoot);
assert.equal(emptyCatalogs.contractMeta.loaded, false);
assert.equal(emptyCatalogs.grid, catalogBoot.grid);
assert.equal(emptyCatalogs.cellXY, catalogBoot.cellXY);
assert.equal(emptyCatalogs.template, null);
assert.equal(emptyCatalogs.conditionView, null);
assert.equal(emptyCatalogs.errorView, null);
assert.deepEqual(controller.schemaSkillRealms({ skill_realms: "starter" }), []);
const schemaOnlyCatalogLeakState = controller.mobKitCatalogsFromSchema({
  schema_version: "mobpack/v1",
  media_type: "application/vnd.mobkit.mobpack+json",
  validation_source: "mobkit/mobpacks/schema",
  deploy_settings: {
    defaults: { command: "rkat mob deploy", surface: "cli" },
  },
  mob_definition: {
    mob_settings: { defaults: { backendDefault: "session", advanced: { topology: null } } },
  },
  models: [{ id: "schema-leak-model", label: "Schema Leak Model", vendor: "openai" }],
  tool_catalog: [{ id: "schema-leak-tool", label: "Schema Leak Tool", desc: "leaked", kind: "runtime", source: "schema" }],
  skill_realms: [{ id: "schema/leak", skills: [{ id: "mob.schema.leak" }] }],
  agent_definitions: [{
    id: "schema_leak_agent",
    name: "Schema Leak Agent",
    role: "schema_leak_agent",
    definitionType: "mobkit/profile-member",
    source: "schema",
    model: "gpt-5.5",
    profileBinding: "inline",
    runtimeMode: "turn_driven",
  }],
  blank_mobpack: {
    id: "schema-leak-blank",
    name: "Schema Leak Blank",
    source: "schema",
    document: { flow: { name: "leak", steps: [] }, members: [] },
  },
}, catalogBoot);
assert.deepEqual(schemaOnlyCatalogLeakState.models, []);
assert.deepEqual(schemaOnlyCatalogLeakState.toolCatalog, []);
assert.deepEqual(schemaOnlyCatalogLeakState.skillRealms, []);
assert.deepEqual(schemaOnlyCatalogLeakState.agentDefinitions, []);
assert.equal(schemaOnlyCatalogLeakState.blankMobpack, null);
assert.equal(schemaOnlyCatalogLeakState.template, null);
const separateCatalogPayloadState = controller.mobKitCatalogsFromSchema({
  schema_version: "mobpack/v1",
  media_type: "application/vnd.mobkit.mobpack+json",
  validation_source: "mobkit/mobpacks/schema",
  deploy_settings: {
    defaults: { command: "rkat mob deploy", surface: "cli" },
  },
  mob_definition: {
    mob_settings: { defaults: { backendDefault: "session", advanced: { topology: null } } },
  },
}, catalogBoot, {
  models: [{ id: "openai/gpt-5.5", label: "GPT-5.5", vendor: "openai" }],
  tool_catalog: [{ id: "shell", label: "shell", desc: "Shell", kind: "runtime", source: "meerkat_mob::ToolConfig" }],
  skill_realms: [{ id: "mobkit/sample-mobpacks", source: "mobkit/sample-mobpack", skills: [{ id: "mob.review" }] }],
  agent_definitions: [{
    id: "sample_reviewer",
    name: "Reviewer",
    role: "reviewer",
    definitionType: "mobkit/profile-member",
    source: "mobkit/sample-mobpack",
    model: "gpt-5.5",
    profileBinding: "inline",
    runtimeMode: "turn_driven",
  }],
  blank_mobpack: {
    id: "blank",
    name: "Blank",
    version: "mobpack/v1",
    stage: "valid",
    trigger: "label · small-fix",
    source: "mobkit/blank-mobpack",
    document: {
      name: "Blank",
      mob_id: "blank",
      flow: { name: "Blank", steps: [{ id: "work", type: "member", role: "m_worker" }] },
      members: [{ id: "m_worker", role: "worker" }],
    },
  },
});
assert.equal(separateCatalogPayloadState.contractMeta.schemaVersion, "mobpack/v1");
assert.deepEqual(separateCatalogPayloadState.models.map((model) => model.id), ["openai/gpt-5.5"]);
assert.deepEqual(separateCatalogPayloadState.toolCatalog.map((tool) => tool.id), ["shell"]);
assert.deepEqual(separateCatalogPayloadState.skillRealms.map((realm) => realm.id), ["mobkit/sample-mobpacks"]);
assert.deepEqual(separateCatalogPayloadState.agentDefinitions.map((definition) => definition.id), ["sample_reviewer"]);
assert.equal(separateCatalogPayloadState.template.name, "Blank");

const hydratedContractAndCatalogFixture = {
  schema_version: "mobpack/v1",
  media_type: "application/vnd.mobkit.mobpack+json",
  validation_source: "mobkit/mobpacks/schema",
  deploy_settings: {
    defaults: {
      command: "rkat mob deploy",
      surface: "cli",
      trust_policy: "permissive",
      isolated: true,
    },
  },
  mob_definition: {
    runtime_modes: ["turn_driven"],
    editor_source_view: {
      drawer_eyebrow: "SOURCE · mob.toml",
      inline_title: "mob.toml",
      loading_text: "rendering mob.toml from mobkit/mobpacks/export...",
      copy_label: "copy",
      close_label: "×",
    },
    editor_condition_view: TEST_CONDITION_VIEW_SCHEMA,
    editor_error_view: TEST_ERROR_VIEW_SCHEMA,
    editor_new_flow_view: TEST_NEW_FLOW_VIEW_SCHEMA,
    editor_flow_registry_view: TEST_FLOW_REGISTRY_VIEW_SCHEMA,
    editor_agent_view: {
      agents_heading: "AGENTS",
      schemas_heading: "SCHEMAS",
      add_schema_label: "+ new schema",
      add_agent_title: "Create an agent from a MobKit profile-member definition.",
      add_agent_unavailable_title: "MobKit schema contract has not provided agent definitions yet.",
      add_agent_unavailable_label: "agents unavailable",
      add_agent_placeholder_label: "+ new agent...",
      empty_title: "AGENT LIBRARY",
      empty_lines: [
        "Select an agent or schema on the left.",
        "Agents are reusable across topologies. Edit one here and every placement updates.",
      ],
      missing_schema_label: "Schema not found.",
      missing_agent_label: "Agent not found.",
    },
    editor_agent_detail_view: {
      used_in_label: "used in",
      instance_singular: "instance",
      instance_plural: "instances",
      delete_label: "DELETE",
      delete_confirm_intro: "Delete agent",
      delete_confirm_placed_prefix: "It is placed in",
      cell_singular: "cell",
      cell_plural: "cells",
      delete_confirm_cells_suffix: "those nodes will be removed.",
      usage_title_prefix: "USED IN",
      empty_usage_hint: "Not yet placed in any cell. Switch to Topology to add.",
      identity_title: "IDENTITY",
      profile_binding_label: "Profile binding",
      missing_profile_binding_label: "missing profile binding",
      realm_profile_label: "Realm profile",
      realm_profile_placeholder: "realm profile id",
      realm_profile_import_hint_fallback: "Realm profile refs are import-only for this editor. Mobpack archives must use inline profiles before validation/export.",
      realm_profile_title: "REALM PROFILE",
      realm_profile_reference_hint_before: "This imported member references",
      realm_profile_reference_hint_after_fallback: "from a target realm. Convert it to an inline profile before validating or exporting a deployable mobpack.",
      model_label: "Model",
      runtime_mode_label: "Runtime mode",
      missing_runtime_mode_label: "missing runtime mode",
      backend_label: "Backend",
      backend_definition_default_label: "definition default",
      inline_peer_notifications_label: "Inline peer notifications",
      inline_peer_notifications_placeholder: "runtime default",
      provider_params_label: "Provider params",
      provider_params_placeholder: '{"thinking_budget":4096}',
      provider_params_rows: 4,
      provider_params_invalid_json_label: "invalid JSON",
      provider_params_object_required_error: "provider_params must be a JSON object",
      system_prompt_title: "SYSTEM PROMPT",
      apply_skeleton_label: "APPLY SKELETON",
      apply_skeleton_title: "Apply a MobKit profile prompt skeleton",
      system_prompt_placeholder: "Describe the member mandate. This text is exported as the profile peer_description.",
      output_schema_title: "OUTPUT SCHEMA",
      schema_none_label: "— none —",
      schema_required_label: "req",
      edit_schema_label: "Edit schema →",
      empty_schema_hint: "No structured output. Agent returns free-form text.",
    },
    editor_agent_access_view: TEST_AGENT_ACCESS_VIEW_SCHEMA,
    editor_deploy_view: TEST_DEPLOY_VIEW_SCHEMA,
    editor_settings_view: TEST_SETTINGS_VIEW_SCHEMA,
    editor_launch_view: TEST_LAUNCH_VIEW_SCHEMA,
    editor_schema_view: {
      eyebrow: "OUTPUT SCHEMA",
      description_title: "DESCRIPTION",
      description_placeholder: "What is this artifact and when is it emitted?",
      fields_title_prefix: "FIELDS",
      add_field_label: "+ field",
      header_labels: {
        name: "NAME",
        type: "TYPE",
        required: "REQ",
        description: "DESCRIPTION",
        action: "",
      },
      empty_fields_hint: "No fields yet. Click + field to start.",
      used_by_prefix: "USED BY",
      empty_used_by_hint: "Not yet referenced by any agent.",
      delete_label: "DELETE",
      delete_blocked_title: "Unassign from agents first",
      field_name_placeholder: "field_name",
      field_description_placeholder: "—",
      field_remove_title: "Remove field",
      field_enum_label: "VALUES",
      field_enum_add_label: "+ value",
      field_enum_add_value: "value",
    },
    editor_basic_view: {
      start_label: "START",
      loop_badge: "LOOP",
      tips_title: "Tips",
      empty_panel_title: "Build your mob flow",
      empty_panel_subtitle_parts: [
        { kind: "text", text: "Pick a node to configure, or press " },
        { kind: "strong", text: "+" },
        { kind: "text", text: " to add a member turn or flow primitive. The result is a " },
        { kind: "code", text: "mob.toml" },
        { kind: "text", text: " flow." },
      ],
      member_step_panel_title_fallback: "Member step",
      member_step_panel_sub_fallback: "Assign a member to run this step",
      member_step_member_label: "Member (profile)",
      member_step_member_placeholder: "— select member —",
      member_step_runtime_default_label: "runtime default",
      member_step_instruction_label: "message — instruction for this turn",
      member_step_instruction_placeholder: "e.g. Run the focused tests and report failures.",
      member_step_dispatch_label: "Dispatch mode",
      member_step_collection_label: "Collection policy",
      member_step_quorum_label: "Quorum",
      member_step_quorum_placeholder: "required",
      member_step_timeout_label: "Timeout (ms)",
      member_step_dependency_label: "depends_on mode",
      member_step_output_format_label: "Output format",
      member_step_allowed_tools_label: "Allowed tools",
      member_step_allowed_tools_empty_label: "Runtime profile default",
      member_step_blocked_tools_label: "Blocked tools",
      member_step_blocked_tools_empty_label: "No step-level blocks",
      member_step_schema_hint_prefix: "Emits ",
      member_step_schema_hint_tools_prefix: " · tools: ",
      member_step_schema_hint_empty_tools_label: "—",
      tool_scope_not_in_catalog_reason: "not in MobKit tool catalog",
      tool_scope_not_enabled_reason: "not enabled on profile",
      tool_scope_tool_description_fallback: "MobKit tool",
      tool_scope_remove_label: "×",
      tool_scope_select_member_placeholder: "select a member first",
      tool_scope_block_catalog_placeholder: "+ block MobKit tool...",
      tool_scope_add_profile_placeholder: "+ add profile tool...",
      input_panel_icon: "▤",
      input_panel_title: "Input",
      input_panel_sub: "The task this mob is run with — its ingress",
      input_task_label: "Task",
      input_task_placeholder: "e.g. Fix the issue described below.",
      input_params_title_prefix: "INPUT PARAMS",
      input_add_param_label: "+ param",
      input_param_source_label: "Input params",
      input_param_header_labels: {
        name: "NAME",
        type: "TYPE",
        required: "REQ",
        description: "DESCRIPTION",
        action: "",
      },
      input_param_name_placeholder: "param_name",
      input_param_description_placeholder: "—",
      input_param_remove_title: "Remove param",
      input_param_enum_label: "VALUES",
      input_param_enum_add_label: "+ value",
      input_param_enum_add_value: "value",
      input_empty_params_parts: [
        { key: "prefix", text: "No params yet. Add one before branching on " },
        { key: "ref", text: "params.*", kind: "code" },
        { key: "suffix", text: "." },
      ],
      input_tips: [
        "Run with: rkat mob deploy <pack> \"<task>\" — or run_flow(input).",
        "Typed fields become the input schema the run is validated against.",
        "Event sources & schedules live outside the mobpack (e.g. fugue).",
      ],
      branch_panel_title: "Branch",
      branch_panel_sub: "Choose one downstream path by condition",
      parallel_panel_title: "Parallel",
      parallel_panel_sub: "fan_out to members, then fan_in and collect",
      branch_route_member_label: "Route member",
      parallel_join_member_label: "Join member",
      branch_controller_placeholder_label: "— direct MobKit lanes —",
      branch_empty_controller_hint: "Without a selected profile, MobKit conditions/parallel lanes attach directly to the first real member in each lane.",
      branch_condition_title: "Branch conditions",
      branch_condition_intro: "Read in order; the first match wins. Conditions read a member's structured output.",
      branch_condition_row_title_prefix: "Branch",
      branch_condition_empty_hint: "Add an upstream member with an output schema before configuring this branch.",
      branch_condition_source_placeholder: "— source —",
      branch_condition_field_placeholder: "— field —",
      branch_condition_no_schema_label: "(no schema)",
      branch_condition_preview_prefix: "when",
      branch_condition_preview_fallback: "…",
      branch_fallback_title: "Fallback",
      branch_fallback_hint: "If none match, the flow follows the fallback path; else it stops.",
      add_branch_label: "+ Add branch",
      add_parallel_branch_label: "+ Add parallel branch",
      parallel_dispatch_label: "Dispatch mode",
      parallel_collection_label: "Collection policy (fan_in)",
      parallel_quorum_label: "Quorum (N)",
      parallel_quorum_placeholder: "required",
      branch_dependency_label: "depends_on mode",
      repeat_panel_title: "Repeat until",
      repeat_panel_sub: "Loop the body, then evaluate the condition after each iteration",
      repeat_loop_id_label: "loop_id",
      repeat_loop_id_placeholder: "quality_loop",
      repeat_condition_title: "Until condition",
      repeat_condition_intro: "Evaluated on a body member's structured output after each pass. The loop exits when it holds.",
      repeat_empty_body_hint: "Add a member step inside the loop first — the condition reads its output schema.",
      repeat_member_placeholder_label: "— member —",
      repeat_condition_field_placeholder: "— field —",
      repeat_condition_no_schema_label: "(no schema)",
      repeat_preview_label: "until",
      repeat_preview_fallback: "…",
      repeat_iteration_input_label: "Iteration input — what each pass receives",
      repeat_max_iterations_label: "max_iterations",
      repeat_max_iterations_placeholder: "required",
      repeat_tips: [
        "The body is its own FrameSpec — add member steps inside the loop.",
        "The condition reads a member's typed output (e.g. reviewer.verdict == green).",
        "max_iterations bounds the loop so it always terminates.",
      ],
      repeat_canvas_while_label: "while",
      repeat_canvas_not_label: "not",
      repeat_canvas_missing_max_iterations_label: "missing max_iterations",
      repeat_canvas_max_iterations_prefix: "max ",
      repeat_canvas_loop_back_prefix: "↑ loop back · ",
      repeat_canvas_exit_prefix: "↓ exit when ",
      repeat_canvas_exit_fallback: "condition met",
      repeat_iteration_runtime_default_label: "runtime default",
      repeat_iteration_carry_label: "carries last output",
      repeat_iteration_reuse_unsupported_label: "unsupported: re-use input task",
      repeat_iteration_feeds_unsupported_prefix: "unsupported: feeds ",
      repeat_iteration_unsupported_prefix: "unsupported: ",
      add_step_title: "Add step",
      input_step_card_title: "Input",
      input_step_card_desc_fallback: "the task this mob is run with",
      branch_step_card_title: "Branch",
      branch_step_card_desc: "Mob picks the first matching path",
      parallel_step_card_title: "Parallel",
      parallel_step_card_desc_prefix: "fan-out → join · ",
      parallel_step_card_collection_fallback: "—",
      repeat_step_card_title: "Repeat until",
      repeat_step_card_desc_prefix: "until ",
      repeat_step_card_desc_fallback: "loop body until condition",
      member_step_card_title_fallback: "Select member",
      picker_kickoff_title: "Input",
      picker_kickoff_sub: "Every mob run starts from a single task input",
      picker_kickoff_hint: "This node is the mob's ingress — the task it's deployed/run with. Select it on the canvas to edit the task and any typed input fields.",
      picker_title: "Add step",
      picker_sub: "A flow node — a member turn or a flow primitive",
      picker_search_icon: "⌕",
      picker_search_placeholder: "Search members & primitives…",
      picker_members_label: "Mob members",
      picker_flow_label: "Flow",
      picker_empty_members_hint: "No members yet — define some in the Agents tab.",
      picker_new_badge_label: "NEW",
      flow_primitive_rows: [
        {
          id: "repeat",
          glyph: "↻",
          tint: "member",
          label: "Repeat until",
          sub: "Loop a body of steps until a condition holds (max_iterations)",
        },
        {
          id: "branch",
          glyph: "⑂",
          tint: "member",
          label: "Branch",
          sub: "Pick one downstream path by condition (first match wins)",
        },
        {
          id: "parallel",
          glyph: "‖",
          tint: "member",
          label: "Parallel",
          sub: "fan_out to several members, then fan_in with a collection policy",
        },
      ],
    },
    editor_graph_view: {
      zoom_out_title: "Zoom out",
      fit_title: "Fit to view",
      zoom_in_title: "Zoom in",
      port_drag_title: "Drag to a member to connect",
      add_node_search_icon: "⌕",
      add_node_search_placeholder: "Add a node…",
      add_node_close_label: "✕",
      add_node_close_title: "Close",
      add_node_agents_label: "Agents",
      add_node_controls_label: "Flow controls",
      add_node_empty_prefix: "No matches for “",
      add_node_empty_suffix: "”",
      add_node_jump_label: "+ New agent in Agents →",
      gate_palette_rows: [
        { id: "branch", glyph: "⑂", label: "Branch gate", meta: "conditional split" },
        { id: "fork", glyph: "‖", label: "Parallel fork", meta: "fan_out lanes" },
        { id: "join", glyph: "⋈", label: "Join gate", meta: "fan_in barrier" },
      ],
      graph_gate_kind_labels: {
        branch: "branch — conditional split",
        fork: "fork — fan out in parallel",
        join: "join — wait for branches",
      },
      graph_terminal_kind_labels: {
        success: "success — done",
        failed: "failed — blocked",
        human: "human — needs human",
      },
      graph_frame_kind_labels: {
        Branch: "Branch — conditional flow frame",
        Parallel: "Parallel — concurrent flow frame",
        RepeatUntil: "RepeatUntil — bounded loop frame",
      },
      graph_edge_kind_labels: {
        next: "next — sequential handoff",
        fanout: "fanout — parallel sibling",
        cond: "cond — guarded branch",
      },
      inspector_delete_label: "DELETE",
      inspector_label_title: "LABEL",
      inspector_kind_title: "KIND",
      inspector_runtime_default_label: "runtime default",
      gate_collection_title: "COLLECTION POLICY",
      gate_join_member_label: "Join member",
      gate_join_member_placeholder: "— select member —",
      gate_join_member_hint: "MobKit uses this real profile to resolve non-all fan-in.",
      gate_dispatch_title: "DISPATCH MODE",
      gate_dispatch_hint: "Exports as the MobKit parallel flow dispatch mode.",
      gate_conditions_title: "CONDITIONS",
      gate_empty_branch_hint: "add outgoing edges, then configure each as a typed condition or fallback",
      gate_wiring_title: "WIRING",
      gate_incoming_label: "incoming",
      gate_outgoing_label: "outgoing",
      branch_condition_mode_condition_label: "condition",
      branch_condition_mode_fallback_label: "fallback",
      branch_condition_target_prefix: "→",
      branch_input_param_source_label: "Input params",
      source_file_label: "mob.toml",
      source_file_aria_label: "Open mob.toml read-only source editor",
      source_file_glyph: "{ }",
      source_file_role_label: "source file",
      branch_condition_field_placeholder: "— field —",
      branch_condition_no_options_hint: "add input params or an upstream schema field for this condition",
      edge_condition_title: "CONDITION",
      edge_no_condition_options_hint: "Add an upstream agent with an output schema before configuring this edge.",
      edge_owner_placeholder: "— member —",
      edge_from_title: "FROM",
      edge_to_title: "TO",
      edge_row_instance_label: "instance",
      edge_row_member_label: "member",
      edge_row_schema_label: "schema",
      edge_row_missing_value: "—",
      edge_terminal_member_value: "(terminal)",
    },
    editor_graph_template_view: {
      template_eyebrow: "TEMPLATE",
      summary_title: "SUMMARY",
      triggers_title: "TRIGGERS",
      trigger_labels_label: "labels",
      trigger_default_label: "default",
      default_yes_label: "yes",
      default_no_label: "no",
      quick_start_title: "QUICK START",
      quick_start_rows: [
        [
          { kind: "text", text: "Click a " },
          { kind: "strong", text: "library member" },
          { kind: "text", text: " on the left to edit it." },
        ],
        [
          { kind: "text", text: "Click an " },
          { kind: "strong", text: "empty grid cell" },
          { kind: "text", text: " to place a member." },
        ],
        [
          { kind: "text", text: "Drag a node's " },
          { kind: "strong", text: "right port" },
          { kind: "text", text: " to wire it to another." },
        ],
        [{ kind: "text", text: "⌫ deletes the selected instance or edge." }],
      ],
    },
  },
  models: [{ id: "openai/gpt-5.5", label: "GPT-5.5", vendor: "openai" }],
  tool_catalog: [{ id: "builtins", label: "builtins", desc: "Built-ins", kind: "runtime", source: "meerkat_mob::ToolConfig" }],
  skill_realms: [{ id: "mobkit/sample-mobpacks", source: "mobkit/sample-mobpack", skills: [{ id: "mob.workpad" }] }],
  agent_definitions: [{
    id: "sample_reviewer",
    name: "Reviewer",
    role: "reviewer",
    definitionType: "mobkit/profile-member",
    source: "mobkit/sample-mobpack",
    model: "gpt-5.5",
    profileBinding: "inline",
    runtimeMode: "turn_driven",
  }],
  blank_mobpack: {
    id: "blank",
    name: "Blank",
    version: "mobpack/v1",
    stage: "valid",
    trigger: "label · small-fix",
    source: "mobkit/blank-mobpack",
    document: {
      name: "Blank",
      mob_id: "blank",
      flow: { name: "Blank", steps: [{ id: "work", type: "member", role: "m_worker" }] },
      members: [{ id: "m_worker", role: "worker" }],
    },
  },
};
const hydratedCatalogs = controller.mobKitCatalogsFromSchema(hydratedContractAndCatalogFixture, catalogBoot, hydratedContractAndCatalogFixture);
assert.equal(hydratedCatalogs.contractMeta.loaded, true);
assert.equal(hydratedCatalogs.contractMeta.schemaVersion, "mobpack/v1");
assert.equal(hydratedCatalogs.deployDefaults.command, "rkat mob deploy");
assert.equal(hydratedCatalogs.mobDefinition.runtime_modes[0], "turn_driven");
assert.deepEqual(hydratedCatalogs.models.map((model) => model.id), ["openai/gpt-5.5"]);
assert.deepEqual(hydratedCatalogs.toolCatalog.map((tool) => tool.id), ["builtins"]);
assert.deepEqual(hydratedCatalogs.skillRealms.map((realm) => realm.id), ["mobkit/sample-mobpacks"]);
assert.deepEqual(hydratedCatalogs.agentDefinitions.map((definition) => definition.id), ["sample_reviewer"]);
assert.deepEqual(hydratedCatalogs.newFlowView, TEST_NEW_FLOW_VIEW);
assert.deepEqual(hydratedCatalogs.flowRegistryView, TEST_FLOW_REGISTRY_VIEW);
assert.deepEqual(hydratedCatalogs.sourceView, {
  drawerEyebrow: "SOURCE · mob.toml",
  inlineTitle: "mob.toml",
  loadingText: "rendering mob.toml from mobkit/mobpacks/export...",
  copyLabel: "copy",
  closeLabel: "×",
});
assert.deepEqual(hydratedCatalogs.conditionView, TEST_CONDITION_VIEW);
assert.deepEqual(hydratedCatalogs.errorView, TEST_ERROR_VIEW);
assert.deepEqual(hydratedCatalogs.agentView, {
  agentsHeading: "AGENTS",
  schemasHeading: "SCHEMAS",
  addSchemaLabel: "+ new schema",
  addAgentTitle: "Create an agent from a MobKit profile-member definition.",
  addAgentUnavailableTitle: "MobKit schema contract has not provided agent definitions yet.",
  addAgentUnavailableLabel: "agents unavailable",
  addAgentPlaceholderLabel: "+ new agent...",
  emptyTitle: "AGENT LIBRARY",
  emptyLines: [
    "Select an agent or schema on the left.",
    "Agents are reusable across topologies. Edit one here and every placement updates.",
  ],
  missingSchemaLabel: "Schema not found.",
  missingAgentLabel: "Agent not found.",
});
assert.deepEqual(hydratedCatalogs.agentDetailView, {
  usedInLabel: "used in",
  instanceSingular: "instance",
  instancePlural: "instances",
  deleteLabel: "DELETE",
  deleteConfirmIntro: "Delete agent",
  deleteConfirmPlacedPrefix: "It is placed in",
  cellSingular: "cell",
  cellPlural: "cells",
  deleteConfirmCellsSuffix: "those nodes will be removed.",
  usageTitlePrefix: "USED IN",
  emptyUsageHint: "Not yet placed in any cell. Switch to Topology to add.",
  identityTitle: "IDENTITY",
  profileBindingLabel: "Profile binding",
  missingProfileBindingLabel: "missing profile binding",
  realmProfileLabel: "Realm profile",
  realmProfilePlaceholder: "realm profile id",
  realmProfileImportHintFallback: "Realm profile refs are import-only for this editor. Mobpack archives must use inline profiles before validation/export.",
  realmProfileTitle: "REALM PROFILE",
  realmProfileReferenceHintBefore: "This imported member references",
  realmProfileReferenceHintAfterFallback: "from a target realm. Convert it to an inline profile before validating or exporting a deployable mobpack.",
  modelLabel: "Model",
  runtimeModeLabel: "Runtime mode",
  missingRuntimeModeLabel: "missing runtime mode",
  backendLabel: "Backend",
  backendDefinitionDefaultLabel: "definition default",
  inlinePeerNotificationsLabel: "Inline peer notifications",
  inlinePeerNotificationsPlaceholder: "runtime default",
  providerParamsLabel: "Provider params",
  providerParamsPlaceholder: '{"thinking_budget":4096}',
  providerParamsRows: 4,
  providerParamsInvalidJsonLabel: "invalid JSON",
  providerParamsObjectRequiredError: "provider_params must be a JSON object",
  systemPromptTitle: "SYSTEM PROMPT",
  applySkeletonLabel: "APPLY SKELETON",
  applySkeletonTitle: "Apply a MobKit profile prompt skeleton",
  systemPromptPlaceholder: "Describe the member mandate. This text is exported as the profile peer_description.",
  outputSchemaTitle: "OUTPUT SCHEMA",
  schemaNoneLabel: "— none —",
  schemaRequiredLabel: "req",
  editSchemaLabel: "Edit schema →",
  emptySchemaHint: "No structured output. Agent returns free-form text.",
});
assert.deepEqual(hydratedCatalogs.agentAccessView, TEST_AGENT_ACCESS_VIEW);
assert.deepEqual(hydratedCatalogs.deployView, TEST_DEPLOY_VIEW);
assert.deepEqual(hydratedCatalogs.settingsView, TEST_SETTINGS_VIEW);
assert.deepEqual(hydratedCatalogs.schemaView, {
  eyebrow: "OUTPUT SCHEMA",
  descriptionTitle: "DESCRIPTION",
  descriptionPlaceholder: "What is this artifact and when is it emitted?",
  fieldsTitlePrefix: "FIELDS",
  addFieldLabel: "+ field",
  headerLabels: {
    name: "NAME",
    type: "TYPE",
    required: "REQ",
    description: "DESCRIPTION",
    action: "",
  },
  emptyFieldsHint: "No fields yet. Click + field to start.",
  usedByPrefix: "USED BY",
  emptyUsedByHint: "Not yet referenced by any agent.",
  deleteLabel: "DELETE",
  deleteBlockedTitle: "Unassign from agents first",
  fieldNamePlaceholder: "field_name",
  fieldDescriptionPlaceholder: "—",
  fieldRemoveTitle: "Remove field",
  fieldEnumLabel: "VALUES",
  fieldEnumAddLabel: "+ value",
  fieldEnumAddValue: "value",
});
assert.deepEqual(hydratedCatalogs.basicView, {
  startLabel: "START",
  loopBadge: "LOOP",
  tipsTitle: "Tips",
  emptyPanelTitle: "Build your mob flow",
  emptyPanelSubtitleParts: [
    { key: "text-0", kind: "text", text: "Pick a node to configure, or press " },
    { key: "strong-1", kind: "strong", text: "+" },
    { key: "text-2", kind: "text", text: " to add a member turn or flow primitive. The result is a " },
    { key: "code-3", kind: "code", text: "mob.toml" },
    { key: "text-4", kind: "text", text: " flow." },
  ],
  memberStepPanelTitleFallback: "Member step",
  memberStepPanelSubFallback: "Assign a member to run this step",
  memberStepMemberLabel: "Member (profile)",
  memberStepMemberPlaceholder: "— select member —",
  memberStepRuntimeDefaultLabel: "runtime default",
  memberStepInstructionLabel: "message — instruction for this turn",
  memberStepInstructionPlaceholder: "e.g. Run the focused tests and report failures.",
  memberStepDispatchLabel: "Dispatch mode",
  memberStepCollectionLabel: "Collection policy",
  memberStepQuorumLabel: "Quorum",
  memberStepQuorumPlaceholder: "required",
  memberStepTimeoutLabel: "Timeout (ms)",
  memberStepDependencyLabel: "depends_on mode",
  memberStepOutputFormatLabel: "Output format",
  memberStepAllowedToolsLabel: "Allowed tools",
  memberStepAllowedToolsEmptyLabel: "Runtime profile default",
  memberStepBlockedToolsLabel: "Blocked tools",
  memberStepBlockedToolsEmptyLabel: "No step-level blocks",
  memberStepSchemaHintPrefix: "Emits ",
  memberStepSchemaHintToolsPrefix: " · tools: ",
  memberStepSchemaHintEmptyToolsLabel: "—",
  toolScopeNotInCatalogReason: "not in MobKit tool catalog",
  toolScopeNotEnabledReason: "not enabled on profile",
  toolScopeToolDescriptionFallback: "MobKit tool",
  toolScopeRemoveLabel: "×",
  toolScopeSelectMemberPlaceholder: "select a member first",
  toolScopeBlockCatalogPlaceholder: "+ block MobKit tool...",
  toolScopeAddProfilePlaceholder: "+ add profile tool...",
  inputPanelIcon: "▤",
  inputPanelTitle: "Input",
  inputPanelSub: "The task this mob is run with — its ingress",
  inputTaskLabel: "Task",
  inputTaskPlaceholder: "e.g. Fix the issue described below.",
  inputParamsTitlePrefix: "INPUT PARAMS",
  inputAddParamLabel: "+ param",
  inputParamSourceLabel: "Input params",
  inputParamHeaderLabels: {
    name: "NAME",
    type: "TYPE",
    required: "REQ",
    description: "DESCRIPTION",
    action: "",
  },
  inputParamNamePlaceholder: "param_name",
  inputParamDescriptionPlaceholder: "—",
  inputParamRemoveTitle: "Remove param",
  inputParamEnumLabel: "VALUES",
  inputParamEnumAddLabel: "+ value",
  inputParamEnumAddValue: "value",
  inputEmptyParamsParts: [
    { key: "prefix", kind: "text", text: "No params yet. Add one before branching on " },
    { key: "ref", kind: "code", text: "params.*" },
    { key: "suffix", kind: "text", text: "." },
  ],
  inputTips: [
    "Run with: rkat mob deploy <pack> \"<task>\" — or run_flow(input).",
    "Typed fields become the input schema the run is validated against.",
    "Event sources & schedules live outside the mobpack (e.g. fugue).",
  ],
  branchPanelTitle: "Branch",
  branchPanelSub: "Choose one downstream path by condition",
  parallelPanelTitle: "Parallel",
  parallelPanelSub: "fan_out to members, then fan_in and collect",
  branchRouteMemberLabel: "Route member",
  parallelJoinMemberLabel: "Join member",
  branchControllerPlaceholderLabel: "— direct MobKit lanes —",
  branchEmptyControllerHint: "Without a selected profile, MobKit conditions/parallel lanes attach directly to the first real member in each lane.",
  branchConditionTitle: "Branch conditions",
  branchConditionIntro: "Read in order; the first match wins. Conditions read a member's structured output.",
  branchConditionRowTitlePrefix: "Branch",
  branchConditionEmptyHint: "Add an upstream member with an output schema before configuring this branch.",
  branchConditionSourcePlaceholder: "— source —",
  branchConditionFieldPlaceholder: "— field —",
  branchConditionNoSchemaLabel: "(no schema)",
  branchConditionPreviewPrefix: "when",
  branchConditionPreviewFallback: "…",
  branchFallbackTitle: "Fallback",
  branchFallbackHint: "If none match, the flow follows the fallback path; else it stops.",
  addBranchLabel: "+ Add branch",
  addParallelBranchLabel: "+ Add parallel branch",
  parallelDispatchLabel: "Dispatch mode",
  parallelCollectionLabel: "Collection policy (fan_in)",
  parallelQuorumLabel: "Quorum (N)",
  parallelQuorumPlaceholder: "required",
  branchDependencyLabel: "depends_on mode",
  repeatPanelTitle: "Repeat until",
  repeatPanelSub: "Loop the body, then evaluate the condition after each iteration",
  repeatLoopIdLabel: "loop_id",
  repeatLoopIdPlaceholder: "quality_loop",
  repeatConditionTitle: "Until condition",
  repeatConditionIntro: "Evaluated on a body member's structured output after each pass. The loop exits when it holds.",
  repeatEmptyBodyHint: "Add a member step inside the loop first — the condition reads its output schema.",
  repeatMemberPlaceholderLabel: "— member —",
  repeatConditionFieldPlaceholder: "— field —",
  repeatConditionNoSchemaLabel: "(no schema)",
  repeatPreviewLabel: "until",
  repeatPreviewFallback: "…",
  repeatIterationInputLabel: "Iteration input — what each pass receives",
  repeatMaxIterationsLabel: "max_iterations",
  repeatMaxIterationsPlaceholder: "required",
  repeatTips: [
    "The body is its own FrameSpec — add member steps inside the loop.",
    "The condition reads a member's typed output (e.g. reviewer.verdict == green).",
    "max_iterations bounds the loop so it always terminates.",
  ],
  repeatCanvasWhileLabel: "while",
  repeatCanvasNotLabel: "not",
  repeatCanvasMissingMaxIterationsLabel: "missing max_iterations",
  repeatCanvasMaxIterationsPrefix: "max ",
  repeatCanvasLoopBackPrefix: "↑ loop back · ",
  repeatCanvasExitPrefix: "↓ exit when ",
  repeatCanvasExitFallback: "condition met",
  repeatIterationRuntimeDefaultLabel: "runtime default",
  repeatIterationCarryLabel: "carries last output",
  repeatIterationReuseUnsupportedLabel: "unsupported: re-use input task",
  repeatIterationFeedsUnsupportedPrefix: "unsupported: feeds ",
  repeatIterationUnsupportedPrefix: "unsupported: ",
  addStepTitle: "Add step",
  inputStepCardTitle: "Input",
  inputStepCardDescFallback: "the task this mob is run with",
  branchStepCardTitle: "Branch",
  branchStepCardDesc: "Mob picks the first matching path",
  parallelStepCardTitle: "Parallel",
  parallelStepCardDescPrefix: "fan-out → join · ",
  parallelStepCardCollectionFallback: "—",
  repeatStepCardTitle: "Repeat until",
  repeatStepCardDescPrefix: "until ",
  repeatStepCardDescFallback: "loop body until condition",
  memberStepCardTitleFallback: "Select member",
  pickerKickoffTitle: "Input",
  pickerKickoffSub: "Every mob run starts from a single task input",
  pickerKickoffHint: "This node is the mob's ingress — the task it's deployed/run with. Select it on the canvas to edit the task and any typed input fields.",
  pickerTitle: "Add step",
  pickerSub: "A flow node — a member turn or a flow primitive",
  pickerSearchIcon: "⌕",
  pickerSearchPlaceholder: "Search members & primitives…",
  pickerMembersLabel: "Mob members",
  pickerFlowLabel: "Flow",
  pickerEmptyMembersHint: "No members yet — define some in the Agents tab.",
  pickerNewBadgeLabel: "NEW",
  flowPrimitiveRows: [
    {
      id: "repeat",
      glyph: "↻",
      tint: "member",
      label: "Repeat until",
      sub: "Loop a body of steps until a condition holds (max_iterations)",
      isNew: false,
    },
    {
      id: "branch",
      glyph: "⑂",
      tint: "member",
      label: "Branch",
      sub: "Pick one downstream path by condition (first match wins)",
      isNew: false,
    },
    {
      id: "parallel",
      glyph: "‖",
      tint: "member",
      label: "Parallel",
      sub: "fan_out to several members, then fan_in with a collection policy",
      isNew: false,
    },
  ],
});
assert.deepEqual(controller.basicEditorViewState(hydratedCatalogs.basicView), hydratedCatalogs.basicView);
assert.deepEqual(controller.basicEditorViewState(null), {
  startLabel: "",
  loopBadge: "",
  tipsTitle: "",
  emptyPanelTitle: "",
  emptyPanelSubtitleParts: [],
  memberStepPanelTitleFallback: "",
  memberStepPanelSubFallback: "",
  memberStepMemberLabel: "",
  memberStepMemberPlaceholder: "",
  memberStepRuntimeDefaultLabel: "",
  memberStepInstructionLabel: "",
  memberStepInstructionPlaceholder: "",
  memberStepDispatchLabel: "",
  memberStepCollectionLabel: "",
  memberStepQuorumLabel: "",
  memberStepQuorumPlaceholder: "",
  memberStepTimeoutLabel: "",
  memberStepDependencyLabel: "",
  memberStepOutputFormatLabel: "",
  memberStepAllowedToolsLabel: "",
  memberStepAllowedToolsEmptyLabel: "",
  memberStepBlockedToolsLabel: "",
  memberStepBlockedToolsEmptyLabel: "",
  memberStepSchemaHintPrefix: "",
  memberStepSchemaHintToolsPrefix: "",
  memberStepSchemaHintEmptyToolsLabel: "",
  toolScopeNotInCatalogReason: "",
  toolScopeNotEnabledReason: "",
  toolScopeToolDescriptionFallback: "",
  toolScopeRemoveLabel: "",
  toolScopeSelectMemberPlaceholder: "",
  toolScopeBlockCatalogPlaceholder: "",
  toolScopeAddProfilePlaceholder: "",
  inputPanelIcon: "",
  inputPanelTitle: "",
  inputPanelSub: "",
  inputTaskLabel: "",
  inputTaskPlaceholder: "",
  inputParamsTitlePrefix: "",
  inputAddParamLabel: "",
  inputParamSourceLabel: "",
  inputParamHeaderLabels: {
    name: "",
    type: "",
    required: "",
    description: "",
    action: "",
  },
  inputParamNamePlaceholder: "",
  inputParamDescriptionPlaceholder: "",
  inputParamRemoveTitle: "",
  inputParamEnumLabel: "",
  inputParamEnumAddLabel: "",
  inputParamEnumAddValue: "",
  inputEmptyParamsParts: [],
  inputTips: [],
  branchPanelTitle: "",
  branchPanelSub: "",
  parallelPanelTitle: "",
  parallelPanelSub: "",
  branchRouteMemberLabel: "",
  parallelJoinMemberLabel: "",
  branchControllerPlaceholderLabel: "",
  branchEmptyControllerHint: "",
  branchConditionTitle: "",
  branchConditionIntro: "",
  branchConditionRowTitlePrefix: "",
  branchConditionEmptyHint: "",
  branchConditionSourcePlaceholder: "",
  branchConditionFieldPlaceholder: "",
  branchConditionNoSchemaLabel: "",
  branchConditionPreviewPrefix: "",
  branchConditionPreviewFallback: "",
  branchFallbackTitle: "",
  branchFallbackHint: "",
  addBranchLabel: "",
  addParallelBranchLabel: "",
  parallelDispatchLabel: "",
  parallelCollectionLabel: "",
  parallelQuorumLabel: "",
  parallelQuorumPlaceholder: "",
  branchDependencyLabel: "",
  repeatPanelTitle: "",
  repeatPanelSub: "",
  repeatLoopIdLabel: "",
  repeatLoopIdPlaceholder: "",
  repeatConditionTitle: "",
  repeatConditionIntro: "",
  repeatEmptyBodyHint: "",
  repeatMemberPlaceholderLabel: "",
  repeatConditionFieldPlaceholder: "",
  repeatConditionNoSchemaLabel: "",
  repeatPreviewLabel: "",
  repeatPreviewFallback: "",
  repeatIterationInputLabel: "",
  repeatMaxIterationsLabel: "",
  repeatMaxIterationsPlaceholder: "",
  repeatTips: [],
  repeatCanvasWhileLabel: "",
  repeatCanvasNotLabel: "",
  repeatCanvasMissingMaxIterationsLabel: "",
  repeatCanvasMaxIterationsPrefix: "",
  repeatCanvasLoopBackPrefix: "",
  repeatCanvasExitPrefix: "",
  repeatCanvasExitFallback: "",
  repeatIterationRuntimeDefaultLabel: "",
  repeatIterationCarryLabel: "",
  repeatIterationReuseUnsupportedLabel: "",
  repeatIterationFeedsUnsupportedPrefix: "",
  repeatIterationUnsupportedPrefix: "",
  addStepTitle: "",
  inputStepCardTitle: "",
  inputStepCardDescFallback: "",
  branchStepCardTitle: "",
  branchStepCardDesc: "",
  parallelStepCardTitle: "",
  parallelStepCardDescPrefix: "",
  parallelStepCardCollectionFallback: "",
  repeatStepCardTitle: "",
  repeatStepCardDescPrefix: "",
  repeatStepCardDescFallback: "",
  memberStepCardTitleFallback: "",
  pickerKickoffTitle: "",
  pickerKickoffSub: "",
  pickerKickoffHint: "",
  pickerTitle: "",
  pickerSub: "",
  pickerSearchIcon: "",
  pickerSearchPlaceholder: "",
  pickerMembersLabel: "",
  pickerFlowLabel: "",
  pickerEmptyMembersHint: "",
  pickerNewBadgeLabel: "",
  flowPrimitiveRows: [],
});
assert.deepEqual(hydratedCatalogs.launchView, TEST_LAUNCH_VIEW);
assert.deepEqual(hydratedCatalogs.graphView, {
  zoomOutTitle: "Zoom out",
  fitTitle: "Fit to view",
  zoomInTitle: "Zoom in",
  portDragTitle: "Drag to a member to connect",
  addNodeSearchIcon: "⌕",
  addNodeSearchPlaceholder: "Add a node…",
  addNodeCloseLabel: "✕",
  addNodeCloseTitle: "Close",
  addNodeAgentsLabel: "Agents",
  addNodeControlsLabel: "Flow controls",
  addNodeEmptyPrefix: "No matches for “",
  addNodeEmptySuffix: "”",
  addNodeJumpLabel: "+ New agent in Agents →",
  gatePaletteRows: [
    { id: "branch", glyph: "⑂", label: "Branch gate", meta: "conditional split" },
    { id: "fork", glyph: "‖", label: "Parallel fork", meta: "fan_out lanes" },
    { id: "join", glyph: "⋈", label: "Join gate", meta: "fan_in barrier" },
  ],
  gateKindLabels: {
    branch: "branch — conditional split",
    fork: "fork — fan out in parallel",
    join: "join — wait for branches",
  },
  terminalKindLabels: {
    success: "success — done",
    failed: "failed — blocked",
    human: "human — needs human",
  },
  frameKindLabels: {
    Branch: "Branch — conditional flow frame",
    Parallel: "Parallel — concurrent flow frame",
    RepeatUntil: "RepeatUntil — bounded loop frame",
  },
  edgeKindLabels: {
    next: "next — sequential handoff",
    fanout: "fanout — parallel sibling",
    cond: "cond — guarded branch",
  },
  inspectorDeleteLabel: "DELETE",
  inspectorLabelTitle: "LABEL",
  inspectorKindTitle: "KIND",
  inspectorRuntimeDefaultLabel: "runtime default",
  gateCollectionTitle: "COLLECTION POLICY",
  gateJoinMemberLabel: "Join member",
  gateJoinMemberPlaceholder: "— select member —",
  gateJoinMemberHint: "MobKit uses this real profile to resolve non-all fan-in.",
  gateDispatchTitle: "DISPATCH MODE",
  gateDispatchHint: "Exports as the MobKit parallel flow dispatch mode.",
  gateConditionsTitle: "CONDITIONS",
  gateEmptyBranchHint: "add outgoing edges, then configure each as a typed condition or fallback",
  gateWiringTitle: "WIRING",
  gateIncomingLabel: "incoming",
  gateOutgoingLabel: "outgoing",
  branchConditionModeConditionLabel: "condition",
  branchConditionModeFallbackLabel: "fallback",
  branchConditionTargetPrefix: "→",
  graphInputParamSourceLabel: "Input params",
  sourceFileLabel: "mob.toml",
  sourceFileAriaLabel: "Open mob.toml read-only source editor",
  sourceFileGlyph: "{ }",
  sourceFileRoleLabel: "source file",
  branchConditionFieldPlaceholder: "— field —",
  branchConditionNoOptionsHint: "add input params or an upstream schema field for this condition",
  edgeConditionTitle: "CONDITION",
  edgeNoConditionOptionsHint: "Add an upstream agent with an output schema before configuring this edge.",
  edgeOwnerPlaceholder: "— member —",
  edgeFromTitle: "FROM",
  edgeToTitle: "TO",
  edgeRowInstanceLabel: "instance",
  edgeRowMemberLabel: "member",
  edgeRowSchemaLabel: "schema",
  edgeRowMissingValue: "—",
  edgeTerminalMemberValue: "(terminal)",
});
assert.deepEqual(controller.graphCanvasViewState(hydratedCatalogs.graphView), hydratedCatalogs.graphView);
assert.deepEqual(controller.graphCanvasViewState(null), {
  zoomOutTitle: "",
  fitTitle: "",
  zoomInTitle: "",
  portDragTitle: "",
  addNodeSearchIcon: "",
  addNodeSearchPlaceholder: "",
  addNodeCloseLabel: "",
  addNodeCloseTitle: "",
  addNodeAgentsLabel: "",
  addNodeControlsLabel: "",
  addNodeEmptyPrefix: "",
  addNodeEmptySuffix: "",
  addNodeJumpLabel: "",
  gatePaletteRows: [],
  gateKindLabels: {},
  terminalKindLabels: {},
  frameKindLabels: {},
  edgeKindLabels: {},
  inspectorDeleteLabel: "",
  inspectorLabelTitle: "",
  inspectorKindTitle: "",
  inspectorRuntimeDefaultLabel: "",
  gateCollectionTitle: "",
  gateJoinMemberLabel: "",
  gateJoinMemberPlaceholder: "",
  gateJoinMemberHint: "",
  gateDispatchTitle: "",
  gateDispatchHint: "",
  gateConditionsTitle: "",
  gateEmptyBranchHint: "",
  gateWiringTitle: "",
  gateIncomingLabel: "",
  gateOutgoingLabel: "",
  branchConditionModeConditionLabel: "",
  branchConditionModeFallbackLabel: "",
  branchConditionTargetPrefix: "",
  graphInputParamSourceLabel: "",
  sourceFileLabel: "",
  sourceFileAriaLabel: "",
  sourceFileGlyph: "",
  sourceFileRoleLabel: "",
  branchConditionFieldPlaceholder: "",
  branchConditionNoOptionsHint: "",
  edgeConditionTitle: "",
  edgeNoConditionOptionsHint: "",
  edgeOwnerPlaceholder: "",
  edgeFromTitle: "",
  edgeToTitle: "",
  edgeRowInstanceLabel: "",
  edgeRowMemberLabel: "",
  edgeRowSchemaLabel: "",
  edgeRowMissingValue: "",
  edgeTerminalMemberValue: "",
});
assert.deepEqual(hydratedCatalogs.graphTemplateView, {
  templateEyebrow: "TEMPLATE",
  summaryTitle: "SUMMARY",
  triggersTitle: "TRIGGERS",
  triggerLabelsLabel: "labels",
  triggerDefaultLabel: "default",
  defaultYesLabel: "yes",
  defaultNoLabel: "no",
  quickStartTitle: "QUICK START",
  quickStartRows: [
    {
      key: "quick-start-0",
      parts: [
        { key: "text-0", kind: "text", text: "Click a " },
        { key: "strong-1", kind: "strong", text: "library member" },
        { key: "text-2", kind: "text", text: " on the left to edit it." },
      ],
    },
    {
      key: "quick-start-1",
      parts: [
        { key: "text-0", kind: "text", text: "Click an " },
        { key: "strong-1", kind: "strong", text: "empty grid cell" },
        { key: "text-2", kind: "text", text: " to place a member." },
      ],
    },
    {
      key: "quick-start-2",
      parts: [
        { key: "text-0", kind: "text", text: "Drag a node's " },
        { key: "strong-1", kind: "strong", text: "right port" },
        { key: "text-2", kind: "text", text: " to wire it to another." },
      ],
    },
    {
      key: "quick-start-3",
      parts: [{ key: "text-0", kind: "text", text: "⌫ deletes the selected instance or edge." }],
    },
  ],
});
assert.equal(hydratedCatalogs.grid, catalogBoot.grid);
assert.deepEqual(hydratedCatalogs.template, {
  name: "Blank",
  repo: "mobkit/blank-mobpack",
  version: "mobpack/v1",
  triggers: { labels: ["label · small-fix"], default: false },
});

assert.deepEqual(controller.mergeSkillRealms([
  { id: "document", default: true, skills: [{ id: "mob.workpad", source: "inline" }, { id: "mob.review", source: "inline" }] },
], [
  { id: "document", default: true, skills: [{ id: "mob.workpad", source: "catalog" }, { id: "mob.tests", source: "path" }] },
  { id: "contract", default: true, skills: [{ id: "mob.review", source: "catalog" }, { id: "mob.docs", source: "inline" }] },
]), [
  { id: "document", default: true, skills: [{ id: "mob.workpad", source: "inline" }, { id: "mob.review", source: "inline" }, { id: "mob.tests", source: "path" }] },
  { id: "contract", default: false, skills: [{ id: "mob.docs", source: "inline" }] },
]);

assert.deepEqual(controller.memberToolAccessPatch(
  { tools: ["builtins"] },
  "shell",
  hydratedCatalogs.toolCatalog,
  hydratedCatalogs.agentAccessView,
), {
  ok: false,
  id: "",
  error: "Use a MobKit-listed runtime tool or configured MCP/Rust source.",
  patch: null,
});
assert.deepEqual(controller.memberToolAccessPatch(
  { tools: ["builtins"] },
  "builtins",
  hydratedCatalogs.toolCatalog,
  hydratedCatalogs.agentAccessView,
), { ok: true, id: "builtins", alreadySelected: true, patch: null });
assert.deepEqual(controller.memberToolAccessPatch(
  { tools: [] },
  "builtins",
  hydratedCatalogs.toolCatalog,
  hydratedCatalogs.agentAccessView,
), { ok: true, id: "builtins", alreadySelected: false, patch: { tools: ["builtins"] } });
assert.deepEqual(controller.memberToolAccessState(
  { tools: ["builtins", "missing"] },
  [
    { id: "builtins", label: "Built-ins", desc: "Built-in runtime tools" },
    { id: "shell", label: "Shell", desc: "Shell tool" },
  ],
  hydratedCatalogs.agentAccessView,
), {
  selectedTools: ["builtins", "missing"],
  title: "TOOL ACCESS",
  hint: "Authority is calculated from this allowlist. Reviewed once here.",
  rows: [
    {
      id: "builtins",
      name: "builtins",
      description: "Built-in runtime tools",
      meta: { id: "builtins", label: "Built-ins", desc: "Built-in runtime tools" },
      className: "tool-row",
      removeLabel: "×",
    },
    {
      id: "missing",
      name: "missing",
      description: "—",
      meta: null,
      className: "tool-row",
      removeLabel: "×",
    },
  ],
  addableRows: [{
    id: "shell",
    value: "shell",
    label: "Shell",
    description: "Shell tool",
    optionLabel: "Shell — Shell tool",
    meta: { id: "shell", label: "Shell", desc: "Shell tool" },
  }],
  addSelectValue: "",
  addSelectPlaceholder: "+ add tool…",
  sourceLabel: "Configured tool source",
  sourcePlaceholder: "choose from MobKit tool catalog",
  addButtonLabel: "ADD",
});
assert.deepEqual(controller.memberToolRemovePatch(
  { tools: ["builtins", "shell", "shell"] },
  "shell",
), { ok: true, id: "shell", patch: { tools: ["builtins"] } });
const toolCascadeResult = controller.memberToolRemoveCascadePatch({
  memberId: "m_tool",
  members: [{ id: "m_tool", tools: ["builtins", "shell"] }],
  flow: {
    name: "tool-cascade-proof",
    steps: [
      {
        id: "tool_step",
        type: "member",
        role: "m_tool",
        allowedTools: ["builtins", "shell"],
        blockedTools: ["shell"],
      },
    ],
  },
  instances: [
    { id: "tool_inst", memberId: "m_tool", allowedTools: ["builtins", "shell"], blockedTools: ["shell"] },
  ],
}, "shell");
assert.equal(toolCascadeResult.ok, true);
assert.deepEqual(toolCascadeResult.members[0].tools, ["builtins"]);
assert.deepEqual(toolCascadeResult.flow.steps[0].allowedTools, ["builtins"]);
assert.deepEqual(toolCascadeResult.flow.steps[0].blockedTools, []);
assert.deepEqual(toolCascadeResult.instances[0].allowedTools, ["builtins"]);
assert.deepEqual(toolCascadeResult.instances[0].blockedTools, []);
const toolCascadeAddResult = controller.memberToolAccessCascadePatch({
  memberId: "m_tool",
  members: [{ id: "m_tool", tools: ["builtins"] }],
  flow: {
    name: "tool-add-cascade-proof",
    steps: [{ id: "tool_step", type: "member", role: "m_tool", allowedTools: ["builtins"] }],
  },
  instances: [{ id: "tool_inst", memberId: "m_tool", allowedTools: ["builtins"] }],
}, "shell", [{ id: "builtins" }, { id: "shell" }], hydratedCatalogs.agentAccessView);
assert.equal(toolCascadeAddResult.ok, true);
assert.deepEqual(toolCascadeAddResult.members[0].tools, ["builtins", "shell"]);
assert.deepEqual(toolCascadeAddResult.flow.steps[0].allowedTools, ["builtins"]);
assert.deepEqual(toolCascadeAddResult.instances[0].allowedTools, ["builtins"]);
assert.deepEqual(controller.memberSkillAccessState({
  member: { skills: ["mob.review", "mob.tests", "mob.missing"] },
  realmId: "docs",
  inlineOpen: true,
  skillRealms: [
    {
      id: "main",
      label: "Main",
      default: true,
      skills: [
        { id: "mob.review", desc: "Review skill" },
        { id: "mob.plan", source: "inline" },
      ],
    },
    {
      id: "docs",
      label: "Docs",
      skills: [
        { id: "mob.tests", path: "skills/tests.md" },
        { id: "mob.docs", source: "path" },
      ],
    },
  ],
  accessView: hydratedCatalogs.agentAccessView,
}), {
  sectionTitle: "SKILLS",
  inlineToggleLabel: "CANCEL",
  hint: "Selected skills are baked into the mobpack. Browse a realm to add more.",
  inlineLabelPlaceholder: "mob.skill-name",
  inlineContentRows: 4,
  inlineContentPlaceholder: "Skill instructions stored as [skills.<id>] content",
  inlineCreateHint: "Creates an inline skill definition in this mobpack.",
  inlineAddLabel: "ADD SKILL",
  inlineErrorFallback: "Could not create inline skill.",
  noRealmsMessage: "MobKit did not provide skill realms for this document.",
  realmLabel: "Realm",
  hasRealms: true,
  realmId: "docs",
  realmOptions: [
    { id: "main", label: "Main · default" },
    { id: "docs", label: "Docs" },
  ],
  skillRows: [
    {
      id: "mob.tests",
      selected: true,
      className: "skill-row is-on",
      checkLabel: "✓",
      name: "mob.tests",
      desc: "skills/tests.md",
      skill: { id: "mob.tests", path: "skills/tests.md" },
    },
    {
      id: "mob.docs",
      selected: false,
      className: "skill-row",
      checkLabel: "",
      name: "mob.docs",
      desc: "path",
      skill: { id: "mob.docs", source: "path" },
    },
  ],
  selectedOutsideRealm: [{
    id: "mob.review",
    realmId: "main",
    realmLabel: "Main",
    className: "skill-chip",
    title: "Main",
    label: "mob.review",
    detail: "Main",
    removeLabel: "×",
  }],
  unavailableSelected: [{
    id: "mob.missing",
    className: "skill-chip is-invalid",
    label: "mob.missing",
    removeLabel: "×",
  }],
  unavailableHeading: "Unavailable in MobKit skill realms:",
  outsideRealmHeading: "Selected from other realms:",
});
assert.deepEqual(controller.memberSkillAccessState({
  member: { skills: ["mob.review"] },
  realmId: "gone",
  skillRealms: [{ id: "main", label: "Main", skills: [{ id: "mob.review" }] }],
}).realmId, "main");
assert.deepEqual(controller.stepToolScopeState({
  member: { tools: ["builtins", "shell", "unknown"] },
  selected: ["builtins", "missing", "builtins"],
  mode: "member",
  toolCatalog: [
    { id: "builtins", label: "Built-ins", desc: "Built-in runtime tools" },
    { id: "shell", label: "Shell", desc: "Shell tool" },
    { id: "git", label: "Git", desc: "Git tool" },
  ],
  basicView: hydratedCatalogs.basicView,
}), {
  selectedTools: ["builtins", "missing"],
  addable: ["shell"],
  addableRows: [{
    id: "shell",
    value: "shell",
    label: "Shell",
    description: "Shell tool",
    optionLabel: "Shell — Shell tool",
    meta: { id: "shell", label: "Shell", desc: "Shell tool" },
  }],
  rows: [
    {
      id: "builtins",
      name: "builtins",
      meta: { id: "builtins", label: "Built-ins", desc: "Built-in runtime tools" },
      unavailable: false,
      reason: "",
      className: "tool-row",
      description: "Built-in runtime tools",
      removeLabel: "×",
    },
    {
      id: "missing",
      name: "missing",
      meta: null,
      unavailable: true,
      reason: "not enabled on profile",
      className: "tool-row tool-row--invalid",
      description: "not enabled on profile",
      removeLabel: "×",
    },
  ],
  addSelectValue: "",
  addSelectPlaceholder: "+ add profile tool...",
  disabled: false,
});
assert.deepEqual(controller.stepToolScopeAddPatch(
  ["builtins"],
  "shell",
  {
    member: { tools: ["builtins", "shell"] },
    mode: "member",
    field: "allowedTools",
    toolCatalog: [{ id: "builtins" }, { id: "shell" }, { id: "git" }],
  },
), { ok: true, id: "shell", patch: { allowedTools: ["builtins", "shell"] } });
assert.deepEqual(controller.stepToolScopeAddPatch(
  ["builtins"],
  "git",
  {
    member: { tools: ["builtins", "shell"] },
    mode: "member",
    field: "allowedTools",
    toolCatalog: [{ id: "builtins" }, { id: "shell" }, { id: "git" }],
  },
), { ok: false, id: "git", patch: null });
assert.deepEqual(controller.stepToolScopeAddPatch(
  [],
  "git",
  {
    member: { tools: ["builtins"] },
    mode: "catalog",
    field: "blockedTools",
    toolCatalog: [{ id: "builtins" }, { id: "shell" }, { id: "git" }],
  },
), { ok: true, id: "git", patch: { blockedTools: ["git"] } });
assert.deepEqual(controller.stepToolScopeRemovePatch(
  ["builtins", "shell", "shell"],
  "shell",
  { field: "allowedTools" },
), { ok: true, id: "shell", patch: { allowedTools: ["builtins"] } });

assert.deepEqual(controller.memberSkillTogglePatch(
  { skills: ["mob.workpad"] },
  "mob.review",
  [{ id: "main", skills: [{ id: "mob.review" }] }],
), { ok: true, id: "mob.review", selected: true, patch: { skills: ["mob.workpad", "mob.review"] } });
assert.deepEqual(controller.memberSkillTogglePatch(
  { skills: ["mob.workpad", "mob.review"] },
  "mob.review",
  [{ id: "main", skills: [{ id: "mob.review" }] }],
), { ok: true, id: "mob.review", selected: false, patch: { skills: ["mob.workpad"] } });
assert.deepEqual(controller.memberSkillTogglePatch(
  { skills: ["mob.workpad"] },
  "mob.fake",
  [{ id: "main", skills: [{ id: "mob.review" }] }],
), { ok: false, id: "mob.fake", patch: null });
assert.deepEqual(controller.memberSkillTogglePatch(
  { skills: ["mob.workpad", "mob.stale"] },
  "mob.stale",
  [{ id: "main", skills: [{ id: "mob.review" }] }],
), { ok: true, id: "mob.stale", selected: false, patch: { skills: ["mob.workpad"] } });
assert.deepEqual(controller.memberSkillRemovePatch(
  { skills: ["mob.workpad", "mob.review", "mob.review"] },
  "mob.review",
), { ok: true, id: "mob.review", patch: { skills: ["mob.workpad"] } });
assert.deepEqual(controller.memberSkillToggleCascadePatch({
  memberId: "m_skill",
  members: [{ id: "m_skill", skills: ["mob.workpad"] }],
  skillRealms: [{ id: "main", skills: [{ id: "mob.review" }] }],
}, "mob.fake"), {
  ok: false,
  id: "mob.fake",
  patch: null,
  members: [{ id: "m_skill", skills: ["mob.workpad"] }],
  skillRealms: [{ id: "main", skills: [{ id: "mob.review" }] }],
});
const skillToggleCascade = controller.memberSkillToggleCascadePatch({
  memberId: "m_skill",
  members: [{ id: "m_skill", skills: ["mob.workpad"] }],
  skillRealms: [{ id: "main", skills: [{ id: "mob.review" }] }],
}, "mob.review");
assert.equal(skillToggleCascade.ok, true);
assert.deepEqual(skillToggleCascade.members[0].skills, ["mob.workpad", "mob.review"]);
const staleSkillRemoveCascade = controller.memberSkillRemoveCascadePatch({
  memberId: "m_skill",
  members: [{ id: "m_skill", skills: ["mob.workpad", "mob.stale", "mob.stale"] }],
  skillRealms: [{ id: "main", skills: [{ id: "mob.workpad" }] }],
}, "mob.stale");
assert.equal(staleSkillRemoveCascade.ok, true);
assert.deepEqual(staleSkillRemoveCascade.members[0].skills, ["mob.workpad"]);
const inlinePatch = controller.memberInlineSkillPatch(
  { skills: ["mob.workpad"] },
  [{ id: "mobkit/sample-mobpacks", skills: [{ id: "mob.workpad" }] }],
  { label: "Quality Gate", content: "Review and emit the QualityVerdict schema." },
  hydratedCatalogs.agentAccessView,
);
assert.equal(inlinePatch.id, "mob.quality.gate");
assert.equal(inlinePatch.realmId, "mobkit/editor-inline");
assert.deepEqual(inlinePatch.patch, { skills: ["mob.workpad", "mob.quality.gate"] });
assert.equal(inlinePatch.skillRealms[0].id, "mobkit/editor-inline");
assert.equal(inlinePatch.skillRealms[0].skills[0].source, "inline");
assert.equal(inlinePatch.skillRealms[0].skills[0].content, "Review and emit the QualityVerdict schema.");
assert.equal(inlinePatch.skillRealms[0].skills[0].desc, "Inline MobKit skill stored in this mobpack.");
const inlineCascade = controller.memberInlineSkillCascadePatch({
  memberId: "m_skill",
  members: [{ id: "m_skill", skills: ["mob.workpad"] }],
  skillRealms: [{ id: "main", skills: [{ id: "mob.workpad" }] }],
}, { label: "Quality Gate", content: "Review and emit the QualityVerdict schema." }, hydratedCatalogs.agentAccessView);
assert.equal(inlineCascade.ok, true);
assert.equal(inlineCascade.id, "mob.quality.gate");
assert.deepEqual(inlineCascade.members[0].skills, ["mob.workpad", "mob.quality.gate"]);
assert.equal(inlineCascade.skillRealms[0].id, "mobkit/editor-inline");
assert.equal(inlineCascade.skillRealms[0].skills[0].source, "inline");
assert.equal(inlineCascade.skillRealms[0].skills[0].content, "Review and emit the QualityVerdict schema.");
assert.throws(
  () => controller.memberInlineSkillPatch(
    { skills: [] },
    [],
    { content: "Do the work." },
    hydratedCatalogs.agentAccessView,
  ),
  new RegExp(hydratedCatalogs.agentAccessView.skillInlineMissingLabelError.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")),
);
assert.throws(
  () => controller.memberInlineSkillPatch(
    { skills: [] },
    [],
    { label: "mob.empty" },
    hydratedCatalogs.agentAccessView,
  ),
  new RegExp(hydratedCatalogs.agentAccessView.skillInlineMissingContentError.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")),
);
assert.throws(
  () => controller.memberInlineSkillPatch(
    { skills: [] },
    [],
    { label: "!!!", content: "Do the work." },
    hydratedCatalogs.agentAccessView,
  ),
  new RegExp(hydratedCatalogs.agentAccessView.skillInlineInvalidIdError.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")),
);

const sampleRows = controller.sampleFlowsFromSchema({
  sample_mobpacks: [
    {
      id: "missing_source",
      name: "Missing source",
      document: { mob_id: "missing_source" },
    },
    {
      source: "mobkit/sample-mobpack",
      document: { mob_id: "" },
    },
    {
      source: "mobkit/sample-mobpack",
      document: { mob_id: "document_id_only", name: "Document Id Only" },
    },
    {
      id: "document_name_only",
      source: "mobkit/sample-mobpack",
      document: { mob_id: "document_name_only", name: "Document Name Only" },
    },
    {
      id: "schema_source",
      name: "Schema source",
      source: "mobkit/sample-mobpack",
      document: { mob_id: "schema_source", schema_version: "0.1" },
      validation: { ok: true },
    },
    {
      id: "schema_source_with_trigger",
      name: "Schema source with trigger",
      source: "mobkit/imported",
      trigger: "imported/mob.toml",
      document: { mob_id: "schema_source_with_trigger" },
    },
  ],
});
assert.deepEqual(sampleRows.map((row) => ({
  id: row.id,
  source: row.source,
  trigger: row.trigger,
  stage: row.stage,
})), [
  {
    id: "schema_source",
    source: "mobkit/sample-mobpack",
    trigger: "mobkit/sample-mobpack",
    stage: "valid",
  },
  {
    id: "schema_source_with_trigger",
    source: "mobkit/imported",
    trigger: "imported/mob.toml",
    stage: "draft",
  },
]);

const bootstrapProjection = controller.flowCatalogBootstrapState({
  sample_mobpacks: [
    {
      id: "starter",
      name: "Starter",
      source: "mobkit/sample-mobpack",
      trigger: "label · starter",
      document: { mob_id: "starter", schema_version: "0.1" },
      validation: { ok: true },
    },
    {
      id: "second",
      name: "Second",
      source: "mobkit/sample-mobpack",
      document: { mob_id: "second", schema_version: "0.1" },
      validation: { ok: false },
    },
  ],
}, {
  openEditor: true,
  deployDefaults: { surface: "local" },
  mobDefaults: { backend: "session" },
});
assert.deepEqual(bootstrapProjection.templates.map((row) => row.id), ["starter", "second"]);
assert.deepEqual(bootstrapProjection.flows.map((row) => row.id), ["starter", "second"]);
assert.equal(bootstrapProjection.initialHydration.result.document.mob_id, "starter");
assert.equal(bootstrapProjection.initialHydration.result.validation.ok, true);
assert.equal(bootstrapProjection.initialHydration.options.id, "starter");
assert.equal(bootstrapProjection.initialHydration.options.flowRow.id, "starter");
assert.equal(bootstrapProjection.initialHydration.options.addToRegistry, false);
assert.equal(bootstrapProjection.initialHydration.options.openEditor, true);
assert.deepEqual(bootstrapProjection.initialHydration.options.deployDefaults, { surface: "local" });
assert.deepEqual(bootstrapProjection.initialHydration.options.mobDefaults, { backend: "session" });
assert.deepEqual(controller.flowCatalogBootstrapState({ sample_mobpacks: [] }), {
  templates: [],
  flows: [],
  initialHydration: null,
});

global.window.treeToGraph = () => {
  throw new Error("controller exports must not call the UI graph renderer");
};

const members = [{
  id: "m_reviewer",
  name: "Reviewer",
  role: "reviewer",
  model: "openai/gpt-5",
  tools: ["git", "shell"],
  skills: ["mob.review"],
  schema: "ReviewArtifact",
}];

const previousFlow = {
  name: "main",
  steps: [{
    id: "input_1",
    type: "input",
    task: "Review the change.",
    inputParams: [],
  }],
};

const flow = controller.graphToFlow({
  previousFlow,
  members,
  instances: [{
    id: "review_step",
    memberId: "m_reviewer",
    col: 0,
    row: 0,
    launchMode: {
      kind: "Resume",
      sessionId: "session-123",
      budgetSplitPolicy: { kind: "Fixed", limit: 2048 },
    },
    dispatchMode: "one_to_one",
    collection: "quorum",
    quorum: 1,
    timeoutMs: 120000,
    allowedTools: ["git"],
    blockedTools: ["shell"],
    outputFormat: "text",
    dependsMode: "any",
  }],
  edges: [],
});

const graphSemanticBaseInstances = [{
  id: "review_step",
  memberId: "m_reviewer",
  col: 0,
  row: 0,
  launchMode: { kind: "Fresh" },
  collection: "all",
}];
const graphSemanticMovedInstances = [{
  ...graphSemanticBaseInstances[0],
  col: 9,
  row: 7,
}];
const graphSemanticChangedInstances = [{
  ...graphSemanticBaseInstances[0],
  launchMode: { kind: "Resume", sessionId: "session-456" },
}];
const graphSemanticBaseEdges = [{
  id: "route",
  from: "review_step",
  to: "review_step",
  kind: "cond",
  cond: { path: "steps.review_step.verdict", op: "==", value: "green" },
}];
const graphSemanticChangedEdges = [{
  ...graphSemanticBaseEdges[0],
  cond: { path: "steps.review_step.verdict", op: "!=", value: "red" },
}];
assert.notEqual(
  controller.graphSignature(graphSemanticBaseInstances, graphSemanticBaseEdges),
  controller.graphSignature(graphSemanticMovedInstances, graphSemanticBaseEdges),
);
assert.equal(
  controller.graphStructureSignature(graphSemanticBaseInstances, graphSemanticBaseEdges),
  controller.graphStructureSignature(graphSemanticMovedInstances, graphSemanticBaseEdges),
);
assert.notEqual(
  controller.graphStructureSignature(graphSemanticBaseInstances, graphSemanticBaseEdges),
  controller.graphStructureSignature(graphSemanticChangedInstances, graphSemanticBaseEdges),
);
assert.notEqual(
  controller.graphStructureSignature(graphSemanticBaseInstances, graphSemanticBaseEdges),
  controller.graphStructureSignature(graphSemanticBaseInstances, graphSemanticChangedEdges),
);

assert.equal(flow.steps.length, 2);
assert.deepEqual(flow.steps[1], {
  id: "review_step",
  type: "member",
  role: "m_reviewer",
  instruction: "",
  launchMode: {
    kind: "Resume",
    sessionId: "session-123",
    budgetSplitPolicy: { kind: "Fixed", limit: 2048 },
  },
  dispatchMode: "one_to_one",
  collection: "quorum",
  quorum: 1,
  timeoutMs: 120000,
  allowedTools: ["git"],
  blockedTools: ["shell"],
  outputFormat: "text",
  dependsMode: "any",
});

const flowWithPriorInstruction = controller.graphToFlow({
  previousFlow: {
    name: "main",
    steps: [
      previousFlow.steps[0],
      {
        id: "review_step",
        type: "member",
        role: "m_reviewer",
        instruction: "Run Reviewer.",
      },
    ],
  },
  members,
  instances: [{
    id: "review_step",
    memberId: "m_reviewer",
    col: 0,
    row: 0,
  }],
  edges: [],
});
assert.equal(flowWithPriorInstruction.steps[1].instruction, "Run Reviewer.");
const flowWithoutPriorInput = controller.graphToFlow({
  previousFlow: {
    name: "no-input",
    steps: [{ id: "input_1", type: "member", role: "m_reviewer" }],
  },
  members,
  instances: [],
  edges: [],
});
assert.deepEqual(flowWithoutPriorInput.steps, [{
  id: "input_2",
  type: "input",
  task: "Run the mobpack flow.",
  fields: "",
  inputParams: [],
}]);

const document = controller.buildDocument({
  flow,
  studio: {
    members,
    schemas: [],
    instances: [],
    edges: [
      { id: "stale_wrong_kind", from: "g_branch_route", to: "left", kind: "next", label: "stale" },
      { id: "stale_extra", from: "left", to: "g_parallel_deleted", kind: "next", label: "stale" },
    ],
    frames: [],
    skillRealms: [],
  },
  currentFlow: { name: "projection-proof" },
  deploySettings: testDeploySettings(),
});

const graphModeDocumentProjection = controller.authoringDocumentFromState({
  editorMode: "advanced",
  flow: { name: "graph-mode-doc", steps: [] },
  studio: {
    members,
    schemas: [],
    instances: [{ id: "review_step", memberId: "m_reviewer", col: 0, row: 0 }],
    edges: [],
    frames: [],
    skillRealms: [],
  },
  currentFlow: { name: "graph-mode-doc" },
  deploySettings: testDeploySettings(),
  mobSettings: { backendDefault: "session" },
});
assert.equal(graphModeDocumentProjection.flow.steps[1].id, "review_step");
assert.equal(graphModeDocumentProjection.flow.steps[1].role, "m_reviewer");
assert.equal(graphModeDocumentProjection.document.flow.steps[1].id, "review_step");
assert.equal(graphModeDocumentProjection.document.instances[0].id, "review_step");
assert.equal(graphModeDocumentProjection.document.mob_settings.backendDefault, "session");

const prunedSkillDocument = controller.buildDocument({
  flow,
  studio: {
    members,
    schemas: [],
    instances: [],
    edges: [],
    frames: [],
    skillRealms: [{
      id: "mobkit/sample-mobpacks",
      label: "mobkit/sample-mobpacks",
      source: "mobkit/sample-mobpack",
      skills: [
        { id: "mob.review", source: "inline", content: "Review the current work." },
        { id: "mob.unused", source: "inline", content: "Unused catalog skill." },
      ],
    }, {
      id: "filesystem",
      label: "filesystem",
      source: "filesystem",
      skills: [{ id: "mob.filesystem.unused", source: "path", path: "/tmp/unused.md" }],
    }],
  },
  currentFlow: { name: "skill-prune-proof" },
  deploySettings: testDeploySettings(),
});
assert.deepEqual(prunedSkillDocument.skill_realms, [{
  id: "mobkit/sample-mobpacks",
  label: "mobkit/sample-mobpacks",
  source: "mobkit/sample-mobpack",
  skills: [
    { id: "mob.review", source: "inline", content: "Review the current work." },
  ],
  default: false,
}]);
assert.deepEqual(controller.skillRealmsForDocument([{ id: "m", skills: [] }], prunedSkillDocument.skill_realms), []);

assert.deepEqual(document.launch_modes, [{
  step_id: "review_step",
  member_id: "m_reviewer",
  profile: "reviewer",
  launch_mode: {
    kind: "Resume",
    sessionId: "session-123",
    budgetSplitPolicy: { kind: "Fixed", limit: 2048 },
  },
  budget_split_policy: {
    type: "fixed",
    value: 2048,
  },
}]);

const missingLaunchFlow = controller.graphToFlow({
  previousFlow,
  members,
  instances: [{
    id: "missing_launch_step",
    memberId: "m_reviewer",
    col: 0,
    row: 0,
  }],
  edges: [],
});
assert.equal(missingLaunchFlow.steps[1].launchMode, null);
assert(!("dispatchMode" in missingLaunchFlow.steps[1]));
assert(!("collection" in missingLaunchFlow.steps[1]));
assert(!("dependsMode" in missingLaunchFlow.steps[1]));
assert(!("outputFormat" in missingLaunchFlow.steps[1]));

const missingLaunchDocument = controller.buildDocument({
  flow: missingLaunchFlow,
  studio: {
    members,
    schemas: [],
    instances: [],
    edges: [],
    frames: [],
    skillRealms: [],
  },
  currentFlow: { name: "missing-launch-proof" },
  deploySettings: testDeploySettings(),
});
assert.deepEqual(missingLaunchDocument.launch_modes, [{
  step_id: "missing_launch_step",
  member_id: "m_reviewer",
  profile: "reviewer",
  launch_mode: null,
}]);

const blankOptionalMetadataDocument = controller.buildDocument({
  flow: {
    name: "blank-optional-metadata",
    steps: [{
      id: "input_1",
      type: "input",
      task: "Review",
      fields: "",
      inputParams: [],
    }, {
      id: "review_blank_output",
      type: "member",
      role: "m_reviewer",
      instruction: "Review.",
      launchMode: { kind: "Fresh" },
      dispatchMode: "",
      collection: "",
      dependsMode: "",
      outputFormat: "",
    }, {
      id: "loop_blank_iteration",
      type: "repeat",
      loopId: "review_loop",
      maxIterations: 2,
      iterationInput: "",
      cond: { stepId: "review_loop_step", field: "verdict", op: "==", val: "green" },
      steps: [{
        id: "review_loop_step",
        type: "member",
        role: "m_reviewer",
        instruction: "Review loop.",
        launchMode: { kind: "Fresh" },
        dispatchMode: null,
        collection: "",
        dependsMode: "",
        outputFormat: null,
      }],
    }],
  },
  studio: {
    members,
    schemas: [],
    instances: [],
    edges: [],
    frames: [],
    skillRealms: [],
  },
  currentFlow: { name: "blank-optional-metadata" },
  deploySettings: testDeploySettings(),
});
const blankSteps = blankOptionalMetadataDocument.flow.steps;
assert(!("dispatchMode" in blankSteps[1]));
assert(!("collection" in blankSteps[1]));
assert(!("dependsMode" in blankSteps[1]));
assert(!("outputFormat" in blankSteps[1]));
assert(!("iterationInput" in blankSteps[2]));
assert(!("dispatchMode" in blankSteps[2].steps[0]));
assert(!("collection" in blankSteps[2].steps[0]));
assert(!("dependsMode" in blankSteps[2].steps[0]));
assert(!("outputFormat" in blankSteps[2].steps[0]));
const blankInstance = blankOptionalMetadataDocument.instances.find((inst) => inst.id === "review_blank_output");
assert(!("dispatchMode" in blankInstance));
assert(!("collection" in blankInstance));
assert(!("dependsMode" in blankInstance));
assert(!("outputFormat" in blankInstance));

const explicitFreshNoBudgetFlow = controller.graphToFlow({
  previousFlow,
  members,
  instances: [{
    id: "fresh_no_budget_step",
    memberId: "m_reviewer",
    col: 0,
    row: 0,
    launchMode: { kind: "Fresh" },
  }],
  edges: [],
});
assert.deepEqual(explicitFreshNoBudgetFlow.steps[1].launchMode, { kind: "Fresh" });
const explicitFreshNoBudgetDocument = controller.buildDocument({
  flow: explicitFreshNoBudgetFlow,
  studio: {
    members,
    schemas: [],
    instances: [],
    edges: [],
    frames: [],
    skillRealms: [],
  },
  currentFlow: { name: "fresh-no-budget-proof" },
  deploySettings: testDeploySettings(),
});
assert.deepEqual(explicitFreshNoBudgetDocument.launch_modes[0], {
  step_id: "fresh_no_budget_step",
  member_id: "m_reviewer",
  profile: "reviewer",
  launch_mode: { kind: "Fresh" },
});
assert(!("budget_split_policy" in explicitFreshNoBudgetDocument.launch_modes[0]));

const schemaSyncedFlow = controller.reconcileFlowMemberSchemas({
  name: "main",
  steps: [{
    id: "review_step",
    type: "member",
    role: "m_reviewer",
    schema: "ReviewArtifact",
    expectedSchemaRef: "schemas/ReviewArtifact.json",
  }],
}, [{ ...members[0], schema: "RenamedVerdict" }]);
assert.deepEqual(schemaSyncedFlow.steps[0], {
  id: "review_step",
  type: "member",
  role: "m_reviewer",
  schema: "RenamedVerdict",
  expectedSchemaRef: "schemas/RenamedVerdict.json",
});

const customSchemaPathFlow = controller.reconcileFlowMemberSchemas({
  name: "main",
  steps: [{
    id: "review_step",
    type: "member",
    role: "m_reviewer",
    schema: "ReviewArtifact",
    expectedSchemaRef: "schemas/custom/reviewer.json",
  }],
}, [{ ...members[0], schema: "RenamedVerdict" }]);
assert.equal(customSchemaPathFlow.steps[0].schema, "RenamedVerdict");
assert.equal(customSchemaPathFlow.steps[0].expectedSchemaRef, "schemas/custom/reviewer.json");

const schemaRemovedFlow = controller.reconcileFlowMemberSchemas({
  name: "main",
  steps: [{
    id: "review_step",
    type: "member",
    role: "m_reviewer",
    schema: "ReviewArtifact",
    expectedSchemaRef: "schemas/ReviewArtifact.json",
    expected_schema_ref: "schemas/ReviewArtifact.json",
  }],
}, [{ ...members[0], schema: "" }]);
assert(!("schema" in schemaRemovedFlow.steps[0]));
assert(!("expectedSchemaRef" in schemaRemovedFlow.steps[0]));
assert(!("expected_schema_ref" in schemaRemovedFlow.steps[0]));

const schemaRename = controller.renameSchemaDefinition({
  schemas: [
    { id: "ReviewArtifact", fields: [] },
    { id: "PlanArtifact", fields: [] },
  ],
  members: [
    { id: "m_reviewer", schema: "ReviewArtifact" },
    { id: "m_planner", schema: "PlanArtifact" },
  ],
  flow: {
    name: "main",
    steps: [{
      id: "review_step",
      type: "member",
      role: "m_reviewer",
      schema: "ReviewArtifact",
      expectedSchemaRef: "schemas/ReviewArtifact.json",
    }],
  },
}, "ReviewArtifact", "RenamedVerdict");
assert.equal(schemaRename.renamed, true);
assert.deepEqual(schemaRename.schemas.map((schema) => schema.id), ["RenamedVerdict", "PlanArtifact"]);
assert.equal(schemaRename.members[0].schema, "RenamedVerdict");
assert.equal(schemaRename.members[1].schema, "PlanArtifact");
assert.equal(schemaRename.flow.steps[0].schema, "RenamedVerdict");
assert.equal(schemaRename.flow.steps[0].expectedSchemaRef, "schemas/RenamedVerdict.json");

const duplicateSchemaRename = controller.renameSchemaDefinition({
  schemas: [
    { id: "ReviewArtifact", fields: [] },
    { id: "PlanArtifact", fields: [] },
  ],
  members: [{ id: "m_reviewer", schema: "ReviewArtifact" }],
  flow: { name: "main", steps: [] },
}, "ReviewArtifact", "PlanArtifact");
assert.equal(duplicateSchemaRename.renamed, false);
assert.equal(duplicateSchemaRename.reason, "duplicate_schema_id");
assert.equal(duplicateSchemaRename.members[0].schema, "ReviewArtifact");
assert.deepEqual(duplicateSchemaRename.flow, { name: "main", steps: [] });

assert.deepEqual(controller.basicConditionFromText('params.route == "docs"'), {
  namespace: "params",
  stepId: "params",
  field: "route",
  op: "==",
  val: "docs",
});
assert.deepEqual(controller.basicConditionFromText("steps.review.verdict > 3"), {
  namespace: "steps",
  stepId: "review",
  field: "verdict",
  op: ">",
  val: "3",
});
assert.equal(controller.basicConditionText({
  namespace: "params",
  stepId: "params",
  field: "route",
  val: "docs",
}, { defaultOperator: "==" }), 'params.route == "docs"');
assert.equal(controller.basicConditionText({
  stepId: "review",
  field: "verdict",
  op: "==",
  val: "green",
}), 'steps.review.verdict == "green"');
assert.equal(controller.basicConditionLabel({
  stepId: "review",
  field: "score",
  op: ">",
  val: 3,
}, [{ stepId: "review", label: "Reviewer" }]), "Reviewer.score > 3");
assert.equal(controller.basicRepeatUntilExpression({
  cond: { stepId: "review_loop", field: "verdict", op: "==", val: "green" },
  steps: [{ id: "review_loop", type: "member", role: "m_reviewer" }],
}, members, { defaultOperator: "==" }), 'Reviewer.verdict == "green"');
assert.equal(controller.basicRepeatUntilExpression({
  until: 'steps.review.verdict == "green"',
  cond: { stepId: "missing", field: "verdict", op: "==", val: "green" },
  steps: [{ id: "review_loop", type: "member", role: "m_reviewer" }],
}, members, { defaultOperator: "==" }), 'steps.review.verdict == "green"');
assert.equal(controller.basicRepeatUntilExpression({
  cond: { stepId: "review_loop", field: "verdict", val: "green" },
  steps: [{ id: "review_loop", type: "member", role: "m_reviewer" }],
}, members, {}), "");
const basicCanvasContract = {
  mob_definition: {
    defaults: {
      collection_policy: "all",
      condition_operator: "==",
    },
    collection_policies: ["all", "quorum"],
    condition_operators: ["=="],
  },
};
assert.deepEqual(controller.basicForkCanvasState({
  step: {
    id: "parallel_canvas",
    type: "parallel",
    branches: [
      { id: "left", label: "Left", steps: [] },
      { id: "right", label: "Right", steps: [{ id: "review_loop", type: "member", role: "m_reviewer" }] },
    ],
  },
  contract: basicCanvasContract,
}), {
  isParallel: true,
  className: "bld-fork bld-fork--parallel",
  lanes: [
    { id: "left", label: "Left", steps: [] },
    { id: "right", label: "Right", steps: [{ id: "review_loop", type: "member", role: "m_reviewer" }] },
  ],
  showRail: true,
  showJoin: true,
  joinLabel: "⋈ join · all",
});
assert.deepEqual(controller.basicForkCanvasState({
  step: {
    id: "branch_canvas",
    type: "branch",
    branches: [{ id: "docs", label: "Docs", steps: [] }],
    fallback: [{ id: "fallback_review", type: "member", role: "m_reviewer" }],
  },
  contract: basicCanvasContract,
  basicView: {
    ...hydratedCatalogs.basicView,
    branchFallbackTitle: "Otherwise",
  },
}).lanes.map((lane) => [lane.id, lane.label, lane.steps.length]), [
  ["docs", "Docs", 0],
  ["fallback", "Otherwise", 1],
]);
assert.equal(controller.basicRepeatIterationLabel({
  iterationInput: "review_loop",
  steps: [{ id: "review_loop", type: "member", role: "m_reviewer" }],
}, members, hydratedCatalogs.basicView), "unsupported: feeds Reviewer's output");
assert.deepEqual(controller.basicRepeatCanvasState({
  step: {
    cond: { stepId: "review_loop", field: "verdict", op: "==", val: "green" },
    maxIterations: 4,
    iterationInput: "carry",
    steps: [{ id: "review_loop", type: "member", role: "m_reviewer" }],
  },
  members,
  contract: basicCanvasContract,
  basicView: hydratedCatalogs.basicView,
}), {
  repeatUntilExpression: 'Reviewer.verdict == "green"',
  whileLabel: "while",
  notLabel: "not",
  conditionLabel: 'Reviewer.verdict == "green"',
  maxIterationsLabel: "max 4",
  loopBackLabel: "↑ loop back · carries last output",
  exitLabel: '↓ exit when Reviewer.verdict == "green"',
});
assert.deepEqual(controller.basicStepCardState({
  step: { type: "parallel" },
  members,
  contract: basicCanvasContract,
  basicView: hydratedCatalogs.basicView,
}), {
  icon: "‖",
  iconTint: "member",
  title: "Parallel",
  desc: "fan-out → join · all",
  configured: true,
  isFlowCard: true,
});
assert.deepEqual(controller.basicStepCardState({
  step: { type: "member", role: "m_reviewer" },
  members,
  contract: basicCanvasContract,
  basicView: hydratedCatalogs.basicView,
}), {
  icon: "◆",
  iconTint: "accent",
  title: "Reviewer",
  desc: "reviewer · openai/gpt-5",
  configured: true,
  isFlowCard: false,
});
assert.equal(controller.conditionValueLiteral("green"), '"green"');
assert.equal(controller.conditionValueLiteral("42"), "42");
assert.equal(controller.conditionValueLiteral("true"), "true");
assert.deepEqual(controller.conditionValueControl({
  type: "enum",
  enumValues: ["green", 7],
}, "green", hydratedCatalogs.conditionView), {
  kind: "enum",
  values: ["green", "7"],
  value: "green",
  optionRows: [
    { value: "", label: "—" },
    { value: "green", label: "green" },
    { value: "7", label: "7" },
  ],
  placeholder: "",
});
assert.deepEqual(controller.conditionValueControl({ type: "boolean" }, "", hydratedCatalogs.conditionView), {
  kind: "boolean",
  values: ["true", "false"],
  value: "",
  optionRows: [
    { value: "", label: "—" },
    { value: "true", label: "true" },
    { value: "false", label: "false" },
  ],
  placeholder: "",
});
assert.equal(controller.conditionValueControl({ type: "bool" }, false, hydratedCatalogs.conditionView).value, "false");
assert.deepEqual(controller.conditionValueControl({ type: "string", enumValues: ["ignored"] }, "typed", hydratedCatalogs.conditionView), {
  kind: "text",
  values: [],
  value: "typed",
  optionRows: [],
  placeholder: "value",
});
assert.deepEqual(controller.conditionValueControl(null, "", hydratedCatalogs.conditionView), { kind: "text", values: [], value: "", optionRows: [], placeholder: "value" });
assert.deepEqual(controller.conditionValueControl({ type: "enum", enumValues: ["yes"] }, "", {
  emptyValueLabel: "(none)",
  textValuePlaceholder: "literal",
}).optionRows, [
  { value: "", label: "(none)" },
  { value: "yes", label: "yes" },
]);

const memberStepPrunedFlow = controller.reconcileFlowMemberSteps({
  name: "member-prune-proof",
  steps: [
    { id: "keep_top", type: "member", role: "m_reviewer" },
    { id: "drop_top", type: "member", role: "m_deleted" },
    {
      id: "repeat_keep",
      type: "repeat",
      steps: [
        { id: "drop_repeat", type: "member", role: "m_deleted" },
        { id: "keep_repeat", type: "member", role: "m_reviewer" },
      ],
    },
    {
      id: "branch_keep",
      type: "branch",
      branches: [
        { id: "drop_branch", steps: [{ id: "drop_branch_step", type: "member", role: "m_deleted" }] },
        { id: "keep_branch", steps: [{ id: "keep_branch_step", type: "member", role: "m_reviewer" }] },
      ],
      fallback: [{ id: "drop_fallback_step", type: "member", role: "m_deleted" }],
    },
    {
      id: "parallel_drop",
      type: "parallel",
      branches: [
        { id: "drop_parallel", steps: [{ id: "drop_parallel_step", type: "member", role: "m_deleted" }] },
      ],
    },
  ],
}, [members[0]]);
assert.deepEqual(memberStepPrunedFlow.steps.map((step) => step.id), ["keep_top", "repeat_keep", "branch_keep"]);
assert.deepEqual(memberStepPrunedFlow.steps[1].steps.map((step) => step.id), ["keep_repeat"]);
assert.deepEqual(memberStepPrunedFlow.steps[2].branches.map((branch) => branch.id), ["keep_branch"]);
assert.deepEqual(memberStepPrunedFlow.steps[2].fallback, []);

const controlRoleFlow = controller.reconcileFlowControlRoles({
  name: "control-role-proof",
  steps: [{
    id: "parallel_review",
    type: "parallel",
    controllerRole: "m_deleted",
    controllerMemberId: "m_deleted",
    controlRole: "m_deleted",
    branches: [{
      id: "br_review",
      steps: [{
        id: "branch_nested",
        type: "branch",
        controllerRole: "m_reviewer",
        branches: [],
        fallback: [],
      }],
    }],
  }],
}, [members[0]]);
assert(!("controllerRole" in controlRoleFlow.steps[0]));
assert(!("controllerMemberId" in controlRoleFlow.steps[0]));
assert(!("controlRole" in controlRoleFlow.steps[0]));
assert.equal(controlRoleFlow.steps[0].branches[0].steps[0].controllerRole, "m_reviewer");

const graphControlInstances = controller.reconcileGraphControlRoles([
  { id: "join_stale", isGate: true, gateKind: "join", controllerRole: "m_deleted", joinRole: "m_deleted" },
  { id: "join_valid", isGate: true, gateKind: "join", controllerRole: "m_reviewer" },
], [members[0]]);
assert(!("controllerRole" in graphControlInstances[0]));
assert(!("joinRole" in graphControlInstances[0]));
assert.equal(graphControlInstances[1].controllerRole, "m_reviewer");

const graphMemberSync = controller.reconcileGraphMemberInstances({
  instances: [
    { id: "gate_branch", isGate: true, gateKind: "branch" },
    { id: "review_step", memberId: "m_reviewer" },
    { id: "deleted_step", memberId: "m_deleted" },
    { id: "source_file", isTerminal: true, kind: "success", label: "mob.toml" },
  ],
  edges: [
    { id: "keep_gate_review", from: "gate_branch", to: "review_step", kind: "next" },
    { id: "drop_deleted_out", from: "deleted_step", to: "review_step", kind: "next" },
    { id: "drop_deleted_in", from: "review_step", to: "deleted_step", kind: "next" },
    { id: "keep_source", from: "review_step", to: "source_file", kind: "next" },
  ],
}, [members[0]]);
assert.deepEqual(graphMemberSync.instances.map((instance) => instance.id), ["gate_branch", "review_step", "source_file"]);
assert.deepEqual(graphMemberSync.edges.map((edge) => edge.id), ["keep_gate_review", "keep_source"]);

const aggregateMemberReconcile = controller.reconcileAuthoringForMembers({
  members: [{ id: "m_reviewer", name: "Reviewer", role: "reviewer", tools: ["shell"], schema: "ReviewArtifact" }],
  previousMembers: [
    { id: "m_reviewer", name: "Review Lead", role: "reviewer" },
    { id: "m_deleted", name: "Deleted", role: "deleted" },
  ],
  mobSettings: {
    orchestrator: "review_lead",
    roleWiring: [{ a: "review_lead", b: "deleted" }, { a: "deleted", b: "review_lead" }],
    backendDefault: "session",
  },
  flow: {
    name: "aggregate-member-reconcile",
    steps: [
      { id: "keep_review", type: "member", role: "m_reviewer", schema: "OldArtifact", allowedTools: ["shell", "git"] },
      { id: "drop_deleted", type: "member", role: "m_deleted" },
      { id: "parallel_review", type: "parallel", controllerRole: "m_deleted", branches: [{ id: "br", steps: [{ id: "nested_review", type: "member", role: "m_reviewer" }] }] },
    ],
  },
  instances: [
    { id: "join_stale", isGate: true, gateKind: "join", controllerRole: "m_deleted" },
    { id: "review_inst", memberId: "m_reviewer", schema: "OldArtifact", allowedTools: ["shell", "git"] },
    { id: "deleted_inst", memberId: "m_deleted" },
  ],
  edges: [
    { id: "keep_join_review", from: "join_stale", to: "review_inst", kind: "next" },
    { id: "drop_deleted", from: "deleted_inst", to: "review_inst", kind: "next" },
  ],
});
assert.deepEqual(aggregateMemberReconcile.flow.steps.map((step) => step.id), ["keep_review", "parallel_review"]);
assert.equal(aggregateMemberReconcile.flow.steps[0].schema, "ReviewArtifact");
assert.deepEqual(aggregateMemberReconcile.flow.steps[0].allowedTools, ["shell"]);
assert(!("controllerRole" in aggregateMemberReconcile.flow.steps[1]));
assert.deepEqual(aggregateMemberReconcile.instances.map((instance) => instance.id), ["join_stale", "review_inst"]);
assert(!("controllerRole" in aggregateMemberReconcile.instances[0]));
assert.deepEqual(aggregateMemberReconcile.instances[1].allowedTools, ["shell"]);
assert.deepEqual(aggregateMemberReconcile.edges.map((edge) => edge.id), ["keep_join_review"]);
assert.equal(aggregateMemberReconcile.mobSettings.orchestrator, "reviewer");
assert.deepEqual(aggregateMemberReconcile.mobSettings.roleWiring, []);

const aggregateContractReconcile = controller.reconcileAuthoringWithContract({
  contractLoaded: true,
  contract: {
    deploy_settings: {
      command: "rkat mob deploy",
      surfaces: ["cli"],
      trust_policies: ["permissive"],
      realm_backends: ["sqlite"],
    },
    mob_definition: {
      runtime_modes: ["turn_driven"],
      profile_binding: ["inline"],
      profile_backends: ["session"],
    },
  },
  modelCatalog: [{ id: "gpt-5.5", label: "GPT-5.5" }],
  toolCatalog: [{ id: "shell", label: "Shell" }],
  skillRealms: [{ id: "main", skills: [{ id: "mob.review" }] }],
  deploySettings: {
    command: "wrong",
    surface: "desktop",
    trustPolicy: "strict",
    realmBackend: "memory",
    model: "bad-model",
  },
  members: [{
    id: "m_worker",
    name: "Worker",
    role: "worker",
    profileBinding: "realm",
    runtimeMode: "autonomous_host",
    backend: "external",
    model: "bad-model",
    tools: ["shell", "git"],
    skills: ["mob.review", "missing.skill"],
  }],
  mobSettings: {
    orchestrator: "worker",
    backendDefault: "external",
    roleWiring: [{ a: "worker", b: "worker" }],
  },
  flow: {
    name: "contract-reconcile",
    steps: [{ id: "work", type: "member", role: "m_worker", allowedTools: ["shell", "git"], blockedTools: ["git"] }],
  },
  instances: [{ id: "i_worker", memberId: "m_worker", allowedTools: ["shell", "git"], blockedTools: ["git"] }],
  edges: [],
});
assert.deepEqual(aggregateContractReconcile.deploySettings, {
  command: "rkat mob deploy",
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
});
assert.deepEqual(aggregateContractReconcile.members[0].skills, ["mob.review"]);
assert.deepEqual(aggregateContractReconcile.members[0].tools, ["shell"]);
assert.equal(aggregateContractReconcile.members[0].profileBinding, "");
assert.equal(aggregateContractReconcile.members[0].runtimeMode, "");
assert.equal(aggregateContractReconcile.members[0].backend, "");
assert.equal(aggregateContractReconcile.members[0].model, "");
assert.deepEqual(aggregateContractReconcile.flow.steps[0].allowedTools, ["shell"]);
assert.deepEqual(aggregateContractReconcile.flow.steps[0].blockedTools, []);
assert.deepEqual(aggregateContractReconcile.instances[0].allowedTools, ["shell"]);
assert.deepEqual(aggregateContractReconcile.instances[0].blockedTools, []);
assert.equal(aggregateContractReconcile.mobSettings.backendDefault, "");
assert.equal(aggregateContractReconcile.mobSettings.orchestrator, "worker");

const launchSourceFlow = controller.reconcileFlowLaunchSources({
  name: "launch-source-proof",
  steps: [
    { id: "plan", type: "member", role: "m_reviewer", launchMode: { kind: "Fresh" } },
    {
      id: "branch_launch",
      type: "branch",
      branches: [{
        id: "br_launch",
        steps: [{
          id: "review_fork",
          type: "member",
          role: "m_reviewer",
          launchMode: { kind: "Fork", from: "deleted_step", context: "full_history", budgetSplitPolicy: { kind: "Fixed", limit: 512 } },
        }],
      }],
      fallback: [],
    },
  ],
}, [members[0]]);
assert.deepEqual(launchSourceFlow.steps[1].branches[0].steps[0].launchMode, {
  kind: "Fresh",
  budgetSplitPolicy: { kind: "Fixed", limit: 512 },
});
const validLaunchSourceFlow = controller.reconcileFlowLaunchSources({
  name: "launch-source-valid-proof",
  steps: [
    { id: "plan", type: "member", role: "m_reviewer", launchMode: { kind: "Fresh" } },
    { id: "review_fork", type: "member", role: "m_reviewer", launchMode: { kind: "Fork", from: "plan", context: "full_history" } },
  ],
}, [members[0]]);
assert.equal(validLaunchSourceFlow.steps[1].launchMode.from, "plan");

const graphLaunchInstances = controller.reconcileGraphLaunchSources([
  { id: "plan", memberId: "m_reviewer", launchMode: { kind: "Fresh" } },
  { id: "review_fork", memberId: "m_reviewer", launchMode: { kind: "Fork", from: "deleted_instance", context: "full_history" } },
  { id: "review_member_fork", memberId: "m_reviewer", launchMode: { kind: "Fork", from: "m_reviewer", context: "full_history" } },
], [members[0]]);
assert.deepEqual(graphLaunchInstances[1].launchMode.kind, "Fresh");
assert.equal(graphLaunchInstances[2].launchMode.from, "m_reviewer");

const scopedToolMembers = [
  { id: "m_tool", name: "Tool User", role: "tool_user", tools: ["builtins", "shell"] },
];
const scopedToolFlow = controller.reconcileFlowStepToolScopes({
  name: "tool-scope-proof",
  steps: [
    { id: "input_1", type: "input", task: "", inputParams: [] },
    {
      id: "branch_1",
      type: "branch",
      branches: [{
        id: "br_1",
        label: "Branch",
        steps: [{
          id: "tool_step",
          type: "member",
          role: "m_tool",
          allowedTools: ["builtins", "mob"],
          blockedTools: ["shell", "memory"],
        }],
      }],
      fallback: [],
    },
  ],
}, scopedToolMembers);
assert.deepEqual(scopedToolFlow.steps[1].branches[0].steps[0].allowedTools, ["builtins"]);
assert.deepEqual(scopedToolFlow.steps[1].branches[0].steps[0].blockedTools, ["shell"]);

const scopedToolInstances = controller.reconcileGraphStepToolScopes([
  { id: "tool_inst", memberId: "m_tool", allowedTools: ["builtins", "mob"], blockedTools: ["shell", "memory"] },
  { id: "gate", isGate: true, allowedTools: ["mob"], blockedTools: ["shell"] },
], scopedToolMembers);
assert.deepEqual(scopedToolInstances[0].allowedTools, ["builtins"]);
assert.deepEqual(scopedToolInstances[0].blockedTools, ["shell"]);
assert.deepEqual(scopedToolInstances[1].allowedTools, ["mob"]);

const skillScopedMembers = [
  { id: "m_skill", name: "Skill User", role: "skill_user", skills: ["mob.workpad", "mob.missing"] },
];
assert.equal(controller.reconcileMemberSkillRefs(skillScopedMembers, []), skillScopedMembers);
assert.deepEqual(controller.reconcileMemberSkillRefs(skillScopedMembers, [], { strictEmpty: true })[0].skills, []);
const reconciledSkillMembers = controller.reconcileMemberSkillRefs(skillScopedMembers, [{
  id: "realm",
  skills: [{ id: "mob.workpad" }, { id: "mob.review" }],
}]);
assert.deepEqual(reconciledSkillMembers[0].skills, ["mob.workpad"]);

const pendingDeploySettings = { surface: "custom", model: "custom-model" };
assert.equal(controller.reconcileDeploySettingsWithContract(pendingDeploySettings, null, []), pendingDeploySettings);
assert.equal(controller.reconcileDeploySettingsWithContract({ model: "custom-model" }, null, []).model, "custom-model");
assert.equal(controller.reconcileDeploySettingsWithContract({ model: "custom-model" }, null, [], { strictEmptyModels: true }).model, "");
const reconciledDeploySettings = controller.reconcileDeploySettingsWithContract({
  command: "legacy deploy command",
  surface: "console",
  trustPolicy: "loose",
  realmBackend: "memory",
  model: "missing-model",
  prompt: "Run it.",
}, {
  deploy_settings: {
    command: "rkat mob deploy",
    surfaces: ["cli", "rpc"],
    trust_policies: ["permissive", "strict"],
    realm_backends: ["jsonl", "sqlite"],
  },
}, [{ id: "openai/gpt-5" }]);
assert.equal(reconciledDeploySettings.command, "rkat mob deploy");
assert.equal(reconciledDeploySettings.surface, "");
assert.equal(reconciledDeploySettings.trustPolicy, "");
assert.equal(reconciledDeploySettings.realmBackend, "");
assert.equal(reconciledDeploySettings.model, "");

const pendingContractMembers = [{ id: "m_custom", model: "custom-model", runtimeMode: "custom", tools: ["custom-tool"] }];
assert.equal(controller.reconcileMembersWithContract(pendingContractMembers, null, {}, []), pendingContractMembers);
const reconciledContractMembers = controller.reconcileMembersWithContract([
  {
    id: "m_bad",
    profileBinding: "legacy_profile",
    runtimeMode: "invalid_runtime",
    backend: "sidecar",
    model: "missing-model",
    tools: ["builtins", "missing-tool"],
  },
  {
    id: "m_host",
    profileBinding: "inline",
    runtimeMode: "autonomous_host",
    backend: "session",
    model: "openai/gpt-5",
    tools: ["shell"],
  },
], {
  mob_definition: {
    defaults: { runtime_mode: "turn_driven" },
    runtime_modes: ["turn_driven", "autonomous_host"],
    profile_binding: ["inline"],
    profile_backends: ["session", "external"],
  },
}, { surface: "cli" }, [{ id: "openai/gpt-5" }], [{ id: "builtins" }]);
assert.equal(reconciledContractMembers[0].profileBinding, "");
assert.equal(reconciledContractMembers[0].runtimeMode, "");
assert.equal(reconciledContractMembers[0].backend, "");
assert.equal(reconciledContractMembers[0].model, "");
assert.deepEqual(reconciledContractMembers[0].tools, ["builtins"]);
assert.equal(reconciledContractMembers[1].runtimeMode, "turn_driven");
assert.deepEqual(reconciledContractMembers[1].tools, []);
assert.equal(controller.reconcileMembersWithContract(pendingContractMembers, null, {}, [], [], { strictEmptyModels: true })[0].model, "");
assert.deepEqual(controller.reconcileMembersWithContract(pendingContractMembers, null, {}, [], [], { strictEmptyTools: true })[0].tools, []);

const pendingMobSettings = { backendDefault: "custom-backend" };
assert.equal(controller.reconcileMobSettingsWithContract(pendingMobSettings, null), pendingMobSettings);
assert.equal(controller.reconcileMobSettingsWithContract({
  backendDefault: "sidecar",
  externalAddressBase: "http://127.0.0.1:9000",
}, {
  mob_definition: { profile_backends: ["session", "external"] },
}).backendDefault, "");

const schemaReferenceFlow = {
  name: "schema-reference-proof",
  steps: [
    { id: "input_1", type: "input" },
    { id: "review_step", type: "member", role: "m_reviewer", schema: "ReviewArtifact" },
    {
      id: "branch_review",
      type: "branch",
      branches: [{
        id: "br_green",
        label: "Green",
        condition: 'steps.review_step.verdict == "green"',
        cond: { namespace: "steps", stepId: "review_step", field: "verdict", op: "==", val: "green" },
        steps: [],
      }],
      fallback: [],
    },
    {
      id: "loop_review",
      type: "repeat",
      until: 'steps.review_loop.verdict == "green"',
      cond: { namespace: "steps", stepId: "review_loop", field: "verdict", op: "==", val: "green" },
      steps: [{ id: "review_loop", type: "member", role: "m_reviewer", schema: "ReviewArtifact" }],
    },
  ],
};
const schemaReferenceEdges = [{
  id: "e_branch_green",
  from: "g_branch_review",
  to: "review_step",
  kind: "cond",
  label: 'steps.review_step.verdict == "green"',
  cond: { var: "steps.review_step.verdict", op: "==", val: "green" },
}];
const renamedReferences = controller.reconcileSchemaFieldReferences({
  flow: schemaReferenceFlow,
  edges: schemaReferenceEdges,
  members,
  instances: [{ id: "review_step", memberId: "m_reviewer" }, { id: "review_loop", memberId: "m_reviewer" }],
  schemaId: "ReviewArtifact",
  oldName: "verdict",
  newName: "decision",
});
assert.equal(renamedReferences.flow.steps[2].branches[0].cond.field, "decision");
assert.equal(renamedReferences.flow.steps[2].branches[0].condition, 'steps.review_step.decision == "green"');
assert.equal(renamedReferences.flow.steps[3].cond.field, "decision");
assert.equal(renamedReferences.flow.steps[3].until, 'steps.review_loop.decision == "green"');
assert.equal(renamedReferences.edges[0].cond.var, "steps.review_step.decision");
assert.equal(renamedReferences.edges[0].label, 'steps.review_step.decision == "green"');

const deletedReferences = controller.reconcileSchemaFieldReferences({
  flow: renamedReferences.flow,
  edges: renamedReferences.edges,
  members,
  instances: [{ id: "review_step", memberId: "m_reviewer" }, { id: "review_loop", memberId: "m_reviewer" }],
  schemaId: "ReviewArtifact",
  oldName: "decision",
  newName: "",
});
assert.deepEqual(deletedReferences.flow.steps[2].branches[0].cond, {});
assert.equal(deletedReferences.flow.steps[2].branches[0].condition, "");
assert.deepEqual(deletedReferences.flow.steps[3].cond, {});
assert.equal(deletedReferences.flow.steps[3].until, "");
assert.equal(deletedReferences.edges[0].cond, null);
assert.equal(deletedReferences.edges[0].label, "");

assert.equal(document.flow.steps[1].timeoutMs, 120000);
assert.deepEqual(document.flow.steps[1].allowedTools, ["git"]);
assert.deepEqual(document.flow.steps[1].blockedTools, ["shell"]);
assert.equal(document.flow.steps[1].outputFormat, "text");

const [agentDefinition] = controller.agentDefinitionsFromSchema({
  agent_definitions: [{
    id: "reviewer",
    role: "reviewer",
    name: "Reviewer",
    model: "gpt-5.5",
    definitionType: "mobkit/profile-member",
    source: "mobkit/mobpack-profile-member",
    sourceMobpack: "sample_review_pr",
    sourceDocumentPath: "document.members[]",
    profileBinding: "inline",
    runtimeMode: "turn_driven",
    backend: "sidecar",
    schema: "ReviewArtifact",
    schemaDefinition: {
      id: "ReviewArtifact",
      description: "Review output",
      fields: [{ id: "f1", name: "verdict", type: "enum", required: true, enumValues: ["green", "red"] }],
    },
  }],
});
assert.equal(agentDefinition.backend, "sidecar");

assert.deepEqual(controller.agentDefinitionsFromSchema({
  members: [{
    id: "legacy-member",
    role: "legacy",
    name: "Legacy",
    model: "gpt-5.5",
  }],
}), []);

assert.deepEqual(controller.agentDefinitionsFromSchema({
  agent_definitions: [{
    id: "partial",
    role: "partial",
    name: "Partial",
    model: "gpt-5.5",
    definitionType: "mobkit/profile-member",
    source: "mobkit/mobpack-profile-member",
    sourceMobpack: "sample_partial",
    sourceDocumentPath: "document.members[]",
  }],
}), []);

assert.deepEqual(controller.agentDefinitionsFromSchema({
  agent_definitions: [{
    id: "model_less",
    role: "model_less",
    name: "Model Less",
    definitionType: "mobkit/profile-member",
    source: "mobkit/mobpack-profile-member",
    sourceMobpack: "sample_partial",
    sourceDocumentPath: "document.members[]",
    profileBinding: "inline",
    runtimeMode: "turn_driven",
  }],
}), []);

assert.equal(controller.memberPromptSkeleton({
  name: "Reviewer",
  role: "reviewer",
  schema: "ReviewArtifact",
  systemPrompt: "  Gate the implementation.\n\nEmit a verdict.  ",
}), [
  "You are Reviewer, a member of a Meerkat mob.",
  "",
  "## Mandate",
  "Gate the implementation. Emit a verdict.",
  "",
  "## Operating rules",
  "- Read the shared mob workpad and prior members' output before acting.",
  "- Do exactly what this step requires — no more, no less.",
  "- Emit a ReviewArtifact as your structured output.",
  "- Hand off cleanly: state what you did and what the next member needs.",
].join("\n"));
assert.match(controller.memberPromptSkeleton({ role: "planner" }), /Act as the planner of the mob\./);
assert.match(controller.memberPromptSkeleton({ name: "Coder" }), /Return a concise, well-structured result\./);
assert.deepEqual(controller.memberNamePatch(" Quality Reviewer "), { name: " Quality Reviewer " });
assert.deepEqual(controller.memberRealmProfilePatch(" qa_profile "), { realmProfile: "qa_profile" });
assert.deepEqual(controller.memberSystemPromptPatch("  Review carefully.\n"), { systemPrompt: "  Review carefully.\n" });

assert.deepEqual(controller.agentDefinitionsFromSchema({
  agent_definitions: [{
    id: "",
    role: "missing_id",
    name: "Missing id",
    definitionType: "mobkit/profile-member",
    source: "mobkit/mobpack-profile-member",
    profileBinding: "inline",
    runtimeMode: "turn_driven",
  }, {
    id: "missing_role",
    role: "",
    name: "Missing role",
    definitionType: "mobkit/profile-member",
    source: "mobkit/mobpack-profile-member",
    profileBinding: "inline",
    runtimeMode: "turn_driven",
  }, {
    id: "missing_name",
    role: "missing_name",
    name: "",
    definitionType: "mobkit/profile-member",
    source: "mobkit/mobpack-profile-member",
    profileBinding: "inline",
    runtimeMode: "turn_driven",
  }],
}), []);

assert.throws(
  () => controller.memberFromAgentDefinition({ id: "partial", role: "partial" }, []),
  /profile-member contract|source contract/,
);
assert.throws(
  () => controller.memberFromAgentDefinition({
    definitionType: "mobkit/profile-member",
    source: "mobkit/mobpack-profile-member",
    id: "partial",
    role: "partial",
    name: "Partial",
  }, []),
  /profileBinding contract|runtimeMode contract/,
);
assert.throws(
  () => controller.memberFromAgentDefinition({
    definitionType: "mobkit/profile-member",
    source: "mobkit/mobpack-profile-member",
    id: "model_less",
    role: "model_less",
    name: "Model Less",
    profileBinding: "inline",
    runtimeMode: "turn_driven",
  }, []),
  /model contract/,
);
assert.throws(
  () => controller.memberFromAgentDefinition({
    definitionType: "mobkit/profile-member",
    source: "mobkit/mobpack-profile-member",
    role: "missing_id",
    name: "Missing Id",
    model: "gpt-5.5",
    profileBinding: "inline",
    runtimeMode: "turn_driven",
  }, []),
  /id contract/,
);
assert.throws(
  () => controller.memberFromAgentDefinition({
    definitionType: "mobkit/profile-member",
    source: "mobkit/mobpack-profile-member",
    id: "missing_role",
    name: "Missing Role",
    model: "gpt-5.5",
    profileBinding: "inline",
    runtimeMode: "turn_driven",
  }, []),
  /role contract/,
);
assert.throws(
  () => controller.memberFromAgentDefinition({
    definitionType: "mobkit/profile-member",
    source: "mobkit/mobpack-profile-member",
    id: "missing_name",
    role: "missing_name",
    model: "gpt-5.5",
    profileBinding: "inline",
    runtimeMode: "turn_driven",
  }, []),
  /name contract/,
);

const agentContract = {
  mob_definition: {
    profile_binding: ["inline"],
    profile_binding_restrictions: {
      realm_profile: {
        deployable: false,
        label: "realm_profile — import-only; rkat mob validate forbids realm refs in packs",
        reason: "rkat mob validate rejects mobpack profiles that use realm_profile references; export deployable packs with inline profiles.",
      },
    },
    runtime_modes: ["turn_driven", "autonomous_host"],
    profile_backends: ["session", "external"],
  },
};
assert.deepEqual(
  controller.profileBindingOptions(agentContract, "").map((option) => option.value),
  ["inline"],
);
assert.deepEqual(
  controller.profileBindingOptions(agentContract, "realm_profile").map((option) => [option.value, option.disabled, option.reason]),
  [
    ["inline", false, ""],
    ["realm_profile", true, "rkat mob validate rejects mobpack profiles that use realm_profile references; export deployable packs with inline profiles."],
  ],
);
assert.deepEqual(
  controller.runtimeModeOptions(agentContract, { surface: "rpc" }, "").map((option) => option.value),
  ["turn_driven", "autonomous_host"],
);
const tweaksState = controller.tweaksControlState({
  flows: [
    { id: "f_draft", name: "Draft Mob", stage: "", source: "", document: { mob_id: "draft" } },
    { id: "f_empty", name: "No Document", stage: "valid" },
    { id: "f_valid", name: "Valid Mob", stage: "valid", document: { mob_id: "valid" } },
  ],
  deploySettings: { surface: "cli", trustPolicy: "permissive", realmBackend: "sqlite" },
  mobSettings: { backendDefault: "session" },
  members: [{ id: "m_reviewer", role: "reviewer", name: "Reviewer" }],
  modelCatalog: [{ id: "openai/gpt-5.5", label: "GPT-5.5", vendor: "OpenAI" }],
  settingsView: TEST_SETTINGS_VIEW,
  contract: {
    deploy_settings: {
      surfaces: ["cli", "rpc"],
      trust_policies: ["permissive", "strict"],
      realm_backends: ["sqlite"],
    },
    mob_definition: {
      profile_backends: ["session", "external"],
    },
  },
});
assert.deepEqual(tweaksState.loadableFlowOptions, [
  { value: "f_draft", label: "Draft Mob · draft" },
  { value: "f_valid", label: "Valid Mob · valid" },
]);
assert.equal(tweaksState.panelTitle, "Tweaks");
assert.equal(tweaksState.loadMobTitle, "Load mob");
assert.equal(tweaksState.loadMobLabel, "Mobpack");
assert.equal(tweaksState.canvasTitle, "Canvas");
assert.equal(tweaksState.edgeStyleLabel, "Edges");
assert.deepEqual(tweaksState.edgeStyleOptions, [
  { value: "text", label: "Text" },
  { value: "icons", label: "Icons" },
  { value: "colored", label: "Color" },
]);
assert.equal(tweaksState.densityLabel, "Density");
assert.deepEqual(tweaksState.densityOptions, [
  { value: "compact", label: "Compact" },
  { value: "comfortable", label: "Comfy" },
]);
assert.equal(tweaksState.themeTitle, "Theme");
assert.equal(tweaksState.themeModeLabel, "Mode");
assert.deepEqual(tweaksState.themeModeOptions, [
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
]);
assert.equal(tweaksState.mobTitle, "Mob");
assert.equal(tweaksState.orchestratorLabel, "Orchestrator");
assert.equal(tweaksState.autoWireLabel, "Auto wire");
assert.deepEqual(tweaksState.autoWireOptions, [
  { value: "no", label: "No" },
  { value: "yes", label: "Yes" },
]);
assert.equal(tweaksState.defaultBackendLabel, "Default backend");
assert.equal(tweaksState.externalBaseLabel, "External base");
assert.equal(tweaksState.externalBasePlaceholder, "http://127.0.0.1:9000");
assert.equal(tweaksState.deployTitle, "Deploy");
assert.equal(tweaksState.surfaceLabel, "Surface");
assert.equal(tweaksState.trustLabel, "Trust");
assert.equal(tweaksState.modelLabel, "Model");
assert.equal(tweaksState.durationLabel, "Duration");
assert.equal(tweaksState.durationPlaceholder, "30s");
assert.equal(tweaksState.toolCallsLabel, "Tool calls");
assert.equal(tweaksState.toolCallsMin, 0);
assert.equal(tweaksState.toolCallsMax, 999);
assert.equal(tweaksState.tokensLabel, "Tokens");
assert.equal(tweaksState.tokensMin, 0);
assert.equal(tweaksState.tokensMax, 200000);
assert.equal(tweaksState.realmLabel, "Realm");
assert.deepEqual(tweaksState.realmOptions, [
  { value: "isolated", label: "Isolated" },
  { value: "shared", label: "Shared" },
]);
assert.equal(tweaksState.realmIdLabel, "Realm ID");
assert.equal(tweaksState.realmIdPlaceholder, "realm id");
assert.equal(tweaksState.backendLabel, "Backend");
assert.equal(tweaksState.promptLabel, "Prompt");
assert.equal(tweaksState.promptPlaceholder, "Deploy prompt");
assert.equal(tweaksState.commandLabel, "Command");
assert.equal(tweaksState.commandFallback, "--");
assert.equal(tweaksState.inspectorTitle, "Inspector");
assert.equal(tweaksState.inspectorLayoutLabel, "Layout");
assert.deepEqual(tweaksState.inspectorLayoutOptions, [
  { value: "right", label: "Right" },
  { value: "bottom", label: "Bottom" },
  { value: "modal", label: "Modal" },
]);
assert.deepEqual(tweaksState.profileOptions, [
  { value: "", label: "none" },
  { value: "reviewer", label: "reviewer" },
]);
assert.deepEqual(tweaksState.profileChoices, [{ value: "reviewer", label: "reviewer" }]);
assert.deepEqual(tweaksState.modelOptions, [
  { value: "", label: "default" },
  { value: "openai/gpt-5.5", label: "GPT-5.5 · OpenAI" },
]);
assert.deepEqual(tweaksState.surfaceOptions.map((option) => option.value), ["cli", "rpc"]);
assert.deepEqual(tweaksState.trustOptions.map((option) => option.value), ["permissive", "strict"]);
assert.deepEqual(tweaksState.realmBackendOptions.map((option) => option.value), ["sqlite"]);
assert.deepEqual(tweaksState.mobBackendOptions.map((option) => option.value), ["session", "external"]);
assert.deepEqual(controller.memberProfileBindingPatch({ role: "reviewer", name: "Reviewer" }, "realm_profile", agentContract), {});
assert.deepEqual(controller.memberProfileBindingPatch({ realmProfile: "qa_profile" }, "inline", agentContract), {
  profileBinding: "inline",
  realmProfile: "",
});
assert.deepEqual(controller.memberRuntimeModePatch("turn_driven", agentContract, { surface: "cli" }), { runtimeMode: "turn_driven" });
assert.deepEqual(controller.memberRuntimeModePatch("autonomous_host", agentContract, { surface: "cli" }), {});
assert.deepEqual(controller.memberRuntimeModePatch("", agentContract, { surface: "cli" }), {});
assert.deepEqual(controller.memberModelPatch(" openai/gpt-5 ", [{ id: "openai/gpt-5" }]), { model: "openai/gpt-5" });
assert.deepEqual(controller.memberModelPatch("openai/ghost", [{ id: "openai/gpt-5" }]), {});
assert.deepEqual(controller.memberModelPatch("", [{ id: "openai/gpt-5" }]), {});
assert.deepEqual(controller.memberSchemaPatch(" ReviewArtifact ", [{ id: "ReviewArtifact" }]), { schema: "ReviewArtifact" });
assert.deepEqual(controller.memberSchemaPatch("GhostArtifact", [{ id: "ReviewArtifact" }]), {});
assert.deepEqual(controller.memberSchemaPatch("", [{ id: "ReviewArtifact" }]), { schema: "" });
const memberSchemaCascade = controller.memberSchemaCascadePatch({
  memberId: "m_reviewer",
  members: [{ id: "m_reviewer", schema: "ReviewArtifact" }],
  schemas: [
    { id: "ReviewArtifact", fields: [{ id: "f1", name: "verdict", type: "enum" }] },
    { id: "SummaryArtifact", fields: [{ id: "f2", name: "summary", type: "string" }] },
  ],
  flow: {
    name: "member-schema-cascade",
    steps: [
      { id: "review_step", type: "member", role: "m_reviewer" },
      {
        id: "route",
        type: "branch",
        branches: [{
          id: "br_green",
          cond: { stepId: "review_step", field: "verdict", op: "==", val: "green" },
          condition: "steps.review_step.verdict == \"green\"",
          steps: [],
        }],
        fallback: [],
      },
    ],
  },
  edges: [{
    id: "e_review_done",
    from: "review_inst",
    to: "done",
    kind: "cond",
    label: "steps.review_inst.verdict == \"green\"",
    cond: { var: "steps.review_inst.verdict", op: "==", val: "green" },
  }],
  instances: [
    { id: "review_inst", memberId: "m_reviewer" },
    { id: "done", isTerminal: true },
  ],
}, "SummaryArtifact");
assert.equal(memberSchemaCascade.ok, true);
assert.deepEqual(memberSchemaCascade.patch, { schema: "SummaryArtifact" });
assert.deepEqual(memberSchemaCascade.members, [{ id: "m_reviewer", schema: "SummaryArtifact" }]);
assert.deepEqual(memberSchemaCascade.flow.steps[1].branches[0].cond, {});
assert.equal(memberSchemaCascade.flow.steps[1].branches[0].condition, "");
assert.deepEqual(memberSchemaCascade.edges[0].cond, null);
assert.equal(memberSchemaCascade.edges[0].label, "");
assert.deepEqual(controller.memberSchemaCascadePatch({
  memberId: "m_reviewer",
  members: [{ id: "m_reviewer", schema: "ReviewArtifact" }],
  schemas: [{ id: "ReviewArtifact" }],
}, "GhostArtifact"), {
  ok: false,
  error: "unknown schema",
  members: [{ id: "m_reviewer", schema: "ReviewArtifact" }],
  flow: undefined,
  edges: undefined,
  patch: null,
});
assert.deepEqual(controller.memberBackendPatch("external", agentContract), { backend: "external" });
assert.deepEqual(controller.memberBackendPatch("daemon", agentContract), {});
assert.deepEqual(controller.memberMaxInlinePeerNotificationsPatch("4"), { maxInlinePeerNotifications: 4 });
assert.deepEqual(controller.memberMaxInlinePeerNotificationsPatch(""), { maxInlinePeerNotifications: null });
assert.deepEqual(controller.memberMaxInlinePeerNotificationsPatch("-2"), { maxInlinePeerNotifications: null });
assert.deepEqual(controller.memberProviderParamsEditorState({
  providerParams: { thinking_budget: 4096, top_k: 20 },
}, hydratedCatalogs.agentDetailView), {
  label: "Provider params",
  text: '{\n  "thinking_budget": 4096,\n  "top_k": 20\n}',
  placeholder: '{"thinking_budget":4096}',
  rows: 4,
  invalidJsonLabel: "invalid JSON",
});
assert.deepEqual(controller.memberProviderParamsEditorState({ providerParams: { top_k: 20 } }, {
  ...hydratedCatalogs.agentDetailView,
  providerParamsLabel: "Provider settings",
  providerParamsPlaceholder: '{"top_k":20}',
  providerParamsRows: 6,
  providerParamsInvalidJsonLabel: "bad JSON",
}), {
  label: "Provider settings",
  text: '{\n  "top_k": 20\n}',
  placeholder: '{"top_k":20}',
  rows: 6,
  invalidJsonLabel: "bad JSON",
});
assert.equal(controller.memberProviderParamsEditorState({ providerParams: null }, hydratedCatalogs.agentDetailView).text, "");
assert.deepEqual(controller.memberProviderParamsPatch('{"thinking_budget":4096}', hydratedCatalogs.agentDetailView), {
  ok: true,
  patch: { providerParams: { thinking_budget: 4096 } },
  error: "",
});
assert.deepEqual(controller.memberProviderParamsPatch("", hydratedCatalogs.agentDetailView), {
  ok: true,
  patch: { providerParams: null },
  error: "",
});
assert.deepEqual(controller.memberProviderParamsPatch("[]", {
  ...hydratedCatalogs.agentDetailView,
  providerParamsObjectRequiredError: "provider settings must be an object",
}), {
  ok: false,
  patch: null,
  error: "provider settings must be an object",
});
assert.equal(controller.memberProviderParamsPatch("{", hydratedCatalogs.agentDetailView).ok, false);

assert.deepEqual(controller.schemaDefinitionsFromAgentDefinition(agentDefinition), [{
  id: "ReviewArtifact",
  description: "Review output",
  fields: [{ id: "f1", name: "verdict", type: "enum", required: true, enumValues: ["green", "red"] }],
}]);

const staleAgentSchemas = [{
  id: "ReviewArtifact",
  description: "Old review output",
  fields: [{ id: "old", name: "stale", type: "string", required: false }],
}];
const mergedAgentSchemas = controller.mergeAgentDefinitionSchemas(staleAgentSchemas, agentDefinition);
assert.deepEqual(mergedAgentSchemas, [{
  id: "ReviewArtifact",
  description: "Review output",
  fields: [{ id: "f1", name: "verdict", type: "enum", required: true, enumValues: ["green", "red"] }],
}]);
assert.notEqual(mergedAgentSchemas, staleAgentSchemas);
assert.equal(controller.mergeAgentDefinitionSchemas(mergedAgentSchemas, agentDefinition), mergedAgentSchemas);

const addedAgent = controller.agentDefinitionAddPatch(agentDefinition, {
  members: [{ id: "m_reviewer", name: "Reviewer", role: "reviewer" }],
  schemas: staleAgentSchemas,
});
assert.equal(addedAgent.member.id, "m_reviewer_2");
assert.equal(addedAgent.members.length, 2);
assert.equal(addedAgent.schemasChanged, true);
assert.deepEqual(addedAgent.schemas, mergedAgentSchemas);
const addedById = controller.agentDefinitionAddByIdPatch([agentDefinition], "reviewer", {
  members: [],
  schemas: [],
});
assert.equal(addedById.ok, true);
assert.equal(addedById.member.id, "m_reviewer");
assert.equal(controller.agentDefinitionAddByIdPatch([agentDefinition], "missing").ok, false);
const agentMembersForProjection = [
  { id: "m_planner", name: "Planner", role: "planner", model: "gpt-5.5", schema: "PlanArtifact" },
  {
    id: "m_reviewer",
    name: "Reviewer",
    role: "reviewer",
    model: "gpt-5.5",
    schema: "ReviewArtifact",
    profileBinding: "inline",
    runtimeMode: "turn_driven",
    backend: "session",
  },
];
const agentSchemasForProjection = [
  { id: "PlanArtifact", fields: [{ id: "f1" }] },
  { id: "ReviewArtifact", fields: [{ id: "f1" }, { id: "f2" }] },
];
const agentInstancesForProjection = [
  { id: "n_review_1", memberId: "m_reviewer", col: 1, row: 2, lane: "main" },
  { id: "n_review_2", memberId: "m_reviewer", col: 2, row: 3 },
];
assert.deepEqual(controller.agentListState({
  members: agentMembersForProjection,
  instances: agentInstancesForProjection,
  schemas: agentSchemasForProjection,
  selection: { kind: "schema", id: "ReviewArtifact" },
  agentView: hydratedCatalogs.agentView,
}), {
  agentsHeading: "AGENTS",
  schemasHeading: "SCHEMAS",
  addSchemaLabel: "+ new schema",
  emptyTitle: "AGENT LIBRARY",
  emptyLines: [
    "Select an agent or schema on the left.",
    "Agents are reusable across topologies. Edit one here and every placement updates.",
  ],
  missingSchemaLabel: "Schema not found.",
  missingAgentLabel: "Agent not found.",
  memberCount: 2,
  schemaCount: 2,
  memberRows: [
    {
      id: "m_planner",
      name: "Planner",
      role: "planner",
      model: "gpt-5.5",
      member: agentMembersForProjection[0],
      selected: false,
      itemClass: "agents-list__item",
      bulletRole: "planner",
      subLabel: "planner · gpt-5.5",
      placedCount: 0,
      placedLabel: "unplaced",
      isUnplaced: true,
      placedClass: "agents-list__placed is-zero",
    },
    {
      id: "m_reviewer",
      name: "Reviewer",
      role: "reviewer",
      model: "gpt-5.5",
      member: agentMembersForProjection[1],
      selected: false,
      itemClass: "agents-list__item",
      bulletRole: "reviewer",
      subLabel: "reviewer · gpt-5.5",
      placedCount: 2,
      placedLabel: "×2",
      isUnplaced: false,
      placedClass: "agents-list__placed",
    },
  ],
  schemaRows: [
    {
      id: "PlanArtifact",
      schema: agentSchemasForProjection[0],
      selected: false,
      itemClass: "agents-list__item",
      bulletRole: "schema",
      fieldCount: 1,
      fieldLabel: "1 field",
      usedCount: 1,
      usageLabel: "used by 1",
      subLabel: "1 field · used by 1",
    },
    {
      id: "ReviewArtifact",
      schema: agentSchemasForProjection[1],
      selected: true,
      itemClass: "agents-list__item is-selected",
      bulletRole: "schema",
      fieldCount: 2,
      fieldLabel: "2 fields",
      usedCount: 1,
      usageLabel: "used by 1",
      subLabel: "2 fields · used by 1",
    },
  ],
});
assert.deepEqual(controller.agentListState({}), {
  agentsHeading: "",
  schemasHeading: "",
  addSchemaLabel: "",
  emptyTitle: "",
  emptyLines: [],
  missingSchemaLabel: "",
  missingAgentLabel: "",
  memberCount: 0,
  schemaCount: 0,
  memberRows: [],
  schemaRows: [],
});
assert.deepEqual(controller.agentSelectionState({
  selection: { kind: "agent", id: "m_reviewer" },
  members: agentMembersForProjection,
  schemas: agentSchemasForProjection,
  agentView: hydratedCatalogs.agentView,
}).member, agentMembersForProjection[1]);
const emptyAgentSelection = controller.agentSelectionState({
  selection: null,
  members: agentMembersForProjection,
  schemas: agentSchemasForProjection,
  agentView: hydratedCatalogs.agentView,
});
assert.equal(emptyAgentSelection.emptyState.title, "AGENT LIBRARY");
assert.deepEqual(emptyAgentSelection.emptyState.lines, [
  "Select an agent or schema on the left.",
  "Agents are reusable across topologies. Edit one here and every placement updates.",
]);
assert.equal(controller.agentSelectionState({
  selection: { kind: "schema", id: "Missing" },
  members: agentMembersForProjection,
  schemas: agentSchemasForProjection,
  agentView: hydratedCatalogs.agentView,
}).missing, true);
assert.equal(controller.agentSelectionState({
  selection: { kind: "schema", id: "Missing" },
  members: agentMembersForProjection,
  schemas: agentSchemasForProjection,
  agentView: hydratedCatalogs.agentView,
}).missingSchemaLabel, "Schema not found.");
assert.equal(controller.agentSelectionState({
  selection: { kind: "agent", id: "Missing" },
  members: agentMembersForProjection,
  schemas: agentSchemasForProjection,
  agentView: hydratedCatalogs.agentView,
}).missingAgentLabel, "Agent not found.");
const agentEditorState = controller.agentEditorControlState({
  member: agentMembersForProjection[1],
  instances: agentInstancesForProjection,
  schemas: agentSchemasForProjection,
  modelCatalog: [{ id: "gpt-5.5", label: "GPT-5.5", vendor: "OpenAI" }],
  agentDetailView: {
    usedInLabel: "placed in",
    instanceSingular: "slot",
    instancePlural: "slots",
    deleteLabel: "REMOVE",
    deleteConfirmIntro: "Remove agent",
    deleteConfirmPlacedPrefix: "It appears in",
    cellSingular: "cell",
    cellPlural: "cells",
    deleteConfirmCellsSuffix: "graph placements will be removed.",
    usageTitlePrefix: "PLACEMENTS",
    emptyUsageHint: "No graph placements.",
    identityTitle: "PROFILE",
    profileBindingLabel: "Binding",
    missingProfileBindingLabel: "missing binding",
    realmProfileLabel: "Realm ref",
    realmProfilePlaceholder: "realm/profile",
    realmProfileImportHintFallback: "Realm refs are import-only.",
    realmProfileTitle: "REALM REF",
    realmProfileReferenceHintBefore: "Imported profile",
    realmProfileReferenceHintAfterFallback: "requires inline conversion.",
    modelLabel: "Model id",
    runtimeModeLabel: "Runtime",
    missingRuntimeModeLabel: "missing runtime",
    backendLabel: "Profile backend",
    backendDefinitionDefaultLabel: "use definition backend",
    inlinePeerNotificationsLabel: "Inline notifications",
    inlinePeerNotificationsPlaceholder: "default",
    providerParamsLabel: "Provider settings",
    providerParamsPlaceholder: '{"top_k":20}',
    providerParamsRows: 6,
    providerParamsInvalidJsonLabel: "bad JSON",
    providerParamsObjectRequiredError: "provider settings must be an object",
    systemPromptTitle: "PEER DESCRIPTION",
    applySkeletonLabel: "SKELETON",
    applySkeletonTitle: "Apply skeleton",
    systemPromptPlaceholder: "Describe the mandate.",
    outputSchemaTitle: "OUTPUT CONTRACT",
    schemaNoneLabel: "none",
    schemaRequiredLabel: "required",
    editSchemaLabel: "Open schema",
    emptySchemaHint: "Free-form output.",
  },
  deploySettings: { surface: "cli" },
  contract: {
    mob_definition: {
      profile_binding: ["inline"],
      profile_binding_restrictions: {
        realm_profile: {
          deployable: false,
          label: "realm_profile — import-only; rkat mob validate forbids realm refs in packs",
          reason: "rkat mob validate rejects mobpack profiles that use realm_profile references; export deployable packs with inline profiles.",
        },
      },
      runtime_modes: ["turn_driven"],
      profile_backends: ["session", "external"],
    },
  },
});
assert.equal(agentEditorState.placedAt.length, 2);
assert.equal(agentEditorState.placedCount, 2);
assert.equal(agentEditorState.idLine, "m_reviewer · placed in 2 slots");
assert.equal(agentEditorState.deleteLabel, "REMOVE");
assert.equal(agentEditorState.deleteNeedsConfirmation, true);
assert.equal(agentEditorState.deleteConfirmMessage, "Remove agent \"Reviewer\"? It appears in 2 cells - graph placements will be removed.");
assert.equal(agentEditorState.usageTitle, "PLACEMENTS · 2");
assert.equal(agentEditorState.emptyUsageHint, "No graph placements.");
assert.deepEqual(agentEditorState.usageRows.map((row) => [row.id, row.cellLabel, row.laneLabel]), [
  ["n_review_1", "cell (2,3)", "main"],
  ["n_review_2", "cell (3,4)", "—"],
]);
assert.equal(agentEditorState.identityTitle, "PROFILE");
assert.equal(agentEditorState.profileBindingLabel, "Binding");
assert.equal(agentEditorState.realmProfileLabel, "Realm ref");
assert.equal(agentEditorState.realmProfilePlaceholder, "realm/profile");
assert.equal(agentEditorState.realmProfileImportHint, "rkat mob validate rejects mobpack profiles that use realm_profile references; export deployable packs with inline profiles.");
assert.equal(agentEditorState.realmProfileTitle, "REALM REF");
assert.equal(agentEditorState.realmProfileReferenceLabel, "reviewer");
assert.equal(agentEditorState.realmProfileReferenceHintBefore, "Imported profile");
assert.equal(agentEditorState.realmProfileReferenceHintAfter, "from a target realm. rkat mob validate rejects mobpack profiles that use realm_profile references; export deployable packs with inline profiles.");
assert.equal(agentEditorState.modelLabel, "Model id");
assert.equal(agentEditorState.runtimeModeLabel, "Runtime");
assert.equal(agentEditorState.backendLabel, "Profile backend");
assert.equal(agentEditorState.inlinePeerNotificationsLabel, "Inline notifications");
assert.equal(agentEditorState.inlinePeerNotificationsPlaceholder, "default");
assert.equal(agentEditorState.systemPromptTitle, "PEER DESCRIPTION");
assert.equal(agentEditorState.applySkeletonLabel, "SKELETON");
assert.equal(agentEditorState.applySkeletonTitle, "Apply skeleton");
assert.equal(agentEditorState.systemPromptPlaceholder, "Describe the mandate.");
assert.deepEqual(controller.agentEditorControlState({
  member: { id: "m_unplaced", name: "Unplaced", role: "writer" },
  instances: [],
  schemas: [],
}).deleteNeedsConfirmation, false);
assert.equal(agentEditorState.schema.id, "ReviewArtifact");
assert.equal(agentEditorState.outputSchemaTitle, "OUTPUT CONTRACT");
assert.equal(agentEditorState.hasOutputSchema, true);
assert.deepEqual(agentEditorState.schemaPreviewRows, [
  { id: "f1", name: undefined, type: undefined, required: false, requiredLabel: "" },
  { id: "f2", name: undefined, type: undefined, required: false, requiredLabel: "" },
]);
assert.equal(agentEditorState.editSchemaLabel, "Open schema");
assert.deepEqual(agentEditorState.editSchemaSelection, { kind: "schema", id: "ReviewArtifact" });
assert.equal(agentEditorState.emptySchemaHint, "Free-form output.");
assert.equal(agentEditorState.profileBinding, "inline");
assert.equal(agentEditorState.runtimeMode, "turn_driven");
assert.equal(agentEditorState.backendValue, "session");
assert.deepEqual(agentEditorState.backendOptions.map((option) => [option.value, option.label]), [
  ["", "use definition backend"],
  ["session", "session"],
  ["external", "external"],
]);
assert.deepEqual(agentEditorState.modelOptions.map((option) => option.label), ["GPT-5.5 · OpenAI"]);
assert.deepEqual(controller.agentEditorControlState({
  member: { id: "m_custom", model: "custom/model" },
  modelCatalog: [],
}).modelOptions, [{ value: "custom/model", label: "custom/model", model: null }]);
assert.deepEqual(agentEditorState.schemaOptions.map((option) => [option.value, option.label]), [
  ["", "none"],
  ["PlanArtifact", "PlanArtifact"],
  ["ReviewArtifact", "ReviewArtifact"],
]);
assert.deepEqual(controller.agentDefinitionOptions([{
  id: "planner",
  role: "planner",
}, {
  id: "reviewer",
  role: "reviewer",
  label: "Quality Reviewer",
}, {
  role: "missing-id",
}]), {
  hasDefinitions: true,
  optionRows: [
    {
      value: "planner",
      label: "planner",
      definition: { id: "planner", role: "planner" },
    },
    {
      value: "reviewer",
      label: "Quality Reviewer",
      definition: { id: "reviewer", role: "reviewer", label: "Quality Reviewer" },
    },
  ],
});
assert.deepEqual(controller.agentDefinitionOptions([]), { hasDefinitions: false, optionRows: [] });
assert.deepEqual(controller.agentDefinitionAddControlState([], hydratedCatalogs.agentView), {
  hasDefinitions: false,
  optionRows: [],
  controlClass: "agents-list__add",
  disabled: true,
  title: "MobKit schema contract has not provided agent definitions yet.",
  unavailableLabel: "agents unavailable",
  placeholderOption: { value: "", label: "+ new agent..." },
  value: "",
});
assert.deepEqual(controller.agentDefinitionAddControlState([{
  id: "reviewer",
  role: "reviewer",
  label: "Quality Reviewer",
}], hydratedCatalogs.agentView), {
  hasDefinitions: true,
  optionRows: [{
    value: "reviewer",
    label: "Quality Reviewer",
    definition: { id: "reviewer", role: "reviewer", label: "Quality Reviewer" },
  }],
  controlClass: "agents-list__add agents-list__add--select",
  disabled: false,
  title: "Create an agent from a MobKit profile-member definition.",
  unavailableLabel: "agents unavailable",
  placeholderOption: { value: "", label: "+ new agent..." },
  value: "",
});
assert.deepEqual(controller.schemaEditorControlState({
  schema: agentSchemasForProjection[1],
  members: agentMembersForProjection,
  schemaView: {
    eyebrow: "SCHEMA CONTRACT",
    descriptionTitle: "SUMMARY",
    descriptionPlaceholder: "Describe emitted data.",
    fieldsTitlePrefix: "COLUMNS",
    addFieldLabel: "+ column",
    headerLabels: {
      name: "FIELD",
      type: "KIND",
      required: "NEED",
      description: "DETAIL",
      action: "",
    },
    emptyFieldsHint: "No contract fields.",
    usedByPrefix: "REFERENCED BY",
    emptyUsedByHint: "No agent references.",
    deleteLabel: "REMOVE",
    deleteBlockedTitle: "Clear agent schema refs first",
  },
}), {
  eyebrow: "SCHEMA CONTRACT",
  descriptionTitle: "SUMMARY",
  descriptionPlaceholder: "Describe emitted data.",
  fieldsTitle: "COLUMNS · 2",
  addFieldLabel: "+ column",
  headerLabels: {
    name: "FIELD",
    type: "KIND",
    required: "NEED",
    description: "DETAIL",
    action: "",
  },
  fieldRows: [
    { id: "f1", field: agentSchemasForProjection[1].fields[0] },
    { id: "f2", field: agentSchemasForProjection[1].fields[1] },
  ],
  emptyFieldsHint: "No contract fields.",
  usedBy: [{
    id: "m_reviewer",
    name: "Reviewer",
    role: "reviewer",
    model: "gpt-5.5",
    selection: { kind: "agent", id: "m_reviewer" },
    member: agentMembersForProjection[1],
  }],
  usedCount: 1,
  usageLabel: "used by 1 agent",
  usedByTitle: "REFERENCED BY · 1",
  emptyUsedByHint: "No agent references.",
  deleteLabel: "REMOVE",
  canDelete: false,
  deleteTitle: "Clear agent schema refs first",
});

const schemaDraftContract = {
  mob_definition: {
    defaults: { schema_field_type: "enum" },
    editor_schema_field_types: ["enum", "string"],
    editor_schema_draft: {
      schema_id_prefix: "Artifact",
      initial_field: {
        name: "field_one",
        required: true,
        description: "",
        enumValues: [],
      },
      added_field: {
        name: "new_field",
        required: false,
        description: "",
        enumValues: [],
      },
    },
  },
};
assert.deepEqual(controller.schemaDefinitionAddPatch([{ id: "Artifact1" }], schemaDraftContract), {
  schema: {
    id: "Artifact2",
    description: "",
    fields: [{ id: "f1", name: "field_one", type: "enum", required: true, description: "", enumValues: [] }],
  },
  schemas: [
    { id: "Artifact1" },
    { id: "Artifact2", description: "", fields: [{ id: "f1", name: "field_one", type: "enum", required: true, description: "", enumValues: [] }] },
  ],
});
assert.deepEqual(controller.schemaDescriptionPatch(" Review output.\n"), { description: " Review output.\n" });
assert.equal(controller.uniqueSchemaFieldName([
  { id: "f1", name: "field" },
  { id: "f2", name: "field_2" },
], "9 field!"), "_9_field");
assert.deepEqual(controller.schemaFieldAddPatch({
  fields: [{ id: "f1", name: "new_field", type: "string" }],
}, schemaDraftContract), {
  field: { id: "f2", name: "new_field_2", type: "enum", required: false, description: "", enumValues: [] },
  patch: { fields: [
    { id: "f1", name: "new_field", type: "string" },
    { id: "f2", name: "new_field_2", type: "enum", required: false, description: "", enumValues: [] },
  ] },
});
assert.deepEqual(controller.schemaDefinitionAddPatch([], {
  mob_definition: {
    defaults: { schema_field_type: "enum" },
    editor_schema_field_types: ["enum", "string"],
  },
}), {
  ok: false,
  error: "MobKit schema is missing mob_definition.editor_schema_draft",
  schemas: [],
});
assert.deepEqual(controller.schemaFieldAddPatch({
  fields: [{ id: "f1", name: "existing", type: "string" }],
}, {
  mob_definition: {
    defaults: { schema_field_type: "enum" },
    editor_schema_field_types: ["enum", "string"],
  },
}), {
  ok: false,
  error: "MobKit schema is missing mob_definition.editor_schema_draft",
  patch: { fields: [{ id: "f1", name: "existing", type: "string" }] },
});
assert.deepEqual(controller.schemaFieldUpdatePatch({
  fields: [{ id: "f1", name: "old", type: "string" }],
}, "f1", { name: "new" }, schemaDraftContract), {
  fields: [{ id: "f1", name: "new", type: "string" }],
});
assert.deepEqual(controller.schemaFieldUpdatePatch({
  fields: [{ id: "f1", name: "old", type: "string" }],
}, "f1", { name: "" }, {
  mob_definition: {
    defaults: { schema_field_type: "string" },
    editor_schema_field_types: ["string"],
    editor_schema_draft: {
      schema_id_prefix: "Result",
      initial_field: { name: "result_field", required: true, description: "", enumValues: [] },
      added_field: { name: "result_field", required: false, description: "", enumValues: [] },
    },
  },
}), {
  fields: [{ id: "f1", name: "result_field", type: "string" }],
});
assert.deepEqual(controller.schemaFieldUpdatePatch({
  fields: [
    { id: "f1", name: "old", type: "string" },
    { id: "f2", name: "new", type: "string" },
  ],
}, "f1", { name: "new" }, schemaDraftContract), {
  fields: [
    { id: "f1", name: "new_2", type: "string" },
    { id: "f2", name: "new", type: "string" },
  ],
});
assert.deepEqual(controller.schemaFieldUpdatePatch({
  fields: [{ id: "f1", name: "old", type: "string" }],
}, "f1", { name: "" }, schemaDraftContract), {
  fields: [{ id: "f1", name: "new_field", type: "string" }],
});
assert.deepEqual(controller.schemaFieldUpdatePatch({
  fields: [{ id: "f1", name: "old", type: "string", enumValues: [] }],
}, "f1", { type: "object" }, schemaDraftContract), {
  fields: [{ id: "f1", name: "old", type: "string", enumValues: [] }],
});
assert.deepEqual(controller.schemaFieldUpdatePatch({
  fields: [{ id: "f1", name: "old", type: "string", enumValues: [] }],
}, "f1", { type: "enum" }, schemaDraftContract), {
  fields: [{ id: "f1", name: "old", type: "enum", enumValues: ["value"] }],
});
assert.deepEqual(controller.schemaFieldDeletePatch({
  fields: [{ id: "f1", name: "old" }, { id: "f2", name: "keep" }],
}, "f1"), {
  removed: { id: "f1", name: "old" },
  patch: { fields: [{ id: "f2", name: "keep" }] },
});
const deletedSchemaFieldCascade = controller.schemaFieldDeleteCascadePatch({
  schema: {
    id: "ReviewArtifact",
    fields: [{ id: "f1", name: "verdict" }, { id: "f2", name: "summary" }],
  },
  schemas: [
    { id: "ReviewArtifact", fields: [{ id: "f1", name: "verdict" }, { id: "f2", name: "summary" }] },
  ],
  members: [{ id: "m_reviewer", schema: "ReviewArtifact" }],
  flow: {
    name: "field-delete-cascade",
    steps: [
      { id: "review_step", type: "member", role: "m_reviewer" },
      {
        id: "route",
        type: "branch",
        branches: [{
          id: "br_green",
          cond: { stepId: "review_step", field: "verdict", op: "==", val: "green" },
          condition: "steps.review_step.verdict == \"green\"",
          steps: [],
        }],
        fallback: [],
      },
    ],
  },
  edges: [{
    id: "e_review_done",
    from: "review_inst",
    to: "done",
    kind: "cond",
    label: "steps.review_inst.verdict == \"green\"",
    cond: { var: "steps.review_inst.verdict", op: "==", val: "green" },
  }],
  instances: [
    { id: "review_inst", memberId: "m_reviewer" },
    { id: "done", isTerminal: true },
  ],
}, "f1");
assert.deepEqual(deletedSchemaFieldCascade.removed, { id: "f1", name: "verdict" });
assert.deepEqual(deletedSchemaFieldCascade.schema.fields, [{ id: "f2", name: "summary" }]);
assert.deepEqual(deletedSchemaFieldCascade.schemas[0].fields, [{ id: "f2", name: "summary" }]);
assert.deepEqual(deletedSchemaFieldCascade.flow.steps[1].branches[0].cond, {});
assert.equal(deletedSchemaFieldCascade.flow.steps[1].branches[0].condition, "");
assert.deepEqual(deletedSchemaFieldCascade.edges[0].cond, null);
assert.equal(deletedSchemaFieldCascade.edges[0].label, "");

const renamedSchemaFieldCascade = controller.schemaFieldRenameCascadePatch({
  schema: {
    id: "ReviewArtifact",
    fields: [{ id: "f1", name: "verdict", type: "enum" }, { id: "f2", name: "summary", type: "string" }],
  },
  schemas: [
    { id: "ReviewArtifact", fields: [{ id: "f1", name: "verdict", type: "enum" }, { id: "f2", name: "summary", type: "string" }] },
  ],
  members: [{ id: "m_reviewer", schema: "ReviewArtifact" }],
  flow: {
    name: "field-rename-cascade",
    steps: [
      { id: "review_step", type: "member", role: "m_reviewer" },
      {
        id: "route",
        type: "branch",
        branches: [{
          id: "br_green",
          cond: { stepId: "review_step", field: "verdict", op: "==", val: "green" },
          condition: "steps.review_step.verdict == \"green\"",
          steps: [],
        }],
        fallback: [],
      },
    ],
  },
  edges: [{
    id: "e_review_done",
    from: "review_inst",
    to: "done",
    kind: "cond",
    label: "steps.review_inst.verdict == \"green\"",
    cond: { var: "steps.review_inst.verdict", op: "==", val: "green" },
  }],
  instances: [
    { id: "review_inst", memberId: "m_reviewer" },
    { id: "done", isTerminal: true },
  ],
}, "f1", "outcome", "verdict", schemaDraftContract);
assert.equal(renamedSchemaFieldCascade.schema.fields[0].name, "outcome");
assert.equal(renamedSchemaFieldCascade.schemas[0].fields[0].name, "outcome");
assert.equal(renamedSchemaFieldCascade.flow.steps[1].branches[0].cond.field, "outcome");
assert.equal(renamedSchemaFieldCascade.flow.steps[1].branches[0].condition, "steps.review_step.outcome == \"green\"");
assert.deepEqual(renamedSchemaFieldCascade.edges[0].cond, { var: "steps.review_inst.outcome", op: "==", val: "green" });
assert.equal(renamedSchemaFieldCascade.edges[0].label, "steps.review_inst.outcome == \"green\"");

const directMemberContract = {
  mob_definition: {
    profile_binding: ["inline"],
    profile_binding_restrictions: {
      inline: { deployable: true },
    },
    runtime_modes: ["turn_driven", "autonomous_host"],
  },
};

assert.deepEqual(controller.studioAddMemberPatch({
  members: [{ id: "m_old" }],
}, { id: "m_new" }), {
  ok: false,
  error: "member must include id and role/name",
  members: [{ id: "m_old" }],
  member: null,
});
assert.deepEqual(controller.studioAddMemberPatch({
  members: [{ id: "m_old" }],
  contract: directMemberContract,
}, {
  id: "m_new",
  name: "New",
  role: "new",
  model: "gpt-5.5",
  profileBinding: "inline",
  runtimeMode: "turn_driven",
  tools: [],
  skills: [],
}), {
  ok: true,
  error: "",
  members: [
    { id: "m_old" },
    {
      id: "m_new",
      name: "New",
      role: "new",
      model: "gpt-5.5",
      profileBinding: "inline",
      runtimeMode: "turn_driven",
      tools: [],
      skills: [],
    },
  ],
  member: {
    id: "m_new",
    name: "New",
    role: "new",
    model: "gpt-5.5",
    profileBinding: "inline",
    runtimeMode: "turn_driven",
    tools: [],
    skills: [],
  },
});
assert.deepEqual(controller.studioAddMemberPatch({
  members: [{ id: "m_old" }],
  contract: directMemberContract,
}, {
  id: "m_old",
  name: "Duplicate",
  role: "duplicate",
  model: "gpt-5.5",
  profileBinding: "inline",
  runtimeMode: "turn_driven",
}), {
  ok: false,
  error: "member id already exists",
  members: [{ id: "m_old" }],
  member: null,
});
assert.deepEqual(controller.studioAddMemberPatch({
  members: [{ id: "m_old" }],
}, {
  id: "m_new",
  name: "New",
  role: "new",
  model: "gpt-5.5",
  profileBinding: "inline",
  runtimeMode: "turn_driven",
}), {
  ok: false,
  error: "MobKit schema contract must allow deployable inline profileBinding",
  members: [{ id: "m_old" }],
  member: null,
});
assert.deepEqual(controller.studioAddMemberPatch({
  members: [{ id: "m_old" }],
  contract: {
    mob_definition: {
      profile_binding: ["inline"],
      profile_binding_restrictions: { inline: { deployable: true } },
      runtime_modes: ["turn_driven"],
    },
  },
}, {
  id: "m_new",
  name: "New",
  role: "new",
  model: "gpt-5.5",
  profileBinding: "inline",
  runtimeMode: "autonomous_host",
}), {
  ok: false,
  error: "member runtimeMode must be allowed by mob_definition.runtime_modes",
  members: [{ id: "m_old" }],
  member: null,
});
assert.deepEqual(controller.studioUpdateMemberPatch({
  members: [{ id: "m_old", name: "Old" }],
}, "m_old", { name: "New" }), {
  ok: true,
  error: "",
  members: [{ id: "m_old", name: "New" }],
  member: { id: "m_old", name: "New" },
});
assert.deepEqual(controller.studioUpdateMemberPatch({
  members: [{ id: "m_old", name: "Old" }],
}, "m_old", { id: "m_new" }), {
  ok: false,
  error: "member id changes must use projection reconciliation",
  members: [{ id: "m_old", name: "Old" }],
});
assert.deepEqual(controller.studioUpdateMemberPatch({
  members: [{ id: "m_real", name: "Real", role: "real", model: "gpt-5.5", profileBinding: "inline", runtimeMode: "turn_driven" }],
  contract: directMemberContract,
}, "m_real", { model: "" }), {
  ok: false,
  error: "inline member updates must keep a model",
  members: [{ id: "m_real", name: "Real", role: "real", model: "gpt-5.5", profileBinding: "inline", runtimeMode: "turn_driven" }],
});
assert.deepEqual(controller.studioUpdateMemberPatch({
  members: [{ id: "m_real", name: "Real", role: "real", model: "gpt-5.5", profileBinding: "inline", runtimeMode: "turn_driven" }],
  contract: directMemberContract,
}, "m_real", { profileBinding: "realm_profile" }), {
  ok: false,
  error: "member updates must keep deployable inline profileBinding",
  members: [{ id: "m_real", name: "Real", role: "real", model: "gpt-5.5", profileBinding: "inline", runtimeMode: "turn_driven" }],
});
assert.deepEqual(controller.studioUpdateMemberPatch({
  members: [{ id: "m_real", name: "Real", role: "real", model: "gpt-5.5", profileBinding: "inline", runtimeMode: "turn_driven" }],
  contract: directMemberContract,
}, "m_real", { runtimeMode: "daemon" }), {
  ok: false,
  error: "member updates must use a mob_definition.runtime_modes value",
  members: [{ id: "m_real", name: "Real", role: "real", model: "gpt-5.5", profileBinding: "inline", runtimeMode: "turn_driven" }],
});
assert.deepEqual(controller.studioUpdateMemberPatch({
  members: [{ id: "m_real", name: "Real", role: "real", model: "gpt-5.5", profileBinding: "inline", runtimeMode: "turn_driven" }],
}, "m_real", { runtimeMode: "turn_driven" }), {
  ok: false,
  error: "MobKit schema contract must allow deployable inline profileBinding",
  members: [{ id: "m_real", name: "Real", role: "real", model: "gpt-5.5", profileBinding: "inline", runtimeMode: "turn_driven" }],
});
assert.equal(controller.studioUpdateMemberPatch({
  members: [{ id: "m_real", name: "Real", role: "real", model: "gpt-5.5", profileBinding: "inline", runtimeMode: "turn_driven" }],
  contract: directMemberContract,
}, "m_real", { runtimeMode: "autonomous_host" }).ok, true);
assert.deepEqual(controller.studioUpdateMemberPatch({
  members: [{ id: "m_real", name: "Real", role: "real", model: "gpt-5.5", profileBinding: "inline", runtimeMode: "turn_driven", tools: [] }],
}, "m_real", { tools: "shell" }), {
  ok: false,
  error: "member tools must be an array of non-empty strings",
  members: [{ id: "m_real", name: "Real", role: "real", model: "gpt-5.5", profileBinding: "inline", runtimeMode: "turn_driven", tools: [] }],
});
assert.deepEqual(controller.studioUpdateMemberPatch({
  members: [{ id: "m_real", name: "Real", role: "real", model: "gpt-5.5", profileBinding: "inline", runtimeMode: "turn_driven", skills: [] }],
}, "m_real", { skills: ["mob.review", ""] }), {
  ok: false,
  error: "member skills must be an array of non-empty strings",
  members: [{ id: "m_real", name: "Real", role: "real", model: "gpt-5.5", profileBinding: "inline", runtimeMode: "turn_driven", skills: [] }],
});
assert.deepEqual(controller.studioUpdateMemberPatch({
  members: [{ id: "m_real", name: "Real", role: "real", model: "gpt-5.5", profileBinding: "inline", runtimeMode: "turn_driven", providerParams: null }],
}, "m_real", { providerParams: ["bad"] }), {
  ok: false,
  error: "member providerParams must be a JSON object",
  members: [{ id: "m_real", name: "Real", role: "real", model: "gpt-5.5", profileBinding: "inline", runtimeMode: "turn_driven", providerParams: null }],
});
assert.deepEqual(controller.studioUpdateMemberPatch({
  members: [{ id: "m_real", name: "Real", role: "real", model: "gpt-5.5", profileBinding: "inline", runtimeMode: "turn_driven", maxInlinePeerNotifications: null }],
}, "m_real", { maxInlinePeerNotifications: "many" }), {
  ok: false,
  error: "member maxInlinePeerNotifications must be an integer >= -1 or blank",
  members: [{ id: "m_real", name: "Real", role: "real", model: "gpt-5.5", profileBinding: "inline", runtimeMode: "turn_driven", maxInlinePeerNotifications: null }],
});
assert.equal(controller.studioUpdateMemberPatch({
  members: [{ id: "m_real", name: "Real", role: "real", model: "gpt-5.5", profileBinding: "inline", runtimeMode: "turn_driven", tools: [], skills: [], providerParams: null, maxInlinePeerNotifications: null }],
}, "m_real", { tools: ["shell"], skills: ["mob.review"], providerParams: { thinking_budget: 2048 }, maxInlinePeerNotifications: -1 }).ok, true);
assert.deepEqual(controller.studioDeleteMemberPatch({
  members: [{ id: "m_keep" }, { id: "m_drop" }],
  instances: [
    { id: "i_keep", memberId: "m_keep" },
    { id: "i_drop", memberId: "m_drop" },
    { id: "done", isTerminal: true },
  ],
  edges: [
    { id: "e_keep", from: "i_keep", to: "done" },
    { id: "e_drop_out", from: "i_drop", to: "done" },
    { id: "e_drop_in", from: "i_keep", to: "i_drop" },
  ],
}, "m_drop"), {
  members: [{ id: "m_keep" }],
  instances: [
    { id: "i_keep", memberId: "m_keep" },
    { id: "done", isTerminal: true },
  ],
  edges: [{ id: "e_keep", from: "i_keep", to: "done" }],
});
const memberDeleteCascade = controller.memberDeleteCascadePatch({
  memberId: "m_drop",
  members: [
    { id: "m_keep", name: "Keep", role: "keep", tools: ["shell"], schema: "SummaryArtifact" },
    { id: "m_drop", name: "Drop", role: "drop", tools: ["git"], schema: "ReviewArtifact" },
  ],
  flow: {
    name: "delete-cascade-proof",
    steps: [
      { id: "keep_step", type: "member", role: "m_keep", allowedTools: ["shell"] },
      { id: "drop_step", type: "member", role: "m_drop", allowedTools: ["git"] },
      {
        id: "parallel_step",
        type: "parallel",
        controllerRole: "m_drop",
        branches: [{
          id: "fork_branch",
          steps: [{ id: "fork_keep", type: "member", role: "m_keep", launchMode: { kind: "Fork", from: "drop_step" } }],
        }],
      },
    ],
  },
  instances: [
    { id: "i_keep", memberId: "m_keep", allowedTools: ["shell"] },
    { id: "i_drop", memberId: "m_drop", allowedTools: ["git"] },
    { id: "join", isGate: true, gateKind: "join", controllerRole: "m_drop" },
    { id: "fork_keep", memberId: "m_keep", launchMode: { kind: "Fork", from: "i_drop" } },
  ],
  edges: [
    { id: "keep_join", from: "i_keep", to: "join" },
    { id: "drop_join", from: "i_drop", to: "join" },
    { id: "join_fork", from: "join", to: "fork_keep" },
  ],
  mobSettings: {
    orchestrator: "drop",
    roleWiring: [{ a: "keep", b: "drop" }],
    backendDefault: "session",
  },
});
assert.equal(memberDeleteCascade.ok, true);
assert.deepEqual(memberDeleteCascade.members.map((member) => member.id), ["m_keep"]);
assert.deepEqual(memberDeleteCascade.flow.steps.map((step) => step.id), ["keep_step", "parallel_step"]);
assert(!("controllerRole" in memberDeleteCascade.flow.steps[1]));
assert.deepEqual(memberDeleteCascade.flow.steps[1].branches[0].steps[0].launchMode, { kind: "Fresh" });
assert.deepEqual(memberDeleteCascade.instances.map((instance) => instance.id), ["i_keep", "join", "fork_keep"]);
assert(!("controllerRole" in memberDeleteCascade.instances[1]));
assert.deepEqual(memberDeleteCascade.instances[2].launchMode, { kind: "Fresh" });
assert.deepEqual(memberDeleteCascade.edges.map((edge) => edge.id), ["keep_join", "join_fork"]);
assert.equal(memberDeleteCascade.mobSettings.orchestrator, "");
assert.deepEqual(memberDeleteCascade.mobSettings.roleWiring, []);
assert.equal(controller.memberDeleteCascadePatch({ memberId: "missing", members: [{ id: "m_keep" }] }).ok, false);
assert.deepEqual(controller.studioAddInstancePatch({
  instances: [{ id: "a", memberId: "m_existing" }],
  members: [{ id: "m_existing" }, { id: "m_new" }],
}, { id: "b", memberId: "m_new" }), {
  ok: true,
  error: "",
  instances: [{ id: "a", memberId: "m_existing" }, { id: "b", memberId: "m_new" }],
  instance: { id: "b", memberId: "m_new" },
});
assert.deepEqual(controller.studioAddInstancePatch({
  instances: [{ id: "a", memberId: "m_existing" }],
  members: [{ id: "m_existing" }],
}, { id: "b", memberId: "m_missing" }), {
  ok: false,
  error: "member graph node must reference an existing member",
  instances: [{ id: "a", memberId: "m_existing" }],
  instance: null,
});
assert.deepEqual(controller.studioAddInstancePatch({
  instances: [{ id: "a", memberId: "m_existing" }],
  members: [{ id: "m_existing" }],
}, { id: "a", isTerminal: true }), {
  ok: false,
  error: "graph node id already exists",
  instances: [{ id: "a", memberId: "m_existing" }],
  instance: null,
});
assert.deepEqual(controller.studioAddInstancePatch({
  instances: [{ id: "a", memberId: "m_existing" }],
  members: [{ id: "m_existing" }],
}, { id: "done", isTerminal: true }), {
  ok: true,
  error: "",
  instances: [{ id: "a", memberId: "m_existing" }, { id: "done", isTerminal: true }],
  instance: { id: "done", isTerminal: true },
});
assert.deepEqual(controller.studioAppendInstancesPatch({
  instances: [{ id: "a", memberId: "m_a" }],
  members: [{ id: "m_a" }, { id: "m_b" }],
}, [{ id: "b", memberId: "m_b" }, { id: "ghost", memberId: "m_missing" }, { id: "done", isTerminal: true }]), {
  instances: [
    { id: "a", memberId: "m_a" },
    { id: "b", memberId: "m_b" },
    { id: "done", isTerminal: true },
  ],
});
assert.deepEqual(controller.studioUpdateInstancePatch({
  instances: [{ id: "a", memberId: "m_a", col: 1 }],
  members: [{ id: "m_a" }],
}, "a", { col: 2 }), {
  ok: true,
  error: "",
  instances: [{ id: "a", memberId: "m_a", col: 2 }],
  instance: { id: "a", memberId: "m_a", col: 2 },
});
assert.deepEqual(controller.studioUpdateInstancePatch({
  instances: [{ id: "a", memberId: "m_a", col: 1 }, { id: "b", isTerminal: true }],
  members: [{ id: "m_a" }],
}, "a", { memberId: "m_missing" }), {
  ok: false,
  error: "member graph node must reference an existing member",
  instances: [{ id: "a", memberId: "m_a", col: 1 }, { id: "b", isTerminal: true }],
});
assert.deepEqual(controller.studioUpdateInstancePatch({
  instances: [{ id: "a", memberId: "m_a", col: 1 }, { id: "b", isTerminal: true }],
  members: [{ id: "m_a" }],
}, "a", { id: "b" }), {
  ok: false,
  error: "graph node id already exists",
  instances: [{ id: "a", memberId: "m_a", col: 1 }, { id: "b", isTerminal: true }],
});
assert.deepEqual(controller.studioMoveInstancePatch({
  instances: [
    { id: "a", col: 0, row: 0 },
    { id: "b", col: 2, row: 2 },
  ],
}, "a", { col: 2, row: 2 }, { col: 0, row: 0 }), {
  instances: [
    { id: "a", col: 2, row: 2 },
    { id: "b", col: 0, row: 0 },
  ],
});
assert.deepEqual(controller.studioMoveInstancePatch({
  instances: [{ id: "a", col: 0, row: 0 }],
}, "missing", { col: 1, row: 1 }, { col: 0, row: 0 }), {
  instances: [{ id: "a", col: 0, row: 0 }],
});
assert.deepEqual(controller.studioDeleteInstancePatch({
  instances: [{ id: "a" }, { id: "b" }, { id: "c" }],
  edges: [{ id: "ab", from: "a", to: "b" }, { id: "ca", from: "c", to: "a" }, { id: "bc", from: "b", to: "c" }],
}, "a"), {
  instances: [{ id: "b" }, { id: "c" }],
  edges: [{ id: "bc", from: "b", to: "c" }],
});
assert.deepEqual(controller.studioDeleteInstancePatch({
  instances: [
    { id: "source", memberId: "m_source" },
    { id: "review", memberId: "m_review", launchMode: { kind: "Fork", from: "source", context: "full_history", budgetSplitPolicy: { kind: "Fixed", limit: 512 } } },
    { id: "done", isTerminal: true },
  ],
  edges: [
    { id: "review_done", from: "review", to: "done", kind: "cond", label: 'steps.source.verdict == "green"', cond: { var: "steps.source.verdict", op: "==", val: "green" } },
    { id: "source_done", from: "source", to: "done", kind: "next" },
  ],
}, "source"), {
  instances: [
    { id: "review", memberId: "m_review", launchMode: { kind: "Fresh", budgetSplitPolicy: { kind: "Fixed", limit: 512 } } },
    { id: "done", isTerminal: true },
  ],
  edges: [
    { id: "review_done", from: "review", to: "done", kind: "cond", label: "", cond: null },
  ],
});
assert.deepEqual(controller.studioAddEdgePatch({
  edges: [{ id: "ab" }],
  instances: [{ id: "b" }, { id: "c" }],
}, { id: "bc" }), {
  ok: false,
  error: "edge must include id, from, and to",
  edges: [{ id: "ab" }],
  edge: null,
});
assert.deepEqual(controller.studioAddEdgePatch({
  edges: [{ id: "ab", from: "a", to: "b" }],
  instances: [{ id: "a" }, { id: "b" }, { id: "c" }],
}, { id: "bc", from: "b", to: "c" }), {
  ok: true,
  error: "",
  edges: [{ id: "ab", from: "a", to: "b" }, { id: "bc", from: "b", to: "c" }],
  edge: { id: "bc", from: "b", to: "c" },
});
assert.deepEqual(controller.studioAddEdgePatch({
  edges: [{ id: "ab", from: "a", to: "b" }],
  instances: [{ id: "a" }, { id: "b" }],
}, { id: "ghost", from: "a", to: "missing" }), {
  ok: false,
  error: "edge endpoints must reference existing graph nodes",
  edges: [{ id: "ab", from: "a", to: "b" }],
  edge: null,
});
assert.deepEqual(controller.studioAddEdgePatch({
  edges: [{ id: "ab", from: "a", to: "b" }],
  instances: [{ id: "a" }, { id: "b" }],
}, { id: "dupe", from: "a", to: "b" }), {
  ok: false,
  error: "edge already exists",
  edges: [{ id: "ab", from: "a", to: "b" }],
  edge: null,
});
assert.deepEqual(controller.studioAppendEdgesPatch({
  edges: [{ id: "ab", from: "a", to: "b" }],
  instances: [{ id: "a" }, { id: "b" }, { id: "c" }],
}, [{ id: "bc", from: "b", to: "c" }, { id: "ghost", from: "c", to: "missing" }]), {
  edges: [{ id: "ab", from: "a", to: "b" }, { id: "bc", from: "b", to: "c" }],
});
assert.deepEqual(controller.studioUpdateEdgePatch({
  edges: [{ id: "ab", label: "" }],
  instances: [{ id: "a" }, { id: "b" }],
}, "ab", { label: "next" }), {
  ok: false,
  error: "edge must include id, from, and to",
  edges: [{ id: "ab", label: "" }],
});
assert.deepEqual(controller.studioUpdateEdgePatch({
  edges: [{ id: "ab", from: "a", to: "b", label: "" }],
  instances: [{ id: "a" }, { id: "b" }],
}, "ab", { label: "next" }), {
  ok: true,
  error: "",
  edges: [{ id: "ab", from: "a", to: "b", label: "next" }],
  edge: { id: "ab", from: "a", to: "b", label: "next" },
});
assert.deepEqual(controller.studioUpdateEdgePatch({
  edges: [
    { id: "ab", from: "a", to: "b" },
    { id: "bc", from: "b", to: "c" },
  ],
  instances: [{ id: "a" }, { id: "b" }, { id: "c" }],
}, "ab", { from: "b", to: "c" }), {
  ok: false,
  error: "edge already exists",
  edges: [
    { id: "ab", from: "a", to: "b" },
    { id: "bc", from: "b", to: "c" },
  ],
});
assert.deepEqual(controller.studioDeleteEdgePatch({
  edges: [{ id: "keep" }, { id: "drop" }],
}, "drop"), {
  edges: [{ id: "keep" }],
});
assert.deepEqual(controller.studioAddSchemaPatch({
  schemas: [{ id: "PlanArtifact" }],
}, { id: "ReviewArtifact" }), {
  ok: true,
  error: "",
  schemas: [{ id: "PlanArtifact" }, { id: "ReviewArtifact" }],
  schema: { id: "ReviewArtifact" },
});
assert.deepEqual(controller.studioAddSchemaPatch({
  schemas: [{ id: "PlanArtifact" }],
}, { id: "PlanArtifact" }), {
  ok: false,
  error: "schema id already exists",
  schemas: [{ id: "PlanArtifact" }],
  schema: null,
});
assert.deepEqual(controller.studioUpdateSchemaPatch({
  schemas: [{ id: "ReviewArtifact", description: "" }],
}, "ReviewArtifact", { description: "Review output" }), {
  ok: true,
  error: "",
  schemas: [{ id: "ReviewArtifact", description: "Review output" }],
  schema: { id: "ReviewArtifact", description: "Review output" },
});
assert.deepEqual(controller.studioUpdateSchemaPatch({
  schemas: [{ id: "ReviewArtifact", description: "" }, { id: "PlanArtifact", description: "" }],
}, "ReviewArtifact", { id: "PlanArtifact" }), {
  ok: false,
  error: "schema id changes must use renameSchemaDefinition",
  schemas: [{ id: "ReviewArtifact", description: "" }, { id: "PlanArtifact", description: "" }],
});
assert.deepEqual(controller.studioDeleteSchemaPatch({
  schemas: [{ id: "ReviewArtifact" }, { id: "PlanArtifact" }],
  members: [
    { id: "m_review", schema: "ReviewArtifact" },
    { id: "m_plan", schema: "PlanArtifact" },
  ],
}, "ReviewArtifact"), {
  schemas: [{ id: "PlanArtifact" }],
  members: [
    { id: "m_review", schema: "" },
    { id: "m_plan", schema: "PlanArtifact" },
  ],
});
const schemaDeleteCascade = controller.studioDeleteSchemaPatch({
  schemas: [
    { id: "ReviewArtifact", fields: [{ id: "f1", name: "verdict", type: "enum" }] },
    { id: "PlanArtifact", fields: [{ id: "f2", name: "summary", type: "string" }] },
  ],
  members: [
    { id: "m_review", schema: "ReviewArtifact" },
    { id: "m_plan", schema: "PlanArtifact" },
  ],
  flow: {
    name: "delete-schema-cascade",
    steps: [
      { id: "review_step", type: "member", role: "m_review" },
      {
        id: "route",
        type: "branch",
        branches: [{
          id: "br_green",
          cond: { stepId: "review_step", field: "verdict", op: "==", val: "green" },
          condition: "steps.review_step.verdict == \"green\"",
          steps: [],
        }],
        fallback: [],
      },
    ],
  },
  edges: [{
    id: "e_review_done",
    from: "review_inst",
    to: "done",
    kind: "cond",
    label: "steps.review_inst.verdict == \"green\"",
    cond: { var: "steps.review_inst.verdict", op: "==", val: "green" },
  }],
  instances: [
    { id: "review_inst", memberId: "m_review", col: 0, row: 0 },
    { id: "done", isTerminal: true, col: 1, row: 0 },
  ],
}, "ReviewArtifact");
assert.deepEqual(schemaDeleteCascade.schemas, [
  { id: "PlanArtifact", fields: [{ id: "f2", name: "summary", type: "string" }] },
]);
assert.deepEqual(schemaDeleteCascade.members, [
  { id: "m_review", schema: "" },
  { id: "m_plan", schema: "PlanArtifact" },
]);
assert.deepEqual(schemaDeleteCascade.flow.steps[1].branches[0].cond, {});
assert.equal(schemaDeleteCascade.flow.steps[1].branches[0].condition, "");
assert.deepEqual(schemaDeleteCascade.edges[0].cond, null);
assert.equal(schemaDeleteCascade.edges[0].label, "");

assert.deepEqual(controller.diagnosticsToRows({
  ok: true,
  display_rows: [{
    kind: "ok",
    glyph: "✓",
    head: "server row",
    sub: "MobKit source",
    meta: "rkat mob validate",
  }],
  diagnostics: [{
    severity: "error",
    code: "client_fallback_must_not_win",
    message: "This diagnostic should not be converted when API rows exist.",
  }],
}), [{
  kind: "ok",
  glyph: "✓",
  head: "server row",
  sub: "MobKit source",
  meta: "rkat mob validate",
}]);
assert.deepEqual(controller.diagnosticsToRows({
  ok: true,
  display_rows: [
    { kind: "ok", head: "server omitted glyph" },
    null,
  ],
}), [{
  kind: "ok",
  glyph: "",
  head: "server omitted glyph",
  sub: "",
  meta: "",
}]);

assert.deepEqual(controller.diagnosticsToRows(null), []);
assert.deepEqual(controller.diagnosticsToRows({}), []);
assert.deepEqual(controller.diagnosticsToRows({
  ok: false,
  diagnostics: [],
  validation_source: "meerkat_mob::SpecValidator",
}), []);

assert.deepEqual(controller.deployResultToRows({
  display_rows: [{
    kind: "warn",
    glyph: "△",
    head: "server deploy row",
    sub: "rkat mob deploy",
    meta: "/tmp/example.mobpack",
  }],
  validation: { ok: false, diagnostics: [] },
}), [{
  kind: "warn",
  glyph: "△",
  head: "server deploy row",
  sub: "rkat mob deploy",
  meta: "/tmp/example.mobpack",
}]);
assert.deepEqual(controller.deployResultToRows({
  display_rows: [{ glyph: "△", head: "server omitted kind" }],
}), [{
  kind: "",
  glyph: "△",
  head: "server omitted kind",
  sub: "",
  meta: "",
}]);

assert.deepEqual(controller.deployResultToRows({
  command: "rkat mob deploy /tmp/example.mobpack prompt",
  validation: { ok: true, diagnostics: [] },
}), []);

const validationOutcome = controller.validationOutcome({ mob_id: "validate_me" }, {
  ok: true,
  display_rows: [{ kind: "ok", glyph: "✓", head: "valid", sub: "", meta: "validate" }],
});
assert.equal(validationOutcome.stage, "valid");
assert.equal(validationOutcome.validation.ok, true);
assert.equal(validationOutcome.validationRows[0].head, "valid");

const invalidOutcome = controller.validationOutcome({ mob_id: "validate_me" }, {
  ok: false,
  display_rows: [{ kind: "crit", glyph: "!", head: "invalid", sub: "", meta: "validate" }],
});
assert.equal(invalidOutcome.stage, "draft");
assert.equal(invalidOutcome.validationRows[0].kind, "crit");

const publishOutcome = controller.exportOutcome({ mob_id: "publish_me" }, {
  content_base64: "cGFjaw==",
  media_type: "application/vnd.meerkat.mobpack",
  filename: "publish-me.mobpack",
  validation: {
    ok: true,
    display_rows: [{ kind: "ok", glyph: "✓", head: "exported", sub: "", meta: "export" }],
  },
});
assert.equal(publishOutcome.stage, "published");
assert.equal(publishOutcome.validationRows[0].head, "exported");
assert.throws(
  () => controller.exportOutcome({ mob_id: "publish_me" }, {
    media_type: "application/vnd.meerkat.mobpack",
    filename: "publish-me.mobpack",
    validation: { ok: true, display_rows: [] },
  }),
  /content_base64/,
);
assert.throws(
  () => controller.exportOutcome({ mob_id: "publish_me" }, {
    content_base64: "cGFjaw==",
    filename: "publish-me.mobpack",
    validation: { ok: true, display_rows: [] },
  }),
  /media_type/,
);
assert.throws(
  () => controller.exportOutcome({ mob_id: "publish_me" }, {
    content_base64: "cGFjaw==",
    media_type: "application/vnd.meerkat.mobpack",
    validation: { ok: true, display_rows: [] },
  }),
  /filename/,
);

const rejectedPublishOutcome = controller.exportOutcome({ mob_id: "publish_me" }, {
  validation: {
    ok: false,
    display_rows: [{ kind: "crit", glyph: "!", head: "rejected", sub: "", meta: "export" }],
  },
});
assert.equal(rejectedPublishOutcome.stage, "draft");

const deployPlanOutcome = controller.deployOutcome({ mob_id: "deploy_me" }, {
  display_rows: [{ kind: "ok", glyph: "✓", head: "planned", sub: "", meta: "deploy" }],
  validation: {
    ok: true,
  },
}, { execute: false });
assert.equal(deployPlanOutcome.stage, "valid");
assert.equal(deployPlanOutcome.validationRows[0].head, "planned");

assert.deepEqual(controller.deployPlanTraceState({ mob_id: "deploy_me" }, {
  command: "rkat mob deploy /tmp/deploy.mobpack prompt",
  pack_path: "/tmp/deploy.mobpack",
  plan_trace: [
    { node: "step_1", head: "MOBPACK · deploy_me", body: "ready" },
    { node: "step_2", head: "VALIDATION · ACCEPTED", body: "ok" },
  ],
}, { deployView: TEST_DEPLOY_VIEW }), {
  steps: [
    { node: "step_1", head: "MOBPACK · deploy_me", body: "ready" },
    { node: "step_2", head: "VALIDATION · ACCEPTED", body: "ok" },
  ],
  eyebrow: "DEPLOY PLAN",
  title: "deploy_me",
  subtitle: "rkat mob deploy /tmp/deploy.mobpack prompt",
  packLabel: "/tmp/deploy.mobpack",
  firstLabel: "first",
  closeLabel: "×",
  stepLabel: "step",
  previousLabel: "‹",
  nextLabel: "›",
});
assert.deepEqual(controller.deployPlanTraceState({ name: "fallback mob" }, {}, { deployView: TEST_DEPLOY_VIEW }), {
  steps: [{
    node: null,
    head: "DEPLOY TRACE UNAVAILABLE",
    body: "mobkit/mobpacks/deploy did not return plan_trace.",
  }],
  eyebrow: "DEPLOY PLAN",
  title: "fallback mob",
  subtitle: "",
  packLabel: "",
  firstLabel: "first",
  closeLabel: "×",
  stepLabel: "step",
  previousLabel: "‹",
  nextLabel: "›",
});

const failedRunOutcome = controller.deployOutcome({ mob_id: "deploy_me" }, {
  success: false,
  display_rows: [{ kind: "crit", glyph: "!", head: "run failed", sub: "", meta: "deploy" }],
  validation: {
    ok: true,
  },
}, { execute: true });
assert.equal(failedRunOutcome.stage, "draft");
assert.equal(failedRunOutcome.validation.ok, true);

const deployPlanError = controller.deployErrorOutcome(new Error("planner offline"), { execute: false, errorView: hydratedCatalogs.errorView });
assert.equal(deployPlanError.stage, "draft");
assert.deepEqual(deployPlanError.validationRows[0], {
  kind: "crit",
  glyph: "!",
  head: "Deploy plan failed",
  sub: "planner offline",
  meta: "mobkit/mobpacks/deploy",
});

const deployRunError = controller.deployErrorOutcome(new Error("runner offline"), { execute: true, errorView: hydratedCatalogs.errorView });
assert.equal(deployRunError.validationRows[0].head, "Deploy failed");
assert.equal(deployRunError.validationRows[0].meta, "mobkit/mobpacks/deploy");

assert.deepEqual(controller.validationSheetState([
  { kind: "ok", head: "mob" },
  { kind: "warn", head: "signature" },
  { kind: "crit", head: "missing profile" },
  { kind: "", head: "unclassified" },
], { deployView: TEST_DEPLOY_VIEW }), {
  rows: [
    { kind: "ok", head: "mob" },
    { kind: "warn", head: "signature" },
    { kind: "crit", head: "missing profile" },
    { kind: "", head: "unclassified" },
  ],
  counts: { ok: 1, warn: 2, crit: 1 },
  eyebrow: "VALIDATE · MobKit",
  title: "1 passed · 2 warnings · 1 blocking",
  publishLabel: "PUBLISH",
  deployPlanLabel: "DEPLOY PLAN",
  deployLabel: "DEPLOY",
  closeLabel: "×",
  actionsDisabled: true,
});
assert.equal(controller.validationSheetState([{ kind: "ok" }, { kind: "warn" }], { deployView: TEST_DEPLOY_VIEW }).actionsDisabled, false);
assert.equal(controller.validationSheetState([{ kind: "ok" }], { stage: "draft", deployView: TEST_DEPLOY_VIEW }).actionsDisabled, true);
assert.equal(controller.validationSheetState([{ kind: "ok" }], { stage: "valid", deployView: TEST_DEPLOY_VIEW }).actionsDisabled, false);
assert.equal(controller.validationSheetState([{ kind: "crit" }], { stage: "valid", deployView: TEST_DEPLOY_VIEW }).actionsDisabled, true);

assert.deepEqual(controller.sourceErrorOutcome(new Error("missing toml"), { errorView: hydratedCatalogs.errorView }).validationRows[0], {
  kind: "crit",
  glyph: "!",
  head: "Source render failed",
  sub: "missing toml",
  meta: "mobkit/mobpacks/export",
});
assert.equal(controller.validationErrorOutcome(new Error("rpc down"), { errorView: hydratedCatalogs.errorView }).validationRows[0].head, "MobKit API unavailable");
assert.equal(controller.validationErrorOutcome(new Error("rpc down"), { errorView: hydratedCatalogs.errorView }).validationRows[0].meta, "/flow-editor/rpc");
assert.equal(controller.exportErrorOutcome(new Error("pack failed"), { errorView: hydratedCatalogs.errorView }).validationRows[0].head, "Export failed");
assert.equal(controller.exportErrorOutcome(new Error("pack failed"), { errorView: hydratedCatalogs.errorView }).validationRows[0].meta, "/flow-editor/rpc");
assert.deepEqual(controller.importErrorOutcome(new Error("bad archive"), { filename: "bad.mobpack", errorView: hydratedCatalogs.errorView }).validationRows[0], {
  kind: "crit",
  glyph: "!",
  head: "Import failed",
  sub: "bad archive",
  meta: "bad.mobpack",
});
assert.deepEqual(controller.deployErrorOutcome(new Error("no plan"), {
  execute: false,
  errorView: { ...hydratedCatalogs.errorView, deployPlanFailedHead: "Plan API failed", deployErrorMeta: "deploy-meta" },
}).validationRows[0], {
  kind: "crit",
  glyph: "!",
  head: "Plan API failed",
  sub: "no plan",
  meta: "deploy-meta",
});

const sourceProjection = controller.sourceDocumentFromExport({
  name: "Source Proof",
  mob_id: "source_proof",
  mob_toml: "[stale]",
}, {
  mob_toml: "[mob]\nid = \"source_proof\"\n",
  source_files: [{
    path: "mobkit/mob.toml",
    media_type: "text/toml",
    size_bytes: 26,
    content_base64: "unused",
    text: "[mob]\nid = \"source_proof\"\n",
  }],
  filename: "source-proof.mobpack",
  media_type: "application/vnd.mobkit.mobpack",
  validation: {
    ok: true,
    display_rows: [{
      kind: "ok",
      glyph: "✓",
      head: "exported",
      sub: "mob.toml",
      meta: "mobkit/mobpacks/export",
    }],
  },
}, {
  sourceView: hydratedCatalogs.sourceView,
});
assert.equal(sourceProjection.document.name, "Source Proof");
assert.equal(sourceProjection.document.mob_toml, "[mob]\nid = \"source_proof\"\n");
assert.equal(sourceProjection.sourceDocument.filename, "source-proof.mobpack");
assert.equal(sourceProjection.sourceDocument.media_type, "application/vnd.mobkit.mobpack");
assert.equal(sourceProjection.sourceDocument.sourcePath, "mobkit/mob.toml");
assert.equal(sourceProjection.sourceDocument.sourceFile.media_type, "text/toml");
assert.equal(sourceProjection.sourceDocument.sourceFiles.length, 1);
assert.equal(sourceProjection.sourceDocument.source, "mobkit/mobpacks/export");
assert.deepEqual(sourceProjection.sourceDocument.sourceView, hydratedCatalogs.sourceView);
assert.equal(sourceProjection.sourceDocument.validation.ok, true);
assert.equal(sourceProjection.validationRows[0].head, "exported");
assert.equal(sourceProjection.stage, "valid");
assert.deepEqual(controller.sourceEditorState(sourceProjection.sourceDocument), {
  source: "[mob]\nid = \"source_proof\"\n",
  drawerEyebrow: "SOURCE · mob.toml",
  inlineTitle: "mob.toml",
  sourceLabel: "mobkit/mobpacks/export · mobkit/mob.toml · source-proof.mobpack · application/vnd.mobkit.mobpack",
  validationSource: "",
  bodyClass: "source-drawer__body",
  showLoading: false,
  loadingText: "rendering mob.toml from mobkit/mobpacks/export...",
  copyLabel: "copy",
  closeLabel: "×",
  copyDisabled: false,
});
assert.deepEqual(controller.sourceEditorState(null, { busy: true, compact: true, sourceView: hydratedCatalogs.sourceView }), {
  source: "",
  drawerEyebrow: "SOURCE · mob.toml",
  inlineTitle: "mob.toml",
  sourceLabel: "",
  validationSource: "",
  bodyClass: "bld-toml__body",
  showLoading: true,
  loadingText: "rendering mob.toml from mobkit/mobpacks/export...",
  copyLabel: "copy",
  closeLabel: "×",
  copyDisabled: true,
});
assert.deepEqual(controller.sourceEditorState(null, { busy: true, compact: true }), {
  source: "",
  drawerEyebrow: "",
  inlineTitle: "",
  sourceLabel: "",
  validationSource: "",
  bodyClass: "bld-toml__body",
  showLoading: true,
  loadingText: "",
  copyLabel: "",
  closeLabel: "",
  copyDisabled: true,
});
assert.throws(
  () => controller.sourceDocumentFromExport({ name: "No files" }, { source_files: [] }),
  /mobkit\/mobpacks\/export did not return source_files/,
);
assert.throws(
  () => controller.sourceDocumentFromExport({ name: "No TOML file" }, {
    source_files: [{ path: "manifest.toml", text: "name = \"missing\"" }],
  }),
  /mobkit\/mobpacks\/export did not return mobkit\/mob\.toml source file/,
);
assert.throws(
  () => controller.sourceDocumentFromExport({ name: "No TOML text" }, {
    source_files: [{ path: "mobkit/mob.toml", text: "" }],
  }),
  /mobkit\/mobpacks\/export did not return mobkit\/mob\.toml text/,
);
assert.throws(
  () => controller.sourceDocumentFromExport({ name: "No filename" }, {
    mob_toml: "[mob]\nid = \"no_filename\"\n",
    source_files: [{ path: "mobkit/mob.toml", text: "[mob]\nid = \"no_filename\"\n" }],
    media_type: "application/vnd.mobkit.mobpack",
  }),
  /mobkit\/mobpacks\/export did not return filename/,
);
assert.throws(
  () => controller.sourceDocumentFromExport({ name: "No media" }, {
    mob_toml: "[mob]\nid = \"no_media\"\n",
    source_files: [{ path: "mobkit/mob.toml", text: "[mob]\nid = \"no_media\"\n" }],
    filename: "no-media.mobpack",
  }),
  /mobkit\/mobpacks\/export did not return media_type/,
);

assert.deepEqual(controller.importParamsFromDecodedFile({
  filename: "mob.toml",
  mediaType: "text/toml",
  text: "[mob]\nid = \"docs\"\n",
}), {
  source_name: "mob.toml",
  source_media_type: "text/toml",
  mob_toml: "[mob]\nid = \"docs\"\n",
});

assert.deepEqual(controller.importParamsFromDecodedFile({
  filename: "editor.json",
  mediaType: "application/json",
  text: "{\"document\":{\"mob_id\":\"docs\"},\"source_name\":\"stale.json\"}",
}), {
  source_name: "editor.json",
  source_media_type: "application/json",
  document: { mob_id: "docs" },
});

assert.deepEqual(controller.importParamsFromDecodedFile({
  filename: "editor.json",
  mediaType: "application/json",
  kind: "json",
  parsedJson: {
    source_name: "stale.json",
    source_media_type: "stale/type",
    document: { mob_id: "docs" },
  },
}), {
  source_name: "editor.json",
  source_media_type: "application/json",
  document: { mob_id: "docs" },
});

assert.deepEqual(controller.importParamsFromDecodedFile({
  filename: "array.json",
  mediaType: "application/json",
  kind: "json",
  parsedJson: [{ mob_id: "invalid-array" }],
}), {
  source_name: "array.json",
  source_media_type: "application/json",
  document: [{ mob_id: "invalid-array" }],
});

assert.deepEqual(controller.importParamsFromDecodedFile({
  filename: "docs.mobpack",
  mediaType: "application/vnd.mobkit.mobpack",
  kind: "binary",
  contentBase64: "YWJj",
}), {
  source_name: "docs.mobpack",
  source_media_type: "application/vnd.mobkit.mobpack",
  content_base64: "YWJj",
});

assert.throws(
  () => controller.importParamsFromDecodedFile({
    filename: "broken.json",
    mediaType: "application/json",
    text: "{nope",
  }),
  /broken\.json is not valid JSON/,
);

assert.deepEqual(controller.mobDefaultsFromSchema({
  mob_definition: {
    mob_settings: {
      defaults: {
        orchestrator: "planner",
        autoWireOrchestrator: true,
        roleWiring: [{ a: "planner", b: "coder" }],
        backendDefault: "sidecar",
        externalAddressBase: "http://127.0.0.1:9000",
        advanced: {
          topology: { kind: "mesh" },
          supervisor: { profile: "planner" },
          limits: { max_members: 4 },
          spawnPolicy: { mode: "manual" },
          eventRouter: { mode: "direct" },
        },
      },
    },
  },
}), {
  orchestrator: "planner",
  autoWireOrchestrator: true,
  roleWiring: [{ a: "planner", b: "coder" }],
  backendDefault: "sidecar",
  externalAddressBase: "http://127.0.0.1:9000",
  advanced: {
    topology: { kind: "mesh" },
    supervisor: { profile: "planner" },
    limits: { max_members: 4 },
    spawnPolicy: { mode: "manual" },
    eventRouter: { mode: "direct" },
  },
});

const previousProfileMembers = [
  { id: "m_planner", name: "Planner", role: "planner" },
  { id: "m_reviewer", name: "Reviewer", role: "reviewer" },
];
const renamedProfileMembers = [
  { id: "m_planner", name: "Planner", role: "planner" },
  { id: "m_reviewer", name: "Lead Reviewer", role: "reviewer" },
];
const renamedMobSettings = controller.reconcileMobSettingsProfiles({
  orchestrator: "reviewer",
  roleWiring: [{ a: "planner", b: "reviewer" }],
  backendDefault: "session",
}, previousProfileMembers, renamedProfileMembers);
assert.equal(renamedMobSettings.orchestrator, "lead_reviewer");
assert.deepEqual(renamedMobSettings.roleWiring, [{ a: "planner", b: "lead_reviewer" }]);

const deletedProfileMobSettings = controller.reconcileMobSettingsProfiles({
  orchestrator: "lead_reviewer",
  roleWiring: [{ a: "planner", b: "lead_reviewer" }],
  backendDefault: "session",
}, renamedProfileMembers, [{ id: "m_planner", name: "Planner", role: "planner" }]);
assert.equal(deletedProfileMobSettings.orchestrator, "");
assert.deepEqual(deletedProfileMobSettings.roleWiring, []);

const graphMembers = [
  { id: "m_left", name: "Left", role: "left", model: "gpt-5.5", tools: ["builtins"], skills: [] },
  { id: "m_right", name: "Right", role: "right", model: "gpt-5.5", tools: ["builtins"], skills: [] },
  { id: "m_review", name: "Review", role: "review", model: "gpt-5.5", tools: ["builtins"], skills: [], schema: "ReviewArtifact" },
];

const branchFlow = controller.graphToFlow({
  previousFlow: {
    name: "main",
    steps: [{
      id: "input_1",
      type: "input",
      task: "Route.",
      inputParams: [{ id: "p1", name: "kind", type: "string", required: true }],
    }],
  },
  members: graphMembers,
  instances: [
    { id: "g_branch_route", isGate: true, gateKind: "branch", col: 0, row: 0 },
    { id: "left", memberId: "m_left", col: 1, row: 0 },
    { id: "right", memberId: "m_right", col: 1, row: 1 },
    { id: "j_branch_route", isGate: true, gateKind: "join", collection: "any", controllerRole: "m_review", col: 2, row: 0 },
  ],
  edges: [
    { id: "e1", from: "g_branch_route", to: "left", kind: "cond", label: "docs", cond: { source: "params.kind", op: "==", value: "docs" } },
    { id: "e2", from: "g_branch_route", to: "right", kind: "cond", label: "code", cond: { namespace: "params", stepId: "params", field: "kind", op: "==", val: "code" } },
    { id: "e3", from: "left", to: "j_branch_route", kind: "next", label: "" },
    { id: "e4", from: "right", to: "j_branch_route", kind: "next", label: "" },
  ],
});

const branchStep = branchFlow.steps[1];
assert.equal(branchStep.type, "branch");
assert.equal(branchStep.controllerRole, "m_review");
assert.deepEqual(branchStep.branches.map((branch) => branch.cond), [
  { namespace: "params", stepId: "params", field: "kind", op: "==", val: "docs" },
  { namespace: "params", stepId: "params", field: "kind", op: "==", val: "code" },
]);

const branchDocument = controller.buildDocument({
  flow: branchFlow,
  studio: {
    members: graphMembers,
    schemas: [],
    instances: [
      { id: "g_branch_route", isGate: true, gateKind: "branch", col: 0, row: 0 },
      { id: "left", memberId: "m_left", col: 1, row: 0 },
      { id: "right", memberId: "m_right", col: 1, row: 1 },
      { id: "j_branch_route", isGate: true, gateKind: "join", collection: "any", controllerRole: "m_review", col: 2, row: 0 },
    ],
    edges: [
      { id: "e1", from: "g_branch_route", to: "left", kind: "cond", label: "docs", cond: { source: "params.kind", op: "==", value: "docs" } },
      { id: "e2", from: "g_branch_route", to: "right", kind: "cond", label: "code", cond: { namespace: "params", stepId: "params", field: "kind", op: "==", val: "code" } },
      { id: "e3", from: "left", to: "j_branch_route", kind: "next", label: "" },
      { id: "e4", from: "right", to: "j_branch_route", kind: "next", label: "" },
    ],
    frames: [],
    skillRealms: [],
  },
  currentFlow: { name: "branch-projection-proof" },
  deploySettings: testDeploySettings(),
});

assert.deepEqual(branchDocument.edges.slice(0, 2).map((edge) => edge.cond), [
  { var: "params.kind", op: "==", val: "docs" },
  { var: "params.kind", op: "==", val: "code" },
]);
assert(branchDocument.frames.some((frame) => frame.id === "frame_branch_route" && frame.kind === "Branch"));

const basicConditionOptionMembers = [
  { id: "m_plan", name: "Planner", schema: "PlanArtifact" },
  { id: "m_review", name: "Reviewer", schema: "ReviewArtifact" },
  { id: "m_future", name: "Future", schema: "FutureArtifact" },
];
const basicConditionOptionFlow = {
  name: "basic-condition-options-proof",
  steps: [
    {
      id: "input_1",
      type: "input",
      task: "Route.",
      inputParams: [{ id: "p1", name: "route", type: "enum", enumValues: ["docs", "code"], required: true }],
    },
    { id: "plan_step", type: "member", role: "m_plan" },
    {
      id: "branch_1",
      type: "branch",
      branches: [{
        id: "br_a",
        label: "A",
        steps: [
          { id: "review_step", type: "member", role: "m_review" },
          {
            id: "nested_branch",
            type: "branch",
            branches: [{ id: "nested_a", label: "Nested A", steps: [] }],
            fallback: [],
          },
        ],
      }],
      fallback: [{ id: "future_step", type: "member", role: "m_future" }],
    },
  ],
};
assert.deepEqual(controller.basicConditionOptions(
  basicConditionOptionFlow,
  "branch_1",
  basicConditionOptionMembers,
  { ...hydratedCatalogs.basicView, inputParamSourceLabel: "Runtime input" },
).map((option) => ({
  stepId: option.stepId,
  namespace: option.namespace,
  label: option.label,
  member: option.member?.id || "",
  fields: (option.fields || []).map((field) => field.name),
})), [
  { stepId: "params", namespace: "params", label: "Runtime input", member: "", fields: ["route"] },
  { stepId: "plan_step", namespace: "steps", label: "Planner", member: "m_plan", fields: [] },
]);
assert.deepEqual(controller.basicConditionOptions(
  basicConditionOptionFlow,
  "nested_branch",
  basicConditionOptionMembers,
  hydratedCatalogs.basicView,
).map((option) => ({
  stepId: option.stepId,
  namespace: option.namespace,
  member: option.member?.id || "",
})), [
  { stepId: "params", namespace: "params", member: "" },
  { stepId: "plan_step", namespace: "steps", member: "m_plan" },
  { stepId: "review_step", namespace: "steps", member: "m_review" },
]);
assert.deepEqual(controller.basicConditionOptions(
  basicConditionOptionFlow,
  "future_step",
  basicConditionOptionMembers,
  hydratedCatalogs.basicView,
).map((option) => option.member?.id).filter(Boolean), ["m_plan"]);

const paramReferenceFlow = {
  name: "param-reference-proof",
  steps: [
    {
      id: "input_1",
      type: "input",
      task: "Route.",
      inputParams: [{ id: "p1", name: "kind", type: "string", required: true }],
    },
    {
      id: "branch_1",
      type: "branch",
      branches: [
        {
          id: "br_docs",
          label: "Docs",
          cond: { namespace: "params", stepId: "params", field: "kind", op: "==", val: "docs" },
          condition: "params.kind == \"docs\"",
          steps: [],
        },
        {
          id: "br_code",
          label: "Code",
          condition: "params.kind == \"code\"",
          steps: [],
        },
      ],
      fallback: [],
    },
  ],
};
const paramReferenceEdges = [
  { id: "e_param_1", from: "g_branch", to: "left", kind: "cond", label: "params.kind == \"docs\"", cond: { var: "params.kind", op: "==", val: "docs" } },
  { id: "e_param_2", from: "g_branch", to: "right", kind: "cond", label: "code", cond: { namespace: "params", stepId: "params", field: "kind", op: "==", val: "code" } },
];
const renamedParamReferences = controller.reconcileInputParamReferences({
  flow: paramReferenceFlow,
  edges: paramReferenceEdges,
  oldName: "kind",
  newName: "category",
});
assert.equal(renamedParamReferences.flow.steps[1].branches[0].cond.field, "category");
assert.equal(renamedParamReferences.flow.steps[1].branches[0].condition, "params.category == \"docs\"");
assert.equal(renamedParamReferences.flow.steps[1].branches[1].condition, "params.category == \"code\"");
assert.deepEqual(renamedParamReferences.edges.map(edge => edge.cond), [
  { var: "params.category", op: "==", val: "docs" },
  { var: "params.category", op: "==", val: "code" },
]);
assert.equal(renamedParamReferences.edges[0].label, "params.category == \"docs\"");
assert.equal(renamedParamReferences.edges[1].label, "code");

const deletedParamReferences = controller.reconcileInputParamReferences({
  flow: renamedParamReferences.flow,
  edges: renamedParamReferences.edges,
  oldName: "category",
  newName: "",
});
assert.deepEqual(deletedParamReferences.flow.steps[1].branches[0].cond, {});
assert.equal(deletedParamReferences.flow.steps[1].branches[0].condition, "");
assert.equal(deletedParamReferences.flow.steps[1].branches[1].condition, "");
assert.deepEqual(deletedParamReferences.edges.map(edge => edge.cond), [null, null]);
assert.deepEqual(deletedParamReferences.edges.map(edge => edge.label), ["", ""]);

const renamedParamCascade = controller.inputParamRenameCascadePatch({
  flow: paramReferenceFlow,
  edges: paramReferenceEdges,
}, "input_1", "p1", "category", "kind", null);
assert.equal(renamedParamCascade.name, "category");
assert.equal(renamedParamCascade.flow.steps[0].inputParams[0].name, "category");
assert.equal(renamedParamCascade.flow.steps[0].fields, "category: string");
assert.equal(renamedParamCascade.flow.steps[1].branches[0].cond.field, "category");
assert.equal(renamedParamCascade.flow.steps[1].branches[0].condition, "params.category == \"docs\"");
assert.deepEqual(renamedParamCascade.edges[0].cond, { var: "params.category", op: "==", val: "docs" });
assert.equal(renamedParamCascade.edges[0].label, "params.category == \"docs\"");

const deletedParamCascade = controller.inputParamDeleteCascadePatch({
  flow: renamedParamCascade.flow,
  edges: renamedParamCascade.edges,
}, "input_1", "p1", null);
assert.equal(deletedParamCascade.removed.name, "category");
assert.deepEqual(deletedParamCascade.flow.steps[0].inputParams, []);
assert.equal(deletedParamCascade.flow.steps[0].fields, "");
assert.deepEqual(deletedParamCascade.flow.steps[1].branches[0].cond, {});
assert.equal(deletedParamCascade.flow.steps[1].branches[0].condition, "");
assert.deepEqual(deletedParamCascade.edges[0].cond, null);
assert.equal(deletedParamCascade.edges[0].label, "");

const generatedConditionEdge = {
  id: "e_generated_condition",
  from: "review",
  to: "finish",
  kind: "cond",
  label: "",
  cond: { var: "steps.review.verdict", op: "==", val: "green" },
};
const generatedConditionPatch = controller.graphEdgeConditionPatch(generatedConditionEdge, { val: "red" });
assert.deepEqual(generatedConditionPatch.cond, { var: "steps.review.verdict", op: "==", val: "red" });
assert.equal(generatedConditionPatch.label, "steps.review.verdict == \"red\"");

const missingOperatorConditionPatch = controller.graphEdgeConditionPatch({
  id: "e_missing_operator",
  from: "review",
  to: "finish",
  kind: "cond",
  label: "",
  cond: { var: "steps.review.verdict", val: "green" },
}, { val: "red" });
assert.deepEqual(missingOperatorConditionPatch.cond, { var: "steps.review.verdict", op: "", val: "red" });
assert.equal(missingOperatorConditionPatch.label, "");

const schemaDefaultConditionPatch = controller.graphEdgeConditionPatch({
  id: "e_schema_default_operator",
  from: "review",
  to: "finish",
  kind: "cond",
  label: "",
  cond: { var: "steps.review.verdict", val: "green" },
}, { val: "red" }, { defaultOperator: "==" });
assert.deepEqual(schemaDefaultConditionPatch.cond, { var: "steps.review.verdict", op: "==", val: "red" });
assert.equal(schemaDefaultConditionPatch.label, "steps.review.verdict == \"red\"");

const customConditionEdge = {
  ...generatedConditionEdge,
  label: "needs reviewer",
};
const customConditionPatch = controller.graphEdgeConditionPatch(customConditionEdge, { op: ">" });
assert.deepEqual(customConditionPatch.cond, { var: "steps.review.verdict", op: ">", val: "green" });
assert.equal(customConditionPatch.label, "needs reviewer");

const graphConditionOperatorContract = {
  mob_definition: {
    defaults: { condition_operator: "==" },
    condition_operators: ["==", "!="],
  },
};
const namedOperatorConditionPatch = controller.graphEdgeConditionOperatorPatch(generatedConditionEdge, "!=", {
  contract: graphConditionOperatorContract,
});
assert.deepEqual(namedOperatorConditionPatch.cond, { var: "steps.review.verdict", op: "!=", val: "green" });
assert.equal(namedOperatorConditionPatch.label, 'steps.review.verdict != "green"');
assert.deepEqual(controller.graphEdgeConditionOperatorPatch(generatedConditionEdge, "contains_everything", {
  contract: graphConditionOperatorContract,
}), {});

const namedValueConditionPatch = controller.graphEdgeConditionValuePatch(generatedConditionEdge, "amber");
assert.deepEqual(namedValueConditionPatch.cond, { var: "steps.review.verdict", op: "==", val: "amber" });
assert.equal(namedValueConditionPatch.label, 'steps.review.verdict == "amber"');

const retargetedConditionPatch = controller.graphEdgeConditionPatch(customConditionEdge, {
  var: "params.route",
  op: "==",
  val: "docs",
}, { forceLabel: true });
assert.deepEqual(retargetedConditionPatch.cond, { var: "params.route", op: "==", val: "docs" });
assert.equal(retargetedConditionPatch.label, "params.route == \"docs\"");
assert.deepEqual(controller.parseGraphConditionVar("steps.review.verdict"), {
  instanceId: "review",
  field: "verdict",
  namespace: "steps",
});
assert.deepEqual(controller.parseGraphConditionVar("params.route"), {
  instanceId: "params",
  field: "route",
  namespace: "params",
});
assert.deepEqual(controller.graphConditionRefForEdge({
  cond: { source: "params.route", op: "==", value: "docs" },
}), { instanceId: "params", field: "route", namespace: "params" });
const graphConditionOptionRows = controller.graphConditionOptions({
  instances: [
    { id: "planner", memberId: "m_planner", col: 0, row: 0 },
    { id: "review", memberId: "m_reviewer", col: 1, row: 0 },
    { id: "target", memberId: "m_coder", col: 2, row: 0 },
    { id: "future", memberId: "m_reviewer", col: 4, row: 0 },
    { id: "g_branch", isGate: true, col: 1, row: 1 },
  ],
  members: [
    { id: "m_planner", name: "Planner", schema: "PlanArtifact" },
    { id: "m_reviewer", name: "Reviewer", schema: "ReviewArtifact" },
    { id: "m_coder", name: "Coder", schema: "" },
  ],
  schemas: [
    { id: "PlanArtifact", fields: [{ id: "f_plan", name: "plan", type: "string" }] },
    { id: "ReviewArtifact", fields: [{ id: "f_verdict", name: "verdict", type: "enum", enumValues: ["green", "red"] }] },
  ],
  edge: { from: "review", to: "target", cond: { var: "params.route", op: "==", val: "docs" } },
  flow: { steps: [{ type: "input", fields: "route: enum(code,docs)" }] },
  graphView: { ...hydratedCatalogs.graphView, graphInputParamSourceLabel: "Runtime input" },
});
assert.deepEqual(graphConditionOptionRows.map((option) => ({
  id: option.inst.id,
  member: option.member.name,
  fields: option.fields.map((field) => field.name),
  isParams: !!option.isParams,
})), [
  { id: "params", member: "Runtime input", fields: ["route"], isParams: true },
  { id: "planner", member: "Planner", fields: ["plan"], isParams: false },
  { id: "review", member: "Reviewer", fields: ["verdict"], isParams: false },
]);
assert.deepEqual(controller.graphFirstConditionPatch(
  { cond: { var: "steps.review.verdict", op: "==", val: "green" } },
  graphConditionOptionRows,
  { defaultOperator: "==" },
), { var: "steps.review.verdict", op: "==", val: "green" });
assert.deepEqual(controller.graphFirstConditionPatch(
  { cond: null },
  graphConditionOptionRows,
  { defaultOperator: "==" },
), { var: "params.route", op: "==", val: "" });
assert.deepEqual(controller.graphEdgeConditionOwnerPatch(
  { kind: "next", label: "", cond: null },
  graphConditionOptionRows,
  "review",
  { defaultOperator: "==", forceLabel: true, includeKind: true },
), {
  kind: "cond",
  cond: { var: "steps.review.verdict", op: "==", val: "" },
  label: "steps.review.verdict == \"\"",
});
assert.deepEqual(controller.graphEdgeConditionOwnerPatch(
  { kind: "cond", label: "params.route == \"docs\"", cond: { var: "params.route", op: "==", val: "docs" } },
  graphConditionOptionRows,
  "planner",
  { defaultOperator: "==", forceLabel: true },
), {
  cond: { var: "steps.planner.plan", op: "==", val: "docs" },
  label: "steps.planner.plan == \"docs\"",
});
assert.deepEqual(controller.graphEdgeConditionOwnerPatch(
  { kind: "cond", label: "params.route == \"docs\"", cond: { var: "params.route", op: "==", val: "docs" } },
  graphConditionOptionRows,
  "ghost_owner",
  { defaultOperator: "==", forceLabel: true },
), {});
assert.deepEqual(controller.graphEdgeConditionFieldPatch(
  { kind: "cond", label: "steps.review.verdict == \"green\"", cond: { var: "steps.review.verdict", op: "==", val: "green" } },
  graphConditionOptionRows,
  "verdict",
  { defaultOperator: "==", forceLabel: true },
), {
  cond: { var: "steps.review.verdict", op: "==", val: "green" },
  label: "steps.review.verdict == \"green\"",
});
assert.deepEqual(controller.graphEdgeConditionFieldPatch(
  { kind: "cond", label: "steps.review.verdict == \"green\"", cond: { var: "steps.review.verdict", op: "==", val: "green" } },
  graphConditionOptionRows,
  "ghost_field",
  { defaultOperator: "==", forceLabel: true },
), {});
assert.deepEqual(controller.graphEdgeConditionFieldPatch(
  { kind: "cond", label: "params.route == \"docs\"", cond: { var: "params.route", op: "==", val: "docs" } },
  graphConditionOptionRows,
  "",
  { defaultOperator: "==", forceLabel: true },
), {
  cond: { var: "", op: "==", val: "docs" },
  label: "",
});
assert.deepEqual(controller.graphConditionOptions({
  instances: [{ id: "review", memberId: "m_reviewer", col: 0, row: 0 }],
  members: [{ id: "m_reviewer", name: "Reviewer", schema: "ReviewArtifact" }],
  schemas: [{ id: "ReviewArtifact", fields: [{ id: "f_verdict", name: "verdict", type: "enum" }] }],
  edge: { from: "review", to: "review", cond: { var: "params.missing", op: "==", val: "x" } },
  flow: { steps: [{ type: "input", inputParams: [] }] },
}).map((option) => ({ id: option.inst.id, fields: option.fields.map((field) => field.name), isParams: !!option.isParams })), [
  { id: "params", fields: ["missing"], isParams: true },
]);

const generatedConditionKindPatch = controller.graphEdgeKindPatch({
  ...generatedConditionEdge,
  label: "steps.review.verdict == \"green\"",
}, "next");
assert.deepEqual(generatedConditionKindPatch, { kind: "next", cond: null, label: "" });

const customConditionKindPatch = controller.graphEdgeKindPatch(customConditionEdge, "fanout");
assert.deepEqual(customConditionKindPatch, { kind: "fanout", cond: null, label: "needs reviewer" });

const conditionKindPatch = controller.graphEdgeKindPatch({
  id: "e_next",
  from: "input",
  to: "writer",
  kind: "next",
  label: "",
  cond: null,
}, "cond", {
  defaultOperator: "==",
  conditionPatch: { var: "params.route", val: "docs" },
  forceLabel: true,
});
assert.deepEqual(conditionKindPatch, {
  kind: "cond",
  cond: { var: "params.route", op: "==", val: "docs" },
  label: "params.route == \"docs\"",
});
const testEditorGraphDraft = {
  branch_gate_label: "branch",
  branch_condition_lane_label: "condition",
  branch_fallback_lane_label: "fallback",
  branch_join_label: "join · branch paths",
  fallback_edge_label: "fallback",
  parallel_lane_labels: ["lane 1", "lane 2"],
  parallel_edge_label: "parallel",
  rework_edge_label: "rework",
  terminal_edge_label_prefix: "to ",
  join_label_prefix: "join · ",
  branch_frame_label_prefix: "BRANCH · ",
  branch_frame_singular_suffix: " path",
  branch_frame_plural_suffix: " paths",
  parallel_frame_label_prefix: "PARALLEL · ",
  parallel_frame_join_infix: " · join ",
  parallel_missing_dispatch_label: "missing dispatch",
  parallel_missing_collection_label: "missing collection",
  repeat_frame_label_prefix: "REPEAT-UNTIL · ",
  repeat_max_iterations_prefix: "max ",
  repeat_missing_max_iterations_label: "missing max_iterations",
  repeat_edge_until_prefix: "until ",
  repeat_edge_until_fallback: "until condition",
};
const graphProjectionTestContract = {
  mob_definition: {
    defaults: {
      graph_edge_kind: "next",
      graph_condition_edge_kind: "cond",
      graph_fanout_edge_kind: "fanout",
    },
    graph_edge_kinds: ["next", "fanout", "cond"],
    editor_graph_draft: testEditorGraphDraft,
  },
};
assert.deepEqual(controller.graphEdgeFallbackPatch({
  id: "e_cond",
  kind: "cond",
  label: "params.route == \"docs\"",
  cond: { var: "params.route", op: "==", val: "docs" },
}, {
  mob_definition: {
    defaults: { graph_edge_kind: "next" },
    graph_edge_kinds: ["next", "cond"],
    editor_graph_draft: testEditorGraphDraft,
  },
}), {
  kind: "next",
  label: "fallback",
  cond: null,
});
assert.deepEqual(controller.graphEdgeFallbackPatch({
  id: "e_cond",
  kind: "when",
  label: "params.route == \"docs\"",
  cond: { var: "params.route", op: "==", val: "docs" },
}, {
  mob_definition: {
    defaults: { graph_edge_kind: "straight" },
    graph_edge_kinds: ["straight", "when"],
    editor_graph_draft: {
      ...testEditorGraphDraft,
      fallback_edge_label: "otherwise",
    },
  },
}), {
  kind: "straight",
  label: "otherwise",
  cond: null,
});
assert.equal(controller.graphEdgeFallbackPatch({ kind: "cond" }, {
  mob_definition: { graph_edge_kinds: ["next", "cond"] },
}), null);

const schemaAvailabilityFlow = {
  name: "schema-availability-proof",
  steps: [
    { id: "input_1", type: "input", task: "Review.", inputParams: [] },
    { id: "review_step", type: "member", role: "m_review", schema: "ReviewArtifact" },
    {
      id: "branch_on_review",
      type: "branch",
      branches: [{
        id: "br_green",
        label: "Green",
        cond: { namespace: "steps", stepId: "review_step", field: "verdict", op: "==", val: "green" },
        condition: "steps.review_step.verdict == \"green\"",
        steps: [],
      }],
      fallback: [],
    },
  ],
};
const schemaAvailabilityEdges = [
  { id: "e_schema_1", from: "review_inst", to: "next", kind: "cond", label: "steps.review_inst.verdict == \"green\"", cond: { var: "steps.review_inst.verdict", op: "==", val: "green" } },
];
const schemaAvailabilityMembers = [
  { id: "m_review", name: "Review", role: "review", schema: "ReviewArtifact" },
];
const schemaAvailabilityInstances = [
  { id: "review_inst", memberId: "m_review", col: 0, row: 0 },
];
const availableConditions = controller.reconcileConditionFieldAvailability({
  flow: schemaAvailabilityFlow,
  edges: schemaAvailabilityEdges,
  members: schemaAvailabilityMembers,
  instances: schemaAvailabilityInstances,
  schemas: [{ id: "ReviewArtifact", fields: [{ id: "f1", name: "verdict", type: "enum" }] }],
});
assert.equal(availableConditions.flow, schemaAvailabilityFlow);
assert.equal(availableConditions.edges, schemaAvailabilityEdges);

const unavailableConditions = controller.reconcileConditionFieldAvailability({
  flow: schemaAvailabilityFlow,
  edges: schemaAvailabilityEdges,
  members: schemaAvailabilityMembers,
  instances: schemaAvailabilityInstances,
  schemas: [{ id: "ReviewArtifact", fields: [{ id: "f2", name: "summary", type: "string" }] }],
});
assert.deepEqual(unavailableConditions.flow.steps[2].branches[0].cond, {});
assert.equal(unavailableConditions.flow.steps[2].branches[0].condition, "");
assert.deepEqual(unavailableConditions.edges[0].cond, null);
assert.equal(unavailableConditions.edges[0].label, "");

const branchDocumentWithStaleFrame = controller.buildDocument({
  flow: branchFlow,
  studio: {
    members: graphMembers,
    schemas: [],
    instances: [
      { id: "g_branch_route", isGate: true, gateKind: "fork", dispatch: "fan_out", col: 7, row: 3 },
      { id: "left", memberId: "m_left", col: 1, row: 0 },
      { id: "right", memberId: "m_right", col: 1, row: 1 },
      { id: "j_branch_route", isGate: true, gateKind: "join", col: 2, row: 0 },
      { id: "g_parallel_deleted", isGate: true, gateKind: "fork", col: 4, row: 4 },
    ],
    edges: [],
    frames: [
      { id: "frame_branch_route", kind: "Parallel", colStart: 9, colEnd: 12, label: "custom layout frame" },
      { id: "frame_deleted_parallel", kind: "Parallel", colStart: 4, colEnd: 6, label: "stale" },
    ],
    skillRealms: [],
  },
  currentFlow: { name: "branch-frame-filter-proof" },
  deploySettings: testDeploySettings(),
  contract: graphProjectionTestContract,
});

assert.deepEqual(branchDocumentWithStaleFrame.frames.map((frame) => frame.id), ["frame_branch_route"]);
assert.equal(branchDocumentWithStaleFrame.frames[0].kind, "Branch");
assert.equal(branchDocumentWithStaleFrame.frames[0].label, "BRANCH · 2 paths");
assert.equal(branchDocumentWithStaleFrame.frames[0].colStart, 9);
assert.deepEqual(branchDocumentWithStaleFrame.instances.map((instance) => instance.id), ["g_branch_route", "left", "right", "j_branch_route"]);
const exportedBranchGate = branchDocumentWithStaleFrame.instances.find((instance) => instance.id === "g_branch_route");
assert.equal(exportedBranchGate.gateKind, "branch");
assert.equal(exportedBranchGate.col, 7);
assert.equal(exportedBranchGate.row, 3);
assert.deepEqual(
  branchDocumentWithStaleFrame.edges.map((edge) => [edge.from, edge.to, edge.kind]),
  [
    ["g_branch_route", "left", "cond"],
    ["g_branch_route", "right", "cond"],
    ["left", "j_branch_route", "next"],
    ["right", "j_branch_route", "next"],
  ],
);

const parallelFlow = controller.graphToFlow({
  previousFlow,
  members: graphMembers,
  instances: [
    { id: "g_parallel_review", isGate: true, gateKind: "fork", dispatch: "fan_out", dependsMode: "any", col: 0, row: 0 },
    { id: "left", memberId: "m_left", col: 1, row: 0 },
    { id: "right", memberId: "m_right", col: 1, row: 1 },
    { id: "j_parallel_review", isGate: true, gateKind: "join", collection: "quorum", quorum: { mode: "NofM", n: 1, m: 2 }, col: 2, row: 0 },
  ],
  edges: [
    { id: "e1", from: "g_parallel_review", to: "left", kind: "fanout", label: "" },
    { id: "e2", from: "g_parallel_review", to: "right", kind: "fanout", label: "" },
    { id: "e3", from: "left", to: "j_parallel_review", kind: "next", label: "" },
    { id: "e4", from: "right", to: "j_parallel_review", kind: "next", label: "" },
  ],
});

const parallelStep = parallelFlow.steps[1];
assert.equal(parallelStep.type, "parallel");
assert.equal(parallelStep.dispatch, "fan_out");
assert.equal(parallelStep.collection, "quorum");
assert.equal(parallelStep.dependsMode, "any");
assert.equal(parallelStep.quorum, 1);
assert.deepEqual(parallelStep.branches.map((branch) => branch.steps[0].role), ["m_left", "m_right"]);

const incompleteParallelFlow = controller.graphToFlow({
  previousFlow,
  members: graphMembers,
  instances: [
    { id: "g_parallel_review", isGate: true, gateKind: "fork", col: 0, row: 0 },
    { id: "left", memberId: "m_left", col: 1, row: 0 },
    { id: "right", memberId: "m_right", col: 1, row: 1 },
    { id: "j_parallel_review", isGate: true, gateKind: "join", col: 2, row: 0 },
  ],
  edges: [
    { id: "e1", from: "g_parallel_review", to: "left", kind: "fanout", label: "" },
    { id: "e2", from: "g_parallel_review", to: "right", kind: "fanout", label: "" },
    { id: "e3", from: "left", to: "j_parallel_review", kind: "next", label: "" },
    { id: "e4", from: "right", to: "j_parallel_review", kind: "next", label: "" },
  ],
});

assert.equal(incompleteParallelFlow.steps[1].type, "parallel");
assert.equal(incompleteParallelFlow.steps[1].dispatch, "");
assert.equal(incompleteParallelFlow.steps[1].collection, "");
assert(!("dependsMode" in incompleteParallelFlow.steps[1]));

const incompleteParallelProjection = controller.graphProjectionForFlow(incompleteParallelFlow, graphMembers, graphProjectionTestContract);
const incompleteParallelFork = incompleteParallelProjection.instances.find((instance) => instance.id === "g_parallel_review");
const incompleteParallelJoin = incompleteParallelProjection.instances.find((instance) => instance.id === "j_parallel_review");
assert.equal(incompleteParallelFork.dispatch, "");
assert.equal(incompleteParallelFork.label, "");
assert.equal(incompleteParallelJoin.collection, "");
assert.equal(incompleteParallelJoin.label, "join · missing collection");
assert.equal(incompleteParallelProjection.frames[0].label, "PARALLEL · missing dispatch · join missing collection");

const incompleteParallelDocument = controller.buildDocument({
  flow: incompleteParallelFlow,
  studio: {
    members: graphMembers,
    schemas: [],
    instances: [],
    edges: [],
    frames: [],
    skillRealms: [],
  },
  currentFlow: { name: "missing-parallel-metadata-proof" },
  deploySettings: testDeploySettings(),
  contract: graphProjectionTestContract,
});
const incompleteDocumentFork = incompleteParallelDocument.instances.find((instance) => instance.id === "g_parallel_review");
const incompleteDocumentJoin = incompleteParallelDocument.instances.find((instance) => instance.id === "j_parallel_review");
assert.equal(incompleteDocumentFork.dispatch, "");
assert.equal(incompleteDocumentFork.label, "");
assert.equal(incompleteDocumentJoin.collection, "");
assert.equal(incompleteParallelDocument.frames[0].label, "PARALLEL · missing dispatch · join missing collection");

const repeatFlow = controller.graphToFlow({
  previousFlow,
  members: graphMembers,
  instances: [
    { id: "review", memberId: "m_review", col: 0, row: 0 },
  ],
  edges: [
    { id: "e1", from: "review", to: "review", kind: "cond", label: "until green", cond: { path: "steps.review.verdict", op: "==", value: "green" } },
  ],
});

const repeatStep = repeatFlow.steps[1];
assert.equal(repeatStep.type, "repeat");
assert.deepEqual(repeatStep.cond, { stepId: "review", field: "verdict", op: "==", val: "green" });
assert.equal(repeatStep.steps[0].role, "m_review");
assert.equal(repeatStep.loopId, "");
assert.equal(repeatStep.maxIterations, null);
assert.equal(repeatStep.iterationInput, "");
const incompleteRepeatProjection = controller.graphProjectionForFlow(repeatFlow, graphMembers, graphProjectionTestContract);
assert.equal(incompleteRepeatProjection.frames[0].label, "REPEAT-UNTIL · missing max_iterations");
assert.notEqual(incompleteRepeatProjection.frames[0].label, "REPEAT-UNTIL · max 3");

const authoredRepeatFlow = controller.graphToFlow({
  previousFlow: {
    name: "main",
    steps: [
      previousFlow.steps[0],
      {
        id: "loop_review",
        type: "repeat",
        loopId: "quality_loop",
        maxIterations: 4,
        iterationInput: "carry",
        steps: [{ id: "review", type: "member", role: "m_review", instruction: "Review the loop output." }],
      },
    ],
  },
  members: graphMembers,
  instances: [
    { id: "review", memberId: "m_review", col: 0, row: 0 },
  ],
  edges: [
    { id: "e1", from: "review", to: "review", kind: "cond", label: "until green", cond: { path: "steps.review.verdict", op: "==", value: "green" } },
  ],
});
assert.equal(authoredRepeatFlow.steps[1].id, "loop_review");
assert.equal(authoredRepeatFlow.steps[1].loopId, "quality_loop");
assert.equal(authoredRepeatFlow.steps[1].maxIterations, 4);
assert.equal(authoredRepeatFlow.steps[1].iterationInput, "carry");
assert.equal(authoredRepeatFlow.steps[1].steps[0].instruction, "Review the loop output.");
const authoredRepeatProjection = controller.graphProjectionForFlow(authoredRepeatFlow, graphMembers, graphProjectionTestContract);
assert.equal(authoredRepeatProjection.frames[0].label, "REPEAT-UNTIL · max 4");

const repeatWithoutCondition = controller.graphToFlow({
  previousFlow,
  members: graphMembers,
  instances: [
    { id: "review", memberId: "m_review", col: 0, row: 0 },
  ],
  edges: [
    { id: "e1", from: "review", to: "review", kind: "cond", label: "", cond: {} },
  ],
});

assert.equal(repeatWithoutCondition.steps[1].type, "repeat");
assert.equal(repeatWithoutCondition.steps[1].cond, null);

const repeatDocument = controller.buildDocument({
  flow: repeatFlow,
  studio: {
    members: graphMembers,
    schemas: [],
    instances: [{ id: "review", memberId: "m_review", col: 0, row: 0 }],
    edges: [{ id: "e1", from: "review", to: "review", kind: "cond", label: "until green", cond: { path: "steps.review.verdict", op: "==", value: "green" } }],
    frames: [],
    skillRealms: [],
  },
  currentFlow: { name: "repeat-projection-proof" },
  deploySettings: testDeploySettings(),
  contract: graphProjectionTestContract,
});

assert(repeatDocument.frames.some((frame) => frame.id === `frame_${repeatStep.id}` && frame.kind === "RepeatUntil"));

const incompleteGraphBranchFlow = controller.graphToFlow({
  previousFlow,
  members: graphMembers,
  instances: [
    { id: "g_branch_route", isGate: true, gateKind: "branch", col: 0, row: 0 },
    { id: "left", memberId: "m_left", col: 1, row: 0 },
    { id: "right", memberId: "m_right", col: 1, row: 1 },
  ],
  edges: [
    { id: "e1", from: "g_branch_route", to: "left", kind: "cond", label: "docs", cond: { source: "params.kind", op: "==" } },
    { id: "e2", from: "g_branch_route", to: "right", kind: "next", label: "fallback" },
  ],
});

assert.equal(incompleteGraphBranchFlow.steps[1].type, "branch");
assert.equal(incompleteGraphBranchFlow.steps[1].branches[0].condition, "");
assert.equal(incompleteGraphBranchFlow.steps[1].branches[0].cond, null);

const missingOperatorGraphBranchFlow = controller.graphToFlow({
  members: graphMembers.slice(0, 2),
  previousFlow,
  instances: [
    { id: "g_branch_route", isGate: true, gateKind: "branch", col: 0, row: 0 },
    { id: "left", memberId: "m_left", col: 1, row: 0 },
    { id: "right", memberId: "m_right", col: 1, row: 1 },
  ],
  edges: [
    { id: "e1", from: "g_branch_route", to: "left", kind: "cond", label: "", cond: { source: "params.kind", val: "docs" } },
    { id: "e2", from: "g_branch_route", to: "right", kind: "next", label: "fallback" },
  ],
});

assert.equal(missingOperatorGraphBranchFlow.steps[1].type, "branch");
assert.equal(missingOperatorGraphBranchFlow.steps[1].branches[0].condition, "");
assert.equal(missingOperatorGraphBranchFlow.steps[1].branches[0].cond, null);

assert.deepEqual(
  controller.toolCatalogFromSchema({
    tool_catalog: [{ id: "mob", label: "Mob tools", kind: "runtime", field: "mob", desc: "real mob tools", source: "meerkat_mob::ToolConfig" }],
    tool_config: [{ id: "stale", label: "stale" }],
  }).map((tool) => [tool.id, tool.kind, tool.raw.field]),
  [["mob", "runtime", "mob"]],
);

assert.deepEqual(
  controller.toolCatalogFromSchema({
    tool_config: [{ id: "shell", label: "Shell", kind: "runtime", field: "shell", desc: "real shell tool", source: "meerkat_mob::ToolConfig" }],
  }).map((tool) => [tool.id, tool.kind, tool.raw.field]),
  [],
);

assert.deepEqual(
  controller.graphControlNodes({
    mob_definition: {
      graph_gate_kinds: ["branch", "fork", "join"],
      graph_palette_gate_kinds: ["branch", "fork"],
    },
  }, hydratedCatalogs.graphView).map((node) => node.gateKind),
  ["branch", "fork"],
);
const addNodeMenuState = controller.graphAddNodeMenuState({
  members: [
    { id: "m_planner", role: "planner", name: "Planner", model: "gpt-5.5" },
    { id: "m_reviewer", role: "reviewer", name: "Reviewer", model: "gpt-5.5" },
  ],
  contract: {
    mob_definition: {
      graph_gate_kinds: ["branch", "fork", "join"],
      graph_palette_gate_kinds: ["branch", "fork"],
    },
  },
  query: "fork",
  graphView: hydratedCatalogs.graphView,
});
assert.equal(addNodeMenuState.searchIcon, "⌕");
assert.equal(addNodeMenuState.searchPlaceholder, "Add a node…");
assert.equal(addNodeMenuState.closeLabel, "✕");
assert.equal(addNodeMenuState.closeTitle, "Close");
assert.equal(addNodeMenuState.agentsLabel, "Agents");
assert.equal(addNodeMenuState.controlsLabel, "Flow controls");
assert.equal(addNodeMenuState.jumpLabel, "+ New agent in Agents →");
assert.deepEqual(addNodeMenuState.memberRows, []);
assert.deepEqual(addNodeMenuState.controlRows.map((row) => [row.id, row.gateKind, row.label, row.pick.kind, row.pick.gateKind]), [
  ["fork", "fork", "Parallel fork", "gate", "fork"],
]);
assert.equal(addNodeMenuState.hasMembers, false);
assert.equal(addNodeMenuState.hasControls, true);
assert.equal(addNodeMenuState.isEmpty, false);
const emptyAddNodeMenuState = controller.graphAddNodeMenuState({
  members: [{ id: "m_planner", role: "planner", name: "Planner", model: "gpt-5.5" }],
  contract: { mob_definition: { graph_gate_kinds: ["branch"], graph_palette_gate_kinds: ["branch"] } },
  query: "zzz",
  graphView: hydratedCatalogs.graphView,
});
assert.equal(emptyAddNodeMenuState.emptyLabel, "No matches for “zzz”");
assert.equal(emptyAddNodeMenuState.isEmpty, true);

const basicPickerKickoffState = controller.basicStepPickerState({
  isKickoff: true,
  basicView: hydratedCatalogs.basicView,
});
assert.equal(basicPickerKickoffState.mode, "kickoff");
assert.equal(basicPickerKickoffState.title, "Input");
assert.match(basicPickerKickoffState.kickoffHint, /mob's ingress/);

const basicPickerState = controller.basicStepPickerState({
  members: [
    { id: "m_planner", role: "planner", name: "Planner", model: "gpt-5.5", schema: "PlanArtifact" },
    { id: "m_reviewer", role: "reviewer", name: "Reviewer", model: "gpt-5.5" },
  ],
  contract: { mob_definition: { editor_flow_step_types: ["repeat", "branch", "parallel"] } },
  query: "parallel",
  basicView: hydratedCatalogs.basicView,
});
assert.equal(basicPickerState.mode, "picker");
assert.equal(basicPickerState.title, "Add step");
assert.equal(basicPickerState.searchIcon, "⌕");
assert.equal(basicPickerState.searchPlaceholder, "Search members & primitives…");
assert.equal(basicPickerState.membersLabel, "Mob members");
assert.equal(basicPickerState.flowLabel, "Flow");
assert.equal(basicPickerState.newBadgeLabel, "NEW");
assert.deepEqual(basicPickerState.memberRows, []);
assert.deepEqual(basicPickerState.primitiveRows.map((row) => [row.id, row.label, row.pick.kind]), [
  ["parallel", "Parallel", "parallel"],
]);
assert.equal(basicPickerState.hasConfiguredMembers, true);

const emptyBasicPickerState = controller.basicStepPickerState({
  members: [],
  contract: { mob_definition: { editor_flow_step_types: ["branch"] } },
  basicView: hydratedCatalogs.basicView,
});
assert.equal(emptyBasicPickerState.emptyMembersHint, "No members yet — define some in the Agents tab.");
assert.equal(emptyBasicPickerState.hasConfiguredMembers, false);
assert.deepEqual(emptyBasicPickerState.memberRows, []);
assert.deepEqual(emptyBasicPickerState.primitiveRows.map((row) => [row.id, row.glyph, row.tint]), [
  ["branch", "⑂", "member"],
]);

const graphShapeContract = {
  mob_definition: {
    defaults: {
      launch_mode: "fresh",
      dispatch_mode: "fan_out",
      collection_policy: "all",
      dependency_mode: "all",
      condition_operator: "==",
      schema_field_type: "string",
      branch_param_type: "enum",
      runtime_mode: "turn_driven",
      graph_terminal_kind: "success",
      graph_edge_kind: "next",
      graph_condition_edge_kind: "cond",
      graph_fanout_edge_kind: "fanout",
    },
    graph_gate_kinds: ["branch", "fork", "join"],
    graph_palette_gate_kinds: ["branch", "fork"],
    graph_terminal_kinds: ["success", "failed", "human"],
    graph_edge_kinds: ["next", "fanout", "cond"],
    editor_graph_draft: testEditorGraphDraft,
    editor_flow_step_types: ["repeat", "branch", "parallel"],
    launch_modes: ["fresh", "resume", "fork"],
    runtime_modes: ["autonomous_host", "turn_driven"],
    dispatch_modes: ["fan_out", "one_to_one", "fan_in"],
    collection_policies: ["all", "any", "quorum"],
    dependency_modes: ["all", "any"],
    condition_operators: ["==", ">", "<"],
    editor_schema_field_types: ["string", "enum"],
    editor_input_param_draft: {
      added_field: {
        name: "param",
        required: true,
        description: "",
        enumValues: [],
      },
    },
  },
};

assert.equal(controller.contractDefaultValue({ mob_definition: {} }, "launch_mode"), "");
assert.equal(controller.contractDefaultValue(graphShapeContract, "launch_mode"), "Fresh");
assert.equal(controller.contractDefaultValue(graphShapeContract, "dispatch_mode"), "fan_out");
assert.equal(controller.contractDefaultValue(graphShapeContract, "collection_policy"), "all");
assert.equal(controller.contractDefaultValue(graphShapeContract, "dependency_mode"), "all");
assert.equal(controller.contractDefaultValue(graphShapeContract, "condition_operator"), "==");
assert.equal(controller.contractDefaultValue(graphShapeContract, "schema_field_type"), "string");
assert.equal(controller.contractDefaultValue(graphShapeContract, "branch_param_type"), "enum");
assert.equal(controller.contractDefaultValue(graphShapeContract, "graph_condition_edge_kind"), "cond");
assert.equal(controller.contractDefaultValue(graphShapeContract, "graph_fanout_edge_kind"), "fanout");
assert.equal(controller.contractDefaultValue(graphShapeContract, "graph_terminal_kind"), "success");
assert.equal(controller.contractDefaultValue(graphShapeContract, "runtime_mode"), "turn_driven");

const customProjectionContract = {
  mob_definition: {
    defaults: {
      graph_edge_kind: "straight",
      graph_condition_edge_kind: "when",
      graph_fanout_edge_kind: "spread",
      condition_operator: "==",
    },
    graph_edge_kinds: ["straight", "when", "spread"],
    editor_graph_draft: testEditorGraphDraft,
  },
};
const customProjectionFlow = {
  name: "custom projection",
  steps: [
    { id: "input", type: "input" },
    {
      id: "route",
      type: "branch",
      branches: [{ id: "br_docs", condition: "params.kind == \"docs\"", steps: [{ id: "left", type: "member", role: "m_left" }] }],
      fallback: [{ id: "right", type: "member", role: "m_right" }],
    },
    {
      id: "fan",
      type: "parallel",
      branches: [
        { id: "br_a", steps: [{ id: "review", type: "member", role: "m_review" }] },
        { id: "br_b", steps: [{ id: "writer", type: "member", role: "m_writer" }] },
      ],
    },
  ],
};
assert.deepEqual(controller.graphConnectionEdgeDraft({
  id: "edge_next",
  from: { id: "plan", col: 0, row: 0 },
  to: { id: "code", col: 1, row: 0 },
  edges: [],
  contract: graphShapeContract,
}), { id: "edge_next", from: "plan", to: "code", kind: "next", label: "" });
assert.deepEqual(controller.graphConnectionEdgeDraft({
  id: "edge_terminal",
  from: { id: "review", col: 2, row: 0 },
  to: { id: "done", label: "Done", isTerminal: true, col: 3, row: 0 },
  edges: [],
  contract: graphShapeContract,
}), { id: "edge_terminal", from: "review", to: "done", kind: "next", label: "to done" });
assert.deepEqual(controller.graphConnectionEdgeDraft({
  id: "edge_fanout",
  from: { id: "fork", isGate: true, gateKind: "fork", col: 0, row: 0 },
  to: { id: "lane", col: 1, row: 0 },
  edges: [],
  contract: graphShapeContract,
}), { id: "edge_fanout", from: "fork", to: "lane", kind: "fanout", label: "" });
assert.deepEqual(controller.graphConnectionEdgeDraft({
  id: "edge_parallel",
  from: { id: "writer", col: 1, row: 0 },
  to: { id: "reviewer", col: 1, row: 1 },
  edges: [],
  contract: graphShapeContract,
}), { id: "edge_parallel", from: "writer", to: "reviewer", kind: "fanout", label: "parallel" });
assert.deepEqual(controller.graphConnectionEdgeDraft({
  id: "edge_rework",
  from: { id: "reviewer", col: 2, row: 0 },
  to: { id: "writer", col: 1, row: 0 },
  edges: [],
  contract: graphShapeContract,
}), { id: "edge_rework", from: "reviewer", to: "writer", kind: "cond", label: "rework" });
assert.equal(controller.graphConnectionEdgeDraft({
  from: { id: "plan", col: 0, row: 0 },
  to: { id: "code", col: 1, row: 0 },
  edges: [{ from: "plan", to: "code" }],
  contract: graphShapeContract,
}), null);
assert.equal(controller.graphConnectionEdgeDraft({
  from: { id: "plan", col: 0, row: 0 },
  to: { id: "code", col: 1, row: 0 },
  edges: [],
  contract: { mob_definition: { graph_edge_kinds: ["next"] } },
}), null);
assert.equal(controller.graphConnectionEdgeDraft({
  from: { id: "fork", isGate: true, gateKind: "fork", col: 0, row: 0 },
  to: { id: "lane", col: 1, row: 0 },
  edges: [],
  contract: { mob_definition: { defaults: { graph_edge_kind: "next" }, graph_edge_kinds: ["next"] } },
}), null);
assert.equal(controller.graphConnectionEdgeDraft({
  from: { id: "reviewer", col: 2, row: 0 },
  to: { id: "writer", col: 1, row: 0 },
  edges: [],
  contract: { mob_definition: { defaults: { graph_edge_kind: "next" }, graph_edge_kinds: ["next"] } },
}), null);
assert.deepEqual(controller.graphConnectionEdgeDraft({
  id: "edge_schema_names",
  from: { id: "router", col: 2, row: 0 },
  to: { id: "worker", col: 1, row: 0 },
  edges: [],
  contract: {
    mob_definition: {
      defaults: {
        graph_edge_kind: "straight",
        graph_condition_edge_kind: "when",
        graph_fanout_edge_kind: "fan",
      },
      graph_edge_kinds: ["straight", "when", "fan"],
      editor_graph_draft: {
        ...testEditorGraphDraft,
        rework_edge_label: "revise",
      },
    },
  },
}), { id: "edge_schema_names", from: "router", to: "worker", kind: "when", label: "revise" });
assert.deepEqual(controller.graphConnectionEdgeDraft({
  from: { id: "plan.node", col: 0, row: 0 },
  to: { id: "code node", col: 1, row: 0 },
  edges: [],
  contract: graphShapeContract,
}), { id: "e_plannode_code_node", from: "plan.node", to: "code node", kind: "next", label: "" });
assert.deepEqual(controller.graphConnectionEdgeDraft({
  from: { id: "plan.node", col: 0, row: 0 },
  to: { id: "code node", col: 1, row: 0 },
  edges: [{ id: "e_plannode_code_node", from: "other", to: "target" }],
  contract: graphShapeContract,
}), { id: "e_plannode_code_node_2", from: "plan.node", to: "code node", kind: "next", label: "" });

const gateState = controller.graphGateControlState({
  id: "join_1",
  gateKind: "join",
  quorum: { n: 2, m: 3 },
}, {
  edges: [
    { from: "a", to: "join_1" },
    { from: "b", to: "join_1" },
    { from: "join_1", to: "c" },
  ],
  members: [{ id: "m_joiner" }],
  contract: graphShapeContract,
  graphView: hydratedCatalogs.graphView,
});
assert.equal(gateState.gateKind, "join");
assert.equal(gateState.eyebrow, "GATE · join");
assert.equal(gateState.title, "");
assert.equal(gateState.idLine, "join_1 · cell (1,1)");
assert.equal(gateState.deleteLabel, "DELETE");
assert.equal(gateState.labelTitle, "LABEL");
assert.equal(gateState.kindTitle, "KIND");
assert.equal(gateState.selectedGateKind.label, "join — wait for branches");
assert.equal(gateState.collectionTitle, "COLLECTION POLICY");
assert.equal(gateState.quorumIncomingLabel, "of 2 incoming");
assert.equal(gateState.joinMemberLabel, "Join member");
assert.deepEqual(gateState.joinMemberPlaceholderOption, { value: "", label: "— select member —" });
assert.equal(gateState.joinMemberHint, "MobKit uses this real profile to resolve non-all fan-in.");
assert.equal(gateState.dispatchTitle, "DISPATCH MODE");
assert.equal(gateState.dispatchHint, "Exports as the MobKit parallel flow dispatch mode.");
assert.equal(gateState.conditionsTitle, "CONDITIONS");
assert.equal(gateState.emptyBranchHint, "add outgoing edges, then configure each as a typed condition or fallback");
assert.equal(gateState.wiringTitle, "WIRING");
assert.equal(gateState.incomingLabel, "incoming");
assert.equal(gateState.outgoingLabel, "outgoing");
assert.equal(gateState.collection, "quorum");
assert.equal(gateState.incoming.length, 2);
assert.equal(gateState.outgoing.length, 1);
assert.equal(gateState.firstMemberId, "m_joiner");
assert.equal(gateState.incomingCount, 2);
assert.equal(gateState.outgoingCount, 1);
assert.deepEqual(gateState.memberOptions, [{
  value: "m_joiner",
  label: "m_joiner · profile",
  member: { id: "m_joiner" },
}]);
const graphProjectionMembers = [
  { id: "m_writer", name: "Writer", role: "writer", schema: "Draft", tools: ["builtins", "shell", "git", "comms"] },
  { id: "m_review", name: "Reviewer", role: "reviewer", schema: "", tools: [] },
];
const customGraphProjectionMembers = [
  ...graphProjectionMembers,
  { id: "m_left", name: "Left", role: "left", schema: "", tools: [] },
  { id: "m_right", name: "Right", role: "right", schema: "", tools: [] },
];
const customProjection = controller.graphProjectionForFlow(customProjectionFlow, customGraphProjectionMembers, customProjectionContract);
assert.deepEqual(customProjection.edges.map((edge) => [edge.from, edge.to, edge.kind, edge.label]), [
  ["g_branch_route", "left", "when", "params.kind == \"docs\""],
  ["g_branch_route", "right", "straight", "fallback"],
  ["left", "j_branch_route", "straight", ""],
  ["right", "j_branch_route", "straight", ""],
  ["j_branch_route", "g_parallel_fan", "straight", ""],
  ["g_parallel_fan", "review", "spread", ""],
  ["g_parallel_fan", "writer", "spread", ""],
  ["review", "j_parallel_fan", "straight", ""],
  ["writer", "j_parallel_fan", "straight", ""],
]);
const customProjectionDocument = controller.buildDocument({
  flow: customProjectionFlow,
  studio: {
    members: customGraphProjectionMembers,
    schemas: [],
    instances: [],
    edges: [],
    frames: [],
  },
  deploySettings: testDeploySettings(),
  contract: customProjectionContract,
});
assert.deepEqual(customProjectionDocument.edges.map((edge) => edge.kind), customProjection.edges.map((edge) => edge.kind));
const customGraphRoundTripFlow = controller.graphToFlow({
  previousFlow: customProjectionFlow,
  members: customGraphProjectionMembers,
  instances: customProjection.instances,
  edges: customProjection.edges,
  contract: customProjectionContract,
});
assert.equal(customGraphRoundTripFlow.steps[1].type, "branch");
assert.deepEqual(customGraphRoundTripFlow.steps[1].branches.map((branch) => [branch.id, branch.cond]), [
  ["br_left", { namespace: "params", stepId: "params", field: "kind", op: "==", val: "docs" }],
]);
assert.deepEqual(customGraphRoundTripFlow.steps[1].fallback.map((step) => [step.id, step.role]), [
  ["right", "m_right"],
]);
assert.equal(customGraphRoundTripFlow.steps[2].type, "parallel");
assert.deepEqual(customGraphRoundTripFlow.steps[2].branches.map((branch) => branch.steps[0]?.role).sort(), ["m_review", "m_writer"]);
const customRepeatGraphFlow = controller.graphToFlow({
  previousFlow: {
    steps: [{ id: "input", type: "input", task: "Repeat until green.", inputParams: [] }],
  },
  members: customGraphProjectionMembers,
  instances: [{ id: "review", memberId: "m_review", col: 0, row: 0 }],
  edges: [{
    id: "e_repeat",
    from: "review",
    to: "review",
    kind: "when",
    label: "until green",
    cond: { path: "steps.review.verdict", op: "==", value: "green" },
  }],
  contract: customProjectionContract,
});
assert.equal(customRepeatGraphFlow.steps[1].type, "repeat");
assert.deepEqual(customRepeatGraphFlow.steps[1].cond, {
  stepId: "review",
  field: "verdict",
  op: "==",
  val: "green",
});
const graphProjectionInstances = [
  { id: "n_writer", memberId: "m_writer", col: 0, row: 0 },
  { id: "n_review", memberId: "m_review", col: 1, row: 0 },
  { id: "n_done", isTerminal: true, col: 2, row: 0 },
];
const graphProjectionEdges = [{ id: "e1", from: "n_writer", to: "n_review" }];
assert.deepEqual(controller.graphSelectionState({
  selection: { kind: "instance", id: "n_writer" },
  instances: graphProjectionInstances,
  edges: graphProjectionEdges,
}).instance, graphProjectionInstances[0]);
assert.equal(controller.graphSelectionState({
  selection: { kind: "edge", id: "missing" },
  instances: graphProjectionInstances,
  edges: graphProjectionEdges,
}).missing, true);
assert.deepEqual(controller.graphTemplateInspectorState({
  studio: {
    members: graphProjectionMembers,
    instances: graphProjectionInstances,
    edges: graphProjectionEdges,
    frames: [{ id: "fr1" }],
  },
  template: { name: "Docs Mob", repo: "mob.toml", version: "0.1.0", trigger: "docs", defaultTrigger: true },
  templateView: hydratedCatalogs.graphTemplateView,
}).summaryRows.map((row) => [row.key, row.value]), [
  ["members", "2 placed / 2 in library"],
  ["instances", 2],
  ["terminals", 1],
  ["edges", 1],
  ["frames", 1],
]);
const populatedTemplateInspector = controller.graphTemplateInspectorState({
  studio: {
    members: graphProjectionMembers,
    instances: graphProjectionInstances,
    edges: graphProjectionEdges,
    frames: [{ id: "fr1" }],
  },
  template: { name: "Docs Mob", repo: "mob.toml", version: "0.1.0", trigger: "docs", defaultTrigger: true },
  templateView: hydratedCatalogs.graphTemplateView,
});
assert.equal(populatedTemplateInspector.templateEyebrow, "TEMPLATE");
assert.equal(populatedTemplateInspector.summaryTitle, "SUMMARY");
assert.equal(populatedTemplateInspector.triggersTitle, "TRIGGERS");
assert.deepEqual(populatedTemplateInspector.triggerRows, [
  { key: "labels", label: "labels", value: "docs" },
  { key: "default", label: "default", value: "yes" },
]);
assert.equal(populatedTemplateInspector.quickStartTitle, "QUICK START");
assert.equal(populatedTemplateInspector.quickStartRows[0].parts[1].text, "library member");
const emptyTemplateInspector = controller.graphTemplateInspectorState({ studio: {} });
assert.equal(emptyTemplateInspector.name, "");
assert.equal(emptyTemplateInspector.repo, "");
assert.equal(emptyTemplateInspector.version, "");
assert.deepEqual(emptyTemplateInspector.triggers.labels, []);
assert.equal(emptyTemplateInspector.templateEyebrow, "");
assert.deepEqual(emptyTemplateInspector.quickStartRows, []);
const instanceControlState = controller.graphInstanceControlState({
  inst: graphProjectionInstances[1],
  instances: graphProjectionInstances,
  members: graphProjectionMembers,
  schemas: [{ id: "Draft", fields: [{ id: "f1", name: "body", type: "string", required: true }] }],
});
assert.equal(instanceControlState.member.id, "m_review");
assert.equal(instanceControlState.memberId, "m_review");
assert.equal(instanceControlState.eyebrow, "INSTANCE");
assert.equal(instanceControlState.title, "Reviewer");
assert.equal(instanceControlState.idLine, "n_review · cell (2,1)");
assert.equal(instanceControlState.deleteLabel, "DELETE");
assert.equal(instanceControlState.memberTitle, "Reviewer");
assert.equal(instanceControlState.memberRoleLabel, "MEMBER · reviewer");
assert.equal(instanceControlState.editMemberLabel, "EDIT MEMBER →");
assert.equal(instanceControlState.memberName, "Reviewer");
assert.deepEqual(instanceControlState.memberSummaryRows, [
  { key: "model", label: "model", value: "—" },
  { key: "schema", label: "schema", value: "—" },
  { key: "tools", label: "tools", value: "0" },
]);
assert.equal(instanceControlState.memberHint, "Editing the member updates every instance that uses it.");
assert.equal(instanceControlState.positionTitle, "POSITION");
assert.deepEqual(instanceControlState.positionRows, [
  { key: "stage", label: "stage (col)", value: 2 },
  { key: "slot", label: "slot (row)", value: 1 },
]);
assert.deepEqual(instanceControlState.forkSourceOptions.map((option) => [option.value, option.label]), [
  ["n_writer", "Writer · n_writer"],
]);
assert.equal(instanceControlState.firstForkSourceId, "n_writer");
const writerInstanceState = controller.graphInstanceControlState({
  inst: graphProjectionInstances[0],
  instances: graphProjectionInstances,
  members: graphProjectionMembers,
  schemas: [{ id: "Draft", fields: [{ id: "f1", name: "body", type: "string", required: true }] }],
});
assert.equal(writerInstanceState.memberToolSummary, "4 · builtins, shell, git…");
assert.equal(writerInstanceState.memberSchemaLabel, "Draft");
assert.deepEqual(writerInstanceState.memberSummaryRows, [
  { key: "model", label: "model", value: "—" },
  { key: "schema", label: "schema", value: "Draft" },
  { key: "tools", label: "tools", value: "4 · builtins, shell, git…" },
]);
assert.equal(writerInstanceState.outputSchema.id, "Draft");
assert.deepEqual(writerInstanceState.outputFields, [{ id: "f1", name: "body", type: "string", required: true }]);
assert.equal(writerInstanceState.outputTitle, "MEMBER OUTPUT · Draft");
assert.deepEqual(writerInstanceState.outputFieldRows, [{ id: "f1", name: "body", type: "string", required: true, requiredLabel: "req" }]);
assert.equal(writerInstanceState.outputHint, "Defined on the member.");
assert.equal(writerInstanceState.outputOpenMemberLabel, "Open member →");
assert.deepEqual(controller.graphNodeCanvasState({
  inst: { id: "n_writer", memberId: "m_writer", launchMode: { kind: "Fork" } },
  members: graphProjectionMembers,
  density: "comfortable",
}), {
  hidden: false,
  isTerminal: false,
  isCompact: false,
  roleLabel: "writer",
  launchLabel: "fork",
  title: "Writer",
  subtitle: undefined,
  toolRows: [
    { id: "builtins", className: "tag" },
    { id: "shell", className: "tag is-shell" },
    { id: "git", className: "tag is-shell" },
    { id: "comms", className: "tag" },
  ],
  overflowLabel: "",
});
assert.deepEqual(controller.graphNodeCanvasState({
  inst: { id: "n_writer", memberId: "m_writer" },
  members: graphProjectionMembers,
  density: "compact",
}).toolRows.map((row) => row.id), ["builtins", "shell", "git"]);
assert.equal(controller.graphNodeCanvasState({
  inst: { id: "n_missing", memberId: "m_missing" },
  members: graphProjectionMembers,
}).hidden, true);
const terminalSourceState = controller.graphNodeCanvasState({
  inst: { id: "source_mob_toml", label: "mob.toml", kind: "success", isTerminal: true },
  graphView: hydratedCatalogs.graphView,
});
assert.equal(terminalSourceState.isSourceFile, true);
assert.equal(terminalSourceState.role, "button");
assert.equal(terminalSourceState.ariaLabel, "Open mob.toml read-only source editor");
assert.equal(terminalSourceState.sourceGlyph, "{ }");
assert.equal(terminalSourceState.roleLabel, "source file");
assert.equal(terminalSourceState.title, "mob.toml");
assert.equal(terminalSourceState.subtitle, "");
const graphSourceFileNode = controller.graphSourceFileNode({
  instances: graphProjectionInstances,
  graphView: hydratedCatalogs.graphView,
});
assert.deepEqual(graphSourceFileNode, {
  id: "source_mob_toml",
  isTerminal: true,
  isSourceFile: true,
  isGraphAdornment: true,
  kind: "source",
  label: "mob.toml",
  col: 0,
  row: -1,
});
assert.equal(controller.graphSourceFileNode({
  instances: [{ id: "source_mob_toml", label: "mob.toml", isTerminal: true }],
}), null);
assert.deepEqual(controller.graphCanvasInstances({
  instances: graphProjectionInstances,
}).map((instance) => instance.id), ["source_mob_toml", "n_writer", "n_review", "n_done"]);
const existingSourceInstances = [{ id: "source_mob_toml", label: "mob.toml", isTerminal: true }];
assert.equal(controller.graphCanvasInstances({
  instances: existingSourceInstances,
}), existingSourceInstances);
assert.deepEqual(controller.graphGateCanvasState({
  inst: { id: "join_1", gateKind: "join", collection: "quorum", quorum: { n: 2, m: 3 } },
  edges: [{ to: "join_1" }, { to: "join_1" }],
}), { glyph: "⋈", sublabel: "barrier · 2/2", gateKind: "join" });
assert.deepEqual(controller.graphGateCanvasState({
  inst: { id: "fork_1", gateKind: "fork", label: "forker" },
  edges: [],
}), { glyph: "‖", sublabel: "forker", gateKind: "fork" });
assert.deepEqual(controller.graphEdgeCanvasState({
  edge: { id: "e_cond", kind: "cond", label: "review" },
  to: { id: "n_review" },
  active: true,
  selected: false,
  edgeStyle: "icons",
}), {
  kind: "cond",
  mode: "icons",
  labelText: "review",
  labelWidth: 48,
  iconGlyph: "?",
  labelFill: "var(--danger)",
  iconLabelClass: "edge-label is-cond",
  textLabelClass: "edge-label is-cond is-active",
  lineClass: "edge-line is-cond is-active",
  markerEnd: "url(#arr-acc)",
});
assert.deepEqual(controller.graphEdgeCanvasState({
  edge: { id: "e_fan", kind: "fanout", label: "" },
  to: { id: "n_done", isTerminal: true },
  edgeStyle: "colored",
}).markerEnd, "url(#arr-acc)");
assert.deepEqual(controller.graphEdgeCanvasState({
  edge: { id: "e_when", kind: "when", label: "route" },
  to: { id: "n_review" },
  contract: customProjectionContract,
}).lineClass, "edge-line is-cond");
assert.deepEqual(controller.graphEdgeCanvasState({
  edge: { id: "e_spread", kind: "spread", label: "" },
  to: { id: "n_review" },
  contract: customProjectionContract,
}).iconGlyph, "‖");
assert.deepEqual(controller.graphEdgeCanvasState({
  edge: { id: "e_term", kind: "next", label: "done" },
  to: { id: "n_done", isTerminal: true },
}), {
  kind: "next",
  mode: "text",
  labelText: "done",
  labelWidth: 36,
  iconGlyph: "■",
  labelFill: "var(--muted)",
  iconLabelClass: "edge-label",
  textLabelClass: "edge-label",
  lineClass: "edge-line is-term",
  markerEnd: "url(#arr-dim)",
});
const terminalControlState = controller.graphTerminalControlState({
  id: "n_done",
  isTerminal: true,
  label: "Done",
  col: 3,
  row: 1,
}, graphShapeContract, hydratedCatalogs.graphView);
assert.equal(terminalControlState.eyebrow, "TERMINAL · success");
assert.equal(terminalControlState.title, "Done");
assert.equal(terminalControlState.idLine, "n_done · cell (4,2)");
assert.equal(terminalControlState.deleteLabel, "DELETE");
assert.equal(terminalControlState.labelTitle, "LABEL");
assert.equal(terminalControlState.labelValue, "Done");
assert.equal(terminalControlState.kindTitle, "KIND");
assert.equal(terminalControlState.terminalKind, "success");
assert.equal(terminalControlState.selectedTerminalKind.label, "success — done");
assert.deepEqual(terminalControlState.terminalKindOptions.map((option) => [option.value, option.label, option.disabled]), [
  ["success", "success — done", false],
  ["failed", "failed — blocked", false],
  ["human", "human — needs human", false],
]);
const unsupportedTerminalControlState = controller.graphTerminalControlState({
  id: "n_done",
  isTerminal: true,
  kind: "archived",
}, graphShapeContract, hydratedCatalogs.graphView);
assert.equal(unsupportedTerminalControlState.terminalKind, "archived");
assert.equal(unsupportedTerminalControlState.selectedTerminalKind.disabled, true);
assert.match(unsupportedTerminalControlState.selectedTerminalKind.reason, /mob_definition\.graph_terminal_kinds/);
const branchConditionRows = controller.graphBranchConditionRows({
  inst: { id: "branch_1", isGate: true, gateKind: "branch", col: 0, row: 0 },
  edges: [
    {
      id: "e_cond",
      from: "branch_1",
      to: "n_review",
      kind: "cond",
      cond: { var: "steps.n_writer.body", op: "==", val: "ok" },
    },
    { id: "e_fallback", from: "branch_1", to: "n_done", kind: "next" },
  ],
  instances: [
    { id: "branch_1", isGate: true, gateKind: "branch", col: 0, row: 0 },
    { id: "n_writer", memberId: "m_writer", col: 0, row: 1 },
    { id: "n_review", memberId: "m_review", col: 1, row: 0 },
    { id: "n_done", isTerminal: true, label: "Done", col: 2, row: 0 },
  ],
  members: graphProjectionMembers,
  schemas: [{ id: "Draft", fields: [{ id: "f1", name: "body", type: "string", required: true }] }],
  contract: graphShapeContract,
  graphView: hydratedCatalogs.graphView,
});
assert.equal(branchConditionRows.length, 2);
assert.deepEqual({
  id: branchConditionRows[0].edge.id,
  modeValue: branchConditionRows[0].modeValue,
  isCondition: branchConditionRows[0].isCondition,
  conditionEdgeKind: branchConditionRows[0].conditionEdgeKind,
  targetLabel: branchConditionRows[0].targetLabel,
  ownerValue: branchConditionRows[0].ownerValue,
  fieldValue: branchConditionRows[0].fieldValue,
  operatorValue: branchConditionRows[0].operatorValue,
  hasConditionOptions: branchConditionRows[0].hasConditionOptions,
}, {
  id: "e_cond",
  modeValue: "cond",
  isCondition: true,
  conditionEdgeKind: "cond",
  targetLabel: "Reviewer",
  ownerValue: "n_writer",
  fieldValue: "body",
  operatorValue: "==",
  hasConditionOptions: true,
});
assert.deepEqual(branchConditionRows[0].ownerOptions.map((option) => [option.value, option.label]), [["n_writer", "Writer"]]);
assert.deepEqual(branchConditionRows[0].fieldOptions.map((option) => [option.value, option.label]), [["body", "body · string"]]);
assert.deepEqual(branchConditionRows[0].modeOptions, [
  { value: "cond", label: "condition" },
  { value: "fallback", label: "fallback" },
]);
assert.equal(branchConditionRows[0].targetPrefix, "→");
assert.deepEqual(branchConditionRows[0].fieldPlaceholderOption, { value: "", label: "— field —" });
assert.equal(branchConditionRows[0].noConditionOptionsHint, "add input params or an upstream schema field for this condition");
assert.deepEqual(branchConditionRows[1].targetLabel, "Done");
assert.equal(branchConditionRows[1].modeValue, "fallback");
assert.equal(branchConditionRows[1].isCondition, false);
assert.deepEqual(controller.graphBranchConditionModePatch(branchConditionRows[1].edge, "cond", {
  conditionOptions: branchConditionRows[1].conditionOptions,
  firstOwnerId: branchConditionRows[1].firstOwnerId,
  defaultOperator: branchConditionRows[1].defaultOperator,
  contract: graphShapeContract,
}), {
  kind: "cond",
  cond: { var: "steps.n_writer.body", op: "==", val: "" },
  label: "steps.n_writer.body == \"\"",
});
assert.deepEqual(controller.graphBranchConditionModePatch(branchConditionRows[0].edge, "fallback", {
  contract: graphShapeContract,
}), {
  kind: "next",
  label: "fallback",
  cond: null,
});
const customGraphConditionContract = {
  mob_definition: {
    defaults: {
      graph_edge_kind: "straight",
      graph_condition_edge_kind: "when",
      condition_operator: "==",
    },
    graph_edge_kinds: ["straight", "when"],
    condition_operators: ["=="],
    editor_graph_draft: testEditorGraphDraft,
  },
};
const customBranchConditionRows = controller.graphBranchConditionRows({
  inst: { id: "branch_1", isGate: true, gateKind: "branch", col: 0, row: 0 },
  edges: [{ id: "e_when", from: "branch_1", to: "n_review", kind: "when", cond: { var: "steps.n_writer.body", op: "==", val: "ok" } }],
  instances: [
    { id: "branch_1", isGate: true, gateKind: "branch", col: 0, row: 0 },
    { id: "n_writer", memberId: "m_writer", col: 0, row: 1 },
    { id: "n_review", memberId: "m_review", col: 1, row: 0 },
  ],
  members: graphProjectionMembers,
  schemas: [{ id: "Draft", fields: [{ id: "f1", name: "body", type: "string", required: true }] }],
  contract: customGraphConditionContract,
  graphView: hydratedCatalogs.graphView,
});
assert.equal(customBranchConditionRows[0].modeValue, "when");
assert.equal(customBranchConditionRows[0].isCondition, true);
assert.deepEqual(customBranchConditionRows[0].modeOptions, [
  { value: "when", label: "condition" },
  { value: "fallback", label: "fallback" },
]);
assert.deepEqual(controller.graphBranchConditionModePatch(customBranchConditionRows[0].edge, "fallback", {
  contract: customGraphConditionContract,
}), { kind: "straight", label: "fallback", cond: null });
assert.deepEqual(controller.graphBranchConditionModePatch({ id: "e_straight", from: "branch_1", to: "n_review", kind: "straight" }, "when", {
  conditionOptions: customBranchConditionRows[0].conditionOptions,
  firstOwnerId: customBranchConditionRows[0].firstOwnerId,
  defaultOperator: customBranchConditionRows[0].defaultOperator,
  contract: customGraphConditionContract,
}), {
  kind: "when",
  cond: { var: "steps.n_writer.body", op: "==", val: "" },
  label: "steps.n_writer.body == \"\"",
});
const edgeInspectorState = controller.graphEdgeInspectorState({
  edge: {
    id: "e_cond",
    from: "n_writer",
    to: "n_review",
    kind: "cond",
    cond: { var: "steps.n_writer.body", op: "==", val: "ok" },
  },
  instances: graphProjectionInstances,
  members: graphProjectionMembers,
  schemas: [{ id: "Draft", fields: [{ id: "f1", name: "body", type: "string", required: true }] }],
  contract: graphShapeContract,
  graphView: hydratedCatalogs.graphView,
});
assert.equal(edgeInspectorState.title, "Writer → Reviewer");
assert.equal(edgeInspectorState.eyebrow, "EDGE · cond");
assert.equal(edgeInspectorState.idLine, "e_cond");
assert.equal(edgeInspectorState.deleteLabel, "DELETE");
assert.equal(edgeInspectorState.kindTitle, "KIND");
assert.equal(edgeInspectorState.labelTitle, "LABEL");
assert.equal(edgeInspectorState.conditionTitle, "CONDITION");
assert.equal(edgeInspectorState.noConditionOptionsHint, "Add an upstream agent with an output schema before configuring this edge.");
assert.deepEqual(edgeInspectorState.ownerPlaceholderOption, { value: "", label: "— member —" });
assert.equal(edgeInspectorState.fromTitle, "FROM");
assert.equal(edgeInspectorState.toTitle, "TO");
assert.equal(edgeInspectorState.edgeKind, "cond");
assert.equal(edgeInspectorState.isCondition, true);
assert.equal(edgeInspectorState.conditionEdgeKind, "cond");
assert.equal(edgeInspectorState.defaultOperator, "==");
assert.equal(edgeInspectorState.operatorValue, "==");
assert.equal(edgeInspectorState.ownerValue, "n_writer");
assert.equal(edgeInspectorState.fieldValue, "body");
assert.deepEqual(edgeInspectorState.ownerOptions.map((option) => [option.value, option.label]), [["n_writer", "Writer"]]);
assert.deepEqual(edgeInspectorState.fieldOptions.map((option) => [option.value, option.label]), [["body", "body · string"]]);
assert.deepEqual(edgeInspectorState.fromRows.map((row) => [row.label, row.value]), [
  ["instance", "n_writer"],
  ["member", "Writer"],
  ["schema", "Draft"],
]);
assert.deepEqual(edgeInspectorState.toRows.map((row) => [row.label, row.value]), [
  ["instance", "n_review"],
  ["member", "Reviewer"],
  ["schema", "—"],
]);
assert.deepEqual(edgeInspectorState.conditionPatch, { var: "steps.n_writer.body", op: "==", val: "ok" });
const defaultEdgeInspectorState = controller.graphEdgeInspectorState({
  edge: { id: "e_next", from: "n_writer", to: "n_done" },
  instances: graphProjectionInstances,
  members: graphProjectionMembers,
  schemas: [{ id: "Draft", fields: [{ id: "f1", name: "body", type: "string", required: true }] }],
  contract: graphShapeContract,
  graphView: hydratedCatalogs.graphView,
});
assert.equal(defaultEdgeInspectorState.title, "Writer → —");
assert.equal(defaultEdgeInspectorState.edgeKind, "next");
assert.equal(defaultEdgeInspectorState.isCondition, false);
assert.equal(defaultEdgeInspectorState.selectedEdgeKind.value, "next");
assert.deepEqual(defaultEdgeInspectorState.toRows.map((row) => [row.label, row.value]), [
  ["instance", "n_done"],
  ["member", "(terminal)"],
  ["schema", "—"],
]);
const customEdgeInspectorState = controller.graphEdgeInspectorState({
  edge: { id: "e_when", from: "n_writer", to: "n_review", kind: "when", cond: { var: "steps.n_writer.body", op: "==", val: "ok" } },
  instances: graphProjectionInstances,
  members: graphProjectionMembers,
  schemas: [{ id: "Draft", fields: [{ id: "f1", name: "body", type: "string", required: true }] }],
  contract: customGraphConditionContract,
  graphView: hydratedCatalogs.graphView,
});
assert.equal(customEdgeInspectorState.edgeKind, "when");
assert.equal(customEdgeInspectorState.isCondition, true);
assert.equal(customEdgeInspectorState.conditionEdgeKind, "when");
assert.deepEqual(controller.graphGateKindPatch(" fork ", graphShapeContract), {
  gateKind: "fork",
});
assert.deepEqual(controller.graphGateKindPatch(" unsupported_gate ", graphShapeContract), {});
assert.deepEqual(controller.graphInstanceLabelPatch(" join label\n"), {
  label: " join label\n",
});
assert.deepEqual(controller.graphEdgeLabelPatch(" fallback path "), {
  label: " fallback path ",
});
assert.deepEqual(controller.graphTerminalKindPatch(" success ", graphShapeContract), {
  kind: "success",
});
assert.deepEqual(controller.graphTerminalKindPatch(" waiting_room ", graphShapeContract), {});
assert.deepEqual(controller.graphJoinCollectionPatch({}, "quorum", {
  incomingCount: 2,
  firstMemberId: "m_joiner",
  contract: graphShapeContract,
}), {
  collection: "quorum",
  label: "join · quorum",
  quorum: { n: 2, m: 2 },
  controllerRole: "m_joiner",
});
assert.deepEqual(controller.graphJoinCollectionPatch({
  quorum: { n: 1 },
  controllerRole: "m_existing",
}, "any", {
  incomingCount: 3,
  firstMemberId: "m_joiner",
  contract: graphShapeContract,
}), {
  collection: "any",
  label: "join · any",
  quorum: null,
  controllerRole: "m_existing",
});
assert.deepEqual(controller.graphJoinCollectionPatch({ controllerRole: "m_existing" }, "all", {
  incomingCount: 3,
  firstMemberId: "m_joiner",
  contract: graphShapeContract,
}), {
  collection: "all",
  label: "join · all",
  quorum: null,
  controllerRole: "",
});
assert.deepEqual(controller.graphJoinCollectionPatch({ controllerRole: "m_existing" }, "lottery", {
  incomingCount: 3,
  firstMemberId: "m_joiner",
  contract: graphShapeContract,
}), {});
assert.deepEqual(controller.graphJoinQuorumPatch({ quorum: { n: 2, m: 5 } }, "4", 3), {
  quorum: { n: 4, m: 3 },
});
assert.deepEqual(controller.graphJoinControllerRolePatch(" m_joiner ", [{ id: "m_joiner" }]), {
  controllerRole: "m_joiner",
});
assert.deepEqual(controller.graphJoinControllerRolePatch("", [{ id: "m_joiner" }]), {
  controllerRole: "",
});
assert.deepEqual(controller.graphJoinControllerRolePatch("m_deleted", [{ id: "m_joiner" }]), {});
assert.deepEqual(controller.graphForkDispatchPatch({}, "fan_out", graphShapeContract), {
  dispatch: "fan_out",
  label: "fan_out",
});
assert.deepEqual(controller.graphForkDispatchPatch({}, "broadcast_everywhere", graphShapeContract), {});

const launchControlContract = {
  mob_definition: {
    ...graphShapeContract.mob_definition,
    defaults: {
      ...graphShapeContract.mob_definition.defaults,
      budget_split_policy: "equal",
      fork_context: "full_history",
    },
    budget_split_policies: ["equal", "fixed"],
    fork_contexts: ["full_history", "last_messages"],
  },
};
const blankLaunchState = controller.launchModeControlState({}, launchControlContract, TEST_LAUNCH_VIEW);
assert.equal(blankLaunchState.launchTitle, "Launch mode");
assert.equal(blankLaunchState.graphLaunchTitle, "LAUNCH MODE · this position");
assert.equal(blankLaunchState.resumeSessionLabel, "Bridge session");
assert.equal(blankLaunchState.resumeSessionPlaceholder, "session id");
assert.equal(blankLaunchState.forkSourceLabel, "Fork from");
assert.equal(blankLaunchState.forkContextLabel, "Fork context");
assert.equal(blankLaunchState.graphForkContextLabel, "Context");
assert.equal(blankLaunchState.budgetPolicyLabel, "Budget split policy");
assert.equal(blankLaunchState.fixedBudgetLabel, "Fixed token budget");
assert.equal(blankLaunchState.fixedBudgetValue, 4096);
assert.equal(blankLaunchState.launchKind, "Fresh");
assert.deepEqual(blankLaunchState.launchMode, { kind: "Fresh" });
assert.equal(blankLaunchState.budgetSplitPolicy.kind, "Equal");
assert.equal(blankLaunchState.forkContextValue, "full_history");
assert.equal(controller.launchModeControlState({
  launchMode: { kind: "Fresh", budgetSplitPolicy: { kind: "Fixed", limit: 2048 } },
}, launchControlContract, TEST_LAUNCH_VIEW).fixedBudgetValue, 2048);
assert.deepEqual(controller.launchModeKindPatch({}, "Fork", launchControlContract, {
  firstForkSourceId: "plan_step",
}), { launchMode: { kind: "Fork", from: "plan_step", context: "full_history" } });
assert.deepEqual(controller.launchModeKindPatch({
  launchMode: { kind: "Fresh", budgetSplitPolicy: { kind: "Fixed", limit: 2048 } },
}, "Fork", launchControlContract, {
  firstForkSourceId: "plan_step",
}), { launchMode: { kind: "Fork", budgetSplitPolicy: { kind: "Fixed", limit: 2048 }, from: "plan_step", context: "full_history" } });
assert.deepEqual(controller.launchModeKindPatch({
  launchMode: { kind: "Fresh" },
}, "Teleport", launchControlContract), {});
assert.deepEqual(controller.launchModeSessionPatch({
  launchMode: { kind: "Resume", sessionId: "old" },
}, "new", launchControlContract), { launchMode: { kind: "Resume", sessionId: "new" } });
assert.deepEqual(controller.launchModeForkSourcePatch({
  launchMode: { kind: "Fork", from: "old_step", context: "last_messages" },
}, "plan_step", launchControlContract, {
  sourceOptions: [{ value: "plan_step" }, { value: "review_step" }],
}), { launchMode: { kind: "Fork", from: "plan_step", context: "last_messages" } });
assert.deepEqual(controller.launchModeForkSourcePatch({
  launchMode: { kind: "Fork", from: "old_step", context: "last_messages" },
}, "", launchControlContract, {
  sourceOptions: [{ value: "plan_step" }],
}), { launchMode: { kind: "Fork", from: "", context: "last_messages" } });
assert.deepEqual(controller.launchModeForkSourcePatch({
  launchMode: { kind: "Fork", from: "old_step", context: "last_messages" },
}, "phantom_step", launchControlContract, {
  sourceOptions: [{ value: "plan_step" }],
}), {});
assert.deepEqual(controller.launchModeForkContextPatch({
  launchMode: { kind: "Fork", from: "plan_step", context: "LastMessages" },
}, "FullHistory", launchControlContract), { launchMode: { kind: "Fork", from: "plan_step", context: "full_history" } });
assert.deepEqual(controller.launchModeForkContextPatch({
  launchMode: { kind: "Fork", from: "plan_step", context: "last_messages" },
}, "entire_universe", launchControlContract), {});
assert.deepEqual(controller.launchBudgetKindPatch({
  launchMode: { kind: "Fork", from: "plan_step", context: "full_history" },
}, "fixed", launchControlContract), {
  launchMode: {
    kind: "Fork",
    from: "plan_step",
    context: "full_history",
    budgetSplitPolicy: { kind: "Fixed", limit: 4096 },
  },
});
assert.deepEqual(controller.launchBudgetFixedLimitPatch({
  launchMode: { kind: "Fork", from: "plan_step", context: "full_history" },
}, 1024, launchControlContract), {
  launchMode: {
    kind: "Fork",
    from: "plan_step",
    context: "full_history",
    budgetSplitPolicy: { kind: "Fixed", limit: 1024 },
  },
});
assert.deepEqual(controller.launchBudgetFixedLimitPatch({
  launchMode: { kind: "Fresh" },
}, "3072", launchControlContract), {
  launchMode: {
    kind: "Fresh",
    budgetSplitPolicy: { kind: "Fixed", limit: 3072 },
  },
});
assert.deepEqual(controller.launchBudgetKindPatch({
  launchMode: { kind: "Fresh", budgetSplitPolicy: { kind: "Fixed", limit: 1024 } },
}, "lottery", launchControlContract), {});
assert.deepEqual(controller.launchBudgetFixedLimitPatch({
  launchMode: { kind: "Fresh" },
}, 1024, {
  mob_definition: {
    ...launchControlContract.mob_definition,
    budget_split_policies: ["equal"],
  },
}), {});
const memberStepControlContract = {
  mob_definition: {
    ...launchControlContract.mob_definition,
    step_output_formats: ["json", "text"],
  },
};
const writeStep = {
  id: "write",
  type: "member",
  role: "m_writer",
  launchMode: { kind: "Fork" },
  dispatchMode: "fan_out",
  collection: "quorum",
  dependsMode: "any",
  outputFormat: "xml",
};
const memberStepControlState = controller.basicMemberStepControlState({
  step: writeStep,
  flow: {
    steps: [
      { id: "plan", type: "member", role: "m_planner" },
      {
        id: "branch",
        type: "branch",
        branches: [{ id: "br1", steps: [{ id: "review", type: "member", role: "m_review" }] }],
        fallback: [],
      },
      writeStep,
    ],
  },
  members: [
    { id: "m_planner", name: "Planner", role: "planner", model: "gpt-5.5" },
    { id: "m_review", name: "Reviewer", role: "reviewer", model: "gpt-5.5" },
    { id: "m_writer", name: "Writer", role: "writer", model: "gpt-5.5", schema: "Draft", tools: ["shell", "git"] },
  ],
  contract: memberStepControlContract,
  basicView: hydratedCatalogs.basicView,
  launchView: hydratedCatalogs.launchView,
});
assert.equal(memberStepControlState.member.name, "Writer");
assert.equal(memberStepControlState.panelTitle, "Writer");
assert.equal(memberStepControlState.panelSub, "writer · gpt-5.5");
assert.equal(memberStepControlState.memberFieldLabel, "Member (profile)");
assert.equal(memberStepControlState.memberPlaceholderLabel, "— select member —");
assert.equal(memberStepControlState.launchState.launchKind, "Fork");
assert.equal(memberStepControlState.firstLaunchSourceId, "plan");
assert.deepEqual(memberStepControlState.launchSourceOptions.map((option) => [option.value, option.label]), [
  ["plan", "Planner · plan"],
  ["review", "Reviewer · review"],
]);
assert.deepEqual(memberStepControlState.memberOptions.map((option) => [option.value, option.label]), [
  ["m_planner", "Planner · planner"],
  ["m_review", "Reviewer · reviewer"],
  ["m_writer", "Writer · writer"],
]);
assert.equal(memberStepControlState.instructionLabel, "message — instruction for this turn");
assert.equal(memberStepControlState.instructionPlaceholder, "e.g. Run the focused tests and report failures.");
assert.equal(memberStepControlState.dispatchLabel, "Dispatch mode");
assert.equal(memberStepControlState.dispatchValue, "fan_out");
assert.equal(memberStepControlState.collectionLabel, "Collection policy");
assert.equal(memberStepControlState.collectionValue, "quorum");
assert.equal(memberStepControlState.showQuorum, true);
assert.equal(memberStepControlState.quorumLabel, "Quorum");
assert.equal(memberStepControlState.quorumPlaceholder, "required");
assert.equal(memberStepControlState.timeoutLabel, "Timeout (ms)");
assert.equal(memberStepControlState.timeoutPlaceholder, "runtime default");
assert.equal(memberStepControlState.dependencyLabel, "depends_on mode");
assert.equal(memberStepControlState.dependencyValue, "any");
assert.equal(memberStepControlState.outputFormatLabel, "Output format");
assert.equal(memberStepControlState.outputValue, "xml");
assert.equal(memberStepControlState.allowedToolsLabel, "Allowed tools");
assert.equal(memberStepControlState.allowedToolsEmptyLabel, "Runtime profile default");
assert.equal(memberStepControlState.blockedToolsLabel, "Blocked tools");
assert.equal(memberStepControlState.blockedToolsEmptyLabel, "No step-level blocks");
assert.deepEqual(memberStepControlState.outputOptions.map((option) => [option.value, option.label, option.disabled]), [
  ["", "runtime default", false],
  ["json", "json — parse terminal output as JSON", false],
  ["text", "text — preserve terminal text", false],
  ["xml", "xml", true],
]);
assert.match(memberStepControlState.selectedOutput.reason, /step_output_formats/);
assert.deepEqual(memberStepControlState.schemaHint, {
  schema: "Draft",
  tools: ["shell", "git"],
  toolSummary: "shell, git",
  parts: [
    { key: "prefix", text: "Emits " },
    { key: "schema", text: "Draft", kind: "code" },
    { key: "tools", text: " · tools: shell, git" },
  ],
});

assert.deepEqual(controller.parseLegacyInputFields("route: enum(code|docs), retries: int\nnotes: string"), [
  { id: "p1", name: "route", type: "enum", required: true, description: "", enumValues: ["code", "docs"] },
  { id: "p2", name: "retries", type: "int", required: true, description: "", enumValues: [] },
  { id: "p3", name: "notes", type: "string", required: true, description: "", enumValues: [] },
]);
assert.equal(controller.uniqueInputParamName([
  { id: "p1", name: "route" },
  { id: "p2", name: "route_2" },
], "route"), "route_3");
assert.equal(controller.uniqueInputParamName([{ id: "p1", name: "route" }], "9 route!", "p1"), "_9_route");
assert.equal(controller.inputParamSummary([
  { name: "route", type: "enum", required: true },
  { name: "notes", type: "", required: false },
], graphShapeContract), "route: enum, notes: string?");
assert.deepEqual(controller.inputParamOptions({
  steps: [{ type: "input", fields: "route: enum(code,docs)" }],
}, { ...hydratedCatalogs.basicView, inputParamSourceLabel: "Runtime input" }), [{
  stepId: "params",
  namespace: "params",
  label: "Runtime input",
  fields: [{ id: "p1", name: "route", type: "enum", required: true, description: "", enumValues: ["code", "docs"] }],
}]);
const inputControlState = controller.basicInputControlState({
  id: "input_1",
  type: "input",
  task: "Fix it.",
  inputParams: [
    { id: "p1", name: "route", type: "enum", required: true, description: "", enumValues: ["code", "docs"] },
  ],
}, graphShapeContract, {
  ...hydratedCatalogs.basicView,
  inputPanelTitle: "Runtime input",
  inputParamsTitlePrefix: "RUNTIME FIELDS",
  inputAddParamLabel: "+ runtime field",
  inputParamHeaderLabels: {
    name: "FIELD",
    type: "KIND",
    required: "MUST",
    description: "NOTES",
    action: "",
  },
});
assert.equal(inputControlState.panelIcon, "▤");
assert.equal(inputControlState.panelTitle, "Runtime input");
assert.equal(inputControlState.panelSub, "The task this mob is run with — its ingress");
assert.equal(inputControlState.taskLabel, "Task");
assert.equal(inputControlState.taskPlaceholder, "e.g. Fix the issue described below.");
assert.equal(inputControlState.paramsTitle, "RUNTIME FIELDS · 1");
assert.equal(inputControlState.addParamLabel, "+ runtime field");
assert.deepEqual(inputControlState.headerRows.map((row) => [row.key, row.label, row.className]), [
  ["name", "FIELD", "sb-col sb-col--name"],
  ["type", "KIND", "sb-col sb-col--type"],
  ["required", "MUST", "sb-col sb-col--req"],
  ["description", "NOTES", "sb-col sb-col--desc"],
  ["actions", "", "sb-col sb-col--act"],
]);
assert.deepEqual(inputControlState.emptyParamsParts, [
  { key: "prefix", kind: "text", text: "No params yet. Add one before branching on " },
  { key: "ref", text: "params.*", kind: "code" },
  { key: "suffix", kind: "text", text: "." },
]);
assert.deepEqual(inputControlState.tips, [
  "Run with: rkat mob deploy <pack> \"<task>\" — or run_flow(input).",
  "Typed fields become the input schema the run is validated against.",
  "Event sources & schedules live outside the mobpack (e.g. fugue).",
]);
assert.equal(inputControlState.params[0].name, "route");
assert.deepEqual(controller.inputParamUpdatePatch([
  { id: "p1", name: "route", type: "enum", required: true, description: "", enumValues: ["code"] },
], "p1", { required: false }, graphShapeContract), {
  inputParams: [{ id: "p1", name: "route", type: "enum", required: false, description: "", enumValues: ["code"] }],
  fields: "route: enum?",
});
assert.deepEqual(controller.inputParamUpdatePatch([
  { id: "p1", name: "route", type: "enum", required: true, description: "", enumValues: ["code"] },
  { id: "p2", name: "route_2", type: "string", required: true, description: "", enumValues: [] },
], "p1", { name: "route_2" }, graphShapeContract), {
  inputParams: [
    { id: "p1", name: "route_2_2", type: "enum", required: true, description: "", enumValues: ["code"] },
    { id: "p2", name: "route_2", type: "string", required: true, description: "", enumValues: [] },
  ],
  fields: "route_2_2: enum, route_2: string",
});
assert.deepEqual(controller.inputParamUpdatePatch([
  { id: "p1", name: "route", type: "enum", required: true, description: "", enumValues: ["code"] },
], "p1", { name: "" }, graphShapeContract), {
  inputParams: [{ id: "p1", name: "param", type: "enum", required: true, description: "", enumValues: ["code"] }],
  fields: "param: enum",
});
assert.deepEqual(controller.inputParamUpdatePatch([
  { id: "p1", name: "route", type: "enum", required: true, description: "", enumValues: ["code"] },
], "p1", { name: "" }, {
  mob_definition: {
    defaults: { schema_field_type: "enum" },
    editor_schema_field_types: ["enum"],
    editor_input_param_draft: {
      added_field: { name: "input_value", required: true, description: "", enumValues: [] },
    },
  },
}), {
  inputParams: [{ id: "p1", name: "input_value", type: "enum", required: true, description: "", enumValues: ["code"] }],
  fields: "input_value: enum",
});
assert.deepEqual(controller.inputParamUpdatePatch([
  { id: "p1", name: "route", type: "string", required: true, description: "", enumValues: [] },
], "p1", { type: "object" }, graphShapeContract), {
  inputParams: [{ id: "p1", name: "route", type: "string", required: true, description: "", enumValues: [] }],
  fields: "route: string",
});
assert.deepEqual(controller.inputParamUpdatePatch([
  { id: "p1", name: "route", type: "string", required: true, description: "", enumValues: [] },
], "p1", { type: "enum" }, graphShapeContract), {
  inputParams: [{ id: "p1", name: "route", type: "enum", required: true, description: "", enumValues: ["value"] }],
  fields: "route: enum",
});
assert.deepEqual(controller.inputParamRenamePatch([
  { id: "p1", name: "route", type: "enum", required: true, description: "", enumValues: [] },
  { id: "p2", name: "route_2", type: "string", required: true, description: "", enumValues: [] },
], "p1", "route_2", graphShapeContract), {
  name: "route_2_2",
  patch: {
    inputParams: [
      { id: "p1", name: "route_2_2", type: "enum", required: true, description: "", enumValues: [] },
      { id: "p2", name: "route_2", type: "string", required: true, description: "", enumValues: [] },
    ],
    fields: "route_2_2: enum, route_2: string",
  },
});
assert.deepEqual(controller.inputParamDeletePatch([
  { id: "p1", name: "route", type: "enum", required: true, description: "", enumValues: [] },
  { id: "p2", name: "notes", type: "string", required: false, description: "", enumValues: [] },
], "p1", graphShapeContract), {
  removed: { id: "p1", name: "route", type: "enum", required: true, description: "", enumValues: [] },
  patch: {
    inputParams: [{ id: "p2", name: "notes", type: "string", required: false, description: "", enumValues: [] }],
    fields: "notes: string?",
  },
});
assert.deepEqual(controller.inputParamAddPatch([
  { id: "p1", name: "param", type: "string", required: true, description: "", enumValues: [] },
], graphShapeContract), {
  param: { id: "p2", name: "param_2", type: "string", required: true, description: "", enumValues: [] },
  patch: {
    inputParams: [
      { id: "p1", name: "param", type: "string", required: true, description: "", enumValues: [] },
      { id: "p2", name: "param_2", type: "string", required: true, description: "", enumValues: [] },
    ],
    fields: "param: string, param_2: string",
  },
});
assert.deepEqual(controller.inputParamAddPatch([
  { id: "p1", name: "param", type: "string", required: true, description: "", enumValues: [] },
], {
  mob_definition: {
    defaults: { schema_field_type: "string" },
    editor_schema_field_types: ["string"],
  },
}), {
  ok: false,
  error: "MobKit schema is missing mob_definition.editor_input_param_draft",
  patch: {
    inputParams: [{ id: "p1", name: "param", type: "string", required: true, description: "", enumValues: [] }],
    fields: "param: string",
  },
});

const treeFlow = {
  name: "tree",
  steps: [
    { id: "input", type: "input" },
    {
      id: "branch",
      type: "branch",
      branches: [
        { id: "br1", steps: [{ id: "left", type: "member", role: "m_left" }] },
        { id: "br2", steps: [] },
      ],
      fallback: [{ id: "fallback_step", type: "member", role: "m_fallback" }],
    },
    {
      id: "loop",
      type: "repeat",
      steps: [{ id: "loop_step", type: "member", role: "m_loop" }],
    },
  ],
};
assert.equal(controller.flowStepUpdatePatch(treeFlow, "loop_step", {
  instruction: "Run loop body",
}).steps[2].steps[0].instruction, "Run loop body");
assert.deepEqual(controller.flowStepUpdatePatch(treeFlow, "loop_step", {
  id: "left",
}, { members: [{ id: "m_loop" }] }), treeFlow);
assert.deepEqual(controller.flowStepUpdatePatch(treeFlow, "loop_step", {
  role: "m_missing",
}, { members: [{ id: "m_loop" }] }), treeFlow);
assert.deepEqual(controller.flowStepInsertPatch(treeFlow, {
  lane: "branch",
  parentId: "branch",
  branchId: "br2",
}, { id: "right", type: "member", role: "m_right" }).steps[1].branches[1].steps, [
  { id: "right", type: "member", role: "m_right" },
]);
assert.deepEqual(controller.flowStepInsertPatch(treeFlow, {
  lane: "branch",
  parentId: "branch",
  branchId: "br2",
}, { id: "left", type: "member", role: "m_right" }, { members: [{ id: "m_right" }] }), treeFlow);
assert.deepEqual(controller.flowStepInsertPatch(treeFlow, {
  lane: "branch",
  parentId: "branch",
  branchId: "br2",
}, { id: "right", type: "member", role: "m_missing" }, { members: [{ id: "m_right" }] }), treeFlow);
assert.deepEqual(controller.flowStepInsertPatch(treeFlow, {
  lane: "branch",
  parentId: "branch",
  branchId: "fallback",
}, { id: "new_fallback", type: "member", role: "m_new" }).steps[1].fallback.map((step) => step.id), [
  "fallback_step",
  "new_fallback",
]);
assert.deepEqual(controller.flowStepInsertPatch(treeFlow, {
  lane: "branch",
  parentId: "loop",
  branchId: "body",
  index: 0,
}, { id: "pre_loop", type: "member", role: "m_pre" }).steps[2].steps.map((step) => step.id), [
  "pre_loop",
  "loop_step",
]);
assert.deepEqual(controller.flowStepInsertPatch(treeFlow, {
  lane: "main",
  index: 1,
}, { id: "main_member", type: "member", role: "m_main" }).steps.map((step) => step.id), [
  "input",
  "main_member",
  "branch",
  "loop",
]);
assert.deepEqual(controller.flowStepDeletePatch(treeFlow, "left").steps[1].branches[0].steps, []);
assert.deepEqual(controller.flowStepDeletePatch(treeFlow, "fallback_step").steps[1].fallback, []);
assert.deepEqual(controller.flowStepDeletePatch(treeFlow, "loop_step").steps[2].steps, []);
const deletedRefFlow = controller.flowStepDeletePatch({
  name: "delete-refs",
  steps: [
    { id: "source", type: "member", role: "m_source" },
    {
      id: "router",
      type: "branch",
      branches: [{
        id: "br1",
        cond: { namespace: "steps", stepId: "source", field: "verdict", op: "==", val: "green" },
        condition: 'steps.source.verdict == "green"',
        steps: [{ id: "review", type: "member", role: "m_review", launchMode: { kind: "Fork", from: "source", context: "full_history", budgetSplitPolicy: { kind: "Fixed", limit: 512 } } }],
      }],
      fallback: [],
    },
    {
      id: "loop",
      type: "repeat",
      cond: { namespace: "steps", stepId: "source", field: "verdict", op: "==", val: "green" },
      until: 'steps.source.verdict == "green"',
      steps: [{ id: "loop_review", type: "member", role: "m_review" }],
    },
  ],
}, "source");
assert.deepEqual(deletedRefFlow.steps.map((step) => step.id), ["router", "loop"]);
assert.deepEqual(deletedRefFlow.steps[0].branches[0].cond, {});
assert.equal(deletedRefFlow.steps[0].branches[0].condition, "");
assert.deepEqual(deletedRefFlow.steps[0].branches[0].steps[0].launchMode, {
  kind: "Fresh",
  budgetSplitPolicy: { kind: "Fixed", limit: 512 },
});
assert.deepEqual(deletedRefFlow.steps[1].cond, {});
assert.equal(deletedRefFlow.steps[1].until, "");

assert.deepEqual(controller.flowStepTaskPatch("Fix the bug.\n"), { task: "Fix the bug.\n" });
assert.deepEqual(controller.flowStepInstructionPatch("Run tests.\n"), { instruction: "Run tests.\n" });
assert.deepEqual(controller.flowStepQuorumPatch("3"), { quorum: 3 });
assert.deepEqual(controller.flowStepQuorumPatch(""), { quorum: null });
assert.deepEqual(controller.flowStepQuorumPatch("0"), { quorum: null });
assert.deepEqual(controller.flowStepTimeoutPatch("120000"), { timeoutMs: 120000 });
assert.deepEqual(controller.flowStepTimeoutPatch(""), { timeoutMs: null });
assert.deepEqual(controller.flowStepTimeoutPatch("-1"), { timeoutMs: null });
assert.deepEqual(controller.flowStepMaxIterationsPatch("5"), { maxIterations: 5 });
assert.deepEqual(controller.flowStepMaxIterationsPatch(""), { maxIterations: null });
assert.deepEqual(controller.flowStepMaxIterationsPatch("1.5"), { maxIterations: null });
assert.deepEqual(controller.flowStepLoopIdPatch(" quality_loop "), { loopId: "quality_loop" });
assert.deepEqual(controller.flowStepLoopIdPatch(""), { loopId: "" });
assert.deepEqual(controller.flowStepRepeatConditionPatch({
  cond: { stepId: "reviewer", field: "verdict", op: "==", val: "red" },
}, { field: "status", val: "green" }), {
  cond: { stepId: "reviewer", field: "status", op: "==", val: "green" },
});
assert.deepEqual(controller.flowStepRepeatConditionPatch({}, { stepId: "reviewer", field: "" }), {
  cond: { stepId: "reviewer", field: "" },
});
assert.deepEqual(controller.basicConditionSourcePatch([
  { stepId: "params", namespace: "params" },
  { stepId: "reviewer", namespace: "steps" },
], "params", { includeNamespace: true }), {
  namespace: "params",
  stepId: "params",
  field: "",
});
assert.deepEqual(controller.basicConditionSourcePatch([
  { stepId: "reviewer" },
], "reviewer"), {
  stepId: "reviewer",
  field: "",
});
assert.deepEqual(controller.basicConditionSourcePatch([
  { stepId: "reviewer" },
], "ghost_step"), {});
assert.deepEqual(controller.basicConditionFieldPatch(" verdict ", [{
  value: "verdict",
  field: { name: "verdict" },
}]), { field: "verdict" });
assert.deepEqual(controller.basicConditionFieldPatch("ghost", [{
  value: "verdict",
  field: { name: "verdict" },
}]), {});
assert.deepEqual(controller.basicConditionOperatorPatch(" == ", graphShapeContract), { op: "==" });
assert.deepEqual(controller.basicConditionOperatorPatch("contains_everything", graphShapeContract), {});
assert.deepEqual(controller.basicConditionValuePatch("green"), { val: "green" });
const iterationInputContract = {
  mob_definition: {
    defaults: { repeat_iteration_input: "carry" },
    repeat_iteration_inputs: ["carry"],
  },
};
assert.deepEqual(controller.flowStepIterationInputPatch(" carry ", iterationInputContract), { iterationInput: "carry" });
assert.deepEqual(controller.flowStepIterationInputPatch("", iterationInputContract), { iterationInput: "" });
assert.deepEqual(controller.flowStepIterationInputPatch("previous", iterationInputContract), {});
assert.deepEqual(controller.flowStepControllerRolePatch(" m_reviewer ", members), { controllerRole: "m_reviewer" });
assert.deepEqual(controller.flowStepControllerRolePatch("", members), { controllerRole: "" });
assert.deepEqual(controller.flowStepControllerRolePatch("m_deleted", members), {});
assert.deepEqual(controller.flowStepMemberRolePatch(" m_reviewer ", members), { role: "m_reviewer" });
assert.deepEqual(controller.flowStepMemberRolePatch("m_coder", members), {});
assert.deepEqual(controller.flowStepDispatchModePatch(" one_to_one ", memberStepControlContract), { dispatchMode: "one_to_one" });
assert.deepEqual(controller.flowStepDispatchModePatch(" broadcast ", memberStepControlContract), {});
assert.deepEqual(controller.flowStepDispatchModePatch("", memberStepControlContract), { dispatchMode: "" });
assert.deepEqual(controller.flowStepParallelDispatchPatch(" fan_out ", memberStepControlContract), { dispatch: "fan_out" });
assert.deepEqual(controller.flowStepParallelDispatchPatch(" broadcast ", memberStepControlContract), {});
assert.deepEqual(controller.flowStepCollectionPatch(" quorum ", memberStepControlContract), { collection: "quorum" });
assert.deepEqual(controller.flowStepCollectionPatch(" lottery ", memberStepControlContract), {});
assert.deepEqual(controller.flowStepDependencyModePatch(" any ", memberStepControlContract), { dependsMode: "any" });
assert.deepEqual(controller.flowStepDependencyModePatch(" maybe ", memberStepControlContract), {});
assert.deepEqual(controller.flowStepOutputFormatPatch(" json ", memberStepControlContract), { outputFormat: "json" });
assert.deepEqual(controller.flowStepOutputFormatPatch(" xml ", memberStepControlContract), {});
assert.deepEqual(controller.flowStepOutputFormatPatch("", memberStepControlContract), { outputFormat: "" });
assert.deepEqual(controller.flowStepAllowedToolsPatch([" shell ", "", "git", "shell"], {
  member: { tools: ["shell", "builtins", "unknown"] },
  toolCatalog: [{ id: "shell" }, { id: "builtins" }, { id: "memory" }],
}), { allowedTools: ["shell"] });
assert.deepEqual(controller.flowStepAllowedToolsPatch(["shell"], {
  member: { tools: ["shell"] },
  toolCatalog: [],
}), { allowedTools: [] });
assert.deepEqual(controller.flowStepBlockedToolsPatch([" memory ", null, "shell", "phantom", "shell"], {
  toolCatalog: [{ id: "memory" }, { id: "shell" }],
}), { blockedTools: ["memory", "shell"] });

assert.deepEqual(controller.basicBranchConditionPatch({
  type: "branch",
  branches: [{ id: "br1", condition: "", steps: [] }],
}, "br1", {
  namespace: "params",
  stepId: "params",
  field: "route",
  op: "==",
  val: "green",
}, graphShapeContract), {
  branches: [{
    id: "br1",
    condition: 'params.route == "green"',
    cond: { namespace: "params", stepId: "params", field: "route", op: "==", val: "green" },
    steps: [],
  }],
});
assert.deepEqual(controller.basicBranchConditionPatch({
  type: "branch",
  branches: [{ id: "br1", condition: 'params.route == "red"', steps: [] }],
}, "br1", { field: "status" }, graphShapeContract).branches[0].cond, {
  namespace: "params",
  stepId: "params",
  field: "status",
  op: "==",
  val: "red",
});
const branchConditionControlState = controller.basicBranchConditionControlState({
  branch: { id: "br1", cond: { namespace: "params", stepId: "params", field: "route", op: "==", val: "docs" } },
  options: [
    {
      stepId: "params",
      namespace: "params",
      label: "Input params",
      fields: [{ id: "p_route", name: "route", type: "enum" }],
    },
    {
      stepId: "review_step",
      namespace: "steps",
      member: { id: "m_review", name: "Reviewer", schema: "ReviewArtifact" },
    },
  ],
  schemas: [{ id: "ReviewArtifact", fields: [{ id: "f_verdict", name: "verdict", type: "enum" }] }],
  contract: graphShapeContract,
  basicView: hydratedCatalogs.basicView,
});
assert.equal(branchConditionControlState.cond.stepId, "params");
assert.equal(branchConditionControlState.selected.label, "Input params");
assert.equal(branchConditionControlState.field.name, "route");
assert.equal(branchConditionControlState.operatorValue, "==");
assert.equal(branchConditionControlState.previewLabel, 'Input params.route == "docs"');
assert.deepEqual(branchConditionControlState.sourceOptions.map((option) => [option.value, option.label]), [
  ["params", "Input params"],
  ["review_step", "Reviewer"],
]);
assert.deepEqual(branchConditionControlState.fieldOptions.map((option) => [option.value, option.label]), [
  ["route", "route · enum"],
]);
const schemaBranchConditionControlState = controller.basicBranchConditionControlState({
  branch: { id: "br1", cond: { namespace: "steps", stepId: "review_step", field: "verdict", op: "!=", val: "red" } },
  options: [{
    stepId: "review_step",
    namespace: "steps",
    member: { id: "m_review", name: "Reviewer", schema: "ReviewArtifact" },
  }],
  schemas: [{ id: "ReviewArtifact", fields: [{ id: "f_verdict", name: "verdict", type: "enum" }] }],
  contract: graphShapeContract,
  basicView: hydratedCatalogs.basicView,
});
assert.equal(schemaBranchConditionControlState.field.name, "verdict");
assert.equal(schemaBranchConditionControlState.operatorValue, "!=");
assert.equal(schemaBranchConditionControlState.operatorOptions.find((option) => option.value === "!=").disabled, true);
assert.match(schemaBranchConditionControlState.operatorOptions.find((option) => option.value === "!=").reason, /condition_operators/);
assert.equal(schemaBranchConditionControlState.previewLabel, 'Reviewer.verdict != "red"');
const branchParallelControlFlow = {
  steps: [
    {
      id: "input_1",
      type: "input",
      inputParams: [{ id: "p_route", name: "route", type: "enum" }],
    },
    { id: "review_step", type: "member", role: "m_reviewer" },
    {
      id: "parallel_1",
      type: "parallel",
      controllerRole: "m_reviewer",
      collection: "quorum",
      dependsMode: "custom_dependency",
      branches: [{ id: "br1", steps: [] }],
    },
  ],
};
const branchParallelControlState = controller.basicBranchParallelControlState({
  step: branchParallelControlFlow.steps[2],
  flow: branchParallelControlFlow,
  members: [
    ...members,
    { id: "m_writer", name: "Writer", role: "writer", schema: "DraftArtifact" },
  ],
  contract: graphShapeContract,
  basicView: hydratedCatalogs.basicView,
});
assert.equal(branchParallelControlState.isParallel, true);
assert.equal(branchParallelControlState.panelTitle, "Parallel");
assert.equal(branchParallelControlState.controllerLabel, "Join member");
assert.equal(branchParallelControlState.controllerPlaceholderLabel, "— direct MobKit lanes —");
assert.equal(branchParallelControlState.controllerRole, "m_reviewer");
assert.deepEqual(branchParallelControlState.memberOptions.map((option) => [option.value, option.label]), [
  ["m_reviewer", "Reviewer · reviewer"],
  ["m_writer", "Writer · writer"],
]);
assert.equal(branchParallelControlState.branchConditionTitle, "Branch conditions");
assert.equal(branchParallelControlState.fallbackTitle, "Fallback");
assert.equal(branchParallelControlState.dispatchLabel, "Dispatch mode");
assert.equal(branchParallelControlState.dispatchValue, "fan_out");
assert.equal(branchParallelControlState.collectionLabel, "Collection policy (fan_in)");
assert.equal(branchParallelControlState.collectionValue, "quorum");
assert.equal(branchParallelControlState.showQuorum, true);
assert.equal(branchParallelControlState.quorumLabel, "Quorum (N)");
assert.equal(branchParallelControlState.quorumPlaceholder, "required");
assert.equal(branchParallelControlState.dependencyLabel, "depends_on mode");
assert.equal(branchParallelControlState.dependencyValue, "custom_dependency");
assert.equal(branchParallelControlState.selectedDependency.disabled, true);
assert.match(branchParallelControlState.selectedDependency.reason, /dependency_modes/);
const basicBranchControlState = controller.basicBranchParallelControlState({
  step: { id: "branch_1", type: "branch", branches: [{ id: "br1", condition: "", steps: [] }] },
  flow: {
    steps: [
      branchParallelControlFlow.steps[0],
      branchParallelControlFlow.steps[1],
      { id: "branch_1", type: "branch", branches: [] },
    ],
  },
  members,
  contract: graphShapeContract,
  basicView: hydratedCatalogs.basicView,
});
assert.equal(basicBranchControlState.isParallel, false);
assert.equal(basicBranchControlState.panelTitle, "Branch");
assert.equal(basicBranchControlState.controllerLabel, "Route member");
assert.equal(basicBranchControlState.controllerPlaceholderLabel, "— direct MobKit lanes —");
assert.equal(basicBranchControlState.addBranchLabel, "+ Add branch");
assert.equal(basicBranchControlState.branchConditionTitle, "Branch conditions");
assert.equal(basicBranchControlState.fallbackTitle, "Fallback");
assert.deepEqual(basicBranchControlState.conditionOptions.map((option) => [option.stepId, option.label || option.member?.name]), [
  ["params", "Input params"],
  ["review_step", "Reviewer"],
]);
const repeatControlContract = {
  mob_definition: {
    ...graphShapeContract.mob_definition,
    defaults: {
      ...graphShapeContract.mob_definition.defaults,
      repeat_iteration_input: "carry",
    },
    repeat_iteration_inputs: ["carry"],
  },
};
const repeatControlState = controller.basicRepeatControlState({
  step: {
    id: "repeat_1",
    type: "repeat",
    cond: { stepId: "loop_review", field: "verdict", op: "==", val: "green" },
    iterationInput: "carry",
    steps: [{ id: "loop_review", type: "member", role: "m_review" }],
  },
  members: [{ id: "m_review", name: "Reviewer", schema: "ReviewArtifact" }],
  schemas: [{ id: "ReviewArtifact", fields: [{ id: "f_verdict", name: "verdict", type: "enum" }] }],
  contract: repeatControlContract,
  basicView: hydratedCatalogs.basicView,
});
assert.equal(repeatControlState.hasBodyMembers, true);
assert.equal(repeatControlState.panelIcon, "↻");
assert.equal(repeatControlState.panelTitle, "Repeat until");
assert.equal(repeatControlState.panelSub, "Loop the body, then evaluate the condition after each iteration");
assert.equal(repeatControlState.loopIdLabel, "loop_id");
assert.equal(repeatControlState.loopIdPlaceholder, "quality_loop");
assert.equal(repeatControlState.conditionTitle, "Until condition");
assert.equal(repeatControlState.conditionIntro, "Evaluated on a body member's structured output after each pass. The loop exits when it holds.");
assert.equal(repeatControlState.emptyBodyHint, "Add a member step inside the loop first — the condition reads its output schema.");
assert.equal(repeatControlState.memberPlaceholderLabel, "— member —");
assert.equal(repeatControlState.previewLabel, "until");
assert.equal(repeatControlState.previewFallback, "…");
assert.equal(repeatControlState.iterationInputLabel, "Iteration input — what each pass receives");
assert.equal(repeatControlState.maxIterationsLabel, "max_iterations");
assert.equal(repeatControlState.maxIterationsPlaceholder, "required");
assert.deepEqual(repeatControlState.tips, [
  "The body is its own FrameSpec — add member steps inside the loop.",
  "The condition reads a member's typed output (e.g. reviewer.verdict == green).",
  "max_iterations bounds the loop so it always terminates.",
]);
assert.deepEqual(repeatControlState.bodyMemberOptions.map((option) => [option.value, option.label]), [
  ["loop_review", "Reviewer"],
]);
assert.deepEqual(repeatControlState.fieldOptions.map((option) => [option.value, option.label]), [
  ["verdict", "verdict · enum"],
]);
assert.equal(repeatControlState.condField.name, "verdict");
assert.equal(repeatControlState.operatorValue, "==");
assert.equal(repeatControlState.repeatUntilExpression, 'Reviewer.verdict == "green"');
assert.deepEqual(repeatControlState.iterationInputOptions.map((option) => [option.value, option.label, option.disabled]), [
  ["", "runtime default", false],
  ["carry", "Carry — last body step's output feeds the next pass", false],
]);
assert.equal(repeatControlState.selectedIterationInput.value, "carry");
const missingRepeatControlState = controller.basicRepeatControlState({
  step: { id: "repeat_2", type: "repeat", cond: { stepId: "missing", field: "verdict", op: "==", val: "green" }, iterationInput: "previous" },
  members: [{ id: "m_review", name: "Reviewer", schema: "ReviewArtifact" }],
  schemas: [{ id: "ReviewArtifact", fields: [{ id: "f_verdict", name: "verdict", type: "enum" }] }],
  contract: repeatControlContract,
  basicView: hydratedCatalogs.basicView,
});
assert.equal(missingRepeatControlState.hasBodyMembers, false);
assert.equal(missingRepeatControlState.fieldPlaceholder, "(no schema)");
assert.equal(missingRepeatControlState.selectedIterationInput.disabled, true);
assert.match(missingRepeatControlState.selectedIterationInput.reason, /repeat_iteration_inputs/);
{
  const patch = controller.basicBranchAddPatch(
    { type: "branch", branches: [{ id: "br1", label: "Path 1", condition: "", steps: [] }] },
    { basicView: { ...hydratedCatalogs.basicView, branchConditionRowTitlePrefix: "Path" } }
  );
  assert.equal(patch.branches.length, 2);
  assert.equal(patch.branches[1].id, "br_1");
  assert.equal(patch.branches[1].label, "Path 2");
  assert.equal(patch.branches[1].condition, "");
  assert.deepEqual(patch.branches[1].steps, []);
}
{
  const patch = controller.basicBranchAddPatch(
    { type: "parallel", branches: [{ id: "br1", label: "Path 1", steps: [] }] },
    { basicView: { ...hydratedCatalogs.basicView, branchConditionRowTitlePrefix: "Path" } }
  );
  assert.equal(patch.branches.length, 2);
  assert.equal(patch.branches[1].id, "br_1");
  assert.equal(patch.branches[1].label, "Path 2");
  assert.equal(Object.prototype.hasOwnProperty.call(patch.branches[1], "condition"), false);
  assert.deepEqual(patch.branches[1].steps, []);
}
{
  const flow = {
    steps: [{
      id: "router",
      type: "branch",
      branches: [
        { id: "br_1", steps: [] },
        { id: "br_2", steps: [] },
      ],
      fallback: [],
    }],
  };
  const patch = controller.basicBranchAddPatch(flow.steps[0], { flow });
  assert.equal(patch.branches[2].id, "br_3");
}
{
  const flow = {
    steps: [{
      id: "fanout",
      type: "parallel",
      branches: [
        { id: "br_1", steps: [] },
        { id: "br_2", steps: [] },
      ],
    }],
  };
  const patch = controller.basicBranchAddPatch(flow.steps[0], { flow });
  assert.equal(patch.branches[2].id, "br_3");
  assert.equal(Object.prototype.hasOwnProperty.call(patch.branches[2], "condition"), false);
}

assert.equal(controller.contractDefaultValue({
  mob_definition: {
    defaults: { dispatch_mode: "fan_out" },
    dispatch_modes: ["one_to_one"],
  },
}, "dispatch_mode"), "");

assert.equal(controller.graphControlShape({
  gateKind: "branch",
  at: { col: 0, row: 0 },
  members: graphMembers.slice(0, 2),
  instances: [],
  flow: previousFlow,
  contract: {
    mob_definition: {
      graph_gate_kinds: ["branch", "fork", "join"],
      graph_palette_gate_kinds: ["branch", "fork"],
    },
  },
}), null);

assert.equal(controller.graphControlShape({
  gateKind: "branch",
  at: { col: 0, row: 0 },
  members: graphMembers.slice(0, 2),
  instances: [],
  flow: previousFlow,
  contract: {
    mob_definition: {
      graph_gate_kinds: ["branch", "fork", "join"],
      graph_palette_gate_kinds: ["branch", "fork"],
      graph_edge_kinds: ["next", "fanout", "cond"],
      editor_flow_step_types: ["repeat", "branch", "parallel"],
      launch_modes: ["fresh", "resume", "fork"],
      dispatch_modes: ["fan_out", "one_to_one", "fan_in"],
      collection_policies: ["all", "any", "quorum"],
      dependency_modes: ["all", "any"],
      condition_operators: ["==", ">", "<"],
      editor_schema_field_types: ["string", "enum"],
    },
  },
}), null);

const branchShape = controller.graphControlShape({
  gateKind: "branch",
  at: { col: 0, row: 0 },
  members: graphMembers.slice(0, 2),
  instances: [],
  flow: previousFlow,
  contract: graphShapeContract,
  graphView: hydratedCatalogs.graphView,
});
assert(branchShape);
assert.equal(branchShape.instances[0].gateKind, "branch");
assert.equal(branchShape.instances[0].label, "branch");
assert.equal(branchShape.instances[1].launchMode.kind, "Fresh");
assert.equal(branchShape.instances[1].lane, "condition");
assert.equal(branchShape.instances[2].lane, "fallback");
assert.equal(branchShape.instances[3].collection, "any");
assert.equal(branchShape.instances[3].controllerRole, "m_left");
assert.equal(branchShape.instances[3].label, "join · branch paths");
assert.equal(branchShape.edges[0].kind, "cond");
assert.equal(branchShape.edges[0].label, "");
assert.equal(branchShape.edges[0].cond, null);
assert.equal(branchShape.edges[1].label, "fallback");
assert.equal(branchShape.flow, previousFlow);
assert.deepEqual(branchShape.flow.steps[0].inputParams, []);

const branchShapeAfterCollision = controller.graphControlShape({
  gateKind: "branch",
  at: { col: 0, row: 0 },
  members: graphMembers.slice(0, 2),
  instances: [
    { id: "g_branch_1", isGate: true },
    { id: "g_branch_1_a", memberId: "m_left" },
    { id: "g_branch_1_b", memberId: "m_right" },
    { id: "j_branch_1", isGate: true },
  ],
  edges: [{ id: "e_g_branch_1_g_branch_1_a", from: "g_branch_1", to: "g_branch_1_a" }],
  flow: previousFlow,
  contract: graphShapeContract,
  graphView: hydratedCatalogs.graphView,
});
assert(branchShapeAfterCollision);
assert.deepEqual(branchShapeAfterCollision.instances.map((instance) => instance.id), [
  "g_branch_2",
  "g_branch_2_a",
  "g_branch_2_b",
  "j_branch_2",
]);
assert.deepEqual(branchShapeAfterCollision.edges.map((edge) => edge.id), [
  "e_g_branch_2_g_branch_2_a",
  "e_g_branch_2_g_branch_2_b",
  "e_g_branch_2_a_j_branch_2",
  "e_g_branch_2_b_j_branch_2",
]);

const forkShape = controller.graphControlShape({
  gateKind: "fork",
  at: { col: 0, row: 0 },
  members: graphMembers.slice(0, 2),
  instances: [],
  flow: previousFlow,
  contract: graphShapeContract,
  graphView: hydratedCatalogs.graphView,
});
assert(forkShape);
assert.equal(forkShape.instances[0].dispatch, "fan_out");
assert.equal(forkShape.instances[1].lane, "lane 1");
assert.equal(forkShape.instances[2].lane, "lane 2");
assert.equal(forkShape.instances[3].collection, "all");
assert.equal(forkShape.instances[3].label, "join · all");
assert.deepEqual(forkShape.edges.map((edge) => edge.kind), ["fanout", "fanout", "next", "next"]);

assert.equal(controller.graphMemberInstanceShape({
  memberId: "m_left",
  at: { col: 2, row: 3 },
  contract: { mob_definition: {} },
}), null);

const memberInstanceShape = controller.graphMemberInstanceShape({
  memberId: "m_left",
  at: { col: 2, row: 3 },
  instances: [{ id: "i_m_left" }],
  contract: graphShapeContract,
});
assert.equal(memberInstanceShape.id, "i_m_left_2");
assert.equal(memberInstanceShape.memberId, "m_left");
assert.deepEqual(memberInstanceShape.launchMode, { kind: "Fresh" });
assert.equal(memberInstanceShape.col, 2);
assert.equal(memberInstanceShape.row, 3);

const quickMemberInsert = controller.graphQuickInsertProjection({
  pick: { kind: "memberInstance", memberId: "m_left" },
  at: { col: 4, row: 5 },
  members: graphMembers.slice(0, 2),
  instances: [{ id: "i_m_left", memberId: "m_left" }],
  edges: [{ id: "keep_edge", from: "i_m_left", to: "i_other" }],
  flow: previousFlow,
  contract: graphShapeContract,
});
assert.equal(quickMemberInsert.ok, true);
assert.equal(quickMemberInsert.snap, true);
assert.equal(quickMemberInsert.flow, previousFlow);
assert.deepEqual(quickMemberInsert.edges, [{ id: "keep_edge", from: "i_m_left", to: "i_other" }]);
assert.equal(quickMemberInsert.instances[1].id, "i_m_left_2");
assert.equal(quickMemberInsert.instances[1].memberId, "m_left");
assert.equal(quickMemberInsert.selectId, "i_m_left_2");

const quickMissingMemberInsert = controller.graphQuickInsertProjection({
  pick: { kind: "memberInstance", memberId: "m_missing" },
  at: { col: 4, row: 5 },
  members: graphMembers.slice(0, 2),
  instances: [],
  edges: [],
  flow: previousFlow,
  contract: graphShapeContract,
});
assert.equal(quickMissingMemberInsert.ok, false);
assert.deepEqual(quickMissingMemberInsert.instances, []);
assert.deepEqual(quickMissingMemberInsert.edges, []);

const quickBranchInsert = controller.graphQuickInsertProjection({
  pick: { kind: "gate", gateKind: "branch" },
  at: { col: 0, row: 0 },
  members: graphMembers.slice(0, 2),
  instances: [{ id: "existing", memberId: "m_left", col: 0, row: 0 }],
  edges: [],
  flow: previousFlow,
  contract: graphShapeContract,
  graphView: hydratedCatalogs.graphView,
});
assert.equal(quickBranchInsert.ok, true);
assert.equal(quickBranchInsert.snap, true);
assert.equal(quickBranchInsert.flow, previousFlow);
assert.equal(quickBranchInsert.selectId, "g_branch_1");
assert.deepEqual(quickBranchInsert.instances.map((instance) => instance.id), [
  "existing",
  "g_branch_1",
  "g_branch_1_a",
  "g_branch_1_b",
  "j_branch_1",
]);
assert.deepEqual(quickBranchInsert.edges.map((edge) => edge.id), [
  "e_g_branch_1_g_branch_1_a",
  "e_g_branch_1_g_branch_1_b",
  "e_g_branch_1_a_j_branch_1",
  "e_g_branch_1_b_j_branch_1",
]);
assert.equal(quickBranchInsert.edges[0].kind, "cond");
assert.equal(quickBranchInsert.edges[1].label, "fallback");

assert.equal(controller.flowStepTemplate({ kind: "parallel" }, {
  mob_definition: {
    editor_flow_step_types: ["parallel"],
  },
}), null);

const parallelTemplate = controller.flowStepTemplate({ kind: "parallel" }, graphShapeContract, {
  basicView: { ...hydratedCatalogs.basicView, branchConditionRowTitlePrefix: "Lane" },
});
assert(parallelTemplate.id.startsWith("s_"));
assert.equal(parallelTemplate.type, "parallel");
assert.equal(parallelTemplate.dispatch, "fan_out");
assert.equal(parallelTemplate.collection, "all");
assert.equal(parallelTemplate.dependsMode, "all");
assert.equal(parallelTemplate.branches.length, 2);
assert.deepEqual(parallelTemplate.branches.map((branch) => branch.label), ["Lane 1", "Lane 2"]);

const memberTemplate = controller.flowStepTemplate({ kind: "member", id: "m_left" }, graphShapeContract);
assert(memberTemplate.id.startsWith("s_"));
assert.equal(memberTemplate.type, "member");
assert.equal(memberTemplate.role, "m_left");
assert.equal(memberTemplate.dependsMode, "all");

const collisionFlowTemplate = controller.flowStepTemplate({ kind: "parallel" }, graphShapeContract, {
  basicView: { ...hydratedCatalogs.basicView, branchConditionRowTitlePrefix: "Lane" },
  flow: {
    steps: [
      { id: "s_1", type: "member", role: "m_left" },
      {
        id: "s_2",
        type: "branch",
        branches: [
          { id: "br_1", steps: [] },
          { id: "br_2", steps: [] },
        ],
        fallback: [],
      },
    ],
  },
});
assert.equal(collisionFlowTemplate.id, "s_3");
assert.deepEqual(collisionFlowTemplate.branches.map((branch) => branch.id), ["br_3", "br_4"]);
assert.deepEqual(collisionFlowTemplate.branches.map((branch) => branch.label), ["Lane 1", "Lane 2"]);
assert.deepEqual(controller.flowStepInsertPatch({
  name: "collision-proof",
  steps: [
    { id: "s_1", type: "member", role: "m_left" },
    { id: "s_2", type: "member", role: "m_right" },
  ],
}, { lane: "main", index: 2 }, controller.flowStepTemplate({ kind: "member", id: "m_left" }, graphShapeContract, {
  flow: {
    steps: [
      { id: "s_1", type: "member", role: "m_left" },
      { id: "s_2", type: "member", role: "m_right" },
    ],
  },
}), { members: graphMembers }).steps.map((step) => step.id), ["s_1", "s_2", "s_3"]);

const registryRows = [
  { id: "f_existing", name: "Existing", version: "old", stage: "valid", validation: { ok: true } },
  { id: "f_other", name: "Other", version: "old", stage: "valid", validation: { ok: true } },
];
const draftRows = controller.flowRegistryMarkDraftPatch(registryRows, "f_existing");
assert.notEqual(draftRows, registryRows);
assert.equal(draftRows[0].stage, "draft");
assert.equal(draftRows[0].validation, null);
assert.equal(draftRows[1], registryRows[1]);

const registryDocument = {
  name: "Planner Coder Reviewer",
  mob_id: "planner_coder_reviewer",
  schema_version: "1.0",
  flow: { name: "Mob Flow" },
};
const selectionRows = [
  { id: "f_existing", name: "Existing", version: "old", stage: "valid", validation: { ok: true }, document: registryDocument },
  { id: "f_draft", name: "Draft", version: "draft", stage: "draft" },
];
const documentSelection = controller.flowRegistrySelectionState(selectionRows, "f_existing");
assert.equal(documentSelection.found, true);
assert.equal(documentSelection.hasDocument, true);
assert.equal(documentSelection.row, selectionRows[0]);
assert.deepEqual(documentSelection.hydration.result, {
  document: registryDocument,
  validation: { ok: true },
});
assert.deepEqual(documentSelection.hydration.options, {
  id: "f_existing",
  flowRow: selectionRows[0],
  addToRegistry: false,
});
assert.equal(documentSelection.fallback, null);

const fallbackSelection = controller.flowRegistrySelectionState(selectionRows, "f_draft");
assert.equal(fallbackSelection.found, true);
assert.equal(fallbackSelection.hasDocument, false);
assert.equal(fallbackSelection.hydration, null);
assert.deepEqual(fallbackSelection.fallback, {
  currentFlowId: "f_draft",
  stage: "draft",
  view: "editor",
});

const missingSelection = controller.flowRegistrySelectionState(selectionRows, "missing");
assert.equal(missingSelection.found, false);
assert.equal(missingSelection.row, null);
assert.equal(missingSelection.hydration, null);
assert.equal(missingSelection.fallback, null);

assert.equal(controller.flowImportedIdFromDocument({
  name: "Imported Quality Flow",
}, {}, []), "f_imported_quality_flow");
assert.equal(controller.flowImportedIdFromDocument({
  mob_id: "imported_quality_flow",
}, {}, [{ id: "f_imported_quality_flow" }]), "f_imported_quality_flow_2");
assert.equal(controller.flowImportedIdFromDocument({}, {
  source_name: "special.mobpack",
}, []), "f_specialmobpack");

const rememberedRows = controller.flowRegistryRememberDocumentPatch(registryRows, {
  currentFlowId: "f_existing",
  document: registryDocument,
  validation: { ok: false },
  stage: "draft",
});
assert.equal(rememberedRows[0].name, "Planner Coder Reviewer");
assert.equal(rememberedRows[0].version, "1.0");
assert.equal(rememberedRows[0].document, registryDocument);
assert.deepEqual(rememberedRows[0].validation, { ok: false });

const emptyPersistence = controller.flowRegistryDocumentPersistence({
  currentFlowId: "",
  document: registryDocument,
});
assert.equal(emptyPersistence.ok, false);
assert.equal(emptyPersistence.changed, false);
assert.equal(emptyPersistence.rowPatch, null);

const draftPersistence = controller.flowRegistryDocumentPersistence({
  currentFlowId: "f_existing",
  document: registryDocument,
  stage: "published",
  validation: null,
});
assert.equal(draftPersistence.ok, true);
assert.equal(draftPersistence.changed, true);
assert.equal(draftPersistence.signature, `f_existing\n${JSON.stringify(registryDocument)}`);
assert.equal(draftPersistence.rowPatch.stage, "draft");
assert.equal(draftPersistence.rowPatch.currentFlowId, "f_existing");
assert.equal(draftPersistence.rowPatch.document, registryDocument);
assert.equal(draftPersistence.rowPatch.validation, null);

const validatedPersistence = controller.flowRegistryDocumentPersistence({
  currentFlowId: "f_existing",
  document: registryDocument,
  stage: "published",
  validation: { ok: true },
});
assert.equal(validatedPersistence.rowPatch.stage, "published");
assert.deepEqual(validatedPersistence.rowPatch.validation, { ok: true });

const seededHydrationNoop = controller.flowRegistryDocumentPersistence({
  currentFlowId: "f_existing",
  document: registryDocument,
  stage: "valid",
  validation: null,
  previousSignature: validatedPersistence.signature,
  skipIfUnchanged: true,
});
assert.equal(seededHydrationNoop.ok, true);
assert.equal(seededHydrationNoop.changed, false);
assert.equal(seededHydrationNoop.rowPatch, null);

const unchangedPersistence = controller.flowRegistryDocumentPersistence({
  currentFlowId: "f_existing",
  document: registryDocument,
  previousSignature: draftPersistence.signature,
  skipIfUnchanged: true,
});
assert.equal(unchangedPersistence.ok, true);
assert.equal(unchangedPersistence.changed, false);
assert.equal(unchangedPersistence.signature, draftPersistence.signature);
assert.equal(unchangedPersistence.rowPatch, null);

const persistedRegistryProjection = controller.flowRegistryPersistDocumentProjection(registryRows, {
  currentFlowId: "f_existing",
  document: registryDocument,
  validation: { ok: true },
  stage: "published",
});
assert.equal(persistedRegistryProjection.ok, true);
assert.equal(persistedRegistryProjection.changed, true);
assert.equal(persistedRegistryProjection.rows[0].stage, "published");
assert.deepEqual(persistedRegistryProjection.rows[0].validation, { ok: true });
assert.equal(persistedRegistryProjection.rows[0].document, registryDocument);
assert.equal(persistedRegistryProjection.rows[1], registryRows[1]);

const unchangedRegistryProjection = controller.flowRegistryPersistDocumentProjection(registryRows, {
  currentFlowId: "f_existing",
  document: registryDocument,
  previousSignature: draftPersistence.signature,
  skipIfUnchanged: true,
});
assert.equal(unchangedRegistryProjection.ok, true);
assert.equal(unchangedRegistryProjection.changed, false);
assert.equal(unchangedRegistryProjection.signature, draftPersistence.signature);
assert.equal(unchangedRegistryProjection.rows, registryRows);

const invalidRegistryProjection = controller.flowRegistryPersistDocumentProjection(registryRows, {
  currentFlowId: "",
  document: registryDocument,
});
assert.equal(invalidRegistryProjection.ok, false);
assert.equal(invalidRegistryProjection.changed, false);
assert.equal(invalidRegistryProjection.rows, registryRows);

const persistedOutcomeProjection = controller.flowRegistryPersistOutcomeProjection(registryRows, {
  currentFlowId: "f_existing",
  outcome: {
    document: registryDocument,
    validation: { ok: true },
    validationRows: [{ kind: "ok", head: "valid" }],
    stage: "valid",
  },
});
assert.equal(persistedOutcomeProjection.ok, true);
assert.equal(persistedOutcomeProjection.changed, true);
assert.equal(persistedOutcomeProjection.stage, "valid");
assert.deepEqual(persistedOutcomeProjection.validationRows, [{ kind: "ok", head: "valid" }]);
assert.deepEqual(persistedOutcomeProjection.rows[0].validation, { ok: true });
assert.equal(persistedOutcomeProjection.rows[0].stage, "valid");

const invalidOutcomeProjection = controller.flowRegistryPersistOutcomeProjection(registryRows, {
  currentFlowId: "f_existing",
  outcome: { stage: "draft", validationRows: [{ kind: "crit" }] },
});
assert.equal(invalidOutcomeProjection.ok, false);
assert.equal(invalidOutcomeProjection.changed, false);
assert.equal(invalidOutcomeProjection.stage, "draft");
assert.deepEqual(invalidOutcomeProjection.validationRows, [{ kind: "crit" }]);
assert.equal(invalidOutcomeProjection.rows, registryRows);

const importedRow = controller.flowRegistryRowFromDocument({
  id: "f_imported",
  document: registryDocument,
  validation: { ok: true },
  sourceLabel: "upload.mobpack",
  source: "file:///tmp/upload.mobpack",
  fallbackVersion: "imported",
});
assert.equal(importedRow.name, "Planner Coder Reviewer");
assert.equal(importedRow.version, "1.0");
assert.equal(importedRow.stage, "valid");
assert.equal(importedRow.trigger, "upload.mobpack");
assert.equal(importedRow.source, "file:///tmp/upload.mobpack");

const storedGraphDocument = {
  name: "Stored Graph Import",
  mob_id: "stored_graph_import",
  schema_version: "1.0",
  members: [{ id: "reviewer", name: "Reviewer", role: "reviewer", profileBinding: "inline", runtimeMode: "turn_driven" }],
  schemas: [{ id: "Verdict", fields: [] }],
  flow: { name: "Stored Graph Import", steps: [{ id: "step_1", type: "member", role: "reviewer", task: "Review", instruction: "Review." }] },
  instances: [{ id: "kept_node", memberId: "reviewer", col: 4, row: 2 }],
  edges: [{ id: "kept_edge", from: "kept_node", to: "done", kind: "next" }],
  frames: [{ id: "kept_frame", kind: "Manual", col: 4, row: 2 }],
  skill_realms: [{ id: "imported", default: true, skills: [{ id: "mob.imported", label: "Imported", content: "Do imported work." }] }],
};
const hydratedStored = controller.hydrateMobpackDocumentState({
  document: storedGraphDocument,
  validation: {
    ok: true,
    display_rows: [{
      kind: "ok",
      glyph: "✓",
      head: "Imported mobpack validated",
      sub: "mob.toml",
      meta: "import.ok",
    }],
  },
  source_label: "upload.mobpack",
  source: "file:///tmp/stored.mobpack",
}, {
  id: "f_stored",
  deployDefaults: testDeploySettings(),
  mobDefaults: controller.mobDefaultsFromSchema(TEST_SCHEMA),
  contractSkillRealms: [{ id: "contract", skills: [{ id: "mob.contract", label: "Contract", content: "Contract skill." }] }],
});
assert.equal(hydratedStored.id, "f_stored");
assert.equal(hydratedStored.stage, "valid");
assert.equal(hydratedStored.flow.name, "Stored Graph Import");
assert.deepEqual(hydratedStored.members, storedGraphDocument.members);
assert.deepEqual(hydratedStored.schemas, storedGraphDocument.schemas);
assert.deepEqual(hydratedStored.graphProjection.instances, storedGraphDocument.instances);
assert.deepEqual(hydratedStored.graphProjection.edges, storedGraphDocument.edges);
assert.deepEqual(hydratedStored.graphProjection.frames, storedGraphDocument.frames);
assert.deepEqual(hydratedStored.skillRealms.map(realm => realm.id), ["imported", "contract"]);
assert.equal(hydratedStored.deploySettings.command, "rkat mob deploy");
assert.equal(hydratedStored.mobSettings.backendDefault, "session");
assert.equal(hydratedStored.registryRow.name, "Stored Graph Import");
assert.equal(hydratedStored.registryRow.source, "file:///tmp/stored.mobpack");
assert.equal(hydratedStored.validationRows[0].head, "Imported mobpack validated");
assert.equal(hydratedStored.validationRows[0].meta, "import.ok");

const importedHydrationId = controller.hydrateMobpackDocumentState({
  document: storedGraphDocument,
  validation: { ok: true },
}, {
  existingRows: [{ id: "f_stored_graph_import" }],
});
assert.equal(importedHydrationId.id, "f_stored_graph_import_2");
assert.equal(importedHydrationId.registryRow.id, "f_stored_graph_import_2");

const flowOnlyHydrated = controller.hydrateMobpackDocumentState({
  document: {
    name: "Flow Only Import",
    mob_id: "flow_only_import",
    members: storedGraphDocument.members,
    flow: storedGraphDocument.flow,
  },
}, {
  addToRegistry: false,
  openEditor: false,
});
assert.equal(flowOnlyHydrated.addToRegistry, false);
assert.equal(flowOnlyHydrated.openEditor, false);
assert.equal(flowOnlyHydrated.graphProjection.instances[0].id, "step_1");
assert.equal(flowOnlyHydrated.graphProjection.instances[0].memberId, "reviewer");
assert.deepEqual(flowOnlyHydrated.graphProjection.frames, []);
assert.deepEqual(flowOnlyHydrated.validationRows, []);

const missingFlowHydrated = controller.hydrateMobpackDocumentState({
  document: { name: "No Flow Import", mob_id: "no_flow_import" },
}, {
  deployDefaults: testDeploySettings(),
  mobDefaults: controller.mobDefaultsFromSchema(TEST_SCHEMA),
  errorView: hydratedCatalogs.errorView,
});
assert.equal(missingFlowHydrated.ok, false);
assert.equal(missingFlowHydrated.error, "missing_editor_flow");
assert.equal(missingFlowHydrated.flow, null);
assert.equal(missingFlowHydrated.graphProjection, null);
assert.equal(missingFlowHydrated.addToRegistry, false);
assert.equal(missingFlowHydrated.openEditor, false);
assert.deepEqual(missingFlowHydrated.members, []);
assert.deepEqual(missingFlowHydrated.schemas, []);
assert.equal(missingFlowHydrated.deploySettings.maxTotalTokens, 64);
assert.equal(missingFlowHydrated.mobSettings.backendDefault, "session");
assert.equal(missingFlowHydrated.validationRows[0].kind, "crit");
assert.equal(missingFlowHydrated.validationRows[0].head, "Imported mobpack is missing a MobKit editor flow");
assert.equal(missingFlowHydrated.validationRows[0].meta, "missing_editor_flow");

const appendedRows = controller.flowRegistryAppendRowPatch(registryRows, importedRow);
assert.equal(appendedRows.length, 3);
assert.equal(appendedRows[2], importedRow);

const replacedRows = controller.flowRegistryUpsertRowPatch(registryRows, { ...importedRow, id: "f_existing", name: "Replacement" });
assert.equal(replacedRows.length, 2);
assert.equal(replacedRows[0].name, "Replacement");
assert.equal(replacedRows[1], registryRows[1]);

const patchedDeploy = controller.deploySettingsPatch({
  surface: "cli",
  trustPolicy: "permissive",
  maxToolCalls: 2,
}, {
  trustPolicy: "strict",
  maxTotalTokens: 128,
  prompt: "Run the mob.",
});
assert.equal(patchedDeploy.surface, "cli");
assert.equal(patchedDeploy.trustPolicy, "strict");
assert.equal(patchedDeploy.maxToolCalls, 2);
assert.equal(patchedDeploy.maxTotalTokens, 128);
assert.equal(patchedDeploy.prompt, "Run the mob.");
const settingsContract = {
  deploy_settings: {
    command: "rkat mob deploy",
    surfaces: ["cli"],
    trust_policies: ["permissive"],
    realm_backends: ["jsonl"],
  },
  mob_definition: {
    profile_backends: ["session", "external"],
  },
};
const catalogedDeploy = controller.deploySettingsPatch({
  command: "rkat mob deploy",
  surface: "cli",
  trustPolicy: "permissive",
  realmBackend: "jsonl",
  model: "openai/gpt-5.5",
}, {
  command: "invalid deploy command",
  surface: "rpc",
  trustPolicy: "strict",
  realmBackend: "sqlite",
  model: "openai/ghost",
  prompt: "Still valid.",
}, {
  contract: settingsContract,
  modelCatalog: [{ id: "openai/gpt-5.5" }],
});
assert.equal(catalogedDeploy.command, "rkat mob deploy");
assert.equal(catalogedDeploy.surface, "cli");
assert.equal(catalogedDeploy.trustPolicy, "permissive");
assert.equal(catalogedDeploy.realmBackend, "jsonl");
assert.equal(catalogedDeploy.model, "openai/gpt-5.5");
assert.equal(catalogedDeploy.prompt, "Still valid.");
const validCatalogedDeploy = controller.deploySettingsPatch(catalogedDeploy, {
  model: "",
  realmBackend: "jsonl",
}, {
  contract: settingsContract,
  modelCatalog: [{ id: "openai/gpt-5.5" }],
});
assert.equal(validCatalogedDeploy.model, "");
assert.equal(validCatalogedDeploy.realmBackend, "jsonl");

const patchedMob = controller.mobSettingsPatch({
  orchestrator: "planner",
  roleWiring: [{ a: "planner", b: "coder" }],
}, {
  backendDefault: "external",
  roleWiring: [{ a: "reviewer", b: "planner" }, { a: "", b: "ignored" }],
  topology: { kind: "mesh" },
  arbitraryToml: { leaks: true },
});
assert.equal(patchedMob.orchestrator, "planner");
assert.equal(patchedMob.backendDefault, "external");
assert.deepEqual(patchedMob.roleWiring, [{ a: "reviewer", b: "planner" }]);
assert.equal(patchedMob.topology, undefined);
assert.equal(patchedMob.arbitraryToml, undefined);
const catalogedMob = controller.mobSettingsPatch({
  backendDefault: "session",
  orchestrator: "planner",
}, {
  backendDefault: "daemon",
  externalAddressBase: "http://127.0.0.1:9000",
}, {
  contract: settingsContract,
});
assert.equal(catalogedMob.backendDefault, "session");
assert.equal(catalogedMob.externalAddressBase, "http://127.0.0.1:9000");
assert.equal(controller.mobSettingsPatch(catalogedMob, { backendDefault: "" }, { contract: settingsContract }).backendDefault, "");
assert.equal(controller.mobSettingsPatch(catalogedMob, { backendDefault: "sidecar" }, {
  contract: { mob_definition: { profile_backends: ["session", "sidecar"] } },
}).backendDefault, "sidecar");

const roleRules = [{ a: "planner", b: "coder" }, { a: "coder", b: "reviewer" }];
assert.deepEqual(
  controller.mobRoleWiringEditorState(
    [{ a: " planner ", b: "coder" }, { a: "", b: "ignored" }],
    [{ value: "planner", label: "Planner" }, { value: "coder", label: "Coder" }],
    TEST_SETTINGS_VIEW,
  ),
  {
    label: "Role wiring",
    countLabel: "1",
    addLabel: "+ rule",
    addDisabled: false,
    options: [{ value: "planner", label: "Planner" }, { value: "coder", label: "Coder" }],
    wiring: [{ a: "planner", b: "coder" }],
  },
);
assert.equal(controller.mobRoleWiringEditorState([], [], TEST_SETTINGS_VIEW).addDisabled, true);
const roleOptions = [{ value: "planner" }, { value: "coder" }, { value: "reviewer" }];
assert.deepEqual(
  controller.mobRoleWiringUpdatePatch(roleRules, 1, { b: "planner" }, roleOptions),
  [{ a: "planner", b: "coder" }, { a: "coder", b: "planner" }],
);
assert.deepEqual(
  controller.mobRoleWiringUpdatePatch(roleRules, 1, { b: "ghost_profile" }, roleOptions),
  [{ a: "planner", b: "coder" }],
);
assert.deepEqual(
  controller.mobRoleWiringDeletePatch(roleRules, 0),
  [{ a: "coder", b: "reviewer" }],
);
assert.deepEqual(
  controller.mobRoleWiringAddPatch(roleRules, [{ value: "reviewer" }, { value: "planner" }]),
  [{ a: "planner", b: "coder" }, { a: "coder", b: "reviewer" }, { a: "reviewer", b: "planner" }],
);

assert.deepEqual(controller.advancedMobSettingsEditorState({ limits: { max_members: 3 } }, TEST_SETTINGS_VIEW), {
  label: "Advanced",
  text: '{\n  "limits": {\n    "max_members": 3\n  }\n}',
});
const advancedOk = controller.advancedMobSettingsDraftPatch('{"limits":{"max_members":3},"spawnPolicy":{"mode":"manual"}}', TEST_SETTINGS_VIEW);
assert.equal(advancedOk.ok, true);
assert.deepEqual(advancedOk.value, {
  topology: null,
  supervisor: null,
  limits: { max_members: 3 },
  spawnPolicy: { mode: "manual" },
  eventRouter: null,
});
assert.equal(controller.advancedMobSettingsDraftPatch("[]", TEST_SETTINGS_VIEW).error, "object required");
assert.equal(controller.advancedMobSettingsDraftPatch("{", TEST_SETTINGS_VIEW).ok, false);

const enumField = { type: "string", enumValues: [] };
assert.deepEqual(controller.schemaLikeFieldTypePatch(enumField, "enum", graphShapeContract), { type: "enum", enumValues: ["value"] });
assert.deepEqual(controller.schemaLikeFieldTypePatch({ type: "enum", enumValues: ["green"] }, "string", graphShapeContract), { type: "string", enumValues: [] });
assert.deepEqual(controller.schemaLikeFieldTypePatch({ type: "string", enumValues: [] }, "object", graphShapeContract), {});
const schemaTypeControlState = controller.schemaLikeFieldTypeControlState({}, graphShapeContract);
assert.equal(schemaTypeControlState.type, "string");
assert.equal(schemaTypeControlState.selectedType.label, "string");
assert.deepEqual(schemaTypeControlState.typeOptions.map((option) => [option.value, option.label, option.disabled]), [
  ["string", "string", false],
  ["enum", "enum — fixed choices", false],
]);
const unsupportedSchemaTypeControlState = controller.schemaLikeFieldTypeControlState({ type: "object" }, graphShapeContract);
assert.equal(unsupportedSchemaTypeControlState.type, "object");
assert.equal(unsupportedSchemaTypeControlState.selectedType.disabled, true);
assert.match(unsupportedSchemaTypeControlState.selectedType.reason, /mob_definition\.editor_schema_field_types/);
const schemaFieldRowState = controller.schemaFieldRowControlState(
  { type: "enum", enumValues: ["green"] },
  graphShapeContract,
  hydratedCatalogs.schemaView,
);
assert.equal(schemaFieldRowState.namePlaceholder, "field_name");
assert.equal(schemaFieldRowState.descriptionPlaceholder, "—");
assert.equal(schemaFieldRowState.removeTitle, "Remove field");
assert.equal(schemaFieldRowState.enumLabel, "VALUES");
assert.equal(schemaFieldRowState.enumAddLabel, "+ value");
assert.equal(schemaFieldRowState.enumAddValue, "value");
assert.deepEqual(schemaFieldRowState.enumValues, ["green"]);
assert.equal(schemaFieldRowState.typeState.type, "enum");
const inputParamFieldState = controller.inputParamFieldControlState(
  { type: "enum", enumValues: ["green"] },
  graphShapeContract,
  hydratedCatalogs.basicView,
);
assert.equal(inputParamFieldState.namePlaceholder, "param_name");
assert.equal(inputParamFieldState.descriptionPlaceholder, "—");
assert.equal(inputParamFieldState.removeTitle, "Remove param");
assert.equal(inputParamFieldState.enumLabel, "VALUES");
assert.equal(inputParamFieldState.enumAddLabel, "+ value");
assert.equal(inputParamFieldState.enumAddValue, "value");
assert.deepEqual(inputParamFieldState.enumValues, ["green"]);
assert.equal(inputParamFieldState.typeState.type, "enum");
assert.deepEqual(controller.enumValueDraftPatch({ enumValues: ["green"] }, 0, ""), { enumValues: [""] });
assert.deepEqual(controller.enumValueCommitPatch({ enumValues: ["green", "red"] }, 1, "green"), { enumValues: ["green", "green_2"] });
assert.deepEqual(controller.enumValueDeletePatch({ enumValues: ["green", "red"] }, 0), { enumValues: ["red"] });
assert.deepEqual(controller.enumValueAddPatch({ enumValues: ["value"] }, "value"), { enumValues: ["value", "value_2"] });

const studioStateA = {
  members: [{ id: "m1" }],
  instances: [{ id: "i1" }],
  edges: [],
  frames: [],
  schemas: [],
  skillRealms: [{ id: "realm" }],
};
const studioStateB = {
  members: [{ id: "m2" }],
  instances: [{ id: "i2" }],
  edges: [{ id: "e1" }],
  frames: [{ id: "f1" }],
  schemas: [{ id: "s1" }],
  skillRealms: [],
};
const snapPatch = controller.studioHistorySnapshotPatch({
  history: Array.from({ length: 31 }, (_, index) => ({ members: [{ id: `m${index}` }] })),
  future: [studioStateB],
  state: studioStateA,
});
assert.equal(snapPatch.history.length, 31);
assert.equal(snapPatch.history[snapPatch.history.length - 1].members[0].id, "m1");
assert.deepEqual(snapPatch.future, []);

const undoPatch = controller.studioUndoPatch({ history: [studioStateA], future: [], state: studioStateB });
assert.deepEqual(undoPatch.state.members, [{ id: "m1" }]);
assert.equal(undoPatch.history.length, 0);
assert.deepEqual(undoPatch.future[0].members, [{ id: "m2" }]);
assert.equal(controller.studioUndoPatch({ history: [], future: [], state: studioStateB }), null);

const redoPatch = controller.studioRedoPatch({ history: [], future: [studioStateB], state: studioStateA });
assert.deepEqual(redoPatch.state.members, [{ id: "m2" }]);
assert.deepEqual(redoPatch.history[0].members, [{ id: "m1" }]);
assert.equal(redoPatch.future.length, 0);
assert.equal(controller.studioRedoPatch({ history: [], future: [], state: studioStateA }), null);

console.log("controller projection metadata ok");
