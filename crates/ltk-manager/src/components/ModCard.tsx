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
      <div className="flex items-center gap-4 p-4 bg-surface-900 border border-surface-800 rounded-lg hover:border-surface-700 transition-colors">
        {/* Thumbnail placeholder */}
        <div className="w-12 h-12 rounded-lg bg-gradient-to-br from-surface-700 to-surface-800 flex items-center justify-center flex-shrink-0">
          <span className="text-lg font-bold text-surface-500">
            {mod.displayName.charAt(0).toUpperCase()}
          </span>
        </div>

        {/* Info */}
        <div className="flex-1 min-w-0">
          <h3 className="font-medium text-surface-100 truncate">{mod.displayName}</h3>
          <p className="text-sm text-surface-500 truncate">
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
            className="p-2 text-surface-500 hover:text-surface-300 hover:bg-surface-800 rounded-lg transition-colors"
          >
            <MoreVertical className="w-4 h-4" />
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
    <div className="group relative bg-surface-900 border border-surface-800 rounded-xl overflow-hidden hover:border-surface-700 transition-colors">
      {/* Thumbnail */}
      <div className="aspect-video bg-gradient-to-br from-surface-800 to-surface-900 flex items-center justify-center">
        <span className="text-4xl font-bold text-surface-700">
          {mod.displayName.charAt(0).toUpperCase()}
        </span>
      </div>

      {/* Content */}
      <div className="p-4">
        <div className="flex items-start justify-between gap-2 mb-2">
          <h3 className="font-medium text-surface-100 line-clamp-1">{mod.displayName}</h3>
          <div className="relative">
            <button
              type="button"
              onClick={() => setShowMenu(!showMenu)}
              className="p-1 text-surface-500 hover:text-surface-300 hover:bg-surface-800 rounded transition-colors"
            >
              <MoreVertical className="w-4 h-4" />
            </button>
            {showMenu && (
              <ContextMenu
                onClose={() => setShowMenu(false)}
                onUninstall={() => onUninstall(mod.id)}
              />
            )}
          </div>
        </div>

        <p className="text-sm text-surface-500 mb-3">v{mod.version}</p>

        {mod.description && (
          <p className="text-sm text-surface-400 line-clamp-2 mb-3">{mod.description}</p>
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
      className={`relative w-11 h-6 rounded-full transition-colors ${
        enabled ? "bg-league-500" : "bg-surface-700"
      }`}
    >
      <span
        className={`absolute top-1 left-1 w-4 h-4 bg-white rounded-full transition-transform ${
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
      <div className="absolute right-0 top-full mt-1 w-48 bg-surface-800 border border-surface-700 rounded-lg shadow-xl z-20 py-1 animate-fade-in">
        <button
          type="button"
          className="w-full flex items-center gap-2 px-3 py-2 text-sm text-surface-300 hover:bg-surface-700 transition-colors"
        >
          <Info className="w-4 h-4" />
          View Details
        </button>
        <button
          type="button"
          className="w-full flex items-center gap-2 px-3 py-2 text-sm text-surface-300 hover:bg-surface-700 transition-colors"
        >
          <FolderOpen className="w-4 h-4" />
          Open Location
        </button>
        <hr className="my-1 border-surface-700" />
        <button
          type="button"
          onClick={() => {
            onUninstall();
            onClose();
          }}
          className="w-full flex items-center gap-2 px-3 py-2 text-sm text-red-400 hover:bg-surface-700 transition-colors"
        >
          <Trash2 className="w-4 h-4" />
          Uninstall
        </button>
      </div>
    </>
  );
}
