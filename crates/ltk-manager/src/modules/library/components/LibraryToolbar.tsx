import { LuFolderOpen, LuGrid3X3, LuList, LuPlus, LuSearch } from "react-icons/lu";

import { Button, IconButton } from "@/components";
import type { OverlayProgress, PatcherStatus } from "@/lib/tauri";
import type { useLibraryActions } from "@/modules/library/api";

import { ProfileSelector } from "./ProfileSelector";

interface PatcherProps {
  status: PatcherStatus | undefined;
  overlayProgress: OverlayProgress | null;
  isStarting: boolean;
  isStopping: boolean;
  onStart: () => void;
  onStop: () => void;
}

interface LibraryToolbarProps {
  searchQuery: string;
  onSearchChange: (query: string) => void;
  viewMode: "grid" | "list";
  onViewModeChange: (mode: "grid" | "list") => void;
  actions: ReturnType<typeof useLibraryActions>;
  patcher: PatcherProps;
  hasEnabledMods: boolean;
  isLoading: boolean;
}

export function LibraryToolbar({
  searchQuery,
  onSearchChange,
  viewMode,
  onViewModeChange,
  actions,
  patcher,
  hasEnabledMods,
  isLoading,
}: LibraryToolbarProps) {
  return (
    <div
      className="flex items-center gap-4 border-b border-surface-600 px-4 py-3"
      data-tauri-drag-region
    >
      <ProfileSelector />

      <IconButton
        icon={<LuFolderOpen className="h-4 w-4" />}
        variant="ghost"
        size="sm"
        onClick={actions.handleOpenStorageDirectory}
        title="Open storage directory"
      />

      {/* Search */}
      <div className="relative flex-1">
        <LuSearch className="absolute top-1/2 left-3 h-4 w-4 -translate-y-1/2 text-surface-500" />
        <input
          type="text"
          placeholder="Search mods..."
          value={searchQuery}
          onChange={(e) => onSearchChange(e.target.value)}
          className="w-full rounded-lg border border-surface-600 bg-surface-800 py-2 pr-4 pl-10 text-surface-100 placeholder:text-surface-500 focus:border-transparent focus:ring-2 focus:ring-brand-500 focus:outline-none"
        />
      </div>

      {/* View toggle */}
      <div className="flex items-center gap-1">
        <IconButton
          icon={<LuGrid3X3 className="h-4 w-4" />}
          variant={viewMode === "grid" ? "default" : "ghost"}
          size="sm"
          onClick={() => onViewModeChange("grid")}
        />
        <IconButton
          icon={<LuList className="h-4 w-4" />}
          variant={viewMode === "list" ? "default" : "ghost"}
          size="sm"
          onClick={() => onViewModeChange("list")}
        />
      </div>

      {/* Actions */}
      <Button
        variant="filled"
        size="sm"
        onClick={actions.handleInstallMod}
        loading={actions.installMod.isPending}
        left={<LuPlus className="h-4 w-4" />}
      >
        {actions.installMod.isPending ? "Installing..." : "Add Mod"}
      </Button>

      {patcher.status?.running ? (
        <Button
          variant="outline"
          size="sm"
          onClick={patcher.onStop}
          loading={patcher.isStopping}
          disabled={actions.installMod.isPending || patcher.isStarting}
        >
          {patcher.isStopping ? "Stopping..." : "Stop Patcher"}
        </Button>
      ) : (
        <Button
          variant={hasEnabledMods ? "filled" : "default"}
          size="sm"
          onClick={patcher.onStart}
          loading={patcher.isStarting && !patcher.overlayProgress}
          disabled={
            isLoading ||
            !hasEnabledMods ||
            actions.installMod.isPending ||
            patcher.isStopping ||
            patcher.isStarting
          }
        >
          {patcher.overlayProgress
            ? patcher.overlayProgress.stage === "indexing"
              ? "Indexing..."
              : patcher.overlayProgress.stage === "patching"
                ? `Patching...`
                : "Starting..."
            : patcher.isStarting
              ? "Starting..."
              : "Start Patcher"}
        </Button>
      )}
    </div>
  );
}
