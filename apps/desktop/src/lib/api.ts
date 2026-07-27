/**
 * Typed bridge to the Rust core.
 *
 * Every call maps 1:1 to a `#[tauri::command]`. When the UI runs in a plain
 * browser (`npm run dev` without Tauri), `invoke` is unavailable: we detect
 * that and serve demo data so the interface is still explorable.
 */

export type SelectMode = "forced" | "exclusive" | "stackable" | "info";

export interface OptionView {
  id: string;
  name: string;
  description: string | null;
  selectMode: SelectMode;
  radioSet: string | null;
  deployable: boolean;
  fileCount: number;
  sizeBytes: number;
  screenshot: string | null;
  /** True when a preview image can be fetched for this option. */
  hasPreview: boolean;
  category: string | null;
}

export interface GroupView {
  index: number | null;
  label: string;
  radioSets: string[];
  options: OptionView[];
}

export interface ModView {
  id: string;
  name: string;
  version: string | null;
  author: string | null;
  category: string | null;
  installerModel: string;
  enabled: boolean;
  priority: number;
  groups: GroupView[];
  selection: string[];
  /** True when this mod's files are currently in the game folder. */
  applied: boolean;
  /** Unix seconds when the mod was imported. */
  addedAt: number;
  totalFiles: number;
  totalBytes: number;
}

export interface GameView {
  id: string;
  name: string;
  engine: string;
  steamAppId: number;
  loadOrder: string;
  installDir: string | null;
  protonPrefix: string | null;
  protonTool: string | null;
  detected: boolean;
  loaderName: string | null;
  loaderDll: string | null;
  loaderOverrideActive: boolean;
  steamLaunchOptions: string | null;
  nexusDomain: string | null;
}

export interface ConflictView {
  path: string;
  contenders: string[];
  winner: string;
}

export interface DryRunView {
  method: string;
  creates: string[];
  replaces: string[];
  missing: string[];
  totalBytes: number;
  fileCount: number;
  conflicts: ConflictView[];
  issues: string[];
}

export interface DeployResultView {
  deploymentId: string;
  filesDeployed: number;
  bytes: number;
  method: string;
}

export interface RollbackView {
  removed: number;
  restored: number;
  skippedModified: string[];
  errors: string[];
  clean: boolean;
}

export interface ProfileView {
  id: number;
  name: string;
  active: boolean;
}

export interface NexusStatusView {
  source: string;
  hasApiKey: boolean;
  userName: string | null;
  isPremium: boolean;
  handlerRegistered: boolean;
  handlerIsDefault: boolean;
  currentHandler: string | null;
  desktopFile: string;
  /** Nexus-issued application id. Browser sign-in needs one. */
  ssoApplication: string;
  canSignIn: boolean;
}

export interface NxmLinkView {
  domain: string;
  modId: number;
  fileId: number;
  hasToken: boolean;
  viewOnly: boolean;
  modPageUrl: string;
}

export type DownloadState = "downloading" | "ready" | "failed" | "cancelled";

export interface DownloadView {
  id: string;
  fileName: string;
  path: string;
  /** Null until the server reports a size, which some mirrors never do. */
  totalBytes: number | null;
  receivedBytes: number;
  state: DownloadState;
  error: string | null;
  /** "Nexus Mods", or "Downloads folder" for a file found rather than fetched. */
  source: string;
  startedAt: number;
  bytesPerSecond: number;
  /** Name of the mod this archive was imported as, when it has been. */
  installedAs: string | null;
}

export interface SettingsView {
  gameDbSource: string;
  dataRoot: string;
  deployMethodPreference: string;
  downloadsDir: string;
  /** False once the user has chosen their own folder. */
  downloadsDirIsDefault: boolean;
}

/** Progress of the running deployment, delivered as `deploy-progress`. */
export interface ApplyProgressView {
  /** "reverting" while the previous deployment is undone, then "linking". */
  phase: string;
  filesDone: number;
  filesTotal: number;
  bytesDone: number;
  bytesTotal: number;
  current: string;
}

/** How the deployment ended. Delivered once, as `deploy-finished`. */
export interface DeployOutcomeView {
  cancelled: boolean;
  result: DeployResultView | null;
  error: string | null;
  /** Present on a cancel, so the interface can say if the undo was complete. */
  rollback: RollbackView | null;
}

export type FileState = "ok" | "missing" | "modified";

export interface FileVerdictView {
  path: string;
  state: FileState;
  /** False when the change log predates staged-path recording. */
  repairable: boolean;
}

export interface VerifyReportView {
  checked: number;
  ok: number;
  problems: FileVerdictView[];
  intact: boolean;
}

export interface RepairReportView {
  repaired: string[];
  skipped: string[];
  errors: string[];
}

/** One folder Apocrypha owns, and what it costs on disk. */
export interface UsageEntryView {
  label: string;
  path: string;
  bytes: number;
  hint: string;
}

export interface StorageUsageView {
  entries: UsageEntryView[];
  total: number;
}

/** True when running inside the Tauri shell (as opposed to a plain browser). */
export const IS_TAURI =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

export async function pickArchive(): Promise<string | null> {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({
    multiple: false,
    filters: [{ name: "Mod archive", extensions: ["zip", "7z", "rar"] }],
  });
  return typeof picked === "string" ? picked : null;
}

export async function pickDirectory(): Promise<string | null> {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({ directory: true, multiple: false });
  return typeof picked === "string" ? picked : null;
}

export const api = {
  listGames: () => call<GameView[]>("list_games"),
  detectGame: (gameId: string) => call<GameView>("detect_game", { gameId }),
  // Which game a Nexus domain belongs to, so a download lands under the right
  // one instead of whichever game happens to be on screen.
  gameForDomain: (domain: string) =>
    call<string | null>("game_for_domain", { domain }),
  setGamePath: (gameId: string, installDir: string, protonPrefix?: string) =>
    call<GameView>("set_game_path", { gameId, installDir, protonPrefix }),

  analyzeArchive: (gameId: string, path: string) =>
    call<ModView>("analyze_archive", { gameId, path }),
  importMod: (gameId: string, archivePath: string, selection: string[]) =>
    call<ModView>("import_mod", { gameId, archivePath, selection }),
  listMods: (gameId: string) => call<ModView[]>("list_mods", { gameId }),
  setModEnabled: (gameId: string, modId: string, enabled: boolean) =>
    call<void>("set_mod_enabled", { gameId, modId, enabled }),
  setModSelection: (gameId: string, modId: string, selection: string[]) =>
    call<string[]>("set_mod_selection", { gameId, modId, selection }),
  setModOrder: (gameId: string, orderedIds: string[]) =>
    call<void>("set_mod_order", { gameId, orderedIds }),
  removeMod: (gameId: string, modId: string) =>
    call<void>("remove_mod", { gameId, modId }),

  /** Previews every enabled mod in the active profile. */
  previewDeploy: (gameId: string) => call<DryRunView>("preview_deploy", { gameId }),
  /**
   * Applies every enabled mod as one transaction. Returns as soon as the work is
   * queued; watch `onDeployProgress` and `onDeployFinished` for the rest.
   */
  startDeploy: (gameId: string) => call<void>("start_deploy", { gameId }),
  /** Asks the running deployment to stop and undo what it has written. */
  cancelDeploy: () => call<void>("cancel_deploy"),
  rollbackLast: (gameId: string) => call<RollbackView>("rollback_last", { gameId }),
  setupLoader: (gameId: string) => call<string>("setup_loader", { gameId }),

  /** Files claimed by more than one enabled mod. Does not touch the disk. */
  conflicts: (gameId: string) => call<ConflictView[]>("list_conflicts", { gameId }),
  setConflictOverride: (gameId: string, path: string, modId: string) =>
    call<void>("set_conflict_override", { gameId, path, modId }),
  clearConflictOverride: (gameId: string, path: string) =>
    call<void>("clear_conflict_override", { gameId, path }),
  conflictOverrides: (gameId: string) =>
    call<Record<string, string>>("conflict_overrides", { gameId }),

  /** Compares the change log against what is actually in the game folder. */
  verifyDeployment: (gameId: string) =>
    call<VerifyReportView>("verify_deployment", { gameId }),
  repairDeployment: (gameId: string, paths: string[]) =>
    call<RepairReportView>("repair_deployment", { gameId, paths }),

  /** Real size on disk of every folder Apocrypha owns. Walks the tree. */
  storageUsage: () => call<StorageUsageView>("storage_usage"),
  openPath: (path: string) => call<void>("open_path", { path }),

  getSettings: () => call<SettingsView>("get_settings"),
  setGameDbSource: (source: string) =>
    call<SettingsView>("set_game_db_source", { source }),
  /** An empty path restores the default downloads folder. */
  setDownloadsDir: (path: string) =>
    call<SettingsView>("set_downloads_dir", { path }),

  listProfiles: (gameId: string) => call<ProfileView[]>("list_profiles", { gameId }),
  createProfile: (gameId: string, name: string) =>
    call<ProfileView[]>("create_profile", { gameId, name }),
  switchProfile: (gameId: string, profileId: number) =>
    call<ProfileView[]>("switch_profile", { gameId, profileId }),
  duplicateProfile: (gameId: string, profileId: number, name: string) =>
    call<ProfileView[]>("duplicate_profile", { gameId, profileId, name }),
  deleteProfile: (gameId: string, profileId: number) =>
    call<ProfileView[]>("delete_profile", { gameId, profileId }),

  previewFromArchive: (gameId: string, archivePath: string, optionId: string) =>
    call<string | null>("preview_from_archive", { gameId, archivePath, optionId }),
  previewFromMod: (gameId: string, modId: string, optionId: string) =>
    call<string | null>("preview_from_mod", { gameId, modId, optionId }),

  steamDiagnostics: () => call<unknown>("steam_diagnostics"),

  /* Nexus Mods */
  nexusStatus: () => call<NexusStatusView>("nexus_status"),
  setDownloadSource: (source: string) =>
    call<NexusStatusView>("set_download_source", { source }),
  setNexusApiKey: (apiKey: string) =>
    call<NexusStatusView>("set_nexus_api_key", { apiKey }),
  registerNxmHandler: () => call<NexusStatusView>("register_nxm_handler"),
  unregisterNxmHandler: () => call<NexusStatusView>("unregister_nxm_handler"),
  parseNxmLink: (url: string) => call<NxmLinkView>("parse_nxm_link", { url }),
  openModPage: (domain: string, modId: number, fileId?: number) =>
    call<string>("open_mod_page", { domain, modId, fileId }),
  /** Queues a download and returns immediately; progress arrives as events. */
  startNxmDownload: (url: string) =>
    call<DownloadView>("start_nxm_download", { url }),
  listDownloads: () => call<DownloadView[]>("list_downloads"),
  cancelDownload: (id: string) => call<void>("cancel_download", { id }),
  removeDownload: (id: string) => call<void>("remove_download", { id }),
  nexusSignIn: () => call<NexusStatusView>("nexus_sign_in"),
  setSsoApplication: (slug: string) =>
    call<NexusStatusView>("set_sso_application", { slug }),

  /** Open a URL in the user's browser. */
  openUrl: async (url: string) => {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    return openUrl(url);
  },
};

/**
 * Attach an async event listener from inside `useEffect`.
 *
 * The obvious spelling leaks:
 *
 *     let dispose;
 *     listen(...).then((un) => { dispose = un; });
 *     return () => dispose?.();
 *
 * React can run the cleanup before the promise resolves, and StrictMode does
 * exactly that on every mount in development. `dispose` is still undefined, so
 * the listener is never removed and the next mount adds a second one. With the
 * nxm listener that meant one download link started two transfers writing the
 * same file. Recording the cancellation up front means a late arrival is
 * disposed immediately instead of outliving the effect.
 */
export function subscribe(start: () => Promise<() => void>): () => void {
  let cancelled = false;
  let stop: (() => void) | undefined;

  start()
    .then((un) => {
      if (cancelled) un();
      else stop = un;
    })
    .catch(() => {
      /* Outside Tauri there are no OS events to listen for. */
    });

  return () => {
    cancelled = true;
    stop?.();
  };
}

/** Subscribe to nxm:// links delivered by the OS. Returns an unsubscribe fn. */
export async function onNxmLink(
  handler: (url: string) => void,
): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  return listen<string>("nxm-url", (e) => handler(e.payload));
}

/**
 * Subscribe to download progress. The whole entry is sent on each change, so the
 * receiver replaces rather than patches and cannot drift out of sync.
 */
export async function onDownloadChanged(
  handler: (d: DownloadView) => void,
): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  return listen<DownloadView>("download-changed", (e) => handler(e.payload));
}

/** Subscribe to deployment progress. Fires several times a second at most. */
export async function onDeployProgress(
  handler: (p: ApplyProgressView) => void,
): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  return listen<ApplyProgressView>("deploy-progress", (e) => handler(e.payload));
}

/** Subscribe to the single final outcome of a deployment. */
export async function onDeployFinished(
  handler: (o: DeployOutcomeView) => void,
): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  return listen<DeployOutcomeView>("deploy-finished", (e) => handler(e.payload));
}

/** "4.2 MB/s", or empty while the rate is not yet meaningful. */
export function formatRate(bytesPerSecond: number): string {
  return bytesPerSecond > 0 ? `${formatBytes(bytesPerSecond)}/s` : "";
}

/** Rough time left, in the coarse units people actually read. */
export function formatEta(
  totalBytes: number | null,
  receivedBytes: number,
  bytesPerSecond: number,
): string {
  if (!totalBytes || bytesPerSecond <= 0) return "";
  const seconds = Math.round((totalBytes - receivedBytes) / bytesPerSecond);
  if (seconds <= 1) return "almost done";
  if (seconds < 60) return `${seconds}s left`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} min left`;
  return `${Math.round(minutes / 60)} hr left`;
}

/**
 * Where an option's preview image should be fetched from. The wizard runs both
 * before import (against the archive) and after (against the staging library).
 */
export type PreviewSource =
  | { kind: "archive"; gameId: string; archivePath: string }
  | { kind: "mod"; gameId: string; modId: string };

export function fetchPreview(
  source: PreviewSource,
  optionId: string,
): Promise<string | null> {
  return source.kind === "archive"
    ? api.previewFromArchive(source.gameId, source.archivePath, optionId)
    : api.previewFromMod(source.gameId, source.modId, optionId);
}

/** Human-readable byte size. */
export function formatBytes(bytes: number): string {
  if (bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let v = bytes;
  let u = 0;
  while (v >= 1024 && u < units.length - 1) {
    v /= 1024;
    u++;
  }
  return `${v.toFixed(v >= 100 || u === 0 ? 0 : 1)} ${units[u]}`;
}

/** Middle-truncate a long path so both ends stay readable. */
export function truncatePath(path: string, max = 56): string {
  if (path.length <= max) return path;
  const head = Math.ceil((max - 1) / 2);
  const tail = Math.floor((max - 1) / 2);
  return `${path.slice(0, head)}…${path.slice(-tail)}`;
}
