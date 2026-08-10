/**
 * Browsing the Apocrypha catalogue from inside the app.
 *
 * A grid of cards, matching the website so the two do not read as different
 * products. The cover is procedural — a gradient tinted by a hue derived from
 * the mod's own slug — because the service has no mod images yet; when it does,
 * that element is the one that gets replaced and nothing else here changes.
 *
 * Scoped to the game selected in Library, matched against the *service's* own
 * game list rather than by assuming this app's game id is the service's slug.
 * The two identifier spaces are maintained separately and only happen to agree
 * today.
 *
 * Nothing is filtered client-side beyond that. What may be seen is the server's
 * decision — scan state, adult content, region — and a filter in a client
 * anyone can rebuild is not one.
 */

import { AnimatePresence } from "framer-motion";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  type CatalogModDetailView,
  type CatalogModView,
  type CatalogPageView,
  type DownloadQuotaView,
} from "../lib/api";
import { CoverArt } from "./CoverArt";
import { DownloadConfirm } from "./DownloadConfirm";
import { Icon } from "./icons";
import { incompatible, ModPage, optional, required } from "./ModPage";
import { Spinner } from "./ui";

/** How the service's game list resolved for the game Library is on. */
type Listing =
  | { state: "no-game" }
  | { state: "loading" }
  /** Present on the service, under this slug — which is what filtering uses. */
  | { state: "listed"; slug: string }
  | { state: "absent" }
  /** The lookup itself failed. Not the same as absent, and must not say so. */
  | { state: "unknown" };

export function BrowseScreen({
  signedIn,
  gameId,
  gameName,
  onDownloadStarted,
  onSignIn,
  onError,
}: {
  signedIn: boolean;
  /** The game Library is on. Matched against the service's own slugs. */
  gameId: string | null;
  gameName: string | null;
  /** Handed the started transfer so the Downloads screen updates immediately. */
  onDownloadStarted: (d: import("../lib/api").DownloadView) => void;
  onSignIn: () => void;
  onError: (e: unknown) => void;
}) {
  const [page, setPage] = useState<CatalogPageView | null>(null);
  const [search, setSearch] = useState("");
  const [busy, setBusy] = useState(false);
  const [quota, setQuota] = useState<DownloadQuotaView | null>(null);
  const [listing, setListing] = useState<Listing>({ state: "loading" });
  const requestId = useRef(0);

  // Which mod page is open, and the confirmation waiting on a decision.
  const [viewing, setViewing] = useState<CatalogModView | null>(null);
  const [detail, setDetail] = useState<CatalogModDetailView | null>(null);
  const [pending, setPending] = useState<{
    mod: CatalogModView;
    detail: CatalogModDetailView;
    fileId: string;
    fileName: string;
  } | null>(null);
  const [sending, setSending] = useState(false);
  const [sentFileIds, setSentFileIds] = useState<Set<string>>(new Set());

  const refreshQuota = useCallback(() => {
    if (!signedIn) return;
    api
      .apocryphaDownloadQuota()
      .then(setQuota)
      .catch(() => setQuota(null));
  }, [signedIn]);

  useEffect(() => refreshQuota(), [refreshQuota]);

  // Resolving the game is the first thing, because everything below depends on
  // it: the filter is the service's slug, not this app's id.
  useEffect(() => {
    // Cleared rather than left standing. A previous game's answer describing
    // the new one is worse than saying nothing, and its results must not stay
    // on screen and downloadable while the replacement loads.
    setPage(null);
    setViewing(null);
    setPending(null);

    if (!signedIn) {
      setListing({ state: "loading" });
      return;
    }
    if (!gameId) {
      setListing({ state: "no-game" });
      return;
    }

    let alive = true;
    setListing({ state: "loading" });
    api
      .apocryphaGames()
      .then((games) => {
        if (!alive) return;
        const match = games.find((g) => g.slug === gameId);
        setListing(match ? { state: "listed", slug: match.slug } : { state: "absent" });
      })
      .catch(() => {
        // Unknown, not absent. A failed lookup must never be reported as "this
        // game is not on Apocrypha" — that is a positive claim about a
        // catalogue nobody managed to read.
        if (alive) setListing({ state: "unknown" });
      });
    return () => {
      alive = false;
    };
  }, [signedIn, gameId]);

  const slug = listing.state === "listed" ? listing.slug : null;

  const load = useCallback(
    async (term: string, pageNumber: number, ticket: number) => {
      if (!slug) return;
      setBusy(true);
      try {
        const result = await api.browseApocryphaMods(slug, term || null, pageNumber);
        // Only the newest request may paint. The ticket is taken by the caller,
        // at the moment the intent changed, so a reply for text that has since
        // been retyped is discarded rather than shown.
        if (ticket === requestId.current) setPage(result);
      } catch (e) {
        if (ticket === requestId.current) onError(e);
      } finally {
        if (ticket === requestId.current) setBusy(false);
      }
    },
    [slug, onError],
  );

  // One effect covers the first load and every search. The ticket is claimed
  // here — on the keystroke, not when the timer fires — so a reply already in
  // flight cannot land against text that has since changed.
  useEffect(() => {
    if (!slug) return;
    const ticket = ++requestId.current;
    const delay = search ? 300 : 0;
    const t = setTimeout(() => void load(search, 1, ticket), delay);
    return () => clearTimeout(t);
  }, [search, slug, load]);

  const openMod = useCallback(
    async (mod: CatalogModView) => {
      setViewing(mod);
      setDetail(null);
      try {
        setDetail(await api.apocryphaModDetail(mod.gameSlug, mod.slug));
      } catch (e) {
        onError(e);
        setViewing(null);
      }
    },
    [onError],
  );

  /**
   * Pressing Download on a card, having read nothing.
   *
   * Detail is always fetched fresh rather than reused, because the decision it
   * feeds — which file, and whether to warn — must reflect what the service
   * says now. A cached copy from an earlier press could name a file that is no
   * longer the newest.
   */
  const askToDownload = useCallback(
    async (mod: CatalogModView) => {
      setSending(true);
      try {
        const d = await api.apocryphaModDetail(mod.gameSlug, mod.slug);
        const release = d.versions.find((v) => v.isLatest) ?? d.versions[0] ?? null;
        const usable = (release?.files ?? []).filter(
          (f) =>
            f.scanState.toLowerCase() === "clean" &&
            f.uploadState.toLowerCase() === "verified",
        );

        // More than one file means a choice, and guessing it is how the wrong
        // variant ends up deployed into a game. The mod page makes it.
        if (usable.length !== 1) {
          setViewing(mod);
          setDetail(d);
          return;
        }

        const file = usable[0]!;
        const hasWarnings =
          required(d.relationships).length > 0 || incompatible(d.relationships).length > 0;

        // Nothing to say, so nothing is said. A confirmation that appears every
        // time is one people learn to dismiss without reading.
        if (!hasWarnings) {
          await send(mod, file.id);
          return;
        }

        setPending({
          mod,
          detail: d,
          fileId: file.id,
          fileName: file.displayName?.trim() || file.fileName,
        });
      } catch (e) {
        onError(e);
      } finally {
        setSending(false);
      }
    },
    // `send` is defined below and is stable for the life of the component.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [onError],
  );

  const send = useCallback(
    async (mod: CatalogModView, fileId: string) => {
      setSending(true);
      try {
        const started = await api.apocryphaDownloadFile(mod.gameSlug, mod.slug, fileId);
        // Handed straight to the shell rather than waiting for a progress
        // event, so the Downloads screen is right the moment this returns.
        onDownloadStarted(started);
        setSentFileIds((prev) => new Set(prev).add(fileId));
        setPending(null);
        refreshQuota();
      } catch (e) {
        onError(e);
      } finally {
        setSending(false);
      }
    },
    [onDownloadStarted, onError, refreshQuota],
  );

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

  if (listing.state === "no-game") {
    return (
      <div className="empty">
        <div className="empty-title">Choose a game first</div>
        <div style={{ maxWidth: 460 }}>
          Browse shows the catalogue for the game you are managing. Pick one in
          Library and it will appear here.
        </div>
      </div>
    );
  }

  if (listing.state === "absent" || listing.state === "unknown") {
    return (
      <div className="empty">
        <div className="empty-title">
          {listing.state === "absent"
            ? `${gameName ?? "This game"} is not on Apocrypha`
            : "Could not reach the catalogue"}
        </div>
        <div style={{ maxWidth: 460 }}>
          {listing.state === "absent"
            ? "The service does not list this game, so there is nothing to browse. You can still install mods from a file, or from Nexus."
            : "The service did not answer. It may be updating — try again in a moment."}
        </div>
      </div>
    );
  }

  const items = page?.items ?? [];
  const showing = listing.state === "loading" || (busy && !page);

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

      {showing ? null : items.length === 0 ? (
        <div className="empty">
          <div className="empty-title">
            {search ? "Nothing matched" : "No mods here yet"}
          </div>
          <div style={{ maxWidth: 460 }}>
            {search
              ? "Try fewer words, or a different spelling."
              : "Nothing has been published for this game yet."}
          </div>
        </div>
      ) : (
        <div className="browse-grid">
          {items.map((m) => (
            <BrowseCard
              key={m.id}
              mod={m}
              busy={sending}
              onView={() => void openMod(m)}
              onDownload={() => void askToDownload(m)}
            />
          ))}
        </div>
      )}

      {page && page.total > page.pageSize ? (
        <div className="row" style={{ gap: "var(--sp-3)" }}>
          <span className="mod-meta">
            Page {page.page} of {Math.max(1, Math.ceil(page.total / page.pageSize))} ·{" "}
            {page.total} mods
          </span>
          <button
            className="btn sm"
            style={{ marginLeft: "auto" }}
            disabled={busy || page.page <= 1}
            onClick={() => void load(search, page.page - 1, ++requestId.current)}
          >
            Previous
          </button>
          <button
            className="btn sm"
            disabled={busy || page.page * page.pageSize >= page.total}
            onClick={() => void load(search, page.page + 1, ++requestId.current)}
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

      <AnimatePresence>
        {pending && (
          <DownloadConfirm
            modName={pending.mod.name}
            fileName={pending.fileName}
            required={required(pending.detail.relationships)}
            optional={optional(pending.detail.relationships)}
            incompatible={incompatible(pending.detail.relationships)}
            busy={sending}
            onConfirm={() => void send(pending.mod, pending.fileId)}
            onView={() => {
              setViewing(pending.mod);
              setDetail(pending.detail);
              setPending(null);
            }}
            onCancel={() => setPending(null)}
          />
        )}
      </AnimatePresence>

      <AnimatePresence>
        {viewing && (
          <ModPage
            mod={viewing}
            detail={detail}
            busy={sending}
            downloadedIds={sentFileIds}
            onDownload={(fileId) => void send(viewing, fileId)}
            onClose={() => {
              setViewing(null);
              setDetail(null);
            }}
          />
        )}
      </AnimatePresence>
    </div>
  );
}

function BrowseCard({
  mod,
  busy,
  onView,
  onDownload,
}: {
  mod: CatalogModView;
  busy: boolean;
  onView: () => void;
  onDownload: () => void;
}) {
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
        <button className="btn primary sm" onClick={onDownload} disabled={busy}>
          Download
        </button>
        <button className="btn sm" onClick={onView} disabled={busy}>
          View
        </button>
      </div>
    </article>
  );
}
