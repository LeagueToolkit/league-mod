import { queryOptions, skipToken, useQuery } from "@tanstack/react-query";
import { convertFileSrc } from "@tauri-apps/api/core";

import { api, type AppError } from "@/lib/tauri";
import { unwrapForQuery } from "@/utils/query";

import { workshopKeys } from "./keys";

/**
 * Query options for fetching a project thumbnail as a Tauri asset URL.
 * The queryFn returns the final asset URL (not a raw path) so that
 * setQueryData can inject cache-busted URLs directly.
 */
export function projectThumbnailOptions(projectPath: string, thumbnailPath: string | undefined) {
  return queryOptions<string, AppError>({
    queryKey: workshopKeys.thumbnail(projectPath, thumbnailPath),
    queryFn: thumbnailPath
      ? async () => {
          const result = await api.getProjectThumbnail(thumbnailPath);
          const path = unwrapForQuery(result);
          return path ? convertFileSrc(path) : "";
        }
      : skipToken,
    staleTime: Infinity,
  });
}

/**
 * Hook to fetch and cache a project thumbnail as a Tauri asset URL.
 */
export function useProjectThumbnail(projectPath: string, thumbnailPath?: string) {
  return useQuery(projectThumbnailOptions(projectPath, thumbnailPath));
}
