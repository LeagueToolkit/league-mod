import { useState } from "react";

import {
  DragDropOverlay,
  ImportProgressDialog,
  LibraryContent,
  LibraryToolbar,
  useInstalledMods,
  useLibraryActions,
  useModFileDrop,
} from "@/modules/library";
import { usePatcherStatus, useStartPatcher, useStopPatcher } from "@/modules/patcher";

export function Library() {
  const [searchQuery, setSearchQuery] = useState("");

  const { data: mods = [], isLoading, error } = useInstalledMods();
  const actions = useLibraryActions();
  const isDragOver = useModFileDrop(actions.handleBulkInstallFiles);

  const { data: patcherStatus } = usePatcherStatus();
  const startPatcher = useStartPatcher();
  const stopPatcher = useStopPatcher();

  const isStarting = patcherStatus?.phase === "building";
  const isPatcherActive = patcherStatus?.running ?? false;

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
        actions={actions}
        patcher={{
          status: patcherStatus,
          isStarting: isStarting,
          isStopping: stopPatcher.isPending,
          onStart: handleStartPatcher,
          onStop: handleStopPatcher,
        }}
        hasEnabledMods={hasEnabledMods}
        isLoading={isLoading}
        isPatcherActive={isPatcherActive}
      />
      <LibraryContent
        mods={mods}
        searchQuery={searchQuery}
        actions={actions}
        isLoading={isLoading}
        error={error}
        onInstall={actions.handleInstallMod}
        isPatcherActive={isPatcherActive}
      />
      <ImportProgressDialog
        open={actions.importDialogOpen}
        onClose={actions.handleCloseImportDialog}
        progress={actions.installProgress}
        result={actions.importResult}
      />
    </div>
  );
}
