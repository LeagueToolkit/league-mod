import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useEffect } from "react";

import { useDevConsoleStore } from "@/stores/devConsole";

interface LogEventPayload {
  timestamp: string;
  level: string;
  target: string;
  message: string;
}

export function useDevLogStream() {
  const addEntry = useDevConsoleStore((s) => s.addEntry);

  useEffect(() => {
    if (!import.meta.env.DEV) return;

    let unlisten: UnlistenFn | null = null;
    let canceled = false;

    listen<LogEventPayload>("log-event", (event) => {
      addEntry(event.payload);
    }).then((fn) => {
      if (canceled) {
        fn();
      } else {
        unlisten = fn;
      }
    });

    return () => {
      canceled = true;
      if (unlisten) {
        unlisten();
      }
    };
  }, [addEntry]);
}
