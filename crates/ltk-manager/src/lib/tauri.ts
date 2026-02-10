import { invoke } from "@tauri-apps/api/core";

import type { AppError } from "@/utils/errors";
import type { Result } from "@/utils/result";

// Re-export Result utilities for convenience
export type { AppError, ErrorCode } from "@/utils/errors";
export type { Result } from "@/utils/result";
export { isErr, isOk, match, unwrap, unwrapOr } from "@/utils/result";

// Types matching Rust structs
export interface AppInfo {
  name: string;
  version: string;
}

export interface AccentColor {
  preset: string | null; // "blue", "purple", "green", "orange", "pink", "red", "teal"
  customHue: number | null; // 0-360 for custom color
}

export interface Settings {
  leaguePath: string | null;
  modStoragePath: string | null;
  /** Directory where mod projects are stored (for Creator Workshop) */
  workshopPath: string | null;
  theme: "light" | "dark" | "system";
  accentColor: AccentColor;
  firstRunComplete: boolean;
}

export interface InstalledMod {
  id: string;
  name: string;
  displayName: string;
  version: string;
  description?: string;
  authors: string[];
  enabled: boolean;
  installedAt: string;
  filePath: string;
  layers: ModLayer[];
  thumbnailPath?: string;
  modDir: string;
}

export interface ModLayer {
  name: string;
  priority: number;
  enabled: boolean;
}

export interface ModpkgInfo {
  name: string;
  displayName: string;
  version: string;
  description?: string;
  authors: string[];
  layers: LayerInfo[];
  fileCount: number;
  totalSize: number;
}

export interface LayerInfo {
  name: string;
  priority: number;
  description?: string;
  fileCount: number;
}

export interface PatcherConfig {
  logFile?: string | null;
  timeoutMs?: number | null;
  flags?: number | null;
}

export interface PatcherStatus {
  running: boolean;
  configPath: string | null;
}

export interface OverlayProgress {
  stage: "indexing" | "patching" | "complete";
  currentFile: string | null;
  current: number;
  total: number;
}

// Profile types
export interface Profile {
  id: string;
  name: string;
  enabledMods: string[];
  createdAt: string;
  lastUsed: string;
}

// Workshop types
export interface WorkshopProject {
  path: string;
  name: string;
  displayName: string;
  version: string;
  description: string;
  authors: WorkshopAuthor[];
  layers: WorkshopLayer[];
  thumbnailPath?: string;
  lastModified: string;
}

export interface WorkshopAuthor {
  name: string;
  role?: string;
}

export interface WorkshopLayer {
  name: string;
  priority: number;
  description?: string;
  stringOverrides: Record<string, Record<string, string>>;
}

export interface CreateProjectArgs {
  name: string;
  displayName: string;
  description: string;
  authors: string[];
}

export interface PackProjectArgs {
  projectPath: string;
  outputDir?: string;
  format: "modpkg" | "fantome";
}

export interface PackResult {
  outputPath: string;
  fileName: string;
  format: string;
}

export interface ValidationResult {
  valid: boolean;
  errors: string[];
  warnings: string[];
}

/**
 * Raw IPC result from Tauri commands.
 * This matches the Rust IpcResult<T> serialization format.
 */
type IpcResponse<T> = { ok: true; value: T } | { ok: false; error: AppError };

/**
 * Transform the raw IPC response to our Result type.
 */
function toResult<T>(response: IpcResponse<T>): Result<T> {
  if (response.ok) {
    return { ok: true, value: response.value };
  }
  return { ok: false, error: response.error };
}

/**
 * Invoke a Tauri command and return a typed Result.
 */
async function invokeResult<T>(cmd: string, args?: Record<string, unknown>): Promise<Result<T>> {
  const response = await invoke<IpcResponse<T>>(cmd, args);
  return toResult(response);
}

// API functions
export const api = {
  getAppInfo: () => invokeResult<AppInfo>("get_app_info"),

  // Settings
  getSettings: () => invokeResult<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invokeResult<void>("save_settings", { settings }),
  autoDetectLeaguePath: () => invokeResult<string | null>("auto_detect_league_path"),
  validateLeaguePath: (path: string) => invokeResult<boolean>("validate_league_path", { path }),
  checkSetupRequired: () => invokeResult<boolean>("check_setup_required"),

  // Mods
  getInstalledMods: () => invokeResult<InstalledMod[]>("get_installed_mods"),
  installMod: (filePath: string) => invokeResult<InstalledMod>("install_mod", { filePath }),
  uninstallMod: (modId: string) => invokeResult<void>("uninstall_mod", { modId }),
  toggleMod: (modId: string, enabled: boolean) =>
    invokeResult<void>("toggle_mod", { modId, enabled }),
  getModThumbnail: (thumbnailPath: string) =>
    invokeResult<string>("get_mod_thumbnail", { thumbnailPath }),
  getStorageDirectory: () => invokeResult<string>("get_storage_directory"),

  // Inspector
  inspectModpkg: (filePath: string) => invokeResult<ModpkgInfo>("inspect_modpkg", { filePath }),

  // Patcher
  startPatcher: (config: PatcherConfig) => invokeResult<void>("start_patcher", { config }),
  stopPatcher: () => invokeResult<void>("stop_patcher"),
  getPatcherStatus: () => invokeResult<PatcherStatus>("get_patcher_status"),

  // Profiles
  listModProfiles: () => invokeResult<Profile[]>("list_mod_profiles"),
  getActiveModProfile: () => invokeResult<Profile>("get_active_mod_profile"),
  createModProfile: (name: string) => invokeResult<Profile>("create_mod_profile", { name }),
  deleteModProfile: (profileId: string) => invokeResult<void>("delete_mod_profile", { profileId }),
  switchModProfile: (profileId: string) =>
    invokeResult<Profile>("switch_mod_profile", { profileId }),
  renameModProfile: (profileId: string, newName: string) =>
    invokeResult<Profile>("rename_mod_profile", { profileId, newName }),

  // Shell
  revealInExplorer: (path: string) => invokeResult<void>("reveal_in_explorer", { path }),

  // Workshop
  getWorkshopProjects: () => invokeResult<WorkshopProject[]>("get_workshop_projects"),
  createWorkshopProject: (args: CreateProjectArgs) =>
    invokeResult<WorkshopProject>("create_workshop_project", { args }),
  getWorkshopProject: (projectPath: string) =>
    invokeResult<WorkshopProject>("get_workshop_project", { projectPath }),
  saveProjectConfig: (
    projectPath: string,
    displayName: string,
    version: string,
    description: string,
    authors: WorkshopAuthor[],
  ) =>
    invokeResult<WorkshopProject>("save_project_config", {
      projectPath,
      displayName,
      version,
      description,
      authors,
    }),
  deleteWorkshopProject: (projectPath: string) =>
    invokeResult<void>("delete_workshop_project", { projectPath }),
  packWorkshopProject: (args: PackProjectArgs) =>
    invokeResult<PackResult>("pack_workshop_project", { args }),
  importFromModpkg: (filePath: string) =>
    invokeResult<WorkshopProject>("import_from_modpkg", { filePath }),
  validateProject: (projectPath: string) =>
    invokeResult<ValidationResult>("validate_project", { projectPath }),
  setProjectThumbnail: (projectPath: string, imagePath: string) =>
    invokeResult<WorkshopProject>("set_project_thumbnail", { projectPath, imagePath }),
  getProjectThumbnail: (thumbnailPath: string) =>
    invokeResult<string>("get_project_thumbnail", { thumbnailPath }),
  saveLayerStringOverrides: (
    projectPath: string,
    layerName: string,
    stringOverrides: Record<string, Record<string, string>>,
  ) =>
    invokeResult<WorkshopProject>("save_layer_string_overrides", {
      projectPath,
      layerName,
      stringOverrides,
    }),
};
