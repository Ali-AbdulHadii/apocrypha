/**
 * Who is overwriting whom, worked out from the conflicts the backend already
 * sends.
 *
 * `list_conflicts` answers per file: this path is claimed by these mods, and
 * this one wins. That is the right shape for the panel that pins a single file,
 * and the wrong shape for a list of mods, where the question is "is anything I
 * installed being buried, and by what". Same data, grouped by pair.
 *
 * Kept out of the screen because it is arithmetic with no view in it, and
 * because the counts are the sort of thing that is easy to get subtly wrong: a
 * mod that loses one file to a mod it also beats on three others is both a
 * winner and a loser, and each of those is one entry in one direction.
 */

import type { ConflictView } from "./api";

/** What one mod does and suffers, against the mods it shares files with. */
export interface Tally {
  /** Files this mod wins, by the mod it takes them from. */
  overwrites: Map<string, number>;
  /** Files this mod loses, by the mod that takes them. */
  overwrittenBy: Map<string, number>;
}

export function emptyTally(): Tally {
  return { overwrites: new Map(), overwrittenBy: new Map() };
}

/** Total files won, across every mod contended with. */
export function winCount(tally: Tally | undefined): number {
  if (!tally) return 0;
  return [...tally.overwrites.values()].reduce((n, c) => n + c, 0);
}

/** Total files lost. */
export function lossCount(tally: Tally | undefined): number {
  if (!tally) return 0;
  return [...tally.overwrittenBy.values()].reduce((n, c) => n + c, 0);
}

function bump(map: Map<string, number>, key: string) {
  map.set(key, (map.get(key) ?? 0) + 1);
}

/**
 * One tally per mod that contends for anything.
 *
 * `overrides` is applied first, because a pinned file is won by the mod that was
 * pinned rather than by the one load order would have given it to, and a row
 * claiming to win a file it has been pinned out of would be a lie the user
 * themselves created.
 */
export function conflictTallies(
  conflicts: ConflictView[],
  overrides: Record<string, string> = {},
): Map<string, Tally> {
  const out = new Map<string, Tally>();
  const of = (id: string) => {
    let t = out.get(id);
    if (!t) {
      t = emptyTally();
      out.set(id, t);
    }
    return t;
  };

  for (const conflict of conflicts) {
    const winner = overrides[conflict.path] ?? conflict.winner;
    for (const loser of conflict.contenders) {
      if (loser === winner) continue;
      bump(of(winner).overwrites, loser);
      bump(of(loser).overwrittenBy, winner);
    }
  }
  return out;
}
