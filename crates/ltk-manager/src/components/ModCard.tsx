import { FolderOpen, Info, MoreVertical, Trash2 } from "lucide-react";
import { useState } from "react";

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
}

interface ModCardProps {
  mod: InstalledMod;
  viewMode: "grid" | "list";
  onToggle: (modId: string, enabled: boolean) => void;
  onUninstall: (modId: string) => void;
}

export function ModCard({ mod, viewMode, onToggle, onUninstall }: ModCardProps) {
  const [showMenu, setShowMenu] = useState(false);

  if (viewMode === "list") {
    return (
      <div className="flex items-center gap-4 rounded-lg border border-surface-800 bg-surface-900 p-4 transition-colors hover:border-surface-700">
        {/* Thumbnail placeholder */}
        <div className="flex h-12 w-12 flex-shrink-0 items-center justify-center rounded-lg bg-gradient-to-br from-surface-700 to-surface-800">
          <span className="text-lg font-bold text-surface-500">
            {mod.displayName.charAt(0).toUpperCase()}
          </span>
        </div>

        {/* Info */}
        <div className="min-w-0 flex-1">
          <h3 className="truncate font-medium text-surface-100">{mod.displayName}</h3>
          <p className="truncate text-sm text-surface-500">
            v{mod.version} • {mod.authors.join(", ") || "Unknown author"}
          </p>
        </div>

        {/* Toggle */}
        <Toggle enabled={mod.enabled} onChange={(enabled) => onToggle(mod.id, enabled)} />

        {/* Menu */}
        <div className="relative">
          <button
            type="button"
            onClick={() => setShowMenu(!showMenu)}
            className="rounded-lg p-2 text-surface-500 transition-colors hover:bg-surface-800 hover:text-surface-300"
          >
            <MoreVertical className="h-4 w-4" />
          </button>
          {showMenu && (
            <ContextMenu
              onClose={() => setShowMenu(false)}
              onUninstall={() => onUninstall(mod.id)}
            />
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="group relative overflow-hidden rounded-xl border border-surface-600 bg-night-500 transition-colors hover:border-surface-300">
      {/* Thumbnail */}
      <div className="flex aspect-video items-center justify-center bg-gradient-to-br from-night-600 to-night-700">
        <span className="text-4xl font-bold text-night-100">
          {mod.displayName.charAt(0).toUpperCase()}
        </span>
      </div>

      {/* Content */}
      <div className="p-4">
        <div className="mb-2 flex items-start justify-between gap-2">
          <h3 className="line-clamp-1 font-medium text-night-100">{mod.displayName}</h3>
          <div className="relative">
            <button
              type="button"
              onClick={() => setShowMenu(!showMenu)}
              className="rounded p-1 text-surface-500 transition-colors hover:bg-surface-800 hover:text-surface-300"
            >
              <MoreVertical className="h-4 w-4" />
            </button>
            {showMenu && (
              <ContextMenu
                onClose={() => setShowMenu(false)}
                onUninstall={() => onUninstall(mod.id)}
              />
            )}
          </div>
        </div>

        <p className="mb-3 text-sm text-surface-500">v{mod.version}</p>

        {mod.description && (
          <p className="mb-3 line-clamp-2 text-sm text-surface-400">{mod.description}</p>
        )}

        {/* Footer */}
        <div className="flex items-center justify-between">
          <span className="text-xs text-surface-500">
            {mod.authors.length > 0 ? mod.authors[0] : "Unknown"}
          </span>
          <Toggle enabled={mod.enabled} onChange={(enabled) => onToggle(mod.id, enabled)} />
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
        enabled ? "bg-league-500" : "bg-surface-700"
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

function ContextMenu({ onClose, onUninstall }: { onClose: () => void; onUninstall: () => void }) {
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
      <div className="absolute top-full right-0 z-20 mt-1 w-48 animate-fade-in rounded-lg border border-surface-700 bg-surface-800 py-1 shadow-xl">
        <button
          type="button"
          className="flex w-full items-center gap-2 px-3 py-2 text-sm text-surface-300 transition-colors hover:bg-surface-700"
        >
          <Info className="h-4 w-4" />
          View Details
        </button>
        <button
          type="button"
          className="flex w-full items-center gap-2 px-3 py-2 text-sm text-surface-300 transition-colors hover:bg-surface-700"
        >
          <FolderOpen className="h-4 w-4" />
          Open Location
        </button>
        <hr className="my-1 border-surface-700" />
        <button
          type="button"
          onClick={() => {
            onUninstall();
            onClose();
          }}
          className="flex w-full items-center gap-2 px-3 py-2 text-sm text-red-400 transition-colors hover:bg-surface-700"
        >
          <Trash2 className="h-4 w-4" />
          Uninstall
        </button>
      </div>
    </>
  );
}
