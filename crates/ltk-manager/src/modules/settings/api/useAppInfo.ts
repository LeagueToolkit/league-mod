import { useQuery } from "@tanstack/react-query";

import { api, type AppError, type AppInfo } from "@/lib/tauri";
import { queryFn } from "@/utils/query";

import { settingsKeys } from "./keys";

export function useAppInfo() {
  return useQuery<AppInfo, AppError>({
    queryKey: settingsKeys.appInfo(),
    queryFn: queryFn(api.getAppInfo),
    staleTime: Infinity, // App info doesn't change during runtime
  });
}
