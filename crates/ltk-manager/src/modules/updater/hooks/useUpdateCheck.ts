import { useEffect, useState, useCallback } from "react";

import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export interface UpdateState {
  /** Whether an update check is in progress */
  checking: boolean;
  /** Whether an update is currently being downloaded/installed */
  updating: boolean;
  /** Available update info, null if no update available */
  update: Update | null;
  /** Error message if check or update failed */
  error: string | null;
  /** Download progress (0-100) */
  progress: number;
}

export interface UseUpdateCheckReturn extends UpdateState {
  /** Manually trigger an update check */
  checkForUpdate: () => Promise<void>;
  /** Download and install the available update */
  downloadAndInstall: () => Promise<void>;
  /** Dismiss the update notification */
  dismiss: () => void;
}

/**
 * Hook to check for application updates on startup and provide update functionality.
 *
 * Uses Tauri's updater plugin to check GitHub releases for new versions.
 * Automatically checks on mount with a configurable delay.
 */
export function useUpdateCheck(
  options: { checkOnMount?: boolean; delayMs?: number } = {},
): UseUpdateCheckReturn {
  const { checkOnMount = true, delayMs = 3000 } = options;

  const [state, setState] = useState<UpdateState>({
    checking: false,
    updating: false,
    update: null,
    error: null,
    progress: 0,
  });

  const checkForUpdate = useCallback(async () => {
    setState((prev) => ({ ...prev, checking: true, error: null }));

    try {
      const update = await check();
      setState((prev) => ({
        ...prev,
        checking: false,
        update: update ?? null,
      }));
    } catch (err) {
      const message = err instanceof Error ? err.message : "Update check failed";
      console.error("Update check failed:", message);
      setState((prev) => ({
        ...prev,
        checking: false,
        error: message,
      }));
    }
  }, []);

  const downloadAndInstall = useCallback(async () => {
    if (!state.update) return;

    setState((prev) => ({ ...prev, updating: true, error: null, progress: 0 }));

    try {
      let downloaded = 0;
      let contentLength = 0;

      await state.update.downloadAndInstall((event) => {
        switch (event.event) {
          case "Started":
            contentLength = event.data.contentLength ?? 0;
            break;
          case "Progress":
            downloaded += event.data.chunkLength;
            if (contentLength > 0) {
              const progress = Math.round((downloaded / contentLength) * 100);
              setState((prev) => ({ ...prev, progress }));
            }
            break;
          case "Finished":
            setState((prev) => ({ ...prev, progress: 100 }));
            break;
        }
      });

      // Relaunch the application to apply the update
      await relaunch();
    } catch (err) {
      const message = err instanceof Error ? err.message : "Update failed";
      console.error("Update installation failed:", message);
      setState((prev) => ({
        ...prev,
        updating: false,
        error: message,
      }));
    }
  }, [state.update]);

  const dismiss = useCallback(() => {
    setState((prev) => ({ ...prev, update: null, error: null }));
  }, []);

  // Check for updates on mount with a delay to not block startup
  useEffect(() => {
    if (!checkOnMount) return;

    const timeoutId = setTimeout(() => {
      checkForUpdate();
    }, delayMs);

    return () => clearTimeout(timeoutId);
  }, [checkOnMount, delayMs, checkForUpdate]);

  return {
    ...state,
    checkForUpdate,
    downloadAndInstall,
    dismiss,
  };
}


