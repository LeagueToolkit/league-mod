import { useMutation, useQueryClient } from "@tanstack/react-query";
import { api, type InstalledMod, type AppError } from "@/lib/tauri";
import { unwrapForQuery } from "@/utils/query";
import { libraryKeys } from "./keys";

interface ToggleModVariables {
  modId: string;
  enabled: boolean;
}

/**
 * Hook to toggle a mod's enabled state.
 * Uses optimistic updates for instant UI feedback.
 */
export function useToggleMod() {
  const queryClient = useQueryClient();

  return useMutation<void, AppError, ToggleModVariables, { previous?: InstalledMod[] }>({
    mutationFn: async ({ modId, enabled }) => {
      const result = await api.toggleMod(modId, enabled);
      return unwrapForQuery(result);
    },
    onMutate: async ({ modId, enabled }) => {
      // Cancel outgoing refetches
      await queryClient.cancelQueries({ queryKey: libraryKeys.mods() });

      // Snapshot current value
      const previous = queryClient.getQueryData<InstalledMod[]>(libraryKeys.mods());

      // Optimistically update
      queryClient.setQueryData<InstalledMod[]>(libraryKeys.mods(), (old) =>
        old?.map((mod) => (mod.id === modId ? { ...mod, enabled } : mod))
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

