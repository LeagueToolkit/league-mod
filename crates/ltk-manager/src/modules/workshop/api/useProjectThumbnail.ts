import { queryOptions, skipToken, useQuery } from "@tanstack/react-query";
import { convertFileSrc } from "@tauri-apps/api/core";

import { api, type AppError } from "@/lib/tauri";
import { queryFn } from "@/utils/query";

import { workshopKeys } from "./keys";

/**
 * Query options for fetching a project thumbnail path.
 */
export function projectThumbnailOptions(projectPath: string, thumbnailPath: string | undefined) {
  return queryOptions<string, AppError>({
    queryKey: workshopKeys.thumbnail(projectPath, thumbnailPath),
    queryFn: thumbnailPath ? queryFn(() => api.getProjectThumbnail(thumbnailPath)) : skipToken,
    staleTime: Infinity,
  });
}

/**
 * Hook to fetch and cache project thumbnail path, returning a Tauri asset URL.
 */
export function useProjectThumbnail(projectPath: string, thumbnailPath?: string) {
  return useQuery({
    ...projectThumbnailOptions(projectPath, thumbnailPath),
    select: (path) => (path ? convertFileSrc(path) : ""),
  });
}
