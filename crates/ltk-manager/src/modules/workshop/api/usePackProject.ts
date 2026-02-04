import { useMutation } from "@tanstack/react-query";

import { api, type AppError, type PackProjectArgs, type PackResult } from "@/lib/tauri";
import { unwrapForQuery } from "@/utils/query";

/**
 * Hook to pack a workshop project to .modpkg or .fantome format.
 */
export function usePackProject() {
  return useMutation<PackResult, AppError, PackProjectArgs>({
    mutationFn: async (args) => {
      const result = await api.packWorkshopProject(args);
      return unwrapForQuery(result);
    },
  });
}
