import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "react";

import type { useInstallMod } from "./useInstallMod";

const VALID_EXTENSIONS = [".modpkg", ".fantome"];

function isValidModFile(path: string): boolean {
  const lowerPath = path.toLowerCase();
  return VALID_EXTENSIONS.some((ext) => lowerPath.endsWith(ext));
}

export function useModFileDrop(installMod: ReturnType<typeof useInstallMod>) {
  const [isDragOver, setIsDragOver] = useState(false);

  useEffect(() => {
    const currentWindow = getCurrentWindow();
    const unlisten = currentWindow.onDragDropEvent((event) => {
      const eventType = event.payload.type;
      if (eventType === "enter" || eventType === "over") {
        setIsDragOver(true);
      } else if (eventType === "drop") {
        setIsDragOver(false);
        const paths = event.payload.paths as string[];
        const validPaths = paths.filter(isValidModFile);

        for (const path of validPaths) {
          installMod.mutate(path, {
            onError: (error) => {
              console.error("Failed to install mod:", error.message);
            },
          });
        }
      } else if (eventType === "leave" || eventType === "cancel") {
        setIsDragOver(false);
      }
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [installMod]);

  return isDragOver;
}
