/**
 * The mod list, which is also the load order.
 *
 * These were two screens. The library searched, filtered and grouped but could
 * not be rearranged; the load order could be rearranged but showed nothing but
 * a name, and only the mods that were switched on. The split was deliberate and
 * its reasoning is worth writing down because it was not wrong: an unfiltered
 * list is always draggable, so keeping the order flat meant never having to say
 * "clear your filters before you can drag".
 *
 * What changed is the answer to that problem. A drop no longer means "put this
 * at row seven", which is meaningless when eleven rows between six and seven are
 * filtered out. It means "put this after that mod", which means the same thing
 * in every view, so the list can be filtered and draggable at once. With that
 * gone the split was costing two places to look for one mod and two
 * implementations of the same search.
 *
 * Four things keep a few hundred mods smooth and legible here.
 *
 * 1. **Sections are groups, not categories.** A category is a fact about a mod
 *    and belongs in the filter, where it now lives. A section is a decision
 *    somebody made about their order, so it can be named, coloured, locked, and
 *    dragged as a block.
 *
 * 2. **A locked group refuses the drag before it starts**, and the store refuses
 *    it again if anything gets past. The second refusal is the real one.
 *
 * 3. **Dragging needs load order sort.** Sorted by name, a drop has nothing to
 *    write, so the grips go away and say why rather than lying about it.
 *
 * 4. Past a threshold the rows are windowed: only those near the viewport are in
 *    the DOM and a plain spacer stands in for the rest. Row height is measured
 *    from a real row rather than written down here, because spacing and text
 *    size are user settings and any baked in number would be wrong the moment
 *    somebody changes them.
 *
 * Archives can also be dropped straight onto this screen. The zone is drawn
 * only while a drag is actually over the window, so a full library is not
 * paying permanent space for it; the empty state shows it outright, because
 * there is nothing else on screen and it is the fastest way to explain what to
 * do next. A dropped archive takes exactly the same path as Add mod.
 */

import {
  AnimatePresence,
  motion,
  Reorder,
  useDragControls,
  useReducedMotion,
} from "framer-motion";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  formatBytes,
  truncatePath,
  type ConflictView,
  type ModGroupView,
  type ModView,
  type OrderMove,
} from "../lib/api";
import {
  conflictTallies,
  lossCount,
  winCount,
  type Tally,
} from "../lib/conflicts";
import { useFileDrop } from "../lib/drop";
import {
  DEFAULT_CRITERIA,
  isDefault,
  loadLast,
  loadSaved,
  putSaved,
  removeSaved,
  sameCriteria,
  saveLast,
  type Criteria,
  type SavedFilter,
  type SortKey,
} from "../lib/filters";
import { Icon } from "./icons";
import { Checkbox, Chip, Switch } from "./ui";

export type { SortKey };

export interface ModsScreenProps {
  mods: ModView[];
  /** The groups in this profile, in the order their blocks appear. */
  groups: ModGroupView[];
  /** Recomputed after every reorder: who claims each contested file. */
  conflicts: ConflictView[];
  /** Game-relative path to the mod id pinned to win it. */
  overrides: Record<string, string>;
  /** Ids of mods whose files are currently in the game folder. */
  appliedIds: Set<string>;
  /** True when enabled mods differ from what is deployed. */
  dirty: boolean;
  /** Which game's filters to remember. Null before one is chosen. */
  gameId: string | null;
  busy?: boolean;
  onToggle: (mod: ModView, enabled: boolean) => void;
  /** Enable or disable a selection in one transaction. */
  onToggleMany: (ids: string[], enabled: boolean) => void;
  onConfigure: (mod: ModView) => void;
  onRemove: (mod: ModView) => void;
  onImport: () => void;
  /** One drag, anchored to the row it landed against. */
  onMove: (move: OrderMove) => void;
  onCreateGroup: (name: string) => void;
  onRenameGroup: (groupId: number, name: string) => void;
  onCollapseGroup: (groupId: number, collapsed: boolean) => void;
  onLockGroup: (groupId: number, locked: boolean) => void;
  onDeleteGroup: (groupId: number) => void;
  onAssignToGroup: (groupId: number | null, modIds: string[]) => void;
  onOverride: (path: string, modId: string) => void;
  onClearOverride: (path: string) => void;
  /**
   * An archive dropped on the window, by path. Given every dropped path in the
   * order the OS listed them; what to do with more than one is decided there,
   * alongside the rest of the install flow, rather than here.
   */
  onDropArchives?: (paths: string[]) => void;
  /** False while something else already owns the install flow. */
  canDrop?: boolean;
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

/* ------------------------------------------------------------ drop zone --- */

/**
 * The dashed square.
 *
 * One component for both places it appears, so the thing that lights up under a
 * drag is visibly the same thing the empty state was showing all along.
 */
function DropZone({ over }: { over: boolean }) {
  return (
    <div
      className={`dropzone ${over ? "over" : ""}`}
      aria-hidden={!over}
      role={over ? "status" : undefined}
    >
      <span className="dropzone-icon">
        <Icon.package size={40} strokeWidth={1} />
      </span>
      <span className="dropzone-title">Drop mod here to install</span>
      <span className="dropzone-hint">ZIP, 7z and RAR archives</span>
    </div>
  );
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
  groups,
  conflicts,
  overrides,
  appliedIds,
  dirty,
  gameId,
  busy = false,
  onToggle,
  onToggleMany,
  onConfigure,
  onRemove,
  onImport,
  onMove,
  onCreateGroup,
  onRenameGroup,
  onCollapseGroup,
  onLockGroup,
  onDeleteGroup,
  onAssignToGroup,
  onOverride,
  onClearOverride,
  onDropArchives,
  canDrop = true,
}: ModsScreenProps) {
  /**
   * The criteria, and the game they were loaded for, as one value.
   *
   * They travel together because they are only meaningful together. On the
   * render where `gameId` changes, this still holds the previous game's
   * criteria, and a save effect that could not tell would write one game's
   * filters into another game's slot. It would usually be corrected a tick
   * later, which is exactly the kind of bug that survives testing and then
   * bites the one time the screen unmounts in between.
   *
   * Seeded from storage rather than defaulted and then corrected, so the first
   * render is already the filtered list. Restoring in an effect would show the
   * whole library for a frame and then snap.
   */
  const [filters, setFilters] = useState<{ game: string | null; criteria: Criteria }>(
    () => ({ game: gameId, criteria: loadLast(gameId) }),
  );
  const criteria = filters.criteria;
  const [saved, setSaved] = useState<SavedFilter[]>(() => loadSaved(gameId));
  const [naming, setNaming] = useState(false);
  const [draftName, setDraftName] = useState("");
  const {
    query,
    sort,
    status,
    category,
    group,
    conflicts: conflictFilter,
  } = criteria;

  const setCriteria = useCallback(
    (next: Criteria) => setFilters((prev) => ({ ...prev, criteria: next })),
    [],
  );

  const [selected, setSelected] = useState<Set<string>>(new Set());
  /** Read out after a keyboard move, which is otherwise silent. */
  const [announcement, setAnnouncement] = useState("");
  const reduceMotion = useReducedMotion();
  /** Where the last checkbox click landed, so shift-click has a range to span. */
  const anchorRef = useRef<string | null>(null);
  /** Set by Escape, so a commit-on-blur knows the naming was abandoned. */
  const cancelledRef = useRef(false);
  const duration = useCollapseDuration();

  const { over } = useFileDrop(canDrop && !!onDropArchives, (paths) =>
    onDropArchives?.(paths),
  );

  const patch = useCallback(
    (part: Partial<Criteria>) =>
      setFilters((prev) => ({
        ...prev,
        criteria: { ...prev.criteria, ...part },
      })),
    [],
  );

  // Switching games must not carry one game's categories into another's list.
  useEffect(() => {
    setFilters({ game: gameId, criteria: loadLast(gameId) });
    setSaved(loadSaved(gameId));
    setSelected(new Set());
    anchorRef.current = null;
  }, [gameId]);

  useEffect(() => {
    // Skipped on the render where the game has changed but the criteria have
    // not caught up yet. The effect above is what makes them agree, and this
    // one then runs again with both halves describing the same game.
    if (filters.game !== gameId) return;
    saveLast(gameId, filters.criteria);
  }, [gameId, filters]);

  const categories = useMemo(() => {
    const set = new Set(mods.map(categoryOf));
    return ["all", ...[...set].sort((a, b) => a.localeCompare(b))];
  }, [mods]);

  /** Who overwrites whom, from the conflicts already fetched for this order. */
  const tallies = useMemo(
    () => conflictTallies(conflicts, overrides),
    [conflicts, overrides],
  );

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    let list = mods.filter((m) => {
      if (status === "enabled" && !m.enabled) return false;
      if (status === "disabled" && m.enabled) return false;
      if (category !== "all" && categoryOf(m) !== category) return false;
      if (group !== "all") {
        const want = group === "none" ? null : Number(group);
        if ((m.groupId ?? null) !== want) return false;
      }
      if (conflictFilter !== "all") {
        const t = tallies.get(m.id);
        const wins = (t?.overwrites.size ?? 0) > 0;
        const loses = (t?.overwrittenBy.size ?? 0) > 0;
        if (conflictFilter === "overwriting" && !wins) return false;
        if (conflictFilter === "overwritten" && !loses) return false;
        if (conflictFilter === "clean" && (wins || loses)) return false;
      }
      if (!q) return true;
      return (
        m.name.toLowerCase().includes(q) ||
        (m.author ?? "").toLowerCase().includes(q) ||
        (m.category ?? "").toLowerCase().includes(q) ||
        String(m.nexusModId ?? "").includes(q) ||
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
      // Ties broken by id, which is how the store breaks them too. It sorts by
      // `priority, mod_id` while the rows arrive ordered by import date, so
      // where priorities are equal the two disagreed about what the order even
      // was, and the first drag on such a profile rearranged everything rather
      // than the one mod that was dragged. Equal priorities are the normal
      // state of a library nobody has ordered yet.
      list = [...list].sort(
        (a, b) => a.priority - b.priority || a.id.localeCompare(b.id),
      );
    }
    return list;
  }, [mods, query, sort, status, category, group, conflictFilter, tallies]);

  /**
   * Only load order is an order. Any other sort is a view of the same mods, and
   * a drop in it would have no position to write.
   */
  const canDrag = sort === "order" && !busy;

  /**
   * The list as sections: a run of mods with no group, then a group with its
   * members, and so on, in the order the mods themselves are in.
   *
   * Built from the filtered list rather than from `groups`, so a group whose
   * every member is filtered out simply is not drawn, and a group is never shown
   * as empty when it is only hidden.
   */
  const sections = useMemo(() => {
    const byId = new Map(groups.map((g) => [g.id, g]));
    const out: { group: ModGroupView | null; items: ModView[] }[] = [];
    for (const m of filtered) {
      const g = m.groupId != null ? (byId.get(m.groupId) ?? null) : null;
      const last = out[out.length - 1];
      if (last && (last.group?.id ?? null) === (g?.id ?? null)) last.items.push(m);
      else out.push({ group: g, items: [m] });
    }
    return out;
  }, [filtered, groups]);

  /**
   * Windowing is decided on the whole visible list, not per category. What
   * costs frames is the total number of mounted rows, and ten categories of
   * fifty is the same problem as one of five hundred.
   */
  const virtualise = filtered.length > VIRTUALISE_ABOVE;

  /**
   * Selection is held as ids over `filtered`, never read back from the DOM.
   * Past the windowing threshold most rows are genuinely not mounted, so
   * anything that counted checkboxes on screen would be counting a viewport.
   *
   * It is also pruned to what is currently visible: filtering to Weapons,
   * selecting six, then filtering to Armour must not leave six mods armed for
   * a bulk action the user can no longer see.
   */
  useEffect(() => {
    setSelected((prev) => {
      if (prev.size === 0) return prev;
      const visible = new Set(filtered.map((m) => m.id));
      const next = new Set([...prev].filter((id) => visible.has(id)));
      return next.size === prev.size ? prev : next;
    });
  }, [filtered]);

  const selectedIds = useMemo(() => [...selected], [selected]);
  const allVisibleSelected =
    filtered.length > 0 && selected.size === filtered.length;

  /**
   * Click selects one; shift-click selects everything between this row and the
   * last one clicked. The range is taken from `filtered`, the whole list in the
   * order it is displayed, so a range can span rows scrolled out of the window.
   */
  const selectRow = useCallback(
    (mod: ModView, on: boolean, shiftKey: boolean) => {
      setSelected((prev) => {
        const next = new Set(prev);
        const anchor = anchorRef.current;
        if (shiftKey && anchor) {
          const from = filtered.findIndex((m) => m.id === anchor);
          const to = filtered.findIndex((m) => m.id === mod.id);
          if (from !== -1 && to !== -1) {
            const [lo, hi] = from < to ? [from, to] : [to, from];
            for (let i = lo; i <= hi; i++) {
              if (on) next.add(filtered[i].id);
              else next.delete(filtered[i].id);
            }
            return next;
          }
        }
        if (on) next.add(mod.id);
        else next.delete(mod.id);
        return next;
      });
      anchorRef.current = mod.id;
    },
    [filtered],
  );

  const clearSelection = useCallback(() => {
    setSelected(new Set());
    anchorRef.current = null;
  }, []);

  const nameById = useMemo(
    () => new Map(mods.map((m) => [m.id, m.name])),
    [mods],
  );

  /** A mod's index in the visible list, which is what the arrows move within. */
  const indexOf = useCallback(
    (id: string) => filtered.findIndex((m) => m.id === id),
    [filtered],
  );

  /**
   * Turn what the list looks like after a drag into what the person did.
   *
   * Reorder hands back the whole visible sequence, which is not what gets sent:
   * the visible sequence is a subset when anything is filtered, so a position in
   * it names nothing on the other side. What is sent is the mod that moved and
   * the row it came to rest under, both of which mean the same thing in a
   * filtered list as in a whole one.
   *
   * Group membership is read from the rows either side of where it landed. A mod
   * that comes to rest strictly between two members of one group has been put in
   * that group; anywhere else, it has been taken out of whatever it was in.
   * Dropping *onto* the boundary is deliberately "out": joining is the change
   * that surprises people, so it takes the unambiguous gesture.
   */
  const onDrop = useCallback(
    (nextVisible: string[]) => {
      const before = filtered.map((m) => m.id);
      const at = nextVisible.findIndex((id, i) => id !== before[i]);
      if (at < 0) return;

      // Two rows differ after any swap. The one that moved is whichever of them
      // is not simply the other one displaced.
      const candidate = nextVisible[at];
      const moved =
        before.indexOf(candidate) === at + 1 ? before[at] : candidate;
      const to = nextVisible.indexOf(moved);
      if (to < 0) return;

      const byId = new Map(mods.map((m) => [m.id, m]));
      const subject = byId.get(moved);
      if (!subject) return;

      const above = to > 0 ? byId.get(nextVisible[to - 1]) : undefined;
      const below =
        to + 1 < nextVisible.length ? byId.get(nextVisible[to + 1]) : undefined;
      const inside =
        above?.groupId != null && above.groupId === below?.groupId
          ? above.groupId
          : null;

      const belonging: OrderMove["belonging"] =
        inside != null
          ? subject.groupId === inside
            ? { kind: "keep" }
            : { kind: "join", groupId: inside }
          : subject.groupId != null
            ? { kind: "leave" }
            : { kind: "keep" };

      onMove({
        subject: { kind: "mod", id: moved },
        placement: above
          ? { at: "after", anchor: above.id }
          : { at: "start" },
        belonging,
      });
      setAnnouncement(
        `${subject.name} moved to position ${to + 1} of ${nextVisible.length}.`,
      );
    },
    [filtered, mods, onMove],
  );

  /** Move one mod one place, for anybody who cannot drag. */
  const nudge = useCallback(
    (index: number, delta: number) => {
      const target = index + delta;
      if (target < 0 || target >= filtered.length) return;
      const next = filtered.map((m) => m.id);
      const [held] = next.splice(index, 1);
      next.splice(target, 0, held);
      onDrop(next);
    },
    [filtered, onDrop],
  );

  /** Move a whole group above or below the section next to it. */
  const nudgeGroup = useCallback(
    (groupId: number, delta: number) => {
      const index = sections.findIndex((s) => s.group?.id === groupId);
      const neighbour = sections[index + delta];
      if (index < 0 || !neighbour) return;
      onMove({
        subject: { kind: "group", id: groupId },
        placement:
          delta < 0
            ? { at: "before", anchor: neighbour.items[0].id }
            : { at: "after", anchor: neighbour.items[neighbour.items.length - 1].id },
        belonging: { kind: "keep" },
      });
    },
    [sections, onMove],
  );

  function applyBulk(enabled: boolean) {
    onToggleMany(selectedIds, enabled);
    clearSelection();
  }

  function applySaved(f: SavedFilter) {
    setCriteria(f.criteria);
  }

  /**
   * Naming is committed on blur, so that clicking away keeps what was typed
   * rather than discarding it. Escape has to be able to mean *no*, and removing
   * a focused input is exactly the case where whether `blur` arrives is a
   * matter of which browser is asked — so the cancel is recorded rather than
   * inferred from the order the two events happen to fire in.
   */
  function cancelName() {
    cancelledRef.current = true;
    setDraftName("");
    setNaming(false);
  }

  function commitName() {
    if (cancelledRef.current) {
      cancelledRef.current = false;
      return;
    }
    const name = draftName.trim();
    if (name) setSaved(putSaved(gameId, name, criteria));
    setDraftName("");
    setNaming(false);
  }

  function assignSelection(groupId: number | null) {
    onAssignToGroup(groupId, selectedIds);
    clearSelection();
  }

  if (mods.length === 0) {
    return (
      <div className="empty">
        <DropZone over={over} />
        <div className="empty-title">No mods yet</div>
        <div>
          Drag an archive onto the window, or add one below. Segmented
          installers, loose files, loaders and PAK mods are all supported.
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
            onChange={(e) => patch({ query: e.target.value })}
            placeholder="Search mods by name, author, or category"
            aria-label="Search mods"
          />
        </div>

        <select
          className="select"
          value={status}
          onChange={(e) => patch({ status: e.target.value as typeof status })}
          aria-label="Filter by state"
        >
          <option value="all">All states</option>
          <option value="enabled">Enabled</option>
          <option value="disabled">Disabled</option>
        </select>

        <select
          className="select"
          value={category}
          onChange={(e) => patch({ category: e.target.value })}
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
          value={group}
          onChange={(e) => patch({ group: e.target.value })}
          aria-label="Filter by group"
        >
          <option value="all">All groups</option>
          <option value="none">Ungrouped</option>
          {groups.map((g) => (
            <option key={g.id} value={String(g.id)}>
              {g.name}
            </option>
          ))}
        </select>

        <select
          className="select"
          value={conflictFilter}
          onChange={(e) =>
            patch({ conflicts: e.target.value as Criteria["conflicts"] })
          }
          aria-label="Filter by file conflicts"
        >
          <option value="all">Any conflicts</option>
          <option value="overwriting">Overwriting something</option>
          <option value="overwritten">Being overwritten</option>
          <option value="clean">Sharing no files</option>
        </select>

        <select
          className="select"
          value={sort}
          onChange={(e) => patch({ sort: e.target.value as SortKey })}
          aria-label="Sort mods"
        >
          <option value="order">Load order</option>
          <option value="name">Name</option>
          <option value="size">Size</option>
          <option value="added">Recently added</option>
        </select>

        <button
          className="btn sm"
          onClick={() => onCreateGroup("New group")}
          title="Make a new group at the end of the order"
        >
          <Icon.plus size={14} /> Group
        </button>
      </div>

      {/* Saved filters. Hidden entirely when there is nothing saved and nothing
          to save, so a small library never grows a row it has no use for. */}
      {(saved.length > 0 || !isDefault(criteria)) && (
        <div className="row filter-bar">
          {saved.map((f) => {
            const active = sameCriteria(f.criteria, criteria);
            return (
              <span key={f.name} className="saved-filter" data-active={active}>
                <button
                  className="btn sm ghost"
                  onClick={() => applySaved(f)}
                  aria-pressed={active}
                  title={`Apply the "${f.name}" filter`}
                >
                  {f.name}
                </button>
                <button
                  className="btn sm icon ghost"
                  onClick={() => setSaved(removeSaved(gameId, f.name))}
                  aria-label={`Delete the "${f.name}" filter`}
                  title="Delete this filter"
                >
                  <Icon.close size={12} />
                </button>
              </span>
            );
          })}

          {naming ? (
            <input
              className="input sm"
              autoFocus
              value={draftName}
              onChange={(e) => setDraftName(e.target.value)}
              onBlur={commitName}
              onKeyDown={(e) => {
                if (e.key === "Enter") commitName();
                if (e.key === "Escape") cancelName();
              }}
              placeholder="Name this filter"
              aria-label="Name for the saved filter"
            />
          ) : (
            !isDefault(criteria) && (
              <button
                className="btn sm ghost"
                onClick={() => {
                  cancelledRef.current = false;
                  setNaming(true);
                }}
              >
                <Icon.plus size={12} /> Save filter
              </button>
            )
          )}

          {!isDefault(criteria) && (
            <button
              className="btn sm ghost"
              style={{ marginLeft: "auto" }}
              onClick={() => setCriteria(DEFAULT_CRITERIA)}
            >
              Clear filters
            </button>
          )}
        </div>
      )}

      {/* The bulk bar appears only with a selection, so the list is unchanged
          for anyone not using it. */}
      {selected.size > 0 && (
        <div className="row bulk-bar" role="region" aria-label="Selected mods">
          <Checkbox
            checked={allVisibleSelected}
            indeterminate={!allVisibleSelected}
            onChange={(on) =>
              on ? setSelected(new Set(filtered.map((m) => m.id))) : clearSelection()
            }
            label={
              allVisibleSelected
                ? "Deselect all shown mods"
                : "Select all shown mods"
            }
          />
          <span className="mod-meta">
            {selected.size} of {filtered.length} selected
          </span>
          <button className="btn sm" onClick={() => applyBulk(true)}>
            Enable
          </button>
          <button className="btn sm" onClick={() => applyBulk(false)}>
            Disable
          </button>
          <select
            className="select sm"
            value=""
            onChange={(e) => {
              const v = e.target.value;
              if (v === "") return;
              assignSelection(v === "none" ? null : Number(v));
            }}
            aria-label="Put the selected mods in a group"
          >
            <option value="">Move to group…</option>
            <option value="none">No group</option>
            {groups.map((g) => (
              <option key={g.id} value={String(g.id)} disabled={g.locked}>
                {g.name}
                {g.locked ? " (locked)" : ""}
              </option>
            ))}
          </select>
          <button
            className="btn sm ghost"
            style={{ marginLeft: "auto" }}
            onClick={clearSelection}
          >
            Clear selection
          </button>
        </div>
      )}

      {sort !== "order" && (
        <div className="row order-note">
          <Icon.info size={14} />
          <span className="card-hint">
            Sorted by {sort === "added" ? "date added" : sort}, so there is
            nowhere for a drag to put anything. Switch back to load order to
            rearrange.
          </span>
          <button
            className="btn sm ghost"
            style={{ marginLeft: "auto", flexShrink: 0 }}
            onClick={() => patch({ sort: "order" })}
          >
            Sort by load order
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
        <>
          <div className="order-edge">Loads first</div>
          <Reorder.Group
            axis="y"
            // Ids, not mod objects: Reorder matches values by identity and
            // every state update rebuilds the mod objects, so objects break
            // mid drag.
            values={filtered.map((m) => m.id)}
            onReorder={onDrop}
            className="order-list"
            data-busy={busy}
            as="div"
          >
            {sections.map((section, index) => (
              <GroupSection
                key={section.group?.id ?? `loose-${section.items[0].id}`}
                first={index === 0}
                last={index === sections.length - 1}
                onNudge={nudge}
                onNudgeGroup={nudgeGroup}
                indexOf={indexOf}
                group={section.group}
                items={section.items}
                virtualise={virtualise}
                duration={duration}
                appliedIds={appliedIds}
                dirty={dirty}
                selected={selected}
                tallies={tallies}
                nameById={nameById}
                canDrag={canDrag}
                busy={busy}
                reduceMotion={!!reduceMotion}
                onToggle={onToggle}
                onSelect={selectRow}
                onConfigure={onConfigure}
                onRemove={onRemove}
                onCollapseGroup={onCollapseGroup}
                onRenameGroup={onRenameGroup}
                onLockGroup={onLockGroup}
                onDeleteGroup={onDeleteGroup}
                onUngroup={(ids) => onAssignToGroup(null, ids)}
              />
            ))}
          </Reorder.Group>
          <div className="order-edge">Loads last, wins shared files</div>
        </>
      )}

      <ClaimsPanel
        conflicts={conflicts}
        overrides={overrides}
        nameById={nameById}
        busy={busy}
        onOverride={onOverride}
        onClearOverride={onClearOverride}
      />

      <div className="visually-hidden" role="status" aria-live="polite">
        {announcement}
      </div>

      {/* Drawn over the whole window rather than inside the list, because a
          drag lands wherever the pointer happens to be and the list may well be
          scrolled somewhere else entirely. */}
      <AnimatePresence>
        {over && (
          <motion.div
            className="drop-overlay"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration, ease: [0.16, 1, 0.3, 1] }}
          >
            <motion.div
              initial={{ scale: duration ? 0.96 : 1 }}
              animate={{ scale: 1 }}
              exit={{ scale: duration ? 0.96 : 1 }}
              transition={{ duration, ease: [0.16, 1, 0.3, 1] }}
            >
              <DropZone over />
            </motion.div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

/* ---------------------------------------------------------- one section --- */

const DRAG_SPRING = { type: "spring", stiffness: 600, damping: 40 } as const;

/**
 * One run of the list: either a named group with its members, or the mods that
 * are in no group at all.
 *
 * A section is drawn from the mods, not from the group, so a group whose members
 * are all filtered out is simply absent rather than shown as empty. The header
 * is not a `Reorder.Item`: two nested reorder contexts would fight over the same
 * pointer, so a block moves by its arrows, which also gives it the keyboard.
 */
function GroupSection({
  group,
  items,
  virtualise,
  duration,
  appliedIds,
  dirty,
  selected,
  tallies,
  nameById,
  canDrag,
  busy,
  reduceMotion,
  first,
  last,
  onToggle,
  onSelect,
  onConfigure,
  onRemove,
  onCollapseGroup,
  onRenameGroup,
  onLockGroup,
  onDeleteGroup,
  onUngroup,
  onNudge,
  onNudgeGroup,
  indexOf,
}: {
  group: ModGroupView | null;
  items: ModView[];
  virtualise: boolean;
  duration: number;
  appliedIds: Set<string>;
  dirty: boolean;
  selected: Set<string>;
  tallies: Map<string, Tally>;
  nameById: Map<string, string>;
  canDrag: boolean;
  busy: boolean;
  reduceMotion: boolean;
  first: boolean;
  last: boolean;
  onToggle: (m: ModView, enabled: boolean) => void;
  onSelect: (m: ModView, on: boolean, shiftKey: boolean) => void;
  onConfigure: (m: ModView) => void;
  onRemove: (m: ModView) => void;
  onCollapseGroup: (groupId: number, collapsed: boolean) => void;
  onRenameGroup: (groupId: number, name: string) => void;
  onLockGroup: (groupId: number, locked: boolean) => void;
  onDeleteGroup: (groupId: number) => void;
  onUngroup: (ids: string[]) => void;
  onNudge: (index: number, delta: number) => void;
  onNudgeGroup: (groupId: number, delta: number) => void;
  indexOf: (id: string) => number;
}) {
  const open = !group?.collapsed;
  const { setBodyEl, win } = useRowWindow(items.length, virtualise);
  const slice = virtualise && open ? items.slice(win.start, win.end) : items;
  const enabledCount = items.filter((m) => m.enabled).length;
  const [renaming, setRenaming] = useState(false);
  const [draft, setDraft] = useState("");

  const rows = (
    <div className="mod-group-body" ref={setBodyEl}>
      {open && win.padTop > 0 && (
        <div
          className="mod-rows-spacer"
          style={{ height: win.padTop, flexShrink: 0 }}
          aria-hidden="true"
        />
      )}

      {(open ? slice : []).map((m) => (
        <ModRow
          key={m.id}
          mod={m}
          applied={appliedIds.has(m.id)}
          dirty={dirty}
          selected={selected.has(m.id)}
          tally={tallies.get(m.id)}
          nameById={nameById}
          // A locked group's members are not draggable at all. The store
          // refuses the move as well; this is so nobody spends a gesture
          // finding that out.
          canDrag={canDrag && !group?.locked}
          lockedBy={group?.locked ? group.name : null}
          busy={busy}
          reduceMotion={reduceMotion}
          onToggle={onToggle}
          onSelect={onSelect}
          onConfigure={onConfigure}
          onRemove={onRemove}
          onMoveUp={() => onNudge(indexOf(m.id), -1)}
          onMoveDown={() => onNudge(indexOf(m.id), 1)}
        />
      ))}

      {open && win.padBottom > 0 && (
        <div
          className="mod-rows-spacer"
          style={{ height: win.padBottom, flexShrink: 0 }}
          aria-hidden="true"
        />
      )}
    </div>
  );

  if (!group) return rows;

  return (
    <section
      className="mod-group"
      data-color={group.color}
      data-locked={group.locked}
    >
      <div className="mod-group-head">
        <button
          className="mod-group-toggle"
          onClick={() => onCollapseGroup(group.id, open)}
          aria-expanded={open}
          title={open ? "Collapse this group" : "Expand this group"}
        >
          <span className={`chevron ${open ? "open" : ""}`}>
            <Icon.chevronRight size={14} />
          </span>
        </button>

        {renaming ? (
          <input
            className="input sm"
            autoFocus
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={() => {
              const name = draft.trim();
              if (name) onRenameGroup(group.id, name);
              setRenaming(false);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") e.currentTarget.blur();
              if (e.key === "Escape") setRenaming(false);
            }}
            aria-label={`Rename ${group.name}`}
          />
        ) : (
          <button
            className="mod-group-name"
            onDoubleClick={() => {
              setDraft(group.name);
              setRenaming(true);
            }}
            onClick={() => onCollapseGroup(group.id, open)}
            title="Double click to rename"
          >
            {group.name}
          </button>
        )}

        <span className="mod-group-count">
          {enabledCount} of {items.length} enabled
        </span>

        {group.locked && <Chip kind="accent">Locked</Chip>}

        <div className="mod-group-actions">
          <button
            className="btn sm icon ghost"
            onClick={() => onNudgeGroup(group.id, -1)}
            disabled={first || busy}
            aria-label={`Move ${group.name} up`}
            title="Move this whole group up"
          >
            <span className="order-chev up">
              <Icon.chevronDown size={14} />
            </span>
          </button>
          <button
            className="btn sm icon ghost"
            onClick={() => onNudgeGroup(group.id, 1)}
            disabled={last || busy}
            aria-label={`Move ${group.name} down`}
            title="Move this whole group down"
          >
            <span className="order-chev">
              <Icon.chevronDown size={14} />
            </span>
          </button>
          <button
            className="btn sm icon ghost"
            onClick={() => onLockGroup(group.id, !group.locked)}
            aria-pressed={group.locked}
            disabled={busy}
            aria-label={
              group.locked ? `Unlock ${group.name}` : `Lock ${group.name}`
            }
            title={
              group.locked
                ? "Unlock: these mods can be rearranged again"
                : "Lock: keep these mods together, in this order"
            }
          >
            {group.locked ? <Icon.lock size={14} /> : <Icon.unlock size={14} />}
          </button>
          <button
            className="btn sm icon ghost"
            onClick={() => onUngroup(items.map((m) => m.id))}
            disabled={busy || group.locked}
            aria-label={`Empty ${group.name}`}
            title="Take every mod out of this group, leaving the order alone"
          >
            <Icon.minus size={14} />
          </button>
          <button
            className="btn sm icon ghost"
            onClick={() => onDeleteGroup(group.id)}
            disabled={busy || group.locked}
            aria-label={`Delete ${group.name}`}
            title="Delete the group. Its mods stay where they are."
          >
            <Icon.trash size={14} />
          </button>
        </div>
      </div>

      <AnimatePresence initial={false}>
        {open && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ duration, ease: [0.16, 1, 0.3, 1] }}
            style={{ overflow: "hidden" }}
          >
            {rows}
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

/**
 * What this mod takes from the others, and what they take from it.
 *
 * Two directional arrows rather than one number, because "wins 3, loses 2" reads
 * as a score and the question people actually arrive with is directional: is
 * anything I installed being buried, and by what. The names are in the tooltip
 * because a row cannot hold five of them, and because the count is what gets
 * scanned while the names are what get checked.
 */
function ConflictMarks({
  tally,
  nameById,
}: {
  tally: Tally | undefined;
  nameById: Map<string, string>;
}) {
  const wins = winCount(tally);
  const losses = lossCount(tally);
  if (!tally || (wins === 0 && losses === 0)) {
    return <span className="mod-conflicts" aria-hidden="true" />;
  }
  const names = (m: Map<string, number>) =>
    [...m.entries()]
      .map(([id, n]) => `${nameById.get(id) ?? id} (${n})`)
      .join(", ");

  return (
    <span className="mod-conflicts">
      {wins > 0 && (
        <span
          className="mod-conflict over"
          title={`Overwrites ${names(tally.overwrites)}`}
          aria-label={`Overwrites ${wins} files`}
        >
          <Icon.arrowUp size={12} />
          {wins}
        </span>
      )}
      {losses > 0 && (
        <span
          className="mod-conflict under"
          title={`Overwritten by ${names(tally.overwrittenBy)}`}
          aria-label={`Overwritten in ${losses} files`}
        >
          <Icon.arrowDown size={12} />
          {losses}
        </span>
      )}
    </span>
  );
}

function ModRow({
  mod,
  applied,
  selected,
  tally,
  nameById,
  canDrag,
  lockedBy,
  busy,
  reduceMotion,
  onToggle,
  onSelect,
  onConfigure,
  onRemove,
  onMoveUp,
  onMoveDown,
}: {
  mod: ModView;
  applied: boolean;
  dirty: boolean;
  selected: boolean;
  tally: Tally | undefined;
  nameById: Map<string, string>;
  canDrag: boolean;
  lockedBy: string | null;
  busy: boolean;
  reduceMotion: boolean;
  onToggle: (m: ModView, enabled: boolean) => void;
  onSelect: (m: ModView, on: boolean, shiftKey: boolean) => void;
  onConfigure: (m: ModView) => void;
  onRemove: (m: ModView) => void;
  onMoveUp: () => void;
  onMoveDown: () => void;
}) {
  const controls = useDragControls();

  /**
   * Start a drag from anywhere on the card that is not something you can press.
   *
   * The grip stays, because a grip is how a row says it can be moved at all, and
   * on a row carrying a checkbox, a switch and four buttons it is the only part
   * guaranteed safe to grab. This makes the rest of the card work too, without
   * stealing the clicks that belong to the controls sitting on it.
   */
  const startDrag = (e: React.PointerEvent) => {
    if (!canDrag || busy) return;
    if (
      (e.target as HTMLElement).closest(
        "button, input, select, a, label, [role=switch], [role=checkbox]",
      )
    ) {
      return;
    }
    e.preventDefault();
    controls.start(e);
  };

  // The `mod-row` class stays on this element and the row keeps its height:
  // `useRowWindow` measures a live `.mod-row` to derive the windowing pitch, so
  // moving the class or making rows differ in height breaks scrolling for
  // everyone, not just for anyone using selection.
  return (
    <Reorder.Item
      value={mod.id}
      dragListener={false}
      dragControls={canDrag ? controls : undefined}
      drag={canDrag ? undefined : false}
      className={`mod-row ${mod.enabled ? "" : "disabled"}`}
      data-selected={selected}
      whileDrag={{ scale: 1.01, zIndex: 2 }}
      transition={reduceMotion ? { duration: 0 } : DRAG_SPRING}
      onPointerDown={startDrag}
      as="div"
    >
      <span
        className="drag-handle"
        // touch-action is none in CSS so the browser cannot claim the gesture
        // for scrolling before the drag starts.
        onPointerDown={(e) => {
          if (!canDrag || busy) return;
          e.preventDefault();
          controls.start(e);
        }}
        role="presentation"
        aria-hidden="true"
        data-fixed={!canDrag}
        title={
          lockedBy
            ? `"${lockedBy}" is locked. Unlock it to move this mod.`
            : canDrag
              ? `Drag to move ${mod.name}`
              : "Sort by load order to rearrange"
        }
      >
        <Icon.grip size={16} />
      </span>

      <Checkbox
        checked={selected}
        onChange={(on, ev) => onSelect(mod, on, ev.shiftKey)}
        label={`Select ${mod.name}`}
      />

      <span className="mod-order" title="Load order position">
        {mod.priority}
      </span>

      <Switch
        checked={mod.enabled}
        onChange={(v) => onToggle(mod, v)}
        label={`Enable ${mod.name}`}
      />

      <div className="mod-main">
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

      <ConflictMarks tally={tally} nameById={nameById} />

      <span className="mod-id mono" title="Nexus mod id">
        {mod.nexusModId != null ? `#${mod.nexusModId}` : ""}
      </span>

      <div className="order-move">
        <button
          className="btn sm icon ghost"
          onClick={onMoveUp}
          disabled={!canDrag}
          aria-label={`Move ${mod.name} up`}
          title="Move up, loads earlier"
        >
          <span className="order-chev up">
            <Icon.chevronDown size={14} />
          </span>
        </button>
        <button
          className="btn sm icon ghost"
          onClick={onMoveDown}
          disabled={!canDrag}
          aria-label={`Move ${mod.name} down`}
          title="Move down, wins more files"
        >
          <span className="order-chev">
            <Icon.chevronDown size={14} />
          </span>
        </button>
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
    </Reorder.Item>
  );
}

/* ------------------------------------------------------- contested files --- */

/**
 * The files more than one mod claims, and the pin that settles one.
 *
 * Kept on this screen rather than left behind with the load order screen it came
 * from: load order already decides every one of these, so the place to see the
 * exceptions is the place the order is arranged. Pinning changes who wins one
 * file and nothing else, which is why it is a separate act from moving the mod.
 */
function ClaimsPanel({
  conflicts,
  overrides,
  nameById,
  busy,
  onOverride,
  onClearOverride,
}: {
  conflicts: ConflictView[];
  overrides: Record<string, string>;
  nameById: Map<string, string>;
  busy: boolean;
  onOverride: (path: string, modId: string) => void;
  onClearOverride: (path: string) => void;
}) {
  if (conflicts.length === 0) return null;
  const nameOf = (id: string) => nameById.get(id) ?? id;

  return (
    <section className="card stack">
      <div className="row order-head">
        <div className="order-head-main">
          <div className="card-title">Files claimed by more than one mod</div>
          <div className="card-hint">
            Load order already decides each of these. Pin a different mod to win
            one file: that changes who wins the file and nothing else, and the
            mod itself stays where it is in the order.
          </div>
        </div>
        <Chip>{conflicts.length}</Chip>
      </div>

      <div className="order-claims">
        {conflicts.map((c) => {
          const pinned: string | undefined = overrides[c.path];
          const winner = pinned ?? c.winner;
          // A pin can outlive the mod it names. Keep it in the list so it can
          // be seen and cleared rather than silently disappearing.
          const choices =
            pinned && !c.contenders.includes(pinned)
              ? [...c.contenders, pinned]
              : c.contenders;
          const stale = !!pinned && !nameById.has(pinned);

          return (
            <div className="order-claim" key={c.path}>
              <div className="order-claim-head">
                <span className="mono order-path" title={c.path}>
                  {truncatePath(c.path, 72)}
                </span>
                {pinned && <Chip kind="accent">Pinned</Chip>}
                {stale && <Chip kind="warn">Not in your library</Chip>}
              </div>

              {stale && (
                <div className="order-stale">
                  The pinned mod is no longer installed. Unpin the file to hand
                  it back to load order.
                </div>
              )}

              <div
                className="order-choices"
                role="group"
                aria-label={`Choose which mod wins ${c.path}`}
              >
                {choices.map((id) => {
                  const active = id === winner;
                  return (
                    <button
                      key={id}
                      type="button"
                      className={`order-choice ${active ? "active" : ""}`}
                      aria-pressed={active}
                      disabled={busy}
                      onClick={() => onOverride(c.path, id)}
                    >
                      <span className="order-choice-name truncate">
                        {nameOf(id)}
                      </span>
                      {active && (
                        <span className="order-choice-why">
                          {pinned ? "pinned" : "by load order"}
                        </span>
                      )}
                    </button>
                  );
                })}

                {pinned && (
                  <button
                    type="button"
                    className="btn sm ghost"
                    disabled={busy}
                    onClick={() => onClearOverride(c.path)}
                  >
                    Unpin
                  </button>
                )}
              </div>
            </div>
          );
        })}
      </div>
    </section>
  );
}
