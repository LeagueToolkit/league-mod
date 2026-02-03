import { Link } from "@tanstack/react-router";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "react";
import type { IconType } from "react-icons";
import { LuHammer, LuLibrary, LuMinus, LuSettings, LuSquare, LuX } from "react-icons/lu";
import { twMerge } from "tailwind-merge";

import { IconButton } from "@/components/Button";

const navItems = [
  { to: "/", label: "Library", icon: LuLibrary, exact: true },
  { to: "/creator", label: "Workshop", icon: LuHammer, exact: false },
] as const;

const linkBaseClass =
  "relative flex h-full items-center gap-1.5 px-3 text-sm font-medium transition-colors";
const settingsLinkBase = "relative flex h-full items-center px-3 transition-colors";
const activeLinkClass = "text-brand-400";
const inactiveLinkClass = "text-surface-400 hover:text-surface-200";

function ActiveIndicator() {
  return <span className="absolute right-0 bottom-0 left-0 h-0.5 bg-brand-500" />;
}

function NavLink({
  to,
  label,
  icon: Icon,
  exact,
}: {
  to: string;
  label: string;
  icon: IconType;
  exact: boolean;
}) {
  return (
    <Link
      to={to}
      activeOptions={{ exact }}
      activeProps={{ className: twMerge(linkBaseClass, activeLinkClass) }}
      inactiveProps={{ className: twMerge(linkBaseClass, inactiveLinkClass) }}
    >
      {({ isActive }) => (
        <>
          <Icon className="h-4 w-4" />
          {label}
          {isActive && <ActiveIndicator />}
        </>
      )}
    </Link>
  );
}

interface TitleBarProps {
  title?: string;
  version?: string;
}

export function TitleBar({ title = "LTK Manager", version }: TitleBarProps) {
  const [isMaximized, setIsMaximized] = useState(false);
  const appWindow = getCurrentWindow();

  useEffect(() => {
    // Check initial maximized state
    appWindow.isMaximized().then(setIsMaximized);

    // Listen for resize events to update maximized state
    const unlisten = appWindow.onResized(() => {
      appWindow.isMaximized().then(setIsMaximized);
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [appWindow]);

  const handleMinimize = () => appWindow.minimize();
  const handleMaximize = () => appWindow.toggleMaximize();
  const handleClose = () => appWindow.close();

  return (
    <header
      className="title-bar flex h-10 shrink-0 items-center justify-between border-b border-surface-600 select-none"
      data-tauri-drag-region
    >
      {/* Left: App icon, title, version, and navigation */}
      <div className="flex h-full items-center" data-tauri-drag-region>
        <div className="flex items-center gap-2 pr-4 pl-3" data-tauri-drag-region>
          <img src="/icon.svg" alt="LTK" className="h-5 w-5" data-tauri-drag-region />
          <span className="text-sm font-medium text-surface-100" data-tauri-drag-region>
            {title}
          </span>
          {version && (
            <span className="text-xs text-surface-500" data-tauri-drag-region>
              v{version}
            </span>
          )}
        </div>

        {/* Navigation tabs */}
        <nav className="flex h-full items-center gap-1">
          {navItems.map((item) => (
            <NavLink key={item.to} {...item} />
          ))}
        </nav>
      </div>

      {/* Right: Settings and window controls */}
      <div className="flex h-full items-center">
        {/* Settings button */}
        <Link
          to="/settings"
          activeProps={{
            className: twMerge(settingsLinkBase, activeLinkClass),
          }}
          inactiveProps={{
            className: twMerge(settingsLinkBase, inactiveLinkClass),
          }}
          aria-label="Settings"
        >
          {({ isActive }) => (
            <>
              <LuSettings className="h-4 w-4" />
              {isActive && <ActiveIndicator />}
            </>
          )}
        </Link>

        {/* Separator */}
        <div className="mx-2 h-5 w-px bg-surface-600" />

        {/* Window controls */}
        <IconButton
          icon={<LuMinus className="h-4 w-4" />}
          variant="ghost"
          size="md"
          onClick={handleMinimize}
          aria-label="Minimize"
          className="h-full w-12 rounded-none text-surface-400 hover:bg-surface-700 hover:text-surface-200"
        />
        <IconButton
          icon={
            isMaximized ? (
              <OverlappingSquares className="h-3.5 w-3.5" />
            ) : (
              <LuSquare className="h-3.5 w-3.5" />
            )
          }
          variant="ghost"
          size="md"
          onClick={handleMaximize}
          aria-label={isMaximized ? "Restore" : "Maximize"}
          className="h-full w-12 rounded-none text-surface-400 hover:bg-surface-700 hover:text-surface-200"
        />
        <IconButton
          icon={<LuX className="h-4 w-4" />}
          variant="ghost"
          size="md"
          onClick={handleClose}
          aria-label="Close"
          className="h-full w-12 rounded-none text-surface-400 hover:bg-red-600 hover:text-white"
        />
      </div>
    </header>
  );
}

// Custom icon for restored/unmaximized state (overlapping squares)
function OverlappingSquares({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      viewBox="0 0 14 14"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
    >
      {/* Back square */}
      <rect x="4" y="1" width="9" height="9" rx="1" />
      {/* Front square */}
      <rect x="1" y="4" width="9" height="9" rx="1" fill="currentColor" fillOpacity="0.1" />
      <rect x="1" y="4" width="9" height="9" rx="1" />
    </svg>
  );
}
