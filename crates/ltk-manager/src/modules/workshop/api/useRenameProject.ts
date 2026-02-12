import { useMutation, useQueryClient } from "@tanstack/react-query";

import { api, type AppError, type WorkshopProject } from "@/lib/tauri";
import { unwrapForQuery } from "@/utils/query";

import { workshopKeys } from "./keys";

interface RenameProjectVariables {
  projectPath: string;
  newName: string;
}

/**
 * Hook to rename a workshop project (change its slug/directory name).
 */
export function useRenameProject() {
  const queryClient = useQueryClient();

  return useMutation<WorkshopProject, AppError, RenameProjectVariables>({
    mutationFn: async ({ projectPath, newName }) => {
      const result = await api.renameWorkshopProject(projectPath, newName);
      return unwrapForQuery(result);
    },
    onSuccess: () => {
      // Path changed, so invalidate all project queries
      queryClient.invalidateQueries({ queryKey: workshopKeys.projects() });
    },
  });
}
