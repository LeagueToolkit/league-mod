import { createRootRoute, Outlet, useLocation, useNavigate } from "@tanstack/react-router";
import { TanStackRouterDevtools } from "@tanstack/react-router-devtools";
import { useEffect, useRef } from "react";

import { useAppInfo, useCheckSetupRequired, useSettings } from "@/modules/settings";
import { TitleBar } from "@/modules/shell";
import { UpdateNotification, useUpdateCheck } from "@/modules/updater";
import { initializeTheme } from "@/stores";

import { Sidebar } from "../components/Sidebar";

function RootLayout() {
  const { data: appInfo } = useAppInfo();
  const { data: settings } = useSettings();
  const updateState = useUpdateCheck({ checkOnMount: true, delayMs: 3000 });
  const navigate = useNavigate();
  const location = useLocation();
  const themeCleanupRef = useRef<(() => void) | null>(null);

  const { data: setupRequired, isLoading: isCheckingSetup } = useCheckSetupRequired();

  // Initialize theme from settings
  useEffect(() => {
    if (settings?.theme) {
      themeCleanupRef.current = initializeTheme(settings.theme);
    }
    return () => {
      themeCleanupRef.current?.();
    };
  }, [settings?.theme]);

  // Redirect to settings if setup is required
  useEffect(() => {
    if (setupRequired && location.pathname !== "/settings") {
      navigate({ to: "/settings", search: { firstRun: true } });
    }
  }, [setupRequired, navigate, location.pathname]);

  // Show loading state while checking setup
  if (isCheckingSetup) {
    return (
      <div className="from-surface-900 via-night-600 to-surface-900 flex h-screen items-center justify-center bg-linear-to-br">
        <div className="text-surface-400">Loading...</div>
      </div>
    );
  }

  return (
    <div className="root flex h-screen flex-col bg-linear-to-br from-surface-900 via-night-600 to-surface-900">
      <TitleBar />
      <UpdateNotification updateState={updateState} />
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
