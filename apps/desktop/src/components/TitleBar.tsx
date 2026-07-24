/**
 * Custom window chrome.
 *
 * The OS decorations are disabled in tauri.conf.json so the window can round its
 * own corners and match the app rather than the desktop. That makes this bar
 * responsible for dragging, double-click maximise, and the window buttons.
 */

import { Icon, Logo } from "./icons";
import { useMaximized, windowActions } from "../lib/window";

export function TitleBar({ subtitle }: { subtitle?: string }) {
  const maximized = useMaximized();

  return (
    <div className="titlebar">
      <div
        className="titlebar-drag"
        data-tauri-drag-region
        onDoubleClick={() => windowActions.toggleMaximize()}
        onPointerDown={(e) => {
          // The drag region attribute alone is unreliable on some Linux window
          // managers, so a primary-button press starts the drag explicitly.
          // Interactive children stop propagation, so this only fires on the bar.
          if (e.button !== 0) return;
          if ((e.target as HTMLElement).closest("button")) return;
          void windowActions.startDragging();
        }}
      >
        <span
          style={{
            color: "var(--text-primary)",
            display: "grid",
            placeItems: "center",
          }}
        >
          <Logo size={18} />
        </span>
        <span className="titlebar-title">Apocrypha</span>
        {subtitle && (
          <>
            <span className="titlebar-sep">/</span>
            <span className="titlebar-title truncate">{subtitle}</span>
          </>
        )}
      </div>

      <div className="win-controls">
        <button
          className="win-btn"
          aria-label="Minimise"
          onClick={() => windowActions.minimize()}
        >
          <Icon.minimize size={14} />
        </button>
        <button
          className="win-btn"
          aria-label={maximized ? "Restore" : "Maximise"}
          onClick={() => windowActions.toggleMaximize()}
        >
          {maximized ? <Icon.restore size={13} /> : <Icon.maximize size={13} />}
        </button>
        <button
          className="win-btn close"
          aria-label="Close"
          onClick={() => windowActions.close()}
        >
          <Icon.close size={14} />
        </button>
      </div>
    </div>
  );
}
