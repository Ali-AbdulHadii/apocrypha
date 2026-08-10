/**
 * What you are about to pull in, before it is pulled in.
 *
 * Shown when Download is pressed from a card — that is, without having opened
 * the mod page and read anything. A mod that requires another, or that is known
 * to break one already deployed, fails at the point the game refuses to start:
 * long after the download, with nothing on screen connecting the two. This is
 * the one moment where saying so is still useful.
 *
 * It is skipped when there is nothing to say. A confirmation that appears every
 * time teaches people to dismiss it, and then it is not a confirmation.
 */

import { motion } from "framer-motion";
import { useEffect } from "react";
import type { CatalogRelationshipView } from "../lib/api";
import { Icon } from "./icons";

export function DownloadConfirm({
  modName,
  fileName,
  required,
  optional,
  incompatible,
  busy,
  onConfirm,
  onView,
  onCancel,
}: {
  modName: string;
  fileName: string | null;
  required: CatalogRelationshipView[];
  optional: CatalogRelationshipView[];
  incompatible: CatalogRelationshipView[];
  busy: boolean;
  onConfirm: () => void;
  onView: () => void;
  onCancel: () => void;
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !busy) onCancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [busy, onCancel]);

  return (
    <div className="overlay" onClick={(e) => e.target === e.currentTarget && !busy && onCancel()}>
      <motion.div
        className="dialog"
        initial={{ opacity: 0, scale: 0.98, y: 8 }}
        animate={{ opacity: 1, scale: 1, y: 0 }}
        exit={{ opacity: 0, scale: 0.99, y: 4 }}
        transition={{ type: "spring", stiffness: 420, damping: 36 }}
        role="dialog"
        aria-modal="true"
        aria-label={`Download ${modName}`}
      >
        <div className="dialog-head">
          <div className="row">
            <span
              style={{
                color: incompatible.length > 0 ? "var(--warning)" : "var(--accent)",
                display: "grid",
                placeItems: "center",
              }}
            >
              <Icon.warning size={20} />
            </span>
            <span className="dialog-title">Before you download</span>
          </div>
        </div>

        <div className="dialog-body stack tight">
          <p className="card-hint" style={{ margin: 0 }}>
            {modName}
            {fileName ? ` · ${fileName}` : ""}
          </p>

          {incompatible.length > 0 ? (
            <div className="modpage-warn">
              <span className="modpage-warn-title">Does not work alongside</span>
              <span className="mod-meta">
                {incompatible.map((r) => r.targetModName).join(", ")}
              </span>
              {/* Said plainly rather than blocked. Whether one of these is
                  actually installed is the app's business and it does not know
                  yet — asserting a conflict it has not checked would be worse
                  than naming the risk. */}
              <span className="mod-meta">
                If you have any of these, expect them to fight.
              </span>
            </div>
          ) : null}

          {required.length > 0 ? (
            <div className="modpage-need">
              <span className="modpage-warn-title">Needs these to work</span>
              <span className="mod-meta">{required.map((r) => r.targetModName).join(", ")}</span>
              <span className="mod-meta">
                Downloading this one does not fetch them.
              </span>
            </div>
          ) : null}

          {optional.length > 0 ? (
            <p className="mod-meta" style={{ margin: 0 }}>
              Optional extras: {optional.map((r) => r.targetModName).join(", ")}
            </p>
          ) : null}

          <p className="mod-meta" style={{ margin: 0 }}>
            This goes to your downloads and installs nothing on its own.
          </p>
        </div>

        <div className="dialog-foot">
          <button className="btn" onClick={onCancel} disabled={busy}>
            Cancel
          </button>
          <button className="btn" onClick={onView} disabled={busy}>
            View mod
          </button>
          <button className="btn primary" onClick={onConfirm} disabled={busy}>
            {busy ? "Starting…" : "Download"}
          </button>
        </div>
      </motion.div>
    </div>
  );
}
