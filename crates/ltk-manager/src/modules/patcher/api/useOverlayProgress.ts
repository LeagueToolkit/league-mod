import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";

import type { OverlayProgress } from "@/lib/tauri";

import { usePatcherStatus } from "./usePatcherStatus";

export function useOverlayProgress() {
  const [progress, setProgress] = useState<OverlayProgress | null>(null);
  const { data: patcherStatus } = usePatcherStatus();
  const wasPatcherRunning = useRef(false);

  useEffect(() => {
    let unlisten: UnlistenFn | null = null;

    listen<OverlayProgress>("overlay-progress", (event) => {
      setProgress(event.payload);

      // Clear progress after completion
      if (event.payload.stage === "complete") {
        setTimeout(() => setProgress(null), 1000);
      }
    }).then((fn) => {
      unlisten = fn;
    });

    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, []);

  // Reset progress when patcher stops
  useEffect(() => {
    const isPatcherRunning = patcherStatus?.running ?? false;

    // If patcher was running and now stopped, reset progress
    if (wasPatcherRunning.current && !isPatcherRunning) {
      setProgress(null);
    }

    wasPatcherRunning.current = isPatcherRunning;
  }, [patcherStatus?.running]);

  return progress;
}
