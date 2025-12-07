import { createFileRoute } from "@tanstack/react-router";

import { Settings } from "../pages/Settings";

interface SettingsSearch {
  firstRun?: boolean;
}

export const Route = createFileRoute("/settings")({
  validateSearch: (search: Record<string, unknown>): SettingsSearch => {
    return {
      firstRun: search.firstRun === true || search.firstRun === "true",
    };
  },
  component: Settings,
});
