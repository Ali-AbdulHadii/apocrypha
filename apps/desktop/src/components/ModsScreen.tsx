/**
 * Mod list.
 *
 * Built for large libraries: mods group into collapsible categories, can be
 * searched and filtered, and carry an explicit applied/pending badge so it is
 * obvious what is actually in the game right now. Load order is set by dragging
 * the grip on the left of each row.
 */

import { AnimatePresence, Reorder, motion, useDragControls } from "framer-motion";
import { useMemo, useState } from "react";
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
  onReorder: (orderedIds: string[]) => void;
  onRemove: (mod: ModView) => void;
  onImport: () => void;
}

function categoryOf(m: ModView): string {
  return m.category?.trim() || "Uncategorised";
}

/**
 * Rebuild the whole load order after one category was reordered.
 *
 * The reordered mods go back into the exact positions that category already
 * occupied, so moving a mod inside "Armor" cannot shuffle every other category
 * to the end of the list.
 */
export function spliceOrder(
  all: ModView[],
  category: string,
  reorderedIds: string[],
): string[] {
  const full = [...all].sort((a, b) => a.priority - b.priority).map((m) => m.id);
  const byId = new Map(all.map((m) => [m.id, m]));

  const slots: number[] = [];
  full.forEach((id, i) => {
    const m = byId.get(id);
    if (m && categoryOf(m) === category) slots.push(i);
  });

  const out = [...full];
  slots.forEach((slot, i) => {
    const id = reorderedIds[i];
    if (id !== undefined) out[slot] = id;
  });
  return out;
}

export function ModsScreen({
  mods,
  appliedIds,
  dirty,
  onToggle,
  onConfigure,
  onReorder,
  onRemove,
  onImport,
}: ModsScreenProps) {
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<SortKey>("order");
  const [status, setStatus] = useState<"all" | "enabled" | "disabled">("all");
  const [category, setCategory] = useState("all");
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  /** Groups whose open animation has finished, so their clip can be dropped. */
  const [settled, setSettled] = useState<Set<string>>(new Set());

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

  /** Dragging only makes sense when the list is in load order and unfiltered. */
  const reorderable =
    sort === "order" && !query.trim() && status === "all" && category === "all";

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
        <button className="btn primary" onClick={onImport} style={{ marginTop: 8 }}>
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

      {!reorderable && sort === "order" && (
        <div className="card-hint">
          Clear the search and filters to drag mods into a different load order.
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
        grouped.map(([name, items]) => {
          const open = !collapsed.has(name);
          const enabledCount = items.filter((m) => m.enabled).length;
          return (
            <section className="mod-group" key={name}>
              <button className="mod-group-head" onClick={() => toggleCategory(name)}>
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
                    transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
                    // Clipped while the height animates, then released, so a
                    // row being dragged is not cut off at the group edge.
                    style={{ overflow: settled.has(name) ? "visible" : "hidden" }}
                    onAnimationComplete={() =>
                      setSettled((prev) => new Set(prev).add(name))
                    }
                    onAnimationStart={() =>
                      setSettled((prev) => {
                        const next = new Set(prev);
                        next.delete(name);
                        return next;
                      })
                    }
                  >
                    {reorderable ? (
                      <Reorder.Group
                        axis="y"
                        // Ids, not mod objects. Reorder tracks values by
                        // identity, and every state update rebuilds the mod
                        // objects, so object values would break mid drag.
                        values={items.map((m) => m.id)}
                        onReorder={(nextIds) => onReorder(spliceOrder(mods, name, nextIds))}
                        className="mod-group-body"
                        as="div"
                      >
                        {items.map((m) => (
                          <DraggableModRow
                            key={m.id}
                            mod={m}
                            applied={appliedIds.has(m.id)}
                            onToggle={onToggle}
                            onConfigure={onConfigure}
                            onRemove={onRemove}
                          />
                        ))}
                      </Reorder.Group>
                    ) : (
                      <div className="mod-group-body">
                        {items.map((m) => (
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
                      </div>
                    )}
                  </motion.div>
                )}
              </AnimatePresence>
            </section>
          );
        })
      )}
    </div>
  );
}

function StatusChip({
  mod,
  applied,
}: {
  mod: ModView;
  applied: boolean;
}) {
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

/** Shared row content, used by both the static and draggable variants. */
function RowBody({
  mod,
  applied,
  handle,
  onToggle,
  onConfigure,
  onRemove,
}: {
  mod: ModView;
  applied: boolean;
  handle: React.ReactNode;
  onToggle: (m: ModView, enabled: boolean) => void;
  onConfigure: (m: ModView) => void;
  onRemove: (m: ModView) => void;
}) {
  return (
    <>
      {handle}

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
          {/* Written as the character itself: a \u escape in JSX text is not
              processed, it renders as the six literal characters. */}
          {mod.author ? `${mod.author} \u00b7 ` : ""}
          {mod.selection.length} of{" "}
          {mod.groups.reduce((n, g) => n + g.options.length, 0)} options \u00b7{" "}
          {mod.totalFiles} files \u00b7 {formatBytes(mod.totalBytes)}
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
    </>
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
      <RowBody
        mod={mod}
        applied={applied}
        handle={<span className="mod-order">{mod.priority}</span>}
        onToggle={onToggle}
        onConfigure={onConfigure}
        onRemove={onRemove}
      />
    </div>
  );
}

/**
 * A row that can be dragged by its grip.
 *
 * The item value is the mod id rather than the mod object, because Reorder
 * matches values by identity and the mod objects are rebuilt on every state
 * update. Dragging only starts from the grip, so the toggle and the Configure
 * button stay clickable.
 */
function DraggableModRow({
  mod,
  applied,
  onToggle,
  onConfigure,
  onRemove,
}: {
  mod: ModView;
  applied: boolean;
  onToggle: (m: ModView, enabled: boolean) => void;
  onConfigure: (m: ModView) => void;
  onRemove: (m: ModView) => void;
}) {
  const controls = useDragControls();

  return (
    <Reorder.Item
      value={mod.id}
      dragListener={false}
      dragControls={controls}
      className={`mod-row ${mod.enabled ? "" : "disabled"}`}
      whileDrag={{ scale: 1.01, zIndex: 2 }}
      transition={{ type: "spring", stiffness: 600, damping: 40 }}
      as="div"
    >
      <RowBody
        mod={mod}
        applied={applied}
        handle={
          <span
            className="drag-handle"
            // touch-action is none in CSS so the browser does not claim the
            // gesture for scrolling before the drag can start.
            onPointerDown={(e) => {
              e.preventDefault();
              controls.start(e);
            }}
            role="button"
            tabIndex={-1}
            aria-label={`Reorder ${mod.name}`}
            title="Drag to change load order"
          >
            <Icon.grip size={14} />
          </span>
        }
        onToggle={onToggle}
        onConfigure={onConfigure}
        onRemove={onRemove}
      />
    </Reorder.Item>
  );
}
