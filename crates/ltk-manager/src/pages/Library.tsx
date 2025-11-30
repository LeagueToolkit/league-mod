import { useState } from "react";

import { open } from "@tauri-apps/plugin-dialog";
import { Grid3X3, List, Plus, Search, Upload } from "lucide-react";

import { ModCard } from "@/components/ModCard";
import type { AppError } from "@/lib/tauri";
import {
  useInstalledMods,
  useInstallMod,
  useToggleMod,
  useUninstallMod,
} from "@/modules/library/api";

export function Library() {
  const [searchQuery, setSearchQuery] = useState("");
  const [viewMode, setViewMode] = useState<"grid" | "list">("grid");

  const { data: mods = [], isLoading, error } = useInstalledMods();
  const installMod = useInstallMod();
  const toggleMod = useToggleMod();
  const uninstallMod = useUninstallMod();

  async function handleInstallMod() {
    const file = await open({
      multiple: false,
      filters: [{ name: "Mod Package", extensions: ["modpkg"] }],
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

  const filteredMods = mods.filter(
    (mod) =>
      mod.displayName.toLowerCase().includes(searchQuery.toLowerCase()) ||
      mod.name.toLowerCase().includes(searchQuery.toLowerCase()),
  );

  return (
    <div className="flex h-full flex-col">
      {/* Header */}
      <header className="border-surface-600 flex h-16 items-center justify-between border-b px-6">
        <h2 className="text-surface-100 text-xl font-semibold">Mod Library</h2>
        <button
          type="button"
          onClick={handleInstallMod}
          disabled={installMod.isPending}
          className="bg-league-500 hover:bg-league-600 disabled:bg-league-500/50 flex items-center gap-2 rounded-lg px-4 py-2 font-medium text-white transition-colors disabled:cursor-not-allowed"
        >
          <Plus className="h-4 w-4" />
          {installMod.isPending ? "Installing..." : "Add Mod"}
        </button>
      </header>

      {/* Toolbar */}
      <div className="border-surface-600/50 flex items-center gap-4 border-b px-6 py-4">
        {/* Search */}
        <div className="relative max-w-md flex-1">
          <Search className="text-surface-500 absolute top-1/2 left-3 h-4 w-4 -translate-y-1/2" />
          <input
            type="text"
            placeholder="Search mods..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="bg-night-500 border-surface-600 text-surface-100 placeholder:text-surface-500 focus:ring-league-500 w-full rounded-lg border py-2 pr-4 pl-10 focus:border-transparent focus:ring-2 focus:outline-none"
          />
        </div>

        {/* View toggle */}
        <div className="flex items-center gap-1 rounded-lg p-1">
          <button
            type="button"
            onClick={() => setViewMode("grid")}
            className={`rounded-md p-2 transition-colors ${
              viewMode === "grid"
                ? "bg-surface-700 text-surface-100"
                : "text-surface-500 hover:text-surface-300"
            }`}
          >
            <Grid3X3 className="h-4 w-4" />
          </button>
          <button
            type="button"
            onClick={() => setViewMode("list")}
            className={`rounded-md p-2 transition-colors ${
              viewMode === "list"
                ? "bg-surface-700 text-surface-100"
                : "text-surface-500 hover:text-surface-300"
            }`}
          >
            <List className="h-4 w-4" />
          </button>
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
      <div className="border-league-500 h-8 w-8 animate-spin rounded-full border-2 border-t-transparent" />
    </div>
  );
}

function ErrorState({ error }: { error: AppError }) {
  return (
    <div className="flex h-64 flex-col items-center justify-center text-center">
      <div className="mb-4 rounded-full bg-red-500/10 p-4">
        <span className="text-2xl">⚠️</span>
      </div>
      <h3 className="text-surface-300 mb-1 text-lg font-medium">Failed to load mods</h3>
      <p className="text-surface-500 mb-2">{error.message}</p>
      <p className="text-surface-600 text-sm">Error code: {error.code}</p>
    </div>
  );
}

function EmptyState({ onInstall, hasSearch }: { onInstall: () => void; hasSearch: boolean }) {
  if (hasSearch) {
    return (
      <div className="flex h-64 flex-col items-center justify-center text-center">
        <Search className="text-surface-600 mb-4 h-12 w-12" />
        <h3 className="text-surface-300 mb-1 text-lg font-medium">No mods found</h3>
        <p className="text-surface-500">Try adjusting your search query</p>
      </div>
    );
  }

  return (
    <div className="flex h-64 flex-col items-center justify-center text-center">
      <div className="mb-4 flex h-20 w-20 items-center justify-center rounded-2xl">
        <Upload className="text-surface-600 h-10 w-10" />
      </div>
      <h3 className="text-surface-300 mb-1 text-lg font-medium">No mods installed</h3>
      <p className="text-surface-500 mb-4">Get started by adding your first mod</p>
      <button
        type="button"
        onClick={onInstall}
        className="bg-league-500 hover:bg-league-600 flex items-center gap-2 rounded-lg px-4 py-2 font-medium text-white transition-colors"
      >
        <Plus className="h-4 w-4" />
        Add Mod
      </button>
    </div>
  );
}
