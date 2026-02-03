import { useMutation, useQueryClient } from "@tanstack/react-query";

import { api, type AppError, type PatcherConfig } from "@/lib/tauri";
import { unwrapForQuery } from "@/utils/query";

import { patcherKeys } from "./keys";

export function useStartPatcher() {
  const queryClient = useQueryClient();

  return useMutation<void, AppError, PatcherConfig>({
    mutationFn: async (config) => {
      const result = await api.startPatcher(config);
      return unwrapForQuery(result);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: patcherKeys.status() });
    },
  });
}
