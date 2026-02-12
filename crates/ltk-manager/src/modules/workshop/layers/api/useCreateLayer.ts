import { useMutation, useQueryClient } from "@tanstack/react-query";

import { api, type AppError, type WorkshopProject } from "@/lib/tauri";
import { unwrapForQuery } from "@/utils/query";

import { workshopKeys } from "../../api/keys";

interface CreateLayerVariables {
  projectPath: string;
  name: string;
  description?: string;
}

export function useCreateLayer() {
  const queryClient = useQueryClient();

  return useMutation<WorkshopProject, AppError, CreateLayerVariables>({
    mutationFn: async ({ projectPath, name, description }) => {
      const result = await api.createProjectLayer(projectPath, name, description);
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
