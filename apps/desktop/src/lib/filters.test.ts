/**
 * Tests for the stored mod-list filters.
 *
 * This module reads a blob somebody else wrote — a previous build of this app,
 * a future one, or a person with the developer tools open — and every field it
 * hands back reaches either a comparison or a list of rows. So the tests are
 * mostly about what it does with input it did not produce, which is the part
 * that fails silently: a sort key this build does not know does not throw, it
 * just quietly matches nothing and the user sees an empty library.
 *
 * `localStorage` is stubbed rather than brought in with a DOM implementation.
 * The module touches `getItem` and `setItem` and nothing else, so a map behind
 * those two names is the whole dependency, and keeping the environment `node`
 * means the `typeof localStorage === "undefined"` guard is still a real branch
 * somewhere rather than something no test could ever reach.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  DEFAULT_CRITERIA,
  MAX_SAVED,
  isDefault,
  loadLast,
  loadSaved,
  putSaved,
  removeSaved,
  sameCriteria,
  saveLast,
  type Criteria,
} from "./filters";

const KEY = "apocrypha.modFilters";

/** The two methods the module uses, over a plain map. */
function installStore(): Map<string, string> {
  const backing = new Map<string, string>();
  const stub = {
    getItem: (k: string) => backing.get(k) ?? null,
    setItem: (k: string, v: string) => void backing.set(k, v),
  };
  vi.stubGlobal("localStorage", stub);
  return backing;
}

let store: Map<string, string>;

beforeEach(() => {
  store = installStore();
});

/** A criteria that is recognisably not the default, for round trips. */
const WEAPONS: Criteria = {
  query: "sword",
  status: "disabled",
  category: "Weapons",
  sort: "name",
};

describe("what was stored last", () => {
  it("comes back for the game that stored it", () => {
    saveLast("monster-hunter-wilds", WEAPONS);
    expect(loadLast("monster-hunter-wilds")).toEqual(WEAPONS);
  });

  it("does not come back for a different game", () => {
    // Categories are derived from the installed mods, so "Weapons" means
    // nothing in a game that has no such category and restoring it there
    // would show an empty list with no visible cause.
    saveLast("monster-hunter-wilds", WEAPONS);
    expect(loadLast("cyberpunk-2077")).toEqual(DEFAULT_CRITERIA);
  });

  it("is the default before any game is chosen", () => {
    expect(loadLast(null)).toEqual(DEFAULT_CRITERIA);
    // And writing under no game writes nothing at all, rather than inventing a
    // "null" slot that a real game could later collide with.
    saveLast(null, WEAPONS);
    expect(store.get(KEY)).toBeUndefined();
  });
});

describe("input this build did not write", () => {
  it("falls back on a sort key it does not know", () => {
    // The one that matters. `colour` would reach the sort comparison and
    // select nothing, and an empty list reads as "the filter is broken"
    // rather than "the stored value is from another build".
    store.set(
      KEY,
      JSON.stringify({ "monster-hunter-wilds": { last: { sort: "colour" } } }),
    );
    expect(loadLast("monster-hunter-wilds").sort).toBe(DEFAULT_CRITERIA.sort);
  });

  it("falls back on a status it does not know", () => {
    store.set(
      KEY,
      JSON.stringify({ "monster-hunter-wilds": { last: { status: "broken" } } }),
    );
    expect(loadLast("monster-hunter-wilds").status).toBe(
      DEFAULT_CRITERIA.status,
    );
  });

  it("keeps the fields it does recognise from a partly wrong record", () => {
    // Coercion is field by field, so one bad value costs that field and not
    // the whole filter.
    store.set(
      KEY,
      JSON.stringify({
        "monster-hunter-wilds": {
          last: { query: "sword", status: "disabled", sort: 42 },
        },
      }),
    );
    expect(loadLast("monster-hunter-wilds")).toEqual({
      query: "sword",
      status: "disabled",
      category: DEFAULT_CRITERIA.category,
      sort: DEFAULT_CRITERIA.sort,
    });
  });

  it("gives defaults for a blob that is not JSON at all", () => {
    store.set(KEY, "{ this is not json");
    expect(loadLast("monster-hunter-wilds")).toEqual(DEFAULT_CRITERIA);
    expect(loadSaved("monster-hunter-wilds")).toEqual([]);
  });

  it("gives defaults for JSON that is not an object", () => {
    store.set(KEY, JSON.stringify(["monster-hunter-wilds"]));
    expect(loadLast("monster-hunter-wilds")).toEqual(DEFAULT_CRITERIA);
  });

  it("drops saved entries with no usable name", () => {
    store.set(
      KEY,
      JSON.stringify({
        "monster-hunter-wilds": {
          saved: [null, { criteria: WEAPONS }, { name: "Kept", criteria: {} }],
        },
      }),
    );
    const saved = loadSaved("monster-hunter-wilds");
    expect(saved.map((f) => f.name)).toEqual(["Kept"]);
    expect(saved[0].criteria).toEqual(DEFAULT_CRITERIA);
  });

  it("survives a store that refuses to be written to", () => {
    // A full or disabled store is not worth interrupting anyone over: the
    // filters simply do not persist and the app keeps working.
    vi.stubGlobal("localStorage", {
      getItem: () => null,
      setItem: () => {
        throw new Error("QuotaExceededError");
      },
    });
    expect(() => saveLast("monster-hunter-wilds", WEAPONS)).not.toThrow();
  });
});

describe("named filters", () => {
  it("are kept per game", () => {
    putSaved("monster-hunter-wilds", "Turned off", WEAPONS);
    expect(loadSaved("monster-hunter-wilds").map((f) => f.name)).toEqual([
      "Turned off",
    ]);
    expect(loadSaved("cyberpunk-2077")).toEqual([]);
  });

  it("replace a same-name filter rather than duplicating it", () => {
    // Two entries with one name are indistinguishable in the toolbar, and the
    // second is always the one that was meant.
    putSaved("monster-hunter-wilds", "Mine", DEFAULT_CRITERIA);
    const saved = putSaved("monster-hunter-wilds", "mine", WEAPONS);
    expect(saved).toHaveLength(1);
    expect(saved[0].name).toBe("mine");
    expect(saved[0].criteria).toEqual(WEAPONS);
  });

  it("ignore a name that is only whitespace", () => {
    putSaved("monster-hunter-wilds", "   ", WEAPONS);
    expect(loadSaved("monster-hunter-wilds")).toEqual([]);
  });

  it("trim the name they are given", () => {
    const saved = putSaved("monster-hunter-wilds", "  Armour  ", WEAPONS);
    expect(saved[0].name).toBe("Armour");
  });

  it("stop at MAX_SAVED, keeping the most recent", () => {
    for (let i = 0; i <= MAX_SAVED; i += 1) {
      putSaved("monster-hunter-wilds", `filter-${i}`, WEAPONS);
    }
    const saved = loadSaved("monster-hunter-wilds");
    expect(saved).toHaveLength(MAX_SAVED);
    expect(saved[0].name).toBe("filter-1");
    expect(saved[MAX_SAVED - 1].name).toBe(`filter-${MAX_SAVED}`);
  });

  it("are removed by name, and only from their own game", () => {
    putSaved("monster-hunter-wilds", "Shared", WEAPONS);
    putSaved("cyberpunk-2077", "Shared", WEAPONS);

    expect(removeSaved("monster-hunter-wilds", "Shared")).toEqual([]);
    expect(loadSaved("cyberpunk-2077").map((f) => f.name)).toEqual(["Shared"]);
  });

  it("do not disturb the last criteria when one is saved", () => {
    saveLast("monster-hunter-wilds", WEAPONS);
    putSaved("monster-hunter-wilds", "Armour", DEFAULT_CRITERIA);
    expect(loadLast("monster-hunter-wilds")).toEqual(WEAPONS);
  });
});

describe("the two comparisons the toolbar asks", () => {
  it("treats a whitespace-only query as no filter", () => {
    // Otherwise "clear" appears for a search of three spaces, which filters
    // nothing and looks like a bug.
    expect(isDefault({ ...DEFAULT_CRITERIA, query: "   " })).toBe(true);
    expect(isDefault({ ...DEFAULT_CRITERIA, query: "sword" })).toBe(false);
    expect(isDefault(WEAPONS)).toBe(false);
  });

  it("compares criteria field by field", () => {
    expect(sameCriteria(WEAPONS, { ...WEAPONS })).toBe(true);
    expect(sameCriteria(WEAPONS, { ...WEAPONS, sort: "size" })).toBe(false);
  });
});
