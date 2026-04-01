import React from "react";

/**
 * Minimal text-fallback icon renderer for shared console components.
 * Renders the icon name as a single-letter label. Replace with an SVG
 * sprite or real icon library when available.
 */
export function Icon({ name, className }: { name: string; className?: string }): React.JSX.Element {
  return (
    <span className={`mc-icon${className ? ` ${className}` : ""}`} aria-label={name} role="img">
      {name.replace(/^i-/, "").charAt(0).toUpperCase()}
    </span>
  );
}
