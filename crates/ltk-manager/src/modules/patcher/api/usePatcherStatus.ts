import { useQuery } from "@tanstack/react-query";

import { api, type AppError, type PatcherStatus } from "@/lib/tauri";
import { queryFn } from "@/utils/query";

import { patcherKeys } from "./keys";

export function usePatcherStatus() {
  return useQuery<PatcherStatus, AppError>({
    queryKey: patcherKeys.status(),
    queryFn: queryFn(api.getPatcherStatus),
    refetchInterval: 1000,
  });
}
