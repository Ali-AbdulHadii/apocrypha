import { describe, expect, it } from "vitest";
import type { ConflictView } from "./api";
import { conflictTallies, lossCount, winCount } from "./conflicts";

function conflict(
  path: string,
  contenders: string[],
  winner: string,
): ConflictView {
  return { path, contenders, winner };
}

describe("who overwrites whom", () => {
  it("counts one file as a win for the winner and a loss for everyone else", () => {
    const t = conflictTallies([conflict("a.dds", ["one", "two"], "two")]);
    expect(t.get("two")!.overwrites.get("one")).toBe(1);
    expect(t.get("one")!.overwrittenBy.get("two")).toBe(1);
    expect(winCount(t.get("two"))).toBe(1);
    expect(lossCount(t.get("one"))).toBe(1);
  });

  it("counts a three way contest as two losses against the one winner", () => {
    const t = conflictTallies([
      conflict("a.dds", ["one", "two", "three"], "three"),
    ]);
    expect(winCount(t.get("three"))).toBe(2);
    expect(lossCount(t.get("one"))).toBe(1);
    expect(lossCount(t.get("two"))).toBe(1);
  });

  it("lets a mod be both a winner and a loser without netting them off", () => {
    // The pair matters, not the total. A mod that beats one thing and loses to
    // another has two facts about it, and a single number would hide one.
    const t = conflictTallies([
      conflict("a.dds", ["one", "two"], "two"),
      conflict("b.dds", ["two", "three"], "three"),
    ]);
    expect(winCount(t.get("two"))).toBe(1);
    expect(lossCount(t.get("two"))).toBe(1);
  });

  it("gives the file to the mod it was pinned to, not to load order", () => {
    // A pinned file is won by the mod that was pinned. A row claiming to win a
    // file it has been pinned out of would be a lie the user created.
    const t = conflictTallies([conflict("a.dds", ["one", "two"], "two")], {
      "a.dds": "one",
    });
    expect(winCount(t.get("one"))).toBe(1);
    expect(lossCount(t.get("two"))).toBe(1);
  });

  it("has nothing to say about a mod that shares no files", () => {
    const t = conflictTallies([]);
    expect(t.size).toBe(0);
    expect(winCount(t.get("one"))).toBe(0);
    expect(lossCount(undefined)).toBe(0);
  });
});
