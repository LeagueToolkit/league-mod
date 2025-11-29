import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { Grid3X3, List, Plus, Search, Upload } from "lucide-react";
import { useEffect, useState } from "react";
import { ModCard } from "../components/ModCard";

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
  layers: ModLayer[];
}

interface ModLayer {
  name: string;
  priority: number;
  enabled: boolean;
}

export function Library() {
  const [mods, setMods] = useState<InstalledMod[]>([]);
  const [searchQuery, setSearchQuery] = useState("");
  const [viewMode, setViewMode] = useState<"grid" | "list">("grid");
  const [isLoading, setIsLoading] = useState(true);

  useEffect(() => {
    loadMods();
  }, []);

  async function loadMods() {
    try {
      const installedMods = await invoke<InstalledMod[]>("get_installed_mods");
      setMods(installedMods);
    } catch (error) {
      console.error("Failed to load mods:", error);
    } finally {
      setIsLoading(false);
    }
  }

  async function handleInstallMod() {
    try {
      const file = await open({
        multiple: false,
        filters: [{ name: "Mod Package", extensions: ["modpkg"] }],
      });

      if (file) {
        const newMod = await invoke<InstalledMod>("install_mod", {
          filePath: file,
        });
        setMods((prev) => [...prev, newMod]);
      }
    } catch (error) {
      console.error("Failed to install mod:", error);
    }
  }

  async function handleToggleMod(modId: string, enabled: boolean) {
    try {
      await invoke("toggle_mod", { modId, enabled });
      setMods((prev) => prev.map((mod) => (mod.id === modId ? { ...mod, enabled } : mod)));
    } catch (error) {
      console.error("Failed to toggle mod:", error);
    }
  }

  async function handleUninstallMod(modId: string) {
    try {
      await invoke("uninstall_mod", { modId });
      setMods((prev) => prev.filter((mod) => mod.id !== modId));
    } catch (error) {
      console.error("Failed to uninstall mod:", error);
    }
  }

  const filteredMods = mods.filter(
    (mod) =>
      mod.displayName.toLowerCase().includes(searchQuery.toLowerCase()) ||
      mod.name.toLowerCase().includes(searchQuery.toLowerCase()),
  );

  return (
    <div className="h-full flex flex-col">
      {/* Header */}
      <header className="h-16 flex items-center justify-between px-6 border-b border-surface-800">
        <h2 className="text-xl font-semibold text-surface-100">Mod Library</h2>
        <button
          type="button"
          onClick={handleInstallMod}
          className="flex items-center gap-2 px-4 py-2 bg-league-500 hover:bg-league-600 text-white rounded-lg font-medium transition-colors"
        >
          <Plus className="w-4 h-4" />
          Add Mod
        </button>
      </header>

      {/* Toolbar */}
      <div className="flex items-center gap-4 px-6 py-4 border-b border-surface-800/50">
        {/* Search */}
        <div className="relative flex-1 max-w-md">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-surface-500" />
          <input
            type="text"
            placeholder="Search mods..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="w-full pl-10 pr-4 py-2 bg-surface-800 border border-surface-700 rounded-lg text-surface-100 placeholder:text-surface-500 focus:outline-none focus:ring-2 focus:ring-league-500 focus:border-transparent"
          />
        </div>

        {/* View toggle */}
        <div className="flex items-center gap-1 p-1 bg-surface-800 rounded-lg">
          <button
            type="button"
            onClick={() => setViewMode("grid")}
            className={`p-2 rounded-md transition-colors ${
              viewMode === "grid"
                ? "bg-surface-700 text-surface-100"
                : "text-surface-500 hover:text-surface-300"
            }`}
          >
            <Grid3X3 className="w-4 h-4" />
          </button>
          <button
            type="button"
            onClick={() => setViewMode("list")}
            className={`p-2 rounded-md transition-colors ${
              viewMode === "list"
                ? "bg-surface-700 text-surface-100"
                : "text-surface-500 hover:text-surface-300"
            }`}
          >
            <List className="w-4 h-4" />
          </button>
        </div>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-auto p-6">
        {isLoading ? (
          <div className="flex items-center justify-center h-64">
            <div className="animate-spin w-8 h-8 border-2 border-league-500 border-t-transparent rounded-full" />
          </div>
        ) : filteredMods.length === 0 ? (
          <EmptyState onInstall={handleInstallMod} hasSearch={!!searchQuery} />
        ) : (
          <div
            className={
              viewMode === "grid"
                ? "grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4"
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

function EmptyState({ onInstall, hasSearch }: { onInstall: () => void; hasSearch: boolean }) {
  if (hasSearch) {
    return (
      <div className="flex flex-col items-center justify-center h-64 text-center">
        <Search className="w-12 h-12 text-surface-600 mb-4" />
        <h3 className="text-lg font-medium text-surface-300 mb-1">No mods found</h3>
        <p className="text-surface-500">Try adjusting your search query</p>
      </div>
    );
  }

  return (
    <div className="flex flex-col items-center justify-center h-64 text-center">
      <div className="w-20 h-20 rounded-2xl bg-surface-800 flex items-center justify-center mb-4">
        <Upload className="w-10 h-10 text-surface-600" />
      </div>
      <h3 className="text-lg font-medium text-surface-300 mb-1">No mods installed</h3>
      <p className="text-surface-500 mb-4">Get started by adding your first mod</p>
      <button
        type="button"
        onClick={onInstall}
        className="flex items-center gap-2 px-4 py-2 bg-league-500 hover:bg-league-600 text-white rounded-lg font-medium transition-colors"
      >
        <Plus className="w-4 h-4" />
        Add Mod
      </button>
    </div>
  );
}
