var __create = Object.create;
var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __getProtoOf = Object.getPrototypeOf;
var __hasOwnProp = Object.prototype.hasOwnProperty;
var __export = (target, all) => {
  for (var name in all)
    __defProp(target, name, { get: all[name], enumerable: true });
};
var __copyProps = (to, from, except, desc) => {
  if (from && typeof from === "object" || typeof from === "function") {
    for (let key of __getOwnPropNames(from))
      if (!__hasOwnProp.call(to, key) && key !== except)
        __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
  }
  return to;
};
var __toESM = (mod, isNodeMode, target) => (target = mod != null ? __create(__getProtoOf(mod)) : {}, __copyProps(
  // If the importer is in node compatibility mode or this is not an ESM
  // file that has been converted to a CommonJS file using a Babel-
  // compatible transform (i.e. "__esModule" has not been set), then set
  // "default" to the CommonJS "module.exports" for node compatibility.
  isNodeMode || !mod || !mod.__esModule ? __defProp(target, "default", { value: mod, enumerable: true }) : target,
  mod
));
var __toCommonJS = (mod) => __copyProps(__defProp({}, "__esModule", { value: true }), mod);

// src/index.tsx
var index_exports = {};
__export(index_exports, {
  ConsoleApp: () => ConsoleApp,
  createConsoleApp: () => createConsoleApp,
  parseSseFrames: () => parseSseFrames
});
module.exports = __toCommonJS(index_exports);
var import_client = require("react-dom/client");

// src/ConsoleApp.tsx
var import_react6 = __toESM(require("react"));

// node_modules/clsx/dist/clsx.mjs
function r(e) {
  var t, f, n = "";
  if ("string" == typeof e || "number" == typeof e) n += e;
  else if ("object" == typeof e) if (Array.isArray(e)) {
    var o = e.length;
    for (t = 0; t < o; t++) e[t] && (f = r(e[t])) && (n && (n += " "), n += f);
  } else for (f in e) e[f] && (n && (n += " "), n += f);
  return n;
}
function clsx() {
  for (var e, t, f = 0, n = "", o = arguments.length; f < o; f++) (e = arguments[f]) && (t = r(e)) && (n && (n += " "), n += t);
  return n;
}
var clsx_default = clsx;

// ../packages/console-components/src/shared.ts
function toneStyle(tone) {
  if (!tone?.variables) {
    return void 0;
  }
  return tone.variables;
}
function fallbackCopyTextToClipboard(text) {
  if (typeof document === "undefined" || !document.body || typeof document.execCommand !== "function") {
    return false;
  }
  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.setAttribute("readonly", "true");
  textarea.style.position = "fixed";
  textarea.style.top = "0";
  textarea.style.left = "0";
  textarea.style.opacity = "0";
  textarea.style.pointerEvents = "none";
  document.body.appendChild(textarea);
  const selection = typeof document.getSelection === "function" ? document.getSelection() : null;
  const existingRanges = selection ? Array.from({ length: selection.rangeCount }, (_value, index) => selection.getRangeAt(index)) : [];
  textarea.focus();
  textarea.select();
  textarea.setSelectionRange(0, textarea.value.length);
  let copied = false;
  try {
    copied = document.execCommand("copy");
  } catch {
    copied = false;
  }
  document.body.removeChild(textarea);
  if (selection) {
    selection.removeAllRanges();
    existingRanges.forEach((range) => selection.addRange(range));
  }
  return copied;
}
async function copyTextToClipboard(text) {
  if (!text.trim()) {
    return false;
  }
  if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch {
    }
  }
  return fallbackCopyTextToClipboard(text);
}

// ../packages/console-components/src/activity/console-activity-rail.tsx
var import_jsx_runtime = require("react/jsx-runtime");
function PinButton({
  Icon: Icon2,
  item,
  onTogglePin
}) {
  if (!item.pinId || !onTogglePin) {
    return null;
  }
  return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(
    "button",
    {
      type: "button",
      className: clsx_default("cc-activity-rail__pin", item.pinned && "is-active"),
      title: item.pinned ? `Unpin ${item.title}` : `Pin ${item.title}`,
      "aria-label": item.pinned ? `Unpin ${item.title}` : `Pin ${item.title}`,
      onClick: (event) => {
        event.stopPropagation();
        onTogglePin(item.pinId, Boolean(item.pinned));
      },
      children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon2, { name: "i-pin" })
    }
  );
}
function RosterPanel({
  Icon: Icon2,
  panel,
  onRemovePanel,
  onSelectItem,
  onTogglePin
}) {
  return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("section", { className: "cc-activity-rail__section", children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { className: "cc-activity-rail__section-row", children: [
      /* @__PURE__ */ (0, import_jsx_runtime.jsx)("h2", { children: panel.title }),
      /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { className: "cc-activity-rail__section-actions", children: [
        panel.meta ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "cc-activity-rail__section-meta", children: panel.meta }) : null,
        panel.actions?.map((action) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(
          "button",
          {
            type: "button",
            className: clsx_default("cc-activity-rail__section-action", action.active && "is-active"),
            "data-testid": `activity-action:${panel.id}:${action.id}`,
            onClick: () => onPanelAction?.(panel.id, action.id),
            children: action.label
          },
          action.id
        )),
        onRemovePanel ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(
          "button",
          {
            type: "button",
            className: "cc-activity-rail__section-action",
            onClick: () => onRemovePanel(panel.id),
            children: "Hide"
          }
        ) : null
      ] })
    ] }),
    panel.groups.length ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "cc-activity-rail__roster-groups", children: panel.groups.map((group) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("section", { className: clsx_default("cc-activity-rail__roster-group", group.inactive && "is-inactive"), children: [
      /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { className: "cc-activity-rail__roster-group-header", children: [
        /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "cc-activity-rail__roster-group-title", children: group.title }),
        group.meta ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "cc-activity-rail__roster-group-meta", children: group.meta }) : null
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "cc-activity-rail__roster-grid", children: group.items.map((item) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(
        "div",
        {
          className: clsx_default("cc-activity-rail__roster-item", item.selected && "is-selected"),
          style: toneStyle(item.tone),
          children: [
            /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(
              "button",
              {
                type: "button",
                className: "cc-activity-rail__roster-main",
                title: item.tooltip || item.subtitle || item.title,
                onClick: () => item.focusId && onSelectItem?.(item.focusId),
                children: [
                  /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "cc-activity-rail__roster-status" }),
                  /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", { className: "cc-activity-rail__roster-copy", children: [
                    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "cc-activity-rail__roster-label", children: item.title }),
                    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "cc-activity-rail__roster-meta", children: item.subtitle || item.meta })
                  ] })
                ]
              }
            ),
            /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PinButton, { Icon: Icon2, item, onTogglePin })
          ]
        },
        item.id
      )) })
    ] }, group.id)) }) : panel.emptyText ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "cc-activity-rail__empty", children: panel.emptyText }) : null
  ] }, panel.id);
}
function PulsePanel({
  Icon: Icon2,
  panel,
  onRemovePanel,
  onPanelAction: onPanelAction2,
  onSelectItem,
  onTogglePin
}) {
  return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("section", { className: "cc-activity-rail__section", children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { className: "cc-activity-rail__section-row", children: [
      /* @__PURE__ */ (0, import_jsx_runtime.jsx)("h2", { children: panel.title }),
      /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { className: "cc-activity-rail__section-actions", children: [
        panel.meta ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "cc-activity-rail__section-meta", children: panel.meta }) : null,
        panel.actions?.map((action) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(
          "button",
          {
            type: "button",
            className: clsx_default("cc-activity-rail__section-action", action.active && "is-active"),
            "data-testid": `activity-action:${panel.id}:${action.id}`,
            onClick: () => onPanelAction2?.(panel.id, action.id),
            children: action.label
          },
          action.id
        )),
        onRemovePanel ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(
          "button",
          {
            type: "button",
            className: "cc-activity-rail__section-action",
            onClick: () => onRemovePanel(panel.id),
            children: "Hide"
          }
        ) : null
      ] })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "cc-activity-rail__pulse-list", children: panel.items.length ? panel.items.map((item) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(
      "div",
      {
        className: clsx_default("cc-activity-rail__pulse-row", item.selected && "is-selected"),
        style: toneStyle(item.tone),
        children: [
          /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(
            "button",
            {
              type: "button",
              className: "cc-activity-rail__pulse-main",
              "data-testid": `activity-item:${panel.id}:${item.id}`,
              title: item.tooltip || `${item.title} \xB7 ${item.line}`,
              onClick: () => item.focusId && onSelectItem?.(item.focusId),
              children: [
                /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "cc-activity-rail__pulse-status" }),
                /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", { className: "cc-activity-rail__pulse-copy", children: [
                  /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", { className: "cc-activity-rail__pulse-head", children: [
                    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "cc-activity-rail__pulse-label", children: item.title }),
                    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "cc-activity-rail__pulse-time", children: item.meta })
                  ] }),
                  /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "cc-activity-rail__pulse-line", children: item.line })
                ] })
              ]
            }
          ),
          /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PinButton, { Icon: Icon2, item, onTogglePin })
        ]
      },
      item.id
    )) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "cc-activity-rail__empty", children: panel.emptyText }) })
  ] }, panel.id);
}
function FeedPanel({
  Icon: Icon2,
  panel,
  onRemovePanel,
  onSelectItem,
  onTogglePin,
  onPanelAction: onPanelAction2,
  renderSlotPreview
}) {
  return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("section", { className: "cc-activity-rail__section cc-activity-rail__section--feed", children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { className: "cc-activity-rail__section-row", children: [
      /* @__PURE__ */ (0, import_jsx_runtime.jsx)("h2", { children: panel.title }),
      /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { className: "cc-activity-rail__section-actions", children: [
        panel.actions?.map((action) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(
          "button",
          {
            type: "button",
            className: clsx_default("cc-activity-rail__section-action", action.active && "is-active"),
            onClick: () => onPanelAction2?.(panel.id, action.id),
            children: action.label
          },
          action.id
        )),
        onRemovePanel ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(
          "button",
          {
            type: "button",
            className: "cc-activity-rail__section-action",
            onClick: () => onRemovePanel(panel.id),
            children: "Hide"
          }
        ) : null
      ] })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "cc-activity-rail__feed-slots", children: panel.slots.map((slot) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(
      "section",
      {
        className: clsx_default("cc-activity-rail__feed-item", slot.selected && "is-selected", slot.focusId && "has-item"),
        style: toneStyle(slot.tone),
        children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { className: "cc-activity-rail__feed-frame", children: [
          /* @__PURE__ */ (0, import_jsx_runtime.jsx)(
            "button",
            {
              type: "button",
              className: "cc-activity-rail__feed-button",
              title: slot.subtitle || slot.title,
              onClick: () => slot.focusId && onSelectItem?.(slot.focusId),
              disabled: !slot.focusId,
              children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { className: "cc-activity-rail__feed-canvas", children: [
                renderSlotPreview(slot),
                /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "cc-activity-rail__feed-overlay", children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { className: "cc-activity-rail__feed-overlay-top", children: [
                  /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "cc-activity-rail__feed-eyebrow", children: slot.eyebrow }),
                  /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "cc-activity-rail__feed-title", children: slot.title }),
                  /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "cc-activity-rail__feed-meta", children: slot.meta })
                ] }) })
              ] })
            }
          ),
          /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "cc-activity-rail__feed-actions", children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(
            PinButton,
            {
              Icon: Icon2,
              item: {
                pinId: slot.pinId,
                pinned: slot.pinned,
                title: slot.title
              },
              onTogglePin
            }
          ) })
        ] })
      },
      slot.id
    )) })
  ] }, panel.id);
}
function renderPanel({
  Icon: Icon2,
  panel,
  onRemovePanel,
  onSelectItem,
  onTogglePin,
  onPanelAction: onPanelAction2,
  renderSlotPreview
}) {
  if (panel.kind === "roster") {
    return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(
      RosterPanel,
      {
        Icon: Icon2,
        onRemovePanel,
        onSelectItem,
        onTogglePin,
        panel
      },
      panel.id
    );
  }
  if (panel.kind === "pulse") {
    return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(
      PulsePanel,
      {
        Icon: Icon2,
        onRemovePanel,
        onPanelAction: onPanelAction2,
        onSelectItem,
        onTogglePin,
        panel
      },
      panel.id
    );
  }
  return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(
    FeedPanel,
    {
      Icon: Icon2,
      onPanelAction: onPanelAction2,
      onRemovePanel,
      onSelectItem,
      onTogglePin,
      panel,
      renderSlotPreview
    },
    panel.id
  );
}
function ConsoleActivityRail({
  Icon: Icon2,
  viewState,
  addPanelButtonRef,
  onTogglePicker,
  onCollapse,
  onEmptyAction,
  onFooterAction,
  onIngressSelect,
  onRemovePanel,
  onSelectItem,
  onTogglePin,
  onPanelAction: onPanelAction2,
  renderSlotPreview
}) {
  return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { className: "cc-theme-scope cc-activity-rail", children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { className: "cc-activity-rail__controls", children: [
      /* @__PURE__ */ (0, import_jsx_runtime.jsx)(
        "button",
        {
          ref: addPanelButtonRef,
          className: "cc-activity-rail__control",
          type: "button",
          title: "Add panel",
          "aria-label": "Add panel",
          onClick: onTogglePicker,
          children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon2, { name: "i-plus" })
        }
      ),
      /* @__PURE__ */ (0, import_jsx_runtime.jsx)(
        "button",
        {
          className: "cc-activity-rail__control",
          type: "button",
          title: "Collapse live panels",
          "aria-label": "Collapse live panels",
          onClick: onCollapse,
          children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon2, { name: "i-sidebar-toggle" })
        }
      )
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("aside", { className: "cc-activity-rail__rail", children: [
      /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { className: "cc-activity-rail__scroll", children: [
        viewState.ingress ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "cc-activity-rail__ingress", children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(
          "button",
          {
            type: "button",
            className: clsx_default(
              "cc-activity-rail__ingress-button",
              viewState.ingress.active && "is-active",
              viewState.ingress.prominent && "is-prominent"
            ),
            onClick: onIngressSelect,
            children: [
              /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "cc-activity-rail__ingress-status", "aria-hidden": "true" }),
              /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", { className: "cc-activity-rail__ingress-copy", children: [
                /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "cc-activity-rail__ingress-title", children: viewState.ingress.label }),
                /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "cc-activity-rail__ingress-meta", children: viewState.ingress.meta })
              ] })
            ]
          }
        ) }) : null,
        viewState.panels.length ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "cc-activity-rail__content", children: viewState.panels.map((panel) => renderPanel({
          Icon: Icon2,
          panel,
          onRemovePanel,
          onSelectItem,
          onTogglePin,
          onPanelAction: onPanelAction2,
          renderSlotPreview
        })) }) : viewState.emptyState ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { className: "cc-activity-rail__empty-shell", children: [
          /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "cc-activity-rail__empty-title", children: viewState.emptyState.title }),
          /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "cc-activity-rail__empty-copy", children: viewState.emptyState.description }),
          /* @__PURE__ */ (0, import_jsx_runtime.jsx)(
            "button",
            {
              type: "button",
              className: "cc-activity-rail__empty-action",
              onClick: onEmptyAction || onTogglePicker,
              children: viewState.emptyState.actionLabel
            }
          )
        ] }) : null
      ] }),
      viewState.footerActionLabel && onFooterAction ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "cc-activity-rail__footer", children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", { className: "cc-activity-rail__footer-button", type: "button", onClick: onFooterAction, children: [
        /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon2, { name: "i-team" }),
        /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: viewState.footerActionLabel })
      ] }) }) : null
    ] })
  ] });
}

// ../packages/console-components/src/conversation/conversation-empty-state.tsx
var import_jsx_runtime2 = require("react/jsx-runtime");
function ConversationEmptyState({
  state,
  Icon: Icon2,
  className,
  onApplySuggestion
}) {
  return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("section", { className: clsx_default("cc-empty-state", className), children: [
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "cc-empty-state__mark", "aria-hidden": "true", children: Icon2 && state.iconName ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Icon2, { name: state.iconName }) : null }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h2", { className: "cc-empty-state__title", children: state.title }),
    state.projectLabel ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "cc-empty-state__project", children: state.projectLabel }) : null,
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "cc-empty-state__subtitle", children: state.subtitle }),
    state.suggestions?.length ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "cc-empty-state__actions", children: state.suggestions.map((suggestion) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(
      "button",
      {
        type: "button",
        className: "cc-empty-state__card",
        onClick: () => onApplySuggestion?.(suggestion.value),
        children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "cc-empty-state__card-icon", "aria-hidden": "true", children: Icon2 && suggestion.iconName ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Icon2, { name: suggestion.iconName }) : null }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "cc-empty-state__card-text", children: suggestion.label })
        ]
      },
      suggestion.id
    )) }) : null
  ] });
}

// ../packages/console-core/src/control-plane.ts
function trimString(value) {
  if (typeof value !== "string") {
    return void 0;
  }
  const trimmed = value.trim();
  return trimmed || void 0;
}
function stringRecord(value) {
  if (!value || typeof value !== "object") {
    return {};
  }
  return Object.fromEntries(
    Object.entries(value).map(([key, raw]) => {
      const normalizedKey = trimString(key);
      const normalizedValue = trimString(raw);
      return normalizedKey && normalizedValue ? [normalizedKey, normalizedValue] : null;
    }).filter((entry) => Boolean(entry))
  );
}
function normalizeResponsePhase(value) {
  switch (value) {
    case "waiting":
    case "tool-executing":
    case "generating":
      return value;
    case null:
    case void 0:
      return null;
    default:
      return null;
  }
}
function normalizeFiniteNumber(value) {
  return typeof value === "number" && Number.isFinite(value) ? value : void 0;
}
function normalizeSidebarWatchFields(value) {
  const record = value && typeof value === "object" ? value : {};
  const normalized = {};
  if (typeof record.watched === "boolean") {
    normalized.watched = record.watched;
  }
  if (record.alertLevel === "elevated" || record.alertLevel === "critical" || record.alertLevel === null) {
    normalized.alertLevel = record.alertLevel;
  }
  if (typeof record.degraded === "boolean") {
    normalized.degraded = record.degraded;
  }
  const degradedReason = trimString(record.degradedReason);
  if (degradedReason) {
    normalized.degradedReason = degradedReason;
  }
  return normalized;
}
function normalizeConsoleInteractionAccepted(value) {
  const record = value && typeof value === "object" ? value : null;
  if (!record) {
    return null;
  }
  const interactionId = trimString(record.interaction_id);
  const identity = trimString(record.identity);
  if (!interactionId || !identity) {
    return null;
  }
  return { interaction_id: interactionId, identity };
}
function normalizeIdentityStatusRow(value) {
  const record = value && typeof value === "object" ? value : null;
  if (!record) {
    return null;
  }
  const identity = trimString(record.identity);
  const state = trimString(record.state);
  if (!identity || !state) {
    return null;
  }
  const addressability = record.addressability === "internal_only" ? "internal_only" : record.addressability === "addressable" ? "addressable" : null;
  if (!addressability) {
    return null;
  }
  return {
    identity,
    state,
    addressability,
    labels: stringRecord(record.labels),
    ...trimString(record.display_name) ? { display_name: trimString(record.display_name) } : {},
    ...trimString(record.profile) ? { profile: trimString(record.profile) } : {},
    ...typeof record.generation === "number" && Number.isFinite(record.generation) ? { generation: record.generation } : {},
    ...typeof record.checkpoint_version === "number" && Number.isFinite(record.checkpoint_version) ? { checkpoint_version: record.checkpoint_version } : {},
    ...typeof record.lease_healthy === "boolean" ? { lease_healthy: record.lease_healthy } : {}
  };
}
function normalizeRoutingSectionView(value) {
  const record = value && typeof value === "object" ? value : null;
  if (!record) {
    return null;
  }
  const routes = Array.isArray(record.routes) ? record.routes.map((entry) => {
    const route = entry && typeof entry === "object" ? entry : null;
    if (!route) {
      return null;
    }
    const routeKey = trimString(route.route_key);
    const recipient = trimString(route.recipient);
    const sink = trimString(route.sink);
    const targetModule = trimString(route.target_module);
    if (!routeKey || !recipient || !sink || !targetModule) {
      return null;
    }
    return {
      route_key: routeKey,
      recipient,
      sink,
      target_module: targetModule,
      ...trimString(route.channel) ? { channel: trimString(route.channel) } : {},
      ...normalizeFiniteNumber(route.retry_max) !== void 0 ? { retry_max: normalizeFiniteNumber(route.retry_max) } : {},
      ...normalizeFiniteNumber(route.backoff_ms) !== void 0 ? { backoff_ms: normalizeFiniteNumber(route.backoff_ms) } : {},
      ...normalizeFiniteNumber(route.rate_limit_per_minute) !== void 0 ? { rate_limit_per_minute: normalizeFiniteNumber(route.rate_limit_per_minute) } : {}
    };
  }).filter((entry) => Boolean(entry)) : [];
  const deliveries = Array.isArray(record.deliveries) ? record.deliveries.map((entry) => {
    const delivery = entry && typeof entry === "object" ? entry : null;
    if (!delivery) {
      return null;
    }
    const deliveryId = trimString(delivery.delivery_id);
    const routeId = trimString(delivery.route_id);
    const recipient = trimString(delivery.recipient);
    const sink = trimString(delivery.sink);
    const targetModule = trimString(delivery.target_module);
    const status = trimString(delivery.status);
    const firstAttempt = normalizeFiniteNumber(delivery.first_attempt_ms);
    const finalAttempt = normalizeFiniteNumber(delivery.final_attempt_ms);
    if (!deliveryId || !routeId || !recipient || !sink || !targetModule || !status || firstAttempt === void 0 || finalAttempt === void 0) {
      return null;
    }
    const attempts = Array.isArray(delivery.attempts) ? delivery.attempts.map((attemptRaw) => {
      const attempt = attemptRaw && typeof attemptRaw === "object" ? attemptRaw : null;
      if (!attempt) {
        return null;
      }
      const attemptNumber = normalizeFiniteNumber(attempt.attempt);
      const attemptStatus = trimString(attempt.status);
      const backoff = normalizeFiniteNumber(attempt.backoff_ms);
      if (attemptNumber === void 0 || !attemptStatus || backoff === void 0) {
        return null;
      }
      return {
        attempt: attemptNumber,
        status: attemptStatus,
        backoff_ms: backoff
      };
    }).filter((attempt) => Boolean(attempt)) : [];
    return {
      delivery_id: deliveryId,
      route_id: routeId,
      recipient,
      sink,
      target_module: targetModule,
      status,
      first_attempt_ms: firstAttempt,
      final_attempt_ms: finalAttempt,
      attempts,
      ...trimString(delivery.idempotency_key) ? { idempotency_key: trimString(delivery.idempotency_key) } : {},
      ...trimString(delivery.sink_adapter) ? { sink_adapter: trimString(delivery.sink_adapter) } : {}
    };
  }).filter((entry) => Boolean(entry)) : [];
  return { routes, deliveries };
}
function normalizeReplayUnavailableError(value) {
  const record = value && typeof value === "object" ? value : null;
  if (!record || record.error !== "replay_unavailable") {
    return null;
  }
  const stream = record.stream === "identity" || record.stream === "all_events" ? record.stream : null;
  const requested = trimString(record.requested_last_event_id);
  const latest = trimString(record.latest_event_id);
  if (!stream || !requested || !latest) {
    return null;
  }
  return {
    error: "replay_unavailable",
    stream,
    requested_last_event_id: requested,
    latest_event_id: latest
  };
}
function normalizeConsoleInteractionRejectedError(value) {
  const record = value && typeof value === "object" ? value : null;
  if (!record) {
    return null;
  }
  const code = record.code;
  const message = trimString(record.message);
  if (code !== -32001 && code !== -32002 && code !== -32003 && code !== -32004 && code !== -32602 && code !== -32603) {
    return null;
  }
  if (!message) {
    return null;
  }
  return { code, message };
}

// ../packages/console-core/src/rich-content.ts
var FILE_CHANGE_RE = /^(Created|Updated|Modified|Deleted)\b/i;
var TERMINAL_DURATION_RE = /^Worked for\s+.+$/i;
var TERMINAL_STATUS_RE = /^(Success|Running|Failed|Cancelled)$/i;
function escapeHtml(value) {
  return String(value || "").replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;").replace(/'/g, "&#39;");
}
function renderConversationInlineMarkdown(text) {
  const codeTokens = [];
  const escaped = escapeHtml(text || "").replace(/`([^`]+)`/g, (_match, code) => {
    const index = codeTokens.push(`<code class="cc-rich-inline-code">${code}</code>`) - 1;
    return `@@CODE_${index}@@`;
  }).replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>").replace(/\[([^\]]+)\]\(([^)]+)\)/g, '<a href="$2">$1</a>').replace(/\n/g, "<br />");
  return escaped.replace(/@@CODE_(\d+)@@/g, (_match, index) => codeTokens[Number(index)] || "");
}
function conversationRichBlockHasCopyAction(block) {
  return block.type === "code" || block.type === "command" || block.type === "file-change";
}
function conversationRichBlockCopyText(block) {
  switch (block.type) {
    case "code":
      return block.body.trim();
    case "command":
      return [block.title, block.body, block.output || "", block.footer || ""].filter(Boolean).join("\n").trim();
    case "file-change":
      return [
        block.verb,
        block.before || "",
        block.name,
        block.after || "",
        `+${block.plus}`,
        `-${block.minus}`
      ].filter(Boolean).join(" ").replace(/\s+/g, " ").trim();
    case "table":
      return [
        block.headers.join(" | "),
        ...block.rows.map((row) => row.join(" | "))
      ].join("\n").trim();
    case "heading":
      return block.text.trim();
    case "paragraph":
    case "divider":
      return block.text.trim();
    case "thinking":
      return [block.label, block.text].filter(Boolean).join("\n").trim();
    default:
      return "";
  }
}
function conversationRichBlocksToText(blocks) {
  return (blocks || []).map((block) => conversationRichBlockCopyText(block)).filter(Boolean).join("\n\n").trim();
}
function parseConversationRichBlocks(content) {
  const source = String(content || "").trim();
  if (!source) {
    return [];
  }
  const blocks = [];
  const fenceRe = /```([^\n`]*)\n([\s\S]*?)```/g;
  let lastIndex = 0;
  let match;
  while (match = fenceRe.exec(source)) {
    const before = source.slice(lastIndex, match.index);
    blocks.push(...parseConversationTextBlocks(before));
    blocks.push({
      type: "code",
      language: (match[1] || "text").trim() || "text",
      body: match[2].replace(/\n+$/u, "")
    });
    lastIndex = fenceRe.lastIndex;
  }
  blocks.push(...parseConversationTextBlocks(source.slice(lastIndex)));
  return compactConversationBlocks(blocks);
}
function parseConversationTextBlocks(fragment) {
  const source = String(fragment || "").trim();
  if (!source) {
    return [];
  }
  const sections = source.split(/\n{2,}/u).map((section) => section.trim()).filter(Boolean);
  const blocks = [];
  for (const section of sections) {
    const heading = parseConversationHeadingBlock(section);
    if (heading) {
      blocks.push(...heading);
      continue;
    }
    const table = parseConversationTableBlock(section);
    if (table) {
      blocks.push(table);
      continue;
    }
    const fileChange = parseConversationFileChangeBlock(section);
    if (fileChange) {
      blocks.push(fileChange);
      continue;
    }
    const command = parseConversationCommandBlock(section);
    if (command) {
      blocks.push(command);
      continue;
    }
    if (TERMINAL_DURATION_RE.test(section)) {
      blocks.push({ type: "divider", text: section });
      continue;
    }
    const normalized = section.replace(/^\s*[-*]\s+/gm, "").replace(/\n{2,}/g, "\n").trim();
    if (normalized) {
      blocks.push({ type: "paragraph", text: normalized });
    }
  }
  return blocks;
}
function compactConversationBlocks(blocks) {
  const deduped = [];
  for (const block of blocks) {
    const previous = deduped.at(-1);
    if (block.type === "paragraph" && previous?.type === "file-change" && previous.name && block.text.startsWith(previous.name)) {
      continue;
    }
    deduped.push(block);
  }
  return deduped;
}
function parseConversationHeadingBlock(section) {
  const lines = String(section || "").split(/\n/u).map((line) => line.trim()).filter(Boolean);
  if (!lines.length || !lines[0].startsWith("#")) {
    return null;
  }
  const headingMatch = lines[0].match(/^(#{1,6})\s+(.+)$/u);
  if (!headingMatch) {
    return null;
  }
  const blocks = [{
    type: "heading",
    level: headingMatch[1].length,
    text: headingMatch[2].trim()
  }];
  const rest = lines.slice(1).join("\n").trim();
  if (rest) {
    blocks.push({ type: "paragraph", text: rest });
  }
  return blocks;
}
function splitMarkdownTableRow(line) {
  const source = String(line || "").trim().replace(/^\|/u, "").replace(/\|$/u, "");
  const cells = [];
  let current = "";
  let escaping = false;
  let codeFenceDepth = 0;
  for (const character of source) {
    if (escaping) {
      current += character;
      escaping = false;
      continue;
    }
    if (character === "\\") {
      escaping = true;
      continue;
    }
    if (character === "`") {
      codeFenceDepth = codeFenceDepth === 0 ? 1 : 0;
      current += character;
      continue;
    }
    if (character === "|" && codeFenceDepth === 0) {
      cells.push(current.trim());
      current = "";
      continue;
    }
    current += character;
  }
  cells.push(current.trim());
  return cells;
}
function parseTableAlignment(cells) {
  if (!cells.length || !cells.every((cell) => /^:?-{3,}:?$/u.test(cell))) {
    return null;
  }
  return cells.map((cell) => {
    const trimmed = cell.trim();
    if (trimmed.startsWith(":") && trimmed.endsWith(":")) {
      return "center";
    }
    if (trimmed.endsWith(":")) {
      return "right";
    }
    return "left";
  });
}
function parseConversationTableBlock(section) {
  const lines = String(section || "").split(/\n/u).map((line) => line.trim()).filter(Boolean);
  if (lines.length < 2) {
    return null;
  }
  const headers = splitMarkdownTableRow(lines[0]);
  const alignments = parseTableAlignment(splitMarkdownTableRow(lines[1]));
  if (!headers.length || !alignments || headers.length !== alignments.length) {
    return null;
  }
  const rows = lines.slice(2).map((line) => splitMarkdownTableRow(line)).filter((cells) => cells.length > 0 && cells.some((cell) => cell.length > 0)).map((cells) => headers.map((_header, index) => cells[index] || ""));
  return {
    type: "table",
    headers,
    alignments,
    rows
  };
}
function parseConversationFileChangeBlock(section) {
  const compact = String(section || "").replace(/\s*\n\s*/g, " ").trim();
  if (!compact) {
    return null;
  }
  const header = compact.match(FILE_CHANGE_RE);
  if (!header) {
    return null;
  }
  const verb = header[1];
  const statsMatch = compact.match(/\s+\+([\d,]+)\s+-([\d,]+)\s*$/u);
  const plus = Number.parseInt((statsMatch?.[1] || "1").replaceAll(",", ""), 10) || 0;
  const minus = Number.parseInt((statsMatch?.[2] || "0").replaceAll(",", ""), 10) || 0;
  const body = statsMatch ? compact.slice(0, statsMatch.index).trim() : compact;
  const fileMatches = [...body.matchAll(/`([^`]+)`/gu)];
  const fileMatch = fileMatches.find((candidate) => !candidate[1].includes("/")) || fileMatches[0];
  if (!fileMatch) {
    return null;
  }
  const fileToken = fileMatch[0];
  const fileName = fileMatch[1].trim();
  const bodyAfterVerb = body.slice(verb.length).trim();
  const tokenIndex = bodyAfterVerb.indexOf(fileToken);
  const before = tokenIndex >= 0 ? bodyAfterVerb.slice(0, tokenIndex).trim() : "";
  const after = tokenIndex >= 0 ? bodyAfterVerb.slice(tokenIndex + fileToken.length).trim() : bodyAfterVerb.replace(fileToken, "").trim();
  return {
    type: "file-change",
    verb,
    before,
    name: fileName,
    after,
    plus,
    minus
  };
}
function parseConversationCommandBlock(section) {
  const lines = String(section || "").split(/\n/u).map((line) => line.replace(/\s+$/u, "")).filter((line) => line.trim().length > 0);
  if (!lines.length) {
    return null;
  }
  const commandIndex = lines.findIndex((line) => line.trim().startsWith("$ "));
  if (commandIndex === -1) {
    return null;
  }
  const command = lines[commandIndex].trim();
  const prefix = lines.slice(0, commandIndex).filter(Boolean);
  const footerCandidate = lines.at(-1)?.trim() || "";
  const footer = TERMINAL_STATUS_RE.test(footerCandidate) ? footerCandidate : "";
  const outputStart = commandIndex + 1;
  const outputEnd = footer ? lines.length - 1 : lines.length;
  const output = lines.slice(outputStart, outputEnd).join("\n").trim();
  return {
    type: "command",
    caption: prefix[0] || "Ran command",
    title: prefix[1] || "Shell",
    body: command,
    output,
    footer
  };
}

// ../packages/console-core/src/conversation.ts
function conversationIdentityPresentation(identity) {
  if (identity?.presentation) {
    return identity.presentation;
  }
  if (identity?.role === "user") {
    return "user";
  }
  if (identity?.role === "system") {
    return "system";
  }
  if (identity?.role === "other") {
    return "participant";
  }
  return "assistant";
}
function conversationIdentityShowsLabel(identity) {
  if (!identity?.label) {
    return false;
  }
  if (typeof identity.showLabel === "boolean") {
    return identity.showLabel;
  }
  const presentation = conversationIdentityPresentation(identity);
  return presentation === "participant" || presentation === "system";
}
function conversationIdentityGroupKey(identity) {
  if (!identity) {
    return "unknown:assistant:hidden";
  }
  return [
    identity.id || "unknown",
    conversationIdentityPresentation(identity),
    conversationIdentityShowsLabel(identity) ? "label" : "hidden"
  ].join(":");
}
function conversationEntryText(entry) {
  if (entry.kind === "summary") {
    const fileLines = entry.files.map((file) => `${file.name} +${file.plus} -${file.minus}`).join("\n");
    return [entry.title, fileLines].filter(Boolean).join("\n");
  }
  return String(entry.copyText || entry.text || conversationRichBlocksToText(entry.blocks)).trim();
}
function conversationMessageHasIntrinsicCopyAction(entry) {
  if (entry.kind !== "message" || entry.variant !== "rich") {
    return false;
  }
  return Boolean(entry.blocks?.some((block) => conversationRichBlockHasCopyAction(block)));
}
function groupConversationTimelineEntries(entries) {
  const groups = [];
  for (const entry of entries) {
    const current = groups.at(-1);
    if (!current || conversationIdentityGroupKey(current.identity) !== conversationIdentityGroupKey(entry.identity)) {
      groups.push({
        id: `${entry.identity.id}-${entry.id}`,
        identity: entry.identity,
        entries: [entry],
        copyText: conversationEntryText(entry)
      });
      continue;
    }
    current.entries.push(entry);
    const nextCopyText = conversationEntryText(entry);
    current.copyText = [current.copyText, nextCopyText].filter(Boolean).join("\n\n");
  }
  return groups;
}

// ../packages/console-core/src/dock.ts
var CONSOLE_DOCK_PRESETS = [
  {
    id: "single",
    label: "Single",
    description: "One focused panel.",
    iconName: "i-compose"
  },
  {
    id: "two_columns",
    label: "Two columns",
    description: "Side-by-side work.",
    iconName: "i-sidebar-toggle"
  },
  {
    id: "two_rows",
    label: "Two rows",
    description: "Top and bottom pairing.",
    iconName: "i-swap"
  },
  {
    id: "grid",
    label: "Grid",
    description: "A 2x2 comparison layout.",
    iconName: "i-team"
  }
];
function isDockPanelNode(node) {
  return Boolean(node && node.kind === "panel" && node.panelId);
}
function isDockSplitNode(node) {
  return Boolean(
    node && node.kind === "split" && node.id && (node.direction === "horizontal" || node.direction === "vertical") && node.first && node.second
  );
}
function normalizeTarget(target) {
  if (!target?.id || !target?.kind || !target?.title) {
    return null;
  }
  return target;
}
function normalizePanelState(panel) {
  if (!panel?.id) {
    return null;
  }
  return {
    id: panel.id,
    target: normalizeTarget(panel.target),
    mode: panel.mode === "terminal" ? "terminal" : "console"
  };
}
function normalizeNode(node, validPanelIds) {
  if (isDockPanelNode(node)) {
    return validPanelIds.has(node.panelId) ? { kind: "panel", panelId: node.panelId } : null;
  }
  if (!isDockSplitNode(node)) {
    return null;
  }
  const first = normalizeNode(node.first, validPanelIds);
  const second = normalizeNode(node.second, validPanelIds);
  if (first && second) {
    return {
      kind: "split",
      id: node.id,
      direction: node.direction,
      ratio: typeof node.ratio === "number" && node.ratio > 0 && node.ratio < 1 ? node.ratio : 0.5,
      first,
      second
    };
  }
  return first || second;
}
function panelNode(panelId) {
  return { kind: "panel", panelId };
}
function presetMeta(presetId) {
  return CONSOLE_DOCK_PRESETS.find((entry) => entry.id === presetId) || CONSOLE_DOCK_PRESETS[0];
}
function uniqueTargets(values, excludedIds) {
  const usedIds = new Set(excludedIds);
  const results = [];
  for (const target of values) {
    if (!target) {
      results.push(null);
      continue;
    }
    if (usedIds.has(target.id)) {
      continue;
    }
    usedIds.add(target.id);
    results.push(target);
  }
  return results;
}
function suggestDockTargets({
  count,
  preferred = null,
  excludedIds = [],
  suggestTargets
}) {
  const suggested = uniqueTargets(
    suggestTargets?.({ count, preferred: preferred || null, excludedIds }) || [],
    excludedIds
  );
  const results = [];
  const usedIds = new Set(excludedIds);
  for (const target of suggested) {
    if (!target) {
      results.push(null);
      continue;
    }
    if (usedIds.has(target.id)) {
      continue;
    }
    usedIds.add(target.id);
    results.push(target);
    if (results.length >= count) {
      return results;
    }
  }
  while (results.length < count) {
    if (preferred && !usedIds.has(preferred.id)) {
      usedIds.add(preferred.id);
      results.push(preferred);
    } else {
      results.push(null);
    }
  }
  return results;
}
function replacePanelStates(panels, nextPanels) {
  const nextById = new Map(nextPanels.map((panel) => [panel.id, panel]));
  const filtered = panels.filter((panel) => !nextById.has(panel.id));
  return [...filtered, ...nextPanels];
}
function consoleDockPresets() {
  return CONSOLE_DOCK_PRESETS;
}
function collectConsoleDockPanelIds(node) {
  if (isDockPanelNode(node)) {
    return [node.panelId];
  }
  if (!isDockSplitNode(node)) {
    return [];
  }
  return [
    ...collectConsoleDockPanelIds(node.first),
    ...collectConsoleDockPanelIds(node.second)
  ];
}
function findConsoleDockFirstPanelId(node) {
  if (isDockPanelNode(node)) {
    return node.panelId;
  }
  if (!isDockSplitNode(node)) {
    return null;
  }
  return findConsoleDockFirstPanelId(node.first) || findConsoleDockFirstPanelId(node.second);
}
function replaceConsoleDockPanelNode(node, panelId, replacement) {
  if (node.kind === "panel") {
    return node.panelId === panelId ? replacement : node;
  }
  return {
    ...node,
    first: replaceConsoleDockPanelNode(node.first, panelId, replacement),
    second: replaceConsoleDockPanelNode(node.second, panelId, replacement)
  };
}
function removeConsoleDockPanelNode(node, panelId) {
  if (!node) {
    return null;
  }
  if (node.kind === "panel") {
    return node.panelId === panelId ? null : node;
  }
  const nextFirst = removeConsoleDockPanelNode(node.first, panelId);
  const nextSecond = removeConsoleDockPanelNode(node.second, panelId);
  if (nextFirst && nextSecond) {
    return {
      ...node,
      first: nextFirst,
      second: nextSecond
    };
  }
  return nextFirst || nextSecond;
}
function clampConsoleDockSplitRatio(ratio) {
  if (typeof ratio !== "number" || Number.isNaN(ratio)) {
    return 0.5;
  }
  return Math.min(0.88, Math.max(0.12, ratio));
}
function updateConsoleDockSplitRatio(node, splitId, ratio) {
  if (node.kind === "panel") {
    return node;
  }
  if (node.id === splitId) {
    return {
      ...node,
      ratio: clampConsoleDockSplitRatio(ratio)
    };
  }
  return {
    ...node,
    first: updateConsoleDockSplitRatio(node.first, splitId, ratio),
    second: updateConsoleDockSplitRatio(node.second, splitId, ratio)
  };
}
function consoleDockSplitDirectionAxis(direction) {
  return direction === "left" || direction === "right" ? "horizontal" : "vertical";
}
function consoleDockSplitDirectionPrecedes(direction) {
  return direction === "left" || direction === "up";
}
function normalizeConsoleDockState(state) {
  const panels = (state?.panels || []).map((panel) => normalizePanelState(panel)).filter(Boolean);
  const validPanelIds = new Set(panels.map((panel) => panel.id));
  const tabs = (state?.tabs || []).filter((tab) => Boolean(tab?.id)).map((tab) => ({
    id: tab.id,
    presetId: tab.presetId || "single",
    layout: normalizeNode(tab.layout, validPanelIds)
  })).filter((tab) => Boolean(tab.layout));
  const activeTabId = tabs.some((tab) => tab.id === state?.activeTabId) ? state?.activeTabId || null : tabs[0]?.id || null;
  const activeTab = tabs.find((tab) => tab.id === activeTabId) || null;
  const activePanelIds = activeTab ? collectConsoleDockPanelIds(activeTab.layout) : [];
  const focusedPanelId = state?.focusedPanelId && activePanelIds.includes(state.focusedPanelId) ? state.focusedPanelId : activePanelIds[0] || null;
  return {
    tabs,
    panels,
    activeTabId,
    focusedPanelId
  };
}
function buildConsoleDockPresetState({
  presetId,
  preferredTarget = null,
  preferredPanel = null,
  createPanelState,
  createSplitId,
  suggestTargets
}) {
  const requestedCount = presetId === "grid" ? 4 : presetId === "single" ? 1 : 2;
  const [firstTarget, secondTarget, thirdTarget, fourthTarget] = suggestDockTargets({
    count: requestedCount,
    preferred: preferredTarget,
    excludedIds: [],
    suggestTargets
  });
  const primary = createPanelState({
    target: preferredPanel ? preferredTarget ?? preferredPanel.target : firstTarget || null,
    sourcePanel: preferredPanel || null
  });
  if (presetId === "single") {
    return {
      presetId,
      layout: panelNode(primary.id),
      panels: [primary],
      focusedPanelId: primary.id
    };
  }
  if (presetId === "two_columns") {
    const right = createPanelState({ target: secondTarget || null, sourcePanel: preferredPanel || primary });
    return {
      presetId,
      layout: {
        kind: "split",
        id: createSplitId(),
        direction: "horizontal",
        ratio: 0.5,
        first: panelNode(primary.id),
        second: panelNode(right.id)
      },
      panels: [primary, right],
      focusedPanelId: primary.id
    };
  }
  if (presetId === "two_rows") {
    const bottom = createPanelState({ target: secondTarget || null, sourcePanel: preferredPanel || primary });
    return {
      presetId,
      layout: {
        kind: "split",
        id: createSplitId(),
        direction: "vertical",
        ratio: 0.5,
        first: panelNode(primary.id),
        second: panelNode(bottom.id)
      },
      panels: [primary, bottom],
      focusedPanelId: primary.id
    };
  }
  const rightTop = createPanelState({ target: secondTarget || null, sourcePanel: preferredPanel || primary });
  const leftBottom = createPanelState({ target: thirdTarget || null, sourcePanel: preferredPanel || primary });
  const rightBottom = createPanelState({ target: fourthTarget || null, sourcePanel: preferredPanel || primary });
  return {
    presetId,
    layout: {
      kind: "split",
      id: createSplitId(),
      direction: "horizontal",
      ratio: 0.5,
      first: {
        kind: "split",
        id: createSplitId(),
        direction: "vertical",
        ratio: 0.5,
        first: panelNode(primary.id),
        second: panelNode(leftBottom.id)
      },
      second: {
        kind: "split",
        id: createSplitId(),
        direction: "vertical",
        ratio: 0.5,
        first: panelNode(rightTop.id),
        second: panelNode(rightBottom.id)
      }
    },
    panels: [primary, rightTop, leftBottom, rightBottom],
    focusedPanelId: primary.id
  };
}
function createConsoleDockState({
  initialTarget = null,
  initialPresetId = "single",
  createPanelState,
  createTabId,
  createSplitId,
  suggestTargets
}) {
  const initial = buildConsoleDockPresetState({
    presetId: initialPresetId,
    preferredTarget: initialTarget,
    createPanelState,
    createSplitId,
    suggestTargets
  });
  const firstTabId = createTabId();
  return {
    tabs: [{
      id: firstTabId,
      presetId: initialPresetId,
      layout: initial.layout
    }],
    panels: initial.panels,
    activeTabId: firstTabId,
    focusedPanelId: initial.focusedPanelId
  };
}
function selectConsoleDockTab(state, tabId) {
  const normalized = normalizeConsoleDockState(state);
  const tab = normalized.tabs.find((entry) => entry.id === tabId) || null;
  const focusedPanelId = tab ? findConsoleDockFirstPanelId(tab.layout) : normalized.focusedPanelId;
  return {
    ...normalized,
    activeTabId: tab ? tab.id : normalized.activeTabId,
    focusedPanelId
  };
}
function focusConsoleDockPanel(state, panelId) {
  const normalized = normalizeConsoleDockState(state);
  return normalized.panels.some((panel) => panel.id === panelId) ? {
    ...normalized,
    focusedPanelId: panelId
  } : normalized;
}
function setConsoleDockPanelTarget(state, panelId, target) {
  const normalized = normalizeConsoleDockState(state);
  return {
    ...normalized,
    panels: normalized.panels.map((panel) => panel.id === panelId ? {
      ...panel,
      target: normalizeTarget(target)
    } : panel)
  };
}
function setConsoleDockPanelMode(state, panelId, mode) {
  const normalized = normalizeConsoleDockState(state);
  return {
    ...normalized,
    panels: normalized.panels.map((panel) => panel.id === panelId ? {
      ...panel,
      mode
    } : panel)
  };
}
function createConsoleDockTab(state, options) {
  const normalized = normalizeConsoleDockState(state);
  const preferredPanel = normalized.focusedPanelId ? normalized.panels.find((panel) => panel.id === normalized.focusedPanelId) || null : null;
  const presetState = buildConsoleDockPresetState({
    presetId: "single",
    preferredTarget: preferredPanel?.target || null,
    preferredPanel,
    createPanelState: options.createPanelState,
    createSplitId: options.createSplitId,
    suggestTargets: options.suggestTargets
  });
  const tabId = options.createTabId();
  return {
    ...normalized,
    tabs: [
      ...normalized.tabs,
      {
        id: tabId,
        presetId: "single",
        layout: presetState.layout
      }
    ],
    panels: replacePanelStates(normalized.panels, presetState.panels),
    activeTabId: tabId,
    focusedPanelId: presetState.focusedPanelId
  };
}
function closeConsoleDockTab(state, tabId, options) {
  const normalized = normalizeConsoleDockState(state);
  const closingIndex = normalized.tabs.findIndex((tab) => tab.id === tabId);
  if (closingIndex < 0) {
    return normalized;
  }
  if (normalized.tabs.length <= 1) {
    return createConsoleDockState({
      initialPresetId: "single",
      createPanelState: options.createPanelState,
      createSplitId: options.createSplitId,
      createTabId: () => normalized.tabs[0]?.id || options.createTabId(),
      suggestTargets: options.suggestTargets
    });
  }
  const closingTab = normalized.tabs[closingIndex];
  const removePanelIds = new Set(collectConsoleDockPanelIds(closingTab.layout));
  const nextTabs = normalized.tabs.filter((tab) => tab.id !== tabId);
  const nextActiveTabId = normalized.activeTabId === tabId ? nextTabs[Math.max(0, closingIndex - 1)]?.id || nextTabs[0]?.id || null : normalized.activeTabId;
  const nextState = {
    tabs: nextTabs,
    panels: normalized.panels.filter((panel) => !removePanelIds.has(panel.id)),
    activeTabId: nextActiveTabId,
    focusedPanelId: normalized.focusedPanelId
  };
  return normalizeConsoleDockState(nextState);
}
function openConsoleDockTarget(state, target, options) {
  const intent = options.intent || "replace_focused";
  const normalized = normalizeConsoleDockState(state);
  if (intent === "new_tab") {
    const presetState = buildConsoleDockPresetState({
      presetId: "single",
      preferredTarget: target,
      createPanelState: options.createPanelState,
      createSplitId: options.createSplitId,
      suggestTargets: options.suggestTargets
    });
    const tabId = options.createTabId();
    return {
      ...normalized,
      tabs: [
        ...normalized.tabs,
        {
          id: tabId,
          presetId: "single",
          layout: presetState.layout
        }
      ],
      panels: replacePanelStates(normalized.panels, presetState.panels),
      activeTabId: tabId,
      focusedPanelId: presetState.focusedPanelId
    };
  }
  if (intent === "split_right" || intent === "split_down") {
    const activeTab = normalized.tabs.find((tab) => tab.id === normalized.activeTabId) || null;
    const focusedPanel = normalized.focusedPanelId ? normalized.panels.find((panel) => panel.id === normalized.focusedPanelId) || null : null;
    if (!activeTab || !focusedPanel) {
      return normalized;
    }
    const direction = intent === "split_right" ? "right" : "down";
    const nextPanel = options.createPanelState({
      target,
      sourcePanel: focusedPanel
    });
    const replacement = {
      kind: "split",
      id: options.createSplitId(),
      direction: consoleDockSplitDirectionAxis(direction),
      ratio: 0.5,
      first: panelNode(focusedPanel.id),
      second: panelNode(nextPanel.id)
    };
    return {
      ...normalized,
      tabs: normalized.tabs.map((tab) => tab.id === activeTab.id ? {
        ...tab,
        layout: replaceConsoleDockPanelNode(tab.layout, focusedPanel.id, replacement)
      } : tab),
      panels: replacePanelStates(normalized.panels, [nextPanel]),
      focusedPanelId: nextPanel.id
    };
  }
  if (!normalized.focusedPanelId) {
    return normalized;
  }
  return setConsoleDockPanelTarget(normalized, normalized.focusedPanelId, target);
}
function resizeConsoleDockSplit(state, splitId, ratio) {
  const normalized = normalizeConsoleDockState(state);
  const activeTab = normalized.tabs.find((tab) => tab.id === normalized.activeTabId) || null;
  if (!activeTab) {
    return normalized;
  }
  return {
    ...normalized,
    tabs: normalized.tabs.map((tab) => tab.id === activeTab.id ? {
      ...tab,
      layout: updateConsoleDockSplitRatio(tab.layout, splitId, ratio)
    } : tab)
  };
}
function splitConsoleDockPanel(state, panelId, direction, options) {
  const normalized = normalizeConsoleDockState(state);
  const activeTab = normalized.tabs.find((tab) => tab.id === normalized.activeTabId) || null;
  const panel = normalized.panels.find((entry) => entry.id === panelId) || null;
  if (!activeTab || !panel) {
    return normalized;
  }
  const excludedIds = collectConsoleDockPanelIds(activeTab.layout).map((id) => normalized.panels.find((entry) => entry.id === id)?.target?.id || "").filter(Boolean);
  const suggestedTarget = suggestDockTargets({
    count: 1,
    preferred: panel.target,
    excludedIds,
    suggestTargets: options.suggestTargets
  })[0] || panel.target || null;
  const nextPanel = options.createPanelState({
    target: suggestedTarget,
    sourcePanel: panel
  });
  const replacement = {
    kind: "split",
    id: options.createSplitId(),
    direction: consoleDockSplitDirectionAxis(direction),
    ratio: 0.5,
    first: consoleDockSplitDirectionPrecedes(direction) ? panelNode(nextPanel.id) : panelNode(panelId),
    second: consoleDockSplitDirectionPrecedes(direction) ? panelNode(panelId) : panelNode(nextPanel.id)
  };
  return {
    ...normalized,
    tabs: normalized.tabs.map((tab) => tab.id === activeTab.id ? {
      ...tab,
      layout: replaceConsoleDockPanelNode(tab.layout, panelId, replacement)
    } : tab),
    panels: replacePanelStates(normalized.panels, [nextPanel]),
    focusedPanelId: nextPanel.id
  };
}
function closeConsoleDockPanel(state, panelId) {
  const normalized = normalizeConsoleDockState(state);
  const activeTab = normalized.tabs.find((tab) => tab.id === normalized.activeTabId) || null;
  const panel = normalized.panels.find((entry) => entry.id === panelId) || null;
  if (!activeTab || !panel) {
    return normalized;
  }
  if (collectConsoleDockPanelIds(activeTab.layout).length <= 1) {
    return {
      ...normalized,
      panels: normalized.panels.map((entry) => entry.id === panelId ? {
        ...entry,
        target: null
      } : entry),
      focusedPanelId: panelId
    };
  }
  const nextLayout = removeConsoleDockPanelNode(activeTab.layout, panelId);
  if (!nextLayout) {
    return normalized;
  }
  return {
    ...normalized,
    tabs: normalized.tabs.map((tab) => tab.id === activeTab.id ? {
      ...tab,
      layout: nextLayout
    } : tab),
    panels: normalized.panels.filter((entry) => entry.id !== panelId),
    focusedPanelId: findConsoleDockFirstPanelId(nextLayout)
  };
}
function applyConsoleDockPreset(state, options) {
  const normalized = normalizeConsoleDockState(state);
  const activeTab = normalized.tabs.find((tab) => tab.id === normalized.activeTabId) || null;
  const focusedPanel = normalized.focusedPanelId ? normalized.panels.find((panel) => panel.id === normalized.focusedPanelId) || null : null;
  if (!activeTab) {
    return normalized;
  }
  const presetState = buildConsoleDockPresetState({
    presetId: options.presetId,
    preferredTarget: focusedPanel?.target || null,
    preferredPanel: focusedPanel,
    createPanelState: options.createPanelState,
    createSplitId: options.createSplitId,
    suggestTargets: options.suggestTargets
  });
  const currentPanelIds = new Set(collectConsoleDockPanelIds(activeTab.layout));
  return {
    ...normalized,
    tabs: normalized.tabs.map((tab) => tab.id === activeTab.id ? {
      ...tab,
      presetId: options.presetId,
      layout: presetState.layout
    } : tab),
    panels: replacePanelStates(
      normalized.panels.filter((panel) => !currentPanelIds.has(panel.id)),
      presetState.panels
    ),
    focusedPanelId: presetState.focusedPanelId
  };
}
function applyConsoleDockAction(state, action, options) {
  switch (action.type) {
    case "create_tab":
      return createConsoleDockTab(state, options);
    case "select_tab":
      return action.tabId ? selectConsoleDockTab(state, action.tabId) : state;
    case "close_tab":
      return action.tabId ? closeConsoleDockTab(state, action.tabId, options) : state;
    case "focus_panel":
      return action.panelId ? focusConsoleDockPanel(state, action.panelId) : state;
    case "set_panel_target":
      return action.panelId ? setConsoleDockPanelTarget(state, action.panelId, action.target || null) : state;
    case "set_panel_mode":
      return action.panelId && action.mode ? setConsoleDockPanelMode(state, action.panelId, action.mode) : state;
    case "open_target":
      return action.target ? openConsoleDockTarget(state, action.target, {
        ...options,
        intent: action.intent
      }) : state;
    case "resize_split":
      return action.splitId && typeof action.ratio === "number" ? resizeConsoleDockSplit(state, action.splitId, action.ratio) : state;
    case "split_panel":
      return action.panelId && action.direction ? splitConsoleDockPanel(state, action.panelId, action.direction, options) : state;
    case "close_panel":
      return action.panelId ? closeConsoleDockPanel(state, action.panelId) : state;
    case "apply_preset":
      return action.presetId ? applyConsoleDockPreset(state, {
        presetId: action.presetId,
        createPanelState: options.createPanelState,
        createSplitId: options.createSplitId,
        suggestTargets: options.suggestTargets
      }) : state;
    default:
      return state;
  }
}
function buildConsoleDockViewState(state, options = {}) {
  const normalized = normalizeConsoleDockState(state);
  const panelsById = new Map(normalized.panels.map((panel) => [panel.id, panel]));
  return {
    activeTabId: normalized.activeTabId,
    focusedPanelId: normalized.focusedPanelId,
    tabs: normalized.tabs.map((tab) => {
      const panelStates = collectConsoleDockPanelIds(tab.layout).map((panelId) => panelsById.get(panelId)).filter(Boolean);
      const firstTarget = panelStates.find((panel) => panel.target)?.target || null;
      const preset = presetMeta(tab.presetId);
      const resolved = options.resolveTabView?.({
        tab,
        panels: panelStates,
        active: tab.id === normalized.activeTabId,
        focusedPanelId: normalized.focusedPanelId
      }) || {};
      return {
        id: tab.id,
        title: resolved.title || firstTarget?.title || preset.label,
        subtitle: resolved.subtitle ?? firstTarget?.subtitle ?? preset.description,
        iconName: resolved.iconName ?? firstTarget?.iconName ?? preset.iconName,
        badgeLabel: resolved.badgeLabel ?? (panelStates.length > 1 ? `x${panelStates.length}` : null),
        closable: resolved.closable ?? true,
        dirty: resolved.dirty ?? false,
        layout: tab.layout
      };
    }),
    panels: normalized.tabs.flatMap((tab) => {
      const activePanelIds = collectConsoleDockPanelIds(tab.layout);
      const activePanelCount = activePanelIds.length;
      return activePanelIds.flatMap((panelId) => {
        const panel = panelsById.get(panelId);
        if (!panel) {
          return [];
        }
        const resolved = options.resolvePanelView?.({
          panel,
          activePanelCount,
          focused: normalized.focusedPanelId === panel.id
        }) || {};
        return [{
          id: panel.id,
          title: resolved.title || panel.target?.title || "Open something",
          subtitle: resolved.subtitle ?? panel.target?.subtitle ?? "Use the launcher or activity rail to open a target.",
          iconName: resolved.iconName ?? panel.target?.iconName ?? "i-compose",
          target: panel.target,
          mode: panel.mode,
          statusLabel: resolved.statusLabel ?? (panel.target ? "Active target" : "Ready"),
          badgeLabel: resolved.badgeLabel ?? panel.target?.badgeLabel ?? null,
          dirty: resolved.dirty ?? false,
          closable: resolved.closable ?? activePanelCount > 1
        }];
      });
    })
  };
}
function normalizeConsoleDockViewState(viewState) {
  const panels = (viewState?.panels || []).filter((panel) => Boolean(panel?.id && panel?.title)).map((panel) => ({
    ...panel,
    target: normalizeTarget(panel.target),
    mode: panel.mode === "terminal" ? "terminal" : "console"
  }));
  const validPanelIds = new Set(panels.map((panel) => panel.id));
  const tabs = (viewState?.tabs || []).filter((tab) => Boolean(tab?.id && tab?.title)).map((tab) => ({
    ...tab,
    layout: normalizeNode(tab.layout, validPanelIds)
  })).filter((tab) => Boolean(tab.layout));
  const activeTabId = tabs.some((tab) => tab.id === viewState?.activeTabId) ? viewState?.activeTabId || null : tabs[0]?.id || null;
  const activeTab = tabs.find((tab) => tab.id === activeTabId) || null;
  const activePanelIds = activeTab ? collectConsoleDockPanelIds(activeTab.layout) : [];
  const focusedPanelId = viewState?.focusedPanelId && activePanelIds.includes(viewState.focusedPanelId) ? viewState.focusedPanelId : activePanelIds[0] || null;
  return {
    tabs,
    panels,
    activeTabId,
    focusedPanelId
  };
}

// ../packages/console-core/src/sidebar.ts
function normalizeMeta(meta) {
  return (meta || []).filter((item) => Boolean(item?.label));
}
function normalizeActions(actions) {
  return (actions || []).filter((action) => Boolean(action?.id && action?.label));
}
function normalizeItems(items) {
  return (items || []).filter((item) => Boolean(item?.id && item?.title)).map((item) => ({
    ...item,
    ...normalizeSidebarWatchFields(item),
    meta: normalizeMeta(item.meta),
    actions: normalizeActions(item.actions)
  }));
}
function normalizeSections(sections) {
  return (sections || []).filter((section) => Boolean(section?.id && typeof section?.title === "string")).map((section) => ({
    ...section,
    meta: normalizeMeta(section.meta),
    actions: normalizeActions(section.actions),
    items: normalizeItems(section.items)
  })).filter((section) => {
    if (section.items.length > 0) {
      return true;
    }
    return Boolean(
      section.title || section.subtitle || section.iconName || section.actions.length || section.meta.length
    );
  });
}
function normalizeConsoleSidebarViewState(viewState) {
  const blocks = (viewState?.blocks || []).filter((block) => Boolean(block?.id && block?.kind)).map((block) => ({
    ...block,
    meta: normalizeMeta(block.meta),
    actions: normalizeActions(block.actions),
    sections: normalizeSections(block.sections)
  })).filter((block) => {
    if (block.kind === "action_strip") {
      return block.actions.length > 0;
    }
    if (block.sections.length > 0) {
      return true;
    }
    return Boolean(block.title || block.meta.length || block.actions.length);
  });
  return { blocks };
}

// ../packages/console-core/src/format.ts
function formatCount(value) {
  return new Intl.NumberFormat("en-US").format(Number(value) || 0);
}

// ../packages/console-components/src/conversation/conversation-message-view.tsx
var import_react2 = require("react");

// ../packages/console-components/src/conversation/change-stat-pair.tsx
var import_jsx_runtime3 = require("react/jsx-runtime");
function ChangeStatPair({
  plus,
  minus,
  className
}) {
  return /* @__PURE__ */ (0, import_jsx_runtime3.jsxs)("span", { className: clsx_default("cc-change-stat", className), children: [
    /* @__PURE__ */ (0, import_jsx_runtime3.jsxs)("span", { className: "cc-change-stat__value is-plus", children: [
      "+",
      formatCount(plus)
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime3.jsxs)("span", { className: "cc-change-stat__value is-minus", children: [
      "-",
      formatCount(minus)
    ] })
  ] });
}

// ../packages/console-components/src/copy-button.tsx
var import_react = require("react");
var import_jsx_runtime4 = require("react/jsx-runtime");
function CopyButton({
  text,
  label,
  copiedLabel = "Copied",
  className,
  Icon: Icon2
}) {
  const [copied, setCopied] = (0, import_react.useState)(false);
  const resetTimerRef = (0, import_react.useRef)(null);
  const disabled = !text.trim();
  (0, import_react.useEffect)(() => () => {
    if (resetTimerRef.current != null) {
      window.clearTimeout(resetTimerRef.current);
    }
  }, []);
  async function handleClick() {
    if (disabled) {
      return;
    }
    const wasCopied = await copyTextToClipboard(text);
    if (!wasCopied) {
      return;
    }
    setCopied(true);
    if (resetTimerRef.current != null) {
      window.clearTimeout(resetTimerRef.current);
    }
    resetTimerRef.current = window.setTimeout(() => {
      setCopied(false);
      resetTimerRef.current = null;
    }, 1600);
  }
  return /* @__PURE__ */ (0, import_jsx_runtime4.jsx)(
    "button",
    {
      className: clsx_default("cc-copy-btn", className),
      type: "button",
      "aria-label": copied ? copiedLabel : label,
      title: copied ? copiedLabel : label,
      "data-copied": copied ? "true" : void 0,
      disabled,
      onClick: () => {
        void handleClick();
      },
      children: Icon2 ? /* @__PURE__ */ (0, import_jsx_runtime4.jsx)(Icon2, { name: copied ? "i-check" : "i-copy" }) : copied ? "Copied" : "Copy"
    }
  );
}

// ../packages/console-components/src/conversation/conversation-rich-content.tsx
var import_jsx_runtime5 = require("react/jsx-runtime");
function markdownHtml(text) {
  return { __html: renderConversationInlineMarkdown(text) };
}
function commandCopyText(block) {
  return [block.title, block.body, block.output || "", block.footer || ""].filter(Boolean).join("\n").trim();
}
function fileChangeCopyText(block) {
  return [
    block.verb,
    block.before || "",
    block.name,
    block.after || "",
    `+${block.plus}`,
    `-${block.minus}`
  ].filter(Boolean).join(" ").replace(/\s+/g, " ").trim();
}
function alignmentAttr(alignment) {
  return alignment || "left";
}
function renderThinkingBlock(block) {
  if (!block.label?.trim() && !block.text?.trim()) {
    return null;
  }
  return /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)(
    "div",
    {
      className: clsx_default(
        "cc-rich-thinking",
        block.final && "cc-rich-thinking--final",
        block.persisted && "cc-rich-thinking--persisted"
      ),
      children: [
        /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("div", { className: "cc-rich-thinking__label", children: block.label }),
        /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("p", { className: "cc-rich-paragraph", dangerouslySetInnerHTML: markdownHtml(block.text) })
      ]
    }
  );
}
function renderBlock(block, index, Icon2) {
  if (block.type === "paragraph") {
    return /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("p", { className: "cc-rich-paragraph", dangerouslySetInnerHTML: markdownHtml(block.text) }, `paragraph-${index}`);
  }
  if (block.type === "heading") {
    return /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(
      "h3",
      {
        className: `cc-rich-heading cc-rich-heading--${Number(block.level) || 2}`,
        dangerouslySetInnerHTML: markdownHtml(block.text)
      },
      `heading-${index}`
    );
  }
  if (block.type === "code") {
    const codeBlock = block;
    return /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("section", { className: "cc-rich-code-card", children: [
      /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("div", { className: "cc-rich-code-card__header", children: [
        /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-rich-code-language", children: codeBlock.language || "text" }),
        /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(
          CopyButton,
          {
            copiedLabel: "Copied code",
            Icon: Icon2,
            label: "Copy code",
            text: codeBlock.body
          }
        )
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("pre", { className: "cc-rich-code-body", children: codeBlock.highlightedHtml ? /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(
        "code",
        {
          className: `cc-rich-code-content language-${codeBlock.language || "text"}`,
          dangerouslySetInnerHTML: { __html: codeBlock.highlightedHtml }
        }
      ) : /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("code", { className: `cc-rich-code-content language-${codeBlock.language || "text"}`, children: codeBlock.body }) })
    ] }, `code-${index}`);
  }
  if (block.type === "table") {
    return /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("div", { className: "cc-rich-table-wrap", children: /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("table", { className: "cc-rich-table", children: [
      /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("thead", { children: /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("tr", { children: block.headers.map((header, cellIndex) => /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(
        "th",
        {
          "data-align": alignmentAttr(block.alignments[cellIndex]),
          dangerouslySetInnerHTML: markdownHtml(header)
        },
        `header-${cellIndex}`
      )) }) }),
      /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("tbody", { children: block.rows.map((row, rowIndex) => /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("tr", { children: block.headers.map((_header, cellIndex) => /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(
        "td",
        {
          "data-align": alignmentAttr(block.alignments[cellIndex]),
          dangerouslySetInnerHTML: markdownHtml(row[cellIndex] || "")
        },
        `cell-${rowIndex}-${cellIndex}`
      )) }, `row-${rowIndex}`)) })
    ] }) }, `table-${index}`);
  }
  if (block.type === "command") {
    return /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("div", { className: "cc-rich-command-stack", children: [
      /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("div", { className: "cc-rich-command-caption", children: block.caption }),
      /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("div", { className: "cc-rich-command-card", children: [
        /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("div", { className: "cc-rich-command-card__header", children: [
          /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("div", { className: "cc-rich-command-card__title", children: block.title }),
          /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(
            CopyButton,
            {
              copiedLabel: "Copied command output",
              Icon: Icon2,
              label: "Copy command output",
              text: commandCopyText(block)
            }
          )
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("pre", { className: "cc-rich-command-card__body", children: block.body }),
        block.output ? /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("pre", { className: "cc-rich-command-card__output", children: block.output }) : null,
        block.footer ? /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("div", { className: "cc-rich-command-card__footer", children: block.footer }) : null
      ] })
    ] }, `command-${index}`);
  }
  if (block.type === "file-change") {
    return /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("section", { className: "cc-rich-file-change", children: [
      /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("div", { className: "cc-rich-file-change__main", children: [
        /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-rich-file-change__verb", children: block.verb }),
        block.before ? /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-rich-file-change__context", dangerouslySetInnerHTML: markdownHtml(block.before) }) : null,
        /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("button", { className: "cc-rich-file-change__link", type: "button", children: block.name }),
        block.after ? /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-rich-file-change__context", dangerouslySetInnerHTML: markdownHtml(block.after) }) : null
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("div", { className: "cc-rich-file-change__stats", children: [
        /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(ChangeStatPair, { minus: block.minus, plus: block.plus }),
        /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-rich-file-change__dot" }),
        /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(
          CopyButton,
          {
            copiedLabel: "Copied file change",
            Icon: Icon2,
            label: "Copy file change",
            text: fileChangeCopyText(block)
          }
        )
      ] })
    ] }, `file-change-${index}`);
  }
  if (block.type === "divider") {
    return /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("div", { className: "cc-rich-divider", children: [
      /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-rich-divider__line" }),
      /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-rich-divider__label", children: block.text }),
      /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-rich-divider__line" })
    ] }, `divider-${index}`);
  }
  const thinking = renderThinkingBlock(block);
  if (!thinking) {
    return null;
  }
  return /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("div", { children: thinking }, `thinking-${index}`);
}
function ConversationRichContent({
  blocks,
  richStyle = "default",
  Icon: Icon2
}) {
  const body = blocks.map((block, index) => renderBlock(block, index, Icon2)).filter(Boolean);
  if (body.length === 0) {
    return null;
  }
  if (richStyle === "streaming") {
    return /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("div", { className: "cc-rich-streaming", children: body });
  }
  return /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(import_jsx_runtime5.Fragment, { children: body });
}

// ../packages/console-components/src/conversation/summary-card.tsx
var import_jsx_runtime6 = require("react/jsx-runtime");
function SummaryCard({ entry, onAction }) {
  return /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)("section", { className: "cc-summary-card", children: [
    /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)("div", { className: "cc-summary-card__header", children: [
      /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)("span", { className: "cc-summary-card__title", children: [
        entry.title,
        " ",
        /* @__PURE__ */ (0, import_jsx_runtime6.jsx)(ChangeStatPair, { minus: entry.minus, plus: entry.plus })
      ] }),
      entry.actionLabel && onAction ? /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("button", { className: "cc-summary-card__action", type: "button", onClick: () => onAction(entry), children: entry.actionLabel }) : null
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("div", { className: "cc-summary-card__files", children: entry.files.map((file) => /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)("div", { className: "cc-summary-card__file", children: [
      /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("span", { className: "cc-summary-card__file-name", children: file.name }),
      /* @__PURE__ */ (0, import_jsx_runtime6.jsx)(ChangeStatPair, { className: "cc-summary-card__file-stats", minus: file.minus, plus: file.plus })
    ] }, file.name)) })
  ] });
}

// ../packages/console-components/src/conversation/conversation-message-view.tsx
var import_jsx_runtime7 = require("react/jsx-runtime");
function renderMultilineText(text) {
  return text.split("\n").map((line, index) => /* @__PURE__ */ (0, import_jsx_runtime7.jsxs)(import_react2.Fragment, { children: [
    index > 0 ? /* @__PURE__ */ (0, import_jsx_runtime7.jsx)("br", {}) : null,
    line
  ] }, `${line}-${index}`));
}
function ConversationMessageView({
  entry,
  compact = false,
  Icon: Icon2
}) {
  const presentation = conversationIdentityPresentation(entry.identity);
  const assistantClassName = [
    "cc-message",
    "cc-message--assistant",
    presentation === "participant" ? "cc-message--participant" : "",
    presentation === "system" ? "cc-message--system" : ""
  ].filter(Boolean).join(" ");
  if (entry.kind === "summary") {
    return /* @__PURE__ */ (0, import_jsx_runtime7.jsx)(SummaryCard, { entry });
  }
  if (entry.variant === "meta") {
    return /* @__PURE__ */ (0, import_jsx_runtime7.jsx)("article", { className: `${assistantClassName} cc-message--meta`, children: /* @__PURE__ */ (0, import_jsx_runtime7.jsx)("p", { children: entry.text }) });
  }
  if (presentation === "user") {
    const copyText = entry.copyText || entry.text || "";
    return /* @__PURE__ */ (0, import_jsx_runtime7.jsxs)("article", { className: "cc-message cc-message--user", children: [
      !compact ? /* @__PURE__ */ (0, import_jsx_runtime7.jsx)(
        CopyButton,
        {
          className: "cc-message__copy",
          copiedLabel: "Copied message",
          Icon: Icon2,
          label: "Copy message",
          text: copyText
        }
      ) : null,
      /* @__PURE__ */ (0, import_jsx_runtime7.jsx)("p", { children: renderMultilineText(entry.text || "") })
    ] });
  }
  const visibleRichBlocks = entry.variant === "rich" && entry.blocks?.length ? entry.blocks.filter((block) => conversationRichBlockCopyText(block).trim().length > 0) : [];
  if (entry.variant === "rich" && visibleRichBlocks.length) {
    return /* @__PURE__ */ (0, import_jsx_runtime7.jsx)("article", { className: `${assistantClassName} cc-message--rich`, children: /* @__PURE__ */ (0, import_jsx_runtime7.jsx)(ConversationRichContent, { blocks: visibleRichBlocks, Icon: Icon2, richStyle: entry.richStyle }) });
  }
  if (entry.variant === "rich") {
    return null;
  }
  return /* @__PURE__ */ (0, import_jsx_runtime7.jsx)("article", { className: assistantClassName, children: /* @__PURE__ */ (0, import_jsx_runtime7.jsx)("p", { children: entry.text || "" }) });
}

// ../packages/console-components/src/conversation/conversation-message-group.tsx
var import_jsx_runtime8 = require("react/jsx-runtime");
function initialsForIdentity(group) {
  const explicit = group.identity.avatarLabel?.trim();
  if (explicit) {
    return explicit.slice(0, 3).toUpperCase();
  }
  const tokens = group.identity.label.split(/\s+/u).map((token) => token.trim()).filter(Boolean);
  if (!tokens.length) {
    return "?";
  }
  return tokens.slice(0, 2).map((token) => token[0] || "").join("").toUpperCase();
}
function groupHasNestedCopyButton(group) {
  return group.entries.some((entry) => conversationMessageHasIntrinsicCopyAction(entry));
}
function ConversationMessageGroup({
  group,
  compact = false,
  Icon: Icon2
}) {
  const presentation = conversationIdentityPresentation(group.identity);
  const isUserGroup = presentation === "user";
  if (isUserGroup) {
    return /* @__PURE__ */ (0, import_jsx_runtime8.jsx)(import_jsx_runtime8.Fragment, { children: group.entries.map((entry) => /* @__PURE__ */ (0, import_jsx_runtime8.jsx)(ConversationMessageView, { compact, entry, Icon: Icon2 }, entry.id)) });
  }
  const copyText = group.copyText || group.entries.map((entry) => conversationEntryText(entry)).filter(Boolean).join("\n\n");
  const showGroupCopy = !compact && !groupHasNestedCopyButton(group);
  const showIdentity = conversationIdentityShowsLabel(group.identity);
  return /* @__PURE__ */ (0, import_jsx_runtime8.jsxs)(
    "section",
    {
      className: clsx_default(
        "cc-message-group",
        compact && "is-compact",
        `is-${presentation}`,
        showIdentity && "has-identity"
      ),
      style: toneStyle(group.identity.tone),
      children: [
        showIdentity ? /* @__PURE__ */ (0, import_jsx_runtime8.jsxs)("div", { className: "cc-message-group__identity", children: [
          /* @__PURE__ */ (0, import_jsx_runtime8.jsx)("span", { className: "cc-message-group__identity-mark", "aria-hidden": "true", children: initialsForIdentity(group) }),
          /* @__PURE__ */ (0, import_jsx_runtime8.jsxs)("span", { className: "cc-message-group__identity-copy", children: [
            /* @__PURE__ */ (0, import_jsx_runtime8.jsx)("span", { className: "cc-message-group__identity-label", children: group.identity.label }),
            group.identity.meta ? /* @__PURE__ */ (0, import_jsx_runtime8.jsx)("span", { className: "cc-message-group__identity-meta", children: group.identity.meta }) : null
          ] })
        ] }) : null,
        showGroupCopy ? /* @__PURE__ */ (0, import_jsx_runtime8.jsx)(
          CopyButton,
          {
            className: "cc-message-group__copy",
            copiedLabel: "Copied response",
            Icon: Icon2,
            label: "Copy response",
            text: copyText
          }
        ) : null,
        /* @__PURE__ */ (0, import_jsx_runtime8.jsx)("div", { className: "cc-message-group__body", children: group.entries.map((entry) => /* @__PURE__ */ (0, import_jsx_runtime8.jsx)(ConversationMessageView, { compact, entry, Icon: Icon2 }, entry.id)) })
      ]
    }
  );
}

// ../packages/console-components/src/conversation/turn-diff-card.tsx
var import_jsx_runtime9 = require("react/jsx-runtime");
function TurnDiffLineView({ line }) {
  const marker = line.type === "add" ? "+" : line.type === "remove" ? "-" : " ";
  return /* @__PURE__ */ (0, import_jsx_runtime9.jsxs)("div", { className: `cc-turn-diff-card__line is-${line.type}`, children: [
    /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("span", { className: "cc-turn-diff-card__line-no", children: line.oldLine != null ? String(line.oldLine) : "" }),
    /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("span", { className: "cc-turn-diff-card__line-no", children: line.newLine != null ? String(line.newLine) : "" }),
    /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("span", { className: "cc-turn-diff-card__line-mark", children: marker }),
    /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("span", { className: "cc-turn-diff-card__line-text", children: line.text })
  ] }, `${line.oldLine ?? "x"}-${line.newLine ?? "y"}-${line.text}`);
}
function TurnDiffFileView({
  expanded,
  file,
  onToggle
}) {
  return /* @__PURE__ */ (0, import_jsx_runtime9.jsxs)("div", { className: `cc-turn-diff-card__file${expanded ? " is-expanded" : ""}`, children: [
    /* @__PURE__ */ (0, import_jsx_runtime9.jsxs)("button", { className: "cc-turn-diff-card__file-row", type: "button", onClick: onToggle, children: [
      /* @__PURE__ */ (0, import_jsx_runtime9.jsxs)("span", { className: "cc-turn-diff-card__file-left", children: [
        /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("span", { className: "cc-turn-diff-card__file-name", children: file.path }),
        /* @__PURE__ */ (0, import_jsx_runtime9.jsx)(ChangeStatPair, { className: "cc-turn-diff-card__file-stats", minus: file.minus, plus: file.plus })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("span", { className: "cc-turn-diff-card__file-caret", children: expanded ? "\u2303" : "\u2304" })
    ] }),
    expanded ? /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("div", { className: "cc-turn-diff-card__file-body", children: file.hunks.map((hunk, index) => /* @__PURE__ */ (0, import_jsx_runtime9.jsxs)("div", { className: "cc-turn-diff-card__hunk", children: [
      /* @__PURE__ */ (0, import_jsx_runtime9.jsxs)("div", { className: "cc-turn-diff-card__hunk-header", children: [
        "@@ -",
        hunk.oldStart,
        ",",
        hunk.oldLines,
        " +",
        hunk.newStart,
        ",",
        hunk.newLines,
        " @@"
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("div", { className: "cc-turn-diff-card__lines", children: hunk.lines.map((line) => /* @__PURE__ */ (0, import_jsx_runtime9.jsx)(TurnDiffLineView, { line }, `${line.oldLine ?? "x"}-${line.newLine ?? "y"}-${line.text}`)) })
    ] }, `${file.path}-${index}`)) }) : null
  ] });
}
function TurnDiffCard({
  turnDiff,
  expandedFile,
  onToggleFile
}) {
  return /* @__PURE__ */ (0, import_jsx_runtime9.jsxs)("section", { className: "cc-summary-card cc-turn-diff-card", children: [
    /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("div", { className: "cc-summary-card__header", children: /* @__PURE__ */ (0, import_jsx_runtime9.jsxs)("span", { className: "cc-summary-card__title", children: [
      `${formatCount(turnDiff.fileCount)} ${turnDiff.fileCount === 1 ? "file" : "files"} changed`,
      " ",
      /* @__PURE__ */ (0, import_jsx_runtime9.jsx)(ChangeStatPair, { minus: turnDiff.minus, plus: turnDiff.plus })
    ] }) }),
    /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("div", { className: "cc-turn-diff-card__files", children: turnDiff.files.map((file) => /* @__PURE__ */ (0, import_jsx_runtime9.jsx)(
      TurnDiffFileView,
      {
        expanded: expandedFile === file.path,
        file,
        onToggle: () => onToggleFile(file.path)
      },
      file.path
    )) })
  ] });
}

// ../packages/console-components/src/conversation/conversation-transcript.tsx
var import_jsx_runtime10 = require("react/jsx-runtime");
function ConversationTranscript({
  viewState,
  compact = false,
  maxGroups = null,
  showTurnDiff = true,
  expandedDiffFile = null,
  onToggleDiffFile = null,
  Icon: Icon2,
  className
}) {
  const canRenderTurnDiff = Boolean(showTurnDiff && viewState.turnDiff && onToggleDiffFile);
  const renderableTurnDiff = canRenderTurnDiff ? viewState.turnDiff : null;
  const groups = typeof maxGroups === "number" && maxGroups > 0 ? viewState.groups.slice(-maxGroups) : viewState.groups;
  if (!groups.length && !renderableTurnDiff) {
    return null;
  }
  return /* @__PURE__ */ (0, import_jsx_runtime10.jsxs)("div", { className: clsx_default("cc-theme-scope", "cc-conversation-transcript", compact && "is-compact", className), children: [
    groups.map((group) => /* @__PURE__ */ (0, import_jsx_runtime10.jsx)(ConversationMessageGroup, { compact, group, Icon: Icon2 }, group.id)),
    renderableTurnDiff && onToggleDiffFile ? /* @__PURE__ */ (0, import_jsx_runtime10.jsx)(
      TurnDiffCard,
      {
        expandedFile: expandedDiffFile,
        onToggleFile: onToggleDiffFile,
        turnDiff: renderableTurnDiff
      }
    ) : null
  ] });
}

// ../packages/console-components/src/conversation/conversation-pane.tsx
var import_jsx_runtime11 = require("react/jsx-runtime");
function ConversationPane({
  viewState,
  Icon: Icon2,
  footer = null,
  className,
  scrollClassName,
  bodyClassName,
  compact = false,
  maxGroups = null,
  showTurnDiff = true,
  expandedDiffFile = null,
  onApplySuggestion,
  onToggleDiffFile = null
}) {
  const canRenderTurnDiff = Boolean(showTurnDiff && viewState.turnDiff && onToggleDiffFile);
  const showEmptyState = Boolean(viewState.emptyState && viewState.entries.length === 0 && !canRenderTurnDiff);
  return /* @__PURE__ */ (0, import_jsx_runtime11.jsxs)("div", { className: clsx_default("cc-theme-scope", "cc-conversation-pane", className), children: [
    /* @__PURE__ */ (0, import_jsx_runtime11.jsx)("section", { className: clsx_default("cc-conversation-pane__scroll", scrollClassName), children: /* @__PURE__ */ (0, import_jsx_runtime11.jsx)("div", { className: clsx_default("cc-conversation-pane__body", bodyClassName), children: showEmptyState && viewState.emptyState ? /* @__PURE__ */ (0, import_jsx_runtime11.jsx)(ConversationEmptyState, { Icon: Icon2, onApplySuggestion, state: viewState.emptyState }) : /* @__PURE__ */ (0, import_jsx_runtime11.jsx)(
      ConversationTranscript,
      {
        Icon: Icon2,
        compact,
        expandedDiffFile,
        maxGroups,
        onToggleDiffFile,
        showTurnDiff,
        viewState
      }
    ) }) }),
    footer ? /* @__PURE__ */ (0, import_jsx_runtime11.jsx)("div", { className: "cc-conversation-pane__footer", children: footer }) : null
  ] });
}

// ../packages/console-components/src/dock/console-dock.tsx
var import_react3 = require("react");

// ../packages/console-components/src/dock/resize-lock.ts
var RESIZE_LOCK_DATA_KEY = "ccResizeLockCount";
var RESIZE_STATE_DATA_KEY = "ccResizing";
function resizeLockRoot() {
  if (typeof document === "undefined") {
    return null;
  }
  return document.documentElement;
}
function readResizeLockCount() {
  const root = resizeLockRoot();
  if (!root) {
    return 0;
  }
  const raw = root.dataset[RESIZE_LOCK_DATA_KEY];
  const count = Number(raw);
  return Number.isFinite(count) && count > 0 ? count : 0;
}
function acquireResizeLock() {
  const root = resizeLockRoot();
  if (!root) {
    return;
  }
  const nextCount = readResizeLockCount() + 1;
  root.dataset[RESIZE_LOCK_DATA_KEY] = String(nextCount);
  root.dataset[RESIZE_STATE_DATA_KEY] = "true";
}
function releaseResizeLock() {
  const root = resizeLockRoot();
  if (!root) {
    return;
  }
  const nextCount = Math.max(0, readResizeLockCount() - 1);
  if (nextCount === 0) {
    delete root.dataset[RESIZE_LOCK_DATA_KEY];
    delete root.dataset[RESIZE_STATE_DATA_KEY];
    return;
  }
  root.dataset[RESIZE_LOCK_DATA_KEY] = String(nextCount);
}

// ../packages/console-components/src/dock/console-dock.tsx
var import_jsx_runtime12 = require("react/jsx-runtime");
function splitGlyph(direction) {
  switch (direction) {
    case "left":
      return "\u2190";
    case "right":
      return "\u2192";
    case "up":
      return "\u2191";
    case "down":
      return "\u2193";
    default:
      return "+";
  }
}
function PanelActionButton({
  direction,
  panelId,
  onClick
}) {
  return /* @__PURE__ */ (0, import_jsx_runtime12.jsx)(
    "button",
    {
      "aria-label": `Split ${direction}`,
      className: "cc-dock-panel__icon-action",
      "data-testid": `dock-split:${panelId}:${direction}`,
      type: "button",
      onClick,
      children: /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("span", { "aria-hidden": "true", children: splitGlyph(direction) })
    }
  );
}
function PanelNodeView({
  node,
  panelsById,
  focusedPanelId,
  isSinglePanelLayout,
  Icon: Icon2,
  onClosePanel,
  onFocusPanel,
  onResizeSplit,
  onSplitPanel,
  renderPanelBody,
  renderPanelFooter
}) {
  if (node.kind === "panel") {
    const panel = panelsById.get(node.panelId);
    if (!panel) {
      return null;
    }
    const panelActions = /* @__PURE__ */ (0, import_jsx_runtime12.jsxs)(import_jsx_runtime12.Fragment, { children: [
      ["left", "right", "up", "down"].map((direction) => /* @__PURE__ */ (0, import_jsx_runtime12.jsx)(
        PanelActionButton,
        {
          direction,
          panelId: panel.id,
          onClick: () => onSplitPanel?.(panel, direction)
        },
        direction
      )),
      panel.closable !== false ? /* @__PURE__ */ (0, import_jsx_runtime12.jsx)(
        "button",
        {
          "aria-label": "Close panel",
          className: "cc-dock-panel__icon-action is-close",
          "data-testid": `dock-close:${panel.id}`,
          type: "button",
          onClick: () => onClosePanel?.(panel),
          children: /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("span", { "aria-hidden": "true", children: "\xD7" })
        }
      ) : null
    ] });
    return /* @__PURE__ */ (0, import_jsx_runtime12.jsxs)(
      "section",
      {
        className: clsx_default(
          "cc-dock-panel",
          isSinglePanelLayout && "is-solitary",
          focusedPanelId === panel.id && "is-focused",
          panel.mode === "terminal" && "is-terminal"
        ),
        "data-testid": `dock-panel:${panel.id}`,
        "data-panel-id": panel.id,
        onMouseDown: () => onFocusPanel?.(panel),
        children: [
          isSinglePanelLayout ? /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("div", { className: "cc-dock-panel__floating-actions", children: /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("div", { className: "cc-dock-panel__actions", children: panelActions }) }) : /* @__PURE__ */ (0, import_jsx_runtime12.jsxs)("header", { className: "cc-dock-panel__header", children: [
            /* @__PURE__ */ (0, import_jsx_runtime12.jsxs)("div", { className: "cc-dock-panel__copy", children: [
              /* @__PURE__ */ (0, import_jsx_runtime12.jsxs)("div", { className: "cc-dock-panel__title-row", children: [
                panel.iconName && Icon2 ? /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("span", { className: "cc-dock-panel__icon", "aria-hidden": "true", children: /* @__PURE__ */ (0, import_jsx_runtime12.jsx)(Icon2, { name: panel.iconName }) }) : null,
                /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("span", { className: "cc-dock-panel__title", children: panel.title }),
                panel.badgeLabel ? /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("span", { className: "cc-dock-panel__badge", children: panel.badgeLabel }) : null
              ] }),
              panel.subtitle || panel.statusLabel ? /* @__PURE__ */ (0, import_jsx_runtime12.jsxs)("div", { className: "cc-dock-panel__meta", children: [
                panel.subtitle ? /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("span", { children: panel.subtitle }) : null,
                panel.statusLabel ? /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("span", { children: panel.statusLabel }) : null
              ] }) : null
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("div", { className: "cc-dock-panel__actions", children: panelActions })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("div", { className: "cc-dock-panel__body", children: renderPanelBody(panel) }),
          renderPanelFooter ? /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("div", { className: "cc-dock-panel__footer", children: renderPanelFooter(panel) }) : null
        ]
      }
    );
  }
  const splitNode = node;
  const splitRef = (0, import_react3.useRef)(null);
  const resizeCleanupRef = (0, import_react3.useRef)(null);
  const firstFlex = typeof splitNode.ratio === "number" && splitNode.ratio > 0 && splitNode.ratio < 1 ? splitNode.ratio : 0.5;
  const secondFlex = 1 - firstFlex;
  (0, import_react3.useEffect)(() => () => {
    resizeCleanupRef.current?.();
    resizeCleanupRef.current = null;
  }, []);
  function handleResizeStart(event) {
    if (!onResizeSplit || !splitRef.current) {
      return;
    }
    resizeCleanupRef.current?.();
    resizeCleanupRef.current = null;
    event.preventDefault();
    event.stopPropagation();
    acquireResizeLock();
    const divider = event.currentTarget;
    const pointerId = event.pointerId;
    let isActive = true;
    const updateRatio = (pointerEvent) => {
      if (pointerEvent.pointerId !== pointerId) {
        return;
      }
      const splitElement = splitRef.current;
      if (!splitElement || !isActive) {
        return;
      }
      const rect = splitElement.getBoundingClientRect();
      const size = splitNode.direction === "horizontal" ? rect.width : rect.height;
      if (size <= 0) {
        return;
      }
      const offset = splitNode.direction === "horizontal" ? pointerEvent.clientX - rect.left : pointerEvent.clientY - rect.top;
      const ratio = offset / size;
      onResizeSplit(splitNode.id, Math.min(0.88, Math.max(0.12, ratio)));
    };
    const handlePointerMove = (pointerEvent) => {
      updateRatio(pointerEvent);
    };
    const cleanup = () => {
      if (!isActive) {
        return;
      }
      isActive = false;
      releaseResizeLock();
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", handlePointerUp);
      window.removeEventListener("pointercancel", handlePointerCancel);
      divider.removeEventListener("lostpointercapture", handleLostPointerCapture);
      if ("hasPointerCapture" in divider && divider.hasPointerCapture(event.pointerId)) {
        divider.releasePointerCapture(event.pointerId);
      }
      resizeCleanupRef.current = null;
    };
    const handlePointerUp = (pointerEvent) => {
      if (pointerEvent.pointerId !== pointerId) {
        return;
      }
      cleanup();
    };
    const handlePointerCancel = (pointerEvent) => {
      if (pointerEvent.pointerId !== pointerId) {
        return;
      }
      cleanup();
    };
    const handleLostPointerCapture = () => {
      cleanup();
    };
    updateRatio(event.nativeEvent);
    if ("setPointerCapture" in divider) {
      divider.setPointerCapture(event.pointerId);
    }
    resizeCleanupRef.current = cleanup;
    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", handlePointerUp);
    window.addEventListener("pointercancel", handlePointerCancel);
    divider.addEventListener("lostpointercapture", handleLostPointerCapture);
  }
  return /* @__PURE__ */ (0, import_jsx_runtime12.jsxs)("div", { className: clsx_default("cc-dock-split", `is-${splitNode.direction}`), ref: splitRef, children: [
    /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("div", { className: "cc-dock-split__slot", style: { flex: `${firstFlex} 1 0%` }, children: /* @__PURE__ */ (0, import_jsx_runtime12.jsx)(
      PanelNodeView,
      {
        focusedPanelId,
        Icon: Icon2,
        isSinglePanelLayout,
        node: node.first,
        onClosePanel,
        onFocusPanel,
        onResizeSplit,
        onSplitPanel,
        panelsById,
        renderPanelBody,
        renderPanelFooter
      }
    ) }),
    /* @__PURE__ */ (0, import_jsx_runtime12.jsx)(
      "button",
      {
        "aria-label": `Resize ${splitNode.direction} split`,
        className: "cc-dock-split__divider",
        "data-testid": `dock-divider:${splitNode.id}`,
        type: "button",
        onPointerDown: handleResizeStart,
        children: /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("span", { className: "cc-dock-split__divider-line", "aria-hidden": "true" })
      }
    ),
    /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("div", { className: "cc-dock-split__slot", style: { flex: `${secondFlex} 1 0%` }, children: /* @__PURE__ */ (0, import_jsx_runtime12.jsx)(
      PanelNodeView,
      {
        focusedPanelId,
        Icon: Icon2,
        isSinglePanelLayout,
        node: node.second,
        onClosePanel,
        onFocusPanel,
        onResizeSplit,
        onSplitPanel,
        panelsById,
        renderPanelBody,
        renderPanelFooter
      }
    ) })
  ] });
}
function ConsoleDock({
  viewState,
  Icon: Icon2,
  className,
  tabActions = null,
  renderEmptyState,
  renderPanelBody,
  renderPanelFooter,
  onCreateTab,
  onClosePanel,
  onCloseTab,
  onFocusPanel,
  onResizeSplit,
  onSelectTab,
  onSplitPanel
}) {
  const normalized = normalizeConsoleDockViewState(viewState);
  const activeTab = normalized.tabs.find((tab) => tab.id === normalized.activeTabId) || null;
  const activePanelCount = activeTab ? collectConsoleDockPanelIds(activeTab.layout).length : 0;
  const hasMultipleTabs = normalized.tabs.length > 1;
  const hasTabToolbar = Boolean(tabActions) || Boolean(onCreateTab);
  const panelsById = new Map(
    normalized.panels.map((panel) => [panel.id, panel])
  );
  return /* @__PURE__ */ (0, import_jsx_runtime12.jsxs)(
    "section",
    {
      className: clsx_default(
        "cc-theme-scope",
        "cc-dock",
        className,
        !hasMultipleTabs && "is-single-tab",
        activePanelCount <= 1 && "is-single-panel"
      ),
      children: [
        hasMultipleTabs || hasTabToolbar ? /* @__PURE__ */ (0, import_jsx_runtime12.jsxs)("header", { className: clsx_default("cc-dock__tab-strip", !hasMultipleTabs && normalized.tabs.length === 0 && "is-toolbar-only"), children: [
          normalized.tabs.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("div", { className: "cc-dock__tabs", role: "tablist", "aria-label": "Dock tabs", children: normalized.tabs.map((tab) => /* @__PURE__ */ (0, import_jsx_runtime12.jsxs)(
            "div",
            {
              className: clsx_default("cc-dock-tab", tab.id === normalized.activeTabId && "is-active"),
              children: [
                /* @__PURE__ */ (0, import_jsx_runtime12.jsxs)(
                  "button",
                  {
                    "aria-selected": tab.id === normalized.activeTabId,
                    className: "cc-dock-tab__button",
                    role: "tab",
                    title: tab.subtitle ? `${tab.title} - ${tab.subtitle}` : tab.title,
                    type: "button",
                    onClick: () => onSelectTab?.(tab),
                    children: [
                      Icon2 && tab.iconName ? /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("span", { className: "cc-dock-tab__icon", "aria-hidden": "true", children: /* @__PURE__ */ (0, import_jsx_runtime12.jsx)(Icon2, { name: tab.iconName }) }) : null,
                      /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("span", { className: "cc-dock-tab__copy", children: /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("span", { className: "cc-dock-tab__title", children: tab.title }) }),
                      tab.badgeLabel ? /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("span", { className: "cc-dock-tab__badge", children: tab.badgeLabel }) : null
                    ]
                  }
                ),
                tab.closable !== false ? /* @__PURE__ */ (0, import_jsx_runtime12.jsx)(
                  "button",
                  {
                    "aria-label": `Close ${tab.title}`,
                    className: "cc-dock-tab__close",
                    type: "button",
                    onClick: () => onCloseTab?.(tab),
                    children: /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("span", { "aria-hidden": "true", children: "\xD7" })
                  }
                ) : null
              ]
            },
            tab.id
          )) }) : null,
          /* @__PURE__ */ (0, import_jsx_runtime12.jsxs)("div", { className: "cc-dock__tab-actions", children: [
            tabActions,
            onCreateTab ? /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("button", { "aria-label": "New tab", className: "cc-dock__new-tab", type: "button", onClick: onCreateTab, children: /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("span", { "aria-hidden": "true", children: "+" }) }) : null
          ] })
        ] }) : null,
        /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("div", { className: "cc-dock__body", children: activeTab ? /* @__PURE__ */ (0, import_jsx_runtime12.jsx)(
          PanelNodeView,
          {
            focusedPanelId: normalized.focusedPanelId,
            Icon: Icon2,
            isSinglePanelLayout: activePanelCount <= 1,
            node: activeTab.layout,
            onClosePanel,
            onFocusPanel,
            onResizeSplit,
            onSplitPanel,
            panelsById,
            renderPanelBody,
            renderPanelFooter
          }
        ) : renderEmptyState ? renderEmptyState() : /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("div", { className: "cc-dock__empty", children: "Open a tab to start arranging panels." }) })
      ]
    }
  );
}

// ../packages/console-components/src/dock/use-console-dock-controller.ts
var import_react4 = require("react");
function useConsoleDockController({
  initialTarget = null,
  initialPresetId = "single",
  createPanelState,
  suggestTargets,
  resolvePanelView,
  resolveTabView
}) {
  const panelCounterRef = (0, import_react4.useRef)(1);
  const splitCounterRef = (0, import_react4.useRef)(1);
  const tabCounterRef = (0, import_react4.useRef)(1);
  function nextPanelId() {
    return `panel-${panelCounterRef.current++}`;
  }
  function nextSplitId() {
    return `split-${splitCounterRef.current++}`;
  }
  function nextTabId() {
    return `tab-${tabCounterRef.current++}`;
  }
  const [state, setState] = (0, import_react4.useState)(() => createConsoleDockState({
    initialTarget,
    initialPresetId,
    createPanelState: (args) => {
      const nextState = createPanelState(args);
      return {
        ...nextState,
        id: nextState.id || nextPanelId()
      };
    },
    createSplitId: nextSplitId,
    createTabId: nextTabId,
    suggestTargets
  }));
  const viewState = (0, import_react4.useMemo)(() => buildConsoleDockViewState(state, {
    resolvePanelView,
    resolveTabView
  }), [resolvePanelView, resolveTabView, state]);
  const focusedPanel = (0, import_react4.useMemo)(
    () => state.panels.find((panel) => panel.id === state.focusedPanelId) || null,
    [state.focusedPanelId, state.panels]
  );
  function dispatch(action) {
    setState((current) => applyConsoleDockAction(current, action, {
      createPanelState: (args) => {
        const nextState = createPanelState(args);
        return {
          ...nextState,
          id: nextState.id || nextPanelId()
        };
      },
      createSplitId: nextSplitId,
      createTabId: nextTabId,
      suggestTargets
    }));
  }
  return {
    state,
    setState,
    viewState,
    presets: consoleDockPresets(),
    focusedPanel,
    focusedPanelId: state.focusedPanelId,
    focusedTarget: focusedPanel?.target || null,
    dispatch,
    createTab: () => dispatch({ type: "create_tab" }),
    selectTab: (tabId) => dispatch({ type: "select_tab", tabId }),
    closeTab: (tabId) => dispatch({ type: "close_tab", tabId }),
    focusPanel: (panelId) => dispatch({ type: "focus_panel", panelId }),
    closePanel: (panelId) => dispatch({ type: "close_panel", panelId }),
    splitPanel: (panelId, direction) => dispatch({ type: "split_panel", panelId, direction }),
    resizeSplit: (splitId, ratio) => dispatch({ type: "resize_split", splitId, ratio }),
    applyPreset: (presetId) => dispatch({ type: "apply_preset", presetId }),
    openTarget: (target, intent) => dispatch({ type: "open_target", target, intent }),
    setPanelTarget: (panelId, target) => dispatch({ type: "set_panel_target", panelId, target }),
    setPanelMode: (panelId, mode) => dispatch({ type: "set_panel_mode", panelId, mode })
  };
}

// ../packages/console-components/src/sidebar/console-sidebar.tsx
var import_react5 = require("react");
var import_jsx_runtime13 = require("react/jsx-runtime");
function SectionIconButton({
  action,
  Icon: Icon2,
  buttonProps,
  className,
  onClick
}) {
  return /* @__PURE__ */ (0, import_jsx_runtime13.jsx)(
    "button",
    {
      ...buttonProps,
      "aria-label": action.label,
      className: clsx_default("cc-sidebar-icon-action", action.active && "is-active", className, buttonProps?.className),
      disabled: action.disabled || buttonProps?.disabled,
      title: action.label,
      type: "button",
      onClick: (event) => {
        event.stopPropagation();
        buttonProps?.onClick?.(event);
        if (!event.defaultPrevented) {
          onClick?.();
        }
      },
      children: Icon2 && action.iconName ? /* @__PURE__ */ (0, import_jsx_runtime13.jsx)(Icon2, { name: action.iconName }) : null
    }
  );
}
function ActionStrip({
  block,
  Icon: Icon2,
  getActionButtonProps,
  onBlockAction
}) {
  if (!block.actions?.length) {
    return null;
  }
  return /* @__PURE__ */ (0, import_jsx_runtime13.jsx)("section", { className: "cc-sidebar-block cc-sidebar-block--action-strip", children: /* @__PURE__ */ (0, import_jsx_runtime13.jsx)("div", { className: "cc-sidebar-action-strip", children: block.actions.map((action, actionIndex) => {
    const buttonProps = getActionButtonProps?.({ kind: "block", block, action });
    return /* @__PURE__ */ (0, import_jsx_runtime13.jsx)(import_react5.Fragment, { children: /* @__PURE__ */ (0, import_jsx_runtime13.jsxs)(
      "button",
      {
        ...buttonProps,
        className: clsx_default("cc-sidebar-action-strip__button", action.active && "is-active", buttonProps?.className),
        disabled: action.disabled || buttonProps?.disabled,
        type: "button",
        onClick: (event) => {
          buttonProps?.onClick?.(event);
          if (!event.defaultPrevented) {
            onBlockAction?.(block, action);
          }
        },
        children: [
          Icon2 && action.iconName ? /* @__PURE__ */ (0, import_jsx_runtime13.jsx)("span", { className: "cc-sidebar-action-strip__icon", "aria-hidden": "true", children: /* @__PURE__ */ (0, import_jsx_runtime13.jsx)(Icon2, { name: action.iconName }) }) : null,
          /* @__PURE__ */ (0, import_jsx_runtime13.jsx)("span", { children: action.label })
        ]
      }
    ) }, `${block.id}:${action.id}:${actionIndex}`);
  }) }) });
}
function BlockHeader({
  block,
  Icon: Icon2,
  getActionButtonProps,
  onBlockAction
}) {
  if (!block.title && !block.meta?.length && !block.actions?.length) {
    return null;
  }
  return /* @__PURE__ */ (0, import_jsx_runtime13.jsxs)("div", { className: "cc-sidebar-block__header", children: [
    /* @__PURE__ */ (0, import_jsx_runtime13.jsxs)("div", { className: "cc-sidebar-block__copy", children: [
      block.title ? /* @__PURE__ */ (0, import_jsx_runtime13.jsx)("h2", { className: "cc-sidebar-block__title", children: block.title }) : null,
      block.meta?.length ? /* @__PURE__ */ (0, import_jsx_runtime13.jsx)("div", { className: "cc-sidebar-block__meta", children: block.meta.map((meta) => /* @__PURE__ */ (0, import_jsx_runtime13.jsxs)("span", { className: clsx_default("cc-sidebar-meta", meta.tone && `is-${meta.tone}`), children: [
        Icon2 && meta.iconName ? /* @__PURE__ */ (0, import_jsx_runtime13.jsx)(Icon2, { className: "cc-sidebar-meta__icon", name: meta.iconName }) : null,
        /* @__PURE__ */ (0, import_jsx_runtime13.jsx)("span", { children: meta.label })
      ] }, meta.id || meta.label)) }) : null
    ] }),
    block.actions?.length ? /* @__PURE__ */ (0, import_jsx_runtime13.jsx)("div", { className: "cc-sidebar-block__actions", children: block.actions.map((action, actionIndex) => /* @__PURE__ */ (0, import_jsx_runtime13.jsx)(
      SectionIconButton,
      {
        action,
        buttonProps: getActionButtonProps?.({ kind: "block", block, action }),
        Icon: Icon2,
        onClick: () => onBlockAction?.(block, action)
      },
      `${block.id}:${action.id}:${actionIndex}`
    )) }) : null
  ] });
}
function hasVisibleSectionHeader(section) {
  return Boolean(
    section.title || section.subtitle || section.iconName || section.meta?.length || section.actions?.length
  );
}
function DefaultSectionHeader({
  block,
  section,
  Icon: Icon2,
  getActionButtonProps,
  onSelectSection,
  onSectionAction
}) {
  if (!hasVisibleSectionHeader(section)) {
    return null;
  }
  return /* @__PURE__ */ (0, import_jsx_runtime13.jsxs)("div", { className: clsx_default("cc-sidebar-section__header", section.selected && "is-selected"), children: [
    /* @__PURE__ */ (0, import_jsx_runtime13.jsxs)(
      "button",
      {
        className: "cc-sidebar-section__header-main",
        type: "button",
        onClick: () => onSelectSection?.(block, section),
        children: [
          section.iconName && Icon2 ? /* @__PURE__ */ (0, import_jsx_runtime13.jsx)("span", { className: "cc-sidebar-section__header-icon", "aria-hidden": "true", children: /* @__PURE__ */ (0, import_jsx_runtime13.jsx)(Icon2, { name: section.iconName }) }) : null,
          /* @__PURE__ */ (0, import_jsx_runtime13.jsxs)("span", { className: "cc-sidebar-section__header-copy", children: [
            /* @__PURE__ */ (0, import_jsx_runtime13.jsxs)("span", { className: "cc-sidebar-section__header-title-row", children: [
              /* @__PURE__ */ (0, import_jsx_runtime13.jsx)("span", { className: "cc-sidebar-section__header-title", children: section.title }),
              section.meta?.length ? /* @__PURE__ */ (0, import_jsx_runtime13.jsx)("span", { className: "cc-sidebar-section__header-meta", children: section.meta.map((meta) => /* @__PURE__ */ (0, import_jsx_runtime13.jsxs)("span", { className: clsx_default("cc-sidebar-meta", meta.tone && `is-${meta.tone}`), children: [
                Icon2 && meta.iconName ? /* @__PURE__ */ (0, import_jsx_runtime13.jsx)(Icon2, { className: "cc-sidebar-meta__icon", name: meta.iconName }) : null,
                /* @__PURE__ */ (0, import_jsx_runtime13.jsx)("span", { children: meta.label })
              ] }, meta.id || meta.label)) }) : null
            ] }),
            section.subtitle ? /* @__PURE__ */ (0, import_jsx_runtime13.jsx)("span", { className: "cc-sidebar-section__header-subtitle", children: section.subtitle }) : null
          ] })
        ]
      }
    ),
    section.actions?.length ? /* @__PURE__ */ (0, import_jsx_runtime13.jsx)("div", { className: "cc-sidebar-section__header-actions", children: section.actions.map((action, actionIndex) => /* @__PURE__ */ (0, import_jsx_runtime13.jsx)(
      SectionIconButton,
      {
        action,
        className: "cc-sidebar-section__action",
        Icon: Icon2,
        buttonProps: getActionButtonProps?.({ kind: "section", block, section, action }),
        onClick: () => onSectionAction?.(block, section, action)
      },
      `${section.id}:${action.id}:${actionIndex}`
    )) }) : null
  ] });
}
function SidebarRow({
  block,
  section,
  item,
  Icon: Icon2,
  getActionButtonProps,
  trailingContent,
  onSelectItem,
  onItemAction,
  onItemContextMenu
}) {
  function handleKeyDown(event) {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      if (!item.disabled) {
        onSelectItem?.(block, section, item);
      }
    }
  }
  return /* @__PURE__ */ (0, import_jsx_runtime13.jsxs)(
    "div",
    {
      className: clsx_default(
        "cc-sidebar-row",
        item.selected && "is-selected",
        item.unread && "is-unread",
        item.disabled && "is-disabled"
      ),
      "data-console-sidebar-part": "row",
      "data-selected": item.selected ? "true" : "false",
      "data-unread": item.unread ? "true" : "false",
      "data-disabled": item.disabled ? "true" : "false",
      role: "button",
      tabIndex: item.disabled ? -1 : 0,
      onClick: () => {
        if (!item.disabled) {
          onSelectItem?.(block, section, item);
        }
      },
      onContextMenu: (event) => onItemContextMenu?.(block, section, item, event),
      onKeyDown: handleKeyDown,
      children: [
        /* @__PURE__ */ (0, import_jsx_runtime13.jsx)("span", { className: "cc-sidebar-row__main", children: /* @__PURE__ */ (0, import_jsx_runtime13.jsxs)("span", { className: "cc-sidebar-row__copy", children: [
          /* @__PURE__ */ (0, import_jsx_runtime13.jsxs)("span", { className: "cc-sidebar-row__title-row", children: [
            item.iconName && Icon2 ? /* @__PURE__ */ (0, import_jsx_runtime13.jsx)("span", { className: "cc-sidebar-row__icon", "aria-hidden": "true", children: /* @__PURE__ */ (0, import_jsx_runtime13.jsx)(Icon2, { name: item.iconName }) }) : null,
            /* @__PURE__ */ (0, import_jsx_runtime13.jsx)("span", { className: "cc-sidebar-row__title", children: item.title })
          ] }),
          item.subtitle ? /* @__PURE__ */ (0, import_jsx_runtime13.jsx)("span", { className: "cc-sidebar-row__subtitle", children: item.subtitle }) : null
        ] }) }),
        /* @__PURE__ */ (0, import_jsx_runtime13.jsxs)("span", { className: "cc-sidebar-row__meta", children: [
          item.badgeIconName && Icon2 ? /* @__PURE__ */ (0, import_jsx_runtime13.jsx)("span", { className: "cc-sidebar-row__badge", "aria-hidden": "true", children: /* @__PURE__ */ (0, import_jsx_runtime13.jsx)(Icon2, { name: item.badgeIconName }) }) : null,
          item.badgeLabel ? /* @__PURE__ */ (0, import_jsx_runtime13.jsx)("span", { className: "cc-sidebar-row__badge-label", children: item.badgeLabel }) : null,
          item.meta?.map((meta) => /* @__PURE__ */ (0, import_jsx_runtime13.jsxs)("span", { className: clsx_default("cc-sidebar-row__meta-item", meta.tone && `is-${meta.tone}`), children: [
            Icon2 && meta.iconName ? /* @__PURE__ */ (0, import_jsx_runtime13.jsx)(Icon2, { className: "cc-sidebar-row__meta-icon", name: meta.iconName }) : null,
            /* @__PURE__ */ (0, import_jsx_runtime13.jsx)("span", { children: meta.label })
          ] }, meta.id || meta.label))
        ] }),
        trailingContent || item.actions?.length && onItemAction ? /* @__PURE__ */ (0, import_jsx_runtime13.jsxs)("span", { className: "cc-sidebar-row__trailing", children: [
          trailingContent ? /* @__PURE__ */ (0, import_jsx_runtime13.jsx)("span", { className: "cc-sidebar-row__trailing-content", children: trailingContent }) : null,
          item.actions?.length && onItemAction ? /* @__PURE__ */ (0, import_jsx_runtime13.jsx)("span", { className: "cc-sidebar-row__actions", children: item.actions.map((action, actionIndex) => /* @__PURE__ */ (0, import_jsx_runtime13.jsx)(
            SectionIconButton,
            {
              action,
              className: "cc-sidebar-row__action",
              Icon: Icon2,
              buttonProps: getActionButtonProps?.({ kind: "item", block, section, item, action }),
              onClick: () => onItemAction(block, section, item, action)
            },
            `${item.id}:${action.id}:${actionIndex}`
          )) }) : null
        ] }) : null
      ]
    }
  );
}
function ListBlock({
  block,
  Icon: Icon2,
  getActionButtonProps,
  renderSectionHeader,
  renderSectionContainer,
  renderItemTrailing,
  onBlockAction,
  onSelectSection,
  onSectionAction,
  onSelectItem,
  onItemAction,
  onItemContextMenu
}) {
  return /* @__PURE__ */ (0, import_jsx_runtime13.jsxs)("section", { className: "cc-sidebar-block cc-sidebar-block--list", children: [
    /* @__PURE__ */ (0, import_jsx_runtime13.jsx)(
      BlockHeader,
      {
        Icon: Icon2,
        block,
        getActionButtonProps,
        onBlockAction
      }
    ),
    /* @__PURE__ */ (0, import_jsx_runtime13.jsx)("div", { className: "cc-sidebar-list", children: (block.sections || []).map((section) => {
      const defaultHeader = /* @__PURE__ */ (0, import_jsx_runtime13.jsx)(
        DefaultSectionHeader,
        {
          Icon: Icon2,
          block,
          getActionButtonProps,
          onSectionAction,
          onSelectSection,
          section
        }
      );
      const header = renderSectionHeader ? renderSectionHeader({ block, section, defaultHeader }) : defaultHeader;
      const defaultSection = /* @__PURE__ */ (0, import_jsx_runtime13.jsxs)("section", { className: clsx_default("cc-sidebar-section", hasVisibleSectionHeader(section) && "has-header"), children: [
        header,
        /* @__PURE__ */ (0, import_jsx_runtime13.jsx)("div", { className: "cc-sidebar-section__rows", children: section.items.map((item) => /* @__PURE__ */ (0, import_jsx_runtime13.jsx)(
          SidebarRow,
          {
            Icon: Icon2,
            block,
            getActionButtonProps,
            item,
            onItemAction,
            onItemContextMenu,
            onSelectItem,
            section,
            trailingContent: renderItemTrailing?.({ block, section, item })
          },
          item.id
        )) })
      ] }, section.id);
      return /* @__PURE__ */ (0, import_jsx_runtime13.jsx)(import_react5.Fragment, { children: renderSectionContainer ? renderSectionContainer({ block, section, defaultSection }) : defaultSection }, section.id);
    }) })
  ] });
}
function ConsoleSidebar({
  viewState,
  Icon: Icon2,
  className,
  getActionButtonProps,
  renderSectionHeader,
  renderSectionContainer,
  renderItemTrailing,
  onBlockAction,
  onSelectSection,
  onSectionAction,
  onSelectItem,
  onItemAction,
  onItemContextMenu
}) {
  const normalizedViewState = normalizeConsoleSidebarViewState(viewState);
  return /* @__PURE__ */ (0, import_jsx_runtime13.jsx)("div", { className: clsx_default("cc-theme-scope", "cc-console-sidebar", className), children: normalizedViewState.blocks.map((block) => block.kind === "action_strip" ? /* @__PURE__ */ (0, import_jsx_runtime13.jsx)(
    ActionStrip,
    {
      Icon: Icon2,
      block,
      getActionButtonProps,
      onBlockAction
    },
    block.id
  ) : /* @__PURE__ */ (0, import_jsx_runtime13.jsx)(
    ListBlock,
    {
      Icon: Icon2,
      block,
      getActionButtonProps,
      onBlockAction,
      onItemAction,
      onItemContextMenu,
      onSelectItem,
      onSectionAction,
      onSelectSection,
      renderItemTrailing,
      renderSectionHeader,
      renderSectionContainer
    },
    block.id
  )) });
}

// ../packages/console-components/src/workbench/console-workbench.tsx
var import_jsx_runtime14 = require("react/jsx-runtime");
function ConsoleWorkbench({
  launcher,
  main,
  activityRail = null,
  launcherResizeHandle = null,
  launcherHeader = null,
  launcherFooter = null,
  activityRailResizeHandle = null,
  activityRailHeader = null,
  activityRailFooter = null,
  mainHeader = null,
  mainFooter = null,
  id,
  className
}) {
  return /* @__PURE__ */ (0, import_jsx_runtime14.jsxs)(
    "div",
    {
      className: clsx_default("cc-theme-scope", "cc-workbench", activityRail && "has-activity-rail", className),
      "data-console-workbench": "root",
      id,
      children: [
        /* @__PURE__ */ (0, import_jsx_runtime14.jsxs)("aside", { className: "cc-workbench__launcher", "data-console-workbench-part": "launcher", children: [
          launcherHeader ? /* @__PURE__ */ (0, import_jsx_runtime14.jsx)("div", { className: "cc-workbench__launcher-header", "data-console-workbench-part": "launcher-header", children: launcherHeader }) : null,
          /* @__PURE__ */ (0, import_jsx_runtime14.jsx)("div", { className: "cc-workbench__launcher-body", "data-console-workbench-part": "launcher-body", children: launcher }),
          launcherFooter ? /* @__PURE__ */ (0, import_jsx_runtime14.jsx)("div", { className: "cc-workbench__launcher-footer", "data-console-workbench-part": "launcher-footer", children: launcherFooter }) : null
        ] }),
        launcherResizeHandle,
        /* @__PURE__ */ (0, import_jsx_runtime14.jsxs)("section", { className: "cc-workbench__main", "data-console-workbench-part": "main", children: [
          mainHeader ? /* @__PURE__ */ (0, import_jsx_runtime14.jsx)("div", { className: "cc-workbench__main-header", "data-console-workbench-part": "main-header", children: mainHeader }) : null,
          /* @__PURE__ */ (0, import_jsx_runtime14.jsx)("div", { className: "cc-workbench__main-body", "data-console-workbench-part": "main-body", children: main }),
          mainFooter ? /* @__PURE__ */ (0, import_jsx_runtime14.jsx)("div", { className: "cc-workbench__main-footer", "data-console-workbench-part": "main-footer", children: mainFooter }) : null
        ] }),
        activityRail ? /* @__PURE__ */ (0, import_jsx_runtime14.jsxs)(import_jsx_runtime14.Fragment, { children: [
          activityRailResizeHandle,
          /* @__PURE__ */ (0, import_jsx_runtime14.jsxs)("aside", { className: "cc-workbench__activity", "data-console-workbench-part": "activity", children: [
            activityRailHeader ? /* @__PURE__ */ (0, import_jsx_runtime14.jsx)("div", { className: "cc-workbench__activity-header", "data-console-workbench-part": "activity-header", children: activityRailHeader }) : null,
            /* @__PURE__ */ (0, import_jsx_runtime14.jsx)("div", { className: "cc-workbench__activity-body", "data-console-workbench-part": "activity-body", children: activityRail }),
            activityRailFooter ? /* @__PURE__ */ (0, import_jsx_runtime14.jsx)("div", { className: "cc-workbench__activity-footer", "data-console-workbench-part": "activity-footer", children: activityRailFooter }) : null
          ] })
        ] }) : null
      ]
    }
  );
}

// ../packages/console-components/src/composer/console-composer.tsx
var import_jsx_runtime15 = require("react/jsx-runtime");
function kindToClassName(kind, zone) {
  switch (kind) {
    case "pill":
      return "cc-composer__pill";
    case "pill-icon":
      return "cc-composer__pill-icon";
    case "sub-pill":
      return "cc-composer__sub-pill";
    case "icon":
      return zone === "main" ? "cc-composer__icon-ghost" : "cc-composer__footer-icon";
    default:
      return "cc-composer__pill";
  }
}
function visible(items) {
  return items.filter((item) => !item.hidden);
}
function ConsoleComposer({
  viewState,
  Icon: Icon2,
  className,
  shellClassName,
  footerClassName,
  inputId,
  inputRef,
  shellId,
  submitButtonId,
  getToolbarButtonProps,
  renderMainRow,
  renderFooter,
  onChange,
  onFocus,
  onKeyDown,
  onSubmit
}) {
  const {
    value,
    disabled = false,
    placeholder,
    submitDisabled = false,
    submitLabel = "Send prompt",
    mainRowItems,
    footerLeftItems,
    footerRightItems
  } = viewState;
  const visibleMain = visible(mainRowItems);
  const visibleFooterLeft = visible(footerLeftItems);
  const visibleFooterRight = visible(footerRightItems);
  const hasFooter = visibleFooterLeft.length > 0 || visibleFooterRight.length > 0;
  function renderToolbarButton(item, zone) {
    const baseClass = kindToClassName(item.kind, zone);
    const extra = getToolbarButtonProps?.({ zone, item });
    const { ref, className: extraClass, ...restExtra } = extra ?? {};
    return /* @__PURE__ */ (0, import_jsx_runtime15.jsxs)(
      "button",
      {
        className: clsx_default(baseClass, item.hasMenu && "has-menu", extraClass),
        disabled: item.disabled,
        type: "button",
        ref,
        ...restExtra,
        children: [
          item.iconName && Icon2 ? /* @__PURE__ */ (0, import_jsx_runtime15.jsxs)(import_jsx_runtime15.Fragment, { children: [
            /* @__PURE__ */ (0, import_jsx_runtime15.jsx)(Icon2, { name: item.iconName }),
            " "
          ] }) : null,
          item.label ?? null,
          item.hasMenu && Icon2 ? /* @__PURE__ */ (0, import_jsx_runtime15.jsxs)(import_jsx_runtime15.Fragment, { children: [
            " ",
            /* @__PURE__ */ (0, import_jsx_runtime15.jsx)(Icon2, { className: "chev", name: "i-chevron" })
          ] }) : null
        ]
      },
      item.id
    );
  }
  const defaultMainRow = /* @__PURE__ */ (0, import_jsx_runtime15.jsxs)("div", { className: "cc-composer__main-row", children: [
    visibleMain.map((item) => renderToolbarButton(item, "main")),
    /* @__PURE__ */ (0, import_jsx_runtime15.jsx)(
      "button",
      {
        className: "cc-composer__send-btn",
        disabled: submitDisabled,
        id: submitButtonId,
        title: submitLabel,
        type: "button",
        onClick: onSubmit,
        children: "\u2191"
      }
    )
  ] });
  const mainRow = renderMainRow ? renderMainRow({ items: visibleMain, defaultMainRow }) : defaultMainRow;
  const defaultFooter = hasFooter ? /* @__PURE__ */ (0, import_jsx_runtime15.jsxs)("div", { className: clsx_default("cc-composer__footer", footerClassName), children: [
    /* @__PURE__ */ (0, import_jsx_runtime15.jsx)("div", { className: "cc-composer__footer-left", children: visibleFooterLeft.map((item) => renderToolbarButton(item, "footer-left")) }),
    /* @__PURE__ */ (0, import_jsx_runtime15.jsx)("div", { className: "cc-composer__footer-right", children: visibleFooterRight.map((item) => renderToolbarButton(item, "footer-right")) })
  ] }) : null;
  const footer = renderFooter ? renderFooter({
    leftItems: visibleFooterLeft,
    rightItems: visibleFooterRight,
    defaultFooter
  }) : defaultFooter;
  return /* @__PURE__ */ (0, import_jsx_runtime15.jsxs)("section", { className: clsx_default("cc-composer", className), children: [
    /* @__PURE__ */ (0, import_jsx_runtime15.jsxs)("div", { className: clsx_default("cc-composer__shell", shellClassName), id: shellId, children: [
      /* @__PURE__ */ (0, import_jsx_runtime15.jsx)(
        "textarea",
        {
          ref: inputRef,
          className: "cc-composer__textarea",
          disabled,
          id: inputId,
          placeholder,
          value,
          onChange: (event) => onChange(event.currentTarget.value),
          onFocus,
          onKeyDown
        }
      ),
      mainRow
    ] }),
    footer
  ] });
}

// src/lib/agents.ts
function normalizeAgents(experience, modules) {
  const identityStatusRows = Array.isArray(experience?.identity_status?.rows) ? experience.identity_status.rows : [];
  const normalizedIdentityStatusRows = identityStatusRows.map((entry) => normalizeIdentityStatusRow(entry)).filter((entry) => entry !== null);
  const identityStatusByIdentity = new Map(
    normalizedIdentityStatusRows.map((row) => [row.identity, row])
  );
  const snapshotAgents = experience?.agent_sidebar?.live_snapshot?.agents;
  if (Array.isArray(snapshotAgents) && snapshotAgents.length > 0) {
    return snapshotAgents.map((entry) => {
      const entryIdentity = typeof entry.identity === "string" ? entry.identity.trim() : "";
      const entryMemberId = typeof entry.member_id === "string" ? entry.member_id.trim() : "";
      const statusRow = identityStatusByIdentity.get(entryIdentity) || identityStatusByIdentity.get(entryMemberId) || normalizeIdentityStatusRow(entry);
      const watchFields = normalizeSidebarWatchFields(entry);
      const responsePhase = normalizeResponsePhase(entry.response_phase);
      return {
        ...statusRow?.identity ? { identity: statusRow.identity } : entry.identity ? { identity: String(entry.identity) } : {},
        agent_id: String(entry.agent_id || statusRow?.identity || entry.identity || entry.member_id || ""),
        member_id: String(entry.member_id || statusRow?.identity || entry.identity || entry.agent_id || ""),
        ...typeof entry.session_id === "string" && entry.session_id.trim() ? { session_id: entry.session_id.trim() } : {},
        label: String(entry.label || statusRow?.display_name || entry.display_name || statusRow?.identity || entry.identity || entry.member_id || entry.agent_id || "unknown"),
        kind: String(entry.kind || statusRow?.profile || entry.profile || "module_agent"),
        ...statusRow?.profile !== void 0 ? { profile: statusRow.profile } : entry.profile !== void 0 ? { profile: String(entry.profile) } : {},
        ...statusRow?.state !== void 0 ? { state: statusRow.state } : entry.state !== void 0 ? { state: String(entry.state) } : {},
        ...statusRow?.addressability ? { addressability: statusRow.addressability } : {},
        ...statusRow?.generation !== void 0 ? { generation: statusRow.generation } : {},
        ...statusRow?.checkpoint_version !== void 0 ? { checkpoint_version: statusRow.checkpoint_version } : {},
        ...statusRow?.lease_healthy !== void 0 ? { lease_healthy: statusRow.lease_healthy } : {},
        ...responsePhase !== null && { response_phase: responsePhase },
        ...entry.wired_to !== void 0 && { wired_to: entry.wired_to },
        ...statusRow?.labels && Object.keys(statusRow.labels).length > 0 ? { labels: statusRow.labels } : entry.labels !== void 0 ? { labels: entry.labels } : {},
        ...entry.group !== void 0 && { group: String(entry.group) },
        ...entry.addressable !== void 0 ? { addressable: Boolean(entry.addressable) } : statusRow?.addressability ? { addressable: statusRow.addressability === "addressable" } : {},
        ...entry.affordances !== void 0 && { affordances: entry.affordances },
        ...watchFields
      };
    });
  }
  if (Array.isArray(identityStatusRows) && identityStatusRows.length > 0) {
    return identityStatusRows.map((entry) => {
      const statusRow = normalizeIdentityStatusRow(entry);
      const identity = statusRow?.identity || "";
      return {
        identity,
        agent_id: String(identity),
        member_id: identity ? `identity-only:${identity}` : "",
        ...typeof statusRow?.session_id === "string" && statusRow.session_id.trim() ? { session_id: statusRow.session_id.trim() } : {},
        label: String(statusRow?.display_name || identity || "unknown"),
        kind: String(statusRow?.profile || "identity"),
        ...statusRow?.profile !== void 0 ? { profile: statusRow.profile } : {},
        ...statusRow?.state !== void 0 ? { state: statusRow.state } : {},
        ...statusRow?.addressability ? { addressability: statusRow.addressability } : {},
        ...statusRow?.generation !== void 0 ? { generation: statusRow.generation } : {},
        ...statusRow?.checkpoint_version !== void 0 ? { checkpoint_version: statusRow.checkpoint_version } : {},
        ...statusRow?.lease_healthy !== void 0 ? { lease_healthy: statusRow.lease_healthy } : {},
        ...statusRow?.labels && Object.keys(statusRow.labels).length > 0 ? { labels: statusRow.labels } : {},
        addressable: false,
        affordances: { can_send_message: false }
      };
    });
  }
  if (Array.isArray(modules) && modules.length > 0) {
    return modules.map((moduleId) => ({
      agent_id: String(moduleId),
      member_id: String(moduleId),
      label: String(moduleId),
      kind: "module_agent"
    }));
  }
  return [];
}

// src/lib/adapters.ts
function buildPanelConversationKey(panelId, target) {
  if (!target) {
    return `panel:${panelId}:none`;
  }
  if (target.kind !== "agent-chat") {
    return `panel:${panelId}:${target.kind}:${target.id}`;
  }
  const targetKey = target.addressingMode === "identity" ? target.identity || target.memberId || target.id : target.memberId || target.id;
  return `panel:${panelId}:${target.kind}:${targetKey}`;
}
function buildDockTarget(agent) {
  const subtitle = [agent.profile, agent.kind].filter(Boolean).join(" \xB7 ") || void 0;
  const identity = typeof agent.identity === "string" && agent.identity.trim() ? agent.identity.trim() : void 0;
  const addressingMode = identity ? "identity" : "member";
  return {
    id: agent.member_id,
    kind: "agent-chat",
    addressingMode,
    memberId: agent.member_id,
    ...identity ? { identity } : {},
    title: agent.label,
    subtitle,
    iconName: "i-team"
  };
}
function buildInspectTarget(agent) {
  return {
    id: `inspect:${agent.identity || agent.member_id}`,
    kind: "identity-inspect",
    identity: agent.identity || agent.member_id,
    memberId: agent.member_id,
    title: `${agent.label} Inspect`,
    subtitle: agent.identity || agent.member_id,
    iconName: "i-terminal"
  };
}
function buildControlTarget(kind) {
  switch (kind) {
    case "routing":
      return { id: "routing", kind, title: "Routing", subtitle: "Routes and delivery history", iconName: "i-swap" };
    case "gating":
      return { id: "gating", kind, title: "Gating", subtitle: "Pending approvals and audit", iconName: "i-bolt" };
    case "topology":
      return { id: "topology", kind, title: "Topology", subtitle: "Identity connectivity", iconName: "i-team" };
    case "health":
      return { id: "health", kind, title: "Health", subtitle: "Runtime and identity health", iconName: "i-gear" };
    default:
      return { id: kind, kind: "health", title: "Health" };
  }
}
function agentGroupKey(agent) {
  return agent.group?.trim() || agent.profile?.trim() || agent.kind?.trim() || "Agents";
}
function agentStateTone(state) {
  switch (state) {
    case "running":
      return "accent";
    case "active":
      return "positive";
    case "idle":
      return "muted";
    case "error":
      return "negative";
    default:
      return "muted";
  }
}
function sectionIconForGroup(group) {
  const lower = group.toLowerCase();
  if (lower.includes("coordinator") || lower.includes("system")) return "i-bolt";
  if (lower.includes("domain") || lower.includes("specialist")) return "i-cube";
  if (lower.includes("internal") || lower.includes("infra")) return "i-gear";
  if (lower.includes("personal") || lower.includes("identity")) return "i-team";
  return "i-folder";
}
function buildSidebarViewState(args) {
  const { agents, selectedMemberId, pinnedAgentIds = /* @__PURE__ */ new Set(), sortMode = "group" } = args;
  const sorted = [...agents].sort((a, b) => {
    const aPinned = pinnedAgentIds.has(a.member_id) ? 0 : 1;
    const bPinned = pinnedAgentIds.has(b.member_id) ? 0 : 1;
    if (aPinned !== bPinned) return aPinned - bPinned;
    if (sortMode === "alpha") return a.label.localeCompare(b.label);
    if (sortMode === "status") {
      const stateOrder = (s) => s === "running" ? 0 : s === "active" ? 1 : 2;
      const diff = stateOrder(a.state) - stateOrder(b.state);
      if (diff !== 0) return diff;
    }
    return a.label.localeCompare(b.label);
  });
  const grouped = /* @__PURE__ */ new Map();
  for (const agent of sorted) {
    const key = agentGroupKey(agent);
    const bucket = grouped.get(key) || [];
    bucket.push(agent);
    grouped.set(key, bucket);
  }
  const sections = Array.from(grouped.entries()).map(([group, members]) => ({
    id: group,
    title: group,
    iconName: sectionIconForGroup(group),
    meta: [{ id: "count", label: `${members.length}` }],
    items: members.map((agent) => {
      const isAddressable = agent.addressable || agent.affordances?.can_send_message;
      const isPinned = pinnedAgentIds.has(agent.member_id);
      const watchFields = normalizeSidebarWatchFields(agent);
      return {
        id: agent.member_id,
        title: agent.label,
        subtitle: agent.identity || agent.member_id,
        selected: agent.member_id === selectedMemberId,
        pinned: isPinned,
        disabled: !isAddressable,
        ...watchFields,
        meta: [
          ...agent.state ? [{ id: "state", label: agent.state, tone: agentStateTone(agent.state) }] : [],
          ...agent.response_phase ? [{ id: "phase", label: agent.response_phase, tone: "accent" }] : []
        ],
        actions: [
          {
            id: "inspect_identity",
            label: "Inspect identity",
            iconName: "i-terminal"
          },
          {
            id: "toggle_pin",
            label: isPinned ? "Unpin agent" : "Pin agent",
            iconName: "i-pin",
            active: isPinned
          }
        ]
      };
    })
  }));
  return {
    blocks: [
      {
        id: "controls",
        kind: "action_strip",
        actions: [
          { id: "open_routing", label: "Routing", iconName: "i-swap" },
          { id: "open_gating", label: "Gating", iconName: "i-bolt" },
          { id: "open_topology", label: "Topology", iconName: "i-team" },
          { id: "open_health", label: "Health", iconName: "i-gear" }
        ]
      },
      {
        id: "agents",
        kind: "list",
        title: "Agents",
        actions: [
          { id: "spawn_agent", label: "Spawn agent", iconName: "i-plus" },
          { id: "filter_sort", label: "Sort & filter", iconName: "i-sliders" }
        ],
        sections
      }
    ]
  };
}
function buildRoutingSectionView(args) {
  const routesRecord = typeof args.routesResponse === "object" && args.routesResponse !== null ? args.routesResponse : {};
  const historyRecord = typeof args.historyResponse === "object" && args.historyResponse !== null ? args.historyResponse : {};
  const normalized = normalizeRoutingSectionView({
    routes: Array.isArray(routesRecord.routes) ? routesRecord.routes : [],
    deliveries: Array.isArray(historyRecord.deliveries) ? historyRecord.deliveries : []
  });
  return normalized ?? { routes: [], deliveries: [] };
}
var USER_IDENTITY = {
  id: "user",
  label: "You",
  role: "user"
};
function agentIdentity(agent) {
  return {
    id: agent?.member_id || "agent",
    label: agent?.label || "Agent",
    role: "assistant"
  };
}
var SYSTEM_IDENTITY = {
  id: "system",
  label: "System",
  role: "system",
  presentation: "system",
  showLabel: true
};
function summarizeFrameData(data) {
  if (typeof data === "string") {
    const trimmed = data.trim();
    if (trimmed.startsWith("{") && trimmed.endsWith("}") || trimmed.startsWith("[") && trimmed.endsWith("]")) {
      try {
        return summarizeFrameData(JSON.parse(trimmed));
      } catch {
        return data;
      }
    }
    return data;
  }
  if (typeof data === "object" && data !== null) {
    const record = data;
    if (typeof record.delta === "string") return record.delta;
    if (typeof record.text === "string" && record.text.trim()) return record.text;
    if (typeof record.result === "string" && record.result.trim()) return record.result;
    if (typeof record.message === "string" && record.message.trim()) return record.message;
    if (typeof record.error === "string" && record.error.trim()) return record.error;
    if (typeof record.reason === "string" && record.reason.trim()) return record.reason;
    if (typeof record.kind === "string" && typeof record.event_type === "string") return "";
    return JSON.stringify(record);
  }
  return String(data ?? "");
}
function eventSortRank(event) {
  switch (event) {
    case "interaction_started":
      return 0;
    case "tool_call_requested":
    case "tool_call":
    case "tool_execution_started":
      return 20;
    case "tool_result_received":
    case "tool_execution_completed":
      return 30;
    case "text_delta":
      return 40;
    case "text_complete":
      return 45;
    case "interaction_complete":
    case "interaction_failed":
    case "run_completed":
    case "run_failed":
      return 90;
    default:
      return 50;
  }
}
function sortFramesForTranscript(frames) {
  const interactionStartMs = /* @__PURE__ */ new Map();
  for (const frame of frames) {
    const interactionId = frame.interactionId?.trim();
    const timestampMs = typeof frame.timestampMs === "number" ? frame.timestampMs : Number.MAX_SAFE_INTEGER;
    if (!interactionId) continue;
    const current = interactionStartMs.get(interactionId);
    if (current === void 0 || timestampMs < current) {
      interactionStartMs.set(interactionId, timestampMs);
    }
  }
  return frames.map((frame, index) => ({ frame, index })).sort((left, right) => {
    const leftInteraction = left.frame.interactionId?.trim() || "";
    const rightInteraction = right.frame.interactionId?.trim() || "";
    const leftGroupTs = (leftInteraction && interactionStartMs.get(leftInteraction)) ?? (typeof left.frame.timestampMs === "number" ? left.frame.timestampMs : Number.MAX_SAFE_INTEGER);
    const rightGroupTs = (rightInteraction && interactionStartMs.get(rightInteraction)) ?? (typeof right.frame.timestampMs === "number" ? right.frame.timestampMs : Number.MAX_SAFE_INTEGER);
    if (leftGroupTs !== rightGroupTs) {
      return leftGroupTs - rightGroupTs;
    }
    if (leftInteraction && rightInteraction && leftInteraction === rightInteraction) {
      const leftRank = eventSortRank(left.frame.event);
      const rightRank = eventSortRank(right.frame.event);
      if (leftRank !== rightRank) {
        return leftRank - rightRank;
      }
    }
    const leftTs = typeof left.frame.timestampMs === "number" ? left.frame.timestampMs : Number.MAX_SAFE_INTEGER;
    const rightTs = typeof right.frame.timestampMs === "number" ? right.frame.timestampMs : Number.MAX_SAFE_INTEGER;
    if (leftTs !== rightTs) {
      return leftTs - rightTs;
    }
    return left.index - right.index;
  }).map(({ frame }) => frame);
}
var HIDDEN_EVENTS = /* @__PURE__ */ new Set([
  "subscribed",
  "run_started",
  "run_completed",
  "turn_started",
  "turn_completed",
  "text_complete",
  "reasoning_delta",
  "reasoning_complete",
  "interaction_started",
  "run_failed",
  "keep-alive",
  "tool_config_changed",
  "tool_scope_changed",
  "tool_call_requested",
  "tool_call",
  "tool_execution_started"
]);
var ACTIVITY_HIDDEN_EVENTS = /* @__PURE__ */ new Set([
  ...HIDDEN_EVENTS,
  "text_delta",
  "tool_result_received",
  "tool_execution_completed"
]);
function isoFromTimestampMs(timestampMs) {
  if (typeof timestampMs !== "number" || !Number.isFinite(timestampMs)) {
    return void 0;
  }
  return new Date(timestampMs).toISOString();
}
function parseToolCallId(frame) {
  const record = frame.data && typeof frame.data === "object" ? frame.data : null;
  const id = record?.tool_call_id ?? record?.id;
  return typeof id === "string" && id.trim() ? id.trim() : null;
}
function parseToolName(frame) {
  const record = frame.data && typeof frame.data === "object" ? frame.data : null;
  return typeof record?.name === "string" && record.name.trim() ? record.name : "tool";
}
function parseToolArguments(frame) {
  const record = frame.data && typeof frame.data === "object" ? frame.data : null;
  if (typeof record?.arguments === "string" && record.arguments.trim()) {
    return record.arguments;
  }
  if ("args" in (record || {}) && record?.args !== void 0) {
    return JSON.stringify(record.args);
  }
  return JSON.stringify(record || {});
}
function parseToolResult(frame) {
  const record = frame.data && typeof frame.data === "object" ? frame.data : null;
  const result = summarizeFrameData(frame.data).trim();
  const isError = Boolean(record?.is_error) || frame.event === "interaction_failed";
  return {
    ...result ? { result } : {},
    status: isError ? "error" : "success"
  };
}
function buildToolBlocks(frames) {
  const toolCalls = /* @__PURE__ */ new Map();
  const pendingResults = /* @__PURE__ */ new Map();
  for (const frame of frames) {
    if (frame.event === "tool_result_received" || frame.event === "tool_execution_completed") {
      const toolCallId = parseToolCallId(frame);
      if (!toolCallId) continue;
      const parsed = parseToolResult(frame);
      if (toolCalls.has(toolCallId)) {
        const current = toolCalls.get(toolCallId);
        toolCalls.set(toolCallId, {
          ...current,
          ...parsed.result ? { result: parsed.result } : {},
          status: parsed.status
        });
      } else {
        pendingResults.set(toolCallId, parsed);
      }
    }
    if (frame.event === "tool_call_requested" || frame.event === "tool_call" || frame.event === "tool_execution_started") {
      const toolCallId = parseToolCallId(frame);
      if (!toolCallId || toolCalls.has(toolCallId)) continue;
      const pending = pendingResults.get(toolCallId);
      toolCalls.set(toolCallId, {
        type: "tool-call",
        toolCallId,
        name: parseToolName(frame),
        arguments: parseToolArguments(frame),
        ...pending?.result ? { result: pending.result } : {},
        status: pending?.status || "pending"
      });
    }
  }
  return toolCalls;
}
function renderTerminalEntry(agent, frame, entryId, streamedText = "") {
  if (frame.event === "interaction_complete") {
    const text = summarizeFrameData(frame.data).trim();
    if (!text) return null;
    if (streamedText.trim() && normalizeComparableText(streamedText) === normalizeComparableText(text)) {
      return null;
    }
    const blocks = parseConversationRichBlocks(text);
    return {
      kind: "message",
      id: entryId,
      identity: agentIdentity(agent),
      variant: blocks.length > 0 ? "rich" : "plain",
      createdAt: isoFromTimestampMs(frame.timestampMs),
      ...blocks.length > 0 ? { blocks } : { text }
    };
  }
  if (frame.event === "interaction_failed" || frame.event === "run_failed") {
    const text = `${frame.event}: ${summarizeFrameData(frame.data)}`.trim();
    if (!text || text === `${frame.event}:`) return null;
    return {
      kind: "message",
      id: entryId,
      identity: SYSTEM_IDENTITY,
      variant: "meta",
      createdAt: isoFromTimestampMs(frame.timestampMs),
      text
    };
  }
  return null;
}
function normalizeComparableText(value) {
  return value.replace(/\s+/g, " ").trim();
}
function buildQuickPromptSuggestions(agent) {
  const labels = agent?.labels ?? {};
  const suggestions = [];
  for (let index = 1; index <= 4; index++) {
    const label = labels[`console_prompt_${index}_label`]?.trim();
    const value = labels[`console_prompt_${index}_value`]?.trim();
    if (!label || !value) continue;
    suggestions.push({
      id: `prompt-${index}`,
      label,
      value,
      iconName: "i-bolt"
    });
  }
  return suggestions;
}
function renderHistoryUserEntry(frame, entryId) {
  if (frame.event !== "interaction_started" || typeof frame.data !== "object" || frame.data === null) {
    return null;
  }
  const record = frame.data;
  const content = typeof record.content === "string" ? record.content.trim() : "";
  if (!content) return null;
  return {
    kind: "message",
    id: entryId,
    identity: USER_IDENTITY,
    variant: "plain",
    createdAt: isoFromTimestampMs(frame.timestampMs),
    text: content
  };
}
function renderRunStartedPromptEntries(frame, entryId, options = {}) {
  if (frame.event !== "run_started" || typeof frame.data !== "object" || frame.data === null) {
    return [];
  }
  const record = frame.data;
  const prompt = typeof record.prompt === "string" ? record.prompt.trim() : "";
  if (!prompt) {
    return [];
  }
  const createdAt = isoFromTimestampMs(frame.timestampMs);
  const entries = [];
  const embeddedPrompt = extractEmbeddedRpcPrompt(prompt);
  if (embeddedPrompt && !options.suppressEmbeddedRpcPrompt) {
    entries.push({
      kind: "message",
      id: `${entryId}:event`,
      identity: USER_IDENTITY,
      variant: "plain",
      ...createdAt ? { createdAt } : {},
      text: embeddedPrompt
    });
  }
  if (prompt.startsWith("[COMMS")) {
    const summarized = summarizeCommsTransport(prompt).trim();
    if (summarized) {
      entries.push({
        kind: "message",
        id: entryId,
        identity: SYSTEM_IDENTITY,
        variant: "meta",
        ...createdAt ? { createdAt } : {},
        text: summarized
      });
    }
  }
  return entries;
}
function summarizeCommsTransport(text) {
  const lines = text.split("\n").map((line) => line.trim()).filter(Boolean);
  if (lines.length === 0) {
    return "";
  }
  const header = lines[0] || "";
  const headerTail = header.includes("]") ? header.slice(header.indexOf("]") + 1).trim() : "";
  const body = lines.slice(1).filter((line) => !line.startsWith("[EVENT via rpc]"));
  if (header.startsWith("[COMMS REQUEST")) {
    const intentLine = body.find((line) => line.startsWith("Intent:"));
    if (intentLine) {
      const summary = intentLine.replace(/^Intent:\s*/, "Peer request: ");
      if (summary === "Peer request: mob.peer_added" || summary === "Peer request: mob.peer_removed") {
        return "";
      }
      return summary;
    }
    return "Peer request received.";
  }
  if (header.startsWith("[COMMS RESPONSE")) {
    const resultIndex = body.findIndex((line) => line.startsWith("Result:"));
    if (resultIndex >= 0) {
      const joined = body.slice(resultIndex).filter((line) => !line.startsWith("[COMMS ")).join(" ");
      return joined.replace(/^Result:\s*/, "Peer response: ");
    }
    return "Peer response received.";
  }
  if (header.startsWith("[COMMS MESSAGE")) {
    const joined = [headerTail, ...body].join(" ").trim();
    return joined ? `Peer message: ${joined}` : "Peer message received.";
  }
  return text;
}
function extractEmbeddedRpcPrompt(text) {
  const match = text.match(/^\[EVENT via rpc\]\s*(.+)$/im);
  return match?.[1]?.trim() || null;
}
function mapFramesToTimelineEntries(agent, frames, options = {}) {
  const orderedFrames = sortFramesForTranscript(frames);
  const entries = [];
  const toolBlocks = buildToolBlocks(orderedFrames);
  const emittedToolCalls = /* @__PURE__ */ new Set();
  let pendingText = "";
  let pendingId = "";
  let pendingCreatedAt;
  function flushPendingText() {
    if (!pendingText) return;
    const blocks = parseConversationRichBlocks(pendingText);
    entries.push({
      kind: "message",
      id: pendingId,
      identity: agentIdentity(agent),
      variant: blocks.length > 0 ? "rich" : "plain",
      ...pendingCreatedAt ? { createdAt: pendingCreatedAt } : {},
      ...blocks.length > 0 ? { blocks } : { text: pendingText }
    });
    pendingText = "";
    pendingId = "";
    pendingCreatedAt = void 0;
  }
  for (let i = 0; i < orderedFrames.length; i++) {
    const frame = orderedFrames[i];
    const entryId = `${frame.id || frame.event || "frame"}:${i}`;
    if (frame.event === "text_delta") {
      if (options.renderTextDeltas === false) {
        continue;
      }
      if (!pendingId) {
        pendingId = entryId;
        pendingCreatedAt = isoFromTimestampMs(frame.timestampMs);
      }
      pendingText += summarizeFrameData(frame.data);
      continue;
    }
    const toolCallId = parseToolCallId(frame);
    if (toolCallId && (frame.event === "tool_call_requested" || frame.event === "tool_call" || frame.event === "tool_execution_started") && !emittedToolCalls.has(toolCallId)) {
      flushPendingText();
      const block = toolBlocks.get(toolCallId);
      if (block) {
        entries.push({
          kind: "message",
          id: entryId,
          identity: agentIdentity(agent),
          variant: "rich",
          createdAt: isoFromTimestampMs(frame.timestampMs),
          blocks: [block]
        });
        emittedToolCalls.add(toolCallId);
      }
      continue;
    }
    if (frame.event === "tool_result_received" || frame.event === "tool_execution_completed") {
      continue;
    }
    if (options.renderInteractionStartsAsUser && frame.event === "interaction_started") {
      flushPendingText();
      const userEntry = renderHistoryUserEntry(frame, entryId);
      if (userEntry) {
        entries.push(userEntry);
      }
      continue;
    }
    if (frame.event === "run_started") {
      flushPendingText();
      const promptEntries = renderRunStartedPromptEntries(frame, entryId, {
        suppressEmbeddedRpcPrompt: options.renderInteractionStartsAsUser === true || options.suppressEmbeddedRunStartedPrompt === true
      });
      if (promptEntries.length > 0) {
        entries.push(...promptEntries);
        continue;
      }
    }
    if (frame.event === "text_complete") {
      continue;
    }
    if (HIDDEN_EVENTS.has(frame.event)) {
      continue;
    }
    const streamedText = pendingText;
    flushPendingText();
    const terminalEntry = renderTerminalEntry(agent, frame, entryId, streamedText);
    if (terminalEntry) {
      entries.push(terminalEntry);
      continue;
    }
    if (frame.event === "interaction_complete") {
      continue;
    }
    const text = `${frame.event}: ${summarizeFrameData(frame.data)}`.trim();
    entries.push({
      kind: "message",
      id: entryId,
      identity: SYSTEM_IDENTITY,
      variant: "meta",
      createdAt: isoFromTimestampMs(frame.timestampMs),
      text
    });
  }
  flushPendingText();
  return entries;
}
function createUserEntry(message) {
  return {
    kind: "message",
    id: `user:${Date.now()}`,
    identity: USER_IDENTITY,
    variant: "plain",
    createdAt: (/* @__PURE__ */ new Date()).toISOString(),
    text: message
  };
}
function sortConversationTimelineEntries(entries) {
  return entries.map((entry, index) => ({ entry, index })).sort((left, right) => {
    const leftTs = Date.parse(String(left.entry.createdAt || ""));
    const rightTs = Date.parse(String(right.entry.createdAt || ""));
    const safeLeft = Number.isFinite(leftTs) ? leftTs : Number.NaN;
    const safeRight = Number.isFinite(rightTs) ? rightTs : Number.NaN;
    if (Number.isFinite(safeLeft) && Number.isFinite(safeRight) && safeLeft !== safeRight) {
      return safeLeft - safeRight;
    }
    if (Number.isFinite(safeLeft) && !Number.isFinite(safeRight)) {
      return 1;
    }
    if (!Number.isFinite(safeLeft) && Number.isFinite(safeRight)) {
      return -1;
    }
    return left.index - right.index;
  }).map(({ entry }) => entry);
}
function buildConversationViewState(args) {
  const groups = groupConversationTimelineEntries(args.entries);
  const suggestions = buildQuickPromptSuggestions(args.agent ?? null);
  return {
    conversationId: args.memberId || "console",
    title: args.agentLabel,
    entries: args.entries,
    groups,
    turnDiff: null,
    emptyState: args.entries.length === 0 ? {
      title: args.agentLabel,
      subtitle: "Send a message to start the conversation.",
      ...suggestions.length ? { suggestions } : {}
    } : null
  };
}
function buildActivityRailViewState(args) {
  const presets = args.filterPresets || [];
  const activePreset = presets.find((preset) => preset.id === args.activePresetId) || null;
  const agentByIdentity = /* @__PURE__ */ new Map();
  const watchedIdentities = /* @__PURE__ */ new Set();
  const criticalIdentities = /* @__PURE__ */ new Set();
  for (const agent of args.agents) {
    if (agent.identity) agentByIdentity.set(agent.identity, agent);
    agentByIdentity.set(agent.member_id, agent);
    if (agent.watched && (agent.identity || agent.member_id)) {
      watchedIdentities.add(agent.identity || agent.member_id);
    }
    if (agent.alertLevel === "critical" && (agent.identity || agent.member_id)) {
      criticalIdentities.add(agent.identity || agent.member_id);
    }
  }
  const filteredFrames = args.eventFrames.filter((frame) => {
    if (ACTIVITY_HIDDEN_EVENTS.has(frame.event)) {
      return false;
    }
    const frameIdentity = frame.identity?.trim();
    if (!activePreset) return true;
    if (activePreset.watchedOnly && frameIdentity && !watchedIdentities.has(frameIdentity)) {
      return false;
    }
    if (activePreset.alertLevels?.length && frameIdentity) {
      const agent = agentByIdentity.get(frameIdentity);
      if (!agent?.alertLevel || !activePreset.alertLevels.includes(agent.alertLevel)) {
        return false;
      }
    }
    if (activePreset.eventTypeFilter?.length && !activePreset.eventTypeFilter.includes(frame.event)) {
      return false;
    }
    return true;
  });
  const pulseItems = filteredFrames.slice(0, 50).map((frame, index) => {
    const frameIdentity = frame.identity?.trim();
    const agent = frameIdentity ? agentByIdentity.get(frameIdentity) : null;
    return {
      id: `event:${frame.id || index}`,
      title: agent?.label || frameIdentity || frame.event || "event",
      line: summarizeFrameData(frame.data).slice(0, 120) || frame.event,
      meta: frame.event || frame.id || "",
      ...agent ? { focusId: agent.member_id } : {}
    };
  });
  return {
    panels: [
      {
        id: "pulse",
        kind: "pulse",
        title: "Activity",
        actions: presets.map((preset) => ({
          id: preset.id,
          label: preset.label,
          active: preset.id === (activePreset?.id || "all")
        })),
        items: pulseItems,
        emptyText: "No events yet"
      }
    ]
  };
}

// src/lib/errors.ts
function errorMessage(error) {
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}

// src/lib/network.ts
function unwrapConsoleEnvelope(eventName, data) {
  if (!data || typeof data !== "object") {
    return { data };
  }
  const record = data;
  if (typeof record.event_id === "string" && typeof record.event_type === "string" && typeof record.identity === "string" && "data" in record) {
    const envelope = record;
    return {
      id: envelope.event_id,
      event: envelope.event_type || eventName,
      identity: envelope.identity,
      interactionId: envelope.interaction_id,
      timestampMs: envelope.timestamp_ms,
      data: envelope.data
    };
  }
  return { data };
}
function parseSseFrames(rawText) {
  const blocks = rawText.split(/\n\n+/).map((part) => part.trim()).filter(Boolean);
  const frames = [];
  for (const block of blocks) {
    const lines = block.split("\n");
    let id = "";
    let event = "message";
    const dataLines = [];
    for (const line of lines) {
      if (line.startsWith("id:")) {
        id = line.slice(3).trim();
        continue;
      }
      if (line.startsWith("event:")) {
        event = line.slice(6).trim();
        continue;
      }
      if (line.startsWith("data:")) {
        dataLines.push(line.slice(5).trim());
      }
    }
    if (!id && dataLines.length === 0) {
      continue;
    }
    const rawData = dataLines.join("\n");
    let data = rawData;
    if (rawData) {
      try {
        data = JSON.parse(rawData);
      } catch (_) {
        data = rawData;
      }
    }
    const normalized = unwrapConsoleEnvelope(event, data);
    frames.push({
      id: normalized.id || id,
      event: normalized.event || event,
      identity: normalized.identity,
      interactionId: normalized.interactionId,
      timestampMs: normalized.timestampMs,
      data: normalized.data
    });
  }
  return frames;
}
async function fetchJson(baseUrl, path) {
  const response = await fetch(`${baseUrl}${path}`);
  if (!response.ok) {
    const text = await response.text();
    throw new Error(`Request failed ${response.status} for ${path}: ${text}`);
  }
  return response.json();
}
async function rpc(baseUrl, method, params) {
  const response = await fetch(`${baseUrl}/console/rpc`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      jsonrpc: "2.0",
      id: `${method}:${Date.now()}`,
      method,
      params
    })
  });
  if (!response.ok) {
    const text = await response.text();
    throw new Error(`${method} request failed ${response.status}: ${text}`);
  }
  const result = await response.json();
  if (result.error) {
    const typedError = normalizeConsoleInteractionRejectedError(result.error);
    if (typedError) {
      const error = new Error(`${method} RPC error ${typedError.code}: ${typedError.message}`);
      error.rpcError = typedError;
      throw error;
    }
    throw new Error(`${method} RPC error: ${result.error.message || JSON.stringify(result.error)}`);
  }
  return result.result;
}
async function sendMessage(baseUrl, memberId, message) {
  return rpc(baseUrl, "mobkit/send_message", {
    member_id: memberId,
    message
  });
}
var TERMINAL_SSE_EVENTS = /* @__PURE__ */ new Set([
  "interaction_complete",
  "run_completed",
  "interaction_failed",
  "run_failed"
]);
function matchesCorrelation(candidate, correlation, allowUnscoped = true) {
  if (!correlation?.sessionId && !correlation?.interactionId) {
    return true;
  }
  if (candidate === null || typeof candidate !== "object") {
    return allowUnscoped;
  }
  const record = candidate;
  const sessionId = record.session_id ?? record.sessionId;
  const interactionId = record.interaction_id ?? record.interactionId;
  const hasScopedField = sessionId !== void 0 || interactionId !== void 0;
  if (!hasScopedField) {
    return allowUnscoped;
  }
  if (correlation.sessionId && sessionId === correlation.sessionId) {
    return true;
  }
  if (correlation.interactionId && interactionId === correlation.interactionId) {
    return true;
  }
  return false;
}
async function streamFramesFromResponse(response, options = {}) {
  const stopOnTerminal = options.stopOnTerminal ?? Boolean(options.correlation);
  if (!response.ok) {
    const text = await response.text();
    let parsed = null;
    try {
      parsed = JSON.parse(text);
    } catch {
      parsed = null;
    }
    const replayError = normalizeReplayUnavailableError(parsed);
    if (replayError) {
      const error = new Error(
        `interaction stream replay unavailable for ${replayError.stream}: ${replayError.requested_last_event_id} -> ${replayError.latest_event_id}`
      );
      error.replayError = replayError;
      throw error;
    }
    throw new Error(`interaction stream request failed ${response.status}: ${text}`);
  }
  if (!response.body || typeof response.body.getReader !== "function") {
    const frames2 = parseSseFrames(await response.text());
    for (const frame of frames2) {
      if (matchesCorrelation(frame, options.correlation, true)) {
        options.onFrame?.(frame);
      }
    }
    return !options.correlation ? frames2 : frames2.filter((frame) => matchesCorrelation(frame, options.correlation, true));
  }
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let frameBuffer = "";
  const frames = [];
  try {
    while (true) {
      const { value, done } = await reader.read();
      if (done) {
        break;
      }
      const chunk = decoder.decode(value, { stream: true });
      frameBuffer += chunk;
      let sawTerminal = false;
      frameBuffer = flushSseBlocks(frameBuffer, (frame) => {
        if (matchesCorrelation(frame, options.correlation, true)) {
          frames.push(frame);
          options.onFrame?.(frame);
          if (stopOnTerminal && TERMINAL_SSE_EVENTS.has(frame.event || "")) {
            sawTerminal = true;
          }
        }
      });
      if (sawTerminal) {
        break;
      }
    }
    const finalChunk = decoder.decode();
    frameBuffer += finalChunk;
    frameBuffer = flushSseBlocks(frameBuffer, (frame) => {
      if (matchesCorrelation(frame, options.correlation, true)) {
        frames.push(frame);
        options.onFrame?.(frame);
      }
    });
    flushTrailingSseBlock(frameBuffer, (frame) => {
      if (matchesCorrelation(frame, options.correlation, true)) {
        frames.push(frame);
        options.onFrame?.(frame);
      }
    });
  } finally {
    try {
      await reader.cancel();
    } catch {
    }
  }
  return frames;
}
function flushSseBlocks(buffer, onFrame) {
  let searchIndex = 0;
  while (true) {
    const boundaryIndex = buffer.indexOf("\n\n", searchIndex);
    if (boundaryIndex === -1) {
      break;
    }
    const block = buffer.slice(0, boundaryIndex + 2);
    buffer = buffer.slice(boundaryIndex + 2);
    searchIndex = 0;
    for (const frame of parseSseFrames(block)) {
      onFrame(frame);
    }
  }
  return buffer;
}
function flushTrailingSseBlock(buffer, onFrame) {
  if (!buffer.trim()) {
    return;
  }
  for (const frame of parseSseFrames(`${buffer}

`)) {
    onFrame(frame);
  }
}
function persistedEventToFrame(raw, index) {
  const record = typeof raw === "object" && raw !== null ? raw : {};
  if (typeof record.event_id === "string" && typeof record.event_type === "string" && typeof record.identity === "string" && "data" in record) {
    return {
      id: String(record.event_id),
      event: String(record.event_type),
      identity: String(record.identity),
      ...typeof record.interaction_id === "string" ? { interactionId: String(record.interaction_id) } : {},
      ...typeof record.timestamp_ms === "number" ? { timestampMs: record.timestamp_ms } : {},
      data: record.data
    };
  }
  const event = typeof record.event === "object" && record.event !== null ? record.event : {};
  if (event.kind === "agent") {
    const payload = typeof event.payload === "object" && event.payload !== null ? event.payload : null;
    return {
      id: String(record.id ?? `event:${index}`),
      event: String(event.event_type ?? "agent_event"),
      ...typeof record.timestamp_ms === "number" ? { timestampMs: record.timestamp_ms } : {},
      data: payload ?? event
    };
  }
  if (event.kind === "module") {
    return {
      id: String(record.id ?? `event:${index}`),
      event: String(event.event_type ?? "module_event"),
      ...typeof record.timestamp_ms === "number" ? { timestampMs: record.timestamp_ms } : {},
      data: event.payload ?? event
    };
  }
  return {
    id: String(record.id ?? `event:${index}`),
    event: String(record.type ?? "event"),
    ...typeof record.timestamp_ms === "number" ? { timestampMs: record.timestamp_ms } : {},
    data: raw
  };
}
async function queryEvents(baseUrl, target, limit = 40) {
  const identity = target.identity?.trim();
  const memberId = target.memberId?.trim();
  const result = await rpc(baseUrl, "mobkit/query_events", {
    limit,
    ...identity ? { identity } : {},
    ...identity ? {} : memberId ? { member_id: memberId } : {}
  });
  let events = result;
  if (typeof result === "object" && result !== null) {
    const record = result;
    if (record.status === "no_event_log_configured") {
      events = Array.isArray(record.events) ? record.events : [];
    }
  }
  if (!Array.isArray(events)) {
    return [];
  }
  return events.filter((raw) => {
    if (typeof raw !== "object" || raw === null) return true;
    const ev = raw.event;
    if (typeof ev !== "object" || ev === null) return true;
    const eventRecord = ev;
    if (eventRecord.kind !== "agent") return true;
    return typeof eventRecord.payload === "object" && eventRecord.payload !== null;
  }).map((event, index) => persistedEventToFrame(event, index));
}
async function sendInteract(baseUrl, identity, content, origin) {
  const accepted = await rpc(baseUrl, "mobkit/interact", {
    identity,
    content,
    origin
  });
  const normalized = normalizeConsoleInteractionAccepted(accepted);
  if (!normalized) {
    throw new Error("mobkit/interact returned an invalid acceptance payload");
  }
  return normalized;
}
async function callConsoleRpc(baseUrl, method, params = {}) {
  return rpc(baseUrl, method, params);
}
function subscribeConsoleEvents(baseUrl, path, onFrame, options) {
  const controller = new AbortController();
  void (async () => {
    const response = await fetch(`${baseUrl}${path}`, {
      method: options?.method || "GET",
      headers: { "content-type": "application/json" },
      ...options?.body ? { body: JSON.stringify(options.body) } : {},
      signal: controller.signal
    });
    await streamFramesFromResponse(response, { onFrame, stopOnTerminal: false });
  })().catch(() => {
  });
  return () => controller.abort();
}

// src/icon.tsx
var import_jsx_runtime16 = require("react/jsx-runtime");
function SpriteSheet() {
  return /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("svg", { className: "sprite-root", width: "0", height: "0", style: { position: "absolute" }, "aria-hidden": "true", children: [
    /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("symbol", { id: "i-plus", viewBox: "0 0 24 24", children: /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M12 5v14M5 12h14" }) }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-compose", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m4 20 4.5-1 9.5-9.5-3.5-3.5L5 15.5 4 20z" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m13.5 4.5 3.5 3.5" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M9 19h11" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-new-thread", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("rect", { x: "4", y: "4", width: "16", height: "16", rx: "3" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m9 15 5.5-5.5 2 2L11 17H9v-2z" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m13 9 2 2" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("symbol", { id: "i-bolt", viewBox: "0 0 24 24", children: /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M13 2 6 13h5l-1 9 8-12h-5z" }) }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-sliders", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M4 6h16M4 12h16M4 18h16" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("circle", { cx: "8", cy: "12", r: "2" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("symbol", { id: "i-folder", viewBox: "0 0 24 24", children: /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M3 6h7l2 2h9v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6z" }) }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("symbol", { id: "i-play", viewBox: "0 0 24 24", children: /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m9 7 9 5-9 5z" }) }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("symbol", { id: "i-stop", viewBox: "0 0 24 24", children: /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M8 8h8v8H8z" }) }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("symbol", { id: "i-chevron", viewBox: "0 0 24 24", children: /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m7 10 5 5 5-5" }) }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-terminal", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m4 6 7 6-7 6" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M13 18h7" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-team", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("circle", { cx: "9", cy: "9", r: "3" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("circle", { cx: "17", cy: "10", r: "2.5" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M4 19a5 5 0 0 1 10 0" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M13.5 19a4 4 0 0 1 7 0" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-branch", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M6 3v6a4 4 0 0 0 4 4h8" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M14 7h4v4" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("circle", { cx: "6", cy: "3", r: "2" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("circle", { cx: "6", cy: "15", r: "2" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("circle", { cx: "18", cy: "13", r: "2" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("symbol", { id: "i-shield", viewBox: "0 0 24 24", children: /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M12 3 4 6v6c0 5 3.5 8 8 9 4.5-1 8-4 8-9V6z" }) }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("symbol", { id: "i-dot", viewBox: "0 0 24 24", children: /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("circle", { cx: "12", cy: "12", r: "4" }) }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-clock", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("circle", { cx: "12", cy: "12", r: "9" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M12 7v6l4 2" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-cube", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m12 3 8 4.5v9L12 21l-8-4.5v-9z" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m12 12 8-4.5" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m12 12-8-4.5" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-sidebar-toggle", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("rect", { x: "3", y: "5", width: "18", height: "14", rx: "2" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M9 5v14" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m14 12 3-3" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m14 12 3 3" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-open", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M4 12V6a2 2 0 0 1 2-2h12" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M20 4v6h-6" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m20 4-9 9" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M20 14v4a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2v-4" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-swap", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M15 7h6" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m18 4 3 3-3 3" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M9 17H3" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m6 14-3 3 3 3" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M21 7H9a4 4 0 0 0-4 4v6" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-copy", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("rect", { x: "9", y: "9", width: "11", height: "11", rx: "2" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("rect", { x: "4", y: "4", width: "11", height: "11", rx: "2" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("symbol", { id: "i-check", viewBox: "0 0 24 24", children: /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m5 12 4.2 4.2L19 6.5" }) }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-archive", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M4 7h16" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M6 7v11a2 2 0 0 0 2 2h8a2 2 0 0 0 2-2V7" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M9 11h6" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M10 3h4l1 2H9l1-2z" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-square-plus", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("rect", { x: "3", y: "3", width: "18", height: "18", rx: "3" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M12 8v8M8 12h8" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-info", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("circle", { cx: "12", cy: "12", r: "9" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M12 10v6" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M12 7h.01" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-refresh", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M21 12a9 9 0 0 1-15.4 6.4" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M3 12A9 9 0 0 1 18.4 5.6" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M3 16v-4h4" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M21 8v4h-4" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-mic", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M12 3a3 3 0 0 1 3 3v6a3 3 0 0 1-6 0V6a3 3 0 0 1 3-3z" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M19 11a7 7 0 0 1-14 0" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M12 18v3" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M8 21h8" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-ellipsis", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("circle", { cx: "5", cy: "12", r: "2" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("circle", { cx: "12", cy: "12", r: "2" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("circle", { cx: "19", cy: "12", r: "2" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-gear", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M12 8a4 4 0 1 1 0 8 4 4 0 0 1 0-8z" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M12 2v3M12 19v3M2 12h3M19 12h3M4.9 4.9l2.2 2.2M16.9 16.9l2.2 2.2M19.1 4.9l-2.2 2.2M7.1 16.9l-2.2 2.2" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-search", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("circle", { cx: "11", cy: "11", r: "6" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m20 20-4.35-4.35" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-pin", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m14 4 6 6" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M11 7l6 6" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m8 10 6 6" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M6 12l6 6" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m11 13-7 7" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("symbol", { id: "i-star", viewBox: "0 0 24 24", children: /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m12 3 2.8 5.7 6.2.9-4.5 4.4 1.1 6.2L12 17.2 6.4 20.2l1.1-6.2L3 9.6l6.2-.9L12 3z" }) })
  ] });
}
function Icon({ name, className }) {
  return /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("svg", { className, "aria-label": name, children: /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("use", { href: `#${name}` }) });
}

// src/ConsoleApp.tsx
var import_jsx_runtime17 = require("react/jsx-runtime");
function normalizeComparableTranscriptText(value) {
  return value.replace(/^\[EVENT via rpc\]\s*/i, "").replace(/\s+/g, " ").trim();
}
function sameUserMessage(entry, candidate) {
  if (!entry || !candidate || entry.kind !== "message" || candidate.kind !== "message") {
    return false;
  }
  if (entry.identity.id !== "user" || candidate.identity.id !== "user") {
    return false;
  }
  return normalizeComparableTranscriptText(entry.text || "") === normalizeComparableTranscriptText(candidate.text || "");
}
function clipTranscriptWindow(entries) {
  const maxEntries = 100;
  return entries.slice(-maxEntries);
}
function hasVisibleConversationContent(entry) {
  if (entry.kind !== "message") {
    return true;
  }
  if (Array.isArray(entry.blocks) && entry.blocks.length > 0) {
    return entry.blocks.some((block) => {
      const record = block;
      const text = [
        typeof record.text === "string" ? record.text : "",
        typeof record.label === "string" ? record.label : "",
        typeof record.result === "string" ? record.result : "",
        typeof record.body === "string" ? record.body : "",
        typeof record.title === "string" ? record.title : ""
      ].join(" ").trim();
      return text.length > 0;
    });
  }
  return Boolean(entry.text && entry.text.trim().length > 0);
}
function richBlockHasVisibleContent(block) {
  if (!block || typeof block !== "object") {
    return false;
  }
  const record = block;
  const scalarText = [
    typeof record.text === "string" ? record.text : "",
    typeof record.label === "string" ? record.label : "",
    typeof record.result === "string" ? record.result : "",
    typeof record.body === "string" ? record.body : "",
    typeof record.title === "string" ? record.title : "",
    typeof record.name === "string" ? record.name : ""
  ].join(" ").trim();
  if (scalarText.length > 0) {
    return true;
  }
  if (Array.isArray(record.headers) && record.headers.some((value) => String(value || "").trim().length > 0)) {
    return true;
  }
  if (Array.isArray(record.rows) && record.rows.some((row) => Array.isArray(row) && row.some((value) => String(value || "").trim().length > 0))) {
    return true;
  }
  return false;
}
function sanitizeConversationEntries(entries) {
  const sanitized = [];
  for (const entry of entries) {
    if (entry.kind !== "message") {
      sanitized.push(entry);
      continue;
    }
    if (entry.variant === "rich" && Array.isArray(entry.blocks)) {
      const blocks = entry.blocks.filter(richBlockHasVisibleContent);
      if (!blocks.length) {
        continue;
      }
      sanitized.push({ ...entry, blocks });
      continue;
    }
    if (hasVisibleConversationContent(entry)) {
      sanitized.push(entry);
    }
  }
  return sanitized;
}
var DEFAULT_APPROVER_ID = "console-ops-lead";
var REFRESH_TRIGGER_EVENTS = /* @__PURE__ */ new Set([
  "interaction_complete",
  "interaction_failed",
  "state_changed",
  "member_ready",
  "member_retired",
  "gating_decision",
  "route_changed"
]);
var PANEL_ROUTABLE_EVENTS = /* @__PURE__ */ new Set([
  "interaction_started",
  "interaction_complete",
  "interaction_failed",
  "text_delta",
  "text_complete",
  "tool_call_requested",
  "tool_call",
  "tool_result_received",
  "tool_execution_started",
  "tool_execution_completed",
  "run_started",
  "run_completed",
  "run_failed"
]);
var HISTORY_REFRESH_EVENTS = /* @__PURE__ */ new Set([
  "interaction_complete",
  "interaction_failed",
  "run_completed",
  "run_failed"
]);
function ConsoleApp({ baseUrl }) {
  const [experience, setExperience] = import_react6.default.useState(null);
  const [agents, setAgents] = import_react6.default.useState([]);
  const [draftByKey, setDraftByKey] = import_react6.default.useState({});
  const [sendingPanels, setSendingPanels] = import_react6.default.useState(/* @__PURE__ */ new Set());
  const [pinnedAgentIds, setPinnedAgentIds] = import_react6.default.useState(/* @__PURE__ */ new Set());
  const [inspectByIdentity, setInspectByIdentity] = import_react6.default.useState({});
  const [routingData, setRoutingData] = import_react6.default.useState({ routes: [], deliveries: [] });
  const [gatingData, setGatingData] = import_react6.default.useState({ pending: [], audit: [] });
  const [activeActivityPresetId, setActiveActivityPresetId] = import_react6.default.useState("all");
  const [loading, setLoading] = import_react6.default.useState(true);
  const [error, setError] = import_react6.default.useState("");
  const [, setRenderTick] = import_react6.default.useState(0);
  const forceRender = import_react6.default.useCallback(() => setRenderTick((n) => n + 1), []);
  const transcriptRef = import_react6.default.useRef({});
  const pendingUserRef = import_react6.default.useRef({});
  const liveFramesRef = import_react6.default.useRef({});
  const activityRef = import_react6.default.useRef([]);
  const phaseRef = import_react6.default.useRef({});
  const refreshInFlightRef = import_react6.default.useRef(/* @__PURE__ */ new Set());
  const experienceTimerRef = import_react6.default.useRef(null);
  const initialTargetOpened = import_react6.default.useRef(false);
  const phaseValueByKey = import_react6.default.useRef({});
  const phaseSinceByKey = import_react6.default.useRef({});
  const phaseTimerByKey = import_react6.default.useRef({});
  const historyLoadedByKey = import_react6.default.useRef({});
  const panelBaselineEntriesByKey = import_react6.default.useRef({});
  const identityPanelCountByIdentity = import_react6.default.useRef({});
  const previousIdentityPanelCountByIdentity = import_react6.default.useRef({});
  const agentsRef = import_react6.default.useRef([]);
  const dockRef = import_react6.default.useRef({ panels: [] });
  import_react6.default.useEffect(() => {
    agentsRef.current = agents;
  }, [agents]);
  const dock = useConsoleDockController({
    createPanelState: ({ target }) => ({
      id: `panel-${crypto.randomUUID()}`,
      target: target || null,
      mode: "console"
    })
  });
  import_react6.default.useEffect(() => {
    dockRef.current = {
      panels: dock.viewState.panels.map((panel) => ({
        id: panel.id,
        target: panel.target
      }))
    };
  }, [dock.viewState.panels]);
  import_react6.default.useEffect(() => {
    const counts = {};
    for (const panel of dock.viewState.panels) {
      const target = panel.target;
      if (!target || target.kind !== "agent-chat") continue;
      const key = target.identity || target.memberId;
      counts[key] = (counts[key] || 0) + 1;
    }
    identityPanelCountByIdentity.current = counts;
  }, [dock.viewState.panels]);
  import_react6.default.useEffect(() => {
    const activePanelKeys = new Set(
      dock.viewState.panels.map((panel) => {
        if (!panel.target) return null;
        return buildPanelConversationKey(panel.id, panel.target);
      }).filter((value) => Boolean(value))
    );
    const pruneRef = (record) => {
      for (const key of Object.keys(record)) {
        if (!activePanelKeys.has(key)) {
          delete record[key];
        }
      }
    };
    pruneRef(transcriptRef.current);
    pruneRef(pendingUserRef.current);
    pruneRef(liveFramesRef.current);
    pruneRef(phaseRef.current);
    pruneRef(historyLoadedByKey.current);
    pruneRef(panelBaselineEntriesByKey.current);
    pruneRef(phaseValueByKey.current);
    pruneRef(phaseSinceByKey.current);
    for (const key of Object.keys(phaseTimerByKey.current)) {
      if (!activePanelKeys.has(key)) {
        window.clearTimeout(phaseTimerByKey.current[key]);
        delete phaseTimerByKey.current[key];
      }
    }
    setDraftByKey((current) => {
      let changed = false;
      const next = {};
      for (const [key, value] of Object.entries(current)) {
        if (activePanelKeys.has(key)) {
          next[key] = value;
        } else {
          changed = true;
        }
      }
      return changed ? next : current;
    });
  }, [dock.viewState.panels]);
  import_react6.default.useEffect(() => {
    const previousCounts = previousIdentityPanelCountByIdentity.current;
    const nextCounts = identityPanelCountByIdentity.current;
    for (const panel of dock.viewState.panels) {
      const target = panel.target;
      if (!target || target.kind !== "agent-chat") continue;
      const identityKey = target.identity || target.memberId;
      const nextCount = nextCounts[identityKey] || 0;
      const previousCount = previousCounts[identityKey] || 0;
      if (nextCount > 1 && nextCount > previousCount) {
        const siblingPanels = dock.viewState.panels.filter((candidate) => {
          const candidateTarget = candidate.target;
          return candidateTarget?.kind === "agent-chat" && (candidateTarget.identity || candidateTarget.memberId) === identityKey;
        });
        const seedTranscript = siblingPanels.map((candidate) => transcriptRef.current[buildPanelConversationKey(candidate.id, candidate.target)]).find((entries) => Array.isArray(entries) && entries.length > 0);
        if (seedTranscript?.length) {
          for (const sibling of siblingPanels) {
            const siblingTarget = sibling.target;
            const siblingKey = buildPanelConversationKey(sibling.id, siblingTarget);
            panelBaselineEntriesByKey.current[siblingKey] = seedTranscript;
            if (!transcriptRef.current[siblingKey]?.length) {
              transcriptRef.current[siblingKey] = seedTranscript;
            }
          }
        }
      }
    }
    previousIdentityPanelCountByIdentity.current = { ...nextCounts };
  }, [dock.viewState.panels]);
  const loadExperience = import_react6.default.useCallback(async () => {
    const [experienceJson, modulesJson] = await Promise.all([
      fetchJson(baseUrl, "/console/experience"),
      fetchJson(baseUrl, "/console/modules")
    ]);
    const loadedModules = Array.isArray(modulesJson.modules) ? modulesJson.modules.map((moduleId) => String(moduleId)) : [];
    const nextAgents = normalizeAgents(experienceJson, loadedModules);
    setExperience(experienceJson);
    setAgents(nextAgents);
    setActiveActivityPresetId((current) => current || experienceJson.activity_feed?.active_preset_id || "all");
  }, [baseUrl]);
  import_react6.default.useEffect(() => {
    let mounted = true;
    setLoading(true);
    setError("");
    void loadExperience().catch((loadError) => {
      if (mounted) setError(errorMessage(loadError));
    }).finally(() => {
      if (mounted) setLoading(false);
    });
    return () => {
      mounted = false;
    };
  }, [loadExperience]);
  import_react6.default.useEffect(() => {
    if (initialTargetOpened.current || dock.focusedTarget || agents.length === 0) return;
    const firstAddressable = agents.find((agent) => agent.addressable || agent.affordances?.can_send_message) || agents[0];
    if (!firstAddressable) return;
    initialTargetOpened.current = true;
    dock.openTarget(buildDockTarget(firstAddressable), "replace_focused");
  }, [agents, dock]);
  const refreshPanelData = import_react6.default.useCallback(async () => {
    const openPanels = dockRef.current.panels.map((p) => p.target).filter(Boolean);
    const inspectTargets = openPanels.filter((t) => t.kind === "identity-inspect");
    const hasRouting = openPanels.some((t) => t.kind === "routing");
    const hasGating = openPanels.some((t) => t.kind === "gating");
    if (inspectTargets.length) {
      const entries = await Promise.all(
        inspectTargets.map(async (target) => {
          const result = await callConsoleRpc(baseUrl, "mobkit/inspect_identity", { identity: target.identity });
          return [target.identity, result];
        })
      );
      setInspectByIdentity((current) => ({ ...current, ...Object.fromEntries(entries) }));
    }
    if (hasRouting) {
      const [routesResponse, historyResponse] = await Promise.all([
        callConsoleRpc(baseUrl, "mobkit/routing/routes/list", {}),
        callConsoleRpc(baseUrl, "mobkit/delivery/history", {})
      ]);
      setRoutingData(buildRoutingSectionView({ routesResponse, historyResponse }));
    }
    if (hasGating) {
      const [pendingResponse, auditResponse] = await Promise.all([
        callConsoleRpc(baseUrl, "mobkit/gating/pending", {}),
        callConsoleRpc(baseUrl, "mobkit/gating/audit", { limit: 50 })
      ]);
      setGatingData({
        pending: Array.isArray(pendingResponse.pending) ? pendingResponse.pending : [],
        audit: Array.isArray(auditResponse.entries) ? auditResponse.entries : []
      });
    }
  }, [baseUrl]);
  import_react6.default.useEffect(() => {
    void refreshPanelData().catch(() => {
    });
  }, [dock.viewState.panels, refreshPanelData]);
  const scheduleExperienceRefresh = import_react6.default.useCallback(() => {
    if (experienceTimerRef.current !== null) return;
    experienceTimerRef.current = window.setTimeout(async () => {
      experienceTimerRef.current = null;
      await loadExperience().catch(() => {
      });
      await refreshPanelData().catch(() => {
      });
    }, 500);
  }, [loadExperience, refreshPanelData]);
  const scheduleHistoryRefresh = import_react6.default.useCallback((identity) => {
    if (refreshInFlightRef.current.has(identity)) return;
    refreshInFlightRef.current.add(identity);
    setTimeout(async () => {
      try {
        for (const panel of dockRef.current.panels) {
          const target = panel.target;
          if (!target || target.kind !== "agent-chat") continue;
          if ((target.identity || target.memberId) !== identity) continue;
          const panelKey = buildPanelConversationKey(panel.id, target);
          const agent = agentsRef.current.find((c) => c.member_id === target.memberId) || null;
          const frames = await queryEvents(baseUrl, {
            memberId: target.memberId,
            ...target.identity ? { identity: target.identity } : {}
          }, 400);
          const mapped = mapFramesToTimelineEntries(agent, frames, {
            renderInteractionStartsAsUser: true,
            renderTextDeltas: false
          });
          const persistedTexts = new Set(
            mapped.filter((e) => e.kind === "message" && e.identity.id === "user").map((e) => normalizeComparableTranscriptText(e.text?.trim() || "")).filter(Boolean)
          );
          const pending = pendingUserRef.current[panelKey];
          if (pending?.kind === "message") {
            const pendingText = normalizeComparableTranscriptText(pending.text?.trim() || "");
            if (pendingText && persistedTexts.has(pendingText)) {
              pendingUserRef.current[panelKey] = null;
            }
          }
          const existingOptimistic = (transcriptRef.current[panelKey] || []).filter((entry) => {
            if (entry.kind !== "message" || entry.identity.id !== "user" || !String(entry.id).startsWith("user:")) return false;
            const text = normalizeComparableTranscriptText(entry.text?.trim() || "");
            return text && !persistedTexts.has(text);
          });
          transcriptRef.current[panelKey] = clipTranscriptWindow(
            sortConversationTimelineEntries([...mapped, ...existingOptimistic])
          );
          liveFramesRef.current[panelKey] = [];
          phaseRef.current[panelKey] = null;
        }
        forceRender();
      } finally {
        refreshInFlightRef.current.delete(identity);
      }
    }, 200);
  }, [baseUrl, forceRender]);
  import_react6.default.useEffect(() => {
    for (const panel of dock.viewState.panels) {
      const target = panel.target;
      if (!target || target.kind !== "agent-chat") continue;
      const panelKey = buildPanelConversationKey(panel.id, target);
      if (historyLoadedByKey.current[panelKey]) continue;
      historyLoadedByKey.current[panelKey] = true;
      void (async () => {
        try {
          const agent = agentsRef.current.find((c) => c.member_id === target.memberId) || null;
          const frames = await queryEvents(baseUrl, {
            memberId: target.memberId,
            ...target.identity ? { identity: target.identity } : {}
          }, 400);
          const mapped = mapFramesToTimelineEntries(agent, frames, {
            renderInteractionStartsAsUser: true,
            renderTextDeltas: false
          });
          transcriptRef.current[panelKey] = clipTranscriptWindow(mapped);
          if (!liveFramesRef.current[panelKey]) liveFramesRef.current[panelKey] = [];
          forceRender();
        } catch {
          historyLoadedByKey.current[panelKey] = false;
        }
      })();
    }
  }, [baseUrl, dock.viewState.panels, forceRender]);
  import_react6.default.useEffect(() => {
    void queryEvents(baseUrl, {}, 80).then((frames) => {
      activityRef.current = dedupeFrames(frames).slice(-80).reverse();
      forceRender();
    }).catch(() => {
    });
    const unsubscribe = subscribeConsoleEvents(baseUrl, "/console/events/stream", (frame) => {
      activityRef.current = [frame, ...activityRef.current].slice(0, 200);
      const identity = frame.identity?.trim();
      if (PANEL_ROUTABLE_EVENTS.has(frame.event) && identity && identity !== "_system") {
        for (const panel of dockRef.current.panels) {
          const target = panel.target;
          if (!target || target.kind !== "agent-chat") continue;
          const panelIdentity = target.identity || target.memberId;
          if (panelIdentity !== identity) continue;
          const panelKey = buildPanelConversationKey(panel.id, target);
          if (!liveFramesRef.current[panelKey]) liveFramesRef.current[panelKey] = [];
          liveFramesRef.current[panelKey] = dedupeFrames([
            ...liveFramesRef.current[panelKey],
            frame
          ]);
          updatePanelPhaseFromFrame(panelKey, frame);
        }
      }
      forceRender();
      if (HISTORY_REFRESH_EVENTS.has(frame.event) && identity && identity !== "_system") {
        scheduleHistoryRefresh(identity);
      }
      if (REFRESH_TRIGGER_EVENTS.has(frame.event)) {
        scheduleExperienceRefresh();
      }
    });
    return () => {
      unsubscribe();
    };
  }, [baseUrl, forceRender, scheduleHistoryRefresh, scheduleExperienceRefresh]);
  import_react6.default.useEffect(() => {
    return () => {
      for (const timer of Object.values(phaseTimerByKey.current)) {
        window.clearTimeout(timer);
      }
      if (experienceTimerRef.current !== null) {
        window.clearTimeout(experienceTimerRef.current);
      }
    };
  }, []);
  function dedupeFrames(frames) {
    const byId = /* @__PURE__ */ new Map();
    const ordered = [];
    for (const frame of frames) {
      const key = frame.id || `${frame.event}:${frame.timestampMs || 0}`;
      if (byId.has(key)) continue;
      byId.set(key, frame);
      ordered.push(frame);
    }
    return ordered;
  }
  function clearPhaseTimer(panelKey) {
    const timer = phaseTimerByKey.current[panelKey];
    if (timer !== void 0) {
      window.clearTimeout(timer);
      delete phaseTimerByKey.current[panelKey];
    }
  }
  function commitPanelPhase(panelKey, phase) {
    clearPhaseTimer(panelKey);
    phaseValueByKey.current[panelKey] = phase;
    phaseSinceByKey.current[panelKey] = Date.now();
    phaseRef.current[panelKey] = phase;
  }
  function schedulePanelPhase(panelKey, phase, delayMs) {
    clearPhaseTimer(panelKey);
    phaseTimerByKey.current[panelKey] = window.setTimeout(() => {
      delete phaseTimerByKey.current[panelKey];
      phaseValueByKey.current[panelKey] = phase;
      phaseSinceByKey.current[panelKey] = Date.now();
      phaseRef.current[panelKey] = phase;
      forceRender();
    }, delayMs);
  }
  function updatePanelPhaseFromFrame(panelKey, frame) {
    const currentPhase = phaseValueByKey.current[panelKey] ?? null;
    const elapsedMs = Date.now() - (phaseSinceByKey.current[panelKey] ?? 0);
    switch (frame.event) {
      case "interaction_started":
        commitPanelPhase(panelKey, "waiting");
        break;
      case "tool_call_requested":
      case "tool_call":
      case "tool_execution_started":
      case "tool_result_received":
      case "tool_execution_completed":
        if (currentPhase === "waiting" && elapsedMs < 300) {
          schedulePanelPhase(panelKey, "tool-executing", 300 - elapsedMs);
          break;
        }
        commitPanelPhase(panelKey, "tool-executing");
        break;
      case "text_delta": {
        if (currentPhase === "tool-executing") {
          const remainingMs = Math.max(0, 300 - elapsedMs);
          if (remainingMs > 0) {
            schedulePanelPhase(panelKey, "generating", remainingMs);
            break;
          }
        }
        if (currentPhase === "waiting" && elapsedMs < 300) {
          schedulePanelPhase(panelKey, "generating", 300 - elapsedMs);
          break;
        }
        commitPanelPhase(panelKey, "generating");
        break;
      }
      case "interaction_complete":
      case "interaction_failed":
      case "run_completed":
      case "run_failed":
        commitPanelPhase(panelKey, null);
        break;
      default:
        break;
    }
  }
  function onSelectAgent(_block, _section, item) {
    const agent = agents.find((candidate) => candidate.member_id === item.id);
    if (agent) {
      dock.openTarget(buildDockTarget(agent), "replace_focused");
    }
  }
  async function onSendMessage(panelId, target) {
    if (!target || target.kind !== "agent-chat") return;
    const panelKey = buildPanelConversationKey(panelId, target);
    const text = (draftByKey[panelKey] || "").trim();
    if (!text) return;
    const userEntry = createUserEntry(text);
    setDraftByKey((current) => ({ ...current, [panelKey]: "" }));
    setSendingPanels((current) => new Set(current).add(panelKey));
    transcriptRef.current[panelKey] = sortConversationTimelineEntries([
      ...transcriptRef.current[panelKey] || [],
      userEntry
    ]);
    pendingUserRef.current[panelKey] = userEntry;
    phaseRef.current[panelKey] = "waiting";
    if (!liveFramesRef.current[panelKey]) liveFramesRef.current[panelKey] = [];
    forceRender();
    try {
      const identity = target.identity?.trim();
      if (identity) {
        await sendInteract(baseUrl, identity, text, `console:${panelId}`);
      } else {
        await sendMessage(baseUrl, target.memberId, text);
      }
    } catch (submitError) {
      setError(errorMessage(submitError));
      transcriptRef.current[panelKey] = (transcriptRef.current[panelKey] || []).filter((e) => e.id !== userEntry.id);
      pendingUserRef.current[panelKey] = null;
      phaseRef.current[panelKey] = null;
      forceRender();
    } finally {
      setSendingPanels((current) => {
        const next = new Set(current);
        next.delete(panelKey);
        return next;
      });
    }
  }
  async function onLifecycleAction(identity, method) {
    await callConsoleRpc(baseUrl, method, { identity });
    await loadExperience();
  }
  async function onGatingDecision(pendingId, decision) {
    await callConsoleRpc(baseUrl, "mobkit/gating/decide", {
      pending_id: pendingId,
      approver_id: DEFAULT_APPROVER_ID,
      decision,
      reason: `console_${decision}`
    });
    const [pendingResponse, auditResponse] = await Promise.all([
      callConsoleRpc(baseUrl, "mobkit/gating/pending", {}),
      callConsoleRpc(baseUrl, "mobkit/gating/audit", { limit: 50 })
    ]);
    setGatingData({
      pending: Array.isArray(pendingResponse.pending) ? pendingResponse.pending : [],
      audit: Array.isArray(auditResponse.entries) ? auditResponse.entries : []
    });
  }
  const SIDEBAR_MIN = 180;
  const SIDEBAR_MAX = 420;
  function handleSidebarResize(event) {
    event.preventDefault();
    const startX = event.clientX;
    const root = event.currentTarget.closest("[data-console-workbench]");
    if (!root) return;
    const startWidth = parseInt(getComputedStyle(root).getPropertyValue("--cc-workbench-sidebar-width") || "260", 10) || 260;
    const handle = event.currentTarget;
    if ("setPointerCapture" in handle) handle.setPointerCapture(event.pointerId);
    document.documentElement.setAttribute("data-cc-resizing", "true");
    function onPointerMove(e) {
      const next = Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, startWidth + (e.clientX - startX)));
      root.style.setProperty("--cc-workbench-sidebar-width", `${next}px`);
    }
    function cleanup() {
      document.documentElement.removeAttribute("data-cc-resizing");
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", cleanup);
      window.removeEventListener("pointercancel", cleanup);
      if ("hasPointerCapture" in handle && handle.hasPointerCapture(event.pointerId)) {
        handle.releasePointerCapture(event.pointerId);
      }
    }
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", cleanup);
    window.addEventListener("pointercancel", cleanup);
  }
  const ACTIVITY_MIN = 200;
  const ACTIVITY_MAX = 480;
  function handleActivityResize(event) {
    event.preventDefault();
    const startX = event.clientX;
    const root = event.currentTarget.closest("[data-console-workbench]");
    if (!root) return;
    const startWidth = parseInt(getComputedStyle(root).getPropertyValue("--cc-workbench-activity-width") || "280", 10) || 280;
    const handle = event.currentTarget;
    if ("setPointerCapture" in handle) handle.setPointerCapture(event.pointerId);
    document.documentElement.setAttribute("data-cc-resizing", "true");
    function onPointerMove(e) {
      const next = Math.min(ACTIVITY_MAX, Math.max(ACTIVITY_MIN, startWidth - (e.clientX - startX)));
      root.style.setProperty("--cc-workbench-activity-width", `${next}px`);
    }
    function cleanup() {
      document.documentElement.removeAttribute("data-cc-resizing");
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", cleanup);
      window.removeEventListener("pointercancel", cleanup);
      if ("hasPointerCapture" in handle && handle.hasPointerCapture(event.pointerId)) {
        handle.releasePointerCapture(event.pointerId);
      }
    }
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", cleanup);
    window.addEventListener("pointercancel", cleanup);
  }
  if (loading) {
    return /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("div", { "data-testid": "console-loading", children: "Loading console..." });
  }
  if (error) {
    return /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("div", { "data-testid": "console-error", children: error });
  }
  const focusedMemberId = dock.focusedTarget?.kind === "agent-chat" ? dock.focusedTarget.memberId : "";
  const sidebarVS = buildSidebarViewState({ agents, selectedMemberId: focusedMemberId, pinnedAgentIds });
  const activityVS = buildActivityRailViewState({
    agents,
    eventFrames: activityRef.current,
    filterPresets: experience?.activity_feed?.filter_presets,
    activePresetId: activeActivityPresetId
  });
  function renderChatPanel(panel) {
    const target = panel.target;
    if (!target || target.kind !== "agent-chat") return null;
    const panelKey = buildPanelConversationKey(panel.id, target);
    const agent = agents.find((candidate) => candidate.member_id === target.memberId) || null;
    const persistedEntries = transcriptRef.current[panelKey] || [];
    const latestPersistedAt = persistedEntries.reduce((latest, entry) => {
      const parsed = Date.parse(String(entry.createdAt || ""));
      return Number.isFinite(parsed) ? Math.max(latest, parsed) : latest;
    }, Number.NEGATIVE_INFINITY);
    const combinedFrames = liveFramesRef.current[panelKey] || [];
    const liveFrames = Number.isFinite(latestPersistedAt) ? combinedFrames.filter((frame) => typeof frame.timestampMs !== "number" || frame.timestampMs > latestPersistedAt) : combinedFrames;
    const pendingUserEntry = pendingUserRef.current[panelKey];
    const baseEntries = [
      ...persistedEntries,
      ...mapFramesToTimelineEntries(agent, liveFrames, {
        renderInteractionStartsAsUser: false,
        renderTextDeltas: false,
        suppressEmbeddedRunStartedPrompt: true
      })
    ];
    const pendingAlreadyMaterialized = pendingUserEntry ? baseEntries.some((entry) => sameUserMessage(entry, pendingUserEntry)) : false;
    const entries = sanitizeConversationEntries([
      ...!pendingAlreadyMaterialized && pendingUserEntry ? [pendingUserEntry] : [],
      ...baseEntries
    ]);
    const conversation = buildConversationViewState({
      memberId: target.memberId,
      agentLabel: target.title,
      agent,
      entries
    });
    const draft = draftByKey[panelKey] || "";
    const isSending = sendingPanels.has(panelKey);
    const phase = phaseRef.current[panelKey] ?? agent?.response_phase ?? null;
    const quickPrompts = buildQuickPromptSuggestions(agent).map((suggestion) => ({
      id: suggestion.id,
      kind: "pill",
      label: suggestion.label,
      iconName: suggestion.iconName || "i-bolt"
    }));
    const footerLeftItems = [
      { id: "target", kind: "sub-pill", label: `To: ${target.title}`, iconName: "i-team" },
      { id: "identity", kind: "sub-pill", label: target.identity || target.memberId, iconName: "i-terminal" }
    ];
    const footerRightItems = [
      ...agent?.profile ? [{ id: "profile", kind: "sub-pill", label: agent.profile }] : [],
      ...phase ? [{ id: "phase", kind: "sub-pill", label: phase, iconName: "i-bolt" }] : [],
      { id: "state", kind: "sub-pill", label: agent?.state || "unknown", iconName: "i-dot" }
    ];
    return /* @__PURE__ */ (0, import_jsx_runtime17.jsx)(
      "div",
      {
        className: "console-panel console-panel--chat",
        "data-panel-id": panel.id,
        "data-panel-key": panelKey,
        "data-testid": `chat-panel:${target.identity || target.memberId}:${panel.id}`,
        children: /* @__PURE__ */ (0, import_jsx_runtime17.jsx)(
          ConversationPane,
          {
            viewState: conversation,
            Icon,
            onApplySuggestion: (value) => setDraftByKey((current) => ({ ...current, [panelKey]: value })),
            footer: /* @__PURE__ */ (0, import_jsx_runtime17.jsx)(
              ConsoleComposer,
              {
                Icon,
                inputId: `composer-input:${panel.id}`,
                shellId: `composer-shell:${panel.id}`,
                submitButtonId: `composer-submit:${panel.id}`,
                viewState: {
                  value: draft,
                  disabled: isSending,
                  placeholder: `Message ${target.title}...`,
                  submitDisabled: !draft.trim() || isSending,
                  submitLabel: `Send to ${target.title}`,
                  mainRowItems: quickPrompts,
                  footerLeftItems,
                  footerRightItems
                },
                getToolbarButtonProps: ({ zone, item }) => {
                  const buttonProps = {
                    "data-testid": `composer-toolbar:${panel.id}:${zone}:${item.id}`
                  };
                  if (zone === "main") {
                    const suggestion = buildQuickPromptSuggestions(agent).find((candidate) => candidate.id === item.id);
                    if (suggestion) {
                      buttonProps.onClick = () => setDraftByKey((current) => ({ ...current, [panelKey]: suggestion.value }));
                    }
                  }
                  return buttonProps;
                },
                onChange: (value) => setDraftByKey((current) => ({ ...current, [panelKey]: value })),
                onSubmit: () => void onSendMessage(panel.id, target),
                onKeyDown: (event) => {
                  if (event.key === "Enter" && !event.shiftKey) {
                    event.preventDefault();
                    void onSendMessage(panel.id, target);
                  }
                }
              }
            )
          }
        )
      }
    );
  }
  function renderInspectPanel(target) {
    const inspect = inspectByIdentity[target.identity];
    return /* @__PURE__ */ (0, import_jsx_runtime17.jsxs)("div", { className: "console-panel", "data-testid": `inspect-panel:${target.identity}`, children: [
      /* @__PURE__ */ (0, import_jsx_runtime17.jsxs)("div", { className: "console-panel__header", children: [
        /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("h3", { children: target.identity }),
        /* @__PURE__ */ (0, import_jsx_runtime17.jsxs)("div", { className: "console-panel__actions", children: [
          /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("button", { "data-testid": `inspect-action:${target.identity}:respawn`, type: "button", onClick: () => void onLifecycleAction(target.identity, "mobkit/respawn"), children: "Respawn" }),
          /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("button", { "data-testid": `inspect-action:${target.identity}:reset`, type: "button", onClick: () => void onLifecycleAction(target.identity, "mobkit/reset"), children: "Reset" }),
          /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("button", { "data-testid": `inspect-action:${target.identity}:retire`, type: "button", onClick: () => void onLifecycleAction(target.identity, "mobkit/retire"), children: "Retire" })
        ] })
      ] }),
      !inspect ? /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("p", { children: "Loading identity details\u2026" }) : /* @__PURE__ */ (0, import_jsx_runtime17.jsxs)("dl", { className: "console-panel__grid", children: [
        /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("dt", { children: "State" }),
        /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("dd", { children: inspect.state }),
        /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("dt", { children: "Profile" }),
        /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("dd", { children: inspect.profile || "n/a" }),
        /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("dt", { children: "Addressability" }),
        /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("dd", { children: inspect.addressability }),
        /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("dt", { children: "Generation" }),
        /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("dd", { children: inspect.continuity?.generation ?? "n/a" }),
        /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("dt", { children: "Checkpoint" }),
        /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("dd", { children: inspect.continuity?.checkpoint_version ?? "n/a" }),
        /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("dt", { children: "Session" }),
        /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("dd", { children: inspect.continuity?.session_id || "n/a" }),
        /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("dt", { children: "Runtime" }),
        /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("dd", { children: inspect.continuity?.agent_runtime_id || "n/a" }),
        /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("dt", { children: "Lease Healthy" }),
        /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("dd", { children: String(inspect.lease_healthy ?? inspect.lease?.healthy ?? false) }),
        /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("dt", { children: "Peers" }),
        /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("dd", { children: inspect.topology_peers?.join(", ") || "none" }),
        /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("dt", { children: "Output Preview" }),
        /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("dd", { children: inspect.output_preview || "n/a" })
      ] })
    ] });
  }
  function renderRoutingPanel() {
    return /* @__PURE__ */ (0, import_jsx_runtime17.jsxs)("div", { className: "console-panel", "data-testid": "routing-panel", children: [
      /* @__PURE__ */ (0, import_jsx_runtime17.jsxs)("div", { className: "console-panel__section", children: [
        /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("h3", { children: "Routes" }),
        /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("ul", { className: "console-panel__list", children: routingData.routes.map((route) => /* @__PURE__ */ (0, import_jsx_runtime17.jsxs)("li", { "data-testid": `routing-route:${route.route_key}`, children: [
          /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("strong", { children: route.route_key }),
          " \u2192 ",
          route.recipient,
          " via ",
          route.sink
        ] }, route.route_key)) })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime17.jsxs)("div", { className: "console-panel__section", children: [
        /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("h3", { children: "Deliveries" }),
        /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("ul", { className: "console-panel__list", children: routingData.deliveries.map((delivery) => /* @__PURE__ */ (0, import_jsx_runtime17.jsxs)("li", { "data-testid": `routing-delivery:${delivery.delivery_id}`, children: [
          /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("strong", { children: delivery.delivery_id }),
          " \xB7 ",
          delivery.status,
          " \xB7 ",
          delivery.recipient
        ] }, delivery.delivery_id)) })
      ] })
    ] });
  }
  function renderGatingPanel() {
    return /* @__PURE__ */ (0, import_jsx_runtime17.jsxs)("div", { className: "console-panel", "data-testid": "gating-panel", children: [
      /* @__PURE__ */ (0, import_jsx_runtime17.jsxs)("div", { className: "console-panel__section", children: [
        /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("h3", { children: "Pending" }),
        /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("ul", { className: "console-panel__list", children: gatingData.pending.map((entry, index) => {
          const record = entry;
          const pendingId = String(record.pending_id || `pending-${index}`);
          return /* @__PURE__ */ (0, import_jsx_runtime17.jsxs)("li", { "data-testid": `gating-pending:${pendingId}`, children: [
            /* @__PURE__ */ (0, import_jsx_runtime17.jsxs)("div", { children: [
              /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("strong", { children: String(record.action_id || pendingId) }),
              " \xB7 ",
              String(record.risk_tier || "unknown")
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime17.jsxs)("div", { className: "console-panel__actions", children: [
              /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("button", { "data-testid": `gating-action:${pendingId}:escalate`, type: "button", onClick: () => void onGatingDecision(pendingId, "escalate"), children: "Escalate" }),
              /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("button", { "data-testid": `gating-action:${pendingId}:approve`, type: "button", onClick: () => void onGatingDecision(pendingId, "approve"), children: "Approve" }),
              /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("button", { "data-testid": `gating-action:${pendingId}:reject`, type: "button", onClick: () => void onGatingDecision(pendingId, "reject"), children: "Reject" })
            ] })
          ] }, pendingId);
        }) })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime17.jsxs)("div", { className: "console-panel__section", children: [
        /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("h3", { children: "Audit" }),
        /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("ul", { className: "console-panel__list", children: gatingData.audit.map((entry, index) => {
          const record = entry;
          return /* @__PURE__ */ (0, import_jsx_runtime17.jsxs)("li", { "data-testid": `gating-audit:${String(record.audit_id || index)}`, children: [
            /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("strong", { children: String(record.event_type || "event") }),
            " \xB7 ",
            String(record.action_id || "unknown")
          ] }, String(record.audit_id || index));
        }) })
      ] })
    ] });
  }
  function renderTopologyPanel(nodes) {
    return /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("div", { className: "console-panel", "data-testid": "topology-panel", children: /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("ul", { className: "console-panel__list", children: nodes.map((node) => /* @__PURE__ */ (0, import_jsx_runtime17.jsxs)("li", { "data-testid": `topology-node:${node.identity || node.label}`, children: [
      /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("strong", { children: node.label || node.identity }),
      /* @__PURE__ */ (0, import_jsx_runtime17.jsxs)("div", { children: [
        node.profile || "unknown",
        " \xB7 ",
        node.state || "unknown"
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime17.jsxs)("div", { children: [
        "Peers: ",
        node.wired_to?.join(", ") || "none"
      ] })
    ] }, node.identity || node.label)) }) });
  }
  function renderHealthPanel(identities) {
    return /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("div", { className: "console-panel", "data-testid": "health-panel", children: /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("ul", { className: "console-panel__list", children: identities.map((row) => /* @__PURE__ */ (0, import_jsx_runtime17.jsxs)("li", { "data-testid": `health-identity:${row.identity}`, children: [
      /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("strong", { children: row.display_name || row.identity }),
      " \xB7 ",
      row.state,
      " \xB7 ",
      row.addressability
    ] }, row.identity)) }) });
  }
  return /* @__PURE__ */ (0, import_jsx_runtime17.jsxs)("div", { className: "cc-theme-scope", "data-cc-theme": "dark", "data-testid": "meerkat-console", children: [
    /* @__PURE__ */ (0, import_jsx_runtime17.jsx)(SpriteSheet, {}),
    /* @__PURE__ */ (0, import_jsx_runtime17.jsx)(
      ConsoleWorkbench,
      {
        launcherResizeHandle: /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("div", { className: "pane-resizer", "aria-hidden": "true", "data-testid": "resize:sidebar", onPointerDown: handleSidebarResize }),
        launcher: /* @__PURE__ */ (0, import_jsx_runtime17.jsx)(
          ConsoleSidebar,
          {
            viewState: sidebarVS,
            Icon,
            getActionButtonProps: (scope) => {
              if (scope.kind === "block") {
                return { "data-testid": `sidebar-action:${scope.action.id}` };
              }
              if (scope.kind === "item") {
                return { "data-testid": `sidebar-item-action:${scope.item.id}:${scope.action.id}` };
              }
              return {};
            },
            onBlockAction: (_block, action) => {
              switch (action.id) {
                case "open_routing":
                  dock.openTarget(buildControlTarget("routing"), "new_tab");
                  break;
                case "open_gating":
                  dock.openTarget(buildControlTarget("gating"), "new_tab");
                  break;
                case "open_topology":
                  dock.openTarget(buildControlTarget("topology"), "new_tab");
                  break;
                case "open_health":
                  dock.openTarget(buildControlTarget("health"), "new_tab");
                  break;
                default:
                  break;
              }
            },
            onSelectItem: onSelectAgent,
            onItemAction: (_block, _section, item, action) => {
              const agent = agents.find((candidate) => candidate.member_id === item.id);
              if (!agent) return;
              if (action.id === "inspect_identity") {
                dock.openTarget(buildInspectTarget(agent), "new_tab");
                return;
              }
              if (action.id === "toggle_pin") {
                setPinnedAgentIds((current) => {
                  const next = new Set(current);
                  if (next.has(item.id)) next.delete(item.id);
                  else next.add(item.id);
                  return next;
                });
              }
            },
            onItemContextMenu: (_block, _section, item, event) => {
              event.preventDefault();
              const agent = agents.find((candidate) => candidate.member_id === item.id);
              if (agent) {
                dock.openTarget(buildInspectTarget(agent), "new_tab");
              }
            }
          }
        ),
        main: /* @__PURE__ */ (0, import_jsx_runtime17.jsx)(
          ConsoleDock,
          {
            viewState: dock.viewState,
            Icon,
            onSelectTab: (tab) => dock.selectTab(tab.id),
            onCloseTab: (tab) => dock.closeTab(tab.id),
            onFocusPanel: (panel) => dock.focusPanel(panel.id),
            onSplitPanel: (panel, direction) => dock.splitPanel(panel.id, direction),
            onClosePanel: (panel) => dock.closePanel(panel.id),
            onResizeSplit: (id, ratio) => dock.resizeSplit(id, ratio),
            onCreateTab: () => dock.createTab(),
            renderPanelBody: (panel) => {
              const target = panel.target;
              if (!target) return /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("div", { className: "console-panel", children: "No panel target" });
              if (target.kind === "agent-chat") return renderChatPanel(panel);
              if (target.kind === "identity-inspect") return renderInspectPanel(target);
              if (target.kind === "routing") return renderRoutingPanel();
              if (target.kind === "gating") return renderGatingPanel();
              if (target.kind === "topology") return renderTopologyPanel(experience?.topology?.live_snapshot?.nodes || []);
              if (target.kind === "health") return renderHealthPanel(experience?.health_overview?.live_snapshot?.identities || []);
              return /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("div", { className: "console-panel", children: "Unsupported panel" });
            }
          }
        ),
        activityRailResizeHandle: /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("div", { className: "pane-resizer pane-resizer--activity", "aria-hidden": "true", "data-testid": "resize:activity", onPointerDown: handleActivityResize }),
        activityRail: /* @__PURE__ */ (0, import_jsx_runtime17.jsx)(
          ConsoleActivityRail,
          {
            viewState: activityVS,
            Icon,
            onTogglePicker: () => {
            },
            onCollapse: () => {
            },
            onPanelAction: (_panelId, actionId) => setActiveActivityPresetId(actionId),
            renderSlotPreview: () => null,
            onSelectItem: (focusId) => {
              const agent = agents.find((candidate) => candidate.member_id === focusId);
              if (agent) {
                dock.openTarget(buildDockTarget(agent), "replace_focused");
              }
            }
          }
        )
      }
    )
  ] });
}

// src/index.tsx
var import_jsx_runtime18 = require("react/jsx-runtime");
function createConsoleApp(target, options = {}) {
  if (!target) {
    throw new Error("target element is required");
  }
  const baseUrl = options.baseUrl || "";
  const root = (0, import_client.createRoot)(target);
  root.render(/* @__PURE__ */ (0, import_jsx_runtime18.jsx)(ConsoleApp, { baseUrl }));
  return {
    unmount() {
      root.unmount();
    }
  };
}
