import React from "react";

export type ConsoleVariant = "rams" | "terminal" | "graphite";
export type ConsoleTheme = "dark" | "light";

interface TweaksProps {
  variant: ConsoleVariant;
  theme: ConsoleTheme;
  onVariant: (v: ConsoleVariant) => void;
  onTheme: (t: ConsoleTheme) => void;
}

const VARIANT_STORAGE = "mobkit-console-variant";

export function useConsoleVariant(): [ConsoleVariant, (v: ConsoleVariant) => void] {
  const [v, setV] = React.useState<ConsoleVariant>(() => {
    try {
      const stored = localStorage.getItem(VARIANT_STORAGE);
      if (stored === "rams" || stored === "terminal" || stored === "graphite") return stored;
    } catch { /* ignore */ }
    return "rams";
  });
  const set = React.useCallback((next: ConsoleVariant) => {
    setV(next);
    try { localStorage.setItem(VARIANT_STORAGE, next); } catch { /* ignore */ }
  }, []);
  return [v, set];
}

export function Tweaks({ variant, theme, onVariant, onTheme }: TweaksProps): React.JSX.Element {
  const [collapsed, setCollapsed] = React.useState<boolean>(() => {
    try { return localStorage.getItem("mobkit-console-tweaks-collapsed") === "1"; } catch { return false; }
  });
  const toggle = () => {
    setCollapsed((c) => {
      const next = !c;
      try { localStorage.setItem("mobkit-console-tweaks-collapsed", next ? "1" : "0"); } catch { /* ignore */ }
      return next;
    });
  };

  return (
    <div className={`tweaks ${collapsed ? "tweaks--collapsed" : ""}`} data-testid="tweaks-panel">
      <div className="tweaks__title">
        <span>Appearance</span>
        <button className="tweaks__toggle" onClick={toggle} data-testid="tweaks-toggle">
          {collapsed ? "expand ↑" : "collapse ↓"}
        </button>
      </div>
      <div className="tweaks__row">
        <label>Variant</label>
        <div className="tweaks__segs">
          {(["rams", "terminal", "graphite"] as const).map((v) => (
            <button
              key={v}
              className={variant === v ? "is-active" : ""}
              onClick={() => onVariant(v)}
              data-testid={`tweak-variant:${v}`}
            >
              {v}
            </button>
          ))}
        </div>
      </div>
      <div className="tweaks__row">
        <label>Theme</label>
        <div className="tweaks__segs">
          {(["light", "dark"] as const).map((t) => (
            <button
              key={t}
              className={theme === t ? "is-active" : ""}
              onClick={() => onTheme(t)}
              data-testid={`tweak-theme:${t}`}
            >
              {t}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
