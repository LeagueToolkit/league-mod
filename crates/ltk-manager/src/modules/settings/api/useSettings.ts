import { useQuery } from "@tanstack/react-query";

import { api, type AppError, type Settings } from "@/lib/tauri";
import { queryFn } from "@/utils/query";
import { settingsKeys } from "./keys";

/**
 * Hook to fetch current app settings.
 */
export function useSettings() {
  return useQuery<Settings, AppError>({
    queryKey: settingsKeys.settings(),
    queryFn: queryFn(api.getSettings),
  });
}
