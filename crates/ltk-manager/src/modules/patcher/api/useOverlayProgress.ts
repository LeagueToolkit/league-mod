import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";

import type { OverlayProgress } from "@/lib/tauri";

export function useOverlayProgress() {
  const [progress, setProgress] = useState<OverlayProgress | null>(null);

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

  return progress;
}
