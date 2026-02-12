import { useMutation, useQueryClient } from "@tanstack/react-query";

import { api, type AppError, type WorkshopProject } from "@/lib/tauri";
import { unwrapForQuery } from "@/utils/query";

import { workshopKeys } from "../../api/keys";

interface DeleteLayerVariables {
  projectPath: string;
  layerName: string;
}

export function useDeleteLayer() {
  const queryClient = useQueryClient();

  return useMutation<WorkshopProject, AppError, DeleteLayerVariables>({
    mutationFn: async ({ projectPath, layerName }) => {
      const result = await api.deleteProjectLayer(projectPath, layerName);
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
