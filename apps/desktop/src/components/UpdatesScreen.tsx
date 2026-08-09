/**
 * Mod updates.
 *
 * Checking costs one Nexus API request per linked mod, against an hourly quota,
 * so it happens when asked and never on mount. That single constraint shapes the
 * screen: there is an explicit button, and the result says how much of the
 * library it actually got through, because a check that stopped halfway and
 * reported "all up to date" would be worse than no check at all.
 *
 * Applying an update is a download, not an install. It lands in Downloads and
 * waits there like any other file, because replacing a working mod deserves a
 * deliberate second step — and on a free Nexus account the transfer has to be
 * started from the website anyway.
 */

import { AnimatePresence, motion } from "framer-motion";
import { Icon } from "./icons";
import { Chip, Spinner } from "./ui";
import type { ModUpdateView, UpdateCheckView } from "../lib/api";

/** Actionable first. Nothing below "available" needs a decision today. */
const SECTIONS = [
  { key: "available", title: "Updates available" },
  { key: "unknown", title: "No longer on the mod page" },
  { key: "error", title: "Could not check" },
  { key: "upToDate", title: "Up to date" },
] as const;

export function UpdatesScreen({
  result,
  checking,
  isPremium,
  busyId,
  onCheck,
  onDownload,
  onOpenPage,
}: {
  result: UpdateCheckView | null;
  checking: boolean;
  /** A free account cannot have the manager fetch a file unattended. */
  isPremium: boolean;
  /** Local mod id whose update is being started, so its row can show it. */
  busyId: string | null;
  onCheck: () => void;
  onDownload: (u: ModUpdateView) => void;
  onOpenPage: (u: ModUpdateView) => void;
}) {
  if (checking && !result) {
    return (
      <div className="empty">
        <span className="empty-icon">
          <Spinner />
        </span>
        <div className="empty-title">Checking for updates</div>
        <div style={{ maxWidth: 460 }}>
          One request per mod, so a large library takes a moment.
        </div>
      </div>
    );
  }

  if (!result) {
    return (
      <div className="empty">
        <span className="empty-icon">
          <Icon.refresh size={40} strokeWidth={1} />
        </span>
        <div className="empty-title">Check for mod updates</div>
        <div style={{ maxWidth: 460 }}>
          Asks Nexus Mods whether anything in your library has a newer file. This
          costs one API request per mod, so it runs only when you ask for it.
        </div>
        <button
          className="btn primary"
          onClick={onCheck}
          disabled={checking}
          style={{ marginTop: 8 }}
        >
          <Icon.refresh /> Check now
        </button>
      </div>
    );
  }

  const available = result.results.filter((r) => r.status === "available").length;

  return (
    <div className="stack">
      <div className="section-head">
        <span className="section-title">
          {available === 0
            ? "Everything checked is up to date"
            : `${available} update${available === 1 ? "" : "s"} available`}
        </span>
        <button className="btn sm" onClick={onCheck} disabled={checking}>
          {checking ? <Spinner /> : <Icon.refresh />} Check again
        </button>
      </div>

      <Caveats result={result} />

      {SECTIONS.map((section) => {
        const rows = result.results.filter((r) => r.status === section.key);
        if (rows.length === 0) return null;
        return (
          <div className="stack tight" key={section.key}>
            <div className="section-head">
              <span className="section-title">{section.title}</span>
              <span className="section-count">{rows.length}</span>
            </div>
            <AnimatePresence initial={false}>
              {rows.map((u) => (
                <UpdateRow
                  key={u.id}
                  u={u}
                  isPremium={isPremium}
                  busy={busyId === u.id}
                  onDownload={onDownload}
                  onOpenPage={onOpenPage}
                />
              ))}
            </AnimatePresence>
          </div>
        );
      })}
    </div>
  );
}

/**
 * Everything that makes the answer less than complete, stated plainly.
 *
 * The two ways a check comes back partial — running out of quota, and mods with
 * no Nexus provenance to compare against — are always visible when they apply,
 * so "up to date" never has to be read as a guarantee it cannot make.
 */
function Caveats({ result }: { result: UpdateCheckView }) {
  const notes: string[] = [];

  if (result.stoppedForQuota) {
    notes.push(
      `Stopped early: the Nexus API quota ran out with ${result.skipped} mod${
        result.skipped === 1 ? "" : "s"
      } left. Those are unknown, not up to date.`,
    );
  }
  if (result.uncheckable > 0) {
    notes.push(
      `${result.uncheckable} mod${
        result.uncheckable === 1 ? " was" : "s were"
      } imported from a file rather than downloaded from Nexus, so there is nothing to compare them against.`,
    );
  }
  if (result.hourlyRemaining !== null) {
    notes.push(`${result.hourlyRemaining} API requests left this hour.`);
  }

  if (notes.length === 0) return null;

  return (
    <div className="card">
      {notes.map((n) => (
        <div className="card-hint" key={n}>
          {n}
        </div>
      ))}
    </div>
  );
}

function UpdateRow({
  u,
  isPremium,
  busy,
  onDownload,
  onOpenPage,
}: {
  u: ModUpdateView;
  isPremium: boolean;
  busy: boolean;
  onDownload: (u: ModUpdateView) => void;
  onOpenPage: (u: ModUpdateView) => void;
}) {
  const meta =
    u.status === "available" ? (
      <>
        {u.currentVersion ?? "installed"} → <strong>{u.newVersion ?? "newer file"}</strong>
        {u.newFileName ? ` · ${u.newFileName}` : ""}
      </>
    ) : u.status === "unknown" ? (
      "Not on the mod page any more. The copy you have still works."
    ) : u.status === "error" ? (
      (u.error ?? "Could not be checked.")
    ) : (
      (u.currentVersion ?? "installed")
    );

  return (
    <motion.div
      className="mod-row"
      layout
      initial={{ opacity: 0, y: 4 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, height: 0, marginBottom: 0 }}
      transition={{ duration: 0.18, ease: [0.16, 1, 0.3, 1] }}
    >
      <div style={{ flex: 1, minWidth: 0 }}>
        <div className="row" style={{ gap: "var(--sp-3)" }}>
          <span className="mod-name">{u.name}</span>
          {u.status === "available" && <Chip kind="ok">new version</Chip>}
          {u.status === "unknown" && <Chip kind="warn">withdrawn</Chip>}
          {u.status === "error" && <Chip kind="bad">not checked</Chip>}
        </div>
        <div className="mod-meta">{meta}</div>
      </div>

      <div className="row" style={{ gap: "var(--sp-2)" }}>
        {u.status === "available" && isPremium ? (
          <button
            className="btn primary sm"
            disabled={busy || u.newFileId === null}
            onClick={() => onDownload(u)}
          >
            {busy ? <Spinner /> : <Icon.downloads />} Download
          </button>
        ) : null}
        {/* A free account cannot have the manager request a file: the API
            refuses without a token only the website mints. The mod page is the
            working path for them, not a consolation prize. */}
        <button className="btn sm" disabled={busy} onClick={() => onOpenPage(u)}>
          {u.status === "available" && !isPremium ? "Open mod page" : "Mod page"}
        </button>
      </div>
    </motion.div>
  );
}
