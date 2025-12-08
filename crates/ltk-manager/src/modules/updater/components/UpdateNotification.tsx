import { AlertCircle, Download, RefreshCw, X } from "lucide-react";

import type { UseUpdateCheckReturn } from "../hooks/useUpdateCheck";

interface UpdateNotificationProps {
  updateState: UseUpdateCheckReturn;
}

/**
 * Notification banner that appears when an update is available.
 * Shows download progress during update installation.
 */
export function UpdateNotification({ updateState }: UpdateNotificationProps) {
  const { update, updating, progress, error, downloadAndInstall, dismiss } = updateState;

  // Don't render if no update available and not in error state
  if (!update && !error) return null;

  // Error state
  if (error) {
    return (
      <div className="mx-4 mt-2 flex items-center gap-3 rounded-lg border border-red-500/50 bg-red-900/80 px-4 py-3 backdrop-blur-sm">
        <AlertCircle className="h-5 w-5 shrink-0 text-red-400" />
        <div className="flex-1">
          <p className="text-sm font-medium text-red-100">Update Error</p>
          <p className="text-xs text-red-300">{error}</p>
        </div>
        <button
          onClick={dismiss}
          className="rounded p-1 text-red-300 transition-colors hover:text-red-100"
          aria-label="Dismiss error"
        >
          <X className="h-4 w-4" />
        </button>
      </div>
    );
  }

  // Downloading/Installing state
  if (updating) {
    return (
      <div className="from-accent-600/20 to-accent-700/20 border-accent-500/30 mx-4 mt-2 rounded-lg border bg-gradient-to-r px-4 py-3 backdrop-blur-sm">
        <div className="flex items-center gap-3">
          <RefreshCw className="text-accent-400 h-5 w-5 shrink-0 animate-spin" />
          <div className="flex-1">
            <p className="text-accent-100 text-sm font-medium">Installing update...</p>
            <p className="text-accent-300 text-xs">
              {progress}% complete - App will restart automatically
            </p>
          </div>
        </div>
        <div className="bg-surface-700 mt-2 h-1.5 overflow-hidden rounded-full">
          <div
            className="bg-accent-500 h-full rounded-full transition-all duration-300"
            style={{ width: `${progress}%` }}
          />
        </div>
      </div>
    );
  }

  // Update available state
  if (update) {
    return (
      <div className="from-accent-600/20 to-accent-700/20 border-accent-500/30 mx-4 mt-2 flex items-center gap-3 rounded-lg border bg-gradient-to-r px-4 py-3 backdrop-blur-sm">
        <Download className="text-accent-400 h-5 w-5 shrink-0" />
        <div className="flex-1">
          <p className="text-accent-100 text-sm font-medium">Update Available: v{update.version}</p>
          <p className="text-accent-300 text-xs">A new version is ready to install</p>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={downloadAndInstall}
            className="bg-accent-600 hover:bg-accent-500 rounded-md px-3 py-1.5 text-sm font-medium text-white transition-colors"
          >
            Update Now
          </button>
          <button
            onClick={dismiss}
            className="text-accent-300 hover:text-accent-100 rounded p-1 transition-colors"
            aria-label="Dismiss"
          >
            <X className="h-4 w-4" />
          </button>
        </div>
      </div>
    );
  }

  return null;
}
