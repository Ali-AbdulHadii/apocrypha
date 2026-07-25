/** Apocrypha desktop shell: title bar, rail navigation, screens, deploy bar. */

import { AnimatePresence, motion } from "framer-motion";
import { useCallback, useEffect, useMemo, useState } from "react";
import { ApplyDialog, type ApplyState } from "./components/ApplyDialog";
import { ConfirmDialog, type Confirm } from "./components/ConfirmDialog";
import { Icon, type IconName } from "./components/icons";
import { InstallWizard } from "./components/InstallWizard";
import { ModsScreen } from "./components/ModsScreen";
import { Splash } from "./components/Splash";
import { TitleBar } from "./components/TitleBar";
import { DownloadsPanel } from "./components/DownloadsPanel";
import { DownloadsScreen } from "./components/DownloadsScreen";
import { Chip, Segmented, Spinner, pageMotion, useToast } from "./components/ui";
import {
  api,
  formatBytes,
  IS_TAURI,
  pickArchive,
  pickDirectory,
  onDownloadChanged,
  onNxmLink,
  subscribe,
  truncatePath,
  type DownloadView,
  type DryRunView,
  type GameView,
  type ModView,
  type PreviewSource,
  type ProfileView,
  type SettingsView,
} from "./lib/api";
import { AppearancePanel, useAppearance } from "./lib/appearance";
import { useMaximized } from "./lib/window";
import { useTheme } from "./lib/theme";

type Screen =
  | "library"
  | "mods"
  | "downloads"
  | "profiles"
  | "conflicts"
  | "settings";

const NAV: { id: Screen; label: string; icon: IconName }[] = [
  { id: "library", label: "Library", icon: "library" },
  { id: "mods", label: "Mods", icon: "mods" },
  { id: "downloads", label: "Downloads", icon: "downloads" },
  { id: "profiles", label: "Profiles", icon: "profiles" },
  { id: "conflicts", label: "Changes", icon: "conflicts" },
  { id: "settings", label: "Settings", icon: "settings" },
];

export default function App() {
  const [screen, setScreen] = useState<Screen>("library");
  const [games, setGames] = useState<GameView[]>([]);
  const [activeGameId, setActiveGameId] = useState<string | null>(null);
  const [mods, setMods] = useState<ModView[]>([]);
  const [profiles, setProfiles] = useState<ProfileView[]>([]);
  const [settings, setSettings] = useState<SettingsView | null>(null);
  const [booting, setBooting] = useState(true);
  const [busy, setBusy] = useState(false);
  const [wizardMod, setWizardMod] = useState<ModView | null>(null);
  const [pendingArchive, setPendingArchive] = useState<string | null>(null);
  const [preview, setPreview] = useState<DryRunView | null>(null);
  const [dirty, setDirty] = useState(false);
  const [apply, setApply] = useState<ApplyState | null>(null);
  const [confirm, setConfirm] = useState<Confirm | null>(null);
  const [downloads, setDownloads] = useState<DownloadView[]>([]);
  const [installingId, setInstallingId] = useState<string | null>(null);

  const { push } = useToast();
  const maximized = useMaximized();
  const activeGame = games.find((g) => g.id === activeGameId) ?? null;

  const fail = useCallback(
    (e: unknown) => push(String(e instanceof Error ? e.message : e), "bad"),
    [push],
  );

  /* ---------------------------------------------------------- loading --- */

  const refreshGames = useCallback(async () => {
    const list = await api.listGames();
    setGames(list);
    setActiveGameId((cur) => cur ?? list[0]?.id ?? null);
    return list;
  }, []);

  const refreshMods = useCallback(async (gameId: string) => {
    setMods(await api.listMods(gameId));
  }, []);

  const refreshProfiles = useCallback(async (gameId: string) => {
    setProfiles(await api.listProfiles(gameId));
  }, []);

  const refreshDownloads = useCallback(async () => {
    setDownloads(await api.listDownloads());
  }, []);

  useEffect(() => {
    if (!IS_TAURI) {
      setBooting(false);
      return;
    }
    (async () => {
      const started = Date.now();
      try {
        await refreshGames();
        setSettings(await api.getSettings());
        await refreshDownloads();
      } catch (e) {
        fail(e);
      } finally {
        // Hold the splash briefly so it reads as intentional, not a flash.
        const elapsed = Date.now() - started;
        setTimeout(() => setBooting(false), Math.max(0, 620 - elapsed));
      }
    })();
  }, [refreshGames, refreshDownloads, fail]);

  useEffect(() => {
    if (!IS_TAURI || !activeGameId) return;
    (async () => {
      try {
        await Promise.all([refreshMods(activeGameId), refreshProfiles(activeGameId)]);
      } catch (e) {
        fail(e);
      }
    })();
  }, [activeGameId, refreshMods, refreshProfiles, fail]);

  // Re-scan on arrival, so a file saved from a browser while the app was open
  // is already listed by the time the user looks.
  useEffect(() => {
    if (!IS_TAURI || screen !== "downloads") return;
    refreshDownloads().catch(fail);
  }, [screen, refreshDownloads, fail]);

  /* ------------------------------------------------- nexus download link --- */

  // Links arrive from the OS when the user presses "Mod Manager Download" on
  // the website. The transfer is queued and the user is sent to Downloads to
  // watch it, rather than the window being taken over by a wizard for a file
  // that has not arrived yet.
  useEffect(() => {
    if (!IS_TAURI) return;
    return subscribe(() =>
      onNxmLink(async (url) => {
        try {
          const started = await api.startNxmDownload(url);
          setDownloads((prev) => [
            started,
            ...prev.filter((d) => d.id !== started.id),
          ]);
          setScreen("downloads");
          push(`Downloading ${started.fileName}`, "info");
        } catch (e) {
          fail(e);
        }
      }),
    );
  }, [push, fail]);

  // Progress carries the whole entry, so the list is updated by replacement.
  useEffect(() => {
    if (!IS_TAURI) return;
    return subscribe(() =>
      onDownloadChanged((d) => {
        setDownloads((prev) =>
          prev.some((x) => x.id === d.id)
            ? prev.map((x) => (x.id === d.id ? d : x))
            : [d, ...prev],
        );
        if (d.state === "ready") push(`${d.fileName} is ready to install`, "ok");
        if (d.state === "failed") push(`${d.fileName} did not finish`, "bad");
      }),
    );
  }, [push]);

  /* ---------------------------------------------------------- actions --- */

  const detect = useCallback(async () => {
    if (!activeGameId) return;
    setBusy(true);
    try {
      const g = await api.detectGame(activeGameId);
      setGames((prev) => prev.map((x) => (x.id === g.id ? g : x)));
      push(
        g.detected
          ? `Found ${g.name} at ${truncatePath(g.installDir ?? "", 42)}`
          : `${g.name} was not found in any Steam library`,
        g.detected ? "ok" : "bad",
      );
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  }, [activeGameId, push, fail]);

  const browseForGame = useCallback(async () => {
    if (!activeGameId) return;
    try {
      const dir = await pickDirectory();
      if (!dir) return;
      const g = await api.setGamePath(activeGameId, dir);
      setGames((prev) => prev.map((x) => (x.id === g.id ? g : x)));
      push(`Game folder set to ${truncatePath(dir, 42)}`, "ok");
    } catch (e) {
      fail(e);
    }
  }, [activeGameId, push, fail]);

  const startImport = useCallback(async () => {
    if (!activeGameId) {
      push("Select a game first", "bad");
      return;
    }
    try {
      const path = await pickArchive();
      if (!path) return;
      setBusy(true);
      const analyzed = await api.analyzeArchive(activeGameId, path);
      if (analyzed.totalFiles === 0) {
        push(
          `${analyzed.name} has no files this game can install. It may be packed in an unusual way.`,
          "bad",
        );
      }
      setPendingArchive(path);
      setWizardMod(analyzed);
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  }, [activeGameId, push, fail]);

  // Installing from Downloads joins the same path as Add mod: analyze, then the
  // wizard. Nothing about the archive is treated differently for having been
  // fetched here rather than picked from disk.
  const installDownload = useCallback(
    async (d: DownloadView) => {
      if (!activeGameId) {
        push("Select a game first", "bad");
        return;
      }
      setInstallingId(d.id);
      setBusy(true);
      try {
        const analyzed = await api.analyzeArchive(activeGameId, d.path);
        if (analyzed.totalFiles === 0) {
          push(
            `${analyzed.name} has no files this game can install. It may be packed in an unusual way.`,
            "bad",
          );
        }
        setPendingArchive(d.path);
        setWizardMod(analyzed);
      } catch (e) {
        fail(e);
      } finally {
        setInstallingId(null);
        setBusy(false);
      }
    },
    [activeGameId, push, fail],
  );

  const cancelDownload = useCallback(
    async (d: DownloadView) => {
      try {
        await api.cancelDownload(d.id);
      } catch (e) {
        fail(e);
      }
    },
    [fail],
  );

  const removeDownload = useCallback(
    (d: DownloadView) => {
      setConfirm({
        title: `Delete ${d.fileName}?`,
        body:
          "This deletes the downloaded file. Mods you have already added to " +
          "your library keep working, because they hold their own copy.",
        confirmLabel: "Delete",
        onConfirm: async () => {
          setBusy(true);
          try {
            await api.removeDownload(d.id);
            setDownloads((prev) => prev.filter((x) => x.id !== d.id));
          } catch (e) {
            fail(e);
          } finally {
            setBusy(false);
            setConfirm(null);
          }
        },
      });
    },
    [fail],
  );

  const confirmWizard = useCallback(
    async (selection: string[]) => {
      if (!activeGameId) return;
      setBusy(true);
      try {
        if (pendingArchive) {
          await api.importMod(activeGameId, pendingArchive, selection);
          push("Mod added to your library", "ok");
          // So the Downloads row stops offering to install what is now in the
          // library. Failing to refresh is not worth reporting as an error.
          refreshDownloads().catch(() => {});
        } else if (wizardMod?.id) {
          await api.setModSelection(activeGameId, wizardMod.id, selection);
          push("Options saved", "ok");
        }
        await refreshMods(activeGameId);
        setDirty(true);
        setWizardMod(null);
        setPendingArchive(null);
      } catch (e) {
        fail(e);
      } finally {
        setBusy(false);
      }
    },
    [
      activeGameId,
      pendingArchive,
      wizardMod,
      push,
      refreshMods,
      refreshDownloads,
      fail,
    ],
  );

  const toggleMod = useCallback(
    async (mod: ModView, enabled: boolean) => {
      if (!activeGameId) return;
      setMods((prev) => prev.map((m) => (m.id === mod.id ? { ...m, enabled } : m)));
      setDirty(true);
      try {
        await api.setModEnabled(activeGameId, mod.id, enabled);
      } catch (e) {
        fail(e);
        await refreshMods(activeGameId);
      }
    },
    [activeGameId, refreshMods, fail],
  );

  const reorderMods = useCallback(
    async (orderedIds: string[]) => {
      if (!activeGameId) return;
      setMods((prev) => {
        const rank = new Map(orderedIds.map((id, i) => [id, i]));
        return [...prev]
          .map((m) => ({ ...m, priority: rank.get(m.id) ?? m.priority }))
          .sort((a, b) => a.priority - b.priority);
      });
      setDirty(true);
      try {
        await api.setModOrder(activeGameId, orderedIds);
      } catch (e) {
        fail(e);
        await refreshMods(activeGameId);
      }
    },
    [activeGameId, refreshMods, fail],
  );

  const removeMod = useCallback(
    (mod: ModView) => {
      setConfirm({
        title: `Remove ${mod.name}?`,
        body:
          "This deletes Apocrypha's copy of the mod. Your original download is " +
          "not touched, and the game folder is left exactly as it is now.",
        confirmLabel: "Remove",
        onConfirm: async () => {
          if (!activeGameId) return;
          setBusy(true);
          try {
            await api.removeMod(activeGameId, mod.id);
            await refreshMods(activeGameId);
            push(`Removed ${mod.name}`, "ok");
            setConfirm(null);
          } catch (e) {
            fail(e);
            setConfirm(null);
          } finally {
            setBusy(false);
          }
        },
      });
    },
    [activeGameId, refreshMods, push, fail],
  );

  const runPreview = useCallback(async () => {
    if (!activeGameId) return;
    if (!mods.some((m) => m.enabled)) {
      push("Turn on at least one mod first", "bad");
      return;
    }
    setBusy(true);
    try {
      setPreview(await api.previewDeploy(activeGameId));
      setScreen("conflicts");
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  }, [activeGameId, mods, push, fail]);

  const runDeploy = useCallback(async () => {
    if (!activeGameId) return;
    if (!mods.some((m) => m.enabled)) {
      push("Turn on at least one mod first", "bad");
      return;
    }
    setApply({ phase: "linking" });
    try {
      const r = await api.deploy(activeGameId);
      setApply({ phase: "done", result: r });
      setDirty(false);
      setPreview(null);
      await refreshMods(activeGameId);
    } catch (e) {
      setApply({
        phase: "done",
        error: String(e instanceof Error ? e.message : e),
      });
    }
  }, [activeGameId, mods, push, refreshMods]);

  const runRollback = useCallback(async () => {
    if (!activeGameId) return;
    setBusy(true);
    try {
      const r = await api.rollbackLast(activeGameId);
      push(
        r.clean
          ? `Removed ${r.removed} files and put back ${r.restored}`
          : `Done, but ${r.skippedModified.length} file(s) were left alone because they changed since install`,
        r.clean ? "ok" : "bad",
      );
      await refreshMods(activeGameId);
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  }, [activeGameId, push, refreshMods, fail]);

  const runLoaderSetup = useCallback(async () => {
    if (!activeGameId) return;
    setBusy(true);
    try {
      push(await api.setupLoader(activeGameId), "ok");
      await refreshGames();
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  }, [activeGameId, push, refreshGames, fail]);

  const enabledCount = useMemo(() => mods.filter((m) => m.enabled).length, [mods]);
  // Only counts what still wants attention. A failed entry the user has seen
  // should not keep a badge lit forever.
  const downloadBadge = useMemo(
    () =>
      downloads.filter((d) => d.state === "downloading" || d.state === "ready")
        .length,
    [downloads],
  );
  const appliedIds = useMemo(
    () => new Set(mods.filter((m) => m.applied).map((m) => m.id)),
    [mods],
  );

  const previewSource = useMemo<PreviewSource | null>(() => {
    if (pendingArchive && activeGameId)
      return { kind: "archive", gameId: activeGameId, archivePath: pendingArchive };
    if (wizardMod?.id && activeGameId)
      return { kind: "mod", gameId: activeGameId, modId: wizardMod.id };
    return null;
  }, [pendingArchive, wizardMod, activeGameId]);

  if (!IS_TAURI) return <BrowserNotice />;

  return (
    <div className={`app ${maximized ? "maximized" : ""}`}>
      <div className="ambient" />
      <TitleBar subtitle={activeGame?.name} />

      <div className="app-body">
        <Rail
          screen={screen}
          setScreen={setScreen}
          modCount={mods.length}
          downloadCount={downloadBadge}
        />

        <div className="content">
          <TopBar
            screen={screen}
            game={activeGame}
            busy={busy}
            onImport={startImport}
            onDetect={detect}
          />

          <div className="scroll">
            <AnimatePresence mode="wait">
              <motion.div key={screen} {...pageMotion}>
                {screen === "library" ? (
                  <LibraryScreen
                    games={games}
                    activeId={activeGameId}
                    onSelect={setActiveGameId}
                    onDetect={detect}
                    onBrowse={browseForGame}
                    onLoaderSetup={runLoaderSetup}
                    modCount={mods.length}
                    enabledCount={enabledCount}
                    busy={busy}
                  />
                ) : screen === "mods" ? (
                  <ModsScreen
                    mods={mods}
                    appliedIds={appliedIds}
                    dirty={dirty}
                    onToggle={toggleMod}
                    onConfigure={(m) => {
                      setPendingArchive(null);
                      setWizardMod(m);
                    }}
                    onReorder={reorderMods}
                    onRemove={removeMod}
                    onImport={startImport}
                  />
                ) : screen === "downloads" ? (
                  <DownloadsScreen
                    downloads={downloads}
                    busy={busy}
                    installingId={installingId}
                    onInstall={installDownload}
                    onCancel={cancelDownload}
                    onRemove={removeDownload}
                    onRefresh={() => refreshDownloads().catch(fail)}
                  />
                ) : screen === "profiles" ? (
                  <ProfilesScreen
                    profiles={profiles}
                    gameId={activeGameId}
                    onChanged={setProfiles}
                    onError={fail}
                  />
                ) : screen === "conflicts" ? (
                  <ChangesScreen preview={preview} onPreview={runPreview} busy={busy} />
                ) : (
                  <SettingsScreen
                    settings={settings}
                    onSettings={setSettings}
                    game={activeGame}
                    onError={fail}
                    onInfo={push}
                  />
                )}
              </motion.div>
            </AnimatePresence>
          </div>

          <DeployBar
            dirty={dirty}
            busy={busy}
            enabledCount={enabledCount}
            ready={Boolean(activeGame?.installDir)}
            onPreview={runPreview}
            onDeploy={runDeploy}
            onRollback={runRollback}
          />
        </div>
      </div>

      <AnimatePresence>
        {wizardMod && (
          <InstallWizard
            mod={wizardMod}
            busy={busy}
            previewSource={previewSource}
            confirmLabel={pendingArchive ? "Add mod" : "Save options"}
            onCancel={() => {
              setWizardMod(null);
              setPendingArchive(null);
            }}
            onConfirm={confirmWizard}
          />
        )}
      </AnimatePresence>

      <AnimatePresence>
        {apply && <ApplyDialog state={apply} onClose={() => setApply(null)} />}
      </AnimatePresence>

      <AnimatePresence>
        {confirm && (
          <ConfirmDialog
            confirm={confirm}
            busy={busy}
            onCancel={() => setConfirm(null)}
          />
        )}
      </AnimatePresence>

      <AnimatePresence>{booting && <Splash />}</AnimatePresence>
    </div>
  );
}

/* ============================================================== chrome === */

function Rail({
  screen,
  setScreen,
  modCount,
  downloadCount,
}: {
  screen: Screen;
  setScreen: (s: Screen) => void;
  modCount: number;
  /** Downloads running or waiting to be installed. */
  downloadCount: number;
}) {
  return (
    <aside className="rail">
      <nav style={{ display: "flex", flexDirection: "column", gap: 2 }}>
        {NAV.map((item) => {
          const IconCmp = Icon[item.icon];
          const active = screen === item.id;
          return (
            <button
              key={item.id}
              className={`nav-item ${active ? "active" : ""}`}
              onClick={() => setScreen(item.id)}
              aria-current={active ? "page" : undefined}
            >
              {active && (
                <motion.span
                  layoutId="nav-pill"
                  className="nav-pill"
                  transition={{ type: "spring", stiffness: 540, damping: 42 }}
                />
              )}
              <span className="nav-icon">
                <IconCmp />
              </span>
              <span>{item.label}</span>
              {item.id === "mods" && modCount > 0 && (
                <span className="nav-badge">{modCount}</span>
              )}
              {item.id === "downloads" && downloadCount > 0 && (
                <span className="nav-badge">{downloadCount}</span>
              )}
            </button>
          );
        })}
      </nav>

      <div className="rail-spacer" />
      <ThemeToggle />
    </aside>
  );
}

function ThemeToggle() {
  const { resolved, toggle } = useTheme();
  return (
    <button className="nav-item" onClick={toggle} aria-label="Switch theme">
      <span className="nav-icon">
        <AnimatePresence mode="wait" initial={false}>
          <motion.span
            key={resolved}
            initial={{ rotate: -90, opacity: 0 }}
            animate={{ rotate: 0, opacity: 1 }}
            exit={{ rotate: 90, opacity: 0 }}
            transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
            style={{ display: "grid", placeItems: "center" }}
          >
            {resolved === "dark" ? <Icon.moon /> : <Icon.sun />}
          </motion.span>
        </AnimatePresence>
      </span>
      <span>{resolved === "dark" ? "Dark" : "Light"}</span>
    </button>
  );
}

function TopBar({
  screen,
  game,
  busy,
  onImport,
  onDetect,
}: {
  screen: Screen;
  game: GameView | null;
  busy: boolean;
  onImport: () => void;
  onDetect: () => void;
}) {
  const titles: Record<Screen, string> = {
    library: "Library",
    mods: "Mods",
    downloads: "Downloads",
    profiles: "Profiles",
    conflicts: "Changes",
    settings: "Settings",
  };
  return (
    <header className="topbar">
      <div>
        <h1>{titles[screen]}</h1>
        <div className="topbar-sub">
          {game ? game.name : "No game selected"}
          {game?.installDir ? " · found" : game ? " · not found" : ""}
        </div>
      </div>
      <div className="topbar-actions">
        {busy && <Spinner />}
        <button className="btn" onClick={onDetect} disabled={busy || !game}>
          <Icon.refresh /> Find game
        </button>
        <button className="btn primary" onClick={onImport} disabled={busy}>
          <Icon.plus /> Add mod
        </button>
      </div>
    </header>
  );
}

function DeployBar({
  dirty,
  busy,
  enabledCount,
  ready,
  onPreview,
  onDeploy,
  onRollback,
}: {
  dirty: boolean;
  busy: boolean;
  enabledCount: number;
  ready: boolean;
  onPreview: () => void;
  onDeploy: () => void;
  onRollback: () => void;
}) {
  return (
    <div className="deploybar">
      <span
        className="dot"
        style={{ color: dirty ? "var(--warning)" : "var(--success)" }}
      />
      <span className="summary">
        {!ready
          ? "Game folder not set. Use Find game, or choose it yourself."
          : dirty
            ? "You have changes that are not in the game yet"
            : "The game matches your mod list"}
        {" · "}
        {enabledCount} on
      </span>
      <div className="spacer" />
      <button className="btn" onClick={onRollback} disabled={busy || !ready}>
        <Icon.undo /> Undo all
      </button>
      <button className="btn" onClick={onPreview} disabled={busy || !ready}>
        <Icon.preview /> Preview changes
      </button>
      <button className="btn primary" onClick={onDeploy} disabled={busy || !ready}>
        <Icon.apply /> Apply
      </button>
    </div>
  );
}

/* ============================================================= screens === */

function LibraryScreen({
  games,
  activeId,
  onSelect,
  onDetect,
  onBrowse,
  onLoaderSetup,
  modCount,
  enabledCount,
  busy,
}: {
  games: GameView[];
  activeId: string | null;
  onSelect: (id: string) => void;
  onDetect: () => void;
  onBrowse: () => void;
  onLoaderSetup: () => void;
  modCount: number;
  enabledCount: number;
  busy: boolean;
}) {
  const active = games.find((g) => g.id === activeId);
  return (
    <div className="stack">
      {games.map((g) => (
        <button
          key={g.id}
          className={`game-card ${g.id === activeId ? "selected" : ""}`}
          onClick={() => onSelect(g.id)}
        >
          <span className="game-art">
            <Icon.package size={22} />
          </span>
          <span style={{ minWidth: 0, flex: 1 }}>
            <span className="row" style={{ gap: "var(--sp-3)" }}>
              <span style={{ fontWeight: 600, fontSize: "var(--text-lg)" }}>
                {g.name}
              </span>
              {g.detected ? (
                <Chip kind="ok">
                  <span className="dot" /> found
                </Chip>
              ) : (
                <Chip kind="warn">not found</Chip>
              )}
              {g.loaderOverrideActive && <Chip kind="accent">loader ready</Chip>}
            </span>
            <span
              className="card-hint mono"
              style={{ display: "block", marginTop: 4 }}
            >
              {g.installDir
                ? truncatePath(g.installDir, 64)
                : `Steam app ${g.steamAppId}`}
            </span>
          </span>
          <Icon.chevronRight />
        </button>
      ))}

      {active && (
        <>
          <div className="stat-grid">
            <Stat label="Engine" value={active.engine} />
            <Stat label="Load order" value={active.loadOrder} />
            <Stat label="Mods" value={String(modCount)} />
            <Stat label="Turned on" value={String(enabledCount)} />
          </div>

          <div className="card stack">
            <div className="row">
              <div className="card-title">Linux and Proton</div>
              <div style={{ marginLeft: "auto" }} className="row">
                <button className="btn sm" onClick={onBrowse} disabled={busy}>
                  <Icon.folder size={14} /> Choose folder
                </button>
                <button className="btn sm" onClick={onDetect} disabled={busy}>
                  <Icon.refresh size={14} /> Find again
                </button>
              </div>
            </div>
            <dl className="kv">
              <dt>Game folder</dt>
              <dd className="mono">{active.installDir ?? "not set"}</dd>
              <dt>Proton files</dt>
              <dd className="mono">{active.protonPrefix ?? "not found"}</dd>
              <dt>Proton version</dt>
              <dd className="mono">{active.protonTool ?? "default"}</dd>
              <dt>Mod loader</dt>
              <dd>
                {active.loaderName ? (
                  <span className="row" style={{ gap: "var(--sp-3)" }}>
                    <span className="mono">
                      {active.loaderName} ({active.loaderDll})
                    </span>
                    {active.loaderOverrideActive ? (
                      <Chip kind="ok">ready</Chip>
                    ) : (
                      <Chip kind="warn">not set up</Chip>
                    )}
                  </span>
                ) : (
                  "none needed"
                )}
              </dd>
              {active.steamLaunchOptions && (
                <>
                  <dt>Steam launch options</dt>
                  <dd className="mono">{active.steamLaunchOptions}</dd>
                </>
              )}
            </dl>
            {active.loaderName && !active.loaderOverrideActive && (
              <div className="row">
                <button
                  className="btn primary"
                  onClick={onLoaderSetup}
                  disabled={busy || !active.protonPrefix}
                >
                  Set up {active.loaderName}
                </button>
                <span className="card-hint">
                  {active.protonPrefix
                    ? "Tells Proton to load the mod loader. Close Steam first."
                    : "Run the game once through Steam so Proton creates its files."}
                </span>
              </div>
            )}
          </div>
        </>
      )}
    </div>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="stat">
      <div className="stat-label">{label}</div>
      <div className="stat-value">{value}</div>
    </div>
  );
}

function ProfilesScreen({
  profiles,
  gameId,
  onChanged,
  onError,
}: {
  profiles: ProfileView[];
  gameId: string | null;
  onChanged: (p: ProfileView[]) => void;
  onError: (e: unknown) => void;
}) {
  const [name, setName] = useState("");
  if (!gameId) return <div className="empty">Select a game first.</div>;

  async function create() {
    if (!name.trim() || !gameId) return;
    try {
      onChanged(await api.createProfile(gameId, name.trim()));
      setName("");
    } catch (e) {
      onError(e);
    }
  }

  async function switchTo(id: number) {
    if (!gameId) return;
    try {
      onChanged(await api.switchProfile(gameId, id));
    } catch (e) {
      onError(e);
    }
  }

  return (
    <div className="stack">
      <div className="card stack">
        <div className="card-title">New profile</div>
        <div className="row">
          <input
            className="input"
            style={{ flex: 1 }}
            placeholder="For example: Multiplayer safe"
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && create()}
          />
          <button className="btn primary" onClick={create} disabled={!name.trim()}>
            Create
          </button>
        </div>
        <div className="card-hint">
          Each profile remembers its own mods, options, and order.
        </div>
      </div>

      <div className="stack tight">
        {profiles.map((p) => (
          <div className="mod-row" key={p.id}>
            <div style={{ minWidth: 0, flex: 1 }}>
              <div className="row" style={{ gap: "var(--sp-3)" }}>
                <span className="mod-name">{p.name}</span>
                {p.active && <Chip kind="ok">in use</Chip>}
              </div>
            </div>
            {!p.active && (
              <button className="btn sm" onClick={() => switchTo(p.id)}>
                Use this
              </button>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

function ChangesScreen({
  preview,
  onPreview,
  busy,
}: {
  preview: DryRunView | null;
  onPreview: () => void;
  busy: boolean;
}) {
  if (!preview) {
    return (
      <div className="empty">
        <span className="empty-icon">
          <Icon.preview size={40} strokeWidth={1} />
        </span>
        <div className="empty-title">Nothing previewed yet</div>
        <div>
          See exactly which files would be added or replaced before anything is
          written.
        </div>
        <button
          className="btn primary"
          onClick={onPreview}
          disabled={busy}
          style={{ marginTop: 8 }}
        >
          <Icon.preview /> Preview changes
        </button>
      </div>
    );
  }
  return (
    <div className="stack">
      <div className="stat-grid">
        <Stat label="Method" value={preview.method} />
        <Stat label="New files" value={String(preview.creates.length)} />
        <Stat label="Replaced" value={String(preview.replaces.length)} />
        <Stat label="Total size" value={formatBytes(preview.totalBytes)} />
      </div>

      {preview.issues.length > 0 && (
        <div className="notice">
          <span style={{ flexShrink: 0 }}>
            <Icon.warning />
          </span>
          <div>
            <div className="notice-title">Some choices need attention</div>
            <div className="notice-body">{preview.issues.join("\n")}</div>
          </div>
        </div>
      )}

      {preview.conflicts.length > 0 && (
        <div className="card stack">
          <div className="card-title">
            Files claimed by more than one mod ({preview.conflicts.length})
          </div>
          <div className="card-hint">
            Mods further down the load order win. Reorder them on the Mods screen
            to change who wins.
          </div>
          <div className="file-list">
            {preview.conflicts.map((c) => (
              <div key={c.path}>
                {c.path} won by <strong>{c.winner}</strong>
              </div>
            ))}
          </div>
        </div>
      )}

      {preview.replaces.length > 0 && (
        <div className="card stack">
          <div className="card-title">Existing files that will be replaced</div>
          <div className="card-hint">
            A copy of each original is kept, and Undo puts them back.
          </div>
          <div className="file-list">
            {preview.replaces.map((p) => (
              <div key={p}>{p}</div>
            ))}
          </div>
        </div>
      )}

      <div className="card stack">
        <div className="card-title">New files ({preview.creates.length})</div>
        <div className="file-list">
          {preview.creates.slice(0, 400).map((p) => (
            <div key={p}>{p}</div>
          ))}
          {preview.creates.length > 400 && (
            <div style={{ opacity: 0.6 }}>
              and {preview.creates.length - 400} more
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function SettingsScreen({
  settings,
  onSettings,
  game,
  onError,
  onInfo,
}: {
  settings: SettingsView | null;
  onSettings: (s: SettingsView) => void;
  game: GameView | null;
  onError: (e: unknown) => void;
  onInfo: (msg: string, kind?: "ok" | "bad" | "info") => void;
}) {
  const appearance = useAppearance();
  if (!settings) return <div className="empty">Loading settings.</div>;

  return (
    <div className="stack">
      <AppearancePanel {...appearance} />

      <DownloadsPanel
        settings={settings}
        onSettings={onSettings}
        onError={onError}
        onInfo={onInfo}
      />

      <div className="card stack">
        <div className="card-title">Where game information comes from</div>
        <div className="card-hint">
          Built in definitions work with no internet. The online source will fetch
          them from the Apocrypha service.
        </div>
        <Segmented
          idPrefix="gamedb"
          value={settings.gameDbSource as "local-builtin" | "online-api"}
          onChange={async (v) => {
            try {
              onSettings(await api.setGameDbSource(v));
            } catch (e) {
              onError(e);
            }
          }}
          options={[
            { value: "local-builtin", label: "Built in" },
            { value: "online-api", label: "Online" },
          ]}
        />
      </div>

      <div className="card stack">
        <div className="card-title">Files</div>
        <dl className="kv">
          <dt>Apocrypha data folder</dt>
          <dd className="mono">{settings.dataRoot}</dd>
          <dt>How files are placed</dt>
          <dd>
            Reflink, then hard link, then copy, whichever the disk supports.
            <div className="card-hint">
              Your mods, backups of replaced files, and the change log all live
              outside the game folder.
            </div>
          </dd>
          {game?.installDir && (
            <>
              <dt>Game folder</dt>
              <dd className="mono">{game.installDir}</dd>
            </>
          )}
        </dl>
      </div>
    </div>
  );
}

function BrowserNotice() {
  return (
    <div className="app" style={{ gridTemplateRows: "1fr" }}>
      <div className="empty" style={{ height: "100%" }}>
        <span className="empty-icon">
          <Icon.package size={40} strokeWidth={1} />
        </span>
        <div className="empty-title">Apocrypha runs as a desktop app</div>
        <div style={{ maxWidth: 420 }}>
          This page is only the interface. Start the full application with{" "}
          <span className="mono">npm run tauri dev</span> so it can reach Steam,
          Proton, and your mod files.
        </div>
      </div>
    </div>
  );
}
