/**
 * Browsing the Apocrypha catalogue from inside the app.
 *
 * Rows rather than a grid: without cover art there is nothing for a grid to
 * show, and a mod is chosen by reading its name, author and summary. When the
 * service serves images this becomes the place they go.
 *
 * The list is whatever the service returned, in the order it returned it.
 * Nothing is filtered here — what may be seen is the server's decision, and a
 * filter in a client anyone can rebuild is not one.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  type CatalogFileView,
  type CatalogModDetailView,
  type CatalogModView,
  type CatalogPageView,
  type DownloadQuotaView,
} from "../lib/api";
import { Icon } from "./icons";
import { Spinner } from "./ui";

/** Bytes as something a person reads, matching the download list's wording. */
function size(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${Math.round(bytes / 1024 ** 2)} MB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

export function BrowseScreen({
  signedIn,
  onSignIn,
  onError,
}: {
  signedIn: boolean;
  onSignIn: () => void;
  onError: (e: unknown) => void;
}) {
  const [page, setPage] = useState<CatalogPageView | null>(null);
  const [search, setSearch] = useState("");
  const [busy, setBusy] = useState(false);
  const [quota, setQuota] = useState<DownloadQuotaView | null>(null);
  const requestId = useRef(0);

  // Read once when the screen opens and again after each claim, so the note
  // under the list is the server's count rather than one this screen kept.
  const refreshQuota = useCallback(() => {
    if (!signedIn) return;
    api
      .apocryphaDownloadQuota()
      .then(setQuota)
      // A quota that will not load is not worth interrupting anyone over: the
      // note simply does not appear, and claiming still reports its own refusal.
      .catch(() => setQuota(null));
  }, [signedIn]);

  useEffect(() => refreshQuota(), [refreshQuota]);

  const load = useCallback(
    async (term: string, pageNumber: number) => {
      if (!signedIn) return;
      // Every request carries a ticket, and a reply is only used if its ticket
      // is still the newest. Without it a slow first search can land after a
      // fast second one and put the wrong results on screen.
      const ticket = ++requestId.current;
      setBusy(true);
      try {
        const result = await api.browseApocryphaMods(null, term || null, pageNumber);
        if (ticket === requestId.current) setPage(result);
      } catch (e) {
        if (ticket === requestId.current) onError(e);
      } finally {
        if (ticket === requestId.current) setBusy(false);
      }
    },
    [signedIn, onError],
  );

  useEffect(() => {
    void load("", 1);
  }, [load]);

  // Typing searches, but not on every keystroke: each one would be a request,
  // and the answer to a half-typed word is never the one wanted.
  useEffect(() => {
    if (!signedIn) return;
    const t = setTimeout(() => void load(search, 1), 300);
    return () => clearTimeout(t);
  }, [search, signedIn, load]);

  if (!signedIn) {
    return (
      <div className="empty">
        <span className="empty-icon">
          <Icon.search size={40} strokeWidth={1} />
        </span>
        <div className="empty-title">Sign in to browse</div>
        <div style={{ maxWidth: 460 }}>
          The catalogue is read as your account, so this computer has to be
          signed in first. It takes one approval in your browser.
        </div>
        <button className="btn primary" onClick={onSignIn} style={{ marginTop: 8 }}>
          Go to Account
        </button>
      </div>
    );
  }

  const items = page?.items ?? [];

  return (
    <div className="stack">
      <div className="row" style={{ gap: "var(--sp-3)" }}>
        <input
          className="input"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder="Search the catalogue"
          spellCheck={false}
          style={{ flex: 1 }}
        />
        {busy ? <Spinner /> : null}
      </div>

      {items.length === 0 && !busy ? (
        <div className="empty">
          <div className="empty-title">
            {search ? "Nothing matched" : "The catalogue is empty"}
          </div>
          <div style={{ maxWidth: 460 }}>
            {search
              ? "Try fewer words, or a different spelling."
              : "Nothing has been published yet."}
          </div>
        </div>
      ) : (
        <div className="lib-group">
          {items.map((m) => (
            <BrowseRow key={m.id} mod={m} onError={onError} onClaimed={refreshQuota} />
          ))}
        </div>
      )}

      {quota && !quota.verified && quota.remaining !== null ? (
        <div className="lib-group-note">
          {quota.remaining > 0
            ? `${quota.remaining} more ${quota.remaining === 1 ? "mod" : "mods"} today. `
            : "That is today's allowance used. "}
          Verifying your email address removes the daily limit.
        </div>
      ) : null}

      {page && page.total > items.length ? (
        <div className="row" style={{ gap: "var(--sp-3)" }}>
          <span className="mod-meta">
            {items.length} of {page.total}
          </span>
          <button
            className="btn sm"
            style={{ marginLeft: "auto" }}
            disabled={busy || page.page * page.pageSize >= page.total}
            onClick={() => void load(search, page.page + 1)}
          >
            Next
          </button>
        </div>
      ) : null}
    </div>
  );
}

function BrowseRow({
  mod,
  onError,
  onClaimed,
}: {
  mod: CatalogModView;
  onError: (e: unknown) => void;
  onClaimed: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [detail, setDetail] = useState<CatalogModDetailView | null>(null);
  const [loading, setLoading] = useState(false);

  // Files are fetched the first time the row is opened, not with the listing.
  // Twenty mods on screen is twenty requests nobody asked for, and the answer
  // is only needed once someone is deciding.
  const toggle = () => {
    const next = !open;
    setOpen(next);
    if (!next || detail || loading) return;
    setLoading(true);
    api
      .apocryphaModDetail(mod.gameSlug, mod.slug)
      .then(setDetail)
      .catch(onError)
      .finally(() => setLoading(false));
  };

  const release = detail?.versions.find((v) => v.isLatest) ?? detail?.versions[0] ?? null;

  return (
    <>
      <div
        className="lib-row tappable"
        role="button"
        tabIndex={0}
        onClick={toggle}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            toggle();
          }
        }}
      >
        <span className="lib-row-art">
          <Icon.package size={16} />
        </span>
        <span className="lib-row-text">
          <span className="row" style={{ gap: "var(--sp-3)" }}>
            <span className="lib-row-label">{mod.name}</span>
            {mod.latestVersion ? (
              <span className="mod-meta mono">{mod.latestVersion}</span>
            ) : null}
          </span>
          <span className="mod-meta">
            {mod.authorName} · {mod.gameName} · {mod.downloadCount.toLocaleString()} downloads
          </span>
        </span>
        <span className={open ? "chevron open" : "chevron"} style={{ marginLeft: "auto" }}>
          <Icon.chevronRight size={14} />
        </span>
      </div>

      {open ? (
        <div className="lib-row" style={{ display: "block" }}>
          {loading ? (
            <Spinner />
          ) : !release || release.files.length === 0 ? (
            <span className="mod-meta">This mod has no files to download yet.</span>
          ) : (
            <div className="stack" style={{ gap: "var(--sp-2)" }}>
              {release.files.map((f) => (
                <FileRow
                  key={f.id}
                  file={f}
                  gameSlug={mod.gameSlug}
                  modSlug={mod.slug}
                  onError={onError}
                  onClaimed={onClaimed}
                />
              ))}
            </div>
          )}
        </div>
      ) : null}
    </>
  );
}

function FileRow({
  file,
  gameSlug,
  modSlug,
  onError,
  onClaimed,
}: {
  file: CatalogFileView;
  gameSlug: string;
  modSlug: string;
  onError: (e: unknown) => void;
  onClaimed: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [sent, setSent] = useState(false);

  // Clean scan and verified bytes, the same pair the service checks before it
  // will serve anything. Checked here only so the button is not offered when it
  // is going to be refused — the refusal itself is the server's.
  const ready = file.scanState.toLowerCase() === "clean"
    && file.uploadState.toLowerCase() === "verified";

  const start = () => {
    setBusy(true);
    api
      .apocryphaDownloadFile(gameSlug, modSlug, file.id)
      .then(() => {
        // It goes to the download queue, exactly like a Nexus download, and
        // waits there until someone chooses to install it. Saying so is the
        // difference between "nothing happened" and "it is downloading".
        setSent(true);
        onClaimed();
      })
      .catch(onError)
      .finally(() => setBusy(false));
  };

  return (
    <div className="row" style={{ gap: "var(--sp-3)" }}>
      <span className="lib-row-text">
        <span className="lib-row-label">{file.displayName?.trim() || file.fileName}</span>
        <span className="mod-meta">
          {size(file.sizeBytes)}
          {ready ? "" : " · not ready to download yet"}
        </span>
      </span>
      <button
        className="btn sm"
        style={{ marginLeft: "auto" }}
        disabled={!ready || busy || sent}
        onClick={start}
      >
        {sent ? "In downloads" : busy ? "Starting…" : "Download"}
      </button>
    </div>
  );
}
