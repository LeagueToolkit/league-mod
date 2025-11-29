import { invoke } from "@tauri-apps/api/core";

// Types matching Rust structs
export interface AppInfo {
  name: string;
  version: string;
}

export interface Settings {
  leaguePath: string | null;
  modStoragePath: string | null;
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

// API functions
export const api = {
  getAppInfo: () => invoke<AppInfo>("get_app_info"),

  // Settings
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  autoDetectLeaguePath: () => invoke<string | null>("auto_detect_league_path"),
  validateLeaguePath: (path: string) => invoke<boolean>("validate_league_path", { path }),

  // Mods
  getInstalledMods: () => invoke<InstalledMod[]>("get_installed_mods"),
  installMod: (filePath: string) => invoke<InstalledMod>("install_mod", { filePath }),
  uninstallMod: (modId: string) => invoke<void>("uninstall_mod", { modId }),
  toggleMod: (modId: string, enabled: boolean) => invoke<void>("toggle_mod", { modId, enabled }),

  // Inspector
  inspectModpkg: (filePath: string) => invoke<ModpkgInfo>("inspect_modpkg", { filePath }),
};
