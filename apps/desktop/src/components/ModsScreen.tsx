/**
 * Mod list.
 *
 * Built for large libraries: mods group into collapsible categories, can be
 * searched and filtered, and carry an explicit applied/pending badge so it is
 * obvious what is actually in the game right now.
 *
 * Two decisions keep a few hundred mods smooth here.
 *
 * 1. Load order is not edited on this screen. It has its own screen now, so
 *    this one never mounts drag machinery. Rows still show their position,
 *    because knowing where a mod sits is useful even where you cannot move it.
 *
 * 2. Past a threshold each category body is windowed: only the rows near the
 *    scroll viewport are in the DOM and a plain spacer stands in for the rest.
 *    Row height is measured from a real row rather than written down here,
 *    because spacing and text size are user settings and any baked in number
 *    would be wrong the moment somebody changes them.
 */

import { AnimatePresence, motion } from "framer-motion";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { formatBytes, type ModView } from "../lib/api";
import { Icon } from "./icons";
import { Chip, Switch } from "./ui";

export type SortKey = "order" | "name" | "size" | "added";

export interface ModsScreenProps {
  mods: ModView[];
  /** Ids of mods whose files are currently in the game folder. */
  appliedIds: Set<string>;
  /** True when enabled mods differ from what is deployed. */
  dirty: boolean;
  onToggle: (mod: ModView, enabled: boolean) => void;
  onConfigure: (mod: ModView) => void;
  onRemove: (mod: ModView) => void;
  onImport: () => void;
  /** Takes the user to the Load order screen, where the order is editable. */
  onOpenLoadOrder?: () => void;
}

/** Below this many rows, windowing costs more than it saves. */
const VIRTUALISE_ABOVE = 60;
/** Rows kept mounted past each edge of the viewport, so scrolling is not bare. */
const OVERSCAN_ROWS = 6;
/** How many rows a group renders before it has measured anything. */
const SEED_ROWS = 24;

function categoryOf(m: ModView): string {
  return m.category?.trim() || "Uncategorised";
}

/* --------------------------------------------------------------- motion --- */

/**
 * Read a duration token, in seconds.
 *
 * framer-motion needs a number, so this value cannot live in CSS alone. Reading
 * the token instead of writing 0.2 in here means the Appearance settings still
 * reach this animation, and the reduced motion preference still switches it off.
 */
function readDuration(): number {
  const raw = getComputedStyle(document.documentElement)
    .getPropertyValue("--dur")
    .trim();
  const n = Number.parseFloat(raw);
  if (!Number.isFinite(n)) return 0.2;
  return raw.endsWith("ms") ? n / 1000 : n;
}

function useCollapseDuration(): number {
  const [seconds, setSeconds] = useState(0.2);

  useEffect(() => {
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)");
    const sync = () => setSeconds(reduced.matches ? 0 : readDuration());
    sync();
    reduced.addEventListener("change", sync);
    return () => reduced.removeEventListener("change", sync);
  }, []);

  return seconds;
}

/* ------------------------------------------------------------ windowing --- */

interface RowWindow {
  /** First row index in the DOM. */
  start: number;
  /** One past the last row index in the DOM. */
  end: number;
  /** Height of the spacer standing in for the rows above, in pixels. */
  padTop: number;
  /** Height of the spacer standing in for the rows below, in pixels. */
  padBottom: number;
}

function sameWindow(a: RowWindow, b: RowWindow): boolean {
  return (
    a.start === b.start &&
    a.end === b.end &&
    a.padTop === b.padTop &&
    a.padBottom === b.padBottom
  );
}

/**
 * Nearest scrolling ancestor.
 *
 * The scroll container belongs to the app shell, not to this screen, so it is
 * found by walking up and asking the browser rather than by naming a class this
 * file does not own. Elements that clip without scrolling are skipped on
 * purpose: the group card and the collapse wrapper both set overflow hidden and
 * neither of them is the scroller.
 */
function findScroller(from: HTMLElement | null): HTMLElement | null {
  let el = from?.parentElement ?? null;
  while (el) {
    const flow = getComputedStyle(el).overflowY;
    if (flow === "auto" || flow === "scroll" || flow === "overlay") return el;
    el = el.parentElement;
  }
  return null;
}

/**
 * Keep only the rows near the viewport mounted.
 *
 * Row height is never assumed. Every pass measures a row that is genuinely on
 * screen and reads the gap straight off the list container, so retuning the
 * spacing or text size tokens while the list is open simply produces a
 * different pitch on the next pass. To guarantee there is always something to
 * measure, a group that has scrolled out of sight still keeps exactly one row
 * mounted: it costs nothing and it doubles as the measuring stick.
 */
function useRowWindow(count: number, enabled: boolean) {
  const [bodyEl, setBodyEl] = useState<HTMLDivElement | null>(null);
  const [win, setWin] = useState<RowWindow>(() => ({
    start: 0,
    end: enabled ? Math.min(count, SEED_ROWS) : count,
    padTop: 0,
    padBottom: 0,
  }));

  const scrollerRef = useRef<HTMLElement | null>(null);
  const frameRef = useRef(0);

  const recompute = useCallback(() => {
    const whole: RowWindow = { start: 0, end: count, padTop: 0, padBottom: 0 };

    if (!enabled || count === 0) {
      if (!sameWindow(win, whole)) setWin(whole);
      return;
    }
    if (!bodyEl) return;

    if (!scrollerRef.current) scrollerRef.current = findScroller(bodyEl);
    const scroller = scrollerRef.current;
    // Nothing scrolls above this list, so there is no window to compute.
    if (!scroller) {
      if (!sameWindow(win, whole)) setWin(whole);
      return;
    }

    const first = bodyEl.querySelector<HTMLElement>(".mod-row");
    if (!first) {
      // No row to measure. This is the first paint, and also the recovery path
      // for when a filter shrinks the list past the current window.
      const seed: RowWindow = {
        start: 0,
        end: Math.min(count, SEED_ROWS),
        padTop: 0,
        padBottom: 0,
      };
      if (!sameWindow(win, seed)) setWin(seed);
      return;
    }

    const box = first.getBoundingClientRect();
    if (box.height <= 0) return;
    const gap = Number.parseFloat(getComputedStyle(bodyEl).rowGap) || 0;
    const pitch = box.height + gap;

    // Anchor on the row that is mounted and step back over the rows the top
    // spacer stands in for. Working from a real row means this never has to
    // know the group's own padding, and it stays correct while a category is
    // mid collapse.
    const viewTop = scroller.getBoundingClientRect().top;
    const zero = box.top - win.start * pitch - viewTop;
    const overscan = OVERSCAN_ROWS * pitch;

    let start = Math.floor((-zero - overscan) / pitch);
    let end = Math.ceil((scroller.clientHeight - zero + overscan) / pitch);
    start = Math.min(Math.max(start, 0), count - 1);
    end = Math.min(Math.max(end, start + 1), count);

    const tail = count - end;
    const next: RowWindow = {
      start,
      end,
      padTop: start > 0 ? Math.round(start * pitch - gap) : 0,
      padBottom: tail > 0 ? Math.round(tail * pitch - gap) : 0,
    };
    if (!sameWindow(win, next)) setWin(next);
  }, [bodyEl, count, enabled, win]);

  // Verify the window after every render, before paint, so a filter, a sort or
  // a category opening never leaves a stale slice on screen for a frame.
  const latest = useRef(recompute);
  useLayoutEffect(() => {
    latest.current = recompute;
    recompute();
  });

  useEffect(() => {
    if (!enabled || !bodyEl) return;
    scrollerRef.current = findScroller(bodyEl);
    const scroller = scrollerRef.current;

    const schedule = () => {
      if (frameRef.current) return;
      frameRef.current = requestAnimationFrame(() => {
        frameRef.current = 0;
        latest.current();
      });
    };

    scroller?.addEventListener("scroll", schedule, { passive: true });
    window.addEventListener("resize", schedule);

    // The group body is watched as well as the viewport. It changes height
    // while a category opens or closes, and it changes height if the spacing
    // tokens are retuned while the list is open, which is exactly the moment
    // the row height needs taking again.
    const observer = new ResizeObserver(schedule);
    observer.observe(bodyEl);
    if (scroller) observer.observe(scroller);

    return () => {
      scroller?.removeEventListener("scroll", schedule);
      window.removeEventListener("resize", schedule);
      observer.disconnect();
      if (frameRef.current) cancelAnimationFrame(frameRef.current);
      frameRef.current = 0;
    };
  }, [bodyEl, enabled]);

  return { setBodyEl, win };
}

/* ------------------------------------------------------------- the list --- */

export function ModsScreen({
  mods,
  appliedIds,
  dirty,
  onToggle,
  onConfigure,
  onRemove,
  onImport,
  onOpenLoadOrder,
}: ModsScreenProps) {
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<SortKey>("order");
  const [status, setStatus] = useState<"all" | "enabled" | "disabled">("all");
  const [category, setCategory] = useState("all");
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const duration = useCollapseDuration();

  const categories = useMemo(() => {
    const set = new Set(mods.map(categoryOf));
    return ["all", ...[...set].sort((a, b) => a.localeCompare(b))];
  }, [mods]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    let list = mods.filter((m) => {
      if (status === "enabled" && !m.enabled) return false;
      if (status === "disabled" && m.enabled) return false;
      if (category !== "all" && categoryOf(m) !== category) return false;
      if (!q) return true;
      return (
        m.name.toLowerCase().includes(q) ||
        (m.author ?? "").toLowerCase().includes(q) ||
        (m.category ?? "").toLowerCase().includes(q) ||
        m.installerModel.toLowerCase().includes(q)
      );
    });

    if (sort === "name") {
      list = [...list].sort((a, b) => a.name.localeCompare(b.name));
    } else if (sort === "size") {
      list = [...list].sort((a, b) => b.totalBytes - a.totalBytes);
    } else if (sort === "added") {
      list = [...list].sort((a, b) => b.addedAt - a.addedAt);
    } else {
      list = [...list].sort((a, b) => a.priority - b.priority);
    }
    return list;
  }, [mods, query, sort, status, category]);

  const grouped = useMemo(() => {
    const map = new Map<string, ModView[]>();
    for (const m of filtered) {
      const c = categoryOf(m);
      if (!map.has(c)) map.set(c, []);
      map.get(c)!.push(m);
    }
    return [...map.entries()].sort((a, b) => a[0].localeCompare(b[0]));
  }, [filtered]);

  /**
   * Windowing is decided on the whole visible list, not per category. What
   * costs frames is the total number of mounted rows, and ten categories of
   * fifty is the same problem as one of five hundred.
   */
  const virtualise = filtered.length > VIRTUALISE_ABOVE;

  function toggleCategory(name: string) {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  }

  if (mods.length === 0) {
    return (
      <div className="empty">
        <span className="empty-icon">
          <Icon.package size={40} strokeWidth={1} />
        </span>
        <div className="empty-title">No mods yet</div>
        <div>
          Add a ZIP archive to begin. Segmented installers, loose files, loaders
          and PAK mods are all supported.
        </div>
        <button
          className="btn primary"
          onClick={onImport}
          style={{ marginTop: "var(--sp-3)" }}
        >
          <Icon.plus /> Add mod
        </button>
      </div>
    );
  }

  return (
    <div className="stack">
      <div className="toolbar">
        <div className="search">
          <span className="search-icon">
            <Icon.search size={14} />
          </span>
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search mods by name, author, or category"
            aria-label="Search mods"
          />
        </div>

        <select
          className="select"
          value={status}
          onChange={(e) => setStatus(e.target.value as typeof status)}
          aria-label="Filter by state"
        >
          <option value="all">All states</option>
          <option value="enabled">Enabled</option>
          <option value="disabled">Disabled</option>
        </select>

        <select
          className="select"
          value={category}
          onChange={(e) => setCategory(e.target.value)}
          aria-label="Filter by category"
        >
          {categories.map((c) => (
            <option key={c} value={c}>
              {c === "all" ? "All categories" : c}
            </option>
          ))}
        </select>

        <select
          className="select"
          value={sort}
          onChange={(e) => setSort(e.target.value as SortKey)}
          aria-label="Sort mods"
        >
          <option value="order">Load order</option>
          <option value="name">Name</option>
          <option value="size">Size</option>
          <option value="added">Recently added</option>
        </select>
      </div>

      {sort === "order" && onOpenLoadOrder && (
        <div className="row">
          <span className="card-hint">
            The number on each row is the position the game loads that mod in.
            You set the order on its own screen, where searching and filtering
            here cannot get in the way.
          </span>
          <button
            className="btn sm"
            onClick={onOpenLoadOrder}
            style={{ marginLeft: "auto", flexShrink: 0 }}
          >
            <Icon.grip size={14} /> Open load order
          </button>
        </div>
      )}

      {filtered.length === 0 ? (
        <div className="empty">
          <span className="empty-icon">
            <Icon.search size={32} strokeWidth={1} />
          </span>
          <div className="empty-title">Nothing matches</div>
          <div>Try a different search or clear the filters.</div>
        </div>
      ) : (
        grouped.map(([name, items]) => (
          <ModGroup
            key={name}
            name={name}
            items={items}
            open={!collapsed.has(name)}
            virtualise={virtualise}
            duration={duration}
            appliedIds={appliedIds}
            dirty={dirty}
            onHeadClick={() => toggleCategory(name)}
            onToggle={onToggle}
            onConfigure={onConfigure}
            onRemove={onRemove}
          />
        ))
      )}
    </div>
  );
}

/* ------------------------------------------------------------ one group --- */

function ModGroup({
  name,
  items,
  open,
  virtualise,
  duration,
  appliedIds,
  dirty,
  onHeadClick,
  onToggle,
  onConfigure,
  onRemove,
}: {
  name: string;
  items: ModView[];
  open: boolean;
  virtualise: boolean;
  duration: number;
  appliedIds: Set<string>;
  dirty: boolean;
  onHeadClick: () => void;
  onToggle: (m: ModView, enabled: boolean) => void;
  onConfigure: (m: ModView) => void;
  onRemove: (m: ModView) => void;
}) {
  const { setBodyEl, win } = useRowWindow(items.length, virtualise);
  const enabledCount = items.filter((m) => m.enabled).length;
  const slice = virtualise ? items.slice(win.start, win.end) : items;

  return (
    <section className="mod-group">
      <button
        className="mod-group-head"
        onClick={onHeadClick}
        aria-expanded={open}
      >
        <span className={`chevron ${open ? "open" : ""}`}>
          <Icon.chevronRight size={14} />
        </span>
        <span className="mod-group-name">{name}</span>
        <span className="mod-group-count">
          {enabledCount} of {items.length} enabled
        </span>
      </button>

      <AnimatePresence initial={false}>
        {open && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ duration, ease: [0.16, 1, 0.3, 1] }}
            style={{ overflow: "hidden" }}
          >
            {/* The spacers give the body the height it would have had with
                every row mounted, so the height animation and the scrollbar
                both behave exactly as they did before windowing. */}
            <div className="mod-group-body" ref={setBodyEl}>
              {win.padTop > 0 && (
                <div
                  className="mod-rows-spacer"
                  style={{ height: win.padTop, flexShrink: 0 }}
                  aria-hidden="true"
                />
              )}

              {slice.map((m) => (
                <ModRow
                  key={m.id}
                  mod={m}
                  applied={appliedIds.has(m.id)}
                  dirty={dirty}
                  onToggle={onToggle}
                  onConfigure={onConfigure}
                  onRemove={onRemove}
                />
              ))}

              {win.padBottom > 0 && (
                <div
                  className="mod-rows-spacer"
                  style={{ height: win.padBottom, flexShrink: 0 }}
                  aria-hidden="true"
                />
              )}
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </section>
  );
}

/* -------------------------------------------------------------- one row --- */

function StatusChip({ mod, applied }: { mod: ModView; applied: boolean }) {
  if (!mod.enabled) {
    return applied ? <Chip kind="warn">Still in game</Chip> : <Chip>Off</Chip>;
  }
  return applied ? (
    <Chip kind="ok">
      <span className="dot" /> In game
    </Chip>
  ) : (
    <Chip kind="warn">Not applied</Chip>
  );
}

function ModRow({
  mod,
  applied,
  onToggle,
  onConfigure,
  onRemove,
}: {
  mod: ModView;
  applied: boolean;
  dirty: boolean;
  onToggle: (m: ModView, enabled: boolean) => void;
  onConfigure: (m: ModView) => void;
  onRemove: (m: ModView) => void;
}) {
  return (
    <div className={`mod-row ${mod.enabled ? "" : "disabled"}`}>
      <span className="mod-order" title="Load order position">
        {mod.priority}
      </span>

      <Switch
        checked={mod.enabled}
        onChange={(v) => onToggle(mod, v)}
        label={`Enable ${mod.name}`}
      />

      <div style={{ minWidth: 0, flex: 1 }}>
        <div className="row" style={{ gap: "var(--sp-3)" }}>
          <span className="mod-name truncate">{mod.name}</span>
          {mod.version && <Chip>{mod.version}</Chip>}
          <StatusChip mod={mod} applied={applied} />
        </div>
        <div className="mod-meta">
          {/* Middots are written as the character itself. A \u escape is only
              processed inside a string; as bare JSX text it renders as the
              six literal characters. */}
          {mod.author ? `${mod.author} · ` : ""}
          {mod.selection.length} of{" "}
          {mod.groups.reduce((n, g) => n + g.options.length, 0)} options ·{" "}
          {mod.totalFiles} files · {formatBytes(mod.totalBytes)}
        </div>
      </div>

      <button className="btn sm" onClick={() => onConfigure(mod)}>
        Configure
      </button>
      <button
        className="btn sm icon ghost"
        onClick={() => onRemove(mod)}
        aria-label={`Remove ${mod.name}`}
        title="Remove from library"
      >
        <Icon.trash size={14} />
      </button>
    </div>
  );
}
