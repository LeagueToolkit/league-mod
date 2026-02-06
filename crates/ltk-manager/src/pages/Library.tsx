import { useState } from "react";

import {
  DragDropOverlay,
  LibraryContent,
  LibraryToolbar,
  useInstalledMods,
  useLibraryActions,
  useModFileDrop,
} from "@/modules/library";
import {
  useOverlayProgress,
  usePatcherStatus,
  useStartPatcher,
  useStopPatcher,
} from "@/modules/patcher";

export function Library() {
  const [searchQuery, setSearchQuery] = useState("");
  const [viewMode, setViewMode] = useState<"grid" | "list">("grid");

  const { data: mods = [], isLoading, error } = useInstalledMods();
  const actions = useLibraryActions();
  const isDragOver = useModFileDrop(actions.installMod);

  const { data: patcherStatus } = usePatcherStatus();
  const startPatcher = useStartPatcher();
  const stopPatcher = useStopPatcher();
  const overlayProgress = useOverlayProgress();

  const hasEnabledMods = mods.some((m) => m.enabled);

  function handleStartPatcher() {
    startPatcher.mutate(
      { timeoutMs: null, logFile: null },
      {
        onError: (error) => {
          console.error("Failed to start patcher:", error.message);
        },
      },
    );
  }

  function handleStopPatcher() {
    stopPatcher.mutate(undefined, {
      onError: (error) => {
        console.error("Failed to stop patcher:", error.message);
      },
    });
  }

  return (
    <div className="relative flex h-full flex-col">
      <DragDropOverlay visible={isDragOver} />
      <LibraryToolbar
        searchQuery={searchQuery}
        onSearchChange={setSearchQuery}
        viewMode={viewMode}
        onViewModeChange={setViewMode}
        actions={actions}
        patcher={{
          status: patcherStatus,
          overlayProgress,
          isStarting: startPatcher.isPending,
          isStopping: stopPatcher.isPending,
          onStart: handleStartPatcher,
          onStop: handleStopPatcher,
        }}
        hasEnabledMods={hasEnabledMods}
        isLoading={isLoading}
      />
      <LibraryContent
        mods={mods}
        searchQuery={searchQuery}
        viewMode={viewMode}
        actions={actions}
        isLoading={isLoading}
        error={error}
        onInstall={actions.handleInstallMod}
      />
    </div>
  );
}
