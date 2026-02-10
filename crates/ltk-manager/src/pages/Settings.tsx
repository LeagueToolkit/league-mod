import { getRouteApi } from "@tanstack/react-router";
import { LuInfo, LuLoader } from "react-icons/lu";

import {
  AboutSection,
  AppearanceSection,
  LeaguePathSection,
  ModStorageSection,
  useAppInfo,
  useSaveSettings,
  useSettings,
  WorkshopSection,
} from "@/modules/settings";

const routeApi = getRouteApi("/settings");

export function Settings() {
  const { firstRun } = routeApi.useSearch();
  const { data: settings, isLoading } = useSettings();
  const { data: appInfo } = useAppInfo();
  const saveSettingsMutation = useSaveSettings();

  if (isLoading || !settings) {
    return (
      <div className="flex h-full items-center justify-center">
        <LuLoader className="h-8 w-8 animate-spin text-brand-500" />
      </div>
    );
  }

  function saveSettings(newSettings: typeof settings) {
    saveSettingsMutation.mutate(newSettings!);
  }

  return (
    <div className="h-full overflow-auto">
      {/* Header */}
      <header className="flex h-16 items-center border-b border-surface-600 px-6">
        <h2 className="text-xl font-semibold text-surface-100">Settings</h2>
      </header>

      <div className="settings-container mx-auto max-w-2xl space-y-8 p-6">
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

        <LeaguePathSection settings={settings} onSave={saveSettings} />
        <ModStorageSection settings={settings} onSave={saveSettings} />
        <WorkshopSection settings={settings} onSave={saveSettings} />
        <AppearanceSection settings={settings} onSave={saveSettings} />
        <AboutSection appInfo={appInfo} />
      </div>
    </div>
  );
}
