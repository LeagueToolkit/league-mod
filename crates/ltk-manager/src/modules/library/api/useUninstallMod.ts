import { useMutation, useQueryClient } from "@tanstack/react-query";

import { api, type AppError, type InstalledMod } from "@/lib/tauri";
import { unwrapForQuery } from "@/utils/query";
import { libraryKeys } from "./keys";

/**
 * Hook to uninstall a mod.
 * Uses optimistic updates for instant UI feedback.
 */
export function useUninstallMod() {
  const queryClient = useQueryClient();

  return useMutation<void, AppError, string, { previous?: InstalledMod[] }>({
    mutationFn: async (modId) => {
      const result = await api.uninstallMod(modId);
      return unwrapForQuery(result);
    },
    onMutate: async (modId) => {
      // Cancel outgoing refetches
      await queryClient.cancelQueries({ queryKey: libraryKeys.mods() });

      // Snapshot current value
      const previous = queryClient.getQueryData<InstalledMod[]>(libraryKeys.mods());

      // Optimistically remove
      queryClient.setQueryData<InstalledMod[]>(libraryKeys.mods(), (old) =>
        old?.filter((mod) => mod.id !== modId),
      );

      return { previous };
    },
    onError: (_error, _variables, context) => {
      // Rollback on error
      if (context?.previous) {
        queryClient.setQueryData(libraryKeys.mods(), context.previous);
      }
    },
    onSettled: () => {
      // Refetch to ensure consistency
      queryClient.invalidateQueries({ queryKey: libraryKeys.mods() });
    },
  });
}
