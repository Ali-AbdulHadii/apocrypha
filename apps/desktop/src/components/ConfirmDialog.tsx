/**
 * Confirmation for actions that destroy something.
 *
 * Used sparingly. A confirmation on a reversible action is noise, so this is
 * reserved for things that delete files with no undo.
 */

import { motion } from "framer-motion";
import { useEffect } from "react";
import { Icon } from "./icons";

export interface Confirm {
  title: string;
  body: string;
  /** Label for the destructive action, for example "Remove". */
  confirmLabel: string;
  onConfirm: () => void;
}

export function ConfirmDialog({
  confirm,
  busy,
  onCancel,
}: {
  confirm: Confirm;
  busy?: boolean;
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
    <div
      className="overlay"
      onClick={(e) => {
        if (e.target === e.currentTarget && !busy) onCancel();
      }}
    >
      <motion.div
        className="dialog"
        initial={{ opacity: 0, scale: 0.98, y: 8 }}
        animate={{ opacity: 1, scale: 1, y: 0 }}
        exit={{ opacity: 0, scale: 0.99, y: 4 }}
        transition={{ type: "spring", stiffness: 420, damping: 36 }}
        role="alertdialog"
        aria-modal="true"
        aria-label={confirm.title}
      >
        <div className="dialog-head">
          <div className="row">
            <span
              style={{
                color: "var(--danger)",
                display: "grid",
                placeItems: "center",
              }}
            >
              <Icon.warning size={20} />
            </span>
            <span className="dialog-title">{confirm.title}</span>
          </div>
        </div>

        <div className="dialog-body">
          <p className="card-hint" style={{ margin: 0 }}>
            {confirm.body}
          </p>
        </div>

        <div className="dialog-foot">
          <div style={{ flex: 1 }} />
          <button className="btn" onClick={onCancel} disabled={busy}>
            Cancel
          </button>
          <button className="btn danger" onClick={confirm.onConfirm} disabled={busy}>
            <Icon.trash /> {confirm.confirmLabel}
          </button>
        </div>
      </motion.div>
    </div>
  );
}
