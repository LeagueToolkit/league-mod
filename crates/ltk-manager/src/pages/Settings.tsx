import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { AlertCircle, CheckCircle, FolderOpen, Loader2 } from "lucide-react";
import { useEffect, useState } from "react";

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
      <div className="flex items-center justify-center h-full">
        <Loader2 className="w-8 h-8 text-league-500 animate-spin" />
      </div>
    );
  }

  return (
    <div className="h-full overflow-auto">
      {/* Header */}
      <header className="h-16 flex items-center px-6 border-b border-surface-800">
        <h2 className="text-xl font-semibold text-surface-100">Settings</h2>
      </header>

      <div className="max-w-2xl mx-auto p-6 space-y-8">
        {/* League Path */}
        <section>
          <h3 className="text-lg font-medium text-surface-100 mb-4">League of Legends</h3>
          <div className="space-y-3">
            <span className="block text-sm font-medium text-surface-400">Installation Path</span>
            <div className="flex gap-2">
              <div className="flex-1 relative">
                <input
                  type="text"
                  value={settings.leaguePath || ""}
                  readOnly
                  placeholder="Not configured"
                  className="w-full px-4 py-2.5 bg-surface-800 border border-surface-700 rounded-lg text-surface-100 placeholder:text-surface-500"
                />
                {settings.leaguePath && (
                  <div className="absolute right-3 top-1/2 -translate-y-1/2">
                    {leaguePathValid === true && <CheckCircle className="w-5 h-5 text-green-500" />}
                    {leaguePathValid === false && <AlertCircle className="w-5 h-5 text-red-500" />}
                  </div>
                )}
              </div>
              <button
                type="button"
                onClick={handleBrowse}
                className="px-4 py-2.5 bg-surface-800 hover:bg-surface-700 border border-surface-700 rounded-lg text-surface-300 transition-colors"
              >
                <FolderOpen className="w-5 h-5" />
              </button>
            </div>
            <button
              type="button"
              onClick={handleAutoDetect}
              disabled={isDetecting}
              className="flex items-center gap-2 text-sm text-league-400 hover:text-league-300 transition-colors disabled:opacity-50"
            >
              {isDetecting ? <Loader2 className="w-4 h-4 animate-spin" /> : null}
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
          <h3 className="text-lg font-medium text-surface-100 mb-4">Appearance</h3>
          <div className="space-y-3">
            <span className="block text-sm font-medium text-surface-400">Theme</span>
            <div className="flex gap-2">
              {(["system", "dark", "light"] as const).map((theme) => (
                <button
                  type="button"
                  key={theme}
                  onClick={() => saveSettings({ ...settings, theme })}
                  className={`px-4 py-2 rounded-lg text-sm font-medium capitalize transition-colors ${
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
          <h3 className="text-lg font-medium text-surface-100 mb-4">About</h3>
          <div className="bg-surface-900 border border-surface-800 rounded-lg p-4">
            <p className="text-surface-400 text-sm">
              LTK Manager is part of the LeagueToolkit project. It provides a graphical interface
              for managing League of Legends mods using the modpkg format.
            </p>
            <div className="mt-4 pt-4 border-t border-surface-800">
              <a
                href="https://github.com/LeagueToolkit/league-mod"
                target="_blank"
                rel="noopener noreferrer"
                className="text-sm text-league-400 hover:text-league-300 transition-colors"
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
