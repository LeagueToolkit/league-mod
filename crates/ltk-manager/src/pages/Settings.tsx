import { useEffect, useState } from "react";

import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { AlertCircle, CheckCircle, FolderOpen, Loader2 } from "lucide-react";

interface Settings {
  leaguePath: string | null;
  modStoragePath: string | null;
  theme: "light" | "dark" | "system";
  firstRunComplete: boolean;
}

export function Settings() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [isDetecting, setIsDetecting] = useState(false);
  const [leaguePathValid, setLeaguePathValid] = useState<boolean | null>(null);

  useEffect(() => {
    loadSettings();
  }, []);

  useEffect(() => {
    if (settings?.leaguePath) {
      validatePath(settings.leaguePath);
    }
  }, [settings?.leaguePath]);

  async function loadSettings() {
    try {
      const loadedSettings = await invoke<Settings>("get_settings");
      setSettings(loadedSettings);
    } catch (error) {
      console.error("Failed to load settings:", error);
    }
  }

  async function saveSettings(newSettings: Settings) {
    try {
      await invoke("save_settings", { settings: newSettings });
      setSettings(newSettings);
    } catch (error) {
      console.error("Failed to save settings:", error);
    }
  }

  async function validatePath(path: string) {
    try {
      const valid = await invoke<boolean>("validate_league_path", { path });
      setLeaguePathValid(valid);
    } catch {
      setLeaguePathValid(false);
    }
  }

  async function handleAutoDetect() {
    setIsDetecting(true);
    try {
      const path = await invoke<string | null>("auto_detect_league_path");
      if (path && settings) {
        saveSettings({ ...settings, leaguePath: path });
      }
    } catch (error) {
      console.error("Failed to auto-detect:", error);
    } finally {
      setIsDetecting(false);
    }
  }

  async function handleBrowse() {
    try {
      const selected = await open({
        directory: true,
        title: "Select League of Legends Installation",
      });

      if (selected && settings) {
        saveSettings({ ...settings, leaguePath: selected as string });
      }
    } catch (error) {
      console.error("Failed to browse:", error);
    }
  }

  if (!settings) {
    return (
      <div className="flex h-full items-center justify-center">
        <Loader2 className="text-league-500 h-8 w-8 animate-spin" />
      </div>
    );
  }

  return (
    <div className="h-full overflow-auto">
      {/* Header */}
      <header className="border-surface-800 flex h-16 items-center border-b px-6">
        <h2 className="text-surface-100 text-xl font-semibold">Settings</h2>
      </header>

      <div className="mx-auto max-w-2xl space-y-8 p-6">
        {/* League Path */}
        <section>
          <h3 className="text-surface-100 mb-4 text-lg font-medium">League of Legends</h3>
          <div className="space-y-3">
            <span className="text-surface-400 block text-sm font-medium">Installation Path</span>
            <div className="flex gap-2">
              <div className="relative flex-1">
                <input
                  type="text"
                  value={settings.leaguePath || ""}
                  readOnly
                  placeholder="Not configured"
                  className="bg-surface-800 border-surface-700 text-surface-100 placeholder:text-surface-500 w-full rounded-lg border px-4 py-2.5"
                />
                {settings.leaguePath && (
                  <div className="absolute top-1/2 right-3 -translate-y-1/2">
                    {leaguePathValid === true && <CheckCircle className="h-5 w-5 text-green-500" />}
                    {leaguePathValid === false && <AlertCircle className="h-5 w-5 text-red-500" />}
                  </div>
                )}
              </div>
              <button
                type="button"
                onClick={handleBrowse}
                className="bg-surface-800 hover:bg-surface-700 border-surface-700 text-surface-300 rounded-lg border px-4 py-2.5 transition-colors"
              >
                <FolderOpen className="h-5 w-5" />
              </button>
            </div>
            <button
              type="button"
              onClick={handleAutoDetect}
              disabled={isDetecting}
              className="text-league-400 hover:text-league-300 flex items-center gap-2 text-sm transition-colors disabled:opacity-50"
            >
              {isDetecting ? <Loader2 className="h-4 w-4 animate-spin" /> : null}
              Auto-detect installation
            </button>
            {leaguePathValid === false && settings.leaguePath && (
              <p className="text-sm text-red-400">
                Could not find League of Legends at this path. Make sure it points to the folder
                containing the "Game" directory.
              </p>
            )}
          </div>
        </section>

        {/* Theme */}
        <section>
          <h3 className="text-surface-100 mb-4 text-lg font-medium">Appearance</h3>
          <div className="space-y-3">
            <span className="text-surface-400 block text-sm font-medium">Theme</span>
            <div className="flex gap-2">
              {(["system", "dark", "light"] as const).map((theme) => (
                <button
                  type="button"
                  key={theme}
                  onClick={() => saveSettings({ ...settings, theme })}
                  className={`rounded-lg px-4 py-2 text-sm font-medium capitalize transition-colors ${
                    settings.theme === theme
                      ? "bg-league-500 text-white"
                      : "bg-surface-800 text-surface-400 hover:text-surface-200 hover:bg-surface-700"
                  }`}
                >
                  {theme}
                </button>
              ))}
            </div>
          </div>
        </section>

        {/* About */}
        <section>
          <h3 className="text-surface-100 mb-4 text-lg font-medium">About</h3>
          <div className="bg-surface-900 border-surface-800 rounded-lg border p-4">
            <p className="text-surface-400 text-sm">
              LTK Manager is part of the LeagueToolkit project. It provides a graphical interface
              for managing League of Legends mods using the modpkg format.
            </p>
            <div className="border-surface-800 mt-4 border-t pt-4">
              <a
                href="https://github.com/LeagueToolkit/league-mod"
                target="_blank"
                rel="noopener noreferrer"
                className="text-league-400 hover:text-league-300 text-sm transition-colors"
              >
                View on GitHub →
              </a>
            </div>
          </div>
        </section>
      </div>
    </div>
  );
}
