import { useMutation, useQueryClient } from "@tanstack/react-query";

import { api, type AppError, type WorkshopProject } from "@/lib/tauri";
import { unwrapForQuery } from "@/utils/query";

import { workshopKeys } from "./keys";

/**
 * Hook to import a .modpkg file as a new workshop project.
 */
export function useImportFromModpkg() {
  const queryClient = useQueryClient();

  return useMutation<WorkshopProject, AppError, string>({
    mutationFn: async (filePath) => {
      const result = await api.importFromModpkg(filePath);
      return unwrapForQuery(result);
    },
    onSuccess: (newProject) => {
      // Add the new project to the cache
      queryClient.setQueryData<WorkshopProject[]>(workshopKeys.projects(), (old) =>
        old ? [newProject, ...old] : [newProject],
      );
    },
  });
}
