import { Library, Settings, Hammer, Package } from 'lucide-react';

type Page = 'library' | 'creator' | 'settings';

interface SidebarProps {
  currentPage: Page;
  onNavigate: (page: Page) => void;
  appVersion?: string;
}

export function Sidebar({ currentPage, onNavigate, appVersion }: SidebarProps) {
  const navItems = [
    { id: 'library' as const, label: 'Library', icon: Library },
    { id: 'creator' as const, label: 'Creator', icon: Hammer },
  ];

  return (
    <aside className="w-64 bg-surface-900 border-r border-surface-800 flex flex-col">
      {/* Logo */}
      <div
        className="h-16 flex items-center gap-3 px-5 border-b border-surface-800"
        data-tauri-drag-region
      >
        <div className="w-9 h-9 rounded-lg bg-gradient-to-br from-league-500 to-league-600 flex items-center justify-center">
          <Package className="w-5 h-5 text-white" />
        </div>
        <div>
          <h1 className="font-semibold text-surface-100">LTK Manager</h1>
          {appVersion && (
            <span className="text-xs text-surface-500">v{appVersion}</span>
          )}
        </div>
      </div>

      {/* Navigation */}
      <nav className="flex-1 p-3 space-y-1">
        {navItems.map((item) => {
          const Icon = item.icon;
          const isActive = currentPage === item.id;

          return (
            <button
              key={item.id}
              onClick={() => onNavigate(item.id)}
              className={`w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-colors ${
                isActive
                  ? 'bg-league-500/10 text-league-400'
                  : 'text-surface-400 hover:text-surface-200 hover:bg-surface-800'
              }`}
            >
              <Icon className="w-5 h-5" />
              {item.label}
            </button>
          );
        })}
      </nav>

      {/* Settings at bottom */}
      <div className="p-3 border-t border-surface-800">
        <button
          onClick={() => onNavigate('settings')}
          className={`w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-colors ${
            currentPage === 'settings'
              ? 'bg-league-500/10 text-league-400'
              : 'text-surface-400 hover:text-surface-200 hover:bg-surface-800'
          }`}
        >
          <Settings className="w-5 h-5" />
          Settings
        </button>
      </div>
    </aside>
  );
}

