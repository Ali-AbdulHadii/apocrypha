/**
 * Apply progress dialog.
 *
 * The backend applies a deployment as one blocking call, so this shows the
 * phases it moves through rather than a fake byte counter. It stays open on
 * completion long enough to report what happened, and on failure it keeps the
 * message on screen instead of dropping a toast that scrolls away.
 */

import { motion } from "framer-motion";
import { Icon } from "./icons";
import { Spinner } from "./ui";
import { formatBytes, type DeployResultView } from "../lib/api";

export type ApplyPhase = "checking" | "reverting" | "linking" | "recording" | "done";

const PHASES: { id: ApplyPhase; label: string }[] = [
  { id: "checking", label: "Checking what changed" },
  { id: "reverting", label: "Removing the previous set" },
  { id: "linking", label: "Copying files into the game" },
  { id: "recording", label: "Saving a record so this can be undone" },
];

export interface ApplyState {
  phase: ApplyPhase;
  result?: DeployResultView;
  error?: string;
}

export function ApplyDialog({
  state,
  onClose,
}: {
  state: ApplyState;
  onClose: () => void;
}) {
  const index = PHASES.findIndex((p) => p.id === state.phase);
  const finished = state.phase === "done";
  const failed = Boolean(state.error);
  const progress = failed ? 1 : finished ? 1 : (index + 1) / (PHASES.length + 1);

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
                  : finished
                    ? "var(--success)"
                    : "var(--accent)",
                display: "grid",
                placeItems: "center",
              }}
            >
              {failed ? (
                <Icon.warning size={20} />
              ) : finished ? (
                <Icon.check size={20} />
              ) : (
                <Spinner />
              )}
            </span>
            <span className="dialog-title">
              {failed
                ? "Could not apply"
                : finished
                  ? "Mods applied"
                  : "Applying mods"}
            </span>
          </div>
        </div>

        <div className="dialog-body">
          {!failed && (
            <div className="progress-track">
              <motion.div
                className="progress-fill"
                initial={{ width: 0 }}
                animate={{ width: `${progress * 100}%` }}
                transition={{ duration: 0.3, ease: [0.16, 1, 0.3, 1] }}
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
              </div>
            </div>
          ) : finished && state.result ? (
            <div className="stack tight">
              <div className="card-hint">
                {state.result.filesDeployed} files copied into the game folder,
                totalling {formatBytes(state.result.bytes)}.
              </div>
              <div className="card-hint">
                Use Undo on the bottom bar to remove them again at any time.
              </div>
            </div>
          ) : (
            <div className="phase-list">
              {PHASES.map((p, i) => {
                const done = i < index;
                const active = i === index;
                return (
                  <div
                    key={p.id}
                    className={`phase ${done ? "done" : ""} ${active ? "active" : ""}`}
                  >
                    <span className="phase-dot">
                      {done && <Icon.check size={11} />}
                    </span>
                    <span>{p.label}</span>
                  </div>
                );
              })}
            </div>
          )}
        </div>

        {(finished || failed) && (
          <div className="dialog-foot">
            <div style={{ flex: 1 }} />
            <button className="btn primary" onClick={onClose}>
              Done
            </button>
          </div>
        )}
      </motion.div>
    </div>
  );
}
