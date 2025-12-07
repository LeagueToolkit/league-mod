import { useQuery } from "@tanstack/react-query";

import { api, type AppError } from "@/lib/tauri";
import { queryFn } from "@/utils/query";
import { settingsKeys } from "./keys";

/**
 * Hook to check if initial setup is required (league path not configured).
 */
export function useCheckSetupRequired() {
  return useQuery<boolean, AppError>({
    queryKey: settingsKeys.setupRequired(),
    queryFn: queryFn(api.checkSetupRequired),
  });
}
