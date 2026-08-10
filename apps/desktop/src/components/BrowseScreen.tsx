/**
 * Browsing the Apocrypha catalogue from inside the app.
 *
 * A grid of cards, matching the website so the two do not read as different
 * products. The cover is procedural — a gradient tinted by a hue derived from
 * the mod's own slug — because the service has no mod images yet; when it does,
 * that element is the one that gets replaced and nothing else here changes.
 *
 * Scoped to the game selected in Library. Someone managing Monster Hunter Wilds
 * is not shopping for Cyberpunk mods, and a catalogue that ignores what the
 * rest of the window is about makes them do the filtering themselves.
 *
 * Nothing is filtered client-side beyond that. What may be seen is the server's
 * decision — scan state, adult content, region — and a filter in a client
 * anyone can rebuild is not one.
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
import { CoverArt } from "./CoverArt";
import { Icon } from "./icons";
import { Spinner } from "./ui";

function size(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${Math.round(bytes / 1024 ** 2)} MB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

/** Clean scan and verified bytes — the pair the service checks before serving. */
function ready(f: CatalogFileView): boolean {
  return f.scanState.toLowerCase() === "clean" && f.uploadState.toLowerCase() === "verified";
}

export function BrowseScreen({
  signedIn,
  gameId,
  gameName,
  onSignIn,
  onError,
}: {
  signedIn: boolean;
  /** The game Library is on. Matched against the service's own slugs. */
  gameId: string | null;
  gameName: string | null;
  onSignIn: () => void;
  onError: (e: unknown) => void;
}) {
  const [page, setPage] = useState<CatalogPageView | null>(null);
  const [search, setSearch] = useState("");
  const [busy, setBusy] = useState(false);
  const [quota, setQuota] = useState<DownloadQuotaView | null>(null);
  // null while unknown, false once the service has been asked and does not list
  // this game. Three states, three different things to say.
  const [listed, setListed] = useState<boolean | null>(null);
  const requestId = useRef(0);

  // Asked once. It is the only way to tell "this game has no mods yet" from
  // "the service has never heard of this game", and the app's game id is not
  // guaranteed to be the service's slug even while they currently match.
  useEffect(() => {
    if (!signedIn || !gameId) return;
    let alive = true;
    api
      .apocryphaGames()
      .then((games) => {
        if (alive) setListed(games.some((g) => g.slug === gameId));
      })
      .catch(() => {
        // Unknown rather than absent: a failed lookup must not claim a game is
        // missing from a catalogue nobody managed to read.
        if (alive) setListed(null);
      });
    return () => {
      alive = false;
    };
  }, [signedIn, gameId]);

  const refreshQuota = useCallback(() => {
    if (!signedIn) return;
    api
      .apocryphaDownloadQuota()
      .then(setQuota)
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
        const result = await api.browseApocryphaMods(gameId, term || null, pageNumber);
        if (ticket === requestId.current) setPage(result);
      } catch (e) {
        if (ticket === requestId.current) onError(e);
      } finally {
        if (ticket === requestId.current) setBusy(false);
      }
    },
    [signedIn, gameId, onError],
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
          The catalogue is read as your account, so this computer has to be signed
          in first. It takes one approval in your browser.
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
          placeholder={gameName ? `Search ${gameName} mods` : "Search the catalogue"}
          spellCheck={false}
          style={{ flex: 1 }}
        />
        {busy ? <Spinner /> : null}
      </div>

      {items.length === 0 && !busy ? (
        <div className="empty">
          <div className="empty-title">
            {search
              ? "Nothing matched"
              : listed === false
                ? `${gameName ?? "This game"} is not on Apocrypha yet`
                : "No mods here yet"}
          </div>
          <div style={{ maxWidth: 460 }}>
            {search
              ? "Try fewer words, or a different spelling."
              : listed === false
                ? "The service does not list this game, so there is nothing to browse. You can still install mods from a file, or from Nexus."
                : "Nothing has been published for this game yet."}
          </div>
        </div>
      ) : (
        <div className="browse-grid">
          {items.map((m) => (
            <BrowseCard key={m.id} mod={m} onError={onError} onClaimed={refreshQuota} />
          ))}
        </div>
      )}

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

      {quota && !quota.verified && quota.remaining !== null ? (
        <div className="browse-note">
          {quota.remaining > 0
            ? `${quota.remaining} more ${quota.remaining === 1 ? "mod" : "mods"} today. `
            : "That is today's allowance used. "}
          Verifying your email address removes the daily limit.
        </div>
      ) : null}
    </div>
  );
}

function BrowseCard({
  mod,
  onError,
  onClaimed,
}: {
  mod: CatalogModView;
  onError: (e: unknown) => void;
  onClaimed: () => void;
}) {
  const [detail, setDetail] = useState<CatalogModDetailView | null>(null);
  const [loading, setLoading] = useState(false);
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [sent, setSent] = useState(false);

  const release = detail?.versions.find((v) => v.isLatest) ?? detail?.versions[0] ?? null;
  const files = release?.files ?? [];

  const fetchDetail = useCallback(async () => {
    if (detail) return detail;
    setLoading(true);
    try {
      const d = await api.apocryphaModDetail(mod.gameSlug, mod.slug);
      setDetail(d);
      return d;
    } finally {
      setLoading(false);
    }
  }, [detail, mod.gameSlug, mod.slug]);

  const start = async (fileId: string) => {
    setBusy(true);
    try {
      await api.apocryphaDownloadFile(mod.gameSlug, mod.slug, fileId);
      setSent(true);
      onClaimed();
    } catch (e) {
      onError(e);
    } finally {
      setBusy(false);
    }
  };

  /**
   * One press does the obvious thing, when there is only one obvious thing.
   *
   * Files are fetched on demand rather than with the listing — twenty cards on
   * screen would otherwise be twenty requests for detail nobody asked to see.
   * With exactly one downloadable file the press downloads it; with more, the
   * card opens so the choice is made deliberately. A mod shipping a main file
   * alongside optional extras is where guessing is worst, because the wrong
   * pick is the one that ends up deployed into a game.
   */
  const onDownload = async () => {
    setBusy(true);
    try {
      const d = await fetchDetail();
      const latest = d.versions.find((v) => v.isLatest) ?? d.versions[0] ?? null;
      const usable = (latest?.files ?? []).filter(ready);
      if (usable.length === 1) {
        await start(usable[0]!.id);
        return;
      }
      setOpen(true);
    } catch (e) {
      onError(e);
    } finally {
      setBusy(false);
    }
  };

  return (
    <article className="browse-card">
      <div className="browse-card-art">
        <CoverArt seed={mod.slug} />
      </div>

      <div className="browse-card-body">
        <h3 className="browse-card-name" title={mod.name}>
          {mod.name}
        </h3>
        <p className="browse-card-by">
          {mod.authorName}
          {mod.latestVersion ? <span className="mono"> · {mod.latestVersion}</span> : null}
        </p>
        <p className="browse-card-summary">{mod.summary}</p>
        <p className="browse-card-facts">
          {mod.downloadCount.toLocaleString()} downloads
          {mod.favorCount > 0 ? ` · ${mod.favorCount.toLocaleString()} favours` : ""}
        </p>
      </div>

      <div className="browse-card-foot">
        <button
          className="btn primary sm"
          onClick={() => void onDownload()}
          disabled={busy || loading || sent}
        >
          {sent ? "In downloads" : busy || loading ? "Working…" : "Download"}
        </button>
        <button
          className="btn sm"
          onClick={() => {
            const next = !open;
            setOpen(next);
            if (next) void fetchDetail().catch(onError);
          }}
          disabled={loading}
        >
          {open ? "Hide files" : "Files"}
        </button>
      </div>

      {open ? (
        <div className="browse-card-files">
          {loading ? (
            <Spinner />
          ) : files.length === 0 ? (
            <span className="mod-meta">No files published yet.</span>
          ) : (
            files.map((f) => (
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
                  disabled={!ready(f) || busy || sent}
                  onClick={() => void start(f.id)}
                >
                  Get
                </button>
              </div>
            ))
          )}
        </div>
      ) : null}
    </article>
  );
}
