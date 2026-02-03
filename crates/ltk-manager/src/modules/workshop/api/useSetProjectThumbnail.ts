import { useMutation, useQueryClient } from "@tanstack/react-query";

import { api, type AppError, type WorkshopProject } from "@/lib/tauri";
import { mutationFn } from "@/utils/query";

import { workshopKeys } from "./keys";

interface SetThumbnailArgs {
  projectPath: string;
  imagePath: string;
}

/**
 * Hook to set a project's thumbnail image.
 */
export function useSetProjectThumbnail() {
  const queryClient = useQueryClient();

  return useMutation<WorkshopProject, AppError, SetThumbnailArgs>({
    mutationFn: mutationFn(({ projectPath, imagePath }) =>
      api.setProjectThumbnail(projectPath, imagePath),
    ),
    onSuccess: (updatedProject) => {
      // Invalidate project list to refresh thumbnailPath
      queryClient.invalidateQueries({ queryKey: workshopKeys.projects() });
      // Invalidate specific project
      queryClient.invalidateQueries({ queryKey: workshopKeys.project(updatedProject.path) });
      // Invalidate thumbnail query
      queryClient.invalidateQueries({
        queryKey: workshopKeys.thumbnail(updatedProject.path, updatedProject.thumbnailPath),
      });
    },
  });
}
