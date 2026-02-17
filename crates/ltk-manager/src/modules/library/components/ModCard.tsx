import { invoke } from "@tauri-apps/api/core";
import { LuEllipsisVertical, LuFolderOpen, LuInfo, LuTrash2 } from "react-icons/lu";

import { IconButton, Menu, Switch } from "@/components";
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
  layers: { name: string; priority: number; enabled: boolean }[];
  modDir: string;
}

interface ModCardProps {
  mod: InstalledMod;
  viewMode: "grid" | "list";
  onToggle: (modId: string, enabled: boolean) => void;
  onUninstall: (modId: string) => void;
  onViewDetails?: (mod: InstalledMod) => void;
  disabled?: boolean;
}

export function ModCard({
  mod,
  viewMode,
  onToggle,
  onUninstall,
  onViewDetails,
  disabled,
}: ModCardProps) {
  const { data: thumbnailUrl } = useModThumbnail(mod.id);

  async function handleOpenLocation() {
    try {
      await invoke("reveal_in_explorer", { path: mod.modDir });
    } catch (error) {
      console.error("Failed to open location:", error);
    }
  }

  function handleCardClick(e: React.MouseEvent) {
    if (disabled) return;
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
        className={`flex items-center gap-4 rounded-lg border p-4 transition-all ${
          disabled ? "cursor-default opacity-60" : "cursor-pointer"
        } ${
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
          <Switch
            disabled={disabled}
            checked={mod.enabled}
            onCheckedChange={(checked) => onToggle(mod.id, checked)}
          />
        </div>

        {/* Menu */}
        <div data-no-toggle>
          <Menu.Root>
            <Menu.Trigger
              disabled={disabled}
              render={
                <IconButton
                  icon={<LuEllipsisVertical className="h-4 w-4" />}
                  variant="ghost"
                  size="sm"
                  disabled={disabled}
                />
              }
            />
            <Menu.Portal>
              <Menu.Positioner>
                <Menu.Popup>
                  <Menu.Item
                    icon={<LuInfo className="h-4 w-4" />}
                    onClick={() => onViewDetails?.(mod)}
                  >
                    View Details
                  </Menu.Item>
                  <Menu.Item
                    icon={<LuFolderOpen className="h-4 w-4" />}
                    onClick={handleOpenLocation}
                  >
                    Open Location
                  </Menu.Item>
                  <Menu.Separator />
                  <Menu.Item
                    icon={<LuTrash2 className="h-4 w-4" />}
                    variant="danger"
                    disabled={disabled}
                    onClick={() => onUninstall(mod.id)}
                  >
                    Uninstall
                  </Menu.Item>
                </Menu.Popup>
              </Menu.Positioner>
            </Menu.Portal>
          </Menu.Root>
        </div>
      </div>
    );
  }

  return (
    <div
      onClick={handleCardClick}
      className={`group relative rounded-xl border transition-all ${
        disabled ? "cursor-default opacity-60" : "cursor-pointer"
      } ${
        mod.enabled
          ? "border-brand-500/40 bg-surface-800 shadow-[0_0_20px_-5px] shadow-brand-500/40"
          : "border-surface-600 bg-surface-800 hover:border-surface-400"
      }`}
    >
      {/* Toggle in top-right corner */}
      <div className="absolute top-2 right-2 z-10" data-no-toggle>
        <Switch
          size="sm"
          disabled={disabled}
          checked={mod.enabled}
          onCheckedChange={(checked) => onToggle(mod.id, checked)}
          className="shadow-lg data-[unchecked]:bg-surface-600/80 data-[unchecked]:backdrop-blur-sm"
        />
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
        <h3 className="mb-1 line-clamp-1 text-sm font-medium text-surface-100">
          {mod.displayName}
        </h3>

        {/* Version, author, and menu on same row */}
        <div className="flex items-center text-xs text-surface-500">
          <span>v{mod.version}</span>
          <span className="mx-1">•</span>
          <span className="flex-1 truncate">
            {mod.authors.length > 0 ? mod.authors[0] : "Unknown"}
          </span>
          <div className="ml-1 shrink-0" data-no-toggle>
            <Menu.Root>
              <Menu.Trigger
                disabled={disabled}
                render={
                  <IconButton
                    icon={<LuEllipsisVertical className="h-3.5 w-3.5" />}
                    variant="ghost"
                    size="xs"
                    className="h-5 w-5"
                    disabled={disabled}
                  />
                }
              />
              <Menu.Portal>
                <Menu.Positioner>
                  <Menu.Popup>
                    <Menu.Item
                      icon={<LuInfo className="h-4 w-4" />}
                      onClick={() => onViewDetails?.(mod)}
                    >
                      View Details
                    </Menu.Item>
                    <Menu.Item
                      icon={<LuFolderOpen className="h-4 w-4" />}
                      onClick={handleOpenLocation}
                    >
                      Open Location
                    </Menu.Item>
                    <Menu.Separator />
                    <Menu.Item
                      icon={<LuTrash2 className="h-4 w-4" />}
                      variant="danger"
                      disabled={disabled}
                      onClick={() => onUninstall(mod.id)}
                    >
                      Uninstall
                    </Menu.Item>
                  </Menu.Popup>
                </Menu.Positioner>
              </Menu.Portal>
            </Menu.Root>
          </div>
        </div>
      </div>
    </div>
  );
}
