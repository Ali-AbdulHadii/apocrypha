/**
 * Downloads.
 *
 * A finished download waits here instead of forcing the install wizard open the
 * moment it lands. That separation is the point of the screen: fetching a file
 * and deciding to install it are two different decisions, and a download that
 * completes while you are reading a conflict list should not steal the window.
 *
 * Archives you saved from a browser appear alongside the ones fetched here, and
 * install through exactly the same button. On a free Nexus account that is the
 * normal path, not a fallback.
 */

import { AnimatePresence, motion } from "framer-motion";
import { Icon } from "./icons";
import { Chip, Spinner } from "./ui";
import {
  formatBytes,
  formatEta,
  formatRate,
  type DownloadView,
} from "../lib/api";

/** Grouped so the eye lands on what needs an action, not on what is finished. */
const SECTIONS = [
  { key: "downloading", title: "Downloading" },
  { key: "ready", title: "Ready to install" },
  { key: "failed", title: "Did not finish" },
] as const;

export function DownloadsScreen({
  downloads,
  busy,
  installingId,
  onInstall,
  onCancel,
  onRemove,
  onRefresh,
}: {
  downloads: DownloadView[];
  busy: boolean;
  /** Id of the download currently being opened, so its row can show it. */
  installingId: string | null;
  onInstall: (d: DownloadView) => void;
  onCancel: (d: DownloadView) => void;
  onRemove: (d: DownloadView) => void;
  onRefresh: () => void;
}) {
  const bucket = (d: DownloadView) =>
    d.state === "downloading"
      ? "downloading"
      : d.state === "ready"
        ? "ready"
        : "failed";

  if (downloads.length === 0) {
    return (
      <div className="empty">
        <span className="empty-icon">
          <Icon.downloads size={40} strokeWidth={1} />
        </span>
        <div className="empty-title">No downloads yet</div>
        <div style={{ maxWidth: 460 }}>
          Press Mod Manager Download on Nexus Mods and the file arrives here.
          Anything you save into the downloads folder yourself shows up here too,
          ready to install.
        </div>
        <button
          className="btn"
          onClick={onRefresh}
          disabled={busy}
          style={{ marginTop: 8 }}
        >
          <Icon.refresh /> Check again
        </button>
      </div>
    );
  }

  return (
    <div className="stack">
      {SECTIONS.map((section) => {
        const rows = downloads.filter((d) => bucket(d) === section.key);
        if (rows.length === 0) return null;
        return (
          <div className="stack tight" key={section.key}>
            <div className="section-head">
              <span className="section-title">{section.title}</span>
              <span className="section-count">{rows.length}</span>
            </div>
            <AnimatePresence initial={false}>
              {rows.map((d) => (
                <DownloadRow
                  key={d.id}
                  d={d}
                  busy={busy}
                  installing={installingId === d.id}
                  onInstall={onInstall}
                  onCancel={onCancel}
                  onRemove={onRemove}
                />
              ))}
            </AnimatePresence>
          </div>
        );
      })}
    </div>
  );
}

function DownloadRow({
  d,
  busy,
  installing,
  onInstall,
  onCancel,
  onRemove,
}: {
  d: DownloadView;
  busy: boolean;
  installing: boolean;
  onInstall: (d: DownloadView) => void;
  onCancel: (d: DownloadView) => void;
  onRemove: (d: DownloadView) => void;
}) {
  const active = d.state === "downloading";
  // A mirror that sends no Content-Length leaves the size unknown, so the bar
  // becomes indeterminate rather than pretending to a percentage.
  const known = active && d.totalBytes !== null && d.totalBytes > 0;
  const percent = known ? Math.min(1, d.receivedBytes / d.totalBytes!) : 0;

  return (
    <motion.div
      className={`download-row ${active ? "active" : ""}`}
      layout
      initial={{ opacity: 0, y: 4 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, height: 0, marginBottom: 0 }}
      transition={{ duration: 0.18, ease: [0.16, 1, 0.3, 1] }}
    >
      <span className={`download-icon ${d.state}`}>
        {active ? (
          <Spinner />
        ) : d.state === "ready" ? (
          d.installedAs ? (
            <Icon.check size={18} />
          ) : (
            <Icon.package size={18} />
          )
        ) : (
          <Icon.warning size={18} />
        )}
      </span>

      <div className="download-main">
        <div className="row" style={{ gap: "var(--sp-3)" }}>
          <span className="mod-name">{d.fileName}</span>
          {d.state === "ready" && d.source === "Downloads folder" && (
            <Chip>found on disk</Chip>
          )}
          {d.installedAs && <Chip kind="ok">in your library</Chip>}
          {d.state === "cancelled" && <Chip kind="warn">stopped</Chip>}
          {d.state === "failed" && <Chip kind="bad">failed</Chip>}
        </div>

        <div className="mod-meta">{metaLine(d)}</div>

        {active && (
          <div className={`progress-track ${known ? "" : "indeterminate"}`}>
            {known && (
              <motion.div
                className="progress-fill"
                animate={{ width: `${percent * 100}%` }}
                transition={{ duration: 0.25, ease: "linear" }}
              />
            )}
          </div>
        )}
      </div>

      <div className="row download-actions">
        {active ? (
          <button className="btn sm" onClick={() => onCancel(d)}>
            Stop
          </button>
        ) : (
          <>
            {d.state === "ready" && (
              // Once it is in the library the row stops leading with a primary
              // action, but stays installable: re-importing the same archive
              // replaces the existing mod rather than duplicating it, which is
              // how you change the options you picked.
              <button
                className={`btn sm ${d.installedAs ? "" : "primary"}`}
                onClick={() => onInstall(d)}
                disabled={busy}
                title={
                  d.installedAs
                    ? `Already added as ${d.installedAs}. Installing again reopens the options.`
                    : undefined
                }
              >
                {installing ? <Spinner /> : <Icon.plus size={14} />}
                {d.installedAs ? "Install again" : "Install"}
              </button>
            )}
            <button
              className="btn sm icon"
              onClick={() => onRemove(d)}
              disabled={busy}
              aria-label={`Delete ${d.fileName}`}
              title="Delete the downloaded file"
            >
              <Icon.trash size={14} />
            </button>
          </>
        )}
      </div>
    </motion.div>
  );
}

function metaLine(d: DownloadView): string {
  if (d.state === "downloading") {
    const size =
      d.totalBytes && d.totalBytes > 0
        ? `${formatBytes(d.receivedBytes)} of ${formatBytes(d.totalBytes)}`
        : formatBytes(d.receivedBytes);
    return [
      size,
      formatRate(d.bytesPerSecond),
      formatEta(d.totalBytes, d.receivedBytes, d.bytesPerSecond),
    ]
      .filter(Boolean)
      .join(" · ");
  }
  if (d.state === "failed") return d.error ?? "The download did not complete";
  if (d.state === "cancelled") return "You stopped this download";
  if (d.installedAs) {
    return `${formatBytes(d.receivedBytes)} · added as ${d.installedAs}`;
  }
  return `${formatBytes(d.receivedBytes)} · from ${d.source}`;
}
