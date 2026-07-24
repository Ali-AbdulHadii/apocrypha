/**
 * Window helpers for the custom title bar.
 *
 * The OS frame is disabled so the app can round its own corners and draw its own
 * controls. That means the app owns dragging, maximise state, and close.
 */

import { useEffect, useState } from "react";
import { subscribe } from "./api";

export async function appWindow() {
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  return getCurrentWindow();
}

/** Tracks whether the window is maximised, so the shell can square its corners. */
export function useMaximized(): boolean {
  const [maximized, setMaximized] = useState(false);

  useEffect(
    () =>
      subscribe(async () => {
        const w = await appWindow();
        const sync = async () => {
          const v = await w.isMaximized();
          setMaximized(v);
          // Mirrored onto the root element so CSS can square the corners
          // without waiting on a React render. The two must never disagree, or
          // the window shows rounded corners over a square backdrop.
          document.documentElement.setAttribute(
            "data-window",
            v ? "maximized" : "normal",
          );
        };
        await sync();
        return w.onResized(sync);
      }),
    [],
  );

  return maximized;
}

export const windowActions = {
  minimize: () => appWindow().then((w) => w.minimize()),
  toggleMaximize: () => appWindow().then((w) => w.toggleMaximize()),
  close: () => appWindow().then((w) => w.close()),
  /** Explicit drag start, used as a fallback where the drag region does not fire. */
  startDragging: () => appWindow().then((w) => w.startDragging()),
};
