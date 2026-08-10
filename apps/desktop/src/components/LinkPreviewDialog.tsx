/**
 * What a website's "Download with Mod Manager" button actually asked for.
 *
 * This is the step between "a web page caused this app to open" and "this app
 * spends the account's allowance and starts a transfer". A page anyone can
 * publish can invoke the link, so the person has to be the one who decides —
 * and to decide they need to be looking at facts the service supplied, never
 * ones the link did. Nothing on this screen comes from the URL: the name, the
 * author, the size and the readiness were all read back from the service using
 * the three identifiers the link carried.
 *
 * It is deliberately not a yes/no on an abstract question. It names the mod,
 * the file, what it will cost, and what happens next — a confirmation nobody
 * can act on is the same as no confirmation.
 */

import { motion } from "framer-motion";
import { useEffect } from "react";
import type { LinkPreviewView } from "../lib/api";
import { Icon } from "./icons";
import { Spinner } from "./ui";

function size(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${Math.round(bytes / 1024 ** 2)} MB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

export function LinkPreviewDialog({
  preview,
  busy,
  onConfirm,
  onCancel,
}: {
  preview: LinkPreviewView | null;
  busy?: boolean;
  onConfirm: () => void;
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
        role="dialog"
        aria-modal="true"
        aria-label="Download from a website link"
      >
        <div className="dialog-head">
          <div className="row">
            <span style={{ color: "var(--accent)", display: "grid", placeItems: "center" }}>
              <Icon.downloads size={20} />
            </span>
            <span className="dialog-title">Download this mod?</span>
          </div>
        </div>

        <div className="dialog-body">
          {!preview ? (
            <div className="row" style={{ gap: "var(--sp-3)" }}>
              <Spinner />
              <span className="card-hint">Checking what this link points at…</span>
            </div>
          ) : (
            <>
              <p className="card-hint" style={{ margin: "0 0 var(--sp-4)" }}>
                A website asked Apocrypha to download this. Everything below was
                read from the service, not from the link.
              </p>

              <div className="lib-group">
                <div className="lib-row">
                  <span className="lib-row-label">Mod</span>
                  <span className="lib-row-value">{preview.modName}</span>
                </div>
                <div className="lib-row">
                  <span className="lib-row-label">Author</span>
                  <span className="lib-row-value">{preview.authorName}</span>
                </div>
                <div className="lib-row">
                  <span className="lib-row-label">Game</span>
                  <span className="lib-row-value">{preview.gameName}</span>
                </div>
                <div className="lib-row">
                  <span className="lib-row-label">File</span>
                  <span className="lib-row-value">
                    {preview.fileLabel}
                    {preview.version ? ` · ${preview.version}` : ""}
                  </span>
                </div>
                <div className="lib-row">
                  <span className="lib-row-label">Size</span>
                  <span className="lib-row-value">{size(preview.sizeBytes)}</span>
                </div>
                <div className="lib-row">
                  <span className="lib-row-label">Checked</span>
                  {/* Worded as what was found, not as a guarantee. A scanner
                      reporting nothing is not the same as a file being safe,
                      and a dialog that says "safe" is making a promise the
                      platform cannot keep. */}
                  <span
                    className="lib-row-value"
                    style={{ color: preview.ready ? "var(--success)" : "var(--warning)" }}
                  >
                    {preview.ready ? "Scanned, nothing found" : "Not ready to download"}
                  </span>
                </div>
              </div>

              <div className="lib-group-note">
                This downloads to your queue and nothing is installed
                automatically — installing stays a separate step.
                {preview.remainingToday !== null
                  ? ` It uses one of today's ${preview.remainingToday} remaining downloads.`
                  : ""}
              </div>
            </>
          )}
        </div>

        <div className="dialog-foot">
          <button className="btn" onClick={onCancel} disabled={busy}>
            Cancel
          </button>
          <button
            className="btn primary"
            onClick={onConfirm}
            disabled={busy || !preview || !preview.ready}
          >
            {busy ? "Starting…" : "Download"}
          </button>
        </div>
      </motion.div>
    </div>
  );
}
