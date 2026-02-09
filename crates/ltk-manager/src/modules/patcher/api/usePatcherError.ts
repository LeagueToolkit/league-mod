import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useEffect } from "react";

import { useToast } from "@/components";
import type { AppError } from "@/lib/tauri";

export function usePatcherError() {
  const toast = useToast();

  useEffect(() => {
    let unlisten: UnlistenFn | null = null;

    listen<AppError>("patcher-error", (event) => {
      toast.error("Patcher Error", event.payload.message);
    }).then((fn) => {
      unlisten = fn;
    });

    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, [toast]);
}
