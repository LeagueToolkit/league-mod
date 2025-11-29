import { useEffect, useState } from "react";

import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Square, X } from "lucide-react";

interface TitleBarProps {
  title?: string;
}

export function TitleBar({ title = "LTK Manager" }: TitleBarProps) {
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
      className="title-bar bg-brand-500/20 border-brand-600 flex h-9 shrink-0 select-none items-center justify-between border-b"
      data-tauri-drag-region
    >
      {/* Left: App title */}
      <div className="flex items-center gap-2 pl-4" data-tauri-drag-region>
        <span
          className="text-surface-100 text-xs font-medium tracking-wide"
          data-tauri-drag-region
        >
          {title}
        </span>
      </div>

      {/* Right: Window controls */}
      <div className="flex h-full">
        <button
          type="button"
          onClick={handleMinimize}
          className="hover:bg-surface-700 text-surface-400 hover:text-surface-200 flex h-full w-12 items-center justify-center transition-colors"
          aria-label="Minimize"
        >
          <Minus className="h-4 w-4" />
        </button>
        <button
          type="button"
          onClick={handleMaximize}
          className="hover:bg-surface-700 text-surface-400 hover:text-surface-200 flex h-full w-12 items-center justify-center transition-colors"
          aria-label={isMaximized ? "Restore" : "Maximize"}
        >
          {isMaximized ? (
            <OverlappingSquares className="h-3.5 w-3.5" />
          ) : (
            <Square className="h-3.5 w-3.5" />
          )}
        </button>
        <button
          type="button"
          onClick={handleClose}
          className="text-surface-400 flex h-full w-12 items-center justify-center transition-colors hover:bg-red-600 hover:text-white"
          aria-label="Close"
        >
          <X className="h-4 w-4" />
        </button>
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

