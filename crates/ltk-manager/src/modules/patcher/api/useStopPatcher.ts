import { useMutation, useQueryClient } from "@tanstack/react-query";

import { api, type AppError } from "@/lib/tauri";
import { unwrapForQuery } from "@/utils/query";

import { patcherKeys } from "./keys";

export function useStopPatcher() {
  const queryClient = useQueryClient();

  return useMutation<void, AppError, void>({
    mutationFn: async () => {
      const result = await api.stopPatcher();
      return unwrapForQuery(result);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: patcherKeys.status() });
    },
  });
}


