import { useMutation, useQueryClient } from "@tanstack/react-query";

import { api, type AppError, type InstalledMod } from "@/lib/tauri";
import { unwrapForQuery } from "@/utils/query";

import { libraryKeys } from "./keys";

/**
 * Hook to reorder enabled mods in the active profile.
 * Uses optimistic updates for instant UI feedback.
 */
export function useReorderMods() {
  const queryClient = useQueryClient();

  return useMutation<void, AppError, string[], { previous?: InstalledMod[] }>({
    mutationFn: async (modIds) => {
      const result = await api.reorderMods(modIds);
      return unwrapForQuery(result);
    },
    onMutate: async (modIds) => {
      await queryClient.cancelQueries({ queryKey: libraryKeys.mods() });

      const previous = queryClient.getQueryData<InstalledMod[]>(libraryKeys.mods());

      queryClient.setQueryData<InstalledMod[]>(libraryKeys.mods(), (old) => {
        if (!old) return old;

        const modMap = new Map(old.map((m) => [m.id, m]));
        const reordered: InstalledMod[] = [];

        // Enabled mods in new order
        for (const id of modIds) {
          const mod = modMap.get(id);
          if (mod) reordered.push(mod);
        }

        // Disabled mods after
        for (const mod of old) {
          if (!mod.enabled) reordered.push(mod);
        }

        return reordered;
      });

      return { previous };
    },
    onError: (_error, _variables, context) => {
      if (context?.previous) {
        queryClient.setQueryData(libraryKeys.mods(), context.previous);
      }
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: libraryKeys.mods() });
    },
  });
}
