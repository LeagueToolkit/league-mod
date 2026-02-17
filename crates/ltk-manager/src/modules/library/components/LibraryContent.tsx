import { LuPlus, LuSearch, LuUpload } from "react-icons/lu";

import { Button } from "@/components";
import type { AppError, InstalledMod } from "@/lib/tauri";
import type { useLibraryActions } from "@/modules/library/api";

import { ModCard } from "./ModCard";
import { SortableModCard } from "./SortableModCard";
import { SortableModList } from "./SortableModList";

function gridClass(viewMode: "grid" | "list", indent = false) {
  if (viewMode === "list") return indent ? "space-y-2 pl-7" : "space-y-2";
  return "grid grid-cols-[repeat(auto-fill,minmax(300px,1fr))] gap-4";
}

interface LibraryContentProps {
  mods: InstalledMod[];
  searchQuery: string;
  viewMode: "grid" | "list";
  actions: ReturnType<typeof useLibraryActions>;
  isLoading: boolean;
  error: AppError | null;
  onInstall: () => void;
  isPatcherActive?: boolean;
}

export function LibraryContent({
  mods,
  searchQuery,
  viewMode,
  actions,
  isLoading,
  error,
  onInstall,
  isPatcherActive,
}: LibraryContentProps) {
  const isSearching = searchQuery.length > 0;

  const filteredMods = mods.filter(
    (mod) =>
      mod.displayName.toLowerCase().includes(searchQuery.toLowerCase()) ||
      mod.name.toLowerCase().includes(searchQuery.toLowerCase()),
  );

  if (isLoading) {
    return (
      <div className="flex-1 overflow-auto p-6">
        <LoadingState />
      </div>
    );
  }

  if (error) {
    return (
      <div className="flex-1 overflow-auto p-6">
        <ErrorState error={error} />
      </div>
    );
  }

  if (filteredMods.length === 0) {
    return (
      <div className="flex-1 overflow-auto p-6">
        <EmptyState onInstall={onInstall} hasSearch={isSearching} />
      </div>
    );
  }

  const Card = isSearching ? ModCard : SortableModCard;

  return (
    <div className="flex-1 overflow-auto p-6">
      <SortableModList
        mods={filteredMods}
        viewMode={viewMode}
        onReorder={actions.handleReorder}
        disabled={isSearching || isPatcherActive}
        onToggle={actions.handleToggleMod}
        onUninstall={actions.handleUninstallMod}
      >
        <div className={gridClass(viewMode, !isSearching)}>
          {filteredMods.map((mod) => (
            <Card
              key={mod.id}
              mod={mod}
              viewMode={viewMode}
              onToggle={actions.handleToggleMod}
              onUninstall={actions.handleUninstallMod}
              disabled={isPatcherActive}
            />
          ))}
        </div>
      </SortableModList>
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
