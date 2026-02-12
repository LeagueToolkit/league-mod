import { createFileRoute, Outlet } from "@tanstack/react-router";

import { useSettings } from "@/modules/settings";
import { NotConfiguredState } from "@/modules/workshop";

export const Route = createFileRoute("/workshop")({
  component: WorkshopLayout,
});

function WorkshopLayout() {
  const { data: settings } = useSettings();
  const workshopConfigured = !!settings?.workshopPath;

  if (!workshopConfigured) {
    return <NotConfiguredState />;
  }

  return <Outlet />;
}
