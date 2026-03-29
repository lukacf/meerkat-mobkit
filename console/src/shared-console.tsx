import React from "react";

export type SidebarTone = "default" | "muted" | "accent" | "positive" | "negative";

export interface ConsoleSidebarMeta {
  id?: string;
  label: string;
  tone?: SidebarTone;
}

export interface ConsoleSidebarItem {
  id: string;
  title: string;
  subtitle?: string;
  meta?: ConsoleSidebarMeta[];
  selected?: boolean;
}

export interface ConsoleSidebarSection {
  id: string;
  title: string;
  items: ConsoleSidebarItem[];
}

export interface ConsoleSidebarBlock {
  id: string;
  kind: "list";
  title?: string;
  sections: ConsoleSidebarSection[];
}

export interface ConsoleSidebarViewState {
  blocks: ConsoleSidebarBlock[];
}

export type ConversationPresentation = "user" | "participant" | "system";

export interface ConversationIdentity {
  id: string;
  label: string;
  presentation: ConversationPresentation;
}

export interface ConversationEntry {
  id: string;
  identity: ConversationIdentity;
  text: string;
}

export interface ConversationGroup {
  id: string;
  identity: ConversationIdentity;
  entries: ConversationEntry[];
}

export interface ConversationViewState {
  conversationId: string;
  title: string;
  entries: ConversationEntry[];
  groups: ConversationGroup[];
  emptyTitle: string;
  emptySubtitle: string;
}

function toneClass(tone: SidebarTone | undefined): string {
  switch (tone) {
    case "accent":
      return "is-accent";
    case "positive":
      return "is-positive";
    case "negative":
      return "is-negative";
    case "muted":
      return "is-muted";
    default:
      return "";
  }
}

export function groupConversationEntries(entries: ConversationEntry[]): ConversationGroup[] {
  const groups: ConversationGroup[] = [];

  for (const entry of entries) {
    const current = groups[groups.length - 1];
    if (!current || current.identity.id !== entry.identity.id || current.identity.presentation !== entry.identity.presentation) {
      groups.push({
        id: `${entry.identity.id}:${entry.id}`,
        identity: entry.identity,
        entries: [entry],
      });
      continue;
    }
    current.entries.push(entry);
  }

  return groups;
}

export function ConsoleWorkbench({
  sidebar,
  main,
}: {
  sidebar: React.ReactNode;
  main: React.ReactNode;
}): React.JSX.Element {
  return (
    <section className="mc-workbench">
      <aside className="mc-workbench__sidebar">{sidebar}</aside>
      <section className="mc-workbench__main">{main}</section>
    </section>
  );
}

export function ConsoleSidebar({
  viewState,
  onSelectItem,
  getItemButtonProps,
}: {
  viewState: ConsoleSidebarViewState;
  onSelectItem?: (item: ConsoleSidebarItem) => void;
  getItemButtonProps?: (item: ConsoleSidebarItem) => React.ButtonHTMLAttributes<HTMLButtonElement>;
}): React.JSX.Element {
  return (
    <section className="mc-sidebar" data-testid="agent-sidebar">
      {viewState.blocks.map((block) => (
        <div className="mc-sidebar__block" key={block.id}>
          {block.title ? <div className="mc-sidebar__block-title">{block.title}</div> : null}
          <div className="mc-sidebar__sections" data-testid="sidebar-list">
            {block.sections.map((section) => (
              <div className="mc-sidebar__section" key={section.id}>
                <div className="mc-sidebar__section-title">{section.title}</div>
                <div className="mc-sidebar__items">
                  {section.items.map((item) => {
                    const buttonProps = getItemButtonProps?.(item) || {};

                    return (
                      <button
                        {...buttonProps}
                        className={`mc-sidebar__item${item.selected ? " is-selected" : ""}${buttonProps.className ? ` ${buttonProps.className}` : ""}`}
                        key={item.id}
                        type="button"
                        onClick={(event) => {
                          buttonProps.onClick?.(event);
                          if (!event.defaultPrevented) {
                            onSelectItem?.(item);
                          }
                        }}
                      >
                        <span className="mc-sidebar__item-copy">
                          <span className="mc-sidebar__item-title">{item.title}</span>
                          {item.subtitle ? <span className="mc-sidebar__item-subtitle">{item.subtitle}</span> : null}
                        </span>
                        {item.meta?.length ? (
                          <span className="mc-sidebar__item-meta">
                            {item.meta.map((meta) => (
                              <span className={`mc-sidebar__meta ${toneClass(meta.tone)}`.trim()} key={meta.id || meta.label}>
                                {meta.label}
                              </span>
                            ))}
                          </span>
                        ) : null}
                      </button>
                    );
                  })}
                </div>
              </div>
            ))}
          </div>
        </div>
      ))}
    </section>
  );
}

export function ConversationPane({
  viewState,
  footer,
}: {
  viewState: ConversationViewState;
  footer?: React.ReactNode;
}): React.JSX.Element {
  return (
    <section className="mc-conversation" data-testid="chat-inspector">
      <div className="mc-conversation__header">
        <div className="mc-conversation__title">{viewState.title}</div>
      </div>
      <div className="mc-conversation__body">
        {viewState.groups.length === 0 ? (
          <div className="mc-conversation__empty">
            <div className="mc-conversation__empty-title">{viewState.emptyTitle}</div>
            <div className="mc-conversation__empty-subtitle">{viewState.emptySubtitle}</div>
          </div>
        ) : (
          <ul className="mc-conversation__events" data-testid="chat-events">
            {viewState.groups.map((group) => (
              <li className={`mc-conversation__group is-${group.identity.presentation}`} key={group.id}>
                <div className="mc-conversation__group-label">{group.identity.label}</div>
                <div className="mc-conversation__messages">
                  {group.entries.map((entry) => (
                    <div className="mc-conversation__message" key={entry.id}>
                      {entry.text}
                    </div>
                  ))}
                </div>
              </li>
            ))}
          </ul>
        )}
      </div>
      {footer ? <div className="mc-conversation__footer">{footer}</div> : null}
    </section>
  );
}
