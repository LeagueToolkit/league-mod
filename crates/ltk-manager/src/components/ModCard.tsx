import { invoke } from "@tauri-apps/api/core";
import { useState } from "react";
import { LuEllipsisVertical, LuFolderOpen, LuInfo, LuTrash2 } from "react-icons/lu";

import { Button, IconButton } from "@/components/Button";
import { useModThumbnail } from "@/modules/library/api/useModThumbnail";

interface InstalledMod {
  id: string;
  name: string;
  displayName: string;
  version: string;
  description?: string;
  authors: string[];
  enabled: boolean;
  installedAt: string;
  filePath: string;
  layers: { name: string; priority: number; enabled: boolean }[];
  thumbnailPath?: string;
  modDir: string;
}

interface ModCardProps {
  mod: InstalledMod;
  viewMode: "grid" | "list";
  onToggle: (modId: string, enabled: boolean) => void;
  onUninstall: (modId: string) => void;
  onViewDetails?: (mod: InstalledMod) => void;
}

export function ModCard({ mod, viewMode, onToggle, onUninstall, onViewDetails }: ModCardProps) {
  const [showMenu, setShowMenu] = useState(false);
  const { data: thumbnailUrl } = useModThumbnail(mod.id, mod.thumbnailPath);

  async function handleOpenLocation() {
    try {
      // Use modDir (installed location) instead of filePath (source file)
      await invoke("reveal_in_explorer", { path: mod.modDir });
    } catch (error) {
      console.error("Failed to open location:", error);
    }
    setShowMenu(false);
  }

  function handleViewDetails() {
    onViewDetails?.(mod);
    setShowMenu(false);
  }

  function handleCardClick(e: React.MouseEvent) {
    // Don't toggle if clicking on menu button or toggle
    if ((e.target as HTMLElement).closest("[data-no-toggle]")) {
      return;
    }
    onToggle(mod.id, !mod.enabled);
  }

  if (viewMode === "list") {
    return (
      <div
        onClick={handleCardClick}
        className={`flex cursor-pointer items-center gap-4 rounded-lg border p-4 transition-all ${
          mod.enabled
            ? "border-brand-500/40 bg-surface-800 shadow-[0_0_15px_-3px] shadow-brand-500/30"
            : "border-surface-700 bg-surface-900 hover:border-surface-600"
        }`}
      >
        {/* Thumbnail */}
        <div className="flex h-12 w-12 shrink-0 items-center justify-center overflow-hidden rounded-lg bg-linear-to-br from-surface-700 to-surface-800">
          {thumbnailUrl ? (
            <img src={thumbnailUrl} alt="" className="h-full w-full object-cover" />
          ) : (
            <span className="text-lg font-bold text-surface-500">
              {mod.displayName.charAt(0).toUpperCase()}
            </span>
          )}
        </div>

        {/* Info */}
        <div className="min-w-0 flex-1">
          <h3 className="truncate font-medium text-surface-100">{mod.displayName}</h3>
          <p className="truncate text-sm text-surface-500">
            v{mod.version} • {mod.authors.join(", ") || "Unknown author"}
          </p>
        </div>

        {/* Toggle */}
        <div data-no-toggle>
          <Toggle enabled={mod.enabled} onChange={(enabled) => onToggle(mod.id, enabled)} />
        </div>

        {/* Menu */}
        <div className="relative" data-no-toggle>
          <IconButton
            icon={<LuEllipsisVertical className="h-4 w-4" />}
            variant="ghost"
            size="sm"
            onClick={() => setShowMenu(!showMenu)}
          />
          {showMenu && (
            <ContextMenu
              onClose={() => setShowMenu(false)}
              onUninstall={() => onUninstall(mod.id)}
              onOpenLocation={handleOpenLocation}
              onViewDetails={handleViewDetails}
            />
          )}
        </div>
      </div>
    );
  }

  return (
    <div
      onClick={handleCardClick}
      className={`group relative cursor-pointer rounded-xl border transition-all ${
        mod.enabled
          ? "border-brand-500/40 bg-surface-800 shadow-[0_0_20px_-5px] shadow-brand-500/40"
          : "border-surface-600 bg-surface-800 hover:border-surface-400"
      }`}
    >
      {/* Toggle in top-right corner */}
      <div className="absolute top-2 right-2 z-10" data-no-toggle>
        <ToggleSmall enabled={mod.enabled} onChange={(enabled) => onToggle(mod.id, enabled)} />
      </div>

      {/* Thumbnail */}
      <div className="flex aspect-video items-center justify-center overflow-hidden rounded-t-xl bg-linear-to-br from-surface-700 to-surface-800">
        {thumbnailUrl ? (
          <img src={thumbnailUrl} alt="" className="h-full w-full object-cover" />
        ) : (
          <span className="text-4xl font-bold text-surface-400">
            {mod.displayName.charAt(0).toUpperCase()}
          </span>
        )}
      </div>

      {/* Content */}
      <div className="p-3">
        {/* Title */}
        <h3 className="line-clamp-1 text-sm font-medium text-surface-100 mb-1">{mod.displayName}</h3>

        {/* Version, author, and menu on same row */}
        <div className="flex items-center text-xs text-surface-500">
          <span>v{mod.version}</span>
          <span className="mx-1">•</span>
          <span className="truncate flex-1">
            {mod.authors.length > 0 ? mod.authors[0] : "Unknown"}
          </span>
          <div className="relative shrink-0 ml-1" data-no-toggle>
            <IconButton
              icon={<LuEllipsisVertical className="h-3.5 w-3.5" />}
              variant="ghost"
              size="xs"
              onClick={() => setShowMenu(!showMenu)}
              className="h-5 w-5"
            />
            {showMenu && (
              <ContextMenu
                onClose={() => setShowMenu(false)}
                onUninstall={() => onUninstall(mod.id)}
                onOpenLocation={handleOpenLocation}
                onViewDetails={handleViewDetails}
              />
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function Toggle({ enabled, onChange }: { enabled: boolean; onChange: (enabled: boolean) => void }) {
  return (
    <button
      type="button"
      onClick={() => onChange(!enabled)}
      className={`relative h-6 w-11 rounded-full transition-colors ${
        enabled ? "bg-brand-500" : "bg-surface-700"
      }`}
    >
      <span
        className={`absolute top-1 left-1 h-4 w-4 rounded-full bg-white transition-transform ${
          enabled ? "translate-x-5" : "translate-x-0"
        }`}
      />
    </button>
  );
}

// Smaller toggle for overlay on thumbnail
function ToggleSmall({ enabled, onChange }: { enabled: boolean; onChange: (enabled: boolean) => void }) {
  return (
    <button
      type="button"
      onClick={(e) => {
        e.stopPropagation();
        onChange(!enabled);
      }}
      className={`relative h-5 w-9 rounded-full transition-colors shadow-lg ${
        enabled ? "bg-brand-500" : "bg-surface-600/80 backdrop-blur-sm"
      }`}
    >
      <span
        className={`absolute top-0.5 left-0.5 h-4 w-4 rounded-full bg-white shadow transition-transform ${
          enabled ? "translate-x-4" : "translate-x-0"
        }`}
      />
    </button>
  );
}

interface ContextMenuProps {
  onClose: () => void;
  onUninstall: () => void;
  onOpenLocation: () => void;
  onViewDetails: () => void;
}

function ContextMenu({ onClose, onUninstall, onOpenLocation, onViewDetails }: ContextMenuProps) {
  return (
    <>
      <div
        className="fixed inset-0 z-10"
        onClick={onClose}
        onKeyDown={(e) => e.key === "Escape" && onClose()}
        role="button"
        tabIndex={0}
        aria-label="Close menu"
      />
      <div className="absolute top-full right-0 z-20 mt-1 w-44 animate-fade-in rounded-lg border border-surface-600 bg-surface-700 py-1 shadow-xl">
        <Button
          variant="ghost"
          size="sm"
          left={<LuInfo className="h-4 w-4" />}
          onClick={onViewDetails}
          className="w-full justify-start rounded-none px-3"
        >
          View Details
        </Button>
        <Button
          variant="ghost"
          size="sm"
          left={<LuFolderOpen className="h-4 w-4" />}
          onClick={onOpenLocation}
          className="w-full justify-start rounded-none px-3"
        >
          Open Location
        </Button>
        <hr className="my-1 border-surface-600" />
        <Button
          variant="ghost"
          size="sm"
          left={<LuTrash2 className="h-4 w-4" />}
          onClick={() => {
            onUninstall();
            onClose();
          }}
          className="w-full justify-start rounded-none px-3 text-red-400 hover:text-red-300"
        >
          Uninstall
        </Button>
      </div>
    </>
  );
}
