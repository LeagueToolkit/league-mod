import { Link, useLocation } from "@tanstack/react-router";
import { LuHammer, LuLibrary, LuSettings } from "react-icons/lu";

export function Sidebar() {
  const location = useLocation();

  const navItems = [
    { to: "/", label: "Library", icon: LuLibrary },
    { to: "/creator", label: "Creator", icon: LuHammer },
  ];

  const isActive = (path: string) => {
    if (path === "/") {
      return location.pathname === "/";
    }
    return location.pathname.startsWith(path);
  };

  return (
    <aside className="flex w-44 flex-col border-r border-surface-600">
      {/* Navigation */}
      <nav className="flex-1 space-y-1 px-2 pt-2">
        {navItems.map((item) => {
          const Icon = item.icon;
          const active = isActive(item.to);

          return (
            <Link
              key={item.to}
              to={item.to}
              className={`flex w-full items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors ${
                active
                  ? "bg-brand-500/10 text-brand-400"
                  : "text-surface-400 hover:bg-surface-800 hover:text-surface-200"
              }`}
            >
              <Icon className="h-4 w-4" />
              {item.label}
            </Link>
          );
        })}
      </nav>

      {/* Settings at bottom */}
      <div className="border-t border-surface-700 px-2 py-2">
        <Link
          to="/settings"
          className={`flex w-full items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors ${
            isActive("/settings")
              ? "bg-brand-500/10 text-brand-400"
              : "text-surface-400 hover:bg-surface-800 hover:text-surface-200"
          }`}
        >
          <LuSettings className="h-4 w-4" />
          Settings
        </Link>
      </div>
    </aside>
  );
}
