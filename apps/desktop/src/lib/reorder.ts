/**
 * Turning a finished drag into the move it meant.
 *
 * `Reorder` hands back the whole visible sequence, and the sequence alone
 * cannot say what happened. Swapping two neighbours produces one array for two
 * different gestures: dragging the upper row down past the lower one, and
 * dragging the lower row up past the upper one, both turn `A B` into `B A`.
 *
 * That ambiguity is not cosmetic, because the move carries a `belonging` worked
 * out from the rows either side of where the subject landed. Guess the subject
 * wrong and the answer is about the wrong mod: dragging a loose row above the
 * first mod of a group was read as that grouped mod moving down and *leaving*
 * its group — refused outright when the group is locked, and undone by the
 * backend's regathering when it is not. Either way the row appeared not to
 * move, which is what the top of the list did for every drag across it.
 *
 * So the subject is passed in, from the drag that knows it, and inferred only
 * when there is nothing to pass — a keyboard nudge builds the sequence itself
 * and names the row directly. The inference is kept for that case and for any
 * caller that has no gesture behind it, and it is right whenever the move is
 * longer than one row, which is the only case where the sequence is unambiguous.
 */

import type { Belonging, OrderMove, Placement } from "./api";

/** The parts of a mod this needs: identity, and what group it is in. */
export interface Placed {
  id: string;
  groupId?: number | null;
}

/**
 * Which id moved, given the sequence before and after.
 *
 * Ambiguous by construction for a swap of neighbours — see above — so this is a
 * fallback rather than the answer. Returns null when nothing moved.
 */
export function inferSubject(
  before: readonly string[],
  after: readonly string[],
): string | null {
  const at = after.findIndex((id, i) => id !== before[i]);
  if (at < 0) return null;

  // Two rows differ after any swap. The one that moved is whichever of them is
  // not simply the other one displaced.
  const candidate = after[at];
  return before.indexOf(candidate) === at + 1 ? (before[at] ?? null) : candidate;
}

/**
 * The move a reordered sequence describes, or null if it describes nothing.
 *
 * `subjectId` is the row the person actually dragged. Pass it whenever a
 * gesture is behind the call; without it the subject is inferred, with the
 * ambiguity that implies.
 */
export function moveFromReorder(
  before: readonly string[],
  after: readonly string[],
  rows: ReadonlyMap<string, Placed>,
  subjectId?: string,
): OrderMove | null {
  // Checked before the subject is resolved, not after. Reorder reports the
  // sequence while the pointer is still down, and reports it unchanged as often
  // as not; a named subject would otherwise turn every one of those into a move
  // that says the list should stay exactly as it is.
  if (
    before.length === after.length &&
    before.every((id, i) => id === after[i])
  ) {
    return null;
  }

  const moved =
    subjectId && after.includes(subjectId)
      ? subjectId
      : inferSubject(before, after);
  if (!moved) return null;

  const to = after.indexOf(moved);
  if (to < 0) return null;

  const subject = rows.get(moved);
  if (!subject) return null;

  const above = to > 0 ? rows.get(after[to - 1]!) : undefined;
  const below = to + 1 < after.length ? rows.get(after[to + 1]!) : undefined;

  // Strictly between two members of one group is the only way in. Landing *on*
  // the boundary is deliberately "out": joining is the change that surprises
  // people, so it takes the unambiguous gesture.
  const inside =
    above?.groupId != null && above.groupId === below?.groupId
      ? above.groupId
      : null;

  const belonging: Belonging =
    inside != null
      ? subject.groupId === inside
        ? { kind: "keep" }
        : { kind: "join", groupId: inside }
      : subject.groupId != null
        ? { kind: "leave" }
        : { kind: "keep" };

  const placement: Placement = above
    ? { at: "after", anchor: above.id }
    : { at: "start" };

  return { subject: { kind: "mod", id: moved }, placement, belonging };
}
