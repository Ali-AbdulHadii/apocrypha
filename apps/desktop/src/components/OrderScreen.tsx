/**
 * Load order.
 *
 * Mods is a library you browse: it groups, searches and filters. Load order is
 * a single ordered sequence you arrange. Keeping them apart means this list is
 * always flat, always in order, and therefore always draggable, with no "clear
 * your filters before you can drag" nag.
 *
 * The whole screen exists to make one rule obvious: when two mods contain the
 * same file, the one further down the list wins. That rule is stated at the top,
 * marked at both ends of the list, and repeated by the per-mod summary that
 * counts the files each mod wins and loses.
 */

import { Reorder, useDragControls, useReducedMotion } from "framer-motion";
import { useMemo, useState } from "react";
import { truncatePath, type ConflictView, type ModView } from "../lib/api";
import { Icon } from "./icons";
import { Chip } from "./ui";

export interface OrderScreenProps {
  /** Every mod in the library. Only the enabled ones are ordered here. */
  mods: ModView[];
  /** Recomputed by App after every reorder. */
  conflicts: ConflictView[];
  /** Game-relative path to the mod id the user pinned to win it. */
  overrides: Record<string, string>;
  busy: boolean;
  /** Receives the complete order, every mod, not just the enabled ones. */
  onReorder: (orderedIds: string[]) => void;
  onOverride: (path: string, modId: string) => void;
  onClearOverride: (path: string) => void;
}

/**
 * Put the reordered enabled mods back into the slots the enabled mods already
 * occupied, so arranging the visible list never shuffles the disabled mods that
 * are not shown on this screen.
 */
export function spliceEnabled(
  all: ModView[],
  orderedEnabledIds: string[],
): string[] {
  const full = [...all].sort((a, b) => a.priority - b.priority).map((m) => m.id);
  const byId = new Map(all.map((m) => [m.id, m]));

  const slots: number[] = [];
  full.forEach((id, i) => {
    if (byId.get(id)?.enabled) slots.push(i);
  });

  const out = [...full];
  slots.forEach((slot, i) => {
    const id = orderedEnabledIds[i];
    if (id !== undefined) out[slot] = id;
  });
  return out;
}

/** "wins 3, loses 2", or nothing at all when the mod shares no files. */
function verdictLine(wins: number, loses: number): string | null {
  if (wins > 0 && loses > 0) return `wins ${wins}, loses ${loses}`;
  if (wins > 0) return `wins ${wins} ${wins === 1 ? "file" : "files"}`;
  if (loses > 0) return `loses ${loses} ${loses === 1 ? "file" : "files"}`;
  return null;
}

export function OrderScreen({
  mods,
  conflicts,
  overrides,
  busy,
  onReorder,
  onOverride,
  onClearOverride,
}: OrderScreenProps) {
  const reduceMotion = useReducedMotion();
  /** Read out to screen readers after a keyboard move, which is silent. */
  const [announcement, setAnnouncement] = useState("");

  const enabled = useMemo(
    () => mods.filter((m) => m.enabled).sort((a, b) => a.priority - b.priority),
    [mods],
  );

  const nameById = useMemo(
    () => new Map(mods.map((m) => [m.id, m.name])),
    [mods],
  );
  /** A pinned id can name a mod that has since been removed: show the id. */
  const nameOf = (id: string) => nameById.get(id) ?? id;

  /** Who actually wins each contested path once pins are taken into account. */
  const winnerOf = useMemo(() => {
    const map = new Map<string, string>();
    for (const c of conflicts) {
      const pinned: string | undefined = overrides[c.path];
      map.set(c.path, pinned ?? c.winner);
    }
    return map;
  }, [conflicts, overrides]);

  const tally = useMemo(() => {
    const map = new Map<string, { wins: number; loses: number }>();
    for (const c of conflicts) {
      const winner = winnerOf.get(c.path);
      for (const id of c.contenders) {
        const entry = map.get(id) ?? { wins: 0, loses: 0 };
        if (id === winner) entry.wins += 1;
        else entry.loses += 1;
        map.set(id, entry);
      }
    }
    return map;
  }, [conflicts, winnerOf]);

  function apply(orderedEnabledIds: string[]) {
    onReorder(spliceEnabled(mods, orderedEnabledIds));
  }

  function move(index: number, delta: number) {
    const target = index + delta;
    if (target < 0 || target >= enabled.length) return;
    const ids = enabled.map((m) => m.id);
    const held = ids[index];
    ids[index] = ids[target];
    ids[target] = held;
    apply(ids);
    setAnnouncement(
      `${enabled[index].name} moved to position ${target + 1} of ${enabled.length}.`,
    );
  }

  if (enabled.length === 0) {
    return (
      <div className="empty">
        <span className="empty-icon">
          <Icon.mods size={40} strokeWidth={1} />
        </span>
        <div className="empty-title">Nothing to arrange yet</div>
        <div className="order-measure">
          Turn a mod on in Mods and it appears here. Once two of them are on, the
          order matters: the mod further down the list wins any file they share.
        </div>
      </div>
    );
  }

  return (
    <div className="stack">
      <section className="card stack">
        <div className="row order-head">
          <div className="order-head-main">
            <div className="card-title">Further down the list wins</div>
            <div className="card-hint">
              Mods load from the top down. When two mods contain the same file,
              the one further down replaces the ones above it. Drag a mod by its
              grip, or use the arrows to move it one place at a time.
            </div>
          </div>
          <Chip>
            {enabled.length} {enabled.length === 1 ? "mod" : "mods"}
          </Chip>
        </div>

        <div className="order-edge">Loads first</div>

        <Reorder.Group
          axis="y"
          // Ids, not mod objects. Reorder matches values by identity and every
          // state update rebuilds the mod objects, so objects break mid drag.
          values={enabled.map((m) => m.id)}
          onReorder={apply}
          className="order-list"
          data-busy={busy}
          as="div"
        >
          {enabled.map((m, i) => (
            <OrderRow
              key={m.id}
              mod={m}
              position={i + 1}
              first={i === 0}
              last={i === enabled.length - 1}
              busy={busy}
              reduceMotion={!!reduceMotion}
              verdict={(() => {
                const t = tally.get(m.id);
                return t ? verdictLine(t.wins, t.loses) : null;
              })()}
              onMoveUp={() => move(i, -1)}
              onMoveDown={() => move(i, 1)}
            />
          ))}
        </Reorder.Group>

        <div className="order-edge">Loads last, wins shared files</div>

        <div className="visually-hidden" role="status" aria-live="polite">
          {announcement}
        </div>
      </section>

      <section className="card stack">
        <div className="row order-head">
          <div className="order-head-main">
            <div className="card-title">Files claimed by more than one mod</div>
            <div className="card-hint">
              Load order already decides each of these. Pin a different mod to
              win one file: that changes who wins the file and nothing else, the
              mod itself stays where it is in the order.
            </div>
          </div>
          {conflicts.length > 0 && <Chip>{conflicts.length}</Chip>}
        </div>

        {conflicts.length === 0 ? (
          <div className="order-quiet">
            <Icon.check size={14} />
            No two mods claim the same file right now.
          </div>
        ) : (
          <div className="order-claims">
            {conflicts.map((c) => {
              const pinned: string | undefined = overrides[c.path];
              const winner = winnerOf.get(c.path) ?? c.winner;
              // A pin can outlive the mod it names. Keep it in the list so it
              // can be seen and cleared instead of silently disappearing.
              const choices = pinned && !c.contenders.includes(pinned)
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
                      The pinned mod is no longer installed. Unpin the file to
                      hand it back to load order.
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
        )}
      </section>
    </div>
  );
}

const DRAG_SPRING = { type: "spring", stiffness: 600, damping: 40 } as const;

/**
 * One row of the order.
 *
 * Dragging starts from the grip only, so the arrows stay clickable. The arrows
 * are not a nicety: dragging is unreachable from the keyboard and ordering is
 * the only thing this screen does.
 */
function OrderRow({
  mod,
  position,
  first,
  last,
  busy,
  reduceMotion,
  verdict,
  onMoveUp,
  onMoveDown,
}: {
  mod: ModView;
  position: number;
  first: boolean;
  last: boolean;
  busy: boolean;
  reduceMotion: boolean;
  verdict: string | null;
  onMoveUp: () => void;
  onMoveDown: () => void;
}) {
  const controls = useDragControls();

  return (
    <Reorder.Item
      value={mod.id}
      dragListener={false}
      dragControls={controls}
      className="order-row"
      whileDrag={{ scale: 1.01, zIndex: 2 }}
      transition={reduceMotion ? { duration: 0 } : DRAG_SPRING}
      as="div"
    >
      <span className="order-index">{position}</span>

      <span
        className="drag-handle"
        // touch-action is none in CSS so the browser cannot claim the gesture
        // for scrolling before the drag starts.
        onPointerDown={(e) => {
          if (busy) return;
          e.preventDefault();
          controls.start(e);
        }}
        role="presentation"
        aria-hidden="true"
        title="Drag to move this mod"
      >
        <Icon.grip size={14} />
      </span>

      <div className="order-main">
        <div className="order-name-row">
          <span className="mod-name truncate">{mod.name}</span>
          {mod.version && <Chip>{mod.version}</Chip>}
        </div>
        {verdict && <div className="order-verdict">{verdict}</div>}
      </div>

      <div className="order-move">
        <button
          type="button"
          className="btn sm icon ghost"
          onClick={onMoveUp}
          disabled={first || busy}
          aria-label={`Move ${mod.name} up`}
          title="Move up, loads earlier"
        >
          <span className="order-chev up">
            <Icon.chevronDown size={14} />
          </span>
        </button>
        <button
          type="button"
          className="btn sm icon ghost"
          onClick={onMoveDown}
          disabled={last || busy}
          aria-label={`Move ${mod.name} down`}
          title="Move down, wins more files"
        >
          <span className="order-chev">
            <Icon.chevronDown size={14} />
          </span>
        </button>
      </div>
    </Reorder.Item>
  );
}
