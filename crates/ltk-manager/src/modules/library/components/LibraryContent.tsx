import { LuPlus, LuSearch, LuUpload } from "react-icons/lu";

import { Button } from "@/components";
import type { AppError, InstalledMod } from "@/lib/tauri";
import type { useLibraryActions } from "@/modules/library/api";

import { ModCard } from "./ModCard";
import { SortableModCard } from "./SortableModCard";
import { SortableModList } from "./SortableModList";

interface LibraryContentProps {
  mods: InstalledMod[];
  searchQuery: string;
  viewMode: "grid" | "list";
  actions: ReturnType<typeof useLibraryActions>;
  isLoading: boolean;
  error: AppError | null;
  onInstall: () => void;
}

export function LibraryContent({
  mods,
  searchQuery,
  viewMode,
  actions,
  isLoading,
  error,
  onInstall,
}: LibraryContentProps) {
  const isSearching = searchQuery.length > 0;

  const filteredMods = mods.filter(
    (mod) =>
      mod.displayName.toLowerCase().includes(searchQuery.toLowerCase()) ||
      mod.name.toLowerCase().includes(searchQuery.toLowerCase()),
  );

  const enabledMods = filteredMods.filter((m) => m.enabled);
  const disabledMods = filteredMods.filter((m) => !m.enabled);

  return (
    <div className="flex-1 overflow-auto p-6">
      {isLoading ? (
        <LoadingState />
      ) : error ? (
        <ErrorState error={error} />
      ) : filteredMods.length === 0 ? (
        <EmptyState onInstall={onInstall} hasSearch={!!searchQuery} />
      ) : (
        <div>
          {/* Enabled mods (sortable) */}
          {enabledMods.length > 0 && (
            <SortableModList
              mods={enabledMods}
              viewMode={viewMode}
              onReorder={actions.handleReorder}
              disabled={isSearching}
              onToggle={actions.handleToggleMod}
              onUninstall={actions.handleUninstallMod}
            >
              <div
                className={
                  viewMode === "grid"
                    ? "grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4"
                    : "space-y-2 pl-7"
                }
              >
                {enabledMods.map((mod) =>
                  isSearching ? (
                    <ModCard
                      key={mod.id}
                      mod={mod}
                      viewMode={viewMode}
                      onToggle={actions.handleToggleMod}
                      onUninstall={actions.handleUninstallMod}
                    />
                  ) : (
                    <SortableModCard
                      key={mod.id}
                      mod={mod}
                      viewMode={viewMode}
                      onToggle={actions.handleToggleMod}
                      onUninstall={actions.handleUninstallMod}
                    />
                  ),
                )}
              </div>
            </SortableModList>
          )}

          {/* Separator between enabled and disabled */}
          {enabledMods.length > 0 && disabledMods.length > 0 && (
            <div className="my-6 flex items-center gap-3">
              <div className="h-px flex-1 bg-surface-700" />
              <span className="text-xs text-surface-500">Disabled ({disabledMods.length})</span>
              <div className="h-px flex-1 bg-surface-700" />
            </div>
          )}

          {/* Disabled mods (not sortable) */}
          {disabledMods.length > 0 && (
            <div
              className={
                viewMode === "grid"
                  ? "grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4"
                  : "space-y-2"
              }
            >
              {disabledMods.map((mod) => (
                <ModCard
                  key={mod.id}
                  mod={mod}
                  viewMode={viewMode}
                  onToggle={actions.handleToggleMod}
                  onUninstall={actions.handleUninstallMod}
                />
              ))}
            </div>
          )}
        </div>
      )}
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
