import { LuCircleAlert, LuDownload, LuRefreshCw, LuX } from "react-icons/lu";

import { Button, IconButton } from "@/components";

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
      <div className="absolute inset-x-0 top-0 z-50 mx-4 mt-2 flex items-center gap-3 rounded-lg border border-red-500/50 bg-red-900/80 px-4 py-3 backdrop-blur-sm">
        <LuCircleAlert className="h-5 w-5 shrink-0 text-red-400" />
        <div className="flex-1">
          <p className="text-sm font-medium text-red-100">Update Error</p>
          <p className="text-xs text-red-300">{error}</p>
        </div>
        <IconButton
          icon={<LuX className="h-4 w-4" />}
          variant="ghost"
          size="xs"
          onClick={dismiss}
          aria-label="Dismiss error"
          className="text-red-300 hover:bg-red-800/50 hover:text-red-100"
        />
      </div>
    );
  }

  // Downloading/Installing state
  if (updating) {
    return (
      <div className="absolute inset-x-0 top-0 z-50 mx-4 mt-2 rounded-lg border border-accent-500/30 bg-gradient-to-r from-accent-600/20 to-accent-700/20 px-4 py-3 backdrop-blur-sm">
        <div className="flex items-center gap-3">
          <LuRefreshCw className="h-5 w-5 shrink-0 animate-spin text-accent-400" />
          <div className="flex-1">
            <p className="text-sm font-medium text-accent-100">Installing update...</p>
            <p className="text-xs text-accent-300">
              {progress}% complete - App will restart automatically
            </p>
          </div>
        </div>
        <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-surface-700">
          <div
            className="h-full rounded-full bg-accent-500 transition-all duration-300"
            style={{ width: `${progress}%` }}
          />
        </div>
      </div>
    );
  }

  // Update available state
  if (update) {
    return (
      <div className="absolute inset-x-0 top-0 z-50 mx-4 mt-2 flex items-center gap-3 rounded-lg border border-accent-500/30 bg-gradient-to-r from-accent-600/20 to-accent-700/20 px-4 py-3 backdrop-blur-sm">
        <LuDownload className="h-5 w-5 shrink-0 text-accent-400" />
        <div className="flex-1">
          <p className="text-sm font-medium text-accent-100">Update Available: v{update.version}</p>
          <p className="text-xs text-accent-300">A new version is ready to install</p>
        </div>
        <div className="flex items-center gap-2">
          <Button
            variant="filled"
            size="sm"
            onClick={downloadAndInstall}
            className="bg-accent-600 hover:bg-accent-500"
          >
            Update Now
          </Button>
          <IconButton
            icon={<LuX className="h-4 w-4" />}
            variant="ghost"
            size="xs"
            onClick={dismiss}
            aria-label="Dismiss"
            className="text-accent-300 hover:bg-accent-600/30 hover:text-accent-100"
          />
        </div>
      </div>
    );
  }

  return null;
}
