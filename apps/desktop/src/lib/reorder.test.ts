import { describe, expect, it } from "vitest";
import { inferSubject, moveFromReorder, type Placed } from "./reorder";

/** A is the first mod and belongs to group 1; the rest are loose. */
function rows(): Map<string, Placed> {
  return new Map<string, Placed>([
    ["A", { id: "A", groupId: 1 }],
    ["B", { id: "B", groupId: null }],
    ["C", { id: "C", groupId: null }],
    ["D", { id: "D", groupId: null }],
  ]);
}

const BEFORE = ["A", "B", "C", "D"];

/** Replay a move the way `apply_move` does, so the order can be asserted. */
function replay(before: string[], mv: NonNullable<ReturnType<typeof moveFromReorder>>) {
  const rest = before.filter((id) => id !== mv.subject.id);
  const at =
    mv.placement.at === "start"
      ? 0
      : rest.indexOf((mv.placement as { anchor: string }).anchor) + 1;
  return [...rest.slice(0, at), mv.subject.id as string, ...rest.slice(at)];
}

describe("the move a drag describes", () => {
  /**
   * The two gestures this module exists for. Both turn `A B C D` into
   * `B A C D`, so the sequence alone cannot tell them apart, and reading it as
   * the other one made the top of the list refuse every drag across it: the
   * subject came out as the grouped mod A, and the belonging as "leave".
   */
  it("names the row that was dragged, not the row it displaced", () => {
    const next = ["B", "A", "C", "D"];

    const draggedDown = moveFromReorder(BEFORE, next, rows(), "A")!;
    expect(draggedDown.subject).toEqual({ kind: "mod", id: "A" });
    expect(draggedDown.placement).toEqual({ at: "after", anchor: "B" });

    const draggedUp = moveFromReorder(BEFORE, next, rows(), "B")!;
    expect(draggedUp.subject).toEqual({ kind: "mod", id: "B" });
    expect(draggedUp.placement).toEqual({ at: "start" });
  });

  it("leaves a bystander's group alone when something is dragged past it", () => {
    // Dragging loose B above grouped A says nothing about A's membership, and
    // used to say A had left group 1 — refused outright when 1 is locked.
    const mv = moveFromReorder(BEFORE, ["B", "A", "C", "D"], rows(), "B")!;
    expect(mv.belonging).toEqual({ kind: "keep" });
  });

  it("keeps a grouped mod in its group when it is the one that moved", () => {
    // A is alone in group 1 here, so moving it down is not a departure.
    const mv = moveFromReorder(BEFORE, ["B", "A", "C", "D"], rows(), "A")!;
    expect(mv.belonging).toEqual({ kind: "leave" });
  });

  it("joins a mod that lands strictly between two members of one group", () => {
    const grouped = new Map<string, Placed>([
      ["A", { id: "A", groupId: 1 }],
      ["B", { id: "B", groupId: 1 }],
      ["C", { id: "C", groupId: null }],
    ]);
    const mv = moveFromReorder(["A", "B", "C"], ["A", "C", "B"], grouped, "C")!;
    expect(mv.belonging).toEqual({ kind: "join", groupId: 1 });
  });

  it("lands a row dropped at the top at the start rather than after anything", () => {
    const mv = moveFromReorder(BEFORE, ["D", "A", "B", "C"], rows(), "D")!;
    expect(mv.placement).toEqual({ at: "start" });
  });

  it("says nothing happened when nothing moved", () => {
    expect(moveFromReorder(BEFORE, [...BEFORE], rows(), "A")).toBeNull();
  });

  /**
   * Every single-row move, replayed. This is the property the screen actually
   * needs — that what the person sees after the drop is what they dragged —
   * and it held even while the subject was being misidentified, which is why
   * the fault showed up as a refusal rather than as a scrambled list.
   */
  it("reproduces the order the person dropped, for every move", () => {
    for (let from = 0; from < BEFORE.length; from++) {
      for (let to = 0; to < BEFORE.length; to++) {
        if (from === to) continue;
        const want = [...BEFORE];
        const [held] = want.splice(from, 1);
        want.splice(to, 0, held!);
        const mv = moveFromReorder(BEFORE, want, rows(), BEFORE[from]);
        expect(replay(BEFORE, mv!), `${BEFORE[from]} ${from}->${to}`).toEqual(want);
      }
    }
  });
});

describe("inferring the subject without a gesture", () => {
  it("is right whenever the move is longer than one row", () => {
    expect(inferSubject(BEFORE, ["B", "C", "A", "D"])).toBe("A");
    expect(inferSubject(BEFORE, ["C", "A", "B", "D"])).toBe("C");
    expect(inferSubject(BEFORE, ["D", "A", "B", "C"])).toBe("D");
  });

  it("cannot tell a swap of neighbours apart, and picks the upper row", () => {
    // Recorded rather than asserted as desirable. It is why callers with a
    // gesture behind them pass the subject in.
    expect(inferSubject(BEFORE, ["B", "A", "C", "D"])).toBe("A");
  });

  it("is null when the sequence is unchanged", () => {
    expect(inferSubject(BEFORE, [...BEFORE])).toBeNull();
  });
});
