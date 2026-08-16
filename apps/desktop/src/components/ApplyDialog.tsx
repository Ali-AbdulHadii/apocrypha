/**
 * Apply progress dialog.
 *
 * The backend streams real counts as it writes, so this shows files and bytes
 * rather than a list of phases with a bar that guesses. It stays open on
 * completion long enough to report what happened, and on failure it keeps the
 * message on screen instead of dropping a toast that scrolls away.
 *
 * Cancelling is offered throughout, because the engine rolls back what it has
 * written when asked to stop. An Apply that could not be interrupted was the
 * thing that made a large deployment feel like a commitment.
 */

import { motion } from "framer-motion";
import { Icon } from "./icons";
import { Spinner } from "./ui";
import {
  api,
  formatBytes,
  type ApplyProgressView,
  type DeployResultView,
  type RollbackView,
} from "../lib/api";
import { supportMailto } from "../lib/support";

export interface ApplyState {
  /** "reverting" and "linking" come from the engine; the rest are local. */
  phase: "starting" | "reverting" | "linking" | "done";
  progress?: ApplyProgressView;
  result?: DeployResultView;
  error?: string;
  cancelled?: boolean;
  rollback?: RollbackView | null;
  /** True once cancelling has been asked for but has not taken effect yet. */
  cancelling?: boolean;
}

function headline(state: ApplyState): string {
  if (state.error) return "Could not apply";
  if (state.cancelled) return "Stopped";
  if (state.phase === "done") return "Mods applied";
  if (state.cancelling) return "Stopping";
  if (state.phase === "reverting") return "Removing the previous set";
  return "Applying mods";
}

export function ApplyDialog({
  state,
  onCancel,
  onClose,
}: {
  state: ApplyState;
  onCancel: () => void;
  onClose: () => void;
}) {
  const finished = state.phase === "done";
  const failed = Boolean(state.error);
  const settled = finished || failed || Boolean(state.cancelled);

  const p = state.progress;
  // Bytes rather than file count: mod archives mix 2 KB scripts with 40 MB
  // textures, so a file-count bar lurches. Falls back to an indeterminate look
  // while the totals are still zero.
  const fraction = p && p.bytesTotal > 0 ? p.bytesDone / p.bytesTotal : 0;
  const known = Boolean(p && p.bytesTotal > 0);

  return (
    <div className="overlay">
      <motion.div
        className="dialog"
        initial={{ opacity: 0, scale: 0.98, y: 8 }}
        animate={{ opacity: 1, scale: 1, y: 0 }}
        exit={{ opacity: 0, scale: 0.99, y: 4 }}
        transition={{ type: "spring", stiffness: 420, damping: 36 }}
        role="dialog"
        aria-modal="true"
        aria-label="Applying mods"
      >
        <div className="dialog-head">
          <div className="row">
            <span
              style={{
                color: failed
                  ? "var(--danger)"
                  : state.cancelled
                    ? "var(--warning)"
                    : finished
                      ? "var(--success)"
                      : "var(--accent)",
                display: "grid",
                placeItems: "center",
              }}
            >
              {failed ? (
                <Icon.warning size={20} />
              ) : state.cancelled ? (
                <Icon.undo size={20} />
              ) : finished ? (
                <Icon.check size={20} />
              ) : (
                <Spinner />
              )}
            </span>
            <span className="dialog-title">{headline(state)}</span>
          </div>
        </div>

        <div className="dialog-body">
          {!failed && (
            <div className={`progress-track ${known || settled ? "" : "indeterminate"}`}>
              <motion.div
                className="progress-fill"
                initial={{ width: 0 }}
                animate={{ width: `${(settled ? 1 : fraction) * 100}%` }}
                transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
              />
            </div>
          )}

          {failed ? (
            <div className="notice">
              <span style={{ flexShrink: 0 }}>
                <Icon.warning />
              </span>
              <div>
                <div className="notice-title">Nothing was changed</div>
                <div className="notice-body">{state.error}</div>
                {/* A failed apply is the one place the app cannot tell someone
                    what to do next: the cause is usually outside it — a
                    permission, a full disk, a game updating underneath. The
                    journal is the evidence anyone helping will ask for, so it
                    is named here rather than left to be discovered. */}
                <div className="card-hint" style={{ marginTop: "var(--sp-2)" }}>
                  The journal under ~/.local/share/apocrypha/ records exactly what
                  was attempted.{" "}
                  <a
                    href={supportMailto("apply failed")}
                    onClick={(e) => {
                      e.preventDefault();
                      void api.openUrl(supportMailto("apply failed"));
                    }}
                  >
                    Get in touch
                  </a>{" "}
                  if this keeps happening.
                </div>
              </div>
            </div>
          ) : state.cancelled ? (
            <div className="stack tight">
              <div className="card-hint">
                {state.rollback && !state.rollback.clean
                  ? `Stopped, but ${state.rollback.skippedModified.length} file(s) were left alone because they changed while Apocrypha was writing.`
                  : "Stopped, and everything written was put back. The game folder is as it was."}
              </div>
            </div>
          ) : finished && state.result ? (
            <div className="stack tight">
              <div className="card-hint">
                {state.result.filesDeployed} files copied into the game folder,
                totalling {formatBytes(state.result.bytes)}.
              </div>
              {/* For a Creation Engine game the files landing is only half of
                  it: a plugin the game's list does not name is installed and
                  ignored. Saying what reached the list is what replaced the
                  notice that used to send people to another tool. */}
              {state.result.pluginList &&
                state.result.pluginList.added.length > 0 && (
                  <div className="card-hint">
                    {state.result.pluginList.added.length === 1
                      ? `${state.result.pluginList.added[0]} was added to the game's plugin list and switched on.`
                      : `${state.result.pluginList.added.length} plugins were added to the game's plugin list and switched on.`}
                  </div>
                )}
              {state.result.pluginList?.problems.map((problem) => (
                <div className="card-hint warn" key={problem}>
                  {problem}
                </div>
              ))}
              <div className="card-hint">
                Use Undo on the bottom bar to remove them again at any time.
              </div>
            </div>
          ) : (
            <div className="stack tight">
              <div className="apply-counts">
                <span>
                  {p ? `${p.filesDone} of ${p.filesTotal} files` : "Preparing"}
                </span>
                <span className="mono">
                  {p && p.bytesTotal > 0
                    ? `${formatBytes(p.bytesDone)} of ${formatBytes(p.bytesTotal)}`
                    : ""}
                </span>
              </div>
              <div className="card-hint truncate" title={p?.current}>
                {state.cancelling
                  ? "Putting back what was already written."
                  : state.phase === "reverting"
                    ? "Taking out the mods that were applied before."
                    : p?.current || "Working out what to write."}
              </div>
            </div>
          )}
        </div>

        <div className="dialog-foot">
          <div style={{ flex: 1 }} />
          {settled ? (
            <button className="btn primary" onClick={onClose}>
              Done
            </button>
          ) : (
            <button className="btn" onClick={onCancel} disabled={state.cancelling}>
              {state.cancelling ? "Stopping" : "Stop and undo"}
            </button>
          )}
        </div>
      </motion.div>
    </div>
  );
}
