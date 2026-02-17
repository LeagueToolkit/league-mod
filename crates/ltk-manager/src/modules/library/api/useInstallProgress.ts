import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";

import type { InstallProgress } from "@/lib/tauri";

export function useInstallProgress() {
  const [progress, setProgress] = useState<InstallProgress | null>(null);

  useEffect(() => {
    let unlisten: UnlistenFn | null = null;

    listen<InstallProgress>("install-progress", (event) => {
      setProgress(event.payload);
    }).then((fn) => {
      unlisten = fn;
    });

    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, []);

  function reset() {
    setProgress(null);
  }

  return { progress, reset };
}
