import { Hammer, Library, Package, Settings } from "lucide-react";

type Page = "library" | "creator" | "settings";

interface SidebarProps {
  currentPage: Page;
  onNavigate: (page: Page) => void;
  appVersion?: string;
}

export function Sidebar({ currentPage, onNavigate, appVersion }: SidebarProps) {
  const navItems = [
    { id: "library" as const, label: "Library", icon: Library },
    { id: "creator" as const, label: "Creator", icon: Hammer },
  ];

  return (
    <aside className="bg-surface-900 border-surface-800 flex w-64 flex-col border-r">
      {/* Logo */}
      <div
        className="border-surface-800 flex h-16 items-center gap-3 border-b px-5"
        data-tauri-drag-region
      >
        <div className="from-league-500 to-league-600 flex h-9 w-9 items-center justify-center rounded-lg bg-gradient-to-br">
          <Package className="h-5 w-5 text-white" />
        </div>
        <div>
          <h1 className="text-surface-100 font-semibold">LTK Manager</h1>
          {appVersion && <span className="text-surface-500 text-xs">v{appVersion}</span>}
        </div>
      </div>

      {/* Navigation */}
      <nav className="flex-1 space-y-1 p-3">
        {navItems.map((item) => {
          const Icon = item.icon;
          const isActive = currentPage === item.id;

          return (
            <button
              type="button"
              key={item.id}
              onClick={() => onNavigate(item.id)}
              className={`flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-sm font-medium transition-colors ${
                isActive
                  ? "bg-league-500/10 text-league-400"
                  : "text-surface-400 hover:text-surface-200 hover:bg-surface-800"
              }`}
            >
              <Icon className="h-5 w-5" />
              {item.label}
            </button>
          );
        })}
      </nav>

      {/* Settings at bottom */}
      <div className="border-surface-800 border-t p-3">
        <button
          type="button"
          onClick={() => onNavigate("settings")}
          className={`flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-sm font-medium transition-colors ${
            currentPage === "settings"
              ? "bg-league-500/10 text-league-400"
              : "text-surface-400 hover:text-surface-200 hover:bg-surface-800"
          }`}
        >
          <Settings className="h-5 w-5" />
          Settings
        </button>
      </div>
    </aside>
  );
}
