/**
 * One mod, in full.
 *
 * Browse cards carry only what fits on a card, so this is where the rest lives:
 * the description, what the mod needs, what it will not sit beside, and every
 * file of the newest release. It replaced an inline file list on the card,
 * which made one card in a row twice the height of its neighbours and still had
 * nowhere to put dependencies.
 *
 * Requirements and incompatibilities are the reason this exists rather than
 * being a nicety. A mod that needs another, or that is known to break one
 * already deployed, fails at the point the game refuses to start — long after
 * the download, and with nothing on screen connecting the two.
 */

import { motion } from "framer-motion";
import { useEffect } from "react";
import type { CatalogFileView, CatalogModDetailView, CatalogRelationshipView } from "../lib/api";
import { CoverArt } from "./CoverArt";
import { Icon } from "./icons";
import { Spinner } from "./ui";

function size(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${Math.round(bytes / 1024 ** 2)} MB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

function ready(f: CatalogFileView): boolean {
  return f.scanState.toLowerCase() === "clean" && f.uploadState.toLowerCase() === "verified";
}

/**
 * The service's own words, matched case-insensitively.
 *
 * Anything it adds later falls through to neither list and is shown under its
 * own name rather than dropped — a relationship nobody displays is one nobody
 * acts on.
 */
export function required(rs: CatalogRelationshipView[]): CatalogRelationshipView[] {
  return rs.filter((r) => r.type.toLowerCase() === "required");
}
export function optional(rs: CatalogRelationshipView[]): CatalogRelationshipView[] {
  return rs.filter((r) => r.type.toLowerCase() === "optional");
}
export function incompatible(rs: CatalogRelationshipView[]): CatalogRelationshipView[] {
  return rs.filter((r) => r.type.toLowerCase() === "incompatible");
}

export function ModPage({
  mod,
  detail,
  busy,
  downloadedIds,
  onDownload,
  onClose,
}: {
  mod: { slug: string; name: string; gameName: string };
  detail: CatalogModDetailView | null;
  busy: boolean;
  /** File ids already sent to the queue this session, so buttons stay honest. */
  downloadedIds: Set<string>;
  onDownload: (fileId: string) => void;
  onClose: () => void;
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const release = detail?.versions.find((v) => v.isLatest) ?? detail?.versions[0] ?? null;
  const rels = detail?.relationships ?? [];
  const needs = required(rels);
  const nice = optional(rels);
  const breaks = incompatible(rels);

  return (
    <div className="overlay" onClick={(e) => e.target === e.currentTarget && onClose()}>
      <motion.div
        className="modpage"
        initial={{ opacity: 0, y: 10 }}
        animate={{ opacity: 1, y: 0 }}
        exit={{ opacity: 0, y: 6 }}
        transition={{ type: "spring", stiffness: 420, damping: 36 }}
        role="dialog"
        aria-modal="true"
        aria-label={mod.name}
      >
        <div className="modpage-art">
          <CoverArt seed={mod.slug} mark={200} />
          <button className="modpage-close" onClick={onClose} aria-label="Close">
            <Icon.close size={16} />
          </button>
        </div>

        <div className="modpage-body">
          <h2 className="modpage-name">{mod.name}</h2>
          <p className="modpage-by">
            {detail ? `${detail.authorName} · ${detail.gameName}` : mod.gameName}
            {release ? <span className="mono"> · {release.versionNumber}</span> : null}
          </p>

          {!detail ? (
            <div className="row" style={{ gap: "var(--sp-3)" }}>
              <Spinner />
              <span className="mod-meta">Loading…</span>
            </div>
          ) : (
            <>
              {breaks.length > 0 ? (
                <div className="modpage-warn">
                  <span className="modpage-warn-title">
                    Does not work alongside {breaks.length === 1 ? "one mod" : `${breaks.length} mods`}
                  </span>
                  <span className="mod-meta">{breaks.map((r) => r.targetModName).join(", ")}</span>
                </div>
              ) : null}

              {needs.length > 0 ? (
                <div className="modpage-need">
                  <span className="modpage-warn-title">Needs</span>
                  <span className="mod-meta">{needs.map((r) => r.targetModName).join(", ")}</span>
                </div>
              ) : null}

              {/* Plain text, deliberately. Descriptions are creator-written and
                  arrive as whatever they typed; rendering them as markup would
                  make a mod page a place other people can put markup. */}
              <p className="modpage-desc">{detail.description || detail.summary}</p>

              {nice.length > 0 ? (
                <p className="mod-meta">
                  Works with: {nice.map((r) => r.targetModName).join(", ")}
                </p>
              ) : null}

              <p className="mod-meta">
                {detail.downloadCount.toLocaleString()} downloads
                {detail.favorCount > 0
                  ? ` · ${detail.favorCount.toLocaleString()} favours`
                  : ""}
              </p>

              <div className="divider" />

              <h3 className="modpage-files-title">Files</h3>
              {!release || release.files.length === 0 ? (
                <span className="mod-meta">No files published yet.</span>
              ) : (
                <div className="modpage-files">
                  {release.files.map((f) => (
                    <div className="browse-file" key={f.id}>
                      <span className="browse-file-text">
                        <span className="browse-file-name">
                          {f.displayName?.trim() || f.fileName}
                        </span>
                        <span className="mod-meta">
                          {size(f.sizeBytes)}
                          {ready(f) ? "" : " · not ready"}
                        </span>
                      </span>
                      <button
                        className="btn sm"
                        disabled={!ready(f) || busy || downloadedIds.has(f.id)}
                        onClick={() => onDownload(f.id)}
                      >
                        {downloadedIds.has(f.id) ? "In downloads" : "Download"}
                      </button>
                    </div>
                  ))}
                </div>
              )}
            </>
          )}
        </div>
      </motion.div>
    </div>
  );
}
