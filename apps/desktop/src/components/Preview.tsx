/**
 * Option preview images.
 *
 * Three things matter here:
 *   1. Images are never cropped. They are contained inside a fixed box and
 *      letterboxed, and clicking opens a full-size lightbox.
 *   2. Fetches are cached per option for the process lifetime, so paging back
 *      and forth through wizard steps costs nothing.
 *   3. The next step is prefetched while the current one is on screen, which is
 *      what removes the hitch when moving between steps.
 */

import { AnimatePresence, motion } from "framer-motion";
import { createContext, useContext, useEffect, useState, type ReactNode } from "react";
import { fetchPreview, type PreviewSource } from "../lib/api";

const cache = new Map<string, string | null>();
const inflight = new Map<string, Promise<string | null>>();

function keyFor(source: PreviewSource, optionId: string): string {
  return source.kind === "archive"
    ? `a:${source.archivePath}:${optionId}`
    : `m:${source.gameId}:${source.modId}:${optionId}`;
}

function load(source: PreviewSource, optionId: string): Promise<string | null> {
  const key = keyFor(source, optionId);
  if (cache.has(key)) return Promise.resolve(cache.get(key) ?? null);
  const existing = inflight.get(key);
  if (existing) return existing;

  const p = fetchPreview(source, optionId)
    .then((uri) => {
      cache.set(key, uri);
      return uri;
    })
    .catch(() => {
      cache.set(key, null);
      return null;
    })
    .finally(() => inflight.delete(key));

  inflight.set(key, p);
  return p;
}

/** Warm the cache for a set of options without rendering them. */
export function prefetchPreviews(source: PreviewSource | null, optionIds: string[]) {
  if (!source) return;
  for (const id of optionIds) void load(source, id);
}

export function isPreviewCached(source: PreviewSource | null, optionId: string) {
  return source ? cache.has(keyFor(source, optionId)) : false;
}

/* ------------------------------------------------------------- lightbox -- */

type LightboxValue = { open: (uri: string, caption: string) => void };
const LightboxContext = createContext<LightboxValue | null>(null);

export function LightboxProvider({ children }: { children: ReactNode }) {
  const [shown, setShown] = useState<{ uri: string; caption: string } | null>(null);

  useEffect(() => {
    if (!shown) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setShown(null);
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [shown]);

  return (
    <LightboxContext.Provider
      value={{ open: (uri, caption) => setShown({ uri, caption }) }}
    >
      {children}
      <AnimatePresence>
        {shown && (
          <motion.div
            className="lightbox"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.16 }}
            onClick={() => setShown(null)}
          >
            <motion.img
              src={shown.uri}
              alt={shown.caption}
              initial={{ scale: 0.98, opacity: 0 }}
              animate={{ scale: 1, opacity: 1 }}
              exit={{ scale: 0.99, opacity: 0 }}
              transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
            />
            <span className="lightbox-caption">{shown.caption}</span>
          </motion.div>
        )}
      </AnimatePresence>
    </LightboxContext.Provider>
  );
}

function useLightbox() {
  return useContext(LightboxContext);
}

/* ---------------------------------------------------------------- views -- */

function usePreview(source: PreviewSource | null, optionId: string, enabled: boolean) {
  const key = source ? keyFor(source, optionId) : "";
  const [uri, setUri] = useState<string | null>(() =>
    key && cache.has(key) ? (cache.get(key) ?? null) : null,
  );
  const [settled, setSettled] = useState<boolean>(() => Boolean(key) && cache.has(key));

  useEffect(() => {
    if (!enabled || !source) return;
    let alive = true;
    load(source, optionId).then((u) => {
      if (!alive) return;
      setUri(u);
      setSettled(true);
    });
    return () => {
      alive = false;
    };
  }, [enabled, source, optionId]);

  return { uri, settled };
}

export function OptionThumb({
  source,
  optionId,
  enabled,
  alt,
}: {
  source: PreviewSource | null;
  optionId: string;
  enabled: boolean;
  alt: string;
}) {
  const { uri, settled } = usePreview(source, optionId, enabled);
  const lightbox = useLightbox();

  if (!enabled) return null;

  return (
    <span
      className="option-thumb"
      onClick={(e) => {
        if (!uri) return;
        e.stopPropagation();
        e.preventDefault();
        lightbox?.open(uri, alt);
      }}
    >
      {uri ? (
        <>
          <motion.img
            src={uri}
            alt={alt}
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            transition={{ duration: 0.2 }}
          />
          <span className="zoom-hint">Click to enlarge</span>
        </>
      ) : (
        <span className={settled ? "thumb-empty" : "skeleton thumb-empty"} />
      )}
    </span>
  );
}

/** Wide image used for cover art and warning notices. */
export function PreviewHero({
  source,
  optionId,
  alt,
}: {
  source: PreviewSource | null;
  optionId: string;
  alt: string;
}) {
  const { uri, settled } = usePreview(source, optionId, true);
  const lightbox = useLightbox();

  return (
    <div
      className="preview-hero"
      onClick={() => uri && lightbox?.open(uri, alt)}
      style={!uri ? { cursor: "default" } : undefined}
    >
      {uri ? (
        <motion.img
          src={uri}
          alt={alt}
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ duration: 0.24, ease: [0.16, 1, 0.3, 1] }}
        />
      ) : (
        <div
          className={settled ? "" : "skeleton"}
          style={{ width: "100%", height: 256 }}
        />
      )}
    </div>
  );
}
