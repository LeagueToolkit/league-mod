import { useEffect, useState } from "react";

import { createRootRoute, Outlet } from "@tanstack/react-router";
import { TanStackRouterDevtools } from "@tanstack/react-router-devtools";
import { invoke } from "@tauri-apps/api/core";

import { Sidebar } from "../components/Sidebar";
import { TitleBar } from "@/modules/shell";

interface AppInfo {
  name: string;
  version: string;
}

function RootLayout() {
  const [appInfo, setAppInfo] = useState<AppInfo | null>(null);

  useEffect(() => {
    invoke<AppInfo>("get_app_info").then(setAppInfo);
  }, []);

  return (
    <div className="flex h-screen flex-col bg-linear-to-br from-surface-900 via-night-600 to-surface-900">
      <TitleBar />
      <div className="flex flex-1 overflow-hidden">
        <Sidebar appVersion={appInfo?.version} />
        <main className="flex-1 overflow-hidden">
          <Outlet />
          <TanStackRouterDevtools />
        </main>
      </div>
    </div>
  );
}

export const Route = createRootRoute({
  component: RootLayout,
});
