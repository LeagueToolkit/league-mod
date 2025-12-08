import { useEffect } from "react";

import { createRootRoute, Outlet, useLocation, useNavigate } from "@tanstack/react-router";
import { TanStackRouterDevtools } from "@tanstack/react-router-devtools";

import { useAppInfo, useCheckSetupRequired } from "@/modules/settings";
import { TitleBar } from "@/modules/shell";
import { UpdateNotification, useUpdateCheck } from "@/modules/updater";
import { Sidebar } from "../components/Sidebar";

function RootLayout() {
  const { data: appInfo } = useAppInfo();
  const updateState = useUpdateCheck({ checkOnMount: true, delayMs: 3000 });
  const navigate = useNavigate();
  const location = useLocation();

  const { data: setupRequired, isLoading: isCheckingSetup } = useCheckSetupRequired();

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
    <div className="from-surface-900 via-night-600 to-surface-900 flex h-screen flex-col bg-linear-to-br">
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
