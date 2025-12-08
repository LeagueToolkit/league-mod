import { getRouteApi } from "@tanstack/react-router";
import { open } from "@tauri-apps/plugin-dialog";
import { AlertCircle, CheckCircle, FolderOpen, Info, Loader2 } from "lucide-react";
import { useEffect, useState } from "react";

import { api, type Settings as SettingsType } from "@/lib/tauri";
import { useAppInfo, useSaveSettings, useSettings } from "@/modules/settings";
import { unwrapForQuery } from "@/utils/query";

const routeApi = getRouteApi("/settings");

export function Settings() {
  const { firstRun } = routeApi.useSearch();
  const { data: settings, isLoading } = useSettings();
  const { data: appInfo } = useAppInfo();
  const saveSettingsMutation = useSaveSettings();

  const [isDetecting, setIsDetecting] = useState(false);
  const [leaguePathValid, setLeaguePathValid] = useState<boolean | null>(null);

  useEffect(() => {
    if (settings?.leaguePath) {
      validatePath(settings.leaguePath);
    } else {
      setLeaguePathValid(null);
    }
  }, [settings?.leaguePath]);

  async function validatePath(path: string) {
    try {
      const result = await api.validateLeaguePath(path);
      setLeaguePathValid(unwrapForQuery(result));
    } catch {
      setLeaguePathValid(false);
    }
  }

  function saveSettings(newSettings: SettingsType) {
    saveSettingsMutation.mutate(newSettings);
  }

  async function handleAutoDetect() {
    if (!settings) return;

    setIsDetecting(true);
    try {
      const result = await api.autoDetectLeaguePath();
      const path = unwrapForQuery(result);
      if (path) {
        saveSettings({ ...settings, leaguePath: path, firstRunComplete: true });
      }
    } catch (error) {
      console.error("Failed to auto-detect:", error);
    } finally {
      setIsDetecting(false);
    }
  }

  async function handleBrowseLeaguePath() {
    if (!settings) return;

    try {
      const selected = await open({
        directory: true,
        title: "Select League of Legends Installation",
      });

      if (selected) {
        saveSettings({ ...settings, leaguePath: selected as string, firstRunComplete: true });
      }
    } catch (error) {
      console.error("Failed to browse:", error);
    }
  }

  async function handleBrowseModStorage() {
    if (!settings) return;

    try {
      const selected = await open({
        directory: true,
        title: "Select Mod Storage Location",
      });

      if (selected) {
        saveSettings({ ...settings, modStoragePath: selected as string });
      }
    } catch (error) {
      console.error("Failed to browse:", error);
    }
  }

  if (isLoading || !settings) {
    return (
      <div className="flex h-full items-center justify-center">
        <Loader2 className="text-league-500 h-8 w-8 animate-spin" />
      </div>
    );
  }

  return (
    <div className="h-full overflow-auto">
      {/* Header */}
      <header className="flex h-16 items-center border-b border-surface-800 px-6">
        <h2 className="text-xl font-semibold text-surface-100">Settings</h2>
      </header>

      <div className="mx-auto max-w-2xl space-y-8 p-6">
        {/* First Run Banner */}
        {firstRun && !settings.leaguePath && (
          <div className="bg-league-500/10 border-league-500/30 flex items-start gap-3 rounded-lg border p-4">
            <Info className="text-league-400 mt-0.5 h-5 w-5 shrink-0" />
            <div>
              <h3 className="text-league-300 font-medium">Welcome to LTK Manager!</h3>
              <p className="text-surface-400 mt-1 text-sm">
                To get started, please configure your League of Legends installation path below. You
                can use auto-detection or browse to the folder manually.
              </p>
            </div>
          </div>
        )}

        {/* League Path */}
        <section>
          <h3 className="mb-4 text-lg font-medium text-surface-100">League of Legends</h3>
          <div className="space-y-3">
            <span className="block text-sm font-medium text-surface-400">Installation Path</span>
            <div className="flex gap-2">
              <div className="relative flex-1">
                <input
                  type="text"
                  value={settings.leaguePath || ""}
                  readOnly
                  placeholder="Not configured"
                  className="w-full rounded-lg border border-surface-700 bg-surface-800 px-4 py-2.5 text-surface-100 placeholder:text-surface-500"
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
                onClick={handleBrowseLeaguePath}
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
                containing the <code className="bg-surface-700 rounded px-1">Game</code> directory.
              </p>
            )}
          </div>
        </section>

        {/* Mod Storage Path */}
        <section>
          <h3 className="text-surface-100 mb-4 text-lg font-medium">Mod Storage</h3>
          <div className="space-y-3">
            <span className="text-surface-400 block text-sm font-medium">Storage Location</span>
            <div className="flex gap-2">
              <input
                type="text"
                value={settings.modStoragePath || ""}
                readOnly
                placeholder="Default (app data directory)"
                className="bg-surface-800 border-surface-700 text-surface-100 placeholder:text-surface-500 flex-1 rounded-lg border px-4 py-2.5"
              />
              <button
                type="button"
                onClick={handleBrowseModStorage}
                className="bg-surface-800 hover:bg-surface-700 border-surface-700 text-surface-300 rounded-lg border px-4 py-2.5 transition-colors"
              >
                <FolderOpen className="h-5 w-5" />
              </button>
            </div>
            <p className="text-surface-500 text-sm">
              Choose where your installed mods will be stored. Leave empty to use the default
              location.
            </p>
          </div>
        </section>

        {/* Theme */}
        <section>
          <h3 className="mb-4 text-lg font-medium text-surface-100">Appearance</h3>
          <div className="space-y-3">
            <span className="block text-sm font-medium text-surface-400">Theme</span>
            <div className="flex gap-2">
              {(["system", "dark", "light"] as const).map((theme) => (
                <button
                  type="button"
                  key={theme}
                  onClick={() => saveSettings({ ...settings, theme })}
                  className={`rounded-lg px-4 py-2 text-sm font-medium capitalize transition-colors ${
                    settings.theme === theme
                      ? "bg-league-500 text-white"
                      : "bg-surface-800 text-surface-400 hover:bg-surface-700 hover:text-surface-200"
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
            <div className="flex items-center justify-between">
              <div>
                <h4 className="text-surface-100 font-medium">LTK Manager</h4>
                {appInfo && <p className="text-surface-500 text-sm">Version {appInfo.version}</p>}
              </div>
            </div>
            <p className="text-surface-400 mt-3 text-sm">
              LTK Manager is part of the LeagueToolkit project. It provides a graphical interface
              for managing League of Legends mods using the modpkg format.
            </p>
            <div className="border-surface-800 mt-4 flex gap-4 border-t pt-4">
              <a
                href="https://github.com/LeagueToolkit/league-mod"
                target="_blank"
                rel="noopener noreferrer"
                className="text-league-400 hover:text-league-300 text-sm transition-colors"
              >
                View on GitHub →
              </a>
              <a
                href="https://github.com/LeagueToolkit/league-mod/wiki"
                target="_blank"
                rel="noopener noreferrer"
                className="text-league-400 hover:text-league-300 text-sm transition-colors"
              >
                Documentation →
              </a>
            </div>
          </div>
        </section>
      </div>
    </div>
  );
}
