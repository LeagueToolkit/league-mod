import { Link, useLocation } from "@tanstack/react-router";
import { Hammer, Library, Package, Settings } from "lucide-react";

interface SidebarProps {
  appVersion?: string;
}

export function Sidebar({ appVersion }: SidebarProps) {
  const location = useLocation();

  const navItems = [
    { to: "/", label: "Library", icon: Library },
    { to: "/creator", label: "Creator", icon: Hammer },
  ];

  const isActive = (path: string) => {
    if (path === "/") {
      return location.pathname === "/";
    }
    return location.pathname.startsWith(path);
  };

  return (
    <aside className="flex w-64 flex-col border-r border-surface-600">
      {/* Logo */}
      <div
        className="flex h-16 items-center gap-3 border-b border-surface-600 px-5"
        data-tauri-drag-region
      >
        <div className="from-league-500 to-league-600 flex h-9 w-9 items-center justify-center rounded-lg bg-linear-to-br">
          <Package className="h-5 w-5 text-white" />
        </div>
        <div>
          <h1 className="font-semibold text-surface-100">LTK Manager</h1>
          {appVersion && <span className="text-xs text-surface-500">v{appVersion}</span>}
        </div>
      </div>

      {/* Navigation */}
      <nav className="flex-1 space-y-1 p-3">
        {navItems.map((item) => {
          const Icon = item.icon;
          const active = isActive(item.to);

          return (
            <Link
              key={item.to}
              to={item.to}
              className={`flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-sm font-medium transition-colors ${
                active
                  ? "bg-league-500/10 text-league-400"
                  : "text-surface-400 hover:bg-surface-800 hover:text-surface-200"
              }`}
            >
              <Icon className="h-5 w-5" />
              {item.label}
            </Link>
          );
        })}
      </nav>

      {/* Settings at bottom */}
      <div className="border-t border-surface-800 p-3">
        <Link
          to="/settings"
          className={`flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-sm font-medium transition-colors ${
            isActive("/settings")
              ? "bg-league-500/10 text-league-400"
              : "text-surface-400 hover:bg-surface-800 hover:text-surface-200"
          }`}
        >
          <Settings className="h-5 w-5" />
          Settings
        </Link>
      </div>
    </aside>
  );
}
