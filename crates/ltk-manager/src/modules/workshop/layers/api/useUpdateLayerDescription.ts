import { useMutation, useQueryClient } from "@tanstack/react-query";

import { api, type AppError, type WorkshopProject } from "@/lib/tauri";
import { unwrapForQuery } from "@/utils/query";

import { workshopKeys } from "../../api/keys";

interface UpdateLayerDescriptionVariables {
  projectPath: string;
  layerName: string;
  description?: string;
}

export function useUpdateLayerDescription() {
  const queryClient = useQueryClient();

  return useMutation<WorkshopProject, AppError, UpdateLayerDescriptionVariables>({
    mutationFn: async ({ projectPath, layerName, description }) => {
      const result = await api.updateLayerDescription(projectPath, layerName, description);
      return unwrapForQuery(result);
    },
    onSuccess: (updatedProject) => {
      queryClient.setQueryData<WorkshopProject[]>(workshopKeys.projects(), (old) =>
        old?.map((p) => (p.path === updatedProject.path ? updatedProject : p)),
      );
      queryClient.setQueryData(workshopKeys.project(updatedProject.path), updatedProject);
    },
  });
}
