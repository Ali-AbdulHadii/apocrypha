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

export interface SettingsView {
  gameDbSource: string;
  dataRoot: string;
  deployMethodPreference: string;
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
    filters: [{ name: "Mod archive", extensions: ["zip"] }],
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

  /** Previews every enabled mod in the active profile. */
  previewDeploy: (gameId: string) => call<DryRunView>("preview_deploy", { gameId }),
  /** Applies every enabled mod as one transaction. */
  deploy: (gameId: string) => call<DeployResultView>("deploy", { gameId }),
  rollbackLast: (gameId: string) => call<RollbackView>("rollback_last", { gameId }),
  setupLoader: (gameId: string) => call<string>("setup_loader", { gameId }),

  getSettings: () => call<SettingsView>("get_settings"),
  setGameDbSource: (source: string) =>
    call<SettingsView>("set_game_db_source", { source }),

  listProfiles: (gameId: string) => call<ProfileView[]>("list_profiles", { gameId }),
  createProfile: (gameId: string, name: string) =>
    call<ProfileView[]>("create_profile", { gameId, name }),
  switchProfile: (gameId: string, profileId: number) =>
    call<ProfileView[]>("switch_profile", { gameId, profileId }),

  previewFromArchive: (gameId: string, archivePath: string, optionId: string) =>
    call<string | null>("preview_from_archive", { gameId, archivePath, optionId }),
  previewFromMod: (gameId: string, modId: string, optionId: string) =>
    call<string | null>("preview_from_mod", { gameId, modId, optionId }),

  steamDiagnostics: () => call<unknown>("steam_diagnostics"),
};

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
