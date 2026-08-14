/**
 * Files dragged onto the window.
 *
 * The webview's own drag events are not usable for this. A browser hands over a
 * `File`, which is a handle to bytes it has already sandboxed; the engine needs
 * a path on disk, because analysing an archive is Rust's job and it opens the
 * file itself. Tauri's drag-drop events carry the real paths, so those are what
 * this listens to.
 *
 * Outside the shell — a bare `npm run dev` in a browser — there is nothing to
 * listen to and no path to be had, so the hook stays inert rather than
 * pretending. That is why `over` never goes true there.
 */

import { useEffect, useRef, useState } from "react";
import { IS_TAURI } from "./api";

export interface FileDrop {
  /** True while a drag is over the window and the zone should be showing. */
  over: boolean;
}

/**
 * Watch for files dropped on the window.
 *
 * @param enabled Whether drops should be accepted at all. The caller passes
 *   false when another screen is in front or an install is already running, so
 *   a drop cannot start a second one behind a dialog.
 * @param onDrop Receives every dropped path, in the order the OS gave them.
 *   Deciding what to do with more than one is the caller's business.
 */
export function useFileDrop(
  enabled: boolean,
  onDrop: (paths: string[]) => void,
): FileDrop {
  const [over, setOver] = useState(false);

  // The listener is registered once and lives as long as the component, so it
  // must not close over a stale callback. Re-subscribing on every render of the
  // parent would drop events in the gap between unlisten and listen.
  const latest = useRef(onDrop);
  latest.current = onDrop;

  const active = useRef(enabled);
  active.current = enabled;

  useEffect(() => {
    if (!IS_TAURI) return;

    let stop: (() => void) | null = null;
    let cancelled = false;

    void (async () => {
      const { getCurrentWebview } = await import("@tauri-apps/api/webview");
      const unlisten = await getCurrentWebview().onDragDropEvent((event) => {
        const payload = event.payload;
        if (payload.type === "enter" || payload.type === "over") {
          setOver(active.current);
          return;
        }
        setOver(false);
        if (payload.type === "drop" && active.current) {
          latest.current(payload.paths);
        }
      });

      // Unmounted while the listener was still being registered. Tear it down
      // rather than leaking a subscription that outlives the screen.
      if (cancelled) unlisten();
      else stop = unlisten;
    })();

    return () => {
      cancelled = true;
      stop?.();
      setOver(false);
    };
  }, []);

  // A drag that was in progress when the screen stopped accepting drops should
  // not leave the zone drawn on top of whatever replaced it.
  useEffect(() => {
    if (!enabled) setOver(false);
  }, [enabled]);

  return { over };
}
