import { getRouteApi } from "@tanstack/react-router";
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { LuCircleAlert, LuCircleCheck, LuFolderOpen, LuInfo, LuLoader } from "react-icons/lu";

import { Button, IconButton } from "@/components/Button";
import { api, type Settings as SettingsType } from "@/lib/tauri";
import { ACCENT_PRESETS, useAppInfo, useSaveSettings, useSettings } from "@/modules/settings";
import { unwrapForQuery } from "@/utils/query";

const routeApi = getRouteApi("/settings");

// Accent color preset display info
const ACCENT_PRESET_DISPLAY: { key: string; label: string; color: string }[] = [
  { key: "blue", label: "Blue", color: "hsl(207, 100%, 50%)" },
  { key: "purple", label: "Purple", color: "hsl(271, 100%, 50%)" },
  { key: "green", label: "Green", color: "hsl(122, 100%, 35%)" },
  { key: "orange", label: "Orange", color: "hsl(36, 100%, 50%)" },
  { key: "pink", label: "Pink", color: "hsl(340, 100%, 50%)" },
  { key: "red", label: "Red", color: "hsl(4, 100%, 50%)" },
  { key: "teal", label: "Teal", color: "hsl(174, 100%, 35%)" },
];

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

  function handleAccentPresetClick(preset: string) {
    if (!settings) return;
    saveSettings({
      ...settings,
      accentColor: { preset, customHue: null },
    });
  }

  function handleCustomHueChange(hue: number) {
    if (!settings) return;
    saveSettings({
      ...settings,
      accentColor: { preset: null, customHue: hue },
    });
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

  async function handleBrowseWorkshopPath() {
    if (!settings) return;

    try {
      const selected = await open({
        directory: true,
        title: "Select Workshop Directory",
      });

      if (selected) {
        saveSettings({ ...settings, workshopPath: selected as string });
      }
    } catch (error) {
      console.error("Failed to browse:", error);
    }
  }

  if (isLoading || !settings) {
    return (
      <div className="flex h-full items-center justify-center">
        <LuLoader className="h-8 w-8 animate-spin text-brand-500" />
      </div>
    );
  }

  // Determine if custom hue is active
  const isCustomHue = settings.accentColor?.customHue != null;
  const currentHue = isCustomHue
    ? settings.accentColor.customHue!
    : settings.accentColor?.preset
      ? (ACCENT_PRESETS[settings.accentColor.preset] ?? 207)
      : 207;

  return (
    <div className="h-full overflow-auto">
      {/* Header */}
      <header className="flex h-16 items-center border-b border-surface-600 px-6">
        <h2 className="text-xl font-semibold text-surface-100">Settings</h2>
      </header>

      <div className="mx-auto max-w-2xl space-y-8 p-6">
        {/* First Run Banner */}
        {firstRun && !settings.leaguePath && (
          <div className="flex items-start gap-3 rounded-lg border border-brand-500/30 bg-brand-500/10 p-4">
            <LuInfo className="mt-0.5 h-5 w-5 shrink-0 text-brand-400" />
            <div>
              <h3 className="font-medium text-brand-300">Welcome to LTK Manager!</h3>
              <p className="mt-1 text-sm text-surface-400">
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
                  className="w-full rounded-lg border border-surface-600 bg-surface-700 px-4 py-2.5 text-surface-200 placeholder:text-surface-500"
                />
                {settings.leaguePath && (
                  <div className="absolute top-1/2 right-3 -translate-y-1/2">
                    {leaguePathValid === true && (
                      <LuCircleCheck className="h-5 w-5 text-green-500" />
                    )}
                    {leaguePathValid === false && (
                      <LuCircleAlert className="h-5 w-5 text-red-500" />
                    )}
                  </div>
                )}
              </div>
              <IconButton
                icon={<LuFolderOpen className="h-5 w-5" />}
                variant="outline"
                size="lg"
                onClick={handleBrowseLeaguePath}
              />
            </div>
            <Button
              variant="transparent"
              size="sm"
              onClick={handleAutoDetect}
              loading={isDetecting}
              left={isDetecting ? undefined : <LuLoader className="h-4 w-4" />}
              className="text-brand-400 hover:text-brand-300"
            >
              Auto-detect installation
            </Button>
            {leaguePathValid === false && settings.leaguePath && (
              <p className="text-sm text-red-400">
                Could not find League of Legends at this path. Make sure it points to the folder
                containing the <code className="rounded bg-surface-700 px-1">Game</code> directory.
              </p>
            )}
          </div>
        </section>

        {/* Mod Storage Path */}
        <section>
          <h3 className="mb-4 text-lg font-medium text-surface-100">Mod Storage</h3>
          <div className="space-y-3">
            <span className="block text-sm font-medium text-surface-400">Storage Location</span>
            <div className="flex gap-2">
              <input
                type="text"
                value={settings.modStoragePath || ""}
                readOnly
                placeholder="Default (app data directory)"
                className="flex-1 rounded-lg border border-surface-600 bg-surface-700 px-4 py-2.5 text-surface-200 placeholder:text-surface-500"
              />
              <IconButton
                icon={<LuFolderOpen className="h-5 w-5" />}
                variant="outline"
                size="lg"
                onClick={handleBrowseModStorage}
              />
            </div>
            <p className="text-sm text-surface-500">
              Choose where your installed mods will be stored. Leave empty to use the default
              location.
            </p>
          </div>
        </section>

        {/* Workshop Directory */}
        <section>
          <h3 className="mb-4 text-lg font-medium text-surface-100">Workshop</h3>
          <div className="space-y-3">
            <span className="block text-sm font-medium text-surface-400">Workshop Directory</span>
            <div className="flex gap-2">
              <input
                type="text"
                value={settings.workshopPath || ""}
                readOnly
                placeholder="Not configured"
                className="flex-1 rounded-lg border border-surface-600 bg-surface-700 px-4 py-2.5 text-surface-200 placeholder:text-surface-500"
              />
              <IconButton
                icon={<LuFolderOpen className="h-5 w-5" />}
                variant="outline"
                size="lg"
                onClick={handleBrowseWorkshopPath}
              />
            </div>
            <p className="text-sm text-surface-500">
              Choose where your mod projects will be stored for the Creator Workshop. This directory
              will contain all your project folders.
            </p>
          </div>
        </section>

        {/* Appearance */}
        <section>
          <h3 className="mb-4 text-lg font-medium text-surface-100">Appearance</h3>

          {/* Theme */}
          <div className="space-y-3">
            <span className="block text-sm font-medium text-surface-400">Theme</span>
            <div className="flex gap-2">
              {(["system", "dark", "light"] as const).map((theme) => (
                <Button
                  key={theme}
                  variant={settings.theme === theme ? "filled" : "default"}
                  size="sm"
                  onClick={() => saveSettings({ ...settings, theme })}
                  className="capitalize"
                >
                  {theme}
                </Button>
              ))}
            </div>
          </div>

          {/* Accent Color */}
          <div className="mt-6 space-y-3">
            <span className="block text-sm font-medium text-surface-400">Accent Color</span>

            {/* Preset Colors */}
            <div className="flex flex-wrap gap-2">
              {ACCENT_PRESET_DISPLAY.map(({ key, label, color }) => (
                <button
                  key={key}
                  onClick={() => handleAccentPresetClick(key)}
                  className={`group relative h-8 w-8 rounded-full transition-transform hover:scale-110 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-brand-500 ${
                    settings.accentColor?.preset === key && !isCustomHue
                      ? "ring-2 ring-surface-100 ring-offset-2 ring-offset-surface-900"
                      : ""
                  }`}
                  style={{ backgroundColor: color }}
                  title={label}
                >
                  {settings.accentColor?.preset === key && !isCustomHue && (
                    <span className="absolute inset-0 flex items-center justify-center">
                      <LuCircleCheck className="h-4 w-4 text-white drop-shadow-md" />
                    </span>
                  )}
                </button>
              ))}
            </div>

            {/* Custom Color Slider */}
            <div className="space-y-2">
              <div className="flex items-center justify-between">
                <span className="text-xs text-surface-500">Custom Color</span>
                {isCustomHue && (
                  <span className="text-xs text-surface-400">Hue: {Math.round(currentHue)}°</span>
                )}
              </div>
              <div className="relative">
                <input
                  type="range"
                  min="0"
                  max="360"
                  value={currentHue}
                  onChange={(e) => handleCustomHueChange(Number(e.target.value))}
                  className="h-3 w-full cursor-pointer appearance-none rounded-full"
                  style={{
                    background: `linear-gradient(to right, 
                      hsl(0, 100%, 50%), 
                      hsl(60, 100%, 50%), 
                      hsl(120, 100%, 50%), 
                      hsl(180, 100%, 50%), 
                      hsl(240, 100%, 50%), 
                      hsl(300, 100%, 50%), 
                      hsl(360, 100%, 50%)
                    )`,
                  }}
                />
                {/* Custom thumb indicator */}
                <div
                  className="pointer-events-none absolute top-1/2 h-5 w-5 -translate-y-1/2 rounded-full border-2 border-white shadow-md"
                  style={{
                    left: `calc(${(currentHue / 360) * 100}% - 10px)`,
                    backgroundColor: `hsl(${currentHue}, 100%, 50%)`,
                  }}
                />
              </div>

              {/* Preview */}
              <div className="flex items-center gap-3">
                <div
                  className="h-6 w-6 rounded"
                  style={{ backgroundColor: `hsl(${currentHue}, 100%, 50%)` }}
                />
                <span className="text-sm text-surface-400">Preview</span>
              </div>
            </div>
          </div>
        </section>

        {/* About */}
        <section>
          <h3 className="mb-4 text-lg font-medium text-surface-100">About</h3>
          <div className="rounded-lg border border-surface-600 bg-surface-800 p-4">
            <div className="flex items-center justify-between">
              <div>
                <h4 className="font-medium text-surface-100">LTK Manager</h4>
                {appInfo && <p className="text-sm text-surface-500">Version {appInfo.version}</p>}
              </div>
            </div>
            <p className="mt-3 text-sm text-surface-400">
              LTK Manager is part of the LeagueToolkit project. It provides a graphical interface
              for managing League of Legends mods using the modpkg format.
            </p>
            <div className="mt-4 flex gap-4 border-t border-surface-600 pt-4">
              <a
                href="https://github.com/LeagueToolkit/league-mod"
                target="_blank"
                rel="noopener noreferrer"
                className="text-sm text-brand-400 transition-colors hover:text-brand-300"
              >
                View on GitHub →
              </a>
              <a
                href="https://github.com/LeagueToolkit/league-mod/wiki"
                target="_blank"
                rel="noopener noreferrer"
                className="text-sm text-brand-400 transition-colors hover:text-brand-300"
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
