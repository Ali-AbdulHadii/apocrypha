/**
 * Mod-list filters that outlive the screen.
 *
 * Every filter control on the mods screen was component state, and the shell
 * swaps screens by unmounting one and mounting the next. So opening Load order
 * to check something and coming back cleared the search, the category and the
 * sort — a library big enough to need filtering is exactly the one where losing
 * them costs the most.
 *
 * Two things live here. The **last** criteria, restored on mount, which is the
 * fix for that. And **named** filters, which are the feature: "the ones I have
 * turned off", "textures I have not applied yet".
 *
 * Keyed per game. `category` values come from the installed mods, so a filter
 * naming "Weapons" means nothing in a game that has no such category, and
 * restoring it there would show an empty list with no obvious cause.
 *
 * Stored in `localStorage`, following `theme.tsx` and `appearance.tsx`. This is
 * view state: which rows somebody is looking at, not what is installed or what
 * will be deployed. The SQLite `settings` table is the right home for anything
 * that must survive a restore of the data root, but it is reached through
 * narrow typed commands rather than a generic key/value surface, and inventing
 * one for a sort order would be the wrong trade.
 */

export type SortKey = "order" | "name" | "size" | "added";
export type StatusFilter = "all" | "enabled" | "disabled";
/**
 * Whether a mod is taking files from another, losing them to one, or neither.
 *
 * "Which of these is burying my textures" is the question this list is opened
 * with most often, and answering it by reading arrows down four hundred rows is
 * not answering it.
 */
export type ConflictFilter = "all" | "overwriting" | "overwritten" | "clean";

export interface Criteria {
  query: string;
  status: StatusFilter;
  category: string;
  sort: SortKey;
  /** A group id as a string, `"none"` for ungrouped, or `"all"`. */
  group: string;
  conflicts: ConflictFilter;
}

export interface SavedFilter {
  name: string;
  criteria: Criteria;
}

export const DEFAULT_CRITERIA: Criteria = {
  query: "",
  status: "all",
  category: "all",
  sort: "order",
  group: "all",
  conflicts: "all",
};

/** Whether anything is actually being filtered, for a "clear" affordance. */
export function isDefault(c: Criteria): boolean {
  return (
    c.query.trim() === "" &&
    c.status === DEFAULT_CRITERIA.status &&
    c.category === DEFAULT_CRITERIA.category &&
    c.sort === DEFAULT_CRITERIA.sort &&
    c.group === DEFAULT_CRITERIA.group &&
    c.conflicts === DEFAULT_CRITERIA.conflicts
  );
}

export function sameCriteria(a: Criteria, b: Criteria): boolean {
  return (
    a.query === b.query &&
    a.status === b.status &&
    a.category === b.category &&
    a.sort === b.sort &&
    a.group === b.group &&
    a.conflicts === b.conflicts
  );
}

const STORAGE_KEY = "apocrypha.modFilters";
/** Enough to be useful, few enough that the toolbar stays a toolbar. */
export const MAX_SAVED = 12;

interface GameFilters {
  last: Criteria;
  saved: SavedFilter[];
}

type Stored = Record<string, GameFilters>;

/**
 * Read the whole blob.
 *
 * Anything unparseable is discarded rather than repaired. This is a
 * convenience, and a user whose stored filters are corrupt is better served by
 * default filters than by an error about them.
 */
function readAll(): Stored {
  if (typeof localStorage === "undefined") return {};
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== "object") return {};
    return parsed as Stored;
  } catch {
    return {};
  }
}

function writeAll(all: Stored) {
  if (typeof localStorage === "undefined") return;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(all));
  } catch {
    // A full or disabled store is not worth interrupting anyone over: the
    // filters simply do not persist, and everything else still works.
  }
}

/**
 * Coerce whatever was stored into a usable `Criteria`.
 *
 * Fields are checked one by one against the values this build accepts, not
 * trusted because the JSON parsed. A `sort` of `"colour"` — written by a
 * future build, or by hand — would otherwise reach the sort comparison and
 * quietly select nothing.
 */
function coerce(value: unknown): Criteria {
  const c = (value ?? {}) as Partial<Criteria>;
  const sorts: SortKey[] = ["order", "name", "size", "added"];
  const states: StatusFilter[] = ["all", "enabled", "disabled"];
  const conflicts: ConflictFilter[] = [
    "all",
    "overwriting",
    "overwritten",
    "clean",
  ];
  return {
    query: typeof c.query === "string" ? c.query : DEFAULT_CRITERIA.query,
    status:
      typeof c.status === "string" && states.includes(c.status as StatusFilter)
        ? (c.status as StatusFilter)
        : DEFAULT_CRITERIA.status,
    category:
      typeof c.category === "string" ? c.category : DEFAULT_CRITERIA.category,
    sort:
      typeof c.sort === "string" && sorts.includes(c.sort as SortKey)
        ? (c.sort as SortKey)
        : DEFAULT_CRITERIA.sort,
    // Not checked against the groups that exist, because this is read before
    // they are known and a group can be deleted between two launches. A filter
    // naming a group that has gone shows nothing, which the empty state already
    // explains and a clear button already fixes.
    group: typeof c.group === "string" ? c.group : DEFAULT_CRITERIA.group,
    conflicts:
      typeof c.conflicts === "string" &&
      conflicts.includes(c.conflicts as ConflictFilter)
        ? (c.conflicts as ConflictFilter)
        : DEFAULT_CRITERIA.conflicts,
  };
}

function forGame(all: Stored, gameId: string): GameFilters {
  const entry = all[gameId];
  const saved = Array.isArray(entry?.saved) ? entry.saved : [];
  return {
    last: coerce(entry?.last),
    saved: saved
      .filter((f): f is SavedFilter => !!f && typeof f.name === "string")
      .slice(0, MAX_SAVED)
      .map((f) => ({ name: f.name, criteria: coerce(f.criteria) })),
  };
}

export function loadLast(gameId: string | null): Criteria {
  if (!gameId) return DEFAULT_CRITERIA;
  return forGame(readAll(), gameId).last;
}

export function saveLast(gameId: string | null, criteria: Criteria) {
  if (!gameId) return;
  const all = readAll();
  const entry = forGame(all, gameId);
  all[gameId] = { ...entry, last: criteria };
  writeAll(all);
}

export function loadSaved(gameId: string | null): SavedFilter[] {
  if (!gameId) return [];
  return forGame(readAll(), gameId).saved;
}

/**
 * Save under `name`, replacing any filter of the same name.
 *
 * Replacing rather than adding a duplicate, because two entries with one name
 * are indistinguishable in the toolbar and the second is always the one that
 * was meant. Returns the new list so the caller need not re-read.
 */
export function putSaved(
  gameId: string | null,
  name: string,
  criteria: Criteria,
): SavedFilter[] {
  const trimmed = name.trim();
  if (!gameId || !trimmed) return loadSaved(gameId);
  const all = readAll();
  const entry = forGame(all, gameId);
  const kept = entry.saved.filter(
    (f) => f.name.toLowerCase() !== trimmed.toLowerCase(),
  );
  const saved = [...kept, { name: trimmed, criteria }].slice(-MAX_SAVED);
  all[gameId] = { ...entry, saved };
  writeAll(all);
  return saved;
}

export function removeSaved(
  gameId: string | null,
  name: string,
): SavedFilter[] {
  if (!gameId) return [];
  const all = readAll();
  const entry = forGame(all, gameId);
  const saved = entry.saved.filter((f) => f.name !== name);
  all[gameId] = { ...entry, saved };
  writeAll(all);
  return saved;
}
