import { useMutation, useQueryClient } from "@tanstack/react-query";

import { api, type AppError, type CreateProjectArgs, type WorkshopProject } from "@/lib/tauri";
import { unwrapForQuery } from "@/utils/query";

import { workshopKeys } from "./keys";

/**
 * Hook to create a new workshop project.
 */
export function useCreateProject() {
  const queryClient = useQueryClient();

  return useMutation<WorkshopProject, AppError, CreateProjectArgs>({
    mutationFn: async (args) => {
      const result = await api.createWorkshopProject(args);
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
