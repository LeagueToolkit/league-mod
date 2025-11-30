import { useState } from "react";

import { FolderOpen, Info, MoreVertical, Trash2 } from "lucide-react";

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
      <div className="bg-surface-900 border-surface-800 hover:border-surface-700 flex items-center gap-4 rounded-lg border p-4 transition-colors">
        {/* Thumbnail placeholder */}
        <div className="from-surface-700 to-surface-800 flex h-12 w-12 flex-shrink-0 items-center justify-center rounded-lg bg-gradient-to-br">
          <span className="text-surface-500 text-lg font-bold">
            {mod.displayName.charAt(0).toUpperCase()}
          </span>
        </div>

        {/* Info */}
        <div className="min-w-0 flex-1">
          <h3 className="text-surface-100 truncate font-medium">{mod.displayName}</h3>
          <p className="text-surface-500 truncate text-sm">
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
            className="text-surface-500 hover:text-surface-300 hover:bg-surface-800 rounded-lg p-2 transition-colors"
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
    <div className="group bg-night-500 border-surface-600 hover:border-surface-300 relative overflow-hidden rounded-xl border transition-colors">
      {/* Thumbnail */}
      <div className="from-night-600 to-night-700 flex aspect-video items-center justify-center bg-gradient-to-br">
        <span className="text-night-100 text-4xl font-bold">
          {mod.displayName.charAt(0).toUpperCase()}
        </span>
      </div>

      {/* Content */}
      <div className="p-4">
        <div className="mb-2 flex items-start justify-between gap-2">
          <h3 className="text-night-100 line-clamp-1 font-medium">{mod.displayName}</h3>
          <div className="relative">
            <button
              type="button"
              onClick={() => setShowMenu(!showMenu)}
              className="text-surface-500 hover:text-surface-300 hover:bg-surface-800 rounded p-1 transition-colors"
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

        <p className="text-surface-500 mb-3 text-sm">v{mod.version}</p>

        {mod.description && (
          <p className="text-surface-400 mb-3 line-clamp-2 text-sm">{mod.description}</p>
        )}

        {/* Footer */}
        <div className="flex items-center justify-between">
          <span className="text-surface-500 text-xs">
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
      <div className="bg-surface-800 border-surface-700 animate-fade-in absolute top-full right-0 z-20 mt-1 w-48 rounded-lg border py-1 shadow-xl">
        <button
          type="button"
          className="text-surface-300 hover:bg-surface-700 flex w-full items-center gap-2 px-3 py-2 text-sm transition-colors"
        >
          <Info className="h-4 w-4" />
          View Details
        </button>
        <button
          type="button"
          className="text-surface-300 hover:bg-surface-700 flex w-full items-center gap-2 px-3 py-2 text-sm transition-colors"
        >
          <FolderOpen className="h-4 w-4" />
          Open Location
        </button>
        <hr className="border-surface-700 my-1" />
        <button
          type="button"
          onClick={() => {
            onUninstall();
            onClose();
          }}
          className="hover:bg-surface-700 flex w-full items-center gap-2 px-3 py-2 text-sm text-red-400 transition-colors"
        >
          <Trash2 className="h-4 w-4" />
          Uninstall
        </button>
      </div>
    </>
  );
}
