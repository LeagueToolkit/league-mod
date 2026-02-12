import { useMutation, useQueryClient } from "@tanstack/react-query";

import { api, type AppError, type WorkshopProject } from "@/lib/tauri";
import { unwrapForQuery } from "@/utils/query";

import { workshopKeys } from "./keys";

interface SaveStringOverridesVariables {
  projectPath: string;
  layerName: string;
  stringOverrides: Record<string, Record<string, string>>;
}

/**
 * Hook to save string overrides for a specific layer.
 */
export function useSaveStringOverrides() {
  const queryClient = useQueryClient();

  return useMutation<WorkshopProject, AppError, SaveStringOverridesVariables>({
    mutationFn: async ({ projectPath, layerName, stringOverrides }) => {
      const result = await api.saveLayerStringOverrides(projectPath, layerName, stringOverrides);
      return unwrapForQuery(result);
    },
    onSuccess: (updatedProject) => {
      // Update the project in the projects list
      queryClient.setQueryData<WorkshopProject[]>(workshopKeys.projects(), (old) =>
        old?.map((p) => (p.path === updatedProject.path ? updatedProject : p)),
      );
      // Update the individual project cache
      queryClient.setQueryData(workshopKeys.project(updatedProject.path), updatedProject);
    },
  });
}
