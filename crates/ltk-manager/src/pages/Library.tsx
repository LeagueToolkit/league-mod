import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "react";
import { LuGrid3X3, LuList, LuPlus, LuSearch, LuUpload } from "react-icons/lu";

import { Button, IconButton } from "@/components/Button";
import { ModCard } from "@/components/ModCard";
import type { AppError } from "@/lib/tauri";
import {
  useInstalledMods,
  useInstallMod,
  useToggleMod,
  useUninstallMod,
} from "@/modules/library/api";
import {
  useOverlayProgress,
  usePatcherStatus,
  useStartPatcher,
  useStopPatcher,
} from "@/modules/patcher";

const VALID_EXTENSIONS = [".modpkg", ".fantome"];

function isValidModFile(path: string): boolean {
  const lowerPath = path.toLowerCase();
  return VALID_EXTENSIONS.some((ext) => lowerPath.endsWith(ext));
}

export function Library() {
  const [searchQuery, setSearchQuery] = useState("");
  const [viewMode, setViewMode] = useState<"grid" | "list">("grid");
  const [isDragOver, setIsDragOver] = useState(false);

  const { data: mods = [], isLoading, error } = useInstalledMods();
  const installMod = useInstallMod();
  const toggleMod = useToggleMod();
  const uninstallMod = useUninstallMod();
  const { data: patcherStatus } = usePatcherStatus();
  const startPatcher = useStartPatcher();
  const stopPatcher = useStopPatcher();
  const overlayProgress = useOverlayProgress();

  const enabledModsCount = mods.filter((m) => m.enabled).length;
  const hasEnabledMods = enabledModsCount > 0;

  // Handle drag and drop
  useEffect(() => {
    const currentWindow = getCurrentWindow();
    const unlisten = currentWindow.onDragDropEvent((event) => {
      const eventType = event.payload.type;
      if (eventType === "enter" || eventType === "over") {
        setIsDragOver(true);
      } else if (eventType === "drop") {
        setIsDragOver(false);
        const paths = event.payload.paths as string[];
        const validPaths = paths.filter(isValidModFile);

        // Install each valid mod file
        for (const path of validPaths) {
          installMod.mutate(path, {
            onError: (error) => {
              console.error("Failed to install mod:", error.message);
            },
          });
        }
      } else if (eventType === "leave" || eventType === "cancel") {
        setIsDragOver(false);
      }
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [installMod]);

  async function handleInstallMod() {
    const file = await open({
      multiple: false,
      filters: [{ name: "Mod Package", extensions: ["modpkg", "fantome"] }],
    });

    if (file) {
      installMod.mutate(file, {
        onError: (error) => {
          console.error("Failed to install mod:", error.message);
        },
      });
    }
  }

  function handleToggleMod(modId: string, enabled: boolean) {
    toggleMod.mutate(
      { modId, enabled },
      {
        onError: (error) => {
          console.error("Failed to toggle mod:", error.message);
        },
      },
    );
  }

  function handleUninstallMod(modId: string) {
    uninstallMod.mutate(modId, {
      onError: (error) => {
        console.error("Failed to uninstall mod:", error.message);
      },
    });
  }

  function handleStartPatcher() {
    startPatcher.mutate(
      {
        timeoutMs: null,
        logFile: null,
      },
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

  const filteredMods = mods.filter(
    (mod) =>
      mod.displayName.toLowerCase().includes(searchQuery.toLowerCase()) ||
      mod.name.toLowerCase().includes(searchQuery.toLowerCase()),
  );

  return (
    <div className="relative flex h-full flex-col">
      {/* Drag overlay */}
      {isDragOver && (
        <div className="absolute inset-0 z-50 flex items-center justify-center bg-night-500/90 backdrop-blur-sm">
          <div className="flex flex-col items-center gap-4 rounded-2xl border-2 border-dashed border-brand-500 bg-night-400/50 p-12">
            <LuUpload className="h-16 w-16 text-brand-500" />
            <div className="text-center">
              <p className="text-lg font-medium text-surface-100">Drop to install</p>
              <p className="text-sm text-surface-400">.modpkg or .fantome files</p>
            </div>
          </div>
        </div>
      )}

      {/* Toolbar with search and actions */}
      <div
        className="flex items-center gap-4 border-b border-surface-600 px-4 py-3"
        data-tauri-drag-region
      >
        {/* Search */}
        <div className="relative flex-1">
          <LuSearch className="absolute top-1/2 left-3 h-4 w-4 -translate-y-1/2 text-surface-500" />
          <input
            type="text"
            placeholder="Search mods..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="w-full rounded-lg border border-surface-600 bg-surface-800 py-2 pr-4 pl-10 text-surface-100 placeholder:text-surface-500 focus:border-transparent focus:ring-2 focus:ring-brand-500 focus:outline-none"
          />
        </div>

        {/* View toggle */}
        <div className="flex items-center gap-1">
          <IconButton
            icon={<LuGrid3X3 className="h-4 w-4" />}
            variant={viewMode === "grid" ? "default" : "ghost"}
            size="sm"
            onClick={() => setViewMode("grid")}
          />
          <IconButton
            icon={<LuList className="h-4 w-4" />}
            variant={viewMode === "list" ? "default" : "ghost"}
            size="sm"
            onClick={() => setViewMode("list")}
          />
        </div>

        {/* Actions */}
        <Button
          variant="filled"
          size="sm"
          onClick={handleInstallMod}
          loading={installMod.isPending}
          left={<LuPlus className="h-4 w-4" />}
        >
          {installMod.isPending ? "Installing..." : "Add Mod"}
        </Button>

        {patcherStatus?.running ? (
          <Button
            variant="outline"
            size="sm"
            onClick={handleStopPatcher}
            loading={stopPatcher.isPending}
            disabled={installMod.isPending || startPatcher.isPending}
          >
            {stopPatcher.isPending ? "Stopping..." : "Stop Patcher"}
          </Button>
        ) : (
          <Button
            variant={hasEnabledMods ? "filled" : "default"}
            size="sm"
            onClick={handleStartPatcher}
            loading={startPatcher.isPending && !overlayProgress}
            disabled={
              isLoading ||
              !hasEnabledMods ||
              installMod.isPending ||
              stopPatcher.isPending ||
              startPatcher.isPending
            }
          >
            {overlayProgress
              ? overlayProgress.stage === "indexing"
                ? "Indexing..."
                : overlayProgress.stage === "patching"
                  ? `Patching...`
                  : "Starting..."
              : startPatcher.isPending
                ? "Starting..."
                : "Start Patcher"}
          </Button>
        )}
      </div>

      {/* Content */}
      <div className="flex-1 overflow-auto p-6">
        {isLoading ? (
          <LoadingState />
        ) : error ? (
          <ErrorState error={error} />
        ) : filteredMods.length === 0 ? (
          <EmptyState onInstall={handleInstallMod} hasSearch={!!searchQuery} />
        ) : (
          <div
            className={
              viewMode === "grid"
                ? "grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4"
                : "space-y-2"
            }
          >
            {filteredMods.map((mod) => (
              <ModCard
                key={mod.id}
                mod={mod}
                viewMode={viewMode}
                onToggle={handleToggleMod}
                onUninstall={handleUninstallMod}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function LoadingState() {
  return (
    <div className="flex h-64 items-center justify-center">
      <div className="h-8 w-8 animate-spin rounded-full border-2 border-brand-500 border-t-transparent" />
    </div>
  );
}

function ErrorState({ error }: { error: AppError }) {
  return (
    <div className="flex h-64 flex-col items-center justify-center text-center">
      <div className="mb-4 rounded-full bg-red-500/10 p-4">
        <span className="text-2xl">⚠️</span>
      </div>
      <h3 className="mb-1 text-lg font-medium text-surface-300">Failed to load mods</h3>
      <p className="mb-2 text-surface-500">{error.message}</p>
      <p className="text-sm text-surface-600">Error code: {error.code}</p>
    </div>
  );
}

function EmptyState({ onInstall, hasSearch }: { onInstall: () => void; hasSearch: boolean }) {
  if (hasSearch) {
    return (
      <div className="flex h-64 flex-col items-center justify-center text-center">
        <LuSearch className="mb-4 h-12 w-12 text-surface-600" />
        <h3 className="mb-1 text-lg font-medium text-surface-300">No mods found</h3>
        <p className="text-surface-500">Try adjusting your search query</p>
      </div>
    );
  }

  return (
    <div className="flex h-64 flex-col items-center justify-center text-center">
      <div className="mb-4 flex h-20 w-20 items-center justify-center rounded-2xl">
        <LuUpload className="h-10 w-10 text-surface-600" />
      </div>
      <h3 className="mb-1 text-lg font-medium text-surface-300">No mods installed</h3>
      <p className="mb-4 text-surface-500">Get started by adding your first mod</p>
      <Button variant="filled" onClick={onInstall} left={<LuPlus className="h-4 w-4" />}>
        Add Mod
      </Button>
    </div>
  );
}
