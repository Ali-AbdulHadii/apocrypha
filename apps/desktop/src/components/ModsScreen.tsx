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
  onImport: () => void;
}

function categoryOf(m: ModView): string {
  return m.category?.trim() || "Uncategorised";
}

export function ModsScreen({
  mods,
  appliedIds,
  dirty,
  onToggle,
  onConfigure,
  onReorder,
  onImport,
}: ModsScreenProps) {
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<SortKey>("order");
  const [status, setStatus] = useState<"all" | "enabled" | "disabled">("all");
  const [category, setCategory] = useState("all");
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());

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
                    style={{ overflow: "hidden" }}
                  >
                    {reorderable ? (
                      <Reorder.Group
                        axis="y"
                        values={items}
                        onReorder={(next) => {
                          // Splice the reordered category back into the full order.
                          const ids = next.map((m) => m.id);
                          const rest = mods
                            .filter((m) => categoryOf(m) !== name)
                            .sort((a, b) => a.priority - b.priority)
                            .map((m) => m.id);
                          onReorder([...ids, ...rest]);
                        }}
                        className="mod-group-body"
                        as="div"
                      >
                        {items.map((m) => (
                          <ModRow
                            key={m.id}
                            mod={m}
                            applied={appliedIds.has(m.id)}
                            dirty={dirty}
                            draggable
                            onToggle={onToggle}
                            onConfigure={onConfigure}
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

function ModRow({
  mod,
  applied,
  draggable,
  onToggle,
  onConfigure,
}: {
  mod: ModView;
  applied: boolean;
  dirty: boolean;
  draggable?: boolean;
  onToggle: (m: ModView, enabled: boolean) => void;
  onConfigure: (m: ModView) => void;
}) {
  const controls = useDragControls();

  const body = (
    <>
      {draggable ? (
        <span
          className="drag-handle"
          onPointerDown={(e) => controls.start(e)}
          aria-label={`Reorder ${mod.name}`}
          title="Drag to change load order"
        >
          <Icon.grip size={14} />
        </span>
      ) : (
        <span className="mod-order">{mod.priority}</span>
      )}

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
          {mod.author ? `${mod.author} · ` : ""}
          {mod.selection.length} of{" "}
          {mod.groups.reduce((n, g) => n + g.options.length, 0)} options ·{" "}
          {mod.totalFiles} files · {formatBytes(mod.totalBytes)}
        </div>
      </div>

      <button className="btn sm" onClick={() => onConfigure(mod)}>
        Configure
      </button>
    </>
  );

  if (!draggable) {
    return <div className={`mod-row ${mod.enabled ? "" : "disabled"}`}>{body}</div>;
  }

  return (
    <Reorder.Item
      value={mod}
      dragListener={false}
      dragControls={controls}
      className={`mod-row ${mod.enabled ? "" : "disabled"}`}
      whileDrag={{ scale: 1.01 }}
      as="div"
    >
      {body}
    </Reorder.Item>
  );
}
