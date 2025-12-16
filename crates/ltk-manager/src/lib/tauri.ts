import { invoke } from "@tauri-apps/api/core";

import type { AppError } from "@/utils/errors";
import type { Result } from "@/utils/result";

// Re-export Result utilities for convenience
export type { AppError, ErrorCode } from "@/utils/errors";
export type { Result } from "@/utils/result";
export { isErr, isOk, match,unwrap, unwrapOr } from "@/utils/result";

// Types matching Rust structs
export interface AppInfo {
  name: string;
  version: string;
}

export interface Settings {
  leaguePath: string | null;
  modStoragePath: string | null;
  /** Directory where mod projects are stored (for Creator Workshop) */
  workshopPath: string | null;
  theme: "light" | "dark" | "system";
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

  // Inspector
  inspectModpkg: (filePath: string) => invokeResult<ModpkgInfo>("inspect_modpkg", { filePath }),

  // Patcher
  startPatcher: (config: PatcherConfig) => invokeResult<void>("start_patcher", { config }),
  stopPatcher: () => invokeResult<void>("stop_patcher"),
  getPatcherStatus: () => invokeResult<PatcherStatus>("get_patcher_status"),
};
