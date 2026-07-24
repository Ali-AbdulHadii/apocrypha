/**
 * Window helpers for the custom title bar.
 *
 * The OS frame is disabled so the app can round its own corners and draw its own
 * controls. That means the app owns dragging, maximise state, and close.
 */

import { useEffect, useState } from "react";

export async function appWindow() {
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  return getCurrentWindow();
}

/** Tracks whether the window is maximised, so the shell can square its corners. */
export function useMaximized(): boolean {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let alive = true;

    (async () => {
      const w = await appWindow();
      const sync = async () => {
        const v = await w.isMaximized();
        if (alive) setMaximized(v);
      };
      await sync();
      unlisten = await w.onResized(sync);
    })().catch(() => {
      /* Outside Tauri there is no window to track. */
    });

    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);

  return maximized;
}

export const windowActions = {
  minimize: () => appWindow().then((w) => w.minimize()),
  toggleMaximize: () => appWindow().then((w) => w.toggleMaximize()),
  close: () => appWindow().then((w) => w.close()),
  /** Explicit drag start, used as a fallback where the drag region does not fire. */
  startDragging: () => appWindow().then((w) => w.startDragging()),
};
