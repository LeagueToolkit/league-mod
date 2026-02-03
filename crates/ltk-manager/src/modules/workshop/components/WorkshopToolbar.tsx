import { LuDownload, LuGrid3X3, LuList, LuPlus, LuSearch } from "react-icons/lu";

import { Button, IconButton } from "@/components/Button";

export type ViewMode = "grid" | "list";

interface WorkshopToolbarProps {
  searchQuery: string;
  onSearchChange: (query: string) => void;
  viewMode: ViewMode;
  onViewModeChange: (mode: ViewMode) => void;
  onImport: () => void;
  onNewProject: () => void;
  isImporting?: boolean;
}

export function WorkshopToolbar({
  searchQuery,
  onSearchChange,
  viewMode,
  onViewModeChange,
  onImport,
  onNewProject,
  isImporting,
}: WorkshopToolbarProps) {
  return (
    <div
      className="flex items-center gap-4 border-b border-surface-600 px-4 py-3"
      data-tauri-drag-region
    >
      {/* Search */}
      <div className="relative flex-1">
        <LuSearch className="absolute top-1/2 left-3 h-4 w-4 -translate-y-1/2 text-surface-500" />
        <input
          type="text"
          placeholder="Search projects..."
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
        variant="outline"
        size="sm"
        onClick={onImport}
        loading={isImporting}
        left={<LuDownload className="h-4 w-4" />}
      >
        Import
      </Button>
      <Button
        variant="filled"
        size="sm"
        onClick={onNewProject}
        left={<LuPlus className="h-4 w-4" />}
      >
        New Project
      </Button>
    </div>
  );
}
