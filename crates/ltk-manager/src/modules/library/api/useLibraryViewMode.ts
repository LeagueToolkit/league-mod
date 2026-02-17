import { useCallback } from "react";

import { useSaveSettings, useSettings } from "@/modules/settings";

export type ViewMode = "grid" | "list";

export function useLibraryViewMode() {
  const { data: settings } = useSettings();
  const saveSettings = useSaveSettings();

  const viewMode: ViewMode = settings?.libraryViewMode === "list" ? "list" : "grid";

  const setViewMode = useCallback(
    (mode: ViewMode) => {
      if (!settings) return;
      saveSettings.mutate({ ...settings, libraryViewMode: mode });
    },
    [settings, saveSettings],
  );

  return { viewMode, setViewMode } as const;
}
