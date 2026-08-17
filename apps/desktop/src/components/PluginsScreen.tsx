/**
 * Plugin order, for a Creation Engine game.
 *
 * The second ordering this application has, and it is not the same as the
 * first. Load order arranges *mods* by an integer each; this arranges a named
 * list of *files*, which is independent of which mod supplied them — two mods
 * can ship one plugin each and the user can want them in either order.
 *
 * Two things are stated here rather than assumed, because both are the reverse
 * of the mod list next door:
 *
 * 1. **Higher loads first, and later overrides earlier.** In the mod list the
 *    mod further *down* wins. Here the plugin further down is the
 *    one that overrides, but the ordering language people use for plugin lists
 *    is "loads first", so the screen says both rather than picking one and
 *    leaving the other to be inferred wrongly.
 * 2. **The master block is not a preference.** The engine loads every master
 *    before any ordinary plugin whatever the list says, so the divider is drawn
 *    and labelled: dragging a `.esp` above an `.esm` visibly cannot happen,
 *    instead of appearing to work and being undone on the next read.
 *
 * Every edit writes `plugins.txt` immediately and the command returns the list
 * as it then is, so this component holds no order of its own — it renders what
 * the last write produced. There is nothing to save and nothing to lose.
 */

import { Reorder, useDragControls, useReducedMotion } from "framer-motion";
import { useMemo, useState } from "react";
import type { PluginEntryView, PluginOrderView } from "../lib/api";
import { Icon } from "./icons";
import { Checkbox, Chip } from "./ui";

export interface PluginsScreenProps {
  order: PluginOrderView;
  busy: boolean;
  /** Move one plugin to an index in the whole list. */
  onMove: (name: string, to: number) => void;
  onToggle: (name: string, enabled: boolean) => void;
}

/** How many plugins the game will actually load. */
function loadedCount(entries: PluginEntryView[]): number {
  return entries.filter((e) => e.enabled || e.implicit).length;
}

export function PluginsScreen({
  order,
  busy,
  onMove,
  onToggle,
}: PluginsScreenProps) {
  const reduceMotion = useReducedMotion();
  /** Read out after a keyboard move, which is otherwise silent. */
  const [announcement, setAnnouncement] = useState("");

  const entries = order.entries;

  /**
   * Where the master block ends.
   *
   * The list arrives already partitioned — the backend returns it in the order
   * the game reads — so this is a boundary to draw, not one to compute.
   */
  const masterCount = useMemo(
    () => entries.filter((e) => e.kind !== "normal").length,
    [entries],
  );

  const byName = useMemo(
    () => new Map(entries.map((e) => [e.name.toLowerCase(), e])),
    [entries],
  );

  /**
   * Turn a dragged sequence back into one move.
   *
   * `Reorder` hands back the whole new order; the command takes one name and
   * one index, because that is what the domain does and what the journal
   * records. The first position that differs is the plugin that moved.
   */
  function applyDrag(next: string[]) {
    const before = entries.map((e) => e.name);
    const at = next.findIndex((name, i) => name !== before[i]);
    if (at < 0) return;

    // Two positions differ after any move; the one that moved is whichever of
    // them is not simply displaced. Comparing against the old index of each
    // candidate settles it.
    const candidate = next[at];
    const wasAt = before.indexOf(candidate);
    const moved = wasAt === at + 1 ? before[at] : candidate;
    onMove(moved, next.indexOf(moved));
  }

  function move(index: number, delta: number) {
    const target = index + delta;
    if (target < 0 || target >= entries.length) return;
    const entry = entries[index];
    if (entry.implicit) return;
    onMove(entry.name, target);
    setAnnouncement(
      `${entry.name} moved to position ${target + 1} of ${entries.length}.`,
    );
  }

  if (entries.length === 0) {
    return (
      <div className="empty">
        <span className="empty-icon">
          <Icon.order size={40} strokeWidth={1} />
        </span>
        <div className="empty-title">No plugins yet</div>
        <div className="order-measure">
          Install a mod that ships an <span className="mono">.esp</span>,{" "}
          <span className="mono">.esm</span> or{" "}
          <span className="mono">.esl</span> and it appears here. Apocrypha adds
          each new plugin to the game&rsquo;s list and switches it on; arranging
          them is this screen.
        </div>
      </div>
    );
  }

  return (
    <div className="stack">
      {order.violations.length > 0 && (
        <section className="card stack">
          <div className="row order-head">
            <div className="order-head-main">
              <div className="card-title">Plugins that load too early</div>
              <div className="card-hint">
                A plugin has to load after everything it depends on. The game
                will not start, or will start with the mod&rsquo;s changes
                silently missing.
              </div>
            </div>
            <Chip kind="warn">{order.violations.length}</Chip>
          </div>

          <div className="plugin-problems">
            {order.violations.map((v) => (
              <div className="plugin-problem" key={`${v.plugin}->${v.master}`}>
                <span className="plugin-problem-text">{v.message}</span>
                {v.fix && (
                  <button
                    type="button"
                    className="btn sm"
                    disabled={busy}
                    onClick={() => onMove(v.fix!.name, v.fix!.to)}
                  >
                    {v.fix.label}
                  </button>
                )}
              </div>
            ))}
          </div>
        </section>
      )}

      <section className="card stack">
        <div className="row order-head">
          <div className="order-head-main">
            <div className="card-title">The order the game reads</div>
            <div className="card-hint">
              Plugins load from the top down, so one further down overrides the
              ones above it and has to come after anything it depends on. Drag
              by the grip, or use the arrows. Every change is written to the
              game&rsquo;s list straight away.
            </div>
          </div>
          <Chip>
            {loadedCount(entries)} of {entries.length} on
          </Chip>
        </div>

        <Reorder.Group
          axis="y"
          // Names, not entry objects: every write rebuilds them and Reorder
          // matches by identity, so objects break mid drag.
          values={entries.map((e) => e.name)}
          onReorder={applyDrag}
          className="order-list"
          data-busy={busy}
          as="div"
        >
          {entries.map((entry, i) => (
            <PluginRow
              key={entry.name}
              entry={entry}
              position={i + 1}
              first={i === 0}
              last={i === entries.length - 1}
              busy={busy}
              reduceMotion={!!reduceMotion}
              // The boundary the engine enforces, drawn where it falls.
              endsMasterBlock={i === masterCount - 1 && masterCount < entries.length}
              missingMaster={entry.masters.some(
                (m) => !byName.has(m.toLowerCase()),
              )}
              onMoveUp={() => move(i, -1)}
              onMoveDown={() => move(i, 1)}
              onToggle={(on) => onToggle(entry.name, on)}
            />
          ))}
        </Reorder.Group>

        <div className="order-quiet">
          <Icon.info size={14} />
          <span className="mono truncate" title={order.path}>
            {order.path}
          </span>
        </div>

        <div className="visually-hidden" role="status" aria-live="polite">
          {announcement}
        </div>
      </section>
    </div>
  );
}

const DRAG_SPRING = { type: "spring", stiffness: 600, damping: 40 } as const;

/**
 * One plugin.
 *
 * An implicit entry — one the engine loads itself, like `Skyrim.esm` — is shown
 * and cannot be dragged, moved or switched off. It is in the list because
 * without it every other plugin's masters would look unsatisfied, not because
 * it is anybody's to arrange.
 */
function PluginRow({
  entry,
  position,
  first,
  last,
  busy,
  reduceMotion,
  endsMasterBlock,
  missingMaster,
  onMoveUp,
  onMoveDown,
  onToggle,
}: {
  entry: PluginEntryView;
  position: number;
  first: boolean;
  last: boolean;
  busy: boolean;
  reduceMotion: boolean;
  endsMasterBlock: boolean;
  missingMaster: boolean;
  onMoveUp: () => void;
  onMoveDown: () => void;
  onToggle: (enabled: boolean) => void;
}) {
  const controls = useDragControls();
  const fixed = entry.implicit;

  return (
    <Reorder.Item
      value={entry.name}
      dragListener={false}
      dragControls={fixed ? undefined : controls}
      drag={fixed ? false : undefined}
      className={`order-row plugin-row ${endsMasterBlock ? "ends-masters" : ""}`}
      whileDrag={{ scale: 1.01, zIndex: 2 }}
      transition={reduceMotion ? { duration: 0 } : DRAG_SPRING}
      as="div"
    >
      <span className="order-index">{position}</span>

      <span
        className="drag-handle"
        onPointerDown={(e) => {
          if (busy || fixed) return;
          e.preventDefault();
          controls.start(e);
        }}
        role="presentation"
        aria-hidden="true"
        data-fixed={fixed}
        title={
          fixed
            ? "Loaded by the game itself"
            : `Drag to move ${entry.name}`
        }
      >
        <Icon.grip size={14} />
      </span>

      <Checkbox
        checked={entry.enabled || entry.implicit}
        disabled={busy || fixed}
        onChange={onToggle}
        label={`Load ${entry.name}`}
      />

      <div className="order-main">
        <div className="order-name-row">
          <span className="mod-name truncate mono">{entry.name}</span>
          {entry.kind === "master" && <Chip>Master</Chip>}
          {entry.kind === "light-master" && <Chip>Light</Chip>}
          {entry.implicit && <Chip kind="accent">Built in</Chip>}
          {!entry.present && !entry.implicit && (
            <Chip kind="warn">File missing</Chip>
          )}
          {missingMaster && <Chip kind="warn">Needs a plugin you do not have</Chip>}
        </div>
        {entry.masters.length > 0 && (
          <div className="order-verdict truncate">
            after {entry.masters.join(", ")}
          </div>
        )}
      </div>

      <div className="order-move">
        <button
          type="button"
          className="btn sm icon ghost"
          onClick={onMoveUp}
          disabled={first || busy || fixed}
          aria-label={`Move ${entry.name} up`}
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
          disabled={last || busy || fixed}
          aria-label={`Move ${entry.name} down`}
          title="Move down, loads later"
        >
          <span className="order-chev">
            <Icon.chevronDown size={14} />
          </span>
        </button>
      </div>
    </Reorder.Item>
  );
}
