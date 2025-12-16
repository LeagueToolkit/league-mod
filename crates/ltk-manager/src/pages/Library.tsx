import { open } from "@tauri-apps/plugin-dialog";
import { useState } from "react";
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
import { usePatcherStatus, useStartPatcher, useStopPatcher } from "@/modules/patcher";

export function Library() {
  const [searchQuery, setSearchQuery] = useState("");
  const [viewMode, setViewMode] = useState<"grid" | "list">("grid");

  const { data: mods = [], isLoading, error } = useInstalledMods();
  const installMod = useInstallMod();
  const toggleMod = useToggleMod();
  const uninstallMod = useUninstallMod();
  const { data: patcherStatus } = usePatcherStatus();
  const startPatcher = useStartPatcher();
  const stopPatcher = useStopPatcher();

  const enabledModsCount = mods.filter((m) => m.enabled).length;
  const hasEnabledMods = enabledModsCount > 0;

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
    <div className="flex h-full flex-col">
      {/* Header */}
      <header className="flex h-16 items-center justify-between border-b border-surface-600 px-6">
        <h2 className="text-xl font-semibold text-surface-100">Mod Library</h2>
        <div className="flex items-center gap-2">
          <Button
            variant="filled"
            onClick={handleInstallMod}
            loading={installMod.isPending}
            left={<LuPlus className="h-4 w-4" />}
          >
            {installMod.isPending ? "Installing..." : "Add Mod"}
          </Button>

          {patcherStatus?.running ? (
            <Button
              variant="outline"
              onClick={handleStopPatcher}
              loading={stopPatcher.isPending}
              disabled={installMod.isPending || startPatcher.isPending}
            >
              {stopPatcher.isPending ? "Stopping..." : "Stop Patcher"}
            </Button>
          ) : (
            <Button
              variant={hasEnabledMods ? "filled" : "default"}
              onClick={handleStartPatcher}
              loading={startPatcher.isPending}
              disabled={
                isLoading ||
                !hasEnabledMods ||
                installMod.isPending ||
                stopPatcher.isPending ||
                startPatcher.isPending
              }
            >
              {startPatcher.isPending
                ? "Starting..."
                : hasEnabledMods
                  ? "Start Patcher"
                  : "Start Patcher (enable a mod)"}
            </Button>
          )}
        </div>
      </header>

      {/* Toolbar */}
      <div className="flex items-center gap-4 border-b border-surface-600/50 px-6 py-4">
        {/* Search */}
        <div className="relative max-w-md flex-1">
          <LuSearch className="absolute top-1/2 left-3 h-4 w-4 -translate-y-1/2 text-surface-500" />
          <input
            type="text"
            placeholder="Search mods..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="w-full rounded-lg border border-surface-600 bg-night-500 py-2 pr-4 pl-10 text-surface-100 placeholder:text-surface-500 focus:border-transparent focus:ring-2 focus:ring-brand-500 focus:outline-none"
          />
        </div>

        {/* View toggle */}
        <div className="flex items-center gap-1 rounded-lg p-1">
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
