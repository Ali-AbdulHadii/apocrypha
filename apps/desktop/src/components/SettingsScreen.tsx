/**
 * Settings.
 *
 * Four sections, one visible at a time, chosen from a row of tabs pinned to the
 * top of the pane. The rail already plays the part of the left column in a
 * system settings window, so this is the right column: grouped inset panels of
 * rows, each row a label on the left and its control on the right.
 *
 * The one group that is allowed to be loud is "Where things live". Mod managers
 * quietly take tens of gigabytes in staging copies and backups of replaced
 * files and never say so. This one shows the folder, the measured size, and a
 * way in. Sizes are computed off the main thread, so the group renders straight
 * away and each row fills in when its number arrives.
 */

import { motion } from "framer-motion";
import {
  useCallback,
  useEffect,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import {
  api,
  formatBytes,
  pickDirectory,
  truncatePath,
  type AppUpdateView,
  type GameView,
  type NexusStatusView,
  type ProfileSourceView,
  type SettingsView,
  type UsageEntryView,
} from "../lib/api";
import { ACCENT_PRESETS, type useAppearance } from "../lib/appearance";
import { useTheme, type ThemeMode } from "../lib/theme";
import type { Confirm } from "./ConfirmDialog";
import { Icon } from "./icons";
import { Chip, Segmented, Switch, pageMotion } from "./ui";
import { SupportAddress, supportMailto } from "../lib/support";

export interface SettingsScreenProps {
  settings: SettingsView | null;
  onSettings: (s: SettingsView) => void;
  game: GameView | null;
  appearance: ReturnType<typeof useAppearance>;
  onError: (e: unknown) => void;
  onInfo: (msg: string, kind?: "ok" | "bad" | "info") => void;
  /** Raise the shared confirmation dialog. Owned by App, which renders it. */
  onConfirm: (c: Confirm) => void;
}

type SectionId = "appearance" | "downloads" | "library" | "advanced";

const SECTIONS: { id: SectionId; label: string }[] = [
  { id: "appearance", label: "Appearance" },
  { id: "downloads", label: "Downloads" },
  { id: "library", label: "Library" },
  { id: "advanced", label: "Advanced" },
];

/**
 * A `nxm://` handler, as a name a person recognises.
 *
 * The backend reports whatever the system holds, and the two systems hold very
 * different things: a desktop entry id on Linux, a full command line on
 * Windows. Neither belongs in a sentence as-is — nobody needs to read
 * `"C:\Program Files\Black Tree Gaming Ltd\Vortex\Vortex.exe" -d "%1"` to learn
 * that Vortex has it.
 */
export function handlerName(handler: string): string {
  const command = handler.trim();
  // Windows: a quoted executable, then arguments that are not ours to show.
  const exe = command.startsWith('"')
    ? command.slice(1).split('"')[0]
    : command.split(/\s/)[0];

  const file = exe.split(/[\\/]/).pop() ?? exe;
  // Linux: `something.desktop`. Windows: `Vortex.exe`. Either way the name is
  // the file without what it is.
  const stripped = file.replace(/\.(desktop|exe)$/i, "");
  return stripped || command;
}

/** How the deploy method preference reads to someone who is not a programmer. */
const PLACEMENT: Record<string, string> = {
  auto: "The fastest your disk allows",
  reflink: "Reflink",
  hardlink: "Hard link",
  symlink: "Symbolic link",
  copy: "Copy",
};

/* ============================================================== screen === */

export function SettingsScreen({
  settings,
  onSettings,
  game,
  appearance,
  onError,
  onInfo,
  onConfirm,
}: SettingsScreenProps) {
  const [section, setSection] = useState<SectionId>("appearance");
  const [status, setStatus] = useState<NexusStatusView | null>(null);

  useEffect(() => {
    api.nexusStatus().then(setStatus).catch(onError);
  }, [onError]);

  // The user can turn motion off in this very screen, so honour it here first.
  const still = appearance.appearance.reduceMotion;
  const anim = still ? {} : pageMotion;

  const onTabKeys = useCallback(
    (e: KeyboardEvent<HTMLDivElement>) => {
      const step =
        e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? SECTIONS.length - 1 : 0;
      if (!step) return;
      e.preventDefault();
      setSection((cur) => {
        const i = SECTIONS.findIndex((s) => s.id === cur);
        const next = SECTIONS[(i + step) % SECTIONS.length]!;
        document.getElementById(`set-tab-${next.id}`)?.focus();
        return next.id;
      });
    },
    [],
  );

  if (!settings) {
    return (
      <div className="empty">
        <div className="empty-title">Getting your settings</div>
        <div>One moment.</div>
      </div>
    );
  }

  return (
    <div className="set-screen">
      <div className="set-tabbar">
        <div
          className="set-tabs"
          role="tablist"
          aria-label="Settings sections"
          onKeyDown={onTabKeys}
        >
          {SECTIONS.map((s) => {
            const active = s.id === section;
            return (
              <button
                key={s.id}
                id={`set-tab-${s.id}`}
                className="set-tab"
                role="tab"
                type="button"
                aria-selected={active}
                aria-controls={`set-panel-${s.id}`}
                tabIndex={active ? 0 : -1}
                onClick={() => setSection(s.id)}
              >
                {active && (
                  <motion.span
                    layoutId="set-tab-pill"
                    className="set-tab-pill"
                    transition={
                      still
                        ? { duration: 0 }
                        : { type: "spring", stiffness: 420, damping: 34 }
                    }
                  />
                )}
                <span>{s.label}</span>
              </button>
            );
          })}
        </div>
      </div>

      {/* Keyed rather than wrapped in `AnimatePresence`, for the reason given
          on `pageMotion`: the waiting mode shows nothing until the outgoing
          panel finishes leaving, and every panel here holds a `Segmented` whose
          pill is a `layoutId`. The same shape emptied the whole content pane
          one level up, and a settings panel that renders nothing would be the
          same bug in a smaller box. */}
      <motion.div
        key={section}
        className="set-section"
        id={`set-panel-${section}`}
        role="tabpanel"
        aria-labelledby={`set-tab-${section}`}
        {...anim}
      >
        {section === "appearance" ? (
          <AppearanceSection appearance={appearance} onInfo={onInfo} />
        ) : section === "downloads" ? (
          <DownloadsSection
            settings={settings}
            onSettings={onSettings}
            status={status}
            onStatus={setStatus}
            onError={onError}
            onInfo={onInfo}
            onConfirm={onConfirm}
          />
        ) : section === "library" ? (
          <LibrarySection
            settings={settings}
            game={game}
            onSettings={onSettings}
            onError={onError}
          />
        ) : (
          <AdvancedSection status={status} onStatus={setStatus} onError={onError} />
        )}
      </motion.div>
    </div>
  );
}

/* ========================================================== row grammar === */

function Group({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="set-group">
      <h2 className="set-eyebrow">{title}</h2>
      <div className="set-panel">{children}</div>
    </section>
  );
}

function Row({
  label,
  desc,
  control,
  stacked,
  className,
}: {
  label: ReactNode;
  desc?: ReactNode;
  control?: ReactNode;
  /** Put the control under the label, for paths, sliders and text fields. */
  stacked?: boolean;
  className?: string;
}) {
  return (
    <div
      className={`set-row${stacked ? " set-row-stacked" : ""}${className ? ` ${className}` : ""}`}
    >
      <div className="set-row-text">
        <div className="set-label">{label}</div>
        {desc && <div className="set-desc">{desc}</div>}
      </div>
      {control && <div className="set-control">{control}</div>}
    </div>
  );
}

function SliderRow({
  id,
  label,
  reading,
  desc,
  min,
  max,
  step,
  value,
  onChange,
}: {
  id: string;
  label: string;
  /** The current value in words, shown beside the label. */
  reading: string;
  desc?: string;
  min: number;
  max: number;
  step: number;
  value: number;
  onChange: (v: number) => void;
}) {
  return (
    <div className="set-row set-row-stacked">
      <div className="set-row-text">
        <label className="set-label" htmlFor={id}>
          {label}
          <span className="set-value">{reading}</span>
        </label>
        {desc && <div className="set-desc">{desc}</div>}
      </div>
      <div className="set-control">
        <input
          id={id}
          className="range"
          type="range"
          min={min}
          max={max}
          step={step}
          value={value}
          onChange={(e) => onChange(Number(e.target.value))}
        />
      </div>
    </div>
  );
}

/* ========================================================== appearance === */

function AppearanceSection({
  appearance: { appearance: a, set, reset },
  onInfo,
}: {
  appearance: ReturnType<typeof useAppearance>;
  onInfo: (msg: string, kind?: "ok" | "bad" | "info") => void;
}) {
  const { mode, setMode } = useTheme();

  return (
    <>
      <Group title="Colour">
        <Row
          label="Theme"
          control={
            <Segmented<ThemeMode>
              idPrefix="set-theme"
              value={mode}
              onChange={setMode}
              options={[
                { value: "light", label: "Light" },
                { value: "dark", label: "Dark" },
                { value: "system", label: "Match system" },
              ]}
            />
          }
        />

        <Row
          label="Accent colour"
          control={
            <div className="swatches">
              {ACCENT_PRESETS.map((p) => {
                const active = a.accentHue === p.hue && a.accentSat === p.sat;
                return (
                  <button
                    key={p.name}
                    type="button"
                    className={`swatch ${active ? "active" : ""}`}
                    title={p.name}
                    aria-label={p.name}
                    aria-pressed={active}
                    onClick={() => {
                      set("accentHue", p.hue);
                      set("accentSat", p.sat);
                      set("accentHue2", p.hue2);
                    }}
                    style={{
                      background: `linear-gradient(135deg, hsl(${p.hue} ${p.sat}% 44%), hsl(${p.hue2} ${p.sat - 4}% 38%))`,
                    }}
                  />
                );
              })}
            </div>
          }
        />

        <SliderRow
          id="set-accent-sat"
          label="Colour intensity"
          reading={`${a.accentSat}%`}
          desc="How strong the accent reads. Lower is closer to grey."
          min={12}
          max={72}
          step={2}
          value={a.accentSat}
          onChange={(v) => set("accentSat", v)}
        />

        <Row
          label="Gradient accent"
          desc="Blends two hues on accented surfaces instead of using one flat colour."
          control={
            <Switch
              checked={a.gradient}
              onChange={(v) => set("gradient", v)}
              label="Use a gradient accent"
            />
          }
        />
      </Group>

      <Group title="Size and spacing">
        <SliderRow
          id="set-text-size"
          label="Text size"
          reading={`${a.baseSize}px`}
          desc="Everything else scales from this, so the layout keeps its proportions."
          min={12}
          max={18}
          step={1}
          value={a.baseSize}
          onChange={(v) => set("baseSize", v)}
        />
        <SliderRow
          id="set-radius"
          label="Corner rounding"
          reading={`${Math.round(a.radiusScale * 100)}%`}
          min={0}
          max={2}
          step={0.25}
          value={a.radiusScale}
          onChange={(v) => set("radiusScale", v)}
        />
        <SliderRow
          id="set-density"
          label="Spacing"
          reading={`${Math.round(a.density * 100)}%`}
          desc="Tighter fits more on screen, looser is easier to scan."
          min={0.75}
          max={1.5}
          step={0.25}
          value={a.density}
          onChange={(v) => set("density", v)}
        />
      </Group>

      <Group title="Depth and motion">
        <SliderRow
          id="set-ambient"
          label="Background glow"
          reading={`${Math.round(a.ambient * 100)}%`}
          desc="A soft wash behind the window. Set it to nothing for a flat background."
          min={0}
          max={1.5}
          step={0.25}
          value={a.ambient}
          onChange={(v) => set("ambient", v)}
        />
        <Row
          label="Reduce motion"
          desc="Turns off animation everywhere, whatever the system is set to."
          control={
            <Switch
              checked={a.reduceMotion}
              onChange={(v) => set("reduceMotion", v)}
              label="Reduce motion"
            />
          }
        />
      </Group>

      <Group title="Start over">
        <Row
          label="Reset appearance"
          desc="Puts colour, text size, spacing and rounding back to the way Apocrypha shipped. Your light or dark choice is kept."
          control={
            <button
              type="button"
              className="btn sm"
              onClick={() => {
                reset();
                onInfo("Appearance reset", "ok");
              }}
            >
              <Icon.refresh size={14} /> Reset
            </button>
          }
        />
      </Group>
    </>
  );
}

/* =========================================================== downloads === */

function DownloadsSection({
  settings,
  onSettings,
  status,
  onStatus,
  onError,
  onInfo,
  onConfirm,
}: {
  settings: SettingsView;
  onSettings: (s: SettingsView) => void;
  status: NexusStatusView | null;
  onStatus: (s: NexusStatusView) => void;
  onError: (e: unknown) => void;
  onInfo: (msg: string, kind?: "ok" | "bad" | "info") => void;
  onConfirm: (c: Confirm) => void;
}) {
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);

  async function run<T>(fn: () => Promise<T>, ok?: string): Promise<T | undefined> {
    setBusy(true);
    try {
      const r = await fn();
      if (ok) onInfo(ok, "ok");
      return r;
    } catch (e) {
      onError(e);
      return undefined;
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <Group title="Files">
        <Row
          stacked
          label="Downloads folder"
          desc={
            "Files already downloaded stay where they are. Point this at a folder you " +
            "already use and Apocrypha lists what is in it, so archives saved by another " +
            "manager can be installed from here."
          }
          control={
            <>
              <span className="set-path mono" title={settings.downloadsDir}>
                {truncatePath(settings.downloadsDir, 64)}
              </span>
              <button
                type="button"
                className="btn sm"
                disabled={busy}
                onClick={async () => {
                  const dir = await pickDirectory();
                  if (!dir) return;
                  const next = await run(
                    () => api.setDownloadsDir(dir),
                    "Downloads folder changed",
                  );
                  if (next) onSettings(next);
                }}
              >
                <Icon.folder size={14} /> Choose folder
              </button>
              {!settings.downloadsDirIsDefault && (
                <button
                  type="button"
                  className="btn sm"
                  disabled={busy}
                  onClick={async () => {
                    const next = await run(
                      () => api.setDownloadsDir(""),
                      "Back to the default folder",
                    );
                    if (next) onSettings(next);
                  }}
                >
                  Use default
                </button>
              )}
            </>
          }
        />
      </Group>

      {status && (
        <>
          <Group title="Where mods come from">
            <Row
              label="Get mods from"
              desc={
                status.source === "nexus"
                  ? undefined
                  : "The Apocrypha service is not running yet. Downloads still come from Nexus Mods until it is."
              }
              control={
                <Segmented<"nexus" | "apocrypha">
                  idPrefix="set-source"
                  value={status.source === "nexus" ? "nexus" : "apocrypha"}
                  onChange={async (v) => {
                    const next = await run(() => api.setDownloadSource(v));
                    if (next) onStatus(next);
                  }}
                  options={[
                    { value: "nexus", label: "Nexus Mods" },
                    { value: "apocrypha", label: "Apocrypha" },
                  ]}
                />
              }
            />

            <Row
              label="Download links from the website"
              desc={
                "Lets the Mod Manager Download button on Nexus Mods send a file straight " +
                "here instead of you saving it by hand."
              }
              control={
                <>
                  {status.handlerIsDefault ? (
                    <Chip kind="ok">
                      <span className="dot" /> Apocrypha opens them
                    </Chip>
                  ) : status.currentHandler ? (
                    <Chip kind="warn">
                      {handlerName(status.currentHandler)} opens them
                    </Chip>
                  ) : (
                    <Chip kind="warn">Not set up</Chip>
                  )}
                  {status.handlerIsDefault ? (
                    <button
                      type="button"
                      className="btn sm"
                      disabled={busy}
                      onClick={async () => {
                        // No message passed to `run`: what to say depends on
                        // where the scheme ended up, which only the status that
                        // comes back can answer.
                        const next = await run(() =>
                          api.unregisterNxmHandler(),
                        );
                        if (!next) return;
                        onStatus(next);
                        onInfo(
                          next.currentHandler
                            ? `${handlerName(next.currentHandler)} opens those links again`
                            : "Apocrypha no longer opens those links",
                          "ok",
                        );
                      }}
                    >
                      Turn off
                    </button>
                  ) : (
                    <button
                      type="button"
                      className="btn sm primary"
                      disabled={busy}
                      onClick={() => {
                        const register = async () => {
                          const next = await run(() =>
                            api.registerNxmHandler(),
                          );
                          if (!next) return;
                          onStatus(next);
                          // The reported result, not the absence of an error.
                          // A desktop environment can accept the request and
                          // keep its own default, and saying "done" then is how
                          // this row came to claim a handler it had not taken.
                          if (next.handlerIsDefault) {
                            onInfo(
                              "Apocrypha now opens Nexus Mods download links",
                              "ok",
                            );
                          } else if (next.currentHandler) {
                            onInfo(
                              `${handlerName(next.currentHandler)} still opens those links. Your desktop kept its own default.`,
                              "bad",
                            );
                          } else {
                            onInfo(
                              "Nothing opens those links yet. The system did not accept the change.",
                              "bad",
                            );
                          }
                        };

                        // Taking the scheme reconfigures another installed
                        // program, so it is asked about by name. Nothing to ask
                        // when nothing holds it.
                        if (status.currentHandler) {
                          const who = handlerName(status.currentHandler);
                          onConfirm({
                            title: "Take over download links?",
                            body:
                              `${who} currently opens Nexus Mods download links. ` +
                              `Apocrypha will open them instead, and turning this ` +
                              `off later hands them back to ${who}.`,
                            confirmLabel: "Take over",
                            onConfirm: () => void register(),
                          });
                        } else {
                          void register();
                        }
                      }}
                    >
                      Set up
                    </button>
                  )}
                </>
              }
            />
          </Group>

          <Group title="Nexus Mods account">
            {status.hasApiKey ? (
              <>
                <Row
                  label={status.userName ?? "Signed in"}
                  desc="Your key is kept on this computer and only ever sent to Nexus Mods."
                  control={
                    <>
                      {status.isPremium ? (
                        <Chip kind="accent">Premium</Chip>
                      ) : (
                        <Chip>Free account</Chip>
                      )}
                      <button
                        type="button"
                        className="btn sm"
                        disabled={busy}
                        onClick={async () => {
                          const next = await run(
                            () => api.setNexusApiKey(""),
                            "Signed out of Nexus Mods",
                          );
                          if (next) onStatus(next);
                          setKey("");
                        }}
                      >
                        Sign out
                      </button>
                    </>
                  }
                />
                {!status.isPremium && (
                  <Row
                    stacked
                    label="How downloading works on a free account"
                    desc={
                      "Nexus Mods only lets a mod manager fetch a file after you press Mod " +
                      "Manager Download on the website. Apocrypha opens the mod page for you, " +
                      "and the moment you press that button the file comes straight here. " +
                      "That is a Nexus Mods rule, not a missing feature."
                    }
                  />
                )}
              </>
            ) : (
              <>
                <Row
                  label="Sign in with Nexus Mods"
                  desc={
                    status.canSignIn
                      ? "Opens your browser. There is no key to copy."
                      : "Nexus Mods issues the application id this needs only to " +
                        "developers who ask for one, and Apocrypha does not have one " +
                        "yet. Paste a personal key below, or enter an id under Advanced."
                  }
                  control={
                    <button
                      type="button"
                      className="btn primary"
                      disabled={busy || !status.canSignIn}
                      onClick={async () => {
                        onInfo("Approve the sign-in in your browser", "info");
                        const next = await run(
                          () => api.nexusSignIn(),
                          "Signed in to Nexus Mods",
                        );
                        if (next) onStatus(next);
                      }}
                    >
                      Sign in
                    </button>
                  }
                />

                <Row
                  stacked
                  label="Personal API key"
                  desc="Kept on this computer and only ever sent to Nexus Mods."
                  control={
                    <>
                      <input
                        className="input"
                        type="password"
                        placeholder="Paste your key"
                        aria-label="Personal API key"
                        value={key}
                        onChange={(e) => setKey(e.target.value)}
                      />
                      <button
                        type="button"
                        className="btn"
                        disabled={busy || !key.trim()}
                        onClick={async () => {
                          const next = await run(
                            () => api.setNexusApiKey(key.trim()),
                            "Nexus Mods account connected",
                          );
                          if (next) {
                            onStatus(next);
                            setKey("");
                          }
                        }}
                      >
                        Connect
                      </button>
                      <button
                        type="button"
                        className="btn ghost sm"
                        onClick={() =>
                          api
                            .openUrl("https://next.nexusmods.com/settings/api-keys")
                            .catch(onError)
                        }
                      >
                        Get a key
                      </button>
                    </>
                  }
                />
              </>
            )}
          </Group>
        </>
      )}
    </>
  );
}

/* ============================================================= library === */

function LibrarySection({
  settings,
  game,
  onSettings,
  onError,
}: {
  settings: SettingsView;
  game: GameView | null;
  onSettings: (s: SettingsView) => void;
  onError: (e: unknown) => void;
}) {
  const [entries, setEntries] = useState<UsageEntryView[] | null>(null);
  const [total, setTotal] = useState(0);
  const [measured, setMeasured] = useState<"waiting" | "done" | "failed">("waiting");

  useEffect(() => {
    let live = true;
    api
      .storageUsage()
      .then((u) => {
        if (!live) return;
        setEntries(u.entries);
        setTotal(u.total);
        setMeasured("done");
      })
      .catch((e) => {
        if (!live) return;
        setMeasured("failed");
        onError(e);
      });
    return () => {
      live = false;
    };
  }, [onError]);

  // The two folders are known before their sizes are, so the group can be drawn
  // in full straight away and each row can wait on its own number.
  const rows: UsageEntryView[] = entries ?? [
    {
      label: "Mod library",
      path: settings.dataRoot,
      bytes: 0,
      hint: "Apocrypha's own copy of every mod, plus backups of files it replaced.",
    },
    {
      label: "Downloads",
      path: settings.downloadsDir,
      bytes: 0,
      hint: "Archives you have downloaded or dropped in yourself.",
    },
  ];

  const size = (bytes: number) =>
    measured === "waiting" ? (
      <span className="set-size-wait skeleton" aria-label="Measuring" />
    ) : measured === "failed" ? (
      "not measured"
    ) : (
      formatBytes(bytes)
    );

  return (
    <>
      <Group title="Where things live">
        {rows.map((e) => (
          <Row
            // Keyed by label, not path: with more than one game set up, several
            // rows deliberately report the same shared folder.
            key={e.label}
            label={e.label}
            desc={
              <>
                <div className="set-path mono">{e.path}</div>
                {e.hint}
              </>
            }
            control={
              <>
                <span className="set-size">{size(e.bytes)}</span>
                <button
                  type="button"
                  className="btn sm"
                  onClick={() => api.openPath(e.path).catch(onError)}
                  aria-label={`Open ${e.label} in the file manager`}
                >
                  <Icon.folder size={14} /> Open
                </button>
              </>
            }
          />
        ))}
        <Row
          className="set-total"
          label="Total on disk"
          desc={
            "Everything Apocrypha keeps, all of it outside your game folder. These " +
            "are measured sizes, not estimates."
          }
          control={<span className="set-size">{size(total)}</span>}
        />
      </Group>

      <Group title="The game">
        <Row
          stacked
          label="Game folder"
          desc={
            "Where mods are placed when you apply them. Change it on the Library " +
            "screen with Find game or Choose folder."
          }
          control={
            <>
              <span className="set-path mono" title={game?.installDir ?? undefined}>
                {game?.installDir ? truncatePath(game.installDir, 64) : "Not set yet"}
              </span>
              <button
                type="button"
                className="btn sm"
                disabled={!game?.installDir}
                onClick={() =>
                  game?.installDir && api.openPath(game.installDir).catch(onError)
                }
              >
                <Icon.folder size={14} /> Open
              </button>
            </>
          }
        />

        <Row
          label="How files are placed"
          desc={
            "Apocrypha tries the cheapest method your disk supports first, so applying a " +
            "large mod list does not mean a second copy of every file."
          }
          control={
            <span className="set-value">
              {PLACEMENT[settings.deployMethodPreference] ??
                settings.deployMethodPreference}
            </span>
          }
        />
      </Group>

      <Group title="Game information">
        <Row
          label="Where game information comes from"
          desc={
            "Built in definitions work with no internet. Online fetches them from " +
            "the Apocrypha service."
          }
          control={
            <Segmented<"local-builtin" | "online-api">
              idPrefix="set-gamedb"
              value={settings.gameDbSource === "online-api" ? "online-api" : "local-builtin"}
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
          }
        />
        <GameDbStatus
          selected={settings.gameDbSource === "online-api" ? "online-api" : "local-builtin"}
          onError={onError}
        />
      </Group>
    </>
  );
}

/**
 * What game information is actually being used.
 *
 * Separate from the setting because the two can disagree: choosing Online and
 * then being unable to reach the service looks, from the outside, exactly like
 * it working. An app quietly using definitions from months ago is worse than
 * one that says it is.
 */
function GameDbStatus({
  selected,
  onError,
}: {
  selected: "local-builtin" | "online-api";
  onError: (e: unknown) => void;
}) {
  const [status, setStatus] = useState<ProfileSourceView | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  const load = useCallback(() => {
    api
      .gameDbStatus()
      .then(setStatus)
      .catch(() => setStatus(null));
  }, []);

  useEffect(load, [load, selected]);

  if (selected !== "online-api") {
    return null;
  }

  const when =
    status?.fetchedAt != null
      ? new Date(status.fetchedAt * 1000).toLocaleString()
      : null;

  return (
    <Row
      label="Published game information"
      desc={
        status?.onlineInEffect
          ? `${status.published} ${status.published === 1 ? "game" : "games"} from the service${when ? `, last updated ${when}` : ""}.`
          : "Nothing has been fetched yet, so the built in definitions are in use. Modding never waits on the service."
      }
      control={
        <div className="row" style={{ gap: "var(--sp-2)" }}>
          {!status?.onlineInEffect && <Chip kind="warn">Built in</Chip>}
          <button
            className="btn sm"
            disabled={busy}
            onClick={async () => {
              setBusy(true);
              setMessage(null);
              try {
                setMessage(await api.refreshGameDb());
                load();
              } catch (e) {
                onError(e);
              } finally {
                setBusy(false);
              }
            }}
          >
            {busy ? "Checking…" : "Refresh now"}
          </button>
          {message && <span className="card-hint">{message}</span>}
        </div>
      }
    />
  );
}

/* ============================================================ advanced === */


/**
 * Whether a newer Apocrypha exists.
 *
 * Checks once when the section is opened rather than at startup: it is a
 * network call for information nobody is waiting on, and an app that reaches
 * out before its window has finished appearing is doing the wrong thing with
 * someone's first second.
 *
 * A failed check says nothing. Offline, rate-limited and GitHub-down all mean
 * "we do not know", and complaining about that every time is worse than
 * silence.
 */
function AppUpdateGroup() {
  const [state, setState] = useState<AppUpdateView | null>(null);
  const [checking, setChecking] = useState(true);

  useEffect(() => {
    let alive = true;
    api
      .checkAppUpdate()
      .then((r) => alive && setState(r))
      .catch(() => {})
      .finally(() => alive && setChecking(false));
    return () => {
      alive = false;
    };
  }, []);

  const how =
    state?.installKind === "appImage"
      ? "Download the new AppImage and replace this one."
      : state?.installKind === "package"
        ? "Update through your package manager, or install the new package from the releases page."
        : "This is a development build. Pull and rebuild.";

  return (
    <Group title="Apocrypha">
      <Row
        label="Version"
        desc={
          checking
            ? "Checking for a newer release."
            : !state
              ? "Could not reach GitHub, so whether a newer release exists is unknown."
              : state.available
                ? `${state.latest} is available. ${how}`
                : "This is the newest release."
        }
        control={
          <div className="row" style={{ gap: "var(--sp-2)" }}>
            <Chip kind={state?.available ? "ok" : undefined}>
              {state?.current ?? "—"}
            </Chip>
            {state?.available ? (
              <button
                className="btn primary sm"
                onClick={() => void api.openUrl(state.url)}
              >
                Get it
              </button>
            ) : null}
          </div>
        }
      />
      <Row
        label="Support"
        desc="For anything the app cannot sort out itself. Bugs are better as issues on GitHub, where someone else hitting the same thing can find them."
        control={
          <button className="btn sm" onClick={() => void api.openUrl(supportMailto())}>
            {SupportAddress}
          </button>
        }
      />
    </Group>
  );
}

function AdvancedSection({
  status,
  onStatus,
  onError,
}: {
  status: NexusStatusView | null;
  onStatus: (s: NexusStatusView) => void;
  onError: (e: unknown) => void;
}) {
  if (!status) {
    return (
      <div className="empty">
        <div className="empty-title">Reading your Nexus Mods setup</div>
        <div>One moment.</div>
      </div>
    );
  }

  return (
    <>
      <AppUpdateGroup />
      <Group title="Nexus Mods">
        <Row
          stacked
          label="Application id"
          desc={
            "Turns on the Sign in with Nexus Mods button. Only Nexus Mods can issue " +
            "one, so leave this empty unless they gave you an id."
          }
          control={
            <input
              className="input"
              key={status.ssoApplication}
              defaultValue={status.ssoApplication}
              placeholder="Issued by Nexus Mods"
              aria-label="Nexus Mods application id"
              onBlur={async (e) => {
                const v = e.target.value.trim();
                if (v === status.ssoApplication) return;
                try {
                  onStatus(await api.setSsoApplication(v));
                } catch (err) {
                  onError(err);
                }
              }}
            />
          }
        />
      </Group>

      <Group title="Link handling">
        <Row
          stacked
          label="Where the registration lives"
          desc={
            "Where the system records that Apocrypha can open nxm links. " +
            "Setting up download links on the Downloads tab writes it for you."
          }
          control={
            <span className="set-path mono" title={status.handlerLocation}>
              {truncatePath(status.handlerLocation, 64)}
            </span>
          }
        />
      </Group>
    </>
  );
}
