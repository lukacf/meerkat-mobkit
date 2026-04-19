import React from "react";

interface TopbarProps {
  mobName: string;
  mobStatus?: string;
  environment?: string;
  theme: "dark" | "light";
  onToggleTheme: () => void;
}

export function Topbar({ mobName, mobStatus = "idle", environment = "dev", theme, onToggleTheme }: TopbarProps): React.JSX.Element {
  return (
    <div className="mobkit-topbar" data-testid="mobkit-topbar">
      <div className="mobkit-topbar__brand">
        <span className="mobkit-topbar__brand-mark" />
        <span>MobKit</span>
      </div>
      <div className="mobkit-topbar__mob">
        <span className="mobkit-topbar__mob-status" title={mobStatus} />
        <span>{mobName}</span>
        <span className="dim">· {mobStatus}</span>
      </div>
      <div className="mobkit-topbar__mob">
        <span>env:</span>
        <span>{environment}</span>
      </div>
      <div className="mobkit-topbar__spacer" />
      <div className="mobkit-topbar__util">
        <button
          type="button"
          onClick={onToggleTheme}
          data-testid="theme-toggle"
          title={`Switch to ${theme === "dark" ? "light" : "dark"} mode`}
        >
          {theme === "dark" ? "☀ light" : "☾ dark"}
        </button>
      </div>
    </div>
  );
}
